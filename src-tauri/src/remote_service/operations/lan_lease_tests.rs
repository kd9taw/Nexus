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

/// An FT8 CQ on the air under the lease held, as a streamed controller's Call CQ puts it there:
/// the native verb, under the session's presence minted from that lease. What the radio loop halts
/// on its next tick once the lease's authority is revoked.
fn keyed_under(f: &Fixture, (session, device): (&str, &str), state: &Value, now: Instant) {
    let lease = state["leaseId"].as_str().unwrap();
    let presence = f
        .authority
        .stream_presence(session, device, lease, now)
        .unwrap();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.take_slot_tx_abort();
    e.start_cq(None).unwrap();
    assert!(e.hold_remote_presence(presence, now));
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
        keyed_under(&f, who, &state, now);
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
        keyed_under(&g, (SESSION, DEVICE), &relay_state, now);
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
    keyed_under(&f, (SESSION, DEVICE), &state, now);
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
    keyed_under(&f, who, &state, now);
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
        keyed_under(&f, who, &state, now);
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
    keyed_under(&f, who, &state, now);
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

/// A computer's Take control, on `connection`: under the station boot its state names.
fn take_on(
    f: &Fixture,
    connection: u64,
    who: (&str, &str),
    now: Instant,
) -> Result<Value, &'static str> {
    let boot = state_on(f, connection, who, now)["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    on(
        f,
        connection,
        who,
        &Request::TakeControl {
            request_id: id(),
            station_boot_id: boot,
        },
        now,
    )
}

/// ★ Take control from another computer. A holds the station with an FT CQ on the air and its
/// stream's presence; B, kept out (`controllerBusy`), takes control. A's over ends, its presence
/// and everything bound to it end, A is no longer the controller (`notController`), and B holds
/// control with TX off. Then A takes it back the same way, and B is out. CONTROL: before the take,
/// A was on the air with its presence held.
#[test]
fn a_computer_takes_control_from_another_and_starts_with_tx_off() {
    let _alone = alone();
    let f = Fixture::new();
    let now = f.now();
    let (lan_a, lan_b) = (
        f.authority.start_lan_connection(),
        f.authority.start_lan_connection(),
    );
    let (a, b) = ((LAN_SESSION, LAN_DEVICE), (LAN_OTHER_SESSION, LAN_OTHER));
    let held = control_on(&f, lan_a, a, now).unwrap();
    keyed_under(&f, a, &held, now);
    let lease = held["leaseId"].as_str().unwrap();
    let presence = f
        .authority
        .stream_presence(LAN_SESSION, LAN_DEVICE, lease, now)
        .unwrap();
    let standing = {
        let mut e = f.engine.lock().unwrap();
        assert!(e.hold_remote_presence(presence.clone(), now));
        e.remote_presence_standing(now)
            .expect("premise: A's presence is live")
    };
    assert!(
        standing.held() && f.engine.lock().unwrap().tx_enabled(),
        "premise: A on the air, its presence held"
    );
    assert_eq!(control_on(&f, lan_b, b, now), Err("controllerBusy"));

    let taken = take_on(&f, lan_b, b, now).unwrap();
    assert_eq!(taken["phase"], "controlling", "{taken}");
    assert_ne!(taken["leaseId"], held["leaseId"]);
    {
        let e = f.engine.lock().unwrap();
        assert!(!e.tx_enabled(), "TX enabled after B took control");
    }
    assert!(!presence.valid(now), "A's presence outlived the take");
    assert!(
        !standing.held(),
        "what A started under its presence goes on"
    );
    assert_eq!(heartbeat_on(&f, lan_a, a, &held, now), Err("notController"));
    assert_eq!(state_on(&f, lan_a, a, now)["phase"], "occupied");

    // A takes it back the same way.
    let back = take_on(&f, lan_a, a, now).unwrap();
    assert_eq!(back["phase"], "controlling", "{back}");
    assert_eq!(
        heartbeat_on(&f, lan_b, b, &taken, now),
        Err("notController")
    );
}

