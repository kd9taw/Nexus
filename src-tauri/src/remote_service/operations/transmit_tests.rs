//! Exercise the real lease/heartbeat authority around native FT ownership.
//! The test enters CQ directly; no browser transmit command exists yet.
use super::*;

fn armed() -> (Fixture, Instant, Value) {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        e.take_slot_tx_abort();
    }
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 3);
    f.authority.permit_transmit(DEVICE, true).unwrap();
    let permit = f.authority.transmit.permit(now + LEASE).unwrap();
    f.engine
        .lock()
        .unwrap()
        .start_remote_ft_cq(permit, None)
        .unwrap();
    (f, now, state)
}

fn heartbeat(
    f: &Fixture,
    state: &Value,
    session: &str,
    now: Instant,
) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 3),
        session,
        DEVICE,
        &Request::Heartbeat {
            request_id: id(),
            lease_id: state["leaseId"].as_str().unwrap().into(),
        },
        &f.engine,
        now,
    )
}

#[test]
fn transmit_permission_is_separate_boot_scoped_and_never_arms() {
    let f = Fixture::new();
    assert_eq!(
        f.authority.permit_transmit(DEVICE, true),
        Err("localPermissionRequired")
    );
    f.authority.permit_station(DEVICE, true).unwrap();
    assert_eq!(f.authority.local_status()["transmitDevices"], json!([]));
    f.authority.permit_transmit(DEVICE, true).unwrap();
    assert_eq!(
        f.authority.local_status()["transmitDevices"],
        json!([DEVICE])
    );
    assert!(!f.engine.lock().unwrap().tx_enabled());
    f.authority.invalidate();
    assert_eq!(f.authority.local_status()["transmitDevices"], json!([]));
    assert_eq!(
        f.authority.permit_transmit(DEVICE, true),
        Err("localPermissionRequired")
    );
}

#[test]
fn transmit_heartbeat_renews_only_the_current_live_owner() {
    let (f, now, state) = armed();
    assert_eq!(
        heartbeat(&f, &state, OTHER, now + Duration::from_secs(1)),
        Err("notController")
    );
    heartbeat(&f, &state, SESSION, now + Duration::from_secs(2)).unwrap();
    let mut e = f.engine.lock().unwrap();
    assert!(!e.poll_remote_transmit(now + LEASE));
    assert!(e.tx_enabled());
    assert!(e.poll_remote_transmit(now + LEASE + Duration::from_secs(2)));
    assert!(!e.tx_enabled());
    drop(e);
    assert_eq!(
        heartbeat(&f, &state, SESSION, now + LEASE + Duration::from_secs(2)),
        Err("leaseExpired")
    );
}

#[test]
fn transmit_revocation_is_synchronous_even_while_engine_is_busy() {
    for cause in [
        "grant",
        "stationGrant",
        "loggingGrant",
        "disconnect",
        "connection",
        "invalidate",
        "retire",
    ] {
        let (f, _, _) = armed();
        // These paths must revoke without waiting on Engine. This lock models
        // an in-flight modem or other native operation.
        let mut e = f.engine.lock().unwrap();
        match cause {
            "grant" => f.authority.permit_transmit(DEVICE, false).unwrap(),
            "stationGrant" => f.authority.permit_station(DEVICE, false).unwrap(),
            "loggingGrant" => f.authority.permit(DEVICE, false).unwrap(),
            "disconnect" => f.authority.disconnect_session(SESSION),
            "connection" => {
                f.authority.start_connection();
            }
            "invalidate" => f.authority.invalidate(),
            "retire" => f.authority.retire_connection(f.connection),
            _ => unreachable!(),
        }
        assert!(e.poll_remote_transmit(Instant::now()), "{cause}");
        assert!(!e.tx_enabled(), "{cause}");
        assert!(e.take_slot_tx_abort(), "{cause}");
    }
}

#[test]
fn transmit_regrant_and_heartbeat_cannot_revive_old_tx_or_own_native_tx() {
    for local in [false, true] {
        let (f, now, state) = armed();
        if local {
            f.engine.lock().unwrap().set_tx_enabled(true);
        }
        f.authority.permit_transmit(DEVICE, false).unwrap();
        f.authority.permit_transmit(DEVICE, true).unwrap();
        heartbeat(&f, &state, SESSION, now + Duration::from_secs(1)).unwrap();
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.tx_enabled(), local);
        assert!(!e.poll_remote_transmit(now + Duration::from_secs(20)));
        assert_eq!(e.tx_enabled(), local);
    }
}

#[test]
fn transmit_release_ends_the_permit_and_other_device_revoke_does_not() {
    let (f, now, state) = armed();
    f.authority.permit_transmit(OTHER, false).unwrap();
    assert!(!f.engine.lock().unwrap().poll_remote_transmit(now));
    f.authority
        .handle_version(
            (f.connection, 3),
            SESSION,
            DEVICE,
            &Request::Release {
                request_id: id(),
                lease_id: state["leaseId"].as_str().unwrap().into(),
            },
            &f.engine,
            now,
        )
        .unwrap();
    assert!(f.engine.lock().unwrap().poll_remote_transmit(now));
}

