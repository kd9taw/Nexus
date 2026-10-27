//! Two Nexus over loopback, end to end: client mode (`crate::client_mode`) on this computer,
//! through Tauri's own IPC on its mock runtime, driving the shack's real channel and command lane
//! behind a loopback port. It connects, takes the lease and the feeds, and every window's calls go
//! where their class says; Leave hands the lease back at once.
use super::commands::{running, App};
use super::computer::{behind_a_port, behind_a_port_on, paired_record};
use super::*;
use crate::client_mode::tests::{ask, eventually, scratch, screens};
use crate::client_mode::{Client, Find, Phase, ThisComputer};
use crate::lan_client::link::Link;
use crate::remote_service::vault::PairedStation;
use tauri::Manager;

/// ★ Client mode drives the shack, end to end: it connects to the station's real lane, takes the
/// lease, and the screens' snapshot poll is answered from the feed the shack pushes; a station
/// command runs at the shack, with this computer's own two settings shown in place of the shack's;
/// a start, with no Stop road beside this link, never leaves this computer, a Stop runs at the
/// shack, and a command of this computer's runs here. Leave hands the lease back, so control is
/// free at once. CONTROL: the shack ran exactly what was sent to it.
#[test]
fn client_mode_drives_the_shack_end_to_end() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("end-to-end");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    let screens = screens(&client);
    // The vault's record, as the store hands it out each time it is asked (a record carries this
    // computer's key, so it is never cloned).
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(tempo_stream::lan::Typed::At(at)),
    ));
    eventually("control taken", || client.status().phase == Phase::Live);
    assert_eq!(client.status().station, at.to_string());
    eventually("the shack's snapshot here", || {
        ask(&screens.main, "get_snapshot", json!({})).is_ok_and(|snapshot| snapshot["n"].is_u64())
    });
    let mut settings = ask(&screens.main, "get_settings", json!({})).unwrap();
    let revision = settings
        .as_object_mut()
        .and_then(|fields| fields.remove("nexusRevision"));
    assert_eq!(
        settings,
        json!({ "settings": "as read", "betaUpdates": false, "launchAtLogin": false })
    );
    assert!(
        revision
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|r| r.len() == 64),
        "the shack's revision of the read, to the screen: {revision:?}"
    );
    assert_eq!(
        ask(&screens.main, "call_cq", json!({})),
        Err(json!("nexusClient:noStopRoad"))
    );
    assert!(ask(&screens.main, "halt_tx", json!({})).is_ok());
    assert_eq!(
        ask(&screens.popout, "app_version", json!({})),
        Ok(json!("ran here"))
    );
    assert_eq!(sent_here(&app), ["get_settings", "halt_tx"]);
    assert_eq!(screens.ran_here(), ["app_version"]);

    driving.abort();
    tauri::async_runtime::block_on(async {
        client.let_go().await;
        let (other, _) = Link::connect(&record, Some(at), 7, None)
            .await
            .expect("the link opens again");
        let mut state = json!({});
        for _ in 0..100 {
            let answer = other
                .operation(json!({"type":"state","requestId":id()}))
                .await
                .expect("an answer");
            if answer["error"] != "stationBusy" && answer["error"] != "remoteBusy" {
                state = answer;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(state["value"]["phase"], "available", "{state}");
    });
    assert_eq!(sent_here(&app), ["get_settings", "halt_tx"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What the shack ran for this computer's calls: everything it ran but the feeds' own commands,
/// which its lane runs itself while a watched feed is pushed (`App::ran` already leaves out the
/// snapshot's).
fn sent_here(app: &App) -> Vec<String> {
    app.ran()
        .into_iter()
        // The feeds' own reads at the shack, which run on their own pace whatever the screens do.
        .filter(|cmd| cmd != "get_meters" && cmd != "get_spectrum_row" && cmd != "get_rtty_scope")
        .collect()
}

/// ★ A station that proves the key pinned for its pairing but then names itself another station
/// is not the one this computer was asked to drive: refused, said why, and nothing runs there.
/// CONTROL (above): the station that names itself the one asked for is driven.
#[test]
fn a_station_that_names_itself_another_is_refused() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("another");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let asked_for = "0aaaaaaa-0000-4000-8000-0000000000aa";
    let client = Arc::new(Client::new(asked_for.to_string(), this));
    let screens = screens(&client);
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(tempo_stream::lan::Typed::At(at)),
    ));
    eventually("refused", || {
        client.status().reason.as_deref() == Some("notThisStation")
    });
    assert_eq!(client.status().phase, Phase::Refused);
    assert_eq!(
        ask(&screens.main, "get_band_plan", json!({})),
        Err(json!("nexusClient:notLive"))
    );
    driving.abort();
    assert!(sent_here(&app).is_empty(), "{:?}", app.ran());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ The lockout after an update here: a station still on an older build refuses this computer,
/// and the status the banner reads names both versions, so it can say which side to update and to
/// what. CONTROL: before any welcome the station's version is not known; this computer's always is.
#[test]
fn a_station_of_an_older_build_is_refused_naming_both_versions() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let mut s = running(&app);
        s.shared.build = ("0.0.1", wire::contract());
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("older-build");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    let before = client.status();
    assert_eq!(
        (before.computer_build, before.station_build),
        (wire::APP, None)
    );
    let screens = screens(&client);
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(tempo_stream::lan::Typed::At(at)),
    ));
    eventually("refused", || {
        client.status().reason.as_deref() == Some("updateStation")
    });
    let status = client.status();
    assert_eq!(status.phase, Phase::Refused);
    let said = serde_json::to_value(&status).unwrap();
    assert_eq!(said["computerBuild"], json!(wire::APP));
    assert_eq!(said["stationBuild"], json!("0.0.1"));
    driving.abort();
    assert!(sent_here(&app).is_empty(), "{:?}", app.ran());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ A station not where it was is found by name. With its one remembered address dead (a new
/// lease from the router, say) and no address typed, client mode looks for the station by name,
/// past another station's advert at a port of this one, and drives it where its own advert says
/// it is. CONTROL: with only the other station's advert found, it is never tried, nothing is
/// driven, and the station's own answer is told (nothing answered at the address it remembered).
/// Skipped, saying so, on a box with no private address of its own.
#[test]
fn client_mode_finds_a_station_by_name_when_its_remembered_address_is_dead() {
    use crate::lan_client::tests::where_nothing_listens;
    use ring::digest::{digest, SHA256};
    use tempo_stream::lan::dnssd::Found;
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let app = App::new();
    let (s, live, also, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let live = behind_a_port_on(&s, private, stop.clone()).await;
        let also = behind_a_port_on(&s, private, stop).await;
        (s, live, also, stopper)
    });
    let dead = where_nothing_listens(private);
    let record = paired_record(&s, live);
    let spki = tempo_stream::protocol::hex_bytes(&s.public_key).unwrap();
    let tag: String = digest(&SHA256, &spki)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..16]
        .to_string();
    let advert = |address: SocketAddrV4, key: &str| Found {
        name: "Nexus 3F2A 9B1C".into(),
        address,
        protocol: 1,
        key: key.into(),
    };
    let other = advert(also, "00c0ffee00c0ffee");
    for (adverts, found) in [
        (vec![other.clone(), advert(live, &tag)], true),
        (vec![other], false),
    ] {
        let dir = scratch("by-name");
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let screens = screens(&client);
        let (device, key, pkcs8) = (
            record.device_id.clone(),
            record.station_key.clone(),
            record.pkcs8.clone(),
        );
        let find: Find = Arc::new(move |_| {
            Ok(Some(PairedStation {
                station_id: STATION.into(),
                device_id: device.clone(),
                station_key: key.clone(),
                pkcs8: pkcs8.clone(),
                addresses: vec![dead.to_string()],
            }))
        });
        let looks = Arc::new(AtomicUsize::new(0));
        let look: crate::lan_client::road::Look = {
            let looks = looks.clone();
            Arc::new(move |_| {
                looks.fetch_add(1, Ordering::SeqCst);
                Ok(adverts.clone())
            })
        };
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            look,
            None,
        ));
        if found {
            eventually("control taken where it was found", || {
                client.status().phase == Phase::Live
            });
            assert_eq!(client.status().station, live.to_string());
        } else {
            eventually("a look made", || looks.load(Ordering::SeqCst) >= 1);
            eventually("the station's own answer told", || {
                client.status().reason.is_some()
            });
            let status = client.status();
            assert_ne!(status.phase, Phase::Live, "{status:?}");
            assert!(
                crate::lan_client::road::UNREACHED.contains(&status.reason.as_deref().unwrap()),
                "{status:?}"
            );
        }
        driving.abort();
        tauri::async_runtime::block_on(client.let_go());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// ⛔ A lease heartbeat the shack answers as busy keeps the lease. Client mode drives the shack
