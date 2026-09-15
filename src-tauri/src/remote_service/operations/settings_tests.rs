//! Operating preferences from a Remote browser: the station's allow-list, a confirmed save with a
//! receipt, the grant each preference needs, a stale revision refused, and no allowed setting that
//! keys, tunes or moves the TX-enable latch.
use super::*;

fn store(f: &Fixture) {
    f.engine
        .lock()
        .unwrap()
        .configure_remote_settings_store(f.dir.join("settings.json"));
}

fn saved(f: &Fixture) -> Option<Value> {
    std::fs::read(f.dir.join("settings.json"))
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}

fn settings(f: &Fixture) -> Value {
    serde_json::to_value(f.engine.lock().unwrap().settings()).unwrap()
}

fn revision(f: &Fixture) -> String {
    super::super::super::query::settings_revision(f.engine.lock().unwrap().settings()).unwrap()
}

fn run(f: &Fixture, request: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 4),
        SESSION,
        DEVICE,
        request,
        &f.engine,
        Instant::now(),
    )
}

/// A lease held under the logging grant, station control, or both.
fn lease(f: &Fixture, logging: bool, control: bool) -> Value {
    if logging {
        f.authority.permit(DEVICE, true).unwrap();
    }
    if control {
        f.authority.permit_station(DEVICE, true).unwrap();
    }
    let state = control_state_version(f, Instant::now(), 4);
    run(
        f,
        &Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        },
    )
    .unwrap()
}

fn change(f: &Fixture, revision: &str, values: Value) -> Request {
    let s = control_state_version(f, Instant::now(), 4);
    serde_json::from_value(json!({"type":"logChange","requestId":id(),"stationBootId":s["stationBootId"],
        "leaseId":s["leaseId"],"expectedRevision":s["revision"],"commandWindowId":s["commandWindowId"],
        "clientSequence":s["nextSequence"],"change":{"kind":"settings","revision":revision,"values":values}}))
    .unwrap()
}

/// Every setting except `keys` is exactly as it was.
fn only_changed(before: &Value, after: &Value, keys: &[&str]) {
    let (before, after) = (before.as_object().unwrap(), after.as_object().unwrap());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (key, value) in before {
        if !keys.contains(&key.as_str()) {
            assert_eq!(&after[key], value, "{key} must not change");
        }
    }
}