/// ⛔ The computer taking control starts with TX off, whoever turned it on: the TX-enable latch
/// turned on at the shack while A held control is off once B takes it, as a Stop leaves it. And
/// taking a station nobody holds is an Acquire: it stops nothing. CONTROL: the latch was on.
#[test]
fn taking_control_never_carries_the_tx_latch_over() {
    let _alone = alone();
    let f = Fixture::new();
    let now = f.now();
    let (lan_a, lan_b) = (
        f.authority.start_lan_connection(),
        f.authority.start_lan_connection(),
    );
    let (a, b) = ((LAN_SESSION, LAN_DEVICE), (LAN_OTHER_SESSION, LAN_OTHER));
    control_on(&f, lan_a, a, now).unwrap();
    f.engine.lock().unwrap().set_tx_enabled(true);
    assert!(f.engine.lock().unwrap().tx_enabled(), "premise: TX on");
    f.authority.permit_station(b.1, true).unwrap();
    assert_eq!(take_on(&f, lan_b, b, now).unwrap()["phase"], "controlling");
    assert!(
        !f.engine.lock().unwrap().tx_enabled(),
        "B started with TX on"
    );

    let g = Fixture::new();
    let lan = g.authority.start_lan_connection();
    g.authority.permit_station(b.1, true).unwrap();
    g.engine.lock().unwrap().set_tx_enabled(true);
    assert_eq!(take_on(&g, lan, b, now).unwrap()["phase"], "controlling");
    assert!(
        g.engine.lock().unwrap().tx_enabled(),
        "a take with nobody to take from stopped the station"
    );
}

/// ⛔ A take that is refused changes nothing: from a device without station control
/// (`localPermissionRequired`), under another boot of the station (`staleStation`), or from the
/// relay's road, where browsers are (`stationUnsupported`). After each, A still holds control and
/// TX is still on. CONTROL: the same take, allowed, ends both.
#[test]
fn a_refused_take_leaves_the_controller_and_its_over_alone() {
    let _alone = alone();
    for refusal in ["permission", "boot", "relay", "allowed"] {
        let f = Fixture::new();
        let now = f.now();
        let (lan_a, lan_b) = (
            f.authority.start_lan_connection(),
            f.authority.start_lan_connection(),
        );
        let (a, b) = ((LAN_SESSION, LAN_DEVICE), (LAN_OTHER_SESSION, LAN_OTHER));
        let held = control_on(&f, lan_a, a, now).unwrap();
        f.engine.lock().unwrap().set_tx_enabled(true);
        if refusal != "permission" {
            f.authority.permit_station(b.1, true).unwrap();
        }
        let said = match refusal {
            "boot" => on(
                &f,
                lan_b,
                b,
                &Request::TakeControl {
                    request_id: id(),
                    station_boot_id: id(),
                },
                now,
            ),
            "relay" => take_on(&f, f.connection, b, now),
            _ => take_on(&f, lan_b, b, now),
        };
        let expected = match refusal {
            "permission" => Err("localPermissionRequired"),
            "boot" => Err("staleStation"),
            "relay" => Err("stationUnsupported"),
            _ => Ok(json!("controlling")),
        };
        assert_eq!(said.map(|s| s["phase"].clone()), expected, "{refusal}");
        let kept = heartbeat_on(&f, lan_a, a, &held, now).is_ok();
        let on_air = f.engine.lock().unwrap().tx_enabled();
        assert_eq!(
            (kept, on_air),
            (refusal != "allowed", refusal != "allowed"),
            "{refusal}: (A keeps control, TX still on)"
        );
    }
}

