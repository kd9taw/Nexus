//! One rule for every device (the operator's decision of 2026-10-10): a device approved once at the
//! shack operates whenever Remote is on, and "End remote control" drops everyone and holds until
//! Remote is turned on again, with no device approved again after it.
//!
//! - A computer approved here for the room's tunnel holds station control as a computer paired on
//!   the LAN does: while hosted Remote is on, under the shack's own name for its key
//!   (`lan::tunnel_device_id`), never the service's id; removed here, it loses it at once.
//! - End remote control turns hosted Remote off (every browser and every tunnel session out) and
//!   ends Remote over this network; turning Remote on again gives each approved device back what
//!   it had, browsers included.
use super::lan_grants::{lan_acquire, lan_store, with_lan};
use super::*;
use crate::remote_service::lan::tests::Scratch;

/// A computer listed waiting, then approved here as the operator was shown it: the name the room's
/// tunnel takes it as.
async fn approved_computer(service: &Service, cloud: &FakeCloud, key: &DeviceKey) -> String {
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(service, COMPUTER, key).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    let named = approved
        .paired(&key.pin())
        .expect("premise: the tunnel takes the computer approved here");
    assert_eq!(
        named,
        lan::tunnel_device_id(&key.pin()),
        "premise: its name"
    );
    named
}

/// ★ A computer approved here for the room's tunnel holds station control while hosted Remote is
/// on, as a LAN computer does, under its own name and never the service's id: it can take the
/// lease. Turn off takes it away, and Turn on gives it back with no approval again. It holds
/// nothing on the relay's lanes.
#[tokio::test(flavor = "multi_thread")]
async fn a_tunnel_computer_holds_station_control_whenever_remote_is_on() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = approved_computer(&service, &cloud, &key).await;
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "an approved computer through the room holds no station control"
    );
    let status = service.status().unwrap();
    assert!(status.station_permissions.contains(&named), "shown held");
    assert!(
        !status.station_permissions.contains(&COMPUTER.to_string()),
        "held under the service's id"
    );

    service.action(Action::Disable {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "held while Remote is off"
    );
    settle().await;
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "came back by itself while Remote is off"
    );

    service.action(Action::Enable {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "Turn on did not give it back"
    );
}

/// A station's control with the service listing each of `rows` (its id and the fingerprint of its
/// key) as an approved computer, pinned here at the approval listed.
fn tunnel_control(rows: &[(&str, &str)]) -> Control {
    let mut control = Control::default();
    for (id, key) in rows {
        let pin = format!("pin-{id}");
        control.natives.insert(
            id.to_string(),
            Native {
                pin: pin.clone(),
                key: key.to_string(),
                until: u64::MAX,
            },
        );
        control.remembered.pins.insert(id.to_string(), pin);
    }
    control
}

/// What the hold says of `name` each time the tunnel's list changes: whether it is on the list, and
/// whether it is held at that moment.
fn watch_hold(control: &Control, name: &str) -> Arc<Mutex<Vec<(bool, bool)>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (kept, operations, name) = (seen.clone(), control.operations.clone(), name.to_string());
    *control.approved.on_set.lock().unwrap() = Some(Box::new(move |names: &[String]| {
        let held = operations.tx_held(&name, Instant::now());
        kept.lock().unwrap().push((names.contains(&name), held));
    }));
    seen
}

/// ★ A COMPUTER JOINING OR LEAVING THE TUNNEL IS NEVER ONE THE HOLD CANNOT PLACE: the moment its name
/// joins the tunnel's list, the hold already reads it as the service's id for it (here a computer
/// approved at the shack, and so free); the moment it leaves, removed or approved again at the
/// service, and before its station control is taken back, its name is held.
#[test]
fn a_computer_joining_or_leaving_the_tunnel_is_never_one_the_hold_cannot_place() {
    let key = "ab".repeat(32);
    let name = lan::tunnel_device_id(&[0xab; 32]);
    for leaves in ["removed", "approved again"] {
        let mut control = tunnel_control(&[(COMPUTER, &key)]);
        control
            .operations
            .set_signin_devices(std::collections::BTreeSet::new());
        let seen = watch_hold(&control, &name);
        control.sync_approved();
        match leaves {
            "removed" => control.natives.clear(),
            _ => control.natives.get_mut(COMPUTER).unwrap().pin = "pin-again".into(),
        }
        control.sync_approved();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "{leaves}: premise: it joined and left");
        assert_eq!(
            seen[0],
            (true, false),
            "{leaves}: held the moment it joined: the hold was told its service id after the list"
        );
        assert_eq!(
            seen[1],
            (false, true),
            "{leaves}: free the moment it left, before its station control was taken back"
        );
    }
}

