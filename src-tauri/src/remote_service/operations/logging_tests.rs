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
    assert_eq!(
        state["controls"]["capabilities"],
        json!([
            "qsoLogging",
            "logEdit",
            "qslMarks",
            "otaHunt",
            "otaActivation"
        ])
    );
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
    // A previous browser cannot parse v4 persistence evidence as its older
    // station-control result. Refuse the query explicitly, without replay.
    for version in [2, 3] {
        assert_eq!(
            f.authority.handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                &Request::Result {
                    request_id: id(),
                    operation_id: request.id().into()
                },
                &f.engine,
                Instant::now()
            ),
            Err("stationUnsupported")
        );
    }
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
            e.set_tx_enabled(false);
            let action = json!({"action":"qso.logCurrent","expectedKey":e.current_qso_log_key().unwrap(),"expectedTier":e.tier(),
                "expectedQso":FtExchangeContext::from(&e.snapshot().qso.unwrap())});
            if replace {
                e.call_station("W1AW");
                e.set_tx_enabled(false);
            }
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

// ---- Log changes: a row is found again by its key, never by a position -------------------------

const SEED: &str = "<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<FREQ:6>14.074<QSO_DATE:8>20260909<TIME_ON:6>010000<NAME:4>Joe <EOR>\n\
<CALL:5>K1ABC<BAND:3>40m<MODE:2>CW<FREQ:5>7.030<QSO_DATE:8>20260909<TIME_ON:6>020000<EOR>\n";

fn seed(f: &Fixture) {
    f.engine.lock().unwrap().import_adif(SEED);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 2);
}

/// The rows exactly as the browser's log page receives them.
fn page_rows(f: &Fixture) -> Vec<Value> {
    let request: super::super::super::query::Request =
        serde_json::from_value(json!({"requestId":id(),
        "collection":"log","cursor":null,"search":"","unconfirmed":false,"after":null}))
        .unwrap();
    let page: Value = serde_json::from_str(
        &super::super::super::query::Publisher::default()
            .read(&request, &f.engine, None, Instant::now())
            .unwrap(),
    )
    .unwrap();
    page["rows"].as_array().unwrap().clone()
}

fn row(f: &Fixture, call: &str) -> Value {
    page_rows(f)
        .into_iter()
        .find(|r| r["call"] == call)
        .unwrap()
}

fn target(row: &Value) -> Value {
    json!({"call":row["call"],"whenUnix":row["whenUnix"],"key":super::super::logging::value_key(row)})
}

fn change(f: &Fixture, change: Value) -> Request {
    let s = control_state_version(f, Instant::now(), 4);
    serde_json::from_value(json!({"type":"logChange","requestId":id(),"stationBootId":s["stationBootId"],
        "leaseId":s["leaseId"],"expectedRevision":s["revision"],"commandWindowId":s["commandWindowId"],
        "clientSequence":s["nextSequence"],"change":change}))
    .unwrap()
}

fn edit(row: &Value) -> Value {
    json!({"kind":"edit","target":target(row),"record":{"call":"W1AW","grid":"FN42","country":null,"state":null,
        "band":"40m","freqMhz":7.074,"mode":"FT8","rstSent":"-05","rstRcvd":"-07","name":"Hiram","qth":null,
        "comment":"fixed band","notes":null,"whenUnix":row["whenUnix"],"confirmed":false,"awardConfirmed":false}})
}

fn adif(f: &Fixture) -> Vec<u8> {
    std::fs::read(f.dir.join("contacts.adi")).unwrap()
}

