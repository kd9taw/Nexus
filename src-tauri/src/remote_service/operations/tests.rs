use super::*;
use std::sync::Arc;
const DEVICE: &str = "10000000-0000-4000-8000-000000000001";
const SESSION: &str = "20000000-0000-4000-8000-000000000001";
const OTHER: &str = "30000000-0000-4000-8000-000000000001";
fn id() -> String {
    super::super::query::snapshot_id().unwrap()
}
struct Fixture {
    authority: Authority,
    engine: crate::SharedEngine,
    connection: u64,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("nexus-log-operation-{}", id()));
        std::fs::create_dir(&dir).unwrap();
        let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
        let mut settings = engine.settings().clone();
        settings.save_qso_wav = false;
        engine.apply_settings(settings);
        engine.set_log_path(dir.join("contacts.adi"));
        let authority = Authority::default();
        let connection = authority.start_connection();
        Self {
            authority,
            engine: Arc::new(Mutex::new(engine)),
            connection,
            dir,
        }
    }
    fn run(&self, request: &Request, now: Instant) -> Result<Value, &'static str> {
        self.authority
            .handle(self.connection, SESSION, DEVICE, request, &self.engine, now)
    }
    fn state(&self, now: Instant) -> Value {
        self.run(&Request::State { request_id: id() }, now).unwrap()
    }
    fn acquire(&self, now: Instant) -> Value {
        self.authority.permit(DEVICE, true).unwrap();
        self.run(
            &Request::Acquire {
                request_id: id(),
                station_boot_id: self.state(now)["stationBootId"].as_str().unwrap().into(),
            },
            now,
        )
        .unwrap()
    }
    fn command(&self, state: &Value) -> Request {
        serde_json::from_value(json!({"type":"logManual","requestId":id(),"stationBootId":state["stationBootId"],"leaseId":state["leaseId"],"expectedRevision":state["revision"],"commandWindowId":state["commandWindowId"],"clientSequence":state["nextSequence"],"record":{"call":"W1AW","grid":"FN31","country":null,"state":"CT","band":"20m","freqMhz":14.25,"mode":"SSB","rstSent":"59","rstRcvd":"57","name":"Joe","qth":"Newington","comment":"Remote test","notes":"Keep this note","whenUnix":super::super::now_ms()/1000,"confirmed":false,"awardConfirmed":false}})).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn control_state(f: &Fixture, now: Instant) -> Value {
    f.authority
        .handle_version(
            (f.connection, 2),
            SESSION,
            DEVICE,
            &Request::State { request_id: id() },
            &f.engine,
            now,
        )
        .unwrap()
}

fn acquire_controls(f: &Fixture, now: Instant) -> Value {
    f.authority.permit_station(DEVICE, true).unwrap();
    f.authority
        .handle_version(
            (f.connection, 2),
            SESSION,
            DEVICE,
            &Request::Acquire {
                request_id: id(),
                station_boot_id: control_state(f, now)["stationBootId"]
                    .as_str()
                    .unwrap()
                    .into(),
            },
            &f.engine,
            now,
        )
        .unwrap()
}

fn control_request(state: &Value, action: Value) -> Request {
    serde_json::from_value(json!({"type":"stationControl", "requestId":id(), "stationBootId":state["stationBootId"],
        "leaseId":state["leaseId"], "expectedRevision":state["revision"], "commandWindowId":state["commandWindowId"],
        "clientSequence":state["nextSequence"], "context":state["controls"]["context"], "action":action})).unwrap()
}

#[test]
#[cfg(feature = "radio")]
fn frequency_admission_waits_for_the_radio_owner_and_revoke_cancels_the_pending_write() {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        e.configure_remote_settings_store(f.dir.join("settings.json"));
        e.set_tx_enabled(false);
        e.set_frequency(14.074, "20m", "USB");
        e.take_immediate_retune();
        let connection = e.remote_open_radio().unwrap();
        let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
        e.remote_observe_cat(Some(&read), Some(true));
        e.remote_observe_dial(Some(&read), Some(14_074_000));
        e.remote_observe_mode(Some(&read), Some("PKTUSB"));
        e.remote_observe_ptt(Some(&read), Some(false));
    }
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let command = control_request(
        &state,
        json!({"action":"radio.frequency","dialMhz":7.074,"band":"40m","sideband":"USB"}),
    );
    let response = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &command, &f.engine, now)
        .unwrap();
    assert_eq!(response["outcome"], "pending");
    let work = {
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.settings().dial_hz(), 14_074_000);
        assert!(!e.take_immediate_retune());
        e.take_remote_frequency().unwrap()
    };
    assert_eq!(work.target(), (7_074_000, "PKTUSB"));
    assert!(work.permission().check(Instant::now()).is_ok());
    assert!(!f.dir.join("settings.json").exists());
    f.authority.permit_station(DEVICE, false).unwrap();
    assert!(work.permission().begin_write(Instant::now()).is_err());
    assert_eq!(f.engine.lock().unwrap().settings().dial_hz(), 14_074_000);
}

