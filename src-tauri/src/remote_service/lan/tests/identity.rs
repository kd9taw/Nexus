//! The station's key for the internet road, said by the station itself on this network's road: its
//! `opened` names it (`channel`), and the computer pins no key the service lists for a station it
//! can ask itself (`crate::lan_client::account::link`), and turns off a road kept for a station that
//! names another key here (`crate::client_mode`). Keys are made for each run, in memory; only their
//! public halves are compared.
use super::computer::{behind_a_port, behind_a_port_on, paired_record};
use super::*;
use crate::client_mode::tests::{scratch, screens};
use crate::client_mode::{Afar, Client, Find, Phase, ThisComputer};
use crate::lan_client::account::tests::{
    answering, env, finding, no_pick, row, signing_in, stations as kept_in, trusting, Cloud,
    Scratch as Kept, SERVICE_STATION,
};
use crate::lan_client::account::{self as signing, Signed};
use crate::lan_client::internet::Roads;
use crate::lan_client::link::Link;
use crate::lan_client::tests::{road_to, where_nothing_listens, StationStore};
use crate::lan_client::{ComputerKey, Stations};
use crate::remote_service::vault::{PairedStation, StationVault};
use tauri::Manager;

/// `s`'s station, paired with the Remote service under `key` (the public half): what its `opened`
/// names from then on. Set before anything serves it.
fn serving(s: &mut Shack, key: &str) {
    let key = key.to_string();
    s.shared.station_key = Arc::new(move || Some(key.clone()));
}

/// A key's public half, made for this run.
fn public() -> String {
    ComputerKey::generate().unwrap().0.public_key().to_string()
}

/// Welcomed on `s`'s station as its paired computer, then the lane opened with this build: the
/// welcome, and what the station answered the `open` with.
async fn welcomed_and_opened(s: &Shack) -> (Value, Value) {
    let (_stopping, stop) = watch::channel(false);
    let (_task, socket) = dial(s, &s.computer.key, PEER, stop).await;
    let mut socket = socket.unwrap();
    let welcome = hello(&mut socket, VERSIONS).await;
    send(
        &mut socket,
        json!({"type":"open","profile":"desktop","app":wire::APP,"contract":wire::contract(),
            "run":1}),
    )
    .await;
    (welcome, next(&mut socket).await)
}

/// ★ The station's `opened` names its key for the internet road: the signer's public half, SPKI as
/// the service lists it, and nothing of the private half anywhere in the frame.
#[tokio::test]
async fn opened_names_this_stations_key_and_no_private_half() {
    let (signer, pkcs8) = crate::remote_service::station_key::Signer::generate().unwrap();
    let mut s = shack(home());
    serving(&mut s, signer.public_key());
    let (welcome, opened) = welcomed_and_opened(&s).await;
    assert_eq!(welcome["type"], "welcome", "{welcome}");
    assert_eq!(
        opened,
        json!({"type":"opened","stationKey":signer.public_key()})
    );
    // The private half sits after the PKCS#8 document's `02 01 01 04 20`.
    assert_eq!(&pkcs8[62..72], "0201010420", "a P-256 PKCS#8 document");
    let private = &pkcs8[72..136];
    for frame in [opened.to_string(), welcome.to_string()] {
        assert!(
            !frame.contains(&pkcs8) && !frame.contains(private),
            "the private key on the wire: {frame}"
        );
    }
}

/// ★ A station with no pairing with the Remote service names no key: its `opened` is as it was.
#[tokio::test]
async fn opened_names_no_station_key_without_a_service_pairing() {
    let s = shack(home());
    let (_, opened) = welcomed_and_opened(&s).await;
    assert_eq!(opened, json!({"type":"opened"}));
}