/// The lease's own requests are unchanged on the wire by Take control: Acquire, Heartbeat and
/// Release serialize to exactly the bytes they always have.
#[test]
fn the_lease_requests_are_byte_identical() {
    let r = "10000000-0000-4000-8000-00000000000a".to_string();
    let x = "20000000-0000-4000-8000-00000000000b".to_string();
    let bytes = |request: Request| serde_json::to_string(&request).unwrap();
    assert_eq!(
        bytes(Request::Acquire {
            request_id: r.clone(),
            station_boot_id: x.clone()
        }),
        format!(r#"{{"type":"acquire","requestId":"{r}","stationBootId":"{x}"}}"#)
    );
    assert_eq!(
        bytes(Request::Heartbeat {
            request_id: r.clone(),
            lease_id: x.clone()
        }),
        format!(r#"{{"type":"heartbeat","requestId":"{r}","leaseId":"{x}"}}"#)
    );
    assert_eq!(
        bytes(Request::Release {
            request_id: r.clone(),
            lease_id: x.clone()
        }),
        format!(r#"{{"type":"release","requestId":"{r}","leaseId":"{x}"}}"#)
    );
}

/// ⛔ Taking control ends a live satellite track, as the Stop it runs does, and hands the dial
/// back: the station steering by itself after its controller changed is what the track's rule
/// forbids. CONTROL: the track and the dial were live before the take.
#[test]
#[cfg(feature = "radio")]
fn taking_control_ends_a_live_satellite_track() {
    let _alone = alone();
    let f = holding_a_transponder(true);
    let now = f.now();
    assert!(crate::SAT_TRACK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some());
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_some());
    let (lan_a, lan_b) = (
        f.authority.start_lan_connection(),
        f.authority.start_lan_connection(),
    );
    let (a, b) = ((LAN_SESSION, LAN_DEVICE), (LAN_OTHER_SESSION, LAN_OTHER));
    control_on(&f, lan_a, a, now).unwrap();
    f.authority.permit_station(b.1, true).unwrap();
    assert_eq!(take_on(&f, lan_b, b, now).unwrap()["phase"], "controlling");
    assert!(track_ended(), "the take left the track flying");
    assert!(
        f.engine.lock().unwrap().sat_transponder_held().is_none(),
        "the dial is the operator's again"
    );
}

/// ★ THE LAN WAIVER IS RECORDED PROVENANCE, NOT TRANSPORT (the 2026-10-23 sign-in decision,
/// finding 3): a device the station paired/enrolled on its OWN network — recorded in the LAN book —
/// is never held, even when the account lists it as sign-in-added with no waiver record, and even
/// when it connects over the relay. A sign-in device that is NOT in the book is held regardless of
/// a LAN-looking transport: a tunnelled connection can never take the LAN waiver.
#[test]
fn the_lan_waiver_keys_on_the_book_not_the_transport() {
    // In the LAN book + listed sign-in-added + no record, but FREE: the book is the waiver.
    let f = Fixture::new();
    f.engine.lock().unwrap().set_tx_enabled(false);
    let now = f.now();
    f.authority
        .set_signin_devices(std::collections::BTreeSet::from([LAN_DEVICE.to_string()]));
    f.authority
        .hold_lan_devices(Box::new(|| vec![LAN_DEVICE.to_string()]));
    control_on(&f, f.connection, (LAN_SESSION, LAN_DEVICE), now).unwrap();
    assert!(
        !f.engine.lock().unwrap().remote_transmit_held(),
        "a device in the LAN book is free, sign-in-added or not, over any road"
    );

    // CONTROL (finding 3): a sign-in device NOT in the book, over a LAN-LOOKING transport, is still
    // held — the transport marker grants no waiver.
    let g = Fixture::new();
    g.engine.lock().unwrap().set_tx_enabled(false);
    g.authority
        .set_signin_devices(std::collections::BTreeSet::from([LAN_DEVICE.to_string()]));
    let lan = g.authority.start_lan_connection();
    control_on(&g, lan, (LAN_SESSION, LAN_DEVICE), now).unwrap();
    assert!(
        g.engine.lock().unwrap().remote_transmit_held(),
        "a sign-in device not in the book is held even on a LAN-looking transport"
    );
}