/// with the lease taken; then the shack's authority is busy for 2.5 s (its lock held, as another
/// request holds it a moment), so each heartbeat that comes meanwhile is answered `remoteBusy`.
/// The lease is held at every look, every 50 ms, through it and after: dropping it there stopped
/// the Stop road's heartbeat and the receive audio for a turn, then took the lease again. CONTROL:
/// the lease was held before the busy time, and looked at through it and 1.5 s past it; with the
/// busy answer read as a refusal, as before, the looks find it dropped.
#[test]
fn a_lease_heartbeat_answered_busy_keeps_the_lease() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("busy-heartbeat");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    let screens = screens(&client);
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(tempo_stream::lan::Typed::At(at)),
    ));
    eventually("control taken", || client.lease_held().is_some());
    let before = client.lease_held();
    let busy = s.shared.authority.busy_for_a_test();
    let mut looks = Vec::new();
    let until = std::time::Instant::now() + Duration::from_millis(2500);
    while std::time::Instant::now() < until {
        looks.push(client.lease_held());
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(busy);
    for _ in 0..30 {
        looks.push(client.lease_held());
        std::thread::sleep(Duration::from_millis(50));
    }
    driving.abort();
    tauri::async_runtime::block_on(client.let_go());
    std::fs::remove_dir_all(&dir).unwrap();
    let dropped = looks.iter().filter(|held| **held != before).count();
    assert_eq!(
        (before.is_some(), looks.len() >= 70, dropped),
        (true, true, 0),
        "(held before, looked through it, looks when the lease was not the one held before)"
    );
}

