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
