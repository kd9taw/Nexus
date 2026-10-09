//! The rotator over Remote: its own v3 actions and hint, a configured rotator only, one rotctld
//! command per gesture on a worker thread (never under the Engine lock), and no transmit
//! authority as a side effect. Tests talk to an in-process fake rotctld, never a socket.
use super::receiver_filter::run;
use super::station::rotator::{self, test_rotor, Command};
use super::*;
use tempo_app::remote_control::{Completion, Reason};

/// Authority that cannot lapse mid-test. The host mints a permit ending 5 s after
/// it accepts a request and commits it inside the same call, so the window bounds
/// the *commit*, not the operator; a whole-process stall between mint and commit
/// (swap or CPU starvation under workspace load) lapses it inside the product's own
/// check, which then correctly refuses. Expiry stays asserted where it is the
/// subject, by the already-lapsed permit in
/// `a_permit_that_lapsed_before_the_worker_ran_never_moves_the_mast`.
fn unexpired_deadline() -> Instant {
    Instant::now() + Duration::from_secs(24 * 60 * 60)
}

/// A station with a rotator configured at a fake address this test alone owns.
fn station() -> (Fixture, std::sync::Arc<test_rotor::Fake>) {
    let f = Fixture::new();
    let addr = format!("rotor-test-{}:4533", id());
    let fake = test_rotor::install(&addr);
    let mut e = f.engine.lock().unwrap();
    let mut settings = e.settings().clone();
    settings.rotator_host = addr;
    settings.mygrid = "FN31".into();
    e.apply_settings(settings);
    e.set_tx_enabled(false);
    drop(e);
    (f, fake)
}

/// The receipt once the rotctld worker has finished it.
fn settle(f: &Fixture, command: &Request) -> Value {
    for _ in 0..400 {
        let value = run(f, 3, command).unwrap();
        if value["outcome"] != "pending" {
            return value;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the rotator receipt never settled");
}

#[test]
fn rotator_point_needs_v3_its_hint_and_one_rotctld_command_off_the_engine_lock() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    let (f, fake) = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("rotator")));
    let command = control_request(&state, json!({"action":"rotator.point","azimuthDeg":123.4}));
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    assert!(fake.lines().is_empty(), "a refused version writes nothing");
    let applied = settle(&f, &command);
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "stationState");
    assert_eq!(
        fake.lines(),
        vec![tempo_audio::rotator::point_line(123.4)],
        "exactly one point, at the requested azimuth"
    );
    // The receipt replays; the mast is not pointed twice.
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    assert_eq!(fake.lines().len(), 1);
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn a_station_with_no_rotator_refuses_and_a_failed_rotctld_reply_is_unknown_not_applied() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
                          // No rotator configured: nothing to point.
    let f = Fixture::new();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let command = control_request(&state, json!({"action":"rotator.stop"}));
    let refused = run(&f, 3, &command).unwrap();
    assert_eq!(refused["outcome"], "rejected");
    assert_eq!(refused["reason"], "hardwareUnavailable");

    // A rotctld command that errored may still have reached the mast: unknown, never applied.
    let (f, fake) = station();
    fake.fail(true);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let command = control_request(&state, json!({"action":"rotator.point","azimuthDeg":45}));
    let unknown = settle(&f, &command);
    assert_eq!(unknown["outcome"], "unknown");
    assert_eq!(unknown["reason"], "hardwareUnconfirmed");
    assert_eq!(fake.lines().len(), 1);
}

#[test]
fn stop_reaches_rotctld_and_point_at_call_uses_the_desktop_bearing() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    let (f, fake) = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let stop = control_request(&state, json!({"action":"rotator.stop"}));
    assert_eq!(settle(&f, &stop)["outcome"], "applied");
    assert_eq!(fake.lines(), vec!["S\n".to_string()]);

    let state = acquire_controls_version(&f, Instant::now(), 3);
    let point = control_request(
        &state,
        json!({"action":"rotator.pointAtCall","call":"JA1ABC"}),
    );
    assert_eq!(settle(&f, &point)["outcome"], "applied");
    // The desktop's own resolver, knowing nothing of JA1ABC's station: great-circle bearing from the
    // station grid to the centre of its entity (a known grid wins: see the test below).
    let me = propagation::geo::maidenhead_to_latlon("FN31").unwrap();
    let info = propagation::dxcc::resolve("JA1ABC").unwrap();
    let bearing = propagation::geo::bearing_deg(me, (info.lat, info.lon));
    assert_eq!(fake.lines()[1], tempo_audio::rotator::point_line(bearing));
    assert!(!f.engine.lock().unwrap().tx_enabled());

    // An unknown call or a missing grid has no bearing: refused before rotctld.
    for (grid, call) in [("FN31", "QQ9QQQ"), ("", "JA1ABC")] {
        let mut settings = f.engine.lock().unwrap().settings().clone();
        settings.mygrid = grid.into();
        f.engine.lock().unwrap().apply_settings(settings);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(&state, json!({"action":"rotator.pointAtCall","call":call}));
        let refused = run(&f, 3, &command).unwrap();
        assert_eq!(refused["outcome"], "rejected", "{grid:?} {call:?}");
        assert_eq!(refused["reason"], "invalidAction");
    }
    assert_eq!(fake.lines().len(), 2);
}