fn capabilities(state: &Value) -> Vec<Value> {
    state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn a_logging_preference_is_saved_with_a_receipt_and_a_replay_writes_nothing_twice() {
    let f = Fixture::new();
    store(&f);
    let state = lease(&f, true, false);
    assert!(capabilities(&state).contains(&json!("settingsLogging")));
    assert!(!capabilities(&state).contains(&json!("settingsControl")));
    let before = settings(&f);
    let auto = !before["autoLog"].as_bool().unwrap();
    let reports = !before["logReportsToComments"].as_bool().unwrap();
    let request = change(
        &f,
        &revision(&f),
        json!({"autoLog":auto,"logReportsToComments":reports}),
    );
    let result = run(&f, &request).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":request.id(),"outcome":"applied","evidence":"settingsSaved"})
    );
    let after = settings(&f);
    assert_eq!(after["autoLog"], auto);
    assert_eq!(after["logReportsToComments"], reports);
    only_changed(&before, &after, &["autoLog", "logReportsToComments"]);
    assert_eq!(
        saved(&f).unwrap()["autoLog"],
        auto,
        "the save reached the disk"
    );
    // A dropped reply is answered from the receipt: nothing is saved a second time.
    std::fs::remove_file(f.dir.join("settings.json")).unwrap();
    assert_eq!(run(&f, &request).unwrap(), result);
    assert_eq!(
        run(
            &f,
            &Request::Result {
                request_id: id(),
                operation_id: request.id().into()
            }
        )
        .unwrap(),
        result
    );
    assert!(saved(&f).is_none());
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn a_station_preference_needs_station_control_and_a_logging_one_needs_the_logging_grant() {
    let pick = json!({"contestCheck":"73"});
    // The logging grant alone cannot change a station preference.
    let f = Fixture::new();
    store(&f);
    lease(&f, true, false);
    assert_eq!(
        run(&f, &change(&f, &revision(&f), pick.clone())),
        Err("localPermissionRequired")
    );
    assert!(saved(&f).is_none());
    // Station control alone changes one, but not a logging preference nor the two together.
    let f = Fixture::new();
    store(&f);
    let state = lease(&f, false, true);
    assert!(capabilities(&state).contains(&json!("settingsControl")));
    assert!(!capabilities(&state).contains(&json!("settingsLogging")));
    for values in [
        json!({"autoLog":true}),
        json!({"autoLog":true,"contestCheck":"73"}),
    ] {
        assert_eq!(
            run(&f, &change(&f, &revision(&f), values)),
            Err("localPermissionRequired")
        );
    }
    assert!(saved(&f).is_none());
    let applied = run(&f, &change(&f, &revision(&f), pick.clone())).unwrap();
    assert_eq!(applied["evidence"], "settingsSaved");
    assert_eq!(f.engine.lock().unwrap().settings().contest_check, "73");
    // Both grants: one change may carry both kinds.
    let f = Fixture::new();
    store(&f);
    lease(&f, true, true);
    let before = settings(&f);
    let both = json!({"autoLog":!before["autoLog"].as_bool().unwrap(),"contestCheck":"73"});
    assert_eq!(
        run(&f, &change(&f, &revision(&f), both)).unwrap()["outcome"],
        "applied"
    );
    only_changed(&before, &settings(&f), &["autoLog", "contestCheck"]);
    // A station at operation v3 never parses one.
    assert_eq!(
        f.authority.handle_version(
            (f.connection, 3),
            SESSION,
            DEVICE,
            &change(&f, &revision(&f), pick),
            &f.engine,
            Instant::now()
        ),
        Err("stationUnsupported")
    );
}

#[test]
fn a_setting_off_the_allow_list_is_denied_before_anything_is_consumed() {
    let f = Fixture::new();
    store(&f);
    lease(&f, true, true);
    let before = settings(&f);
    let sequence = control_state_version(&f, Instant::now(), 4)["nextSequence"].clone();
    for values in [
        // Not a setting at all, shaped like one that would matter.
        json!({"rigPort":"COM3"}),
        json!({"serialPort":"/dev/ttyUSB0"}),
        json!({"rigModel":1}),
        json!({"licenseClass":"Extra"}),
        json!({"mycall":"W1AW"}),
        json!({"audioOut":"Speakers"}),
        json!({"betaUpdates":true}),
        json!({"clusterHosts":[]}),
        json!({"saveWav":"all"}),
        json!({"txEven":true}),
        json!({"dialMhz":14.074}),
        json!({"band":"20m"}),
        json!({"tunePowerPct":100}),
        json!({"alertCq":true}),
        json!({"soundTxState":true}),
        json!({"specialOp":"hound"}),
        json!({"fdActive":true}),
        json!({"lotwAutoUpload":true}),
        json!({"clublogApiKey":"secret"}),
        json!({"radios":[]}),
        json!({"contestCheck":"73","serialPort":"COM3"}),
        json!({}),
    ] {
        assert_eq!(
            run(&f, &change(&f, &revision(&f), values.clone())),
            Err("invalidRecord"),
            "{values}"
        );
    }
    assert_eq!(
        control_state_version(&f, Instant::now(), 4)["nextSequence"],
        sequence,
        "a denial spends no command"
    );
    assert_eq!(settings(&f), before);
    assert!(saved(&f).is_none());
    // Positive control: the same request naming an allowed setting is saved.
    assert_eq!(
        run(&f, &change(&f, &revision(&f), json!({"contestCheck":"73"}))).unwrap()["outcome"],
        "applied"
    );
}

#[test]
fn a_value_the_setting_cannot_hold_is_refused_and_saves_nothing() {
    let f = Fixture::new();
    store(&f);
    lease(&f, true, true);
    let before = settings(&f);
    for values in [
        json!({"decodeDepth":"2"}),
        json!({"autoLog":"yes"}),
        json!({"contestCqZone":-1}),
        json!({"contestCqZone":4.5}),
        json!({"macros":[]}),
        json!({"macros":{"chat":["73"]}}),
        json!({"units":null}),
    ] {
        let result = run(&f, &change(&f, &revision(&f), values.clone())).unwrap();
        assert_eq!(result["outcome"], "rejected", "{values}");
        assert_eq!(result["reason"], "invalidChange", "{values}");
    }
    assert_eq!(settings(&f), before);
    assert!(saved(&f).is_none());
}

#[test]
fn a_change_against_settings_that_moved_at_the_station_is_refused() {
    let f = Fixture::new();
    store(&f);
    lease(&f, true, true);
    let shown = revision(&f);
    {
        let mut e = f.engine.lock().unwrap();
        let mut moved = e.settings().clone();
        moved.cw_wpm += 1;
        e.apply_settings(moved);
    }
    let before = settings(&f);
    let result = run(&f, &change(&f, &shown, json!({"contestCheck":"73"}))).unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "contextChanged");
    assert_eq!(settings(&f), before);
    assert!(saved(&f).is_none());
    // Positive control: the revision the station shows now is accepted.
    assert_eq!(
        run(&f, &change(&f, &revision(&f), json!({"contestCheck":"73"}))).unwrap()["outcome"],
        "applied"
    );
}