fn stop_request(f: &Fixture, state: &Value) -> Request {
    Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: format!("{:016x}", f.authority.transmit.generation()),
    }
}

#[test]
fn transmit_stop_does_not_wait_for_engine_or_a_pending_file_operation() {
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    let mut engine = f.engine.lock().unwrap();
    let _pending_file_operation = f.authority.core.lock().unwrap();
    let result = f
        .authority
        .handle_version((f.connection, 4), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(result, json!({"stop":"accepted"}));
    assert!(engine.poll_remote_transmit(now));
    assert!(!engine.tx_enabled());
    assert!(engine.take_slot_tx_abort());
}

#[test]
fn transmit_stop_rejects_a_replay_after_a_new_remote_arm() {
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    f.authority
        .stop_transmit(f.connection, SESSION, DEVICE, &request, now)
        .unwrap();
    {
        let mut e = f.engine.lock().unwrap();
        assert!(e.poll_remote_transmit(now));
        e.take_immediate_retune();
        e.start_remote_ft_cq(f.authority.transmit.permit(now + LEASE).unwrap(), None)
            .unwrap();
    }
    assert_eq!(
        f.authority
            .stop_transmit(f.connection, SESSION, DEVICE, &request, now),
        Err("staleContext")
    );
    assert!(!f.engine.lock().unwrap().poll_remote_transmit(now));
    assert!(f.engine.lock().unwrap().tx_enabled());
    f.authority
        .stop_transmit(
            f.connection,
            SESSION,
            DEVICE,
            &stop_request(&f, &state),
            now,
        )
        .unwrap();
    assert!(f.engine.lock().unwrap().poll_remote_transmit(now));
}

#[test]
fn transmit_stop_cannot_stop_a_new_local_transmission() {
    let (f, now, state) = armed();
    let request = stop_request(&f, &state);
    let mut e = f.engine.lock().unwrap();
    e.set_tx_enabled(true);
    f.authority
        .stop_transmit(f.connection, SESSION, DEVICE, &request, now)
        .unwrap();
    assert!(!e.poll_remote_transmit(now));
    assert!(e.tx_enabled());
}

#[test]
fn transmit_stop_requires_the_exact_granted_live_lease_and_new_protocol() {
    for scene in [
        "version",
        "device",
        "session",
        "lease",
        "boot",
        "grant",
        "expiry",
        "connection",
    ] {
        let (f, now, state) = armed();
        let mut request = stop_request(&f, &state);
        let mut session = SESSION;
        let mut device = DEVICE;
        let mut version = 4;
        let mut at = now;
        let expected = match scene {
            "version" => {
                version = 3;
                "stationUnsupported"
            }
            "device" => {
                device = OTHER;
                "notController"
            }
            "session" => {
                session = OTHER;
                "notController"
            }
            "lease" => {
                if let Request::StopTransmit { lease_id, .. } = &mut request {
                    *lease_id = id();
                }
                "notController"
            }
            "boot" => {
                if let Request::StopTransmit {
                    station_boot_id, ..
                } = &mut request
                {
                    *station_boot_id = id();
                }
                "staleStation"
            }
            "grant" => {
                f.authority.permit_transmit(DEVICE, false).unwrap();
                "localPermissionRequired"
            }
            "expiry" => {
                at += LEASE;
                "leaseExpired"
            }
            "connection" => {
                f.authority.start_connection();
                "staleConnection"
            }
            _ => unreachable!(),
        };
        assert_eq!(
            f.authority.handle_version(
                (f.connection, version),
                session,
                device,
                &request,
                &f.engine,
                at
            ),
            Err(expected),
            "{scene}"
        );
        if !matches!(scene, "grant" | "connection" | "expiry") {
            assert!(
                !f.engine.lock().unwrap().poll_remote_transmit(now),
                "{scene}"
            );
            assert!(f.engine.lock().unwrap().tx_enabled(), "{scene}");
        }
    }
}

#[test]
fn transmit_stop_token_is_only_returned_to_the_granted_v4_owner() {
    let (f, now, _) = armed();
    for version in 1..=4 {
        let value = f
            .authority
            .handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                &Request::State { request_id: id() },
                &f.engine,
                now,
            )
            .unwrap();
        if version < 4 {
            assert!(value.get("transmitEpoch").is_none());
        } else {
            assert_eq!(
                value["transmitEpoch"],
                format!("{:016x}", f.authority.transmit.generation())
            );
        }
    }
    let value = f
        .authority
        .handle_version(
            (f.connection, 4),
            OTHER,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap();
    assert!(value["transmitEpoch"].is_null());
    f.authority.permit_transmit(DEVICE, false).unwrap();
    let value = f
        .authority
        .handle_version(
            (f.connection, 4),
            SESSION,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap();
    assert!(value["transmitEpoch"].is_null());
}

fn ready_ft(tier: tempo_app::dto::Tier) -> (Fixture, Instant, Value) {
    ready_ft_link(tier, true)
}

fn ready_ft_link(tier: tempo_app::dto::Tier, connected: bool) -> (Fixture, Instant, Value) {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.set_tier(tier);
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        e.take_slot_tx_abort();
        let radio = e.remote_open_radio().unwrap();
        let read = e.remote_radio_read(&radio, Instant::now());
        let hz = (e.settings().dial_mhz * 1e6).round() as u64;
        e.remote_observe_cat(read.as_ref(), Some(connected));
        e.remote_observe_dial(read.as_ref(), Some(hz));
        e.remote_observe_mode(read.as_ref(), Some("PKTUSB"));
        e.remote_observe_ptt(read.as_ref(), Some(false));
    }
    let now = Instant::now();
    acquire_controls_version(&f, now, 4);
    f.authority.permit_transmit(DEVICE, true).unwrap();
    let state = control_state_version(&f, now, 4);
    (f, now, state)
}
fn ft_command(state: &Value, action: Value) -> Request {
    serde_json::from_value(json!({"type":"stationControl","requestId":id(),"stationBootId":state["stationBootId"],
        "leaseId":state["leaseId"],"expectedRevision":state["revision"],"commandWindowId":state["commandWindowId"],
        "clientSequence":state["nextSequence"],"context":state["controls"]["context"],"action":action})).unwrap()
}
fn ft_run(f: &Fixture, request: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 4),
        SESSION,
        DEVICE,
        request,
        &f.engine,
        Instant::now(),
    )
}

