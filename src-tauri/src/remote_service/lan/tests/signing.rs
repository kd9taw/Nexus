//! Signing in on the computer (`crate::lan_client::account`) with a station on the network: the
//! whole sign-in against the stand-in service and issuer (`lan_client::account::tests::Cloud`) and a
//! stand-in station that takes an enrolment as the real channel does up to its proof check (TLS with
//! its own LAN key and any computer's, then the first message), and answers as the real one does a
//! proof that holds. The real channel reading the computer's enrolment is in `enrol`; the proof
//! check itself is the station's (`lan::enrol`) and tempo-client's.
use super::*;
use crate::lan_client::account::tests::{
    answering, device_at, env, finding, no_pick, nonce_for, row, shown, signing_in, stations,
    trusting, unsigned_token, Cloud, Scratch, LAN_STATION, SERVICE_STATION,
};
use crate::lan_client::account::Signed;
use crate::lan_client::tests::{where_nothing_listens, StationStore};
use crate::lan_client::ComputerKey;
use crate::remote_service::vault::{InternetRoad, PairedStation, StationVault};
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, Ordering};
use tempo_stream::lan::dnssd::Found;

/// A stand-in station on loopback: where it listens, its LAN key (SPKI, lowercase hex), and each
/// enrolment it heard with the key the computer's TLS session proved.
struct Standing {
    at: SocketAddrV4,
    key: String,
    heard: Arc<Mutex<Vec<(Value, String)>>>,
}

/// How a stand-in station answers an enrolment.
#[derive(Clone, Copy)]
enum Answer {
    /// As the real station takes one: `enrolled`, with the device id worked out from the key the
    /// computer's TLS session proved.
    Take,
    /// `refused`, by this reason.
    Refuse(&'static str),
    /// `refused`, by this reason, the first time; then as the real station takes one.
    RefuseOnce(&'static str),
    /// `enrolled`, with a device id no station works out from that key.
    OtherDevice,
}

/// A stand-in station whose id on its network is `station`, answering every enrolment as `answer`
/// says (a refusal when one is given, else as the real station takes one). It listens on this box's private
/// address, as a station does (a computer keeps no other address for one); `None` on a box with no
/// private address, where the test is skipped as the station's own are.
async fn standing(station: &'static str, refusal: Option<&'static str>) -> Option<Standing> {
    standing_as(station, refusal.map_or(Answer::Take, Answer::Refuse)).await
}

async fn standing_as(station: &'static str, answer: Answer) -> Option<Standing> {
    let Some(private) = own_private_address() else {
        eprintln!("skipped: no private address");
        return None;
    };
    let identity = tls::Identity::new(&fixture_key(), station.into()).unwrap();
    let key = identity.public_key().to_string();
    let config = tls::server(&identity, Arc::new(|_: &[u8; 32]| None), Arc::new(|| true)).unwrap();
    let listener = tokio::net::TcpListener::bind((private, 0)).await.unwrap();
    let SocketAddr::V4(at) = listener.local_addr().unwrap() else {
        unreachable!()
    };
    let heard: Arc<Mutex<Vec<(Value, String)>>> = Arc::default();
    let kept = heard.clone();
    let refused_once = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (config, kept) = (config.clone(), kept.clone());
            let refused_once = refused_once.clone();
            tokio::spawn(async move {
                let Ok(tls) = tokio_rustls::TlsAcceptor::from(config).accept(stream).await else {
                    return;
                };
                let spki = tls.get_ref().1.peer_certificates().unwrap()[0].to_vec();
                let Ok(mut socket) = tokio_tungstenite::accept_async(tls).await else {
                    return;
                };
                let Some(Ok(Message::Text(text))) = socket.next().await else {
                    return;
                };
                let message: Value = serde_json::from_str(&text).unwrap_or_default();
                let answer = match answer {
                    Answer::Refuse(reason) => json!({"type":"refused","reason":reason}),
                    Answer::RefuseOnce(reason) if !refused_once.swap(true, Ordering::SeqCst) => {
                        json!({"type":"refused","reason":reason})
                    }
                    Answer::Take | Answer::RefuseOnce(_) => json!({"type":"enrolled",
                        "deviceId":book::device_id(&tls::pin(&spki)),"stationId":station}),
                    Answer::OtherDevice => json!({"type":"enrolled",
                        "deviceId":book::device_id(&[7; 32]),"stationId":station}),
                };
                kept.lock()
                    .unwrap()
                    .push((message, tempo_client::hex(&spki)));
                let _ = socket.send(Message::Text(answer.to_string().into())).await;
                let _ = socket.close(None).await;
            });
        }
    });
    Some(Standing { at, key, heard })
}

