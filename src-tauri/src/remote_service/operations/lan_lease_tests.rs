//! Both roads at once (the operator's ruling of 2026-10-04): the relay's connection and the LAN's
//! share one operations authority and one lease, and the lease is bound to the connection that took
//! it. Each test holds a transmission, or a stream's presence, under one road's lease and moves the
//! other road, then checks what the radio loop does on its next tick.
use super::*;

const LAN_DEVICE: &str = "40000000-0000-4000-8000-000000000001";
const LAN_SESSION: &str = "50000000-0000-4000-8000-000000000001";
const LAN_OTHER: &str = "40000000-0000-4000-8000-000000000002";
const LAN_OTHER_SESSION: &str = "50000000-0000-4000-8000-000000000002";

fn on(
    f: &Fixture,
    connection: u64,
    (session, device): (&str, &str),
    request: &Request,
    now: Instant,
) -> Result<Value, &'static str> {
    f.authority
        .handle_version((connection, 4), session, device, request, &f.engine, now)
}

fn state_on(f: &Fixture, connection: u64, who: (&str, &str), now: Instant) -> Value {
    on(
        f,
        connection,
        who,
        &Request::State { request_id: id() },
        now,
    )
    .unwrap()
}

/// Station control for `who`'s device, then its lease on `connection`.
fn control_on(
    f: &Fixture,
    connection: u64,
    who: (&str, &str),
    now: Instant,
) -> Result<Value, &'static str> {
    f.authority.permit_station(who.1, true).unwrap();
    let boot = state_on(f, connection, who, now)["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    on(
        f,
        connection,
        who,
        &Request::Acquire {
            request_id: id(),
            station_boot_id: boot,
        },
        now,
    )
}

fn heartbeat_on(
    f: &Fixture,
    connection: u64,
    who: (&str, &str),
    state: &Value,
    now: Instant,
) -> Result<Value, &'static str> {
    on(
        f,
        connection,
        who,
        &Request::Heartbeat {
            request_id: id(),
            lease_id: state["leaseId"].as_str().unwrap().into(),
        },
        now,
    )
}

/// An FT8 CQ on the air under `who`'s lease, with its transmit grant: what the radio loop halts on
/// its next tick once the authority that permitted it is revoked.
fn keyed_under(f: &Fixture, who: (&str, &str), now: Instant) {
    f.authority.permit_transmit(who.1, true).unwrap();
    let permit = f.authority.transmit.permit(now + LEASE).unwrap();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.take_slot_tx_abort();
    e.start_remote_ft_cq(permit, None).unwrap();
    assert!(e.tx_enabled(), "premise: on the air");
}

/// Did the radio loop halt on this tick?
fn halted(f: &Fixture, now: Instant) -> bool {
    let mut e = f.engine.lock().unwrap();
    e.poll_remote_transmit(now) || !e.tx_enabled()
}

/// The relay's road moves in each of the three ways it can without a word from the shack: its
/// socket is replaced, it ends, or a browser on it leaves.
const RELAY_EVENTS: [&str; 3] = ["reconnect", "retire", "browserLeft"];

fn relay_event(f: &Fixture, event: &str) {
    match event {
        "reconnect" => {
            f.authority.start_connection();
        }
        "retire" => f.authority.retire_connection(f.connection),
        "browserLeft" => f.authority.disconnect_session(SESSION),
        _ => unreachable!(),
    }
}

/// ★ Both roads at once: whatever happens on the relay's road, a LAN controller keeps its lease,
/// its transmission and its stream's presence. CONTROL, in the same loop: the same event still ends
/// the relay's own controller and halts its transmission, exactly as before.
#[test]
fn the_relays_road_moving_never_drops_a_lan_controller() {
    for event in RELAY_EVENTS {
        let f = Fixture::new();
        let now = Instant::now();
        let lan = f.authority.start_lan_connection();
        let who = (LAN_SESSION, LAN_DEVICE);
        let state = control_on(&f, lan, who, now).unwrap();
        assert_eq!(state["phase"], "controlling", "{event}: premise");
        keyed_under(&f, who, now);
        let lease = state["leaseId"].as_str().unwrap();
        let presence = f
            .authority
            .stream_presence(LAN_SESSION, LAN_DEVICE, lease, now)
            .unwrap();

        relay_event(&f, event);

        assert!(
            !halted(&f, now),
            "{event} on the relay's road halted the LAN controller's transmission"
        );
        assert!(
            presence.valid(now),
            "{event} on the relay's road ended the LAN stream's presence"
        );
        let later = now + Duration::from_secs(1);
        let renewed = heartbeat_on(&f, lan, who, &state, later);
        assert_eq!(
            renewed.as_ref().map(|s| s["phase"].clone()),
            Ok(json!("controlling")),
            "{event} on the relay's road dropped the LAN controller's lease: {renewed:?}"
        );

        // CONTROL: the same event, with the relay's own browser in control, ends it as it always
        // has.
        let g = Fixture::new();
        let relay_state = control_on(&g, g.connection, (SESSION, DEVICE), now).unwrap();
        keyed_under(&g, (SESSION, DEVICE), now);
        relay_event(&g, event);
        assert!(
            halted(&g, now),
            "control: {event} did not halt the relay's own controller"
        );
        assert!(
            heartbeat_on(&g, g.connection, (SESSION, DEVICE), &relay_state, later).is_err(),
            "control: {event} left the relay's own controller its lease"
        );
    }
}