#[test]
fn transmit_browser_cq_tx_off_and_stop_preserve_native_ft_behavior() {
    use tempo_app::dto::Tier;
    for tier in [Tier::Ft8, Tier::Ft4] {
        let (f, _, state) = ready_ft(tier);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("ftOperate")));
        let command = ft_command(
            &state,
            json!({"action":"ft.cq","expectedTier":tier,"transmitEpoch":state["transmitEpoch"],"direction":"DX"}),
        );
        assert_eq!(ft_run(&f, &command).unwrap()["evidence"], "stationState");
        assert!(f.engine.lock().unwrap().tx_enabled());
        let armed = control_state_version(&f, Instant::now(), 4);
        assert_eq!(armed["txArmed"], true);
        let off = ft_command(
            &armed,
            json!({"action":"ft.txEnabled","expectedTier":tier,"transmitEpoch":armed["transmitEpoch"],"on":false}),
        );
        assert_eq!(ft_run(&f, &off).unwrap()["outcome"], "applied");
        assert!(!f.engine.lock().unwrap().take_slot_tx_abort());
        assert!(!f.engine.lock().unwrap().tx_enabled());
        let stop = stop_request(&f, &state);
        assert_eq!(ft_run(&f, &stop).unwrap(), json!({"stop":"accepted"}));
        assert!(f
            .engine
            .lock()
            .unwrap()
            .poll_remote_transmit(Instant::now()));
        assert!(f.engine.lock().unwrap().take_slot_tx_abort());
    }
}

#[test]
fn transmit_stop_invalidates_an_unsent_arm_even_if_its_command_window_still_matches() {
    let (f, _, state) = ready_ft(tempo_app::dto::Tier::Ft8);
    let command = ft_command(
        &state,
        json!({"action":"ft.cq","expectedTier":"FT8","transmitEpoch":state["transmitEpoch"],"direction":null}),
    );
    assert_eq!(
        ft_run(&f, &stop_request(&f, &state)).unwrap(),
        json!({"stop":"accepted"})
    );
    assert_eq!(ft_run(&f, &command), Err("staleContext"));
    assert!(!f.engine.lock().unwrap().tx_enabled());
    let fresh = control_state_version(&f, Instant::now(), 4);
    let command = ft_command(
        &fresh,
        json!({"action":"ft.cq","expectedTier":"FT8","transmitEpoch":fresh["transmitEpoch"],"direction":null}),
    );
    assert_eq!(ft_run(&f, &command).unwrap()["outcome"], "applied");
    assert!(f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn transmit_browser_arm_requires_its_own_grant_and_fresh_radio_context() {
    for cause in [
        "permission",
        "link",
        "tier",
        "generation",
        "direction",
        "local",
    ] {
        let (f, _, state) = ready_ft_link(tempo_app::dto::Tier::Ft8, cause != "link");
        let mut action = json!({"action":"ft.cq","expectedTier":"FT8","transmitEpoch":state["transmitEpoch"],"direction":null});
        match cause {
            "permission" => f.authority.permit_transmit(DEVICE, false).unwrap(),
            "link" => {}
            "tier" => action["expectedTier"] = json!("FT4"),
            "generation" => action["transmitEpoch"] = json!("ffffffffffffffff"),
            "direction" => action["direction"] = json!("CQ DX W1AW"),
            "local" => f.engine.lock().unwrap().set_tx_enabled(true),
            _ => unreachable!(),
        }
        let result = ft_run(&f, &ft_command(&state, action));
        assert!(
            result.is_err() || result.unwrap()["outcome"] == "rejected",
            "{cause}"
        );
        assert_eq!(
            f.engine.lock().unwrap().tx_enabled(),
            cause == "local",
            "{cause}"
        );
    }
}