#[test]
fn the_row_key_matches_the_browser_vector_and_the_log_page_row() {
    // The same vector as ui/src/remote-web/log-change.test.ts: both implementations must build
    // these bytes, or every remote edit would be refused as stale.
    let vector: Value = serde_json::from_str(r#"{"call":"W1AW","freqMhz":14.074,"txPower":-0.5,"whenUnix":1788940800,"name":"José 日本","qth":"a\"b}","grid":null,"confirmed":false,"ota":{"theirRef":"US-0001","iota":null},"credit":["DXCC",7],"big":12345678901234567}"#).unwrap();
    assert_eq!(
        super::super::logging::row_canonical(&vector),
        "o11{s3:bign12345678901234567741440;s4:calls4:W1AWs9:confirmedfs6:credita2[s4:DXCCn7000000;]s7:freqMhzn14074000;s4:gridzs4:names12:José 日本s3:otao2{s4:iotazs8:theirRefs7:US-0001}s3:qths4:a\"b}s7:txPowern-500000;s8:whenUnixn1788940800000000;}"
    );
    assert_eq!(
        super::super::logging::value_key(&vector),
        "d33eff984a6c2d66e74a3a7fcb19d04b318508cd7cc6dcfcdb77d80ea6121fb3"
    );
    let f = Fixture::new();
    seed(&f);
    // The page read needs Engine, so take the stored keys first and release it.
    let stored: Vec<(String, String)> = f
        .engine
        .lock()
        .unwrap()
        .log_records()
        .iter()
        .map(|r| (r.call.clone(), super::super::logging::row_key(r)))
        .collect();
    let page = page_rows(&f);
    assert_eq!(page.len(), stored.len());
    for (call, key) in stored {
        let row = page.iter().find(|r| r["call"] == call.as_str()).unwrap();
        assert_eq!(key, super::super::logging::value_key(row));
    }
}

#[test]
fn an_edit_is_found_by_its_key_synced_and_replayed_without_a_second_write() {
    let f = Fixture::new();
    seed(&f);
    {
        let mut e = f.engine.lock().unwrap();
        let index = e
            .log_records()
            .iter()
            .position(|r| r.call == "W1AW")
            .unwrap();
        assert!(e.mark_qsl_card(index, true));
    }
    acquire(&f);
    let state = control_state_version(&f, Instant::now(), 4);
    assert_eq!(state["actions"], json!(["log.manual"]));
    let before = row(&f, "W1AW");
    let request = change(&f, edit(&before));
    let result = run(&f, &request).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":request.id(),"outcome":"applied","evidence":"fileSynced"})
    );
    let bytes = adif(&f);
    {
        let e = f.engine.lock().unwrap();
        let edited = e.log_records().iter().find(|r| r.call == "W1AW").unwrap();
        assert_eq!(
            (edited.band.as_str(), edited.grid.as_deref()),
            ("40m", Some("FN42"))
        );
        assert_eq!(edited.name.as_deref(), Some("Hiram"));
        assert!(
            edited.qsl_rcvd.card,
            "an ordinary edit keeps the paper card"
        );
        assert_eq!(e.log_records().len(), 2);
        assert!(!e.tx_enabled());
    }
    assert!(String::from_utf8_lossy(&bytes).contains("FN42"));
    // A dropped reply is answered from the receipt: no second write, no second change.
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
    assert_eq!(adif(&f), bytes);
    assert_ne!(target(&row(&f, "W1AW"))["key"], target(&before)["key"]);
}

#[test]
fn a_change_against_a_row_that_changed_at_the_station_is_refused_and_writes_nothing() {
    let f = Fixture::new();
    seed(&f);
    acquire(&f);
    let stale = row(&f, "W1AW");
    {
        let mut e = f.engine.lock().unwrap();
        let index = e
            .log_records()
            .iter()
            .position(|r| r.call == "W1AW")
            .unwrap();
        let mut local = e.log_records()[index].clone();
        local.comment = Some("changed at the shack".into());
        assert!(e.update_qso(index, local));
    }
    let bytes = adif(&f);
    for kind in [
        edit(&stale),
        json!({"kind":"delete","target":target(&stale)}),
    ] {
        let result = run(&f, &change(&f, kind)).unwrap();
        assert_eq!(result["outcome"], "rejected");
        assert_eq!(result["reason"], "contextChanged");
        assert_eq!(adif(&f), bytes);
        assert_eq!(f.engine.lock().unwrap().log_records().len(), 2);
    }
    // Positive control: the same delete from a current page applies.
    let fresh = row(&f, "W1AW");
    let result = run(
        &f,
        &change(&f, json!({"kind":"delete","target":target(&fresh)})),
    )
    .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 1);
}