/// An advert of a station at `at` whose key tag is the start of `key`'s fingerprint.
fn advert(at: SocketAddrV4, key: &str) -> Found {
    let spki = tempo_stream::protocol::hex_bytes(key).unwrap();
    let fingerprint = tempo_client::hex(&tls::pin(&spki));
    Found {
        name: "Nexus".into(),
        address: at,
        protocol: 1,
        key: fingerprint[..16].to_string(),
    }
}

/// The public half of a key this computer keeps, as the station and the service see it.
fn public(pkcs8: &str) -> String {
    ComputerKey::restore(pkcs8)
        .unwrap()
        .public_key()
        .to_string()
}

/// What a computer's enrolment says, as the station reads it.
fn enrolment(road: &str, proof: &str) -> Value {
    json!({"type":"enrol","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
        "name":"Den PC","kind":"computer","publicKey":road,"proof":proof})
}

/// This computer's key for its road as the station shows it, for the device the service last
/// registered: empty when none was, which the `Signed` it is compared in then shows.
fn last_shown(cloud: &Cloud) -> String {
    let seen = cloud.seen.lock().unwrap();
    seen.devices
        .last()
        .map(|(body, _)| shown(body))
        .unwrap_or_default()
}

/// The proof the stand-in issuer gave the last sign-in.
fn proof_given(cloud: &Cloud) -> String {
    let seen = cloud.seen.lock().unwrap();
    unsigned_token(
        &seen.subject,
        seen.issued_at,
        seen.nonce.as_deref().unwrap(),
    )
}

/// ★ One sign-in, and the station is on this computer on its network and over the internet: the
/// sign-in names exactly the two keys this computer then keeps; the service is given the road key,
/// the LAN key and the proof, signed by the road key; the station, found by name, hears the
/// enrolment on TLS proved by the LAN key, with the road key and the same proof; and what is kept
/// is the station's record on its network (its LAN key pinned, the ids it gave, where it answered),
/// its internet road (the service's ids and the station key the account listed) and when the proof
/// was signed. The staged keys go. The service approved the road, and since the 2026-10-23 sign-in decision the station
/// grants control on a sign-in (`remote_service::account::GRANTS_ON_SIGN_IN`), so the road does not
/// wait for Approve at the station — as every approved road a sign-in adds below does not.
#[tokio::test(flavor = "multi_thread")]
async fn one_sign_in_adds_the_station_on_its_network_and_over_the_internet() {
    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("the-stations-proof"),
        "owner"
    )]));
    let (find, looked) = finding(vec![advert(station.at, &station.key)]);
    let kept = stations(&store, &scratch.0);
    let env = env(&cloud, kept.clone(), find, trusting);
    let (signed, opened) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(
        signed,
        Ok(Signed {
            station: LAN_STATION.into(),
            waiting: false,
            away: false,
            key: last_shown(&cloud)
        })
    );
    assert_eq!(looked.load(Ordering::SeqCst), 1);
    let record = kept.get(LAN_STATION).unwrap().unwrap();
    let road = kept.internet(LAN_STATION).unwrap().unwrap();
    let (road_key, lan_key) = (public(&road.pkcs8), public(&record.pkcs8));
    assert!(
        opened.ends_with(&format!("&nonce={}", nonce_for(&road.pkcs8, &record.pkcs8))),
        "{opened}"
    );
    let proof = proof_given(&cloud);
    {
        let seen = cloud.seen.lock().unwrap();
        assert_eq!(seen.devices.len(), 1);
        let (mut body, authorization) = seen.devices[0].clone();
        body.as_object_mut().unwrap().remove("platform");
        assert_eq!(authorization, "Bearer access-token-never-to-be-shown");
        assert_eq!(
            body,
            json!({"name":"Den PC","kind":"computer","publicKey":road_key,"lanKey":lan_key,
                "proof":proof})
        );
        assert!(seen
            .paths()
            .last()
            .unwrap()
            .ends_with(&format!("/stations/{SERVICE_STATION}/client/device")));
    }
    assert_eq!(
        *station.heard.lock().unwrap(),
        vec![(enrolment(&road_key, &proof), lan_key.clone())]
    );
    let spki = tempo_stream::protocol::hex_bytes(&lan_key).unwrap();
    assert_eq!(
        (
            record.station_id.as_str(),
            record.device_id.clone(),
            record.station_key.as_str(),
            record.addresses.clone()
        ),
        (
            LAN_STATION,
            book::device_id(&tls::pin(&spki)),
            station.key.as_str(),
            vec![station.at.to_string()]
        )
    );
    assert_eq!(
        (
            road.station_id.as_str(),
            road.device_id.clone(),
            road.station_key.as_str()
        ),
        (SERVICE_STATION, device_at(0), signing_key.public_key())
    );
    assert_eq!(
        kept.signed(LAN_STATION),
        Some(cloud.seen.lock().unwrap().issued_at)
    );
    assert!(store.staged_keys().unwrap().is_none(), "the staged keys go");
}

