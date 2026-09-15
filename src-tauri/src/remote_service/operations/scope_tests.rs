//! Remote parity batch 1: rig-scope settings. Receive-display one-shots the station applies with the
//! local scope verbs, only for the scope family its own configuration runs, and never while the
//! transmitter is owned or a tune carrier is up. None of them may arm or key anything.
use super::*;
use tempo_app::settings::LicenseClass;

#[derive(Clone, Copy, Debug)]
enum Rig {
    Icom,
    Flex,
    Ft710,
    NoScope,
}

/// A Phone station at 14.200 with a fresh, unkeyed CAT reading and the named scope configuration.
fn station(rig: Rig) -> Fixture {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    let mut settings = e.settings().clone();
    // An IC-7300 on serial CI-V, a FLEX-6000 on the network with its native pan opted in, and the
    // Hamlib dummy for the FT-710 (whose family is the mode code it reports) and for no scope.
    let (model, conn, addr, pan) = match rig {
        Rig::Icom => (3073, "serial", "", false),
        Rig::Flex => (2036, "network", "192.0.2.10:5002", true),
        Rig::Ft710 | Rig::NoScope => (1, "serial", "", false),
    };
    settings.rig_model = model;
    settings.rig_conn = conn.into();
    settings.rig_addr = addr.into();
    settings.flex_native_pan = pan;
    settings.license_class = LicenseClass::Extra;
    e.apply_settings(settings);
    assert_eq!(e.settings().rig_model, model, "{rig:?} configuration took");
    if matches!(rig, Rig::Ft710) {
        e.set_scope_mode_code(Some(u32::from(b'4')));
    }
    e.set_operating_mode("phone", false);
    e.set_frequency(14.2, "20m", "USB");
    e.set_tx_enabled(false);
    e.take_immediate_retune();
    let connection = e.remote_open_radio().unwrap();
    let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_ptt(Some(&read), Some(false));
    drop(e);
    f
}

fn run(f: &Fixture, version: u8, command: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, version),
        SESSION,
        DEVICE,
        command,
        &f.engine,
        Instant::now(),
    )
}

fn send(f: &Fixture, action: Value) -> Value {
    let state = acquire_controls_version(f, Instant::now(), 3);
    run(f, 3, &control_request(&state, action)).unwrap()
}

/// Nothing queued in any scope one-shot, and nothing keyed, owned or armed.
fn assert_nothing_queued_or_keyed(f: &Fixture, pan: (f64, Option<i32>), why: &str) {
    let mut e = f.engine.lock().unwrap();
    assert_eq!(e.take_scope_span_request(), None, "{why}");
    assert_eq!(e.take_scope_ref_request(), None, "{why}");
    assert_eq!(e.take_yaesu_scope_mode_request(), None, "{why}");
    assert_eq!((e.flex_pan_span_hz(), e.flex_pan_ref_dbm()), pan, "{why}");
    assert!(!e.tx_enabled(), "{why}");
    assert!(e.tx_owner().is_none(), "{why}");
}

#[test]
fn each_scope_setting_needs_v3_applies_the_local_verb_once_and_never_keys() {
    for (rig, action) in [
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"span","hz":25000}),
        ),
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"ref","tenthsDb":-35}),
        ),
        (
            Rig::Ft710,
            json!({"action":"radio.scope","setting":"span","hz":500000}),
        ),
        (
            Rig::Ft710,
            json!({"action":"radio.scope","setting":"position","position":"fix"}),
        ),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"panSpan","hz":200000}),
        ),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"panRef","refDbm":-80}),
        ),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"panRef","refDbm":null}),
        ),
    ] {
        let f = station(rig);
        if action["setting"] == "panRef" && action["refDbm"].is_null() {
            // Auto must be a change from a stated level, or the check below proves nothing.
            f.engine.lock().unwrap().set_flex_pan_ref(Some(-100));
        }
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("rigScope")));
        let command = control_request(&state, action.clone());
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"), "{action}");
        let applied = run(&f, 3, &command).unwrap();
        assert_eq!(applied["outcome"], "applied", "{action}");
        assert_eq!(applied["evidence"], "stationState", "{action}");
        {
            let mut e = f.engine.lock().unwrap();
            match action["setting"].as_str().unwrap() {
                "span" => assert_eq!(
                    e.take_scope_span_request(),
                    Some(action["hz"].as_u64().unwrap() as u32)
                ),
                "ref" => assert_eq!(e.take_scope_ref_request(), Some(-35)),
                "position" => {
                    // FIX in the W/F NORMAL family the rig reported, resolved as the desktop does.
                    let fix = tempo_audio::yaesu_wf::mode_code_for(
                        tempo_audio::yaesu_wf::ScopePosition::Fix,
                        b'4',
                    );
                    assert_eq!(fix, b'A');
                    assert_eq!(e.take_yaesu_scope_mode_request(), Some(fix));
                }
                "panSpan" => assert_eq!(e.flex_pan_span_hz(), 200_000.0),
                _ => assert_eq!(
                    e.flex_pan_ref_dbm(),
                    action["refDbm"].as_i64().map(|dbm| dbm as i32)
                ),
            }
            assert!(!e.tx_enabled(), "{action} leaves the latch down");
            assert!(e.tx_owner().is_none(), "{action}");
            assert!(e.take_remote_radio().is_none(), "{action} retunes nothing");
        }
        // A replay returns the same receipt and applies nothing a second time.
        let pan = {
            let e = f.engine.lock().unwrap();
            (e.flex_pan_span_hz(), e.flex_pan_ref_dbm())
        };
        assert_eq!(run(&f, 3, &command).unwrap(), applied, "{action}");
        assert_nothing_queued_or_keyed(&f, pan, "replay");
    }
}

