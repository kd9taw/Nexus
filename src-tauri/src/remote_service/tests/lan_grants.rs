//! Remote over this network beside the hosted mode (the operator's ruling of 2026-10-04, "Both at
//! once"): a computer paired over the network keeps its station control through every hosted
//! decision that clears the browsers' grants (Turn Remote on, Turn it off, revoking a browser), as
//! `restore` gives approved browsers theirs, and loses it with "End remote control", which turns
//! LAN off.
use super::*;
use crate::remote_service::lan::tests::{only, LanStore, Scratch};

const LAN_STATION: &str = "60000000-0000-4000-8000-0000000000aa";

/// A credential store holding this station's LAN key and one computer paired with it.
fn lan_store() -> LanStore {
    let (_, station) = station_key::Signer::generate().unwrap();
    let (computer, _) = station_key::Signer::generate().unwrap();
    let spki = tempo_stream::protocol::hex_bytes(computer.public_key()).unwrap();
    let pin: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, &spki)
        .as_ref()
        .try_into()
        .unwrap();
    LanStore::holding(&station, LAN_STATION, &[(&pin, "Den PC")])
}

/// The hosted service with its LAN road attached as `Service::new` attaches it, on `store` and a
/// switch of the test's own, with no network to listen on.
fn with_lan(
    service: Service,
    engine: &crate::SharedEngine,
    store: &LanStore,
    at: &Scratch,
) -> Service {
    service.with_lan(
        at.path(),
        engine.clone(),
        transport::Feeds {
            monitor: crate::remote_monitor::Publisher::default(),
            spectrum: None,
            meters: Default::default(),
            sources: None,
            #[cfg(feature = "radio")]
            audio: None,
            stream: stream::Host::default(),
        },
        Arc::new(lan::Book::new(Arc::new(store.clone()))),
        Arc::new(|_| only(Err(tempo_stream::lan::NoNetwork::Choose))),
    )
}

fn fresh_id() -> String {
    let hex = transport::random_secret().unwrap();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Can `device` take control on the LAN road now? The state and an acquire on a LAN connection
/// of its own, through the station's operations authority, then the connection closes. The
/// acquire's phase, or the refusal.
fn lan_acquire(service: &Service, engine: &crate::SharedEngine, device: &str) -> String {
    let authority = service.control.lock().unwrap().operations.clone();
    let connection = operations::LanConnection::new(authority.clone());
    let session = fresh_id();
    let ask = |request: operations::Request| {
        for _ in 0..50 {
            match authority.handle_version(
                (connection.id, 4),
                &session,
                device,
                &request,
                engine,
                Instant::now(),
            ) {
                Err("stationBusy" | "remoteBusy") => std::thread::sleep(Duration::from_millis(20)),
                other => return other,
            }
        }
        Err("busy throughout")
    };
    let boot = match ask(operations::Request::State {
        request_id: fresh_id(),
    }) {
        Ok(state) => state["stationBootId"].as_str().unwrap().to_string(),
        Err(refused) => return refused.into(),
    };
    match ask(operations::Request::Acquire {
        request_id: fresh_id(),
        station_boot_id: boot,
    }) {
        Ok(acquired) => acquired["phase"].as_str().unwrap_or("none").into(),
        Err(refused) => refused.into(),
    }
}

/// ★ Hosted Turn on, Turn off, Turn on again and revoking a browser each clear every grant the
/// station holds, and each leaves the LAN computer able to take control. "End remote control"
/// turns LAN off and takes its control with it, and LAN on gives it back. CONTROL: the hosted
/// browser's own grant goes with Turn off, as it always has.
#[tokio::test(flavor = "multi_thread")]
async fn a_lan_computer_keeps_control_through_hosted_turn_off_and_on() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let store = lan_store();
    let scratch = Scratch::new();
    let service = with_lan(launch(&cloud, &vault, &engine), &engine, &store, &scratch);
    service
        .action(Action::LanOn {
            address: None,
            port: None,
        })
        .await
        .unwrap();
    let status = eventually(&service, "the LAN book was read", |s| {
        s.lan.as_ref().is_some_and(|lan| !lan.devices.is_empty())
    })
    .await;
    let computer = status.lan.unwrap().devices[0].id.clone();
    assert_eq!(
        lan_acquire(&service, &engine, &computer),
        "controlling",
        "premise"
    );

    on_with_grants(&service).await;
    assert!(
        service
            .status()
            .unwrap()
            .station_permissions
            .contains(&BROWSER.to_string()),
        "premise: the browser's grant"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &computer),
        "controlling",
        "stranded by Turn on"
    );

    service.action(Action::Disable {}).await.unwrap();
    assert!(
        !service
            .status()
            .unwrap()
            .station_permissions
            .contains(&BROWSER.to_string()),
        "control: Turn off kept the browser's grant"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &computer),
        "controlling",
        "stranded by Turn off"
    );

    service.action(Action::Enable {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &computer),
        "controlling",
        "stranded by Turn on again"
    );

    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            transmit: false,
            key: None,
        })
        .await
        .unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &computer),
        "controlling",
        "stranded by revoking a browser"
    );

    service.action(Action::TakeOverLogging {}).await.unwrap();
    let lan = service.status().unwrap().lan.unwrap();
    assert!(!lan.on);
    assert_eq!(lan.reason, Some("endedAtShack"));
    assert_eq!(
        lan_acquire(&service, &engine, &computer),
        "localPermissionRequired",
        "End remote control left it control"
    );
    assert!(!service
        .status()
        .unwrap()
        .station_permissions
        .contains(&computer));

    service
        .action(Action::LanOn {
            address: None,
            port: None,
        })
        .await
        .unwrap();
    assert_eq!(lan_acquire(&service, &engine, &computer), "controlling");
}

/// The status a hosted-only build of the service sends is unchanged: no `lan` at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_service_without_a_lan_road_says_nothing_of_it() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = Arc::new(Mutex::new(Engine::with_settings(Settings::default())));
    let service = launch(&cloud, &vault, &engine);
    let status = serde_json::to_value(service.status().unwrap()).unwrap();
    assert!(status.get("lan").is_none(), "{status}");
    let scratch = Scratch::new();
    let service = with_lan(service, &engine, &lan_store(), &scratch);
    let status = serde_json::to_value(service.status().unwrap()).unwrap();
    assert_eq!(status["lan"]["on"], false, "control: {status}");
}