/// ★ Both roads at once: a LAN connection that closes takes only its own lease. The relay's
/// controller and another LAN computer's keep theirs and stay on the air; CONTROL: the controller's
/// own close ends its lease and halts its transmission on the next tick, and the station is free
/// for someone else.
#[test]
fn a_lan_connection_that_closes_drops_only_its_own_lease() {
    let now = Instant::now();
    let later = now + Duration::from_secs(1);

    // The relay's browser in control; a LAN computer that holds nothing leaves.
    let f = Fixture::new();
    let state = control_on(&f, f.connection, (SESSION, DEVICE), now).unwrap();
    keyed_under(&f, (SESSION, DEVICE), now);
    let idle = f.authority.start_lan_connection();
    f.authority.retire_lan_connection(idle);
    assert!(
        !halted(&f, now),
        "a LAN close halted the relay's controller"
    );
    assert_eq!(
        heartbeat_on(&f, f.connection, (SESSION, DEVICE), &state, later)
            .map(|s| s["phase"].clone()),
        Ok(json!("controlling")),
        "a LAN close dropped the relay's controller"
    );

    // One LAN computer in control; another LAN computer leaves.
    let f = Fixture::new();
    let first = f.authority.start_lan_connection();
    let second = f.authority.start_lan_connection();
    let who = (LAN_SESSION, LAN_DEVICE);
    let state = control_on(&f, first, who, now).unwrap();
    keyed_under(&f, who, now);
    f.authority.retire_lan_connection(second);
    assert!(
        !halted(&f, now),
        "another LAN computer's close halted the controller"
    );
    assert_eq!(
        heartbeat_on(&f, first, who, &state, later).map(|s| s["phase"].clone()),
        Ok(json!("controlling")),
        "another LAN computer's close dropped the controller"
    );

    // CONTROL: the controller's own connection closes.
    f.authority.retire_lan_connection(first);
    assert!(
        halted(&f, now),
        "the controller's own close left it on the air"
    );
    assert!(heartbeat_on(&f, first, who, &state, later).is_err());
    let freed = control_on(&f, f.connection, (SESSION, DEVICE), later).unwrap();
    assert_eq!(
        freed["phase"], "controlling",
        "the station stayed held after its controller left"
    );
}

/// ★ Both roads at once: a stream's presence on the LAN road ends with its connection, at once, and
/// nothing a heartbeat or a presence renewal sends afterwards brings it back.
#[test]
fn a_lan_close_ends_its_presence_at_once_and_nothing_renews_it() {
    let f = Fixture::new();
    let now = Instant::now();
    let lan = f.authority.start_lan_connection();
    let who = (LAN_SESSION, LAN_DEVICE);
    let state = control_on(&f, lan, who, now).unwrap();
    let lease = state["leaseId"].as_str().unwrap().to_string();
    let presence = f
        .authority
        .stream_presence(LAN_SESSION, LAN_DEVICE, &lease, now)
        .unwrap();
    assert!(presence.valid(now), "premise: presence");
    f.authority.retire_lan_connection(lan);
    assert!(!presence.valid(now), "presence outlived its connection");
    assert!(
        f.authority
            .stream_presence(LAN_SESSION, LAN_DEVICE, &lease, now)
            .is_err(),
        "a closed connection's lease minted presence"
    );
    assert_eq!(
        heartbeat_on(&f, lan, who, &state, now),
        Err("staleConnection"),
        "a closed connection was answered"
    );
}