#[test]
#[cfg(feature = "radio")]
fn an_amplifier_command_has_one_native_receipt_and_needs_later_hardware_confirmation() {
    let f = Fixture::new();
    let (radio, amp) = {
        let mut e = f.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.ensure_radio_profiles();
        let active = settings.active_radio;
        let p = settings.radios.iter_mut().find(|p| p.id == active).unwrap();
        p.amp_model = "spe".into();
        p.amp_port = "fake-remote-amp".into();
        settings.sync_flat_from_active();
        e.apply_settings(settings);
        e.set_tx_enabled(false);
        (e.remote_open_radio().unwrap(), e.remote_open_amp().unwrap())
    };
    let sample = |operate| {
        let mut e = f.engine.lock().unwrap();
        let r = e.remote_radio_read(&radio, Instant::now()).unwrap();
        e.remote_observe_cat(Some(&r), Some(true));
        e.remote_observe_ptt(Some(&r), Some(false));
        let r = e.remote_amp_read(&amp, Instant::now()).unwrap();
        e.remote_observe_amp(
            Some(&r),
            tempo_app::dto::AmpStatusDto {
                family: "spe".into(),
                linked: true,
                operate: Some(operate),
                transmitting: Some(false),
                output_watts: Some(0),
                band_label: Some("20m".into()),
                ..Default::default()
            },
        );
    };
    sample(false);
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let command = control_request(
        &state,
        json!({"action":"amplifier.operate","expectedOperate":false,"operate":true}),
    );
    let run = |request: &Request| {
        f.authority.handle_version(
            (f.connection, 2),
            SESSION,
            DEVICE,
            request,
            &f.engine,
            Instant::now(),
        )
    };
    let first = run(&command).unwrap();
    assert_eq!(first["outcome"], "pending");
    assert_eq!(run(&command).unwrap(), first);
    let mut request = f.engine.lock().unwrap().take_remote_amp().unwrap();
    assert!(
        f.engine.lock().unwrap().take_remote_amp().is_none(),
        "a duplicate cannot enqueue a second toggle"
    );
    let result = Request::Result {
        request_id: id(),
        operation_id: command.id().into(),
    };
    assert_eq!(run(&result).unwrap()["outcome"], "pending");
    sample(false);
    request.begin(&f.engine.lock().unwrap()).unwrap();
    assert!(request.begin_write());
    sample(true);
    request.confirm(&f.engine.lock().unwrap());
    let confirmed = run(&result).unwrap();
    assert_eq!(confirmed["outcome"], "applied");
    assert_eq!(confirmed["evidence"], "amplifierReadback");
    assert_eq!(run(&command).unwrap(), confirmed);
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn logging_and_station_permissions_do_not_grant_each_other() {
    let f = Fixture::new();
    let now = Instant::now();
    f.acquire(now);
    let command = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.arm","receiver":"rtty","on":true}),
    );
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &command, &f.engine, now),
        Err("localPermissionRequired")
    );
    assert!(!f.engine.lock().unwrap().rtty_armed());
    f.authority.permit(DEVICE, false).unwrap();
    let control = acquire_controls(&f, now);
    #[cfg(feature = "radio")]
    assert_eq!(
        control["controls"]["capabilities"],
        json!(["decoder", "amplifier", "frequency"])
    );
    #[cfg(not(feature = "radio"))]
    assert_eq!(control["controls"]["capabilities"], json!(["decoder"]));
    assert_eq!(control["actions"], json!([]));
    assert_eq!(control["txArmed"], false);
    assert_eq!(
        f.run(&f.command(&control), now),
        Err("localPermissionRequired")
    );
    assert_eq!(
        f.run(
            &control_request(&control, json!({"action":"decoder.clear","receiver":"cw"})),
            now
        ),
        Err("stationUnsupported")
    );
}

