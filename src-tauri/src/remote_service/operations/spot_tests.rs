use super::receiver_filter::{run, station};
use super::*;

fn sample(
    f: &Fixture,
    connection: &tempo_app::remote_monitor::provenance::Connection,
    hz: u64,
    mode: &str,
) {
    let mut engine = f.engine.lock().unwrap();
    let read = engine
        .remote_radio_read(connection, Instant::now())
        .unwrap();
    engine.remote_observe_cat(Some(&read), Some(true));
    engine.remote_observe_dial(Some(&read), Some(hz));
    engine.remote_observe_mode(Some(&read), Some(mode));
    engine.remote_observe_ptt(Some(&read), Some(false));
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

#[test]
fn remote_spot_requires_v3_and_readback_before_native_handoff_without_replay() {
    for (mode, dial, target) in [("cw", 7.02345, "CW"), ("phone", 7.19876, "LSB")] {
        let (mut f, connection) = station("phone", 2400);
        f.authority.spots = Some(Default::default());
        let before = std::fs::read(f.dir.join("settings.json")).unwrap();
        let tick = f.engine.lock().unwrap().snapshot().work_tick;
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("workSpot")));
        let command = control_request(
            &state,
            json!({"action":"radio.workSpot","mode":mode,"dialMhz":dial,"band":"40m","call":"n2spot/p"}),
        );
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
        let pending = run(&f, 3, &command).unwrap();
        assert_eq!(pending["outcome"], "pending");
        assert_eq!(run(&f, 3, &command).unwrap(), pending);
        assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), before);
        let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
        assert_eq!(work.target(), ((dial * 1e6).round() as u64, target));
        assert_eq!(f.engine.lock().unwrap().snapshot().work_tick, tick);
        work.permission().begin_write(Instant::now()).unwrap();
        sample(&f, &connection, (dial * 1e6).round() as u64, target);
        let power = work.power_limit();
        assert!(work.commit_readback(&mut f.engine.lock().unwrap(), power));
        let applied = run(&f, 3, &command).unwrap();
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(applied["evidence"], "radioReadback");
        let after = std::fs::read(f.dir.join("settings.json")).unwrap();
        assert_ne!(after, before);
        assert_eq!(run(&f, 3, &command).unwrap(), applied);
        let mut engine = f.engine.lock().unwrap();
        assert!(engine.take_remote_radio().is_none());
        assert!(!engine.take_immediate_retune());
        assert!(!engine.tx_enabled());
        assert_eq!(engine.snapshot().work_tick, tick + 1);
        assert_eq!(engine.snapshot().work_call.as_deref(), Some("N2SPOT/P"));
        assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), after);
    }
}

#[test]
fn remote_spot_cannot_discard_station_split_or_treat_unavailable_evidence_as_simplex() {
    for evidence in ["missing", "busy", "split", "simplex"] {
        let (mut f, _connection) = station("phone", 2400);
        let spots: crate::SharedSpots = Default::default();
        if evidence == "split" {
            spots
                .lock()
                .unwrap()
                .push(cluster("N2SPOT/P", 14023.0, "UP 2"));
        }
        if evidence != "missing" {
            f.authority.spots = Some(spots.clone());
        }
        let _held = (evidence == "busy").then(|| spots.lock().unwrap());
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(
            &state,
            json!({"action":"radio.workSpot","mode":"cw","dialMhz":14.023,"band":"20m","call":"N2SPOT"}),
        );
        let result = run(&f, 3, &command).unwrap();
        if evidence == "simplex" {
            assert_eq!(
                result["outcome"], "pending",
                "empty available evidence is the positive control"
            );
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
fn desktop_and_remote_split_lookup_share_call_boundary_frequency_and_age_rules() {
    let now = Instant::now();
    let mut spots = tempo_net::cluster::SpotBuffer::default();
    spots.push_at(now, cluster("N2SPOT/P", 14023.0, "UP 2"));
    assert_eq!(
        crate::work_spot_split_offset(&spots, "n2spot", 14.023, now),
        Some(2.0)
    );
    assert_eq!(
        crate::work_spot_split_offset(&spots, "N2SPO", 14.023, now),
        None
    );
    assert_eq!(
        crate::work_spot_split_offset(&spots, "N2SPOT", 7.023, now),
        None
    );
    assert_eq!(
        crate::work_spot_split_offset(&spots, "N2SPOT", 14.023, now + Duration::from_secs(1801)),
        None
    );
}

#[test]
fn remote_spot_rejects_logging_only_authority_and_extra_native_arguments() {
    let (f, _connection) = station("phone", 2400);
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    let action = json!({"action":"radio.workSpot","mode":"cw","dialMhz":14.023,"band":"20m","call":"N2SPOT"});
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())),
        Err("localPermissionRequired")
    );
    for field in [
        "splitUpKhz",
        "tier",
        "settings",
        "command",
        "txEnabled",
        "radioId",
    ] {
        let mut invalid = action.clone();
        invalid[field] = json!(true);
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    }
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
}