/// A different value of the same JSON type, for the sweep below.
fn different(current: &Value) -> Value {
    match current {
        Value::Bool(b) => json!(!b),
        Value::Number(n) => json!(match n.as_u64() {
            Some(0) | None => 1,
            Some(v) => v - 1,
        }),
        Value::String(s) if s.is_empty() => json!("TEST"),
        Value::String(s) => json!(format!("{s}X")),
        Value::Object(fields) => {
            let mut fields = fields.clone();
            fields.insert("chat".into(), json!(["TEST"]));
            Value::Object(fields)
        }
        other => panic!("no test value for {other}"),
    }
}

/// Each allowed setting, changed alone, with the TX-enable latch off and on: it is saved, nothing
/// else moves, the latch stays where it was, and the dial, band, mode and tier do not change.
#[test]
fn no_allowed_setting_keys_tunes_or_moves_the_tx_enable_latch() {
    let writable: Vec<&str> = super::super::super::query::WRITABLE_CONTROL_KEYS
        .iter()
        .chain(super::super::super::query::WRITABLE_LOGGING_KEYS)
        .copied()
        .collect();
    assert!(
        writable.len() > 20,
        "positive control: the allow-list is really read"
    );
    for latch in [false, true] {
        let f = Fixture::new();
        store(&f);
        lease(&f, true, true);
        {
            let mut e = f.engine.lock().unwrap();
            e.set_tx_enabled(latch);
            e.take_immediate_retune();
            assert_eq!(
                e.tx_enabled(),
                latch,
                "positive control: the latch really is {latch}"
            );
        }
        for key in &writable {
            let before = settings(&f);
            let (dial, band, mode, tier) = {
                let e = f.engine.lock().unwrap();
                (
                    e.settings().dial_hz(),
                    e.settings().band.clone(),
                    e.settings().operating_mode,
                    e.tier(),
                )
            };
            let value = different(&before[*key]);
            let mut values = serde_json::Map::new();
            values.insert((*key).into(), value.clone());
            let result = run(&f, &change(&f, &revision(&f), Value::Object(values))).unwrap();
            assert_eq!(result["evidence"], "settingsSaved", "{key}: {result}");
            let after = settings(&f);
            assert_eq!(after[*key], value, "{key}");
            only_changed(&before, &after, &[key]);
            let mut e = f.engine.lock().unwrap();
            assert_eq!(e.tx_enabled(), latch, "{key} moved the TX-enable latch");
            assert_eq!(e.settings().dial_hz(), dial, "{key}");
            assert_eq!(e.settings().band, band, "{key}");
            assert_eq!(e.settings().operating_mode, mode, "{key}");
            assert_eq!(e.tier(), tier, "{key}");
            assert!(!e.take_immediate_retune(), "{key} queued a retune");
            assert!(!e.remote_ft_tx_owned(), "{key}");
        }
    }
}