#[test]
fn a_delete_is_idempotent_across_a_dropped_response_and_never_reaches_a_later_contact() {
    let f = Fixture::new();
    seed(&f);
    acquire(&f);
    let delete = change(
        &f,
        json!({"kind":"delete","target":target(&row(&f, "W1AW"))}),
    );
    let result = run(&f, &delete).unwrap();
    assert_eq!(result["evidence"], "fileSynced");
    let remaining: Vec<_> = f
        .engine
        .lock()
        .unwrap()
        .log_records()
        .iter()
        .map(|r| r.call.clone())
        .collect();
    assert_eq!(remaining, ["K1ABC"]);
    assert!(!String::from_utf8_lossy(&adif(&f)).contains("W1AW"));
    // The same contact logged again later is a new row; replaying the old delete cannot touch it.
    f.engine.lock().unwrap().import_adif(SEED);
    let bytes = adif(&f);
    assert_eq!(run(&f, &delete).unwrap(), result);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 2);
    assert_eq!(adif(&f), bytes);
}

#[test]
fn log_changes_need_logging_permission_and_operation_v4() {
    let f = Fixture::new();
    seed(&f);
    let stale_free = row(&f, "W1AW");
    let state = acquire_controls_version(&f, Instant::now(), 4);
    let capabilities = state["controls"]["capabilities"].as_array().unwrap();
    assert!(!capabilities.contains(&json!("logEdit")));
    let delete = json!({"kind":"delete","target":target(&stale_free)});
    assert_eq!(
        run(&f, &change(&f, delete.clone())),
        Err("localPermissionRequired")
    );
    f.authority.permit(DEVICE, true).unwrap();
    let request = change(&f, delete);
    for version in [1, 2, 3] {
        assert!(
            !control_state_version(&f, Instant::now(), version)["controls"]["capabilities"]
                .as_array()
                .is_some_and(|c| c.contains(&json!("logEdit")))
        );
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
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 2);
}

#[cfg(unix)]
#[test]
fn a_rewrite_that_did_not_reach_the_disk_is_unknown_never_applied() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    seed(&f);
    acquire(&f);
    let request = change(&f, edit(&row(&f, "W1AW")));
    let bytes = adif(&f);
    // The rewrite writes a temporary file beside the log; a read-only directory fails it.
    std::fs::set_permissions(&f.dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = std::fs::write(f.dir.join("probe"), b"x").is_err();
    let result = run(&f, &request);
    std::fs::set_permissions(&f.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !probe {
        return; // running as a user the directory mode cannot stop; nothing to prove here
    }
    let result = result.unwrap();
    assert_eq!(result["outcome"], "unknown");
    assert_eq!(result["reason"], "persistenceUnconfirmed");
    assert_eq!(adif(&f), bytes);
}

#[test]
fn qsl_marks_are_found_by_key_synced_and_a_card_sent_is_never_a_confirmation() {
    let f = Fixture::new();
    seed(&f);
    acquire(&f);
    let card = change(
        &f,
        json!({"kind":"qslCard","target":target(&row(&f, "W1AW")),"received":true}),
    );
    let result = run(&f, &card).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":card.id(),"outcome":"applied","evidence":"fileSynced"})
    );
    assert!(
        f.engine
            .lock()
            .unwrap()
            .log_records()
            .iter()
            .find(|r| r.call == "W1AW")
            .unwrap()
            .qsl_rcvd
            .card
    );
    let bytes = adif(&f);
    assert_eq!(run(&f, &card).unwrap(), result);
    assert_eq!(adif(&f), bytes);
    let sent = change(
        &f,
        json!({"kind":"qslSent","target":target(&row(&f, "K1ABC")),"via":"D"}),
    );
    assert_eq!(run(&f, &sent).unwrap()["outcome"], "applied");
    {
        let e = f.engine.lock().unwrap();
        let k1abc = e.log_records().iter().find(|r| r.call == "K1ABC").unwrap();
        assert!(k1abc.qsl_sent.sent);
        assert!(!k1abc.confirmed && !k1abc.award_confirmed);
        assert!(!e.tx_enabled());
    }
    // Only null withdraws. The row it was aimed at is stale afterwards and refused.
    let marked = target(&row(&f, "K1ABC"));
    let withdraw = change(&f, json!({"kind":"qslSent","target":marked,"via":null}));
    assert_eq!(run(&f, &withdraw).unwrap()["outcome"], "applied");
    assert!(
        !f.engine
            .lock()
            .unwrap()
            .log_records()
            .iter()
            .find(|r| r.call == "K1ABC")
            .unwrap()
            .qsl_sent
            .sent
    );
    let bytes = adif(&f);
    let refused = run(
        &f,
        &change(&f, json!({"kind":"qslSent","target":marked,"via":"B"})),
    )
    .unwrap();
    assert_eq!(refused["outcome"], "rejected");
    assert_eq!(refused["reason"], "contextChanged");
    assert_eq!(adif(&f), bytes);
}