/// ★ Take control from a second computer, through client mode end to end. Computer A holds the
/// station; B, started after it, is kept out ("another device"), with nothing said of a take. B's
/// Take control: B holds control, and A's banner says another computer took it. A takes it back
/// the same way, and B's banner says so. CONTROL: before B's take, A held control and B was out.
#[test]
fn a_second_computer_takes_control_and_the_first_can_take_it_back() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let drive = |name: &str| {
        let dir = scratch(name);
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let screens = screens(&client);
        let (device, key, pkcs8) = (
            record.device_id.clone(),
            record.station_key.clone(),
            record.pkcs8.clone(),
        );
        let find: Find = Arc::new(move |_| {
            Ok(Some(PairedStation {
                station_id: STATION.into(),
                device_id: device.clone(),
                station_key: key.clone(),
                pkcs8: pkcs8.clone(),
                addresses: vec![at.to_string()],
            }))
        });
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            Arc::new(|_| Ok(Vec::new())),
            Some(tempo_stream::lan::Typed::At(at)),
        ));
        (client, driving, dir, screens)
    };
    let seen = |client: &Client| {
        let status = client.status();
        (status.phase, status.reason)
    };
    let (a, a_driving, a_dir, _a_screens) = drive("take-control-a");
    eventually("A holds control", || a.lease_held().is_some());
    let (b, b_driving, b_dir, _b_screens) = drive("take-control-b");
    eventually("B is kept out", || seen(&b) == (Phase::Busy, None));
    assert!(a.lease_held().is_some(), "premise: A holds control");

    b.take_control();
    eventually("B holds control", || {
        b.lease_held().is_some() && seen(&b) == (Phase::Live, None)
    });
    eventually("A is told another took control", || {
        seen(&a) == (Phase::Busy, Some("takenOver".into()))
    });
    // …and it stays said over the lease's later turns, which find the station held.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(seen(&a), (Phase::Busy, Some("takenOver".into())));
    assert!(a.lease_held().is_none());

    a.take_control();
    eventually("A holds control again", || {
        a.lease_held().is_some() && seen(&a) == (Phase::Live, None)
    });
    eventually("B is told another took control", || {
        seen(&b) == (Phase::Busy, Some("takenOver".into()))
    });

    for (client, driving, dir) in [(a, a_driving, a_dir), (b, b_driving, b_dir)] {
        driving.abort();
        tauri::async_runtime::block_on(client.let_go());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// ★ Operate at an address the operator typed: with the station's one remembered address dead and
/// nothing found by name, the launch's typed address (`operate_args`, read back by `typed_in`) is
/// where client mode drives the station, which proves the key pinned for its pairing there. A
/// station at the typed address that proves a different key is refused as one whose key changed,
/// and nothing runs there: an address typed is never an identity. CONTROL: the same typed address
/// with the key pinned for the station is driven, at that address.
/// Skipped, saying so, on a box with no private address of its own.
#[test]
fn operate_at_a_typed_address_proves_the_pinned_key() {
    use crate::client_mode::{operate_args, typed_in};
    use crate::lan_client::tests::where_nothing_listens;
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let app = App::new();
    let (s, live, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let live = behind_a_port_on(&s, private, stop).await;
        (s, live, stopper)
    });
    let dead = where_nothing_listens(private);
    let typed = typed_in(
        operate_args(STATION.into(), Some(tempo_stream::lan::Typed::At(live))).into_iter(),
    );
    for pinned_ours in [false, true] {
        let mut record = paired_record(&s, live);
        if !pinned_ours {
            record.station_key = super::computer::another_key();
        }
        let dir = scratch("typed");
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let screens = screens(&client);
        let (device, key, pkcs8) = (record.device_id, record.station_key, record.pkcs8);
        let find: Find = Arc::new(move |_| {
            Ok(Some(PairedStation {
                station_id: STATION.into(),
                device_id: device.clone(),
                station_key: key.clone(),
                pkcs8: pkcs8.clone(),
                addresses: vec![dead.to_string()],
            }))
        });
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            Arc::new(|_| Ok(Vec::new())),
            typed.clone(),
        ));
        if pinned_ours {
            eventually("control taken at the typed address", || {
                client.status().phase == Phase::Live
            });
            assert_eq!(client.status().station, live.to_string());
        } else {
            eventually("the changed key told", || {
                client.status().reason.as_deref() == Some("keyChanged")
            });
            assert_ne!(client.status().phase, Phase::Live);
            assert_eq!(
                ask(&screens.main, "get_band_plan", json!({})),
                Err(json!("nexusClient:notLive"))
            );
            assert!(sent_here(&app).is_empty(), "{:?}", app.ran());
        }
        driving.abort();
        tauri::async_runtime::block_on(client.let_go());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// ★ Operate at a host name, through the launch as the app starts it (`start_driving`, from the