#[test]
fn point_at_call_turns_where_the_desktop_point_does_to_the_station_not_its_country() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    let (f, fake) = station();
    // AA1AA's own square, as the desktop's log form hands it to the engine: Boston, about 60° from
    // the station's FN31. The centre of the United States is about 260° from it, so the line on
    // the wire says which of the two the point took.
    f.engine
        .lock()
        .unwrap()
        .set_log_form_grid("AA1AA", "FN42KH");
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let point = control_request(
        &state,
        json!({"action":"rotator.pointAtCall","call":"AA1AA"}),
    );
    assert_eq!(settle(&f, &point)["outcome"], "applied");

    // The desktop's answer for the same call from the same engine: what its Rotor box shows and
    // its point turns to (`call_bearing`).
    let desktop = crate::call_bearing(&f.engine.lock().unwrap(), "AA1AA").expect("a bearing");
    let me = propagation::geo::maidenhead_to_latlon("FN31").unwrap();
    let grid = propagation::geo::bearing_deg(
        me,
        propagation::geo::maidenhead_to_latlon("FN42KH").unwrap(),
    );
    let info = propagation::dxcc::resolve("AA1AA").unwrap();
    let country = propagation::geo::bearing_deg(me, (info.lat, info.lon));
    let apart = |a: f64, b: f64| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs();
    assert!(
        apart(grid, country) > 90.0,
        "control: the grid ({grid:.1}°) and the country ({country:.1}°) must disagree"
    );
    assert_eq!(desktop.pointed.to, "grid");
    assert!(
        apart(desktop.pointed.bearing, grid) < 1e-9,
        "the desktop aims at the grid: {:.1}°",
        desktop.pointed.bearing
    );
    assert_eq!(
        fake.lines(),
        vec![tempo_audio::rotator::point_line(desktop.pointed.bearing)],
        "the Remote's point turns where the desktop's does ({grid:.1}°), not to the centre of the \
         United States ({country:.1}°)"
    );
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn a_permit_that_lapsed_before_the_worker_ran_never_moves_the_mast() {
    let (_f, fake) = station();
    let authority = tempo_app::remote_control::Revocation::default();
    let completion = Completion::guarded(authority.permit(Instant::now()).unwrap());
    std::thread::sleep(Duration::from_millis(2));
    rotator::write(&completion, fake.addr(), Command::Point(90.0));
    assert!(fake.lines().is_empty());
    assert!(!matches!(
        completion.outcome(),
        tempo_app::remote_control::Outcome::Applied { .. }
    ));
    // Positive control: the same write with a live permit reaches rotctld once.
    let completion = Completion::guarded(authority.permit(unexpired_deadline()).unwrap());
    rotator::write(&completion, fake.addr(), Command::Point(90.0));
    assert_eq!(fake.lines(), vec![tempo_audio::rotator::point_line(90.0)]);
}

#[test]
fn a_satellite_track_refuses_a_point_but_never_a_stop() {
    let (f, fake) = station();
    let settings = f.engine.lock().unwrap().settings().clone();
    let permit = || {
        tempo_app::remote_control::Revocation::default()
            .permit(unexpired_deadline())
            .unwrap()
    };
    assert!(matches!(
        rotator::queue(&settings, Command::Point(10.0), permit(), true),
        Err(Reason::StationBusy)
    ));
    let stop = rotator::queue(&settings, Command::Stop, permit(), true).unwrap();
    for _ in 0..400 {
        if !matches!(stop.outcome(), tempo_app::remote_control::Outcome::Pending) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fake.lines(), vec!["S\n".to_string()]);
    // Positive control: with no track the same point is admitted.
    rotator::queue(&settings, Command::Point(10.0), permit(), false).unwrap();
}

#[test]
fn a_pending_point_holds_the_single_receipt_so_stop_waits_behind_it() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
                          // Documented, not designed: the station admits one pending control receipt at a time, so a
                          // Stop sent while a point is still waiting on rotctld is refused as busy until it settles.
    let (f, fake) = station();
    let release = fake.hold();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let point = control_request(&state, json!({"action":"rotator.point","azimuthDeg":200}));
    assert_eq!(run(&f, 3, &point).unwrap()["outcome"], "pending");
    let state = control_state_version(&f, Instant::now(), 3);
    let stop = control_request(&state, json!({"action":"rotator.stop"}));
    assert_eq!(run(&f, 3, &stop), Err("remoteBusy"));
    release.send(()).unwrap();
    assert_eq!(settle(&f, &point)["outcome"], "applied");
}