#[test]
fn remote_receiver_gestures_use_real_native_state_without_arming_transmit() {
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls(&f, now);
    for receiver in ["rtty", "psk", "sstv", "aprs"] {
        for on in [true, false, true] {
            let request = control_request(
                &control_state(&f, now),
                json!({"action":"decoder.arm","receiver":receiver,"on":on}),
            );
            let r = f
                .authority
                .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
                .unwrap();
            assert_eq!(r["outcome"], "applied");
            assert_eq!(r["evidence"], "receiverState");
            let engine = f.engine.lock().unwrap();
            let armed = match receiver {
                "rtty" => engine.rtty_armed(),
                "psk" => engine.psk_armed(),
                "sstv" => engine.sstv_armed(),
                _ => engine.aprs_armed(),
            };
            assert_eq!(armed, on);
            assert!(!engine.tx_enabled());
            if receiver == "aprs" && on {
                assert_eq!(engine.aprs_arm_source(), tempo_app::engine::AprsArm::Auto);
            }
        }
    }
}

#[test]
fn a_replayed_clear_does_not_erase_text_received_after_the_original_gesture() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let request = control_request(&state, json!({"action":"decoder.clear","receiver":"rtty"}));
    let applied = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    f.engine.lock().unwrap().push_rtty_decode(
        &[tempo_core::textmode::DecodedChar {
            ch: 'X',
            confidence: 1.0,
        }],
        0.0,
        false,
    );
    let replay = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(replay, applied);
    assert_eq!(f.engine.lock().unwrap().rtty_state().text, "X");
}

#[test]
fn receiver_net_reacquire_and_psk_mode_reach_the_decoder_without_changing_local_tx() {
    let f = Fixture::new();
    let now = Instant::now();
    f.engine.lock().unwrap().set_tx_enabled(true);
    acquire_controls(&f, now);
    for receiver in ["rtty", "psk"] {
        for action in [
            json!({"action":"decoder.net","receiver":receiver,"hz":1625.0}),
            json!({"action":"decoder.afcReset","receiver":receiver}),
        ] {
            let request = control_request(&control_state(&f, now), action);
            let result = f
                .authority
                .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
                .unwrap();
            assert_eq!(result["outcome"], "applied");
            let mut e = f.engine.lock().unwrap();
            if receiver == "rtty" {
                assert_eq!(e.rtty_center_hz(), 1625.0);
                assert!(e.take_rtty_afc_reset());
            } else {
                assert_eq!(e.psk_center_hz(), 1625.0);
                assert!(e.take_psk_afc_reset());
            }
            assert!(e.tx_enabled());
        }
    }
    let request = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.pskMode","mode":"QPSK31","reverse":true}),
    );
    let result = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now)
        .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(
        f.engine.lock().unwrap().psk_mode(),
        (tempo_core::psk::PskModeKind::Qpsk31, true)
    );
    let invalid = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.net","receiver":"psk","hz":1.0}),
    );
    let result = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &invalid, &f.engine, now)
        .unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "invalidAction");
    assert_eq!(f.engine.lock().unwrap().psk_center_hz(), 1625.0);
}