/// ★ A sign-in that stopped part-way (registered at the service, but the station could not keep
/// this computer this time) keeps nothing but the keys it made, and the next sign-in names those
/// same keys: the service finds the device it registered, and the station hears the LAN key the
/// first sign-in vouched for.
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_that_stopped_part_way_is_tried_again_with_the_same_keys() {
    let Some(station) = standing_as(LAN_STATION, Answer::RefuseOnce("unavailable")).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);

    let first = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(station.at, &station.key)]).0,
        trusting,
    );
    let (signed, opened_first) =
        signing_in(&first, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(signed, Err("stationCouldNotKeep"));
    assert!(kept.list().unwrap().is_empty());
    assert!(
        kept.list_away().unwrap().is_empty(),
        "kept by its service id"
    );
    let staged = store.staged_keys().unwrap().expect("the keys kept");
    let (road, lan) = (staged.road.clone(), staged.lan.clone());

    let again = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(station.at, &station.key)]).0,
        trusting,
    );
    let (signed, opened_again) =
        signing_in(&again, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert!(signed.is_ok(), "{signed:?}");
    let nonce = |opened: &str| opened.rsplit("&nonce=").next().unwrap().to_string();
    assert_eq!(nonce(&opened_first), nonce_for(&road, &lan));
    assert_eq!(nonce(&opened_again), nonce(&opened_first));
    let seen = cloud.seen.lock().unwrap();
    assert_eq!(seen.keys, vec![public(&road)], "one device at the service");
    assert_eq!(seen.devices.len(), 2);
    assert_eq!(station.heard.lock().unwrap()[0].1, public(&lan));
    assert_eq!(kept.get(LAN_STATION).unwrap().unwrap().pkcs8, lan);
}

/// ★ A station a look by name does not find (an advert for another key is never tried) is asked
/// for: the operator says where it is, an address that does not answer is asked about again with
/// why, and where it does answer it is enrolled and kept.
#[tokio::test(flavor = "multi_thread")]
async fn the_station_is_found_where_the_operator_says_it_is() {
    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let Some(elsewhere) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);
    let nowhere = where_nothing_listens(*station.at.ip());
    let (ask, asked) = answering(vec![nowhere.to_string(), station.at.to_string()]);
    let env = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(elsewhere.at, &elsewhere.key)]).0,
        trusting,
    );
    let (signed, _) = signing_in(&env, &cloud, None, no_pick(), ask).await;
    assert!(signed.is_ok(), "{signed:?}");
    assert!(
        elsewhere.heard.lock().unwrap().is_empty(),
        "another key's advert was tried"
    );
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert_eq!(asked[0], ("Shack".to_string(), None));
    assert_eq!(asked[1].0, "Shack");
    assert!(asked[1].1.is_some_and(road_unreached), "{asked:?}");
    assert_eq!(
        kept.get(LAN_STATION).unwrap().unwrap().addresses,
        vec![station.at.to_string()]
    );
}

