//! Hosted admission and receipt recovery. Physical owner behavior is exercised
//! separately in tempo-audio's two-radio worker tests.
use super::receiver_filter::run;
use super::*;

fn station(ready: bool) -> (Fixture, u32) {
    let f = Fixture::new();
    let incoming = {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.configure_remote_selection_host(ready);
        let incoming = e.add_radio();
        e.set_active_radio(0);
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        e.settings().save(&f.dir.join("settings.json")).unwrap();
        let connection = e.remote_open_radio().unwrap();
        let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
        let hz = e.settings().dial_hz();
        let mode = e.rig_mode_effective();
        e.remote_observe_cat(Some(&read), Some(true));
        e.remote_observe_dial(Some(&read), Some(hz));
        e.remote_observe_mode(Some(&read), Some(&mode));
        e.remote_observe_ptt(Some(&read), Some(false));
        incoming
    };
    (f, incoming)
}

#[test]
fn radio_selection_uses_one_guarded_native_request_and_recovers_its_durable_receipt() {
    let (f, incoming) = station(true);
    let before = std::fs::read(f.dir.join("settings.json")).unwrap();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let command = control_request(&state, json!({"action":"radio.select","radioId":incoming}));
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let pending = run(&f, 3, &command).unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(run(&f, 3, &command).unwrap(), pending);
    assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), before);
    let request = f
        .engine
        .lock()
        .unwrap()
        .take_remote_radio_selection()
        .unwrap();
    assert_eq!(request.settings().active_radio, incoming);
    let mode = request.settings().rig_mode();
    let readback = tempo_app::engine::remote_selection::Readback {
        radio: incoming,
        dial_hz: request.settings().dial_hz(),
        mode: &mode,
        sampled_at: Instant::now(),
    };
    let decoder = loop {
        if let Some(guard) = tempo_app::engine::remote_selection::Ft8A7ResetGuard::try_acquire() {
            break guard;
        }
        std::thread::yield_now();
    };
    assert!(request.commit_with_install(&mut f.engine.lock().unwrap(), readback, decoder, |_| {}));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "radioReadback");
    assert_eq!(
        tempo_app::settings::Settings::load(&f.dir.join("settings.json")).active_radio,
        incoming
    );
    assert!(f
        .engine
        .lock()
        .unwrap()
        .take_remote_radio_selection()
        .is_none());
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
}

#[test]
fn radio_selection_needs_host_wiring_and_station_control_permission() {
    for ready in [false, true] {
        let (f, incoming) = station(ready);
        let state = if ready {
            f.acquire(Instant::now());
            control_state_version(&f, Instant::now(), 3)
        } else {
            acquire_controls_version(&f, Instant::now(), 3)
        };
        let command = control_request(&state, json!({"action":"radio.select","radioId":incoming}));
        let result = run(&f, 3, &command);
        if ready {
            assert_eq!(result, Err("localPermissionRequired"));
        } else {
            let result = result.unwrap();
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "unsupportedAction");
        }
        assert!(f
            .engine
            .lock()
            .unwrap()
            .take_remote_radio_selection()
            .is_none());
        assert_eq!(f.engine.lock().unwrap().settings().active_radio, 0);
    }
}

#[test]
fn radio_selection_revocation_cancels_the_taken_request_and_does_not_change_the_profile() {
    let (f, incoming) = station(true);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let command = control_request(&state, json!({"action":"radio.select","radioId":incoming}));
    assert_eq!(run(&f, 3, &command).unwrap()["outcome"], "pending");
    let request = f
        .engine
        .lock()
        .unwrap()
        .take_remote_radio_selection()
        .unwrap();
    f.authority.invalidate();
    assert!(request.validate(&f.engine.lock().unwrap()).is_err());
    drop(request);
    assert_eq!(f.engine.lock().unwrap().settings().active_radio, 0);
}