#[test]
fn receiver_control_refuses_local_takeover_and_an_old_click_context() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let request = control_request(
        &state,
        json!({"action":"decoder.arm","receiver":"psk","on":true}),
    );
    f.engine.lock().unwrap().set_frequency(7.074, "40m", "USB");
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now),
        Err("staleContext")
    );
    assert!(!f.engine.lock().unwrap().psk_armed());
    let request = control_request(
        &control_state(&f, now),
        json!({"action":"decoder.arm","receiver":"psk","on":true}),
    );
    f.authority.invalidate();
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now),
        Err("localPermissionRequired")
    );
    assert!(!f.engine.lock().unwrap().psk_armed());
}

#[test]
fn local_amp_gestures_invalidate_old_windows_without_changing_the_tx_context() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = acquire_controls(&f, now);
    let action = json!({"action":"decoder.arm","receiver":"psk","on":true});
    let request = control_request(&state, action.clone());
    {
        let e = f.engine.lock().unwrap();
        let tx_generation = e.remote_log_context_generation();
        let actuation = e.remote_actuation_context_generation();
        e.note_local_amplifier_command();
        e.note_local_amplifier_command();
        assert_eq!(e.remote_log_context_generation(), tx_generation);
        assert_eq!(e.remote_actuation_context_generation(), actuation + 2);
    }
    assert_eq!(
        f.authority
            .handle_version((f.connection, 2), SESSION, DEVICE, &request, &f.engine, now),
        Err("staleContext")
    );
    assert!(!f.engine.lock().unwrap().psk_armed());
    let fresh = control_request(&control_state(&f, now), action);
    let result = f
        .authority
        .handle_version((f.connection, 2), SESSION, DEVICE, &fresh, &f.engine, now)
        .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert!(f.engine.lock().unwrap().psk_armed());
}