#[test]
fn a_qsl_sent_mark_accepts_only_the_menu_codes() {
    let f = Fixture::new();
    seed(&f);
    acquire(&f);
    let bytes = adif(&f);
    // The empty string is the menu's placeholder: a non-choice, never a withdrawal.
    for via in ["", "X", "b", "bureau"] {
        let request = change(
            &f,
            json!({"kind":"qslSent","target":target(&row(&f, "W1AW")),"via":via}),
        );
        assert_eq!(run(&f, &request), Err("invalidRecord"));
    }
    // A missing `via` is malformed, not a withdrawal: only an explicit null clears the mark.
    let s = control_state_version(&f, Instant::now(), 4);
    assert!(
        serde_json::from_value::<Request>(json!({"type":"logChange","requestId":id(),
        "stationBootId":s["stationBootId"],"leaseId":s["leaseId"],"expectedRevision":s["revision"],
        "commandWindowId":s["commandWindowId"],"clientSequence":s["nextSequence"],
        "change":{"kind":"qslSent","target":target(&row(&f, "W1AW"))}}))
        .is_err()
    );
    assert_eq!(adif(&f), bytes);
    assert!(!f
        .engine
        .lock()
        .unwrap()
        .log_records()
        .iter()
        .any(|r| r.qsl_sent.sent));
}

#[test]
fn a_remote_hunt_tags_the_next_logged_contact_and_never_keys_or_tunes() {
    let f = Fixture::new();
    acquire(&f);
    let dial = f.engine.lock().unwrap().settings().dial_hz();
    let hunt = change(
        &f,
        json!({"kind":"hunt","call":"W1AW","program":"POTA","reference":"US-0002"}),
    );
    let result = run(&f, &hunt).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":hunt.id(),"outcome":"applied","evidence":"stationState"})
    );
    assert_eq!(run(&f, &hunt).unwrap(), result);
    assert_eq!(
        f.engine.lock().unwrap().hunt_target(),
        Some(("POTA".into(), "US-0002".into(), "W1AW".into()))
    );
    // The station's own funnel stamps the hunted reference on the next contact with that call,
    // including one logged from this browser.
    let logged = run(
        &f,
        &f.command(&control_state_version(&f, Instant::now(), 4)),
    )
    .unwrap();
    assert_eq!(logged["outcome"], "applied");
    {
        let e = f.engine.lock().unwrap();
        let contact = e.log_records().iter().find(|r| r.call == "W1AW").unwrap();
        assert_eq!(contact.ota.their_ref.as_deref(), Some("US-0002"));
        assert!(e.hunt_target().is_none(), "the pend is consumed by the tag");
        assert!(!e.tx_enabled());
        assert_eq!(e.settings().dial_hz(), dial);
    }
    let hunt = change(
        &f,
        json!({"kind":"hunt","call":"K1ABC","program":"POTA","reference":"US-0003"}),
    );
    assert_eq!(run(&f, &hunt).unwrap()["evidence"], "stationState");
    let clear = change(&f, json!({"kind":"clearHunt"}));
    assert_eq!(run(&f, &clear).unwrap()["evidence"], "stationState");
    assert!(f.engine.lock().unwrap().hunt_target().is_none());
}