/// ★ TWO LISTED COMPUTERS WITH ONE KEY: the tunnel takes both under one name, and the hold reads that
/// name as held while the one added by its own sign-in is, whichever id sorts first. The one approved
/// at the shack never frees it; only a waiver for the sign-in one does.
#[test]
fn a_tunnel_name_two_listed_computers_share_is_held_while_the_sign_in_one_is() {
    const FIRST: &str = "10000000-0000-4000-8000-0000000000e1";
    const LAST: &str = "f0000000-0000-4000-8000-0000000000e2";
    let key = "ab".repeat(32);
    let name = lan::tunnel_device_id(&[0xab; 32]);
    for (signed, shack) in [(FIRST, LAST), (LAST, FIRST)] {
        let control = tunnel_control(&[(signed, &key), (shack, &key)]);
        control
            .operations
            .set_signin_devices(std::collections::BTreeSet::from([signed.to_string()]));
        control.sync_approved();
        let now = Instant::now();
        assert!(
            control.operations.tx_held(&name, now),
            "freed by the row added at the shack (the sign-in one {signed})"
        );
        control.operations.waive_transmit(signed);
        assert!(
            !control.operations.tx_held(&name, now),
            "CONTROL: the sign-in one's waiver frees it (the sign-in one {signed})"
        );
    }
}

/// A computer approved here, the service listing it as added at the shack, is never held from
/// transmit on the room's tunnel: its tunnel name stands for a device the list names as not
/// signed in, as its service id does.
#[tokio::test(flavor = "multi_thread")]
async fn a_tunnel_computer_approved_here_is_not_held_from_transmit() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = approved_computer(&service, &cloud, &key).await;
    service.action(Action::Refresh {}).await.unwrap();
    let operations = service.control.lock().unwrap().operations.clone();
    let past = Instant::now() + account::TX_HOLD + Duration::from_secs(60);
    assert!(
        !operations.tx_held(COMPUTER, past),
        "premise: its service id is free"
    );
    assert!(!operations.tx_held(&named, Instant::now()), "held");
}

/// ★ Removed here, a tunnel computer loses station control at once, before the next request; a
/// computer still waiting, one the service lists with a key other than the one pinned, and one
/// never approved here are never held.
#[tokio::test(flavor = "multi_thread")]
async fn a_tunnel_computer_removed_or_never_approved_here_holds_nothing() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = lan::tunnel_device_id(&key.pin());
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "a computer waiting for approval"
    );
    // Listed approved by the service, but never approved here: no pin, so nothing.
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "a computer the service approved and the shack never did"
    );

    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    let named = approved_computer(&service, &cloud, &key).await;
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "premise"
    );
    // The service lists another key for it: not the pin, so out at once.
    let other = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &other)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "a listing with another key left it control"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &lan::tunnel_device_id(&other.pin())),
        "localPermissionRequired",
        "the key never pinned"
    );
    // Back, then removed here: out at once.
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "control: back"
    );
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert!(
        !service
            .status()
            .unwrap()
            .station_permissions
            .contains(&named),
        "removed, still shown held"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "removed here, it kept station control"
    );
}

/// ★ A tunnel computer holding the lease loses it the moment it is removed here: its control is
/// revoked, not only refused to its next acquire.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_tunnel_computer_ends_the_lease_it_holds() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = approved_computer(&service, &cloud, &key).await;
    let authority = service.control.lock().unwrap().operations.clone();
    let connection = operations::LanConnection::new(authority.clone());
    let session = lan_grants::fresh_id();
    let ask = |request: operations::Request| {
        authority.handle_version(
            (connection.id, 4),
            &session,
            &named,
            &request,
            &engine,
            Instant::now(),
        )
    };
    let boot = ask(operations::Request::State {
        request_id: lan_grants::fresh_id(),
    })
    .unwrap()["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    let acquired = ask(operations::Request::Acquire {
        request_id: lan_grants::fresh_id(),
        station_boot_id: boot,
    })
    .unwrap();
    assert_eq!(acquired["phase"], "controlling", "premise");
    assert!(authority.lan_controls(connection.id, &named, Instant::now()));
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert!(
        !authority.lan_controls(connection.id, &named, Instant::now()),
        "removed here, it still held the lease"
    );
}