/// paired stations' store and the launch's arguments): the name is looked up when it is tried, and
/// the station there still proves the key pinned for its pairing, so a name that resolves to a
/// machine with another key is refused as one whose key changed, runs nothing, and is never kept.
/// Where the pinned key answers, Operate opens at the name's address and the name is kept for the
/// station, in the typed file outside the credential store, whose entry for the station is not
/// written (byte-identical, and the write count unmoved); the next launch, from the station's own Operate with nothing typed, tries
/// the kept name first and opens there. A name that resolves to nothing, with nothing answering
/// anywhere else, is told as `nameUnresolved`. The station's one remembered address is dead and
/// nothing is found by name throughout. Skipped, saying so, on a box with no private address.
#[test]
fn operate_at_a_typed_host_name_proves_the_pinned_key_and_is_tried_first_next_launch() {
    use crate::client_mode::{operate_args, start_driving, typed_in};
    use crate::lan_client::tests::{shack_pc_at, typed_file, where_nothing_listens, StationStore};
    use crate::lan_client::Stations;
    use tempo_stream::lan::Typed;
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let app = App::new();
    let (s, live, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let live = behind_a_port_on(&s, private, stop).await;
        (s, live, stopper)
    });
    let dead = where_nothing_listens(private);
    let name = |host: &str| Typed::Name {
        host: host.into(),
        port: live.port(),
    };
    // One launch of client mode: the store's record, the lookup, and the launch's arguments.
    let (typed_dir, file) = typed_file("operate-name");
    let stations =
        |store: &StationStore| Stations::new(Arc::new(store.clone())).typed_in(file.clone());
    let launch = |store: &StationStore, args: Vec<String>, label: &str| {
        let dir = scratch(label);
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let screens = screens(&client);
        client.look_up_with(shack_pc_at(&private.to_string()));
        let driving = start_driving(
            Arc::clone(&client),
            screens.main.app_handle().clone(),
            Arc::new(stations(store)),
            Arc::new(|_| Ok(Vec::new())),
            typed_in(args.into_iter().skip(1)),
        );
        (client, screens, driving, dir)
    };
    let end = |client: Arc<Client>, driving: tauri::async_runtime::JoinHandle<()>, dir| {
        driving.abort();
        tauri::async_runtime::block_on(client.let_go());
        std::fs::remove_dir_all(dir).unwrap();
    };
    let paired = |pinned_ours: bool| {
        let _ = std::fs::remove_file(&file);
        let store = StationStore::default();
        let mut record = paired_record(&s, dead);
        if !pinned_ours {
            record.station_key = super::computer::another_key();
        }
        stations(&store).keep(&record).unwrap();
        store
    };
    let kept = |store: &StationStore| stations(store).typed(STATION).map(|t| t.to_string());
    let entry = format!("lan-station-{STATION}");
    let typed = operate_args(STATION.into(), Some(name("shack-pc.local")));

    // A machine with another key at the name: refused, nothing run there, nothing kept.
    let store = paired(false);
    let (client, screens, driving, dir) = launch(&store, typed.clone(), "name-other-key");
    eventually("the changed key told", || {
        client.status().reason.as_deref() == Some("keyChanged")
    });
    assert_ne!(client.status().phase, Phase::Live);
    assert_eq!(
        ask(&screens.main, "get_band_plan", json!({})),
        Err(json!("nexusClient:notLive"))
    );
    assert!(sent_here(&app).is_empty(), "{:?}", app.ran());
    assert_eq!(kept(&store), None);
    end(client, driving, dir);

    // The pinned key at the name: driven there, and the name kept with the station.
    let store = paired(true);
    let (as_paired, writes) = (store.raw(&entry), store.writes.load(Ordering::SeqCst));
    let (client, _screens, driving, dir) = launch(&store, typed, "name-ours");
    eventually("control taken at the name's address", || {
        client.status().phase == Phase::Live
    });
    assert_eq!(client.status().station, live.to_string());
    eventually("the name kept with the station", || {
        kept(&store) == Some(format!("shack-pc.local:{}", live.port()))
    });
    assert_eq!(
        store.raw(&entry),
        as_paired,
        "the credential entry rewritten"
    );
    assert_eq!(
        store.writes.load(Ordering::SeqCst),
        writes,
        "a credential entry written"
    );
    end(client, driving, dir);

    // Started again from the station's own Operate, nothing typed: the kept name first.
    let (client, _screens, driving, dir) =
        launch(&store, operate_args(STATION.into(), None), "name-kept");
    eventually("control taken at the kept name", || {
        client.status().phase == Phase::Live
    });
    assert_eq!(client.status().station, live.to_string());
    end(client, driving, dir);

    // A name whose address does not answer, with the station at its remembered address: driven
    // there, and the name, which did not open Operate, is not kept.
    let _ = std::fs::remove_file(&file);
    let store = StationStore::default();
    stations(&store).keep(&paired_record(&s, live)).unwrap();
    let elsewhere = Typed::Name {
        host: "shack-pc.local".into(),
        port: dead.port(),
    };
    let args = operate_args(STATION.into(), Some(elsewhere));
    let (client, _screens, driving, dir) = launch(&store, args, "name-not-there");
    eventually("control taken at the remembered address", || {
        client.status().phase == Phase::Live
    });
    assert_eq!(client.status().station, live.to_string());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(kept(&store), None);
    end(client, driving, dir);

    // A name that resolves to nothing, and nothing answering anywhere else: the name is told.
    let store = paired(true);
    let nowhere = operate_args(STATION.into(), Some(name("nowhere.local")));
    let (client, _screens, driving, dir) = launch(&store, nowhere, "name-unresolved");
    eventually("the unresolved name told", || {
        client.status().reason.as_deref() == Some("nameUnresolved")
    });
    assert_ne!(client.status().phase, Phase::Live);
    assert_eq!(kept(&store), None);
    end(client, driving, dir);
    let _ = std::fs::remove_dir_all(typed_dir);
}