#[test]
fn a_setting_the_configured_scope_cannot_take_is_hardware_unavailable_and_queues_nothing() {
    for (rig, action) in [
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"position","position":"fix"}),
        ),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"span","hz":25000}),
        ),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"ref","tenthsDb":0}),
        ),
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"panSpan","hz":200000}),
        ),
        (
            Rig::NoScope,
            json!({"action":"radio.scope","setting":"span","hz":25000}),
        ),
        (
            Rig::Ft710,
            json!({"action":"radio.scope","setting":"panRef","refDbm":null}),
        ),
    ] {
        let f = station(rig);
        let pan = {
            let e = f.engine.lock().unwrap();
            (e.flex_pan_span_hz(), e.flex_pan_ref_dbm())
        };
        let result = send(&f, action.clone());
        assert_eq!(result["outcome"], "rejected", "{rig:?} {action}");
        assert_eq!(result["reason"], "hardwareUnavailable", "{rig:?} {action}");
        assert_nothing_queued_or_keyed(&f, pan, &format!("{rig:?} {action}"));
    }
    // Positive controls: each setting on the family that runs it is applied.
    for (rig, action) in [
        (
            Rig::Ft710,
            json!({"action":"radio.scope","setting":"position","position":"fix"}),
        ),
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"span","hz":25000}),
        ),
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"ref","tenthsDb":0}),
        ),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"panSpan","hz":200000}),
        ),
    ] {
        let f = station(rig);
        assert_eq!(
            send(&f, action.clone())["outcome"],
            "applied",
            "{rig:?} {action}"
        );
    }
}

#[test]
fn refused_while_a_tune_carrier_is_up_with_the_idle_request_as_control() {
    for tune in [true, false] {
        let f = station(Rig::Icom);
        if tune {
            let mut e = f.engine.lock().unwrap();
            e.set_tune(true);
            assert!(e.tuning(), "the setup really holds a tune carrier");
        }
        let result = send(
            &f,
            json!({"action":"radio.scope","setting":"ref","tenthsDb":-35}),
        );
        let mut e = f.engine.lock().unwrap();
        if tune {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "stationBusy");
            assert_eq!(e.take_scope_ref_request(), None);
            assert!(e.tuning(), "the refusal leaves the carrier as it was");
        } else {
            assert_eq!(result["outcome"], "applied");
            assert_eq!(e.take_scope_ref_request(), Some(-35));
        }
        assert!(!e.tx_enabled());
    }
}

#[test]
fn unknown_fields_and_values_fail_to_parse_and_a_setting_needs_exactly_its_own_field() {
    for bad in [
        json!({"action":"radio.scope","setting":"span","hz":25000,"command":"SS05;"}),
        json!({"action":"radio.scope","setting":"span","hz":25000,"family":"civ"}),
        json!({"action":"radio.scope","setting":"fixed","fixed":true}),
        json!({"action":"radio.scope","setting":"position","position":"middle"}),
        json!({"action":"radio.scope","setting":"span","hz":-2500}),
        json!({"action":"radio.scope","setting":"ref","tenthsDb":"0"}),
        json!({"action":"radio.scope","hz":25000}),
    ] {
        assert!(
            serde_json::from_value::<station::Action>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    for (rig, bad) in [
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"span","hz":25000,"tenthsDb":0}),
        ),
        (Rig::Icom, json!({"action":"radio.scope","setting":"span"})),
        (
            Rig::Flex,
            json!({"action":"radio.scope","setting":"panRef"}),
        ),
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"span","hz":2400}),
        ),
        (
            Rig::Icom,
            json!({"action":"radio.scope","setting":"ref","tenthsDb":201}),
        ),
    ] {
        let f = station(rig);
        let pan = {
            let e = f.engine.lock().unwrap();
            (e.flex_pan_span_hz(), e.flex_pan_ref_dbm())
        };
        let result = send(&f, bad.clone());
        assert_eq!(result["outcome"], "rejected", "{bad}");
        assert_eq!(result["reason"], "invalidAction", "{bad}");
        assert_nothing_queued_or_keyed(&f, pan, &bad.to_string());
    }
}

#[test]
fn a_logging_only_browser_cannot_change_the_scope() {
    let f = station(Rig::Icom);
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(
            &f,
            3,
            &control_request(
                &state,
                json!({"action":"radio.scope","setting":"ref","tenthsDb":-35})
            )
        ),
        Err("localPermissionRequired")
    );
    assert_eq!(f.engine.lock().unwrap().take_scope_ref_request(), None);
}
