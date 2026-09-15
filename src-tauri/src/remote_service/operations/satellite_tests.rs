//! The satellite section over Remote: its own v3 actions and hint, each verb reaching the
//! desktop's own function on a worker thread (never under the Engine lock), and the three things
//! that must end a track — the rail Stop, the session Stop, and this browser going away.
//!
//! The authority-loss end is tested where the loop lives (`lib.rs`,
//! `a_remotely_armed_pass_ends_when_the_browser_that_armed_it_goes_away`): it is a property of the
//! shipped track loop, and asserting it here would be asserting on a copy.
use super::receiver_filter::run;
use super::*;

/// Exclusive use of the process-wide track badge for the length of one test. Held by every test
/// in this file, because `SAT_TRACK`/`SAT_TRACK_GEN` are process-wide: a sibling arming or
/// clearing a badge mid-test would (correctly) win, which reads as flake rather than as the
/// collision it is.
/// It also starts the test from an idle badge, so "no track is running" is a fact this test
/// established rather than whatever the previous one happened to leave behind.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::TEST_SAT_TRACK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *crate::SAT_TRACK.lock().unwrap_or_else(|e| e.into_inner()) = None;
    guard
}

/// A station holding a transponder with a live track badge — what a Stop has to end.
fn tracking() -> Fixture {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.set_sat_transponder(Some((
            "SO-50|FM repeater".into(),
            0,
            tempo_core::doppler::Transponder::channel(145_850_000, 436_795_000),
        )));
    }
    crate::test_live_sat_track("SO-50");
    f
}

/// The receipt once the satellite worker has finished it.
///
/// `stationBusy` is retried rather than failed: unlike the rotator's, these workers DO take the
/// Engine lock — they are the desktop's own verbs — so a poll landing mid-write is refused exactly
/// as any other request would be while the radio loop holds it.
fn settle(f: &Fixture, command: &Request) -> Value {
    for _ in 0..400 {
        match run(f, 3, command) {
            Ok(value) if value["outcome"] != "pending" => return value,
            Ok(_) | Err("stationBusy") | Err("remoteBusy") => {}
            Err(other) => panic!("the satellite receipt failed: {other}"),
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the satellite receipt never settled");
}

/// True once the process-wide badge is clear. The stop runs on a worker, so this is a wait.
fn track_ended() -> bool {
    for _ in 0..400 {
        if crate::SAT_TRACK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn the_satellite_section_needs_v3_and_its_own_hint() {
    let _alone = alone();
    let f = Fixture::new();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("satellite")));
    let command = control_request(&state, json!({"action":"satellite.stopTrack"}));
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    // Positive control: the same command at v3 is admitted and settles.
    assert_eq!(settle(&f, &command)["outcome"], "applied");

    // A browser with the logging grant only never reaches any of it.
    let f2 = Fixture::new();
    f2.acquire(Instant::now());
    let state = control_state_version(&f2, Instant::now(), 3);
    let command = control_request(&state, json!({"action":"satellite.peg","on":true}));
    assert_eq!(run(&f2, 3, &command), Err("localPermissionRequired"));
    assert!(!f2.engine.lock().unwrap().settings().radio_pegged);
}

/// ⭐ **THE SAFETY ITEM.** A remote Stop must end an active satellite track.
///
/// At the shack these are two controls — the header's Stop TX and the readiness rail's Stop — and
/// the operator picks. A browser has ONE Stop, and a satellite track is the app's only standing
/// instruction to keep MOVING the radio and the mast by itself; a Stop that left it running would
/// leave the station steering after the operator said stop.
#[test]
fn a_remote_stop_ends_an_active_satellite_track_and_hands_the_dial_back() {
    let _alone = alone();
    let f = tracking();
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 3);
    // POSITIVE CONTROL: the badge and the hold really are live before the Stop, so "gone
    // afterwards" is about the Stop and not about a fixture that was never set up.
    assert!(crate::SAT_TRACK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some());
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_some());

    let request = Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: format!("{:016x}", f.authority.transmit.generation()),
    };
    let accepted = f
        .authority
        .handle_version((f.connection, 4), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(accepted, json!({"stop":"accepted"}));

    assert!(track_ended(), "the Stop ended the track");
    // Stopping a live track IS the dial handback, exactly as the rail Stop's own verb performs it.
    assert!(
        f.engine.lock().unwrap().sat_transponder_held().is_none(),
        "the dial is the operator's again"
    );
    // ⛔ And it is still a STOP: nothing was armed, keyed or re-enabled on the way.
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

/// A Stop with no track running must leave a transponder the operator STAGED for a later pass
/// exactly where it is — releasing a hold nobody asked to release is doing more than was asked.
/// This is the desktop's own idle-stop rule (`stop_sat_track`'s `was_live`), reached from here.
#[test]
fn a_remote_stop_with_no_track_running_leaves_a_staged_transponder_alone() {
    let _alone = alone();
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.set_sat_transponder(Some((
            "AO-91|FM repeater".into(),
            0,
            tempo_core::doppler::Transponder::channel(145_960_000, 435_250_000),
        )));
    }
    let now = Instant::now();
    let state = acquire_controls_version(&f, now, 3);
    let request = Request::StopTransmit {
        request_id: id(),
        station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        lease_id: state["leaseId"].as_str().unwrap().into(),
        transmit_epoch: format!("{:016x}", f.authority.transmit.generation()),
    };
    f.authority
        .handle_version((f.connection, 4), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        f.engine.lock().unwrap().sat_transponder_held().is_some(),
        "an idle Stop must not release a pick staged for a later pass"
    );
}