/// ★ The welcome is unchanged, so a 1.20.0 computer, which reads it whole and exactly, still hears
/// its "update this computer" there: the key goes in `opened`, which only the same build reaches.
/// CONTROL: the client core's own strict reader of the welcome takes it, and the link opens.
#[tokio::test]
async fn welcome_is_unchanged_for_a_1_20_computer() {
    let mut s = shack(home());
    serving(&mut s, &public());
    let (welcome, opened) = welcomed_and_opened(&s).await;
    let fields: Vec<&str> = welcome
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for field in &fields {
        assert!(
            [
                "type",
                "sessionId",
                "deviceId",
                "stationId",
                "app",
                "contract",
                "stopRoad",
                "audioRoad"
            ]
            .contains(field),
            "the welcome grew {field}: {welcome}"
        );
    }
    assert!(opened.get("stationKey").is_some(), "{opened}");
    let (_stopping, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    let record = paired_record(&s, at);
    let (link, _) = Link::connect_at(&record, &[at], 1, None).await.unwrap();
    assert_eq!(link.station_key, (s.shared.station_key)());
    link.close();
}

/// ★ Linking a station paired here with a code, whose owner has not signed in at the shack: the
/// service lists the station's key, and this computer asks the station itself on this network
/// before anything is registered. A listed key the station does not say is refused
/// (`stationKeyDisagrees`): nothing registered at the service, no road kept, the staged keys gone.
/// CONTROL: the key the station says is registered and kept, with nothing to confirm; and a station
/// not found here is registered and kept HELD until the operator checks the key.
#[tokio::test(flavor = "multi_thread")]
async fn a_station_key_the_service_lists_is_not_pinned_when_the_station_says_another() {
    let Some(private) = own_private_address() else {
        eprintln!("skipped: no private address");
        return;
    };
    let real = public();
    for case in ["hostile", "home", "away"] {
        let mut s = shack(home());
        serving(&mut s, &real);
        let (_listening, stop) = watch::channel(false);
        let at = match case {
            "away" => where_nothing_listens(private),
            _ => behind_a_port_on(&s, private, stop).await,
        };
        let listed = if case == "hostile" {
            public()
        } else {
            real.clone()
        };
        let (cloud, kept, store) = (Cloud::start().await, Kept::new(), StationStore::default());
        // No proof: the owner has not signed in at the shack.
        cloud.lists(json!([row(&listed, &s.public_key, None, "owner")]));
        let stations = kept_in(&store, &kept.0);
        stations.keep(&paired_record(&s, at)).unwrap();
        let mut env = env(&cloud, stations.clone(), finding(Vec::new()).0, trusting);
        env.hear = signing::hear;
        let (signed, _) = signing_in(
            &env,
            &cloud,
            Some(STATION.into()),
            no_pick(),
            answering(Vec::new()).0,
        )
        .await;
        let registered = cloud.seen.lock().unwrap().devices.len();
        match case {
            "hostile" => {
                assert_eq!(signed, Err("stationKeyDisagrees"));
                assert_eq!(registered, 0, "registered at the service");
                assert!(stations.internet(STATION).unwrap().is_none(), "a road kept");
                assert!(store.staged_keys().unwrap().is_none(), "staged keys kept");
            }
            _ => {
                assert!(
                    matches!(signed, Ok(Signed { ref station, .. }) if station == STATION),
                    "{case}: {signed:?}"
                );
                assert_eq!(registered, 1, "{case}");
                let road = stations.internet(STATION).unwrap().unwrap();
                assert_eq!(road.station_key, real, "{case}");
                let held = if case == "away" {
                    Some("stationKeyUnconfirmed")
                } else {
                    None
                };
                assert_eq!(stations.road_held(STATION), Ok(held), "{case}");
            }
        }
    }
}

/// ★ A road kept for a station that, on this network's road (pinned to its key here), names another
/// key for the internet road is turned off: client mode opens nothing for it, the service is never
/// asked for a pipe, and it is refused `stationKeyDisagrees`, for good. CONTROL: a station naming
/// the key the road was pinned to leaves it as it was.
#[test]
fn a_kept_road_the_station_disowns_on_the_lan_is_turned_off() {
    let _alone = alone();
    let pinned = public();
    for says in [public(), pinned.clone()] {
        let disowns = says != pinned;
        let app = commands::App::new();
        let mut s = commands::running(&app);
        serving(&mut s, &says);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (_listening, stop) = watch::channel(false);
        let at = tauri::async_runtime::block_on(behind_a_port(&s, stop));
        // The road, as a sign-in kept it: pinned to the key the service listed.
        let store = StationStore::default();
        let mut road = road_to(SERVICE_STATION);
        road.station_key = pinned.clone();
        store.save_internet_road(STATION, &road).unwrap();
        let stations = Arc::new(Stations::new(Arc::new(store.clone())));
        // The service: every connection to it counted, none answered.
        let service = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", service.local_addr().unwrap());
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = asked.clone();
        std::thread::spawn(move || {
            for _ in service.incoming() {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        let roads = Arc::new(Roads::at(stations.clone(), &origin).unwrap());
        let dir = scratch("identity");
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        client.reach_afar(Arc::clone(&roads) as Arc<dyn Afar>);
        let screens = screens(&client);
        let record = paired_record(&s, at);
        let find: Find = Arc::new(move |_| {
            Ok(Some(PairedStation {
                station_id: record.station_id.clone(),
                device_id: record.device_id.clone(),
                station_key: record.station_key.clone(),
                pkcs8: record.pkcs8.clone(),
                addresses: record.addresses.clone(),
            }))
        });
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            Arc::new(|_| Ok(Vec::new())),
            // Typed, as a loopback address is tried only when it is.
            Some(tempo_stream::lan::Typed::At(at)),
        ));
        let mut last = client.status();
        for _ in 0..100 {
            last = client.status();
            if last.phase == Phase::Live && !last.internet {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            last.phase == Phase::Live && !last.internet,
            "never live on this network: {last:?}"
        );
        if disowns {
            assert_eq!(stations.road_held(STATION), Ok(Some("stationKeyDisagrees")));
            let opened = tauri::async_runtime::block_on(roads.open(STATION.into(), None));
            assert_eq!(opened.err(), Some("stationKeyDisagrees"));
            assert!(crate::lan_client::internet::lasting("stationKeyDisagrees"));
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(asked.load(Ordering::SeqCst), 0, "the service asked");
        } else {
            assert_eq!(stations.road_held(STATION), Ok(None));
        }
        driving.abort();
        tauri::async_runtime::block_on(client.let_go());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