/// ★ End remote control drops everyone, browser, tunnel computer and LAN computer alike: hosted
/// Remote goes off (and stays off across a restart), Remote over this network ends, and nobody
/// holds anything. Turning Remote on again gives every approved device back what it had with no
/// approval again: the browser its control, the tunnel computer its
/// control; the LAN computer comes back with its own switch, as before. The TX-enable latch is off
/// throughout.
#[tokio::test(flavor = "multi_thread")]
async fn end_remote_control_drops_everyone_and_holds_until_remote_is_turned_on_again() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = engine();
    let store = lan_store();
    let scratch = Scratch::new();
    let service = with_lan(launch(&cloud, &vault, &engine), &engine, &store, &scratch);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
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
    let lan_computer = status.lan.unwrap().devices[0].id.clone();
    service.action(Action::Enable {}).await.unwrap();
    pending_browser(&cloud);
    service.action(Action::Refresh {}).await.unwrap();
    let granted = approve(&service).await;
    assert!(
        granted.station_permissions.contains(&BROWSER.to_string()),
        "premise: the browser"
    );
    // The computer beside the browser in the service's list.
    let key = DeviceKey::new();
    let browser_listed: serde_json::Value =
        serde_json::from_str::<serde_json::Value>(&cloud.devices.lock().unwrap().clone()).unwrap()
            ["devices"][0]
            .clone();
    *cloud.devices.lock().unwrap() =
        json!({ "devices": [browser_listed.clone(), computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(&service, COMPUTER, &key).await.unwrap();
    let named = lan::tunnel_device_id(&key.pin());
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "premise: the computer"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &lan_computer),
        "controlling",
        "premise: the LAN computer"
    );
    let browser_held = |s: &Status| s.station_permissions.contains(&BROWSER.to_string());
    assert!(
        browser_held(&service.status().unwrap()),
        "premise: the browser's grants"
    );

    let ended = service.action(Action::TakeOverLogging {}).await.unwrap();
    assert!(!on(&ended), "End remote control left hosted Remote on");
    assert!(
        ended.station_permissions.is_empty(),
        "End remote control left a grant: {}",
        serde_json::to_string(&ended).unwrap()
    );
    let lan = ended.lan.clone().unwrap();
    assert!(
        !lan.on && lan.reason == Some("endedAtShack"),
        "LAN not ended"
    );
    for (who, device) in [
        ("browser", BROWSER),
        ("computer", &named),
        ("LAN computer", &lan_computer),
    ] {
        assert_eq!(
            lan_acquire(&service, &engine, device),
            "localPermissionRequired",
            "the {who} kept station control"
        );
    }
    assert!(transmitter_idle(&engine));
    // It holds: across a wait and a restart, nobody is back and Remote stays off.
    settle().await;
    drop(service);
    let service = with_lan(launch(&cloud, &vault, &engine), &engine, &store, &scratch);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    settle().await;
    let held = service.status().unwrap();
    assert!(
        !on(&held),
        "Remote came back on by itself after End remote control"
    );
    assert!(
        held.station_permissions.is_empty(),
        "a grant came back by itself"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "the computer came back by itself"
    );

    // Turned on again: everyone approved is back, with no approval again.
    service.action(Action::Enable {}).await.unwrap();
    let back = eventually(&service, "the browser's grants came back", browser_held).await;
    assert!(on(&back), "Remote is on");
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "the computer needed approving again"
    );
    service
        .action(Action::LanOn {
            address: None,
            port: None,
        })
        .await
        .unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &lan_computer),
        "controlling",
        "the LAN computer"
    );
    assert!(transmitter_idle(&engine), "the latch is still off");
}

/// The service tells a Nexus that asks for kinds which approvals last until removed, and this
/// station shows so; a service from before says nothing of it, and the device is read as before.
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_until_removed_is_read_and_shown_and_a_service_without_it_still_lists() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let mut listed = computer_listed(1, &key);
    listed["untilRemoved"] = 1.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [listed] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    let shown = serde_json::to_value(&status.devices).unwrap();
    assert_eq!(shown[0]["untilRemoved"], 1, "{shown}");
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    let shown = serde_json::to_value(&status.devices).unwrap();
    assert_eq!(shown[0]["untilRemoved"], 0, "{shown}");
}
