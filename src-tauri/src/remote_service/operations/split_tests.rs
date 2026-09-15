//! Remote parity batch 1: split, RIT, XIT and VFO. Split, XIT and VFO move the transmit
//! frequency, so they are refused while TX is armed and whenever the licence would not allow the
//! resulting emission. None of them may arm or key anything.
use super::*;
use tempo_app::settings::LicenseClass;

/// A General-class CW station at 14.030 (legal for a General; 14.000-14.025 is Extra-only CW).
fn station(class: LicenseClass) -> Fixture {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    let mut settings = e.settings().clone();
    settings.license_class = class;
    e.apply_settings(settings);
    e.set_operating_mode("cw", false);
    e.set_frequency(14.030, "20m", "USB");
    e.take_immediate_retune();
    e.set_tx_enabled(false);
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

/// Nothing keyed, nothing owned, nothing armed.
fn assert_never_keyed(f: &Fixture) {
    let e = f.engine.lock().unwrap();
    assert!(!e.tx_enabled(), "the TX latch stays off");
    assert!(e.tx_owner().is_none());
    assert!(!e.snapshot().radio.transmitting);
}

#[test]
fn split_needs_v3_applies_the_local_verb_once_and_never_keys() {
    let f = station(LicenseClass::General);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let capabilities = state["controls"]["capabilities"].as_array().unwrap();
    assert!(capabilities.contains(&json!("splitTuning")));
    assert!(capabilities.contains(&json!("ritTuning")));
    let command = control_request(
        &state,
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.032}),
    );
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    assert_eq!(f.engine.lock().unwrap().split_tx_mhz(), None);
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "stationState");
    assert_eq!(f.engine.lock().unwrap().split_tx_mhz(), Some(14.032));
    assert_eq!(
        f.engine.lock().unwrap().snapshot().radio.split_tx_mhz,
        Some(14.032)
    );
    assert_never_keyed(&f);
    // A replay returns the same receipt and queues no second request.
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    let mut e = f.engine.lock().unwrap();
    assert_eq!(e.take_split_request(), Some(Some(14.032)));
    assert_eq!(e.take_split_request(), None);
    assert!(e.take_remote_radio().is_none());
}

#[test]
fn rit_xit_and_vfo_apply_their_local_verbs_without_keying() {
    let f = station(LicenseClass::General);
    let rit = send(&f, json!({"action":"radio.rit","expectedHz":0,"hz":50}));
    assert_eq!(rit["evidence"], "stationState");
    let xit = send(&f, json!({"action":"radio.xit","expectedHz":0,"hz":-4000}));
    assert_eq!(xit["evidence"], "stationState", "{xit}");
    let vfo = send(
        &f,
        json!({"action":"radio.vfo","expectedVfo":"A","vfo":"B"}),
    );
    assert_eq!(vfo["evidence"], "stationState", "{vfo}");
    assert_never_keyed(&f);
    let mut e = f.engine.lock().unwrap();
    let radio = e.snapshot().radio;
    assert_eq!(
        (radio.rit_hz, radio.xit_hz, radio.active_vfo.as_str()),
        (50, -4000, "B")
    );
    assert_eq!(e.take_rit_apply(), Some(50));
    assert_eq!(e.take_xit_apply(), Some(-4000));
    assert_eq!(e.take_vfo_apply(), Some(true));
}

#[test]
fn a_split_or_xit_outside_the_licence_is_refused_and_a_legal_one_is_not() {
    let f = station(LicenseClass::General);
    let refused = send(
        &f,
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.020}),
    );
    assert_eq!(refused["outcome"], "rejected");
    assert_eq!(refused["reason"], "outsidePrivileges");
    let xit = send(&f, json!({"action":"radio.xit","expectedHz":0,"hz":-6000}));
    assert_eq!(xit["reason"], "outsidePrivileges");
    {
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.split_tx_mhz(), None);
        assert_eq!(e.take_split_request(), None);
        assert_eq!(e.take_xit_apply(), None);
    }
    assert_never_keyed(&f);
    // Positive controls: a legal split and XIT for the same General station.
    let legal = send(
        &f,
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.032}),
    );
    assert_eq!(legal["outcome"], "applied");
    let f = station(LicenseClass::General);
    let legal_xit = send(&f, json!({"action":"radio.xit","expectedHz":0,"hz":-4000}));
    assert_eq!(legal_xit["outcome"], "applied");
    // The refusal is the licence: an Extra may split into the Extra-only segment.
    let extra = station(LicenseClass::Extra);
    let accepted = send(
        &extra,
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.020}),
    );
    assert_eq!(accepted["outcome"], "applied");
}