fn road_unreached(why: &str) -> bool {
    crate::lan_client::road::UNREACHED.contains(&why)
}

/// ★ Another machine at an address the station seemed to be at (a forged advert naming the
/// station's key tag) never hears the proof: TLS pins the LAN key the account vouched for, and the
/// other key is refused before this computer says anything. The operator is then asked where the
/// station is, told the key there was another; with no answer, the station is kept by its service
/// id for the internet road, the LAN key the account vouched for still the one pinned.
#[tokio::test(flavor = "multi_thread")]
async fn another_machine_at_the_stations_address_never_hears_the_proof() {
    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let Some(impostor) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    let (ask, asked) = answering(Vec::new());
    let kept = stations(&store, &scratch.0);
    let env = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(impostor.at, &station.key)]).0,
        trusting,
    );
    let (signed, _) = signing_in(&env, &cloud, None, no_pick(), ask).await;
    assert_eq!(
        signed,
        Ok(Signed {
            station: SERVICE_STATION.into(),
            waiting: false,
            away: true,
            key: last_shown(&cloud)
        })
    );
    assert!(
        impostor.heard.lock().unwrap().is_empty(),
        "the impostor heard the proof"
    );
    assert!(station.heard.lock().unwrap().is_empty());
    assert_eq!(
        *asked.lock().unwrap(),
        vec![("Shack".to_string(), Some("keyChanged"))]
    );
    assert_eq!(
        kept.away(SERVICE_STATION).unwrap().unwrap().station_key,
        station.key
    );
}

/// ★ Signed in from elsewhere, first contact on the station's network. A station kept by its service id is signed
/// in to from its row there: the sign-in names the SAME two keys this computer kept for it, and asks
/// the service for nothing but the sign-in (no second device there, no second enrolment); the
/// station, found by name and reached on TLS pinned to the LAN key the account vouched for at first
/// sight, hears the enrolment with the new proof. It is then kept in place as any station on its
/// network: its record by its network's ids, the same road (the same device at the service) under
/// them, and the new proof's time, from which its thirty days there run (rulings 490, 528).
/// CONTROL: still away (another machine advertising its key tag, no address from the operator), it
/// stays as it was, and that machine never hears the proof.
#[tokio::test(flavor = "multi_thread")]
async fn first_contact_on_its_network_keeps_a_station_kept_away_in_place() {
    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let Some(impostor) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);
    // Signed in from elsewhere: found nowhere, so kept by its service id.
    let elsewhere = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, _) =
        signing_in(&elsewhere, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert!(signed.is_ok_and(|signed| signed.away));
    let away = kept.away(SERVICE_STATION).unwrap().unwrap();
    let road = kept.internet(SERVICE_STATION).unwrap().unwrap();

    // CONTROL: still not on its network.
    let (ask, asked) = answering(Vec::new());
    let still = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(impostor.at, &station.key)]).0,
        trusting,
    );
    let (signed, _) =
        signing_in(&still, &cloud, Some(SERVICE_STATION.into()), no_pick(), ask).await;
    assert_eq!(signed, Err("notOnThisNetwork"));
    assert!(
        impostor.heard.lock().unwrap().is_empty(),
        "the impostor heard the proof"
    );
    assert_eq!(
        *asked.lock().unwrap(),
        vec![("Shack".to_string(), Some("keyChanged"))]
    );
    assert!(
        kept.away(SERVICE_STATION).unwrap().is_some(),
        "kept as it was"
    );
    assert!(kept.list().unwrap().is_empty());

    // On the station's network at last, a day later.
    cloud.seen.lock().unwrap().issued_at += 86_400;
    let (find, looked) = finding(vec![advert(station.at, &station.key)]);
    let home = env(&cloud, kept.clone(), find, trusting);
    let before = cloud.seen.lock().unwrap().requests.len();
    let (signed, opened) = signing_in(
        &home,
        &cloud,
        Some(SERVICE_STATION.into()),
        no_pick(),
        answering(Vec::new()).0,
    )
    .await;
    assert_eq!(
        signed,
        Ok(Signed {
            station: LAN_STATION.into(),
            waiting: false,
            away: false,
            key: last_shown(&cloud)
        })
    );
    assert_eq!(looked.load(Ordering::SeqCst), 1);
    assert!(
        opened.ends_with(&format!("&nonce={}", nonce_for(&road.pkcs8, &away.pkcs8))),
        "{opened}"
    );
    let (paths, devices, issued_at) = {
        let seen = cloud.seen.lock().unwrap();
        (seen.paths(), seen.devices.len(), seen.issued_at)
    };
    assert_eq!(
        paths[before..],
        ["/api/remote/config".to_string(), "/oauth/token".to_string()]
    );
    assert_eq!(devices, 1, "a second device at the service");
    assert_eq!(
        *station.heard.lock().unwrap(),
        vec![(
            enrolment(&public(&road.pkcs8), &proof_given(&cloud)),
            public(&away.pkcs8)
        )]
    );
    let record = kept.get(LAN_STATION).unwrap().expect("kept on its network");
    let spki = tempo_stream::protocol::hex_bytes(&public(&away.pkcs8)).unwrap();
    assert_eq!(
        (
            record.device_id,
            record.station_key,
            record.pkcs8,
            record.addresses
        ),
        (
            book::device_id(&tls::pin(&spki)),
            station.key.clone(),
            away.pkcs8.clone(),
            vec![station.at.to_string()]
        )
    );
    let now_road = kept.internet(LAN_STATION).unwrap().expect("its road");
    assert_eq!(
        (now_road.device_id, now_road.pkcs8),
        (road.device_id.clone(), road.pkcs8.clone())
    );
    assert_eq!(kept.signed(LAN_STATION), Some(issued_at));
    assert!(kept.away(SERVICE_STATION).unwrap().is_none());
    assert!(kept.internet(SERVICE_STATION).unwrap().is_none());
    assert!(kept.list_away().unwrap().is_empty());
}

