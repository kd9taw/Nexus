//! Logging-only authority, durable outcomes and exact pending-contact identity.
use super::*;
use tempo_app::engine::remote_transmit::FtExchangeContext;

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

fn acquire(f: &Fixture) -> Value {
    f.authority.permit(DEVICE, true).unwrap();
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

fn hold(f: &Fixture) -> String {
    let state = acquire(f);
    let Request::LogManual { record, .. } = f.command(&state) else {
        unreachable!()
    };
    let mut e = f.engine.lock().unwrap();
    e.set_pending_qso_path(f.dir.join("pending.json"));
    e.load_pending_qso(record.record().unwrap().into());
    e.persist_pending_qso();
    e.pending_qso_log_key().unwrap()
}

fn confirm(key: &str) -> Value {
    json!({"action":"qso.confirm","expectedKey":key,"edits":{"call":"W1AW","grid":"FN32","rstSent":"59","rstRcvd":"58"}})
}

fn command(f: &Fixture, action: Value) -> Request {
    control_request(&control_state_version(f, Instant::now(), 4), action)
}

#[test]
fn pending_confirm_requires_logging_not_radio_or_transmit_permission() {
    let f = Fixture::new();
    let key = hold(&f);
    let state = control_state_version(&f, Instant::now(), 4);
    assert_eq!(state["controls"]["capabilities"], json!(["qsoLogging"]));
    assert!(state["transmitEpoch"].is_null());
    assert!(!f.engine.lock().unwrap().tx_enabled());
    let request = control_request(&state, confirm(&key));
    for version in [1, 2, 3] {
        assert_eq!(
            f.authority.handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                &request,
                &f.engine,
                Instant::now()
            ),
            Err("stationUnsupported")
        );
    }
    let before = f.engine.lock().unwrap().settings().clone();
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "fileSynced");
    let bytes = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    assert!(!f.dir.join("pending.json").exists());
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
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), bytes);
    let mut e = f.engine.lock().unwrap();
    assert!(e.pending_qso_log_key().is_none());
    assert_eq!(e.log_records().len(), 1);
    let record = &e.log_records()[0];
    assert_eq!(record.grid.as_deref(), Some("FN32"));
    assert_eq!(record.rst_rcvd.as_deref(), Some("58"));
    assert_eq!(record.name.as_deref(), Some("Joe"));
    assert_eq!(record.notes.as_deref(), Some("Keep this note"));
    assert_eq!(e.take_pending_uploads().len(), 1);
    assert_eq!(
        serde_json::to_value(e.settings()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert!(!e.tx_enabled());
}

#[test]
fn radio_permission_cannot_confirm_or_discard_a_contact() {
    for action in ["confirm", "discard"] {
        let f = Fixture::new();
        let key = hold(&f);
        f.authority.permit(DEVICE, false).unwrap();
        let state = acquire_controls_version(&f, Instant::now(), 4);
        assert!(!state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("qsoLogging")));
        let action = if action == "confirm" {
            confirm(&key)
        } else {
            json!({"action":"qso.discard","expectedKey":key})
        };
        assert_eq!(
            run(&f, &control_request(&state, action)),
            Err("localPermissionRequired")
        );
        assert_eq!(
            f.engine.lock().unwrap().pending_qso_log_key().as_deref(),
            Some(key.as_str())
        );
        assert!(f.engine.lock().unwrap().log_records().is_empty());
        assert!(f.dir.join("pending.json").exists());
    }
}

#[test]
fn replacing_a_hold_with_identical_fields_refuses_both_old_gestures() {
    for action in ["confirm", "discard"] {
        let f = Fixture::new();
        let key = hold(&f);
        let action = if action == "confirm" {
            confirm(&key)
        } else {
            json!({"action":"qso.discard","expectedKey":key})
        };
        let request = command(&f, action);
        let new_key = {
            let mut e = f.engine.lock().unwrap();
            let record = e.pending_log_identity().unwrap().record().clone();
            e.discard_pending_log();
            e.load_pending_qso(record);
            e.persist_pending_qso();
            e.pending_qso_log_key().unwrap()
        };
        assert_ne!(new_key, key);
        let result = run(&f, &request).unwrap();
        assert_eq!(result["outcome"], "rejected");
        assert_eq!(result["reason"], "contextChanged");
        assert_eq!(
            f.engine.lock().unwrap().pending_qso_log_key(),
            Some(new_key)
        );
        assert!(f.engine.lock().unwrap().log_records().is_empty());
        assert!(f.dir.join("pending.json").exists());
    }
}

#[test]
fn a_failed_append_retains_pending_record_and_journal_without_replaying() {
    let f = Fixture::new();
    let key = hold(&f);
    let journal = std::fs::read(f.dir.join("pending.json")).unwrap();
    let blocked = f.dir.join("not-a-log-file");
    std::fs::create_dir(&blocked).unwrap();
    f.engine.lock().unwrap().set_log_path(blocked);
    let request = command(&f, confirm(&key));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "unknown");
    assert_eq!(result["reason"], "persistenceFailed");
    assert_eq!(run(&f, &request).unwrap(), result);
    assert_eq!(std::fs::read(f.dir.join("pending.json")).unwrap(), journal);
    assert_eq!(f.engine.lock().unwrap().pending_qso_log_key(), Some(key));
}

#[test]
fn confirmation_sync_releases_engine_and_never_clears_a_newer_hold() {
    let mut f = Fixture::new();
    let key = hold(&f);
    let engine = f.engine.clone();
    f.authority.before_sync = Some(Box::new(move || {
        let mut e = engine
            .try_lock()
            .expect("disk wait must release the engine");
        let record = e.pending_log_identity().unwrap().record().clone();
        e.discard_pending_log();
        e.load_pending_qso(record);
        e.persist_pending_qso();
    }));
    let request = command(&f, confirm(&key));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "fileSynced");
    assert_ne!(f.engine.lock().unwrap().pending_qso_log_key().unwrap(), key);
    assert!(f.dir.join("pending.json").exists());
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
}

#[test]
fn discard_is_durable_and_duplicate_receipts_do_not_discard_a_later_contact() {
    let f = Fixture::new();
    let key = hold(&f);
    let request = command(&f, json!({"action":"qso.discard","expectedKey":key}));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["evidence"], "pendingDiscarded");
    assert!(!f.dir.join("pending.json").exists());
    let newer = hold(&f);
    assert_eq!(run(&f, &request).unwrap(), result);
    assert_eq!(f.engine.lock().unwrap().pending_qso_log_key(), Some(newer));
    assert!(f.dir.join("pending.json").exists());
    assert!(f.engine.lock().unwrap().log_records().is_empty());
}

#[test]
fn current_log_checks_native_eligibility_and_contact_incarnation() {
    for replace in [false, true] {
        let f = Fixture::new();
        acquire(&f);
        let action = {
            let mut e = f.engine.lock().unwrap();
            e.call_station("W1AW");
            let action = json!({"action":"qso.logCurrent","expectedKey":e.current_qso_log_key().unwrap(),"expectedTier":e.tier(),
                "expectedQso":FtExchangeContext::from(&e.snapshot().qso.unwrap())});
            if replace {
                e.call_station("W1AW");
            }
            e.set_tx_enabled(false);
            action
        };
        let result = run(&f, &command(&f, action)).unwrap();
        assert_eq!(result["outcome"], "rejected");
        assert_eq!(
            result["reason"],
            if replace {
                "contextChanged"
            } else {
                "noEligibleContact"
            }
        );
        assert!(f.engine.lock().unwrap().log_records().is_empty());
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}