#[test]
fn split_xit_and_vfo_are_refused_while_transmit_is_armed_and_rit_is_not() {
    for action in [
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.032}),
        json!({"action":"radio.xit","expectedHz":0,"hz":100}),
        json!({"action":"radio.vfo","expectedVfo":"A","vfo":"B"}),
    ] {
        let f = station(LicenseClass::General);
        {
            let mut e = f.engine.lock().unwrap();
            e.set_tx_enabled(true);
            // Arming TX queues a retune one-shot the radio loop consumes on its next pass.
            e.take_immediate_retune();
        }
        let busy = send(&f, action.clone());
        assert_eq!(busy["outcome"], "rejected", "{action}");
        assert_eq!(busy["reason"], "stationBusy", "{action}");
        let mut e = f.engine.lock().unwrap();
        assert!(e.tx_enabled(), "the TX latch is left exactly as it was");
        assert_eq!(e.split_tx_mhz(), None);
        assert_eq!(e.take_split_request(), None);
        assert_eq!(e.take_xit_apply(), None);
        assert_eq!(e.take_vfo_apply(), None);
    }
    // RIT is receive-only: accepted with TX armed, and the latch is untouched.
    let f = station(LicenseClass::General);
    {
        let mut e = f.engine.lock().unwrap();
        e.set_tx_enabled(true);
        // Arming TX queues a retune one-shot the radio loop consumes on its next pass.
        e.take_immediate_retune();
    }
    let rit = send(&f, json!({"action":"radio.rit","expectedHz":0,"hz":-20}));
    assert_eq!(rit["outcome"], "applied");
    assert!(f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn stale_other_band_and_vfo_under_split_requests_are_refused() {
    let f = station(LicenseClass::General);
    let stale = send(&f, json!({"action":"radio.xit","expectedHz":10,"hz":0}));
    assert_eq!(stale["reason"], "contextChanged");
    let stale_split = send(
        &f,
        json!({"action":"radio.split","expectedTxMhz":14.031,"txMhz":14.032}),
    );
    assert_eq!(stale_split["reason"], "contextChanged");
    let other_band = send(
        &f,
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":7.030}),
    );
    assert_eq!(other_band["reason"], "invalidAction");
    assert_eq!(
        send(
            &f,
            json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.032})
        )["outcome"],
        "applied"
    );
    // The first request is not yet applied by the radio loop: a second one waits.
    let again = send(
        &f,
        json!({"action":"radio.split","expectedTxMhz":14.032,"txMhz":14.033}),
    );
    assert_eq!(again["reason"], "stationBusy");
    let vfo = send(
        &f,
        json!({"action":"radio.vfo","expectedVfo":"A","vfo":"B"}),
    );
    assert_eq!(vfo["reason"], "stationBusy");
    let mut e = f.engine.lock().unwrap();
    assert_eq!(e.take_vfo_apply(), None);
    assert_eq!(e.split_tx_mhz(), Some(14.032));
    drop(e);
    assert_never_keyed(&f);
}

#[test]
fn split_and_clarifiers_reject_logging_only_grants_and_extra_fields() {
    let f = station(LicenseClass::General);
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    let actions = [
        json!({"action":"radio.split","expectedTxMhz":null,"txMhz":14.032}),
        json!({"action":"radio.rit","expectedHz":0,"hz":10}),
        json!({"action":"radio.xit","expectedHz":0,"hz":10}),
        json!({"action":"radio.vfo","expectedVfo":"A","vfo":"B"}),
    ];
    for action in &actions {
        assert_eq!(
            run(&f, 3, &control_request(&state, action.clone())),
            Err("localPermissionRequired")
        );
        assert!(serde_json::from_value::<station::Action>(action.clone()).is_ok());
        for field in ["settings", "command", "txEnabled", "radioId", "dialMhz"] {
            let mut invalid = action.clone();
            invalid[field] = json!(1);
            assert!(serde_json::from_value::<station::Action>(invalid).is_err());
        }
        for key in action
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| *k != "action")
        {
            let mut missing = action.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<station::Action>(missing).is_err(),
                "missing {key}"
            );
        }
    }
    for invalid in [
        json!({"action":"radio.rit","expectedHz":0,"hz":10.5}),
        json!({"action":"radio.vfo","expectedVfo":"A","vfo":"C"}),
        json!({"action":"radio.swapVfo"}),
    ] {
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    }
    let e = f.engine.lock().unwrap();
    assert_eq!(e.split_tx_mhz(), None);
    assert!(!e.tx_enabled());
}