/// ★ A station that refuses the enrolment says why, and nothing is kept but the staged keys; and
/// one that answers with a device id it did not work out from this computer's key is no station's
/// answer (`notStation`), and keeps nothing either.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_enrolment_is_said_and_keeps_nothing() {
    for (answer, said) in [
        (Answer::Refuse("proofNotYours"), "proofNotYours"),
        (Answer::OtherDevice, "notStation"),
    ] {
        refused_with(answer, said).await;
    }
}

async fn refused_with(answer: Answer, said: &'static str) {
    let Some(station) = standing_as(LAN_STATION, answer).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "operate"
    )]));
    let kept = stations(&store, &scratch.0);
    let env = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(station.at, &station.key)]).0,
        trusting,
    );
    let (signed, _) = signing_in(&env, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(signed, Err(said));
    assert!(kept.list().unwrap().is_empty());
    assert!(store.staged_keys().unwrap().is_some());
}

/// This computer's record for `station`, as code pairing kept it, with a key of its own.
fn code_paired(station: &Standing) -> PairedStation {
    let (key, pkcs8) = ComputerKey::generate().unwrap();
    let spki = tempo_stream::protocol::hex_bytes(key.public_key()).unwrap();
    PairedStation {
        station_id: LAN_STATION.into(),
        device_id: book::device_id(&tls::pin(&spki)),
        station_key: station.key.clone(),
        pkcs8,
        addresses: vec![station.at.to_string()],
    }
}

