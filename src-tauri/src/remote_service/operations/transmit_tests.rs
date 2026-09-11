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