/// ★ Both roads at once: `invalidate`, the shack's "End remote control", drops the lease on either
/// road and halts what it held. CONTROL: before it, both controllers were live.
#[test]
fn invalidate_drops_every_lease_on_either_road() {
    let now = Instant::now();
    for road in ["relay", "lan"] {
        let f = Fixture::new();
        let (connection, who) = if road == "lan" {
            (
                f.authority.start_lan_connection(),
                (LAN_SESSION, LAN_DEVICE),
            )
        } else {
            (f.connection, (SESSION, DEVICE))
        };
        let state = control_on(&f, connection, who, now).unwrap();
        keyed_under(&f, who, now);
        assert!(!halted(&f, now), "{road}: premise, on the air");
        f.authority.invalidate();
        assert!(
            halted(&f, now),
            "{road}: invalidate left the transmission up"
        );
        assert!(heartbeat_on(&f, connection, who, &state, now).is_err());
        assert_eq!(
            state_on(&f, connection, who, now)["phase"],
            "localPermissionRequired",
            "{road}: invalidate left the grant"
        );
    }
}

/// ★ Both roads at once: one controller for the whole station. Whoever asks second, on either road,
/// is told `controllerBusy`; CONTROL: once the first releases, the second takes it.
#[test]
fn a_second_controller_on_either_road_is_busy() {
    let now = Instant::now();
    for (first, second) in [("relay", "lan"), ("lan", "relay"), ("lan", "lan")] {
        let f = Fixture::new();
        let lan_a = f.authority.start_lan_connection();
        let lan_b = f.authority.start_lan_connection();
        let pick = |road: &str, n: u8| match (road, n) {
            ("relay", _) => (f.connection, (SESSION, DEVICE)),
            ("lan", 0) => (lan_a, (LAN_SESSION, LAN_DEVICE)),
            _ => (lan_b, (LAN_OTHER_SESSION, LAN_OTHER)),
        };
        let (c1, w1) = pick(first, 0);
        let (c2, w2) = pick(second, 1);
        let held = control_on(&f, c1, w1, now).unwrap();
        assert_eq!(held["phase"], "controlling");
        assert_eq!(
            control_on(&f, c2, w2, now),
            Err("controllerBusy"),
            "{first} then {second}"
        );
        // CONTROL: released, the station is the second's.
        on(
            &f,
            c1,
            w1,
            &Request::Release {
                request_id: id(),
                lease_id: held["leaseId"].as_str().unwrap().into(),
            },
            now,
        )
        .unwrap();
        assert_eq!(
            control_on(&f, c2, w2, now).map(|s| s["phase"].clone()),
            Ok(json!("controlling")),
            "{first} then {second}: not freed"
        );
    }
}

/// ★ Stop on the LAN road: a LAN controller's Stop, sent on its own connection, stops the
/// station although the relay's road has moved since; the same Stop on a closed connection is
/// refused `staleConnection`. CONTROL: before the Stop the station was on the air.
#[test]
fn a_lan_controllers_stop_holds_through_the_relays_road_moving() {
    let _alone = alone(); // a Stop disarms the satellite track: see `alone()`
    let f = Fixture::new();
    let now = Instant::now();
    let lan = f.authority.start_lan_connection();
    let who = (LAN_SESSION, LAN_DEVICE);
    let state = control_on(&f, lan, who, now).unwrap();
    keyed_under(&f, who, now);
    let stop = Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: state["transmitEpoch"].as_str().unwrap().into(),
    };
    f.authority.start_connection();
    assert!(!halted(&f, now), "premise: still on the air");
    assert_eq!(on(&f, lan, who, &stop, now), Ok(json!({"stop":"accepted"})));
    assert!(
        halted(&f, now),
        "the LAN controller's Stop left the station on the air"
    );

    let g = Fixture::new();
    let lan = g.authority.start_lan_connection();
    let state = control_on(&g, lan, who, now).unwrap();
    let stop = Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: state["transmitEpoch"].as_str().unwrap().into(),
    };
    g.authority.retire_lan_connection(lan);
    assert_eq!(on(&g, lan, who, &stop, now), Err("staleConnection"));
}

/// The two roads' ids never meet: a LAN connection's id is never a relay connection's, whatever the
/// relay's counter has reached, and the relay's exhausted counter stays its own.
#[test]
fn lan_ids_are_never_relay_ids() {
    let f = Fixture::new();
    let lan = f.authority.start_lan_connection();
    assert_ne!(lan & LAN_ROAD, 0, "a LAN id without the LAN bit: {lan:#x}");
    assert_ne!(lan, f.connection);
    assert_eq!(f.connection & LAN_ROAD, 0, "a relay id with the LAN bit");
    let next = f.authority.start_lan_connection();
    assert_ne!(next, lan, "two LAN connections share an id");
}