/// ★ A station paired here with a code is linked to the account in place: the sign-in names the
/// pairing's own LAN key (and a new road key), the service is given that LAN key, the station hears
/// the enrolment on TLS proved by that same key at the address it answered at before, and the
/// pairing's record is left as it was; the internet road and the sign-in's time are kept beside it.
/// CONTROL: a station answering under another id on its network is not the one paired here
/// (`notThisStation`), and nothing is kept for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_station_paired_with_a_code_is_linked_in_place() {
    let Some(other) = standing("60000000-0000-4000-8000-0000000000bb", None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &other.key,
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);
    kept.keep(&code_paired(&other)).unwrap();
    let control = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, _) = signing_in(
        &control,
        &cloud,
        Some(LAN_STATION.into()),
        no_pick(),
        answering(Vec::new()).0,
    )
    .await;
    assert_eq!(signed, Err("notThisStation"));
    assert!(kept.internet(LAN_STATION).unwrap().is_none());
    assert_eq!(kept.signed(LAN_STATION), None);

    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);
    let record = code_paired(&station);
    kept.keep(&record).unwrap();
    let (find, looked) = finding(Vec::new());
    let env = env(&cloud, kept.clone(), find, trusting);
    let (signed, opened) = signing_in(
        &env,
        &cloud,
        Some(LAN_STATION.into()),
        no_pick(),
        answering(Vec::new()).0,
    )
    .await;
    assert_eq!(
        signed,
        Ok(Signed {
            station: LAN_STATION.into(),
            waiting: false,
            away: false,
            key: last_shown(&cloud)
        })
    );
    assert_eq!(
        looked.load(Ordering::SeqCst),
        0,
        "found where it answered before"
    );
    let road = kept.internet(LAN_STATION).unwrap().unwrap();
    assert!(opened.ends_with(&format!("&nonce={}", nonce_for(&road.pkcs8, &record.pkcs8))));
    let proof = proof_given(&cloud);
    assert_eq!(
        cloud.seen.lock().unwrap().devices[0].0["lanKey"],
        public(&record.pkcs8)
    );
    assert_eq!(
        *station.heard.lock().unwrap(),
        vec![(
            enrolment(&public(&road.pkcs8), &proof),
            public(&record.pkcs8)
        )]
    );
    let now = kept.get(LAN_STATION).unwrap().unwrap();
    assert_eq!(
        (now.pkcs8, now.device_id, now.station_key),
        (record.pkcs8, record.device_id, record.station_key)
    );
    assert_eq!(road.station_id, SERVICE_STATION);
    assert_eq!(
        kept.signed(LAN_STATION),
        Some(cloud.seen.lock().unwrap().issued_at)
    );
}

/// ★ A station paired with a code whose internet road was added and approved before is linked with
/// a new road key, never that road's: the service keeps a key it approved as it was, with no LAN
/// key, and the station would then drop this computer from its network at its next list. The new
/// road is registered with the pairing's LAN key and the proof and kept; the old road's approval is
/// ended at the service, signed by its own key, as Forget ends it.
#[tokio::test(flavor = "multi_thread")]
async fn a_code_paired_station_with_an_old_road_is_linked_with_a_new_road_key() {
    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    let kept = stations(&store, &scratch.0);
    let record = code_paired(&station);
    kept.keep(&record).unwrap();
    let (_, old_pkcs8) = ComputerKey::generate().unwrap();
    // The service knows the old road's key as its first device, approved at the station.
    cloud.seen.lock().unwrap().keys.push(public(&old_pkcs8));
    kept.keep_internet(
        LAN_STATION,
        &InternetRoad {
            station_id: SERVICE_STATION.into(),
            device_id: device_at(0),
            station_key: signing_key.public_key().into(),
            pkcs8: old_pkcs8.clone(),
        },
    )
    .unwrap();
    let env = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, opened) = signing_in(
        &env,
        &cloud,
        Some(LAN_STATION.into()),
        no_pick(),
        answering(Vec::new()).0,
    )
    .await;
    assert_eq!(
        signed,
        Ok(Signed {
            station: LAN_STATION.into(),
            waiting: false,
            away: false,
            key: last_shown(&cloud)
        })
    );
    let road = kept.internet(LAN_STATION).unwrap().unwrap();
    assert_ne!(road.pkcs8, old_pkcs8, "a new road key");
    assert_eq!(road.device_id, device_at(1));
    assert!(opened.ends_with(&format!("&nonce={}", nonce_for(&road.pkcs8, &record.pkcs8))));
    let seen = cloud.seen.lock().unwrap();
    let body = &seen.devices[0].0;
    assert_eq!(
        (&body["publicKey"], &body["lanKey"]),
        (&json!(public(&road.pkcs8)), &json!(public(&record.pkcs8)))
    );
    assert_eq!(seen.forgotten, vec![device_at(0)], "the old road ended");
    assert!(store.staged_keys().unwrap().is_none());
}