#[test]
fn rotator_actions_refuse_extra_fields_and_a_logging_only_grant() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    for (action, extra) in [
        (
            json!({"action":"rotator.point","azimuthDeg":90}),
            "elevationDeg",
        ),
        (json!({"action":"rotator.point","azimuthDeg":90}), "host"),
        (
            json!({"action":"rotator.pointAtCall","call":"JA1ABC"}),
            "azimuthDeg",
        ),
        (json!({"action":"rotator.stop"}), "reason"),
    ] {
        let mut invalid = action.clone();
        invalid[extra] = json!(1);
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
        assert!(serde_json::from_value::<station::Action>(action).is_ok());
    }
    let (f, fake) = station();
    for azimuth in [360.0, -1.0, 12.34] {
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(
            &state,
            json!({"action":"rotator.point","azimuthDeg":azimuth}),
        );
        let refused = run(&f, 3, &command).unwrap();
        assert_eq!(refused["reason"], "invalidAction", "{azimuth}");
    }
    let (f2, _fake2) = station();
    f2.acquire(Instant::now());
    let state = control_state_version(&f2, Instant::now(), 3);
    let command = control_request(&state, json!({"action":"rotator.stop"}));
    assert_eq!(run(&f2, 3, &command), Err("localPermissionRequired"));
    assert!(fake.lines().is_empty());
}

/// One rotator gesture from a browser, and its receipt once the worker has finished it.
fn point_from_a_browser(f: &Fixture, action: Value) -> Value {
    let state = acquire_controls_version(f, Instant::now(), 3);
    settle(f, &control_request(&state, action))
}

#[test]
fn a_remote_azimuth_move_keeps_an_az_el_masts_elevation() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    let (f, fake) = station();
    fake.mast(test_rotor::Mast::AzEl {
        az: 123.0,
        el: 45.0,
    });
    let turned = point_from_a_browser(&f, json!({"action":"rotator.point","azimuthDeg":200}));
    assert_eq!(turned["outcome"], "applied");
    // ⭐ THE BUG, from a browser: `P 200.0 0` laid a G-5500's antenna on the horizon.
    assert_eq!(fake.lines(), vec!["P 200.0 45.0\n".to_string()]);
    // Point-at-call is the same move on the wire, so it keeps the elevation too.
    let called = point_from_a_browser(&f, json!({"action":"rotator.pointAtCall","call":"JA1ABC"}));
    assert_eq!(called["outcome"], "applied");
    let me = propagation::geo::maidenhead_to_latlon("FN31").unwrap();
    let info = propagation::dxcc::resolve("JA1ABC").unwrap();
    let bearing = propagation::geo::bearing_deg(me, (info.lat, info.lon));
    let kept = tempo_audio::rotator::point_line(bearing).replace(" 0\n", " 45.0\n");
    assert_eq!(fake.lines()[1], kept);
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn a_remote_azimuth_move_sends_an_azimuth_only_mast_exactly_the_line_it_always_did() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    let (f, fake) = station(); // the fake's default: an azimuth-only backend
    let turned = point_from_a_browser(&f, json!({"action":"rotator.point","azimuthDeg":200}));
    assert_eq!(turned["outcome"], "applied");
    assert_eq!(fake.lines(), vec![tempo_audio::rotator::point_line(200.0)]);
}

#[test]
fn a_remote_azimuth_move_sends_nothing_when_rotctld_does_not_say_what_the_mast_is() {
    let _alone = alone(); // a point is refused while a track is live: see `alone()`
    let (f, fake) = station();
    fake.mast(test_rotor::Mast::Silent);
    // A busy rotctld leaves `\dump_state` unanswered. Reading that as "azimuth-only" would send
    // a G-5500 `P <az> 0`, so, as on the desktop, nothing reaches the mast.
    let turned = point_from_a_browser(&f, json!({"action":"rotator.point","azimuthDeg":200}));
    assert_ne!(turned["outcome"], "applied");
    assert!(fake.lines().is_empty(), "{:?}", fake.lines());
}

/// The host's own `send` is `#[cfg(not(test))]`, so no test above runs it: which function it
/// calls is the property, and only its source says so. Source-scanned, like the desktop's
/// `manual_pointing_tests`.
#[test]
fn the_host_turns_a_remote_mast_through_the_desktops_keeping_path() {
    let src = include_str!("rotator.rs");
    let body = src
        .split_once("#[cfg(not(test))]\nfn send(")
        .expect("the host's own send")
        .1
        .split_once("\n}\n")
        .expect("its end")
        .0;
    assert!(
        body.contains("tempo_audio::rotator::point_keeping(addr, Some(az), None)"),
        "a Remote azimuth move must keep the elevation, as the desktop's does"
    );
    assert!(
        !body.contains("tempo_audio::rotator::point(addr"),
        "the host sends `P <az> 0`, the elevation of zero, on a Remote azimuth move"
    );
}