/// The rail's own Stop, as a satellite action rather than the session Stop.
#[test]
fn the_rail_stop_ends_the_track_and_hands_the_dial_back() {
    let _alone = alone();
    let f = tracking();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_some());
    let stop = control_request(&state, json!({"action":"satellite.stopTrack"}));
    assert_eq!(settle(&f, &stop)["outcome"], "applied");
    assert!(track_ended());
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_none());
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

/// Handing the dial back is a transponder pick of `null`, and it is the desktop's own verb.
#[test]
fn clearing_the_transponder_hands_the_dial_back() {
    let _alone = alone();
    let f = tracking();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let clear = control_request(
        &state,
        json!({"action":"satellite.transponder","name":"SO-50","index":null,"auto":false}),
    );
    assert_eq!(settle(&f, &clear)["outcome"], "applied");
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_none());
    // A pick the station cannot resolve is refused, never silently applied: this test station has
    // no element set, so no bird can be named.
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let pick = control_request(
        &state,
        json!({"action":"satellite.transponder","name":"SO-50","index":0,"auto":false}),
    );
    let refused = settle(&f, &pick);
    assert_eq!(refused["outcome"], "rejected");
    assert_eq!(refused["reason"], "invalidAction");
    assert!(f.engine.lock().unwrap().sat_transponder_held().is_none());
}

/// Peg-lock, the Doppler switch and the uplink mapping each reach the station's own writer and are
/// persisted — the three rail/binding fixes.
#[test]
fn peg_doppler_and_the_uplink_mapping_are_written_and_saved() {
    let _alone = alone();
    let f = Fixture::new();
    f.engine
        .lock()
        .unwrap()
        .configure_remote_settings_store(f.dir.join("settings.json"));
    assert!(!f.engine.lock().unwrap().settings().radio_pegged);
    assert!(!f.engine.lock().unwrap().settings().sat_doppler_off);

    // Doppler OFF, so the write is visible against the default.
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let off = control_request(&state, json!({"action":"satellite.doppler","on":false}));
    assert_eq!(settle(&f, &off)["evidence"], "settingsSaved");
    assert!(f.engine.lock().unwrap().settings().sat_doppler_off);
    // …and back on, which is the gesture the rail actually offers.
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let on = control_request(&state, json!({"action":"satellite.doppler","on":true}));
    assert_eq!(settle(&f, &on)["evidence"], "settingsSaved");
    assert!(!f.engine.lock().unwrap().settings().sat_doppler_off);

    let state = acquire_controls_version(&f, Instant::now(), 3);
    let peg = control_request(&state, json!({"action":"satellite.peg","on":true}));
    assert_eq!(settle(&f, &peg)["evidence"], "settingsSaved");
    assert!(f.engine.lock().unwrap().settings().radio_pegged);

    let state = acquire_controls_version(&f, Instant::now(), 3);
    let map = control_request(
        &state,
        json!({"action":"satellite.uplinkMap","map":"a-up-b-down","radioId":null}),
    );
    assert_eq!(settle(&f, &map)["evidence"], "settingsSaved");
    assert_eq!(
        f.engine.lock().unwrap().settings().sat_vfo_map,
        tempo_app::settings::SatVfoMap::AUpBDown
    );
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

/// An arm the station will not make is REFUSED, never silently ignored: this station has no
/// element set, so there is no bird to fly and no pass to match.
#[test]
fn an_arm_the_station_will_not_make_is_refused() {
    let _alone = alone();
    let f = Fixture::new();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let arm = control_request(
        &state,
        json!({"action":"satellite.track","name":"SO-50","aosUnix":1785542400}),
    );
    let refused = settle(&f, &arm);
    assert_eq!(refused["outcome"], "rejected");
    assert_eq!(refused["reason"], "invalidAction");
    assert!(crate::SAT_TRACK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_none());
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

/// The wire is closed: an extra field is not understood, and every shape the page can send is.
#[test]
fn satellite_actions_refuse_extra_fields() {
    for (action, extra) in [
        (
            json!({"action":"satellite.track","name":"SO-50","aosUnix":1785542400}),
            "elevationDeg",
        ),
        (json!({"action":"satellite.stopTrack"}), "name"),
        (
            json!({"action":"satellite.transponder","name":"SO-50","index":0,"auto":false}),
            "uplinkMhz",
        ),
        (json!({"action":"satellite.doppler","on":true}), "radioId"),
        (
            json!({"action":"satellite.uplinkMap","map":null,"radioId":null}),
            "confirmed",
        ),
        (json!({"action":"satellite.peg","on":false}), "radioId"),
        (json!({"action":"satellite.elements"}), "source"),
    ] {
        let mut invalid = action.clone();
        invalid[extra] = json!(1);
        assert!(
            serde_json::from_value::<station::Action>(invalid).is_err(),
            "{action} + {extra}"
        );
        assert!(
            serde_json::from_value::<station::Action>(action.clone()).is_ok(),
            "{action}"
        );
    }
    // A mapping the enum does not name is not a mapping.
    assert!(serde_json::from_value::<station::Action>(
        json!({"action":"satellite.uplinkMap","map":"a-up-b-sideways","radioId":null})
    )
    .is_err());
}