/// ★ Signing in again for a station added with the account proves the same two keys anew and tells
/// only the station: the service is asked for its sign-in's audience and the token, never for the
/// account's stations or a device; the station hears the new proof on TLS proved by the same LAN
/// key; the sign-in's time moves to the new proof's. CONTROL: a station answering under another
/// id on its network is not this one (`notThisStation`), and the time stays.
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_again_proves_the_same_keys_and_tells_only_the_station() {
    for other in [false, true] {
        let Some(station) = standing(
            if other {
                "60000000-0000-4000-8000-0000000000bb"
            } else {
                LAN_STATION
            },
            None,
        )
        .await
        else {
            return;
        };
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        let kept = stations(&store, &scratch.0);
        let record = code_paired(&station);
        kept.keep(&record).unwrap();
        let (_, road_pkcs8) = ComputerKey::generate().unwrap();
        let (signing_key, _) = ComputerKey::generate().unwrap();
        kept.keep_internet(
            LAN_STATION,
            &InternetRoad {
                station_id: SERVICE_STATION.into(),
                device_id: device_at(0),
                station_key: signing_key.public_key().into(),
                pkcs8: road_pkcs8.clone(),
            },
        )
        .unwrap();
        let before = cloud.seen.lock().unwrap().issued_at - 20 * 86_400;
        kept.keep_signed(LAN_STATION, before).unwrap();
        let env = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
        let (signed, opened) = signing_in(
            &env,
            &cloud,
            Some(LAN_STATION.into()),
            no_pick(),
            answering(Vec::new()).0,
        )
        .await;
        assert!(opened.ends_with(&format!("&nonce={}", nonce_for(&road_pkcs8, &record.pkcs8))));
        assert_eq!(
            cloud.seen.lock().unwrap().paths(),
            vec!["/api/remote/config".to_string(), "/oauth/token".to_string()]
        );
        let heard = station.heard.lock().unwrap().clone();
        assert_eq!(
            heard,
            vec![(
                enrolment(&public(&road_pkcs8), &proof_given(&cloud)),
                public(&record.pkcs8)
            )]
        );
        if other {
            assert_eq!(signed, Err("notThisStation"));
            assert_eq!(kept.signed(LAN_STATION), Some(before));
        } else {
            assert_eq!(
                signed,
                Ok(Signed {
                    station: LAN_STATION.into(),
                    waiting: false,
                    away: false,
                    key: crate::remote_service::fingerprint(&public(&road_pkcs8)).unwrap()
                })
            );
            assert_eq!(
                kept.signed(LAN_STATION),
                Some(cloud.seen.lock().unwrap().issued_at)
            );
        }
    }
}

/// ★ Security review (2026-10-20): a station that says, on first contact, the id of another station
/// this computer holds (with another key) takes nothing of it. Adding it (`Stations::keep`) and,
/// for a station kept by its service id, first contact on its network (`Stations::fold`) are each
/// refused as no station's answer (`notStation`), and the station held under that id keeps its
/// record, its keys and its road exactly as they were, and the station kept away stays as it was.
/// CONTROL: the same steps with the station answering under an id of its own add it and fold it
/// (`one_sign_in_adds_the_station_on_its_network_and_over_the_internet`,
/// `first_contact_on_its_network_keeps_a_station_kept_away_in_place`).
#[tokio::test(flavor = "multi_thread")]
async fn a_station_saying_another_stations_id_replaces_nothing() {
    let Some(station) = standing(LAN_STATION, None).await else {
        return;
    };
    let (cloud, scratch, store) = (
        Cloud::start().await,
        Scratch::new(),
        StationStore::default(),
    );
    let kept = stations(&store, &scratch.0);
    // Another station already on this computer under LAN_STATION, with its road.
    let (held_key, _) = ComputerKey::generate().unwrap();
    let (_, held_pkcs8) = ComputerKey::generate().unwrap();
    kept.keep(&PairedStation {
        station_id: LAN_STATION.into(),
        device_id: device_at(7),
        station_key: held_key.public_key().into(),
        pkcs8: held_pkcs8,
        addresses: vec!["192.168.1.99:42075".into()],
    })
    .unwrap();
    let (held_signing, _) = ComputerKey::generate().unwrap();
    let (_, held_road) = ComputerKey::generate().unwrap();
    kept.keep_internet(
        LAN_STATION,
        &InternetRoad {
            station_id: "20000000-0000-4000-8000-0000000000b0".into(),
            device_id: device_at(8),
            station_key: held_signing.public_key().into(),
            pkcs8: held_road,
        },
    )
    .unwrap();
    let held = |store: &StationStore| {
        (
            store.raw(&format!("lan-station-{LAN_STATION}")),
            store.raw(&format!("lan-internet-{LAN_STATION}")),
        )
    };
    let before = held(&store);
    let (signing_key, _) = ComputerKey::generate().unwrap();
    cloud.lists(json!([row(
        signing_key.public_key(),
        &station.key,
        Some("p"),
        "owner"
    )]));
    // Adding it: found by name, it says LAN_STATION.
    let adding = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(station.at, &station.key)]).0,
        trusting,
    );
    let (signed, _) = signing_in(&adding, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert_eq!(signed, Err("notStation"), "adding");
    assert_eq!(held(&store), before, "adding replaced the station held");
    assert_eq!(station.heard.lock().unwrap().len(), 1, "premise: enrolled");
    // Kept by its service id from elsewhere, then first contact: it says LAN_STATION again.
    let elsewhere = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
    let (signed, _) =
        signing_in(&elsewhere, &cloud, None, no_pick(), answering(Vec::new()).0).await;
    assert!(signed.is_ok_and(|signed| signed.away), "premise: kept away");
    let home = env(
        &cloud,
        kept.clone(),
        finding(vec![advert(station.at, &station.key)]).0,
        trusting,
    );
    let (signed, _) = signing_in(
        &home,
        &cloud,
        Some(SERVICE_STATION.into()),
        no_pick(),
        answering(Vec::new()).0,
    )
    .await;
    assert_eq!(signed, Err("notStation"), "first contact");
    assert_eq!(
        held(&store),
        before,
        "first contact replaced the station held"
    );
    assert!(
        kept.away(SERVICE_STATION).unwrap().is_some(),
        "kept away as it was"
    );
}