#[test]
fn a_receive_only_aprs_gesture_never_upgrades_the_ack_interlock() {
    let mut engine = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    engine.set_tx_enabled(true);
    for _ in 0..2 {
        engine.set_aprs_receive_only(false);
        engine.set_aprs_receive_only(true);
        assert_eq!(engine.aprs_arm_source(), tempo_app::engine::AprsArm::Auto);
        assert!(
            engine.tx_enabled(),
            "a receiver gesture preserves the existing local TX choice"
        );
    }
}
#[test]
fn manual_logging_requires_local_permission_and_one_controller() {
    let f = Fixture::new();
    let now = Instant::now();
    let s = f.state(now);
    assert_eq!(s["phase"], "localPermissionRequired");
    assert_eq!(s["txArmed"], false);
    assert_eq!(
        f.run(
            &Request::Acquire {
                request_id: id(),
                station_boot_id: s["stationBootId"].as_str().unwrap().into()
            },
            now
        ),
        Err("localPermissionRequired")
    );
    let s = f.acquire(now);
    assert_eq!(s["phase"], "controlling");
    f.authority.permit(OTHER, true).unwrap();
    assert_eq!(
        f.authority.handle(
            f.connection,
            OTHER,
            OTHER,
            &Request::Acquire {
                request_id: id(),
                station_boot_id: s["stationBootId"].as_str().unwrap().into()
            },
            &f.engine,
            now
        ),
        Err("controllerBusy")
    );
    assert!(f.engine.lock().unwrap().log_records().is_empty());
}
#[test]
fn manual_logging_syncs_the_actual_adif_and_returns_one_receipt_on_replay() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = f.acquire(now);
    let request = f.command(&state);
    let before = f.engine.lock().unwrap().settings().clone();
    let result = f.run(&request, now).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "fileSynced");
    let bytes = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    let adif = String::from_utf8(bytes.clone()).unwrap();
    assert!(adif.contains("W1AW") && adif.contains("Keep this note"));
    assert_eq!(
        f.run(&request, now + Duration::from_secs(8)).unwrap(),
        result
    );
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), bytes);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
    let mut reloaded = tempo_app::engine::Engine::new("W9XYZ", "EN52", 0);
    reloaded.set_log_path(f.dir.join("contacts.adi"));
    assert_eq!(reloaded.log_records().len(), 1);
    assert_eq!(reloaded.log_records()[0].call, "W1AW");
    assert_eq!(
        serde_json::to_value(f.engine.lock().unwrap().settings()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let query = Request::Result {
        request_id: id(),
        operation_id: request.id().into(),
    };
    assert_eq!(
        f.authority
            .handle(f.connection, OTHER, DEVICE, &query, &f.engine, now)
            .unwrap(),
        result
    );
    assert_eq!(
        f.authority
            .handle(f.connection, OTHER, OTHER, &query, &f.engine, now),
        Err("localPermissionRequired")
    );
    let mut changed = request.clone();
    if let Request::LogManual { record, .. } = &mut changed {
        record.comment = Some("changed".into())
    }
    assert_eq!(f.run(&changed, now), Err("requestConflict"));
}
#[test]
fn manual_logging_preserves_memory_and_uncertainty_on_file_failure_without_retry() {
    let f = Fixture::new();
    std::fs::create_dir(f.dir.join("contacts.adi")).unwrap();
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    let result = f.run(&command, now).unwrap();
    assert_eq!(result["outcome"], "unknown");
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
    assert_eq!(f.run(&command, now).unwrap(), result);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
    assert!(f.dir.join("contacts.adi").is_dir());
}
#[test]
fn manual_logging_refuses_expired_windows_context_changes_and_contest_recording() {
    for kind in ["window", "context", "fieldDay", "recording"] {
        let f = Fixture::new();
        let now = Instant::now();
        let command = f.command(&f.acquire(now));
        let expected = match kind {
            "window" => "windowExpired",
            "context" => {
                let mut engine = f.engine.lock().unwrap();
                let mut s = engine.settings().clone();
                s.mycall = "W2XYZ".into();
                engine.apply_settings(s);
                "staleContext"
            }
            "fieldDay" => {
                let mut engine = f.engine.lock().unwrap();
                let mut s = engine.settings().clone();
                s.fd_active = true;
                engine.apply_settings(s);
                "staleContext"
            }
            _ => {
                let mut engine = f.engine.lock().unwrap();
                let mut s = engine.settings().clone();
                s.save_qso_wav = true;
                engine.apply_settings(s);
                "staleContext"
            }
        };
        assert_eq!(
            f.run(&command, if kind == "window" { now + WINDOW } else { now }),
            Err(expected)
        );
        assert!(f.engine.lock().unwrap().log_records().is_empty());
        if kind == "fieldDay" || kind == "recording" {
            let command = f.command(&f.state(now));
            assert_eq!(
                f.run(&command, now),
                Err(if kind == "fieldDay" {
                    "fieldDayUnsupported"
                } else {
                    "recordingUnsupported"
                })
            );
        }
    }
}
#[test]
fn manual_logging_disconnect_takeover_and_reconnect_cannot_restore_a_lease() {
    for kind in ["disconnect", "takeover", "socket"] {
        let mut f = Fixture::new();
        let now = Instant::now();
        let command = f.command(&f.acquire(now));
        match kind {
            "disconnect" => f.authority.disconnect_session(SESSION),
            "takeover" => f.authority.invalidate(),
            _ => {
                f.authority.retire_connection(f.connection);
                f.connection = f.authority.start_connection();
            }
        }
        assert!(f.run(&command, now).is_err());
        let state = f.state(now);
        assert_ne!(state["phase"], "controlling");
        assert_eq!(state["allowed"], kind != "takeover");
        assert!(f.engine.lock().unwrap().log_records().is_empty());
    }
}
#[test]
fn manual_logging_refuses_busy_engine_instead_of_queueing_and_counter_wrap() {
    let f = Fixture::new();
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    let lock = f.engine.lock().unwrap();
    assert_eq!(f.run(&command, now), Err("stationBusy"));
    drop(lock);
    f.authority.connection.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(f.authority.start_connection(), u64::MAX);
    assert_eq!(
        f.authority
            .handle(u64::MAX, SESSION, DEVICE, &command, &f.engine, now),
        Err("authorityUnavailable")
    );
}

#[test]
fn authority_counter_exhaustion_revokes_already_issued_hardware_permission() {
    let f = Fixture::new();
    let now = Instant::now();
    acquire_controls(&f, now);
    let permit = f
        .authority
        .hardware
        .permit(now + Duration::from_secs(5))
        .unwrap();
    assert!(permit.valid(now));
    {
        let mut core = f.authority.core.lock().unwrap();
        core.revision = MAX_COUNTER;
        assert_eq!(f.authority.advance(&mut core), Err("authorityUnavailable"));
        assert!(core.lease.is_none());
        assert!(core.windows.is_empty());
    }
    assert!(!permit.valid(now));
}

#[test]
fn manual_logging_releases_the_engine_before_waiting_for_file_sync() {
    let mut f = Fixture::new();
    let engine = f.engine.clone();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = called.clone();
    f.authority.before_sync = Some(Box::new(move || {
        assert!(
            engine.try_lock().is_ok(),
            "storage sync must not block the engine scheduler or local controls"
        );
        observed.store(true, Ordering::SeqCst);
    }));
    let now = Instant::now();
    let command = f.command(&f.acquire(now));
    assert_eq!(f.run(&command, now).unwrap()["outcome"], "applied");
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn manual_logging_uses_station_time_unless_the_operator_explicitly_overrides_it() {
    let f = Fixture::new();
    let now = Instant::now();
    let mut command = f.command(&f.acquire(now));
    if let Request::LogManual { record, .. } = &mut command {
        record.when_unix = None;
    }
    let before = super::super::now_ms() / 1000;
    assert_eq!(f.run(&command, now).unwrap()["outcome"], "applied");
    let engine = f.engine.lock().unwrap();
    let qso = &engine.log_records()[0];
    assert!(qso.when_unix >= before && qso.when_unix <= super::super::now_ms() / 1000);
}

#[test]
fn expired_receipt_never_reopens_an_old_sequence_under_a_live_lease() {
    let f = Fixture::new();
    let now = Instant::now();
    let state = f.acquire(now);
    let request = f.command(&state);
    assert_eq!(f.run(&request, now).unwrap()["outcome"], "applied");
    let adif = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    let lease_id = state["leaseId"].as_str().unwrap().to_string();
    // Keep the same controller alive while the bounded result history expires.
    // A receipt's absence must never turn an old action into a new append.
    for second in 1..=601 {
        let heartbeat = Request::Heartbeat {
            request_id: id(),
            lease_id: lease_id.clone(),
        };
        assert_eq!(
            f.run(&heartbeat, now + Duration::from_secs(second))
                .unwrap()["phase"],
            "controlling"
        );
    }
    let later = now + Duration::from_secs(601);
    assert_eq!(
        f.run(
            &Request::Result {
                request_id: id(),
                operation_id: request.id().into()
            },
            later
        ),
        Err("resultExpired")
    );
    assert_eq!(f.run(&request, later), Err("resultExpired"));
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), adif);
}

#[test]
fn a_desktop_log_collision_and_a_returned_profile_do_not_repeat_remote_work() {
    let f = Fixture::new();
    let now = Instant::now();
    let request = f.command(&f.acquire(now));
    if let Request::LogManual { record, .. } = &request {
        f.engine
            .lock()
            .unwrap()
            .log_qso(record.record().unwrap().into());
    }
    let adif = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    let result = f.run(&request, now).unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "alreadyPresent");
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), adif);
    let next = f.command(&f.state(now));
    {
        let mut engine = f.engine.lock().unwrap();
        let original = engine.settings().clone();
        let mut changed = original.clone();
        changed.mycall = "W2XYZ".into();
        engine.apply_settings(changed);
        engine.apply_settings(original);
    }
    assert_eq!(f.run(&next, now), Err("staleContext"));
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), adif);
}
