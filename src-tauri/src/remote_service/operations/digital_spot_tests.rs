//! FT8/FT4 click-to-work over Remote: its own v3 action and hint, the CW/Phone Work intent
//! unchanged, and no transmit authority as a side effect.
use super::receiver_filter::run;
use super::*;
use tempo_app::dto::Tier;
use tempo_app::remote_monitor::provenance::Connection;

fn station(tier: Tier) -> (Fixture, Connection) {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.set_tier(tier);
    e.set_operating_mode("digital", false);
    e.set_tx_enabled(false);
    e.take_immediate_retune();
    let connection = e.remote_open_radio().unwrap();
    drop(e);
    let (hz, mode) = current(&f);
    sample(&f, &connection, hz, &mode);
    (f, connection)
}

fn current(f: &Fixture) -> (u64, String) {
    let e = f.engine.lock().unwrap();
    (e.settings().dial_hz(), e.rig_mode_effective())
}

fn sample(f: &Fixture, connection: &Connection, hz: u64, mode: &str) {
    let mut e = f.engine.lock().unwrap();
    let read = e.remote_radio_read(connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_dial(Some(&read), Some(hz));
    e.remote_observe_mode(Some(&read), Some(mode));
    e.remote_observe_ptt(Some(&read), Some(false));
}

fn cluster(call: &str, freq_khz: f64, comment: &str) -> tempo_net::cluster::ClusterSpot {
    tempo_net::cluster::ClusterSpot {
        spotter: "W3LPL".into(),
        dx_call: call.into(),
        freq_khz,
        comment: comment.into(),
        time_utc: None,
        received_unix: 0,
        corroborators: Vec::new(),
        rbn: false,
    }
}

fn action() -> Value {
    json!({"action":"radio.workDigitalSpot","tier":"FT4","dialMhz":14.0815,"band":"20m","call":"ja2def/p"})
}

#[test]
fn digital_spot_needs_v3_its_hint_and_readback_and_never_arms_transmit() {
    let (mut f, connection) = station(Tier::Ft8);
    f.authority.spots = Some(Default::default());
    let before = std::fs::read(f.dir.join("settings.json")).ok();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("workDigitalSpot")));
    let command = control_request(&state, action());
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let pending = run(&f, 3, &command).unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(run(&f, 3, &command).unwrap(), pending);
    assert_eq!(std::fs::read(f.dir.join("settings.json")).ok(), before);
    let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
    assert_eq!(work.target().0, 14_081_500);
    assert_eq!(f.engine.lock().unwrap().tier(), Tier::Ft8);
    work.permission().begin_write(Instant::now()).unwrap();
    let target = work.target().1.to_owned();
    sample(&f, &connection, 14_081_500, &target);
    let power = work.power_limit();
    assert!(work.commit_readback(&mut f.engine.lock().unwrap(), power));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "radioReadback");
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    let mut engine = f.engine.lock().unwrap();
    assert!(
        engine.take_remote_radio().is_none(),
        "one receipt, no replay"
    );
    assert_eq!(engine.tier(), Tier::Ft4);
    assert!(!engine.tx_enabled());
    assert!(!engine.remote_ft_tx_owned());
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.work_call.as_deref(), Some("JA2DEF/P"));
    assert!(
        snapshot.qso.as_ref().is_none_or(|q| q.dxcall.is_none()),
        "Work tunes; it does not call the station"
    );
}

#[test]
fn digital_spot_is_refused_while_transmit_is_enabled_with_the_idle_request_as_control() {
    for armed in [false, true] {
        let (mut f, connection) = station(Tier::Ft8);
        f.authority.spots = Some(Default::default());
        if armed {
            let mut e = f.engine.lock().unwrap();
            e.set_tx_enabled(true);
            e.take_immediate_retune();
        }
        let (hz, mode) = current(&f);
        sample(&f, &connection, hz, &mode);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(&f, 3, &control_request(&state, action())).unwrap();
        let mut engine = f.engine.lock().unwrap();
        if armed {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "stationBusy");
            assert!(engine.take_remote_radio().is_none());
            assert!(
                engine.tx_enabled(),
                "a refusal leaves the operator's latch alone"
            );
        } else {
            assert_eq!(result["outcome"], "pending");
            assert!(engine.take_remote_radio().is_some());
            assert!(!engine.tx_enabled());
        }
        assert_eq!(engine.tier(), Tier::Ft8);
    }
}

#[test]
fn digital_spot_cannot_discard_a_station_resolved_pile_up_split() {
    for evidence in ["missing", "split", "simplex"] {
        let (mut f, _connection) = station(Tier::Ft8);
        let spots: crate::SharedSpots = Default::default();
        if evidence == "split" {
            spots
                .lock()
                .unwrap()
                .push(cluster("JA2DEF/P", 14081.5, "UP 2"));
        }
        if evidence != "missing" {
            f.authority.spots = Some(spots.clone());
        }
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(&f, 3, &control_request(&state, action())).unwrap();
        if evidence == "simplex" {
            assert_eq!(result["outcome"], "pending", "the positive control");
        } else {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(
                result["reason"],
                if evidence == "split" {
                    "unsupportedAction"
                } else {
                    "readingUnavailable"
                }
            );
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        }
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}

#[test]
fn digital_spot_rejects_logging_only_authority_and_any_other_native_argument() {
    let (mut f, _connection) = station(Tier::Ft8);
    f.authority.spots = Some(Default::default());
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action())),
        Err("localPermissionRequired")
    );
    for field in [
        "mode",
        "splitUpKhz",
        "settings",
        "command",
        "txEnabled",
        "radioId",
    ] {
        let mut invalid = action();
        invalid[field] = json!(true);
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    }
    for field in ["tier", "dialMhz", "band", "call"] {
        let mut missing = action();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<station::Action>(missing).is_err());
    }
    // The CW/Phone Work intent an older desktop parses is unchanged: it still names no tier.
    let mut widened = json!({"action":"radio.workSpot","mode":"cw","dialMhz":14.023,"band":"20m","call":"N2SPOT"});
    assert!(serde_json::from_value::<station::Action>(widened.clone()).is_ok());
    widened["tier"] = json!("FT8");
    assert!(serde_json::from_value::<station::Action>(widened).is_err());
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
}