/// ★ A station the account was invited to is bound to someone else's sign-in, which nothing on this
/// computer checks, so its internet road is kept HELD even when the station is reached on its
/// network: added there by a sign-in, and linked in place from a code pairing alike. CONTROL: the
/// account's own station (its proof the signed-in person's) is kept free, both ways.
#[tokio::test(flavor = "multi_thread")]
async fn an_invited_stations_road_is_held_on_its_network_too() {
    for (role, held) in [("operate", Some("stationKeyUnconfirmed")), ("owner", None)] {
        // Added by a sign-in, found by name on this network.
        let Some(station) = standing(LAN_STATION, None).await else {
            return;
        };
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        let (signing_key, _) = ComputerKey::generate().unwrap();
        cloud.lists(json!([row(
            signing_key.public_key(),
            &station.key,
            Some("p"),
            role
        )]));
        let kept = stations(&store, &scratch.0);
        let (find, _) = finding(vec![advert(station.at, &station.key)]);
        let added = env(&cloud, kept.clone(), find, trusting);
        let (signed, _) =
            signing_in(&added, &cloud, None, no_pick(), answering(Vec::new()).0).await;
        assert!(
            matches!(signed, Ok(Signed { away: false, .. })),
            "{role}: {signed:?}"
        );
        assert_eq!(kept.road_held(LAN_STATION), Ok(held), "{role}: added");

        // Linked in place from a code pairing.
        let Some(station) = standing(LAN_STATION, None).await else {
            return;
        };
        let (cloud, scratch, store) = (
            Cloud::start().await,
            Scratch::new(),
            StationStore::default(),
        );
        let (signing_key, _) = ComputerKey::generate().unwrap();
        cloud.lists(json!([row(
            signing_key.public_key(),
            &station.key,
            Some("p"),
            role
        )]));
        let kept = stations(&store, &scratch.0);
        kept.keep(&code_paired(&station)).unwrap();
        let linking = env(&cloud, kept.clone(), finding(Vec::new()).0, trusting);
        let (signed, _) = signing_in(
            &linking,
            &cloud,
            Some(LAN_STATION.into()),
            no_pick(),
            answering(Vec::new()).0,
        )
        .await;
        assert!(signed.is_ok(), "{role}: {signed:?}");
        assert_eq!(kept.road_held(LAN_STATION), Ok(held), "{role}: linked");
        drop(scratch);
    }
}