#[test]
fn a_hunt_the_station_cannot_normalize_is_refused_and_leaves_no_pend() {
    let f = Fixture::new();
    acquire(&f);
    // A summit reference under POTA passes the wire grammar; the station's own normalizer refuses it.
    let result = run(
        &f,
        &change(
            &f,
            json!({"kind":"hunt","call":"W1AW","program":"POTA","reference":"W7A/MN-001"}),
        ),
    )
    .unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "invalidChange");
    assert!(f.engine.lock().unwrap().hunt_target().is_none());
    // Outside the wire grammar entirely: refused before anything is admitted.
    for bad in [
        json!({"kind":"hunt","call":"w1aw","program":"POTA","reference":"US-0002"}),
        json!({"kind":"hunt","call":"W1AW","program":"WWFF","reference":"US-0002"}),
        json!({"kind":"hunt","call":"W1AW","program":"POTA","reference":"US 0002"}),
    ] {
        assert_eq!(run(&f, &change(&f, bad)), Err("invalidRecord"));
    }
    assert!(f.engine.lock().unwrap().hunt_target().is_none());
}

#[test]
fn a_remote_activation_stamps_your_reference_on_logged_contacts_and_never_keys_or_tunes() {
    let f = Fixture::new();
    acquire(&f);
    let dial = f.engine.lock().unwrap().settings().dial_hz();
    let start = change(
        &f,
        json!({"kind":"activation","program":"POTA","reference":"US-0001"}),
    );
    let result = run(&f, &start).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":start.id(),"outcome":"applied","evidence":"stationState"})
    );
    assert_eq!(run(&f, &start).unwrap(), result);
    assert_eq!(
        f.engine.lock().unwrap().activation(),
        Some(("POTA".into(), "US-0001".into()))
    );
    // The station's own funnel stamps your park on a contact logged from this browser.
    let logged = run(
        &f,
        &f.command(&control_state_version(&f, Instant::now(), 4)),
    )
    .unwrap();
    assert_eq!(logged["outcome"], "applied");
    {
        let e = f.engine.lock().unwrap();
        let contact = e.log_records().iter().find(|r| r.call == "W1AW").unwrap();
        assert_eq!(contact.ota.my_ref.as_deref(), Some("US-0001"));
        assert_eq!(contact.ota.my_program.as_deref(), Some("POTA"));
        assert!(!e.tx_enabled());
        assert_eq!(e.settings().dial_hz(), dial);
    }
    let end = change(&f, json!({"kind":"clearActivation"}));
    assert_eq!(run(&f, &end).unwrap()["evidence"], "stationState");
    assert!(f.engine.lock().unwrap().activation().is_none());
}

#[test]
fn an_activation_the_station_cannot_normalize_is_refused_and_starts_nothing() {
    let f = Fixture::new();
    acquire(&f);
    // A summit reference under POTA passes the wire grammar; the station's own normalizer refuses it.
    let result = run(
        &f,
        &change(
            &f,
            json!({"kind":"activation","program":"POTA","reference":"W7A/MN-001"}),
        ),
    )
    .unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "invalidChange");
    assert!(f.engine.lock().unwrap().activation().is_none());
    for bad in [
        json!({"kind":"activation","program":"WWFF","reference":"US-0001"}),
        json!({"kind":"activation","program":"POTA","reference":"US 0001"}),
    ] {
        assert_eq!(run(&f, &change(&f, bad)), Err("invalidRecord"));
    }
    assert!(f.engine.lock().unwrap().activation().is_none());
}
