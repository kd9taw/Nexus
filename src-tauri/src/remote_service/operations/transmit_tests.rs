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
