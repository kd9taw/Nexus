//! Remote Sub-receiver levels (`radio.subLevel`, capability `subReceiverLevels`): RF gain, AF
//! gain and squelch on a dual-receiver radio's second receiver. A receive-side one-shot the
//! station hands to the same engine verb the desktop's Sub row calls, answered `stationState` —
//! the station took the request; what the radio accepted reaches the page in the snapshot, and
//! nothing reads the Sub back, so no receipt claims a read-back. Nothing here may arm or key.
use super::*;
use tempo_app::engine::sub_controls::SubLevel;

/// A Phone station on Hamlib `model` at 14.200 with a fresh, unkeyed CAT reading, whose radio
/// loop has reported whether its CAT path can name the Sub (`route`).
fn station(model: u32, route: bool) -> Fixture {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    let mut settings = e.settings().clone();
    settings.rig_model = model;
    e.apply_settings(settings);
    assert_eq!(e.settings().rig_model, model, "the configuration took");
    e.set_operating_mode("phone", false);
    e.set_frequency(14.2, "20m", "USB");
    e.set_tx_enabled(false);
    e.take_immediate_retune();
    assert!(
        e.take_sub_level_requests(route).is_empty(),
        "nothing queued yet"
    );
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

/// What the radio loop would be handed, as (level, value).
fn drained(f: &Fixture) -> Vec<(SubLevel, f32)> {
    f.engine
        .lock()
        .unwrap()
        .take_sub_level_requests(true)
        .into_iter()
        .map(|w| (w.level, w.value))
        .collect()
}

#[test]
fn a_sub_level_needs_v3_reaches_the_sub_once_and_never_keys() {
    for (name, level) in [
        ("rfGain", SubLevel::Rf),
        ("afGain", SubLevel::Af),
        ("squelch", SubLevel::Sql),
    ] {
        let f = station(3078, true); // IC-7610 on Nexus's own CI-V control
        let bytes = std::fs::read(f.dir.join("settings.json")).ok();
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("subReceiverLevels")));
        let command = control_request(
            &state,
            json!({"action":"radio.subLevel","level":name,"value":0.25}),
        );
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"), "{name}");
        assert!(drained(&f).is_empty(), "{name}: v2 queued nothing");
        let applied = run(&f, 3, &command).unwrap();
        assert_eq!(applied["outcome"], "applied", "{name}");
        assert_eq!(applied["evidence"], "stationState", "{name}");
        assert_eq!(drained(&f), vec![(level, 0.25)], "{name}: the Sub's, once");
        // A replay returns the same receipt and queues nothing a second time.
        assert_eq!(run(&f, 3, &command).unwrap(), applied, "{name}");
        assert!(drained(&f).is_empty(), "{name}: replay");
        let e = f.engine.lock().unwrap();
        assert!(!e.tx_enabled(), "{name} leaves the latch down");
        assert!(e.tx_owner().is_none(), "{name}");
        assert_eq!(e.af_gain(), None, "{name}: Main's AF never moves");
        assert_eq!(e.rf_gain(), None, "{name}: Main's RF never moves");
        assert_eq!(e.squelch(), None, "{name}: Main's squelch never moves");
        drop(e);
        assert_eq!(
            std::fs::read(f.dir.join("settings.json")).ok(),
            bytes,
            "{name} saves nothing"
        );
    }
}

/// ⭐ The desktop's refusals, through the Remote contract: each is `hardwareUnavailable` and
/// queues nothing — and a level the grammar does not name is `invalidAction`.
#[test]
fn a_sub_level_the_station_cannot_carry_is_refused_and_queues_nothing() {
    for (why, model, route, level) in [
        ("no Sub offered (IC-7300)", 3073u32, true, "rfGain"),
        (
            "IC-9700 AF: not confirmed for its Sub",
            3081,
            true,
            "afGain",
        ),
        ("a CAT path that cannot name the Sub", 3078, false, "afGain"),
    ] {
        let f = station(model, route);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(
            &f,
            3,
            &control_request(
                &state,
                json!({"action":"radio.subLevel","level":level,"value":0.5}),
            ),
        )
        .unwrap();
        assert_eq!(result["outcome"], "rejected", "{why}");
        assert_eq!(result["reason"], "hardwareUnavailable", "{why}");
        assert!(drained(&f).is_empty(), "{why}");
    }
    for level in ["AF", "micGain", "power", ""] {
        let f = station(3078, true);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(
            &f,
            3,
            &control_request(
                &state,
                json!({"action":"radio.subLevel","level":level,"value":0.5}),
            ),
        )
        .unwrap();
        assert_eq!(result["reason"], "invalidAction", "{level:?}");
        assert!(drained(&f).is_empty(), "{level:?}");
    }
    // Positive control: the IC-9700's RF gain is its Sub's own front end.
    let f = station(3081, true);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let result = run(
        &f,
        3,
        &control_request(
            &state,
            json!({"action":"radio.subLevel","level":"rfGain","value":0.5}),
        ),
    )
    .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(drained(&f), vec![(SubLevel::Rf, 0.5)]);
}

/// Closed grammar: no field beyond the level and its value, and logging-only authority is not
/// station control.
#[test]
fn a_sub_level_refuses_open_fields_and_logging_only_authority() {
    let action = json!({"action":"radio.subLevel","level":"afGain","value":0.5});
    for field in ["mode", "expected", "receiver", "txEnabled", "radioId"] {
        let mut bad = action.clone();
        bad[field] = json!(true);
        assert!(
            serde_json::from_value::<station::Action>(bad).is_err(),
            "{field} must not parse"
        );
    }
    assert!(serde_json::from_value::<station::Action>(action.clone()).is_ok());
    let f = station(3078, true);
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action)),
        Err("localPermissionRequired")
    );
    assert!(drained(&f).is_empty());
}
