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
            "otaActivation",
            "activationExport",
            "settingsLogging",
            "selfSpot"
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
        // The write went through DESPITE 0o555, so this process is root (or holds
        // CAP_DAC_OVERRIDE) and the failure this test exists to stage cannot be
        // staged at all. That is a legitimate reason not to run — but not a
        // reason to report a pass, which is what the bare `return` here did:
        // `cargo test` captures `eprintln!`, so the four assertions below went
        // silently unexecuted on every root container. Straight to the real
        // stderr, which libtest does not capture. On GitHub Actions it is not a
        // skip at all: hosted runners execute as the non-root `runner` user, so
        // this arm is unreachable there today, and a workflow that started
        // running privileged would lose this coverage permanently and silently.
        // (`GITHUB_ACTIONS` rather than `CI`, for the reason spelled out in
        // `crates/tempo-audio/tests/common/mod.rs`: `CI` is also set by
        // `scripts/gates` on developer boxes, some of which are root containers.)
        let promised = std::env::var_os("GITHUB_ACTIONS").is_some_and(|v| !v.is_empty());
        assert!(
            !promised,
            "this test runs privileged, so a read-only directory does not stop a write and the \
             unknown/persistenceUnconfirmed assertions below cannot be reached — on GitHub \
             Actions that is a permanent, silent loss of the coverage, not a skip. Hosted \
             runners run as the non-root `runner` user; run the job that way."
        );
        use std::io::Write;
        let thread = std::thread::current();
        let mut err = std::io::stderr().lock();
        let _ = writeln!(
            err,
            "!! NOT RUN, and NOT A PASS: {} — running privileged, so a 0o555 directory does not \
             fail the rewrite and there is no unconfirmed-persistence case to observe.",
            thread.name().unwrap_or("<unnamed test>")
        );
        let _ = err.flush();
        return;
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

/// pota.app bodies (as JSON) and cluster spots a test's station posted.
type Posted = Arc<Mutex<(Vec<Value>, Vec<(f64, String, String)>)>>;

/// Record every spot the station would post, answering `pota` for pota.app and `cluster` for the
/// cluster. Without posters a test's self-spot reaches neither: the test build has no path to them.
fn posters(
    f: &mut Fixture,
    pota: Result<propagation::live::pota::SpotAnswer, String>,
    cluster: Result<(), String>,
) -> Posted {
    let posted = Posted::default();
    let (to_pota, to_cluster) = (posted.clone(), posted.clone());
    f.authority.spot_pota = Some(Box::new(move |spot| {
        to_pota
            .lock()
            .unwrap()
            .0
            .push(serde_json::to_value(spot).unwrap());
        pota.clone()
    }));
    f.authority.spot_cluster = Some(Box::new(move |freq, call, comment| {
        to_cluster
            .lock()
            .unwrap()
            .1
            .push((freq, call.to_string(), comment.to_string()));
        cluster.clone()
    }));
    posted
}

fn recorder(f: &mut Fixture) -> Posted {
    posters(f, Ok(propagation::live::pota::SpotAnswer::Posted), Ok(()))
}

fn nothing_posted(posted: &Posted) -> bool {
    let posted = posted.lock().unwrap();
    posted.0.is_empty() && posted.1.is_empty()
}

fn spot(f: &Fixture, reference: &str) -> Request {
    let dial = f.engine.lock().unwrap().settings().dial_hz();
    change(
        f,
        json!({"kind":"selfSpot","reference":reference,"dialHz":dial}),
    )
}

#[test]
fn self_spot_is_on_and_its_switch_still_refuses_before_anything_is_consumed() {
    // The SWITCH is a compile-time fact, so it is pinned at compile time: asserting it at
    // runtime is a tautology (clippy's `assertions_on_constants` says so, correctly) and the
    // pin is the point — flipping `SELF_SPOT` to false must say so here, by name, rather than
    // leave the `offered` assertion below failing for a reason nobody can read off it.
    const _: () = assert!(
        super::super::logging::SELF_SPOT,
        "self-spot is off: this test pins it ON, and its `offered` half asserts the capability \
         is advertised — turn that half around before flipping the switch"
    );
    let mut f = Fixture::new();
    acquire(&f);
    f.engine
        .lock()
        .unwrap()
        .set_activation("POTA", "US-0001")
        .unwrap();
    let offered = |f: &Fixture| {
        control_state_version(f, Instant::now(), 4)["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("selfSpot"))
    };
    assert!(offered(&f));
    // Positive control: with the switch off, the station neither offers nor accepts it again.
    f.authority.self_spot_off = true;
    assert!(!offered(&f));
    let state = control_state_version(&f, Instant::now(), 4);
    let request = spot(&f, "US-0001");
    assert_eq!(run(&f, &request), Err("stationUnsupported"));
    // The refusal consumed no sequence and no window, so the next write still lands.
    let next = run(&f, &f.command(&state)).unwrap();
    assert_eq!(next["outcome"], "applied");
}

#[test]
fn a_self_spot_posts_both_targets_the_station_call_dial_park_and_mode_exactly_once() {
    let mut f = Fixture::new();
    let posted = recorder(&mut f);
    acquire(&f);
    f.engine
        .lock()
        .unwrap()
        .set_activation("POTA", "US-0001")
        .unwrap();
    assert!(
        control_state_version(&f, Instant::now(), 4)["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("selfSpot"))
    );
    let (dial, tx) = {
        let e = f.engine.lock().unwrap();
        (e.settings().dial_hz(), e.tx_enabled())
    };
    let request = spot(&f, "US-0001");
    let result = run(&f, &request).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":request.id(),"outcome":"applied","evidence":"spotPosted",
            "spot":{"pota":"posted","cluster":"queued"}})
    );
    // A dropped reply is answered from the receipt: a public spot never posts twice.
    assert_eq!(run(&f, &request).unwrap(), result);
    let (pota, cluster) = posted.lock().unwrap().clone();
    // The fixture station is on FT8 at its default dial. The body is the website's seven fields.
    assert_eq!(
        pota,
        vec![json!({
            "activator": "W9XYZ", "spotter": "W9XYZ", "frequency": format!("{}", dial / 1000),
            "reference": "US-0001", "mode": "FT8", "source": "Nexus", "comments": "QRV"
        })]
    );
    assert_eq!(
        cluster,
        vec![(
            dial as f64 / 1e6,
            "W9XYZ".to_string(),
            "POTA US-0001".to_string()
        )]
    );
    let e = f.engine.lock().unwrap();
    assert_eq!(e.settings().dial_hz(), dial);
    assert_eq!(e.tx_enabled(), tx);
}

#[test]
fn a_self_spot_whose_context_moved_or_has_no_activation_posts_nothing() {
    let mut f = Fixture::new();
    let posted = recorder(&mut f);
    acquire(&f);
    // No activation: nothing to spot.
    let none = run(&f, &spot(&f, "US-0001")).unwrap();
    assert_eq!(none["reason"], "invalidChange");
    f.engine
        .lock()
        .unwrap()
        .set_activation("POTA", "US-0001")
        .unwrap();
    // The confirm showed another reference, or a dial that has since moved.
    let other = run(&f, &spot(&f, "US-0002")).unwrap();
    assert_eq!(other["reason"], "contextChanged");
    // A fresh request whose confirm showed a dial the station is no longer on.
    let dial = f.engine.lock().unwrap().settings().dial_hz();
    let elsewhere = change(
        &f,
        json!({"kind":"selfSpot","reference":"US-0001","dialHz":dial + 1000}),
    );
    let moved = run(&f, &elsewhere).unwrap();
    assert_eq!(moved["outcome"], "rejected");
    assert_eq!(moved["reason"], "contextChanged");
    // A dial that moves after the page's state was read changes the station's command context, so
    // the existing freshness rule refuses the request before the self-spot even runs.
    let stale = spot(&f, "US-0001");
    {
        let mut e = f.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.dial_mhz += 0.001;
        e.apply_settings(settings);
    }
    assert_eq!(run(&f, &stale), Err("staleContext"));
    assert!(nothing_posted(&posted));
}

fn activating(f: &Fixture) {
    f.engine
        .lock()
        .unwrap()
        .set_activation("POTA", "US-0001")
        .unwrap();
}

#[test]
fn a_pota_app_login_refusal_is_on_the_receipt_keeps_the_cluster_spot_and_stays_off() {
    let mut f = Fixture::new();
    let posted = posters(
        &mut f,
        Ok(propagation::live::pota::SpotAnswer::LoginRequired),
        Ok(()),
    );
    acquire(&f);
    activating(&f);
    let request = spot(&f, "US-0001");
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "spotPosted");
    assert_eq!(
        result["spot"],
        json!({"pota":"loginRequired","cluster":"queued"})
    );
    assert_eq!(run(&f, &request).unwrap(), result);
    // A later press on another frequency: pota.app is not asked again this session.
    {
        let mut e = f.engine.lock().unwrap();
        let mut settings = e.settings().clone();
        settings.dial_mhz += 0.005;
        e.apply_settings(settings);
    }
    let later = run(&f, &spot(&f, "US-0001")).unwrap();
    assert_eq!(
        later["spot"],
        json!({"pota":"loginRequired","cluster":"queued"})
    );
    let posted = posted.lock().unwrap();
    assert_eq!((posted.0.len(), posted.1.len()), (1, 2));
}

#[test]
fn a_failure_on_one_target_still_posts_and_reports_the_other() {
    let mut f = Fixture::new();
    let posted = posters(&mut f, Err("timed out".into()), Ok(()));
    acquire(&f);
    activating(&f);
    let result = run(&f, &spot(&f, "US-0001")).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["spot"], json!({"pota":"failed","cluster":"queued"}));
    assert_eq!(posted.lock().unwrap().1.len(), 1);

    let mut f = Fixture::new();
    posters(
        &mut f,
        Ok(propagation::live::pota::SpotAnswer::Posted),
        Err("no DX cluster connected — set a cluster host in Settings".into()),
    );
    acquire(&f);
    activating(&f);
    let result = run(&f, &spot(&f, "US-0001")).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(
        result["spot"],
        json!({"pota":"posted","cluster":"unavailable"})
    );
}

#[test]
fn a_self_spot_neither_target_took_is_refused_and_names_both_reasons() {
    let mut f = Fixture::new();
    posters(
        &mut f,
        Ok(propagation::live::pota::SpotAnswer::Refused {
            status: 500,
            excerpt: "down".into(),
        }),
        Err("no DX cluster connected — set a cluster host in Settings".into()),
    );
    acquire(&f);
    activating(&f);
    let request = spot(&f, "US-0001");
    let result = run(&f, &request).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":request.id(),"outcome":"rejected","reason":"spotNotPosted",
            "spot":{"pota":"failed","cluster":"unavailable"}})
    );
    // The receipt answers the replay; it is not a second attempt.
    assert_eq!(run(&f, &request).unwrap(), result);
}

#[test]
fn a_self_spot_needs_logging_permission_and_current_control() {
    let mut f = Fixture::new();
    let posted = recorder(&mut f);
    f.engine
        .lock()
        .unwrap()
        .set_activation("POTA", "US-0001")
        .unwrap();
    // Station control is not the logging grant: not offered, and refused.
    let state = acquire_controls_version(&f, Instant::now(), 4);
    assert!(!state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("selfSpot")));
    assert_eq!(
        run(&f, &spot(&f, "US-0001")),
        Err("localPermissionRequired")
    );
    f.authority.permit(DEVICE, true).unwrap();
    // A session that does not hold the control lease is refused.
    let request = spot(&f, "US-0001");
    assert_eq!(
        f.authority.handle_version(
            (f.connection, 4),
            OTHER,
            DEVICE,
            &request,
            &f.engine,
            Instant::now()
        ),
        Err("notController")
    );
    assert!(nothing_posted(&posted));
    // Positive control: the same request from the controller is accepted, and posts once to each.
    assert_eq!(run(&f, &request).unwrap()["outcome"], "applied");
    let posted = posted.lock().unwrap();
    assert_eq!((posted.0.len(), posted.1.len()), (1, 1));
}

#[test]
fn a_test_build_with_no_poster_refuses_a_self_spot_instead_of_posting_it() {
    let f = Fixture::new();
    acquire(&f);
    activating(&f);
    let result = run(&f, &spot(&f, "US-0001")).unwrap();
    // Both `failed`. No cluster is connected in a test, so the real cluster door would have
    // answered `unavailable`; the real pota.app door is not compiled into a test build at all.
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "spotNotPosted");
    assert_eq!(result["spot"], json!({"pota":"failed","cluster":"failed"}));
}

// ── A public spot of ANOTHER station ────────────────────────────────────────────────────────
//
// It posts from the station's own cluster login, so it rides STATION CONTROL, not the logging
// grant, and it touches no log record at all. Every test here goes through the self-spot's own
// poster hook: a test build has no path to a real cluster, so nothing can reach the world.

/// Station control only, the grant a spot of another station needs.
fn controlling(f: &Fixture) {
    f.authority.permit_station(DEVICE, true).unwrap();
    let state = control_state_version(f, Instant::now(), 4);
    run(
        f,
        &Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        },
    )
    .unwrap();
}

fn dx_spot(f: &Fixture, call: &str) -> Request {
    change(
        f,
        json!({"kind":"spot","call":call,"freqMhz":14.0765,"comment":"FT8 up 2"}),
    )
}

fn offers_post_spot(f: &Fixture) -> bool {
    control_state_version(f, Instant::now(), 4)["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("postSpot"))
}

#[test]
fn spotting_another_station_needs_station_control_and_never_the_logging_grant_alone() {
    let mut f = Fixture::new();
    let posted = recorder(&mut f);
    // A logging-only browser: the station does not offer it and refuses it if asked anyway.
    acquire(&f);
    assert!(!offers_post_spot(&f));
    assert_eq!(
        run(&f, &dx_spot(&f, "JA2DEF/P")),
        Err("localPermissionRequired")
    );
    assert!(nothing_posted(&posted));
    // Positive control: the same request under station control is offered and accepted.
    controlling(&f);
    assert!(offers_post_spot(&f));
    let request = dx_spot(&f, "JA2DEF/P");
    let result = run(&f, &request).unwrap();
    assert_eq!(
        result,
        json!({"operation":"logChange","operationId":request.id(),"outcome":"applied","evidence":"clusterQueued"})
    );
    // A dropped reply is answered from the receipt: a public spot never posts twice.
    assert_eq!(run(&f, &request).unwrap(), result);
    let (pota, cluster) = posted.lock().unwrap().clone();
    assert!(
        pota.is_empty(),
        "a spot of another station is not a self-spot"
    );
    assert_eq!(
        cluster,
        vec![(14.0765, "JA2DEF/P".to_string(), "FT8 up 2".to_string())]
    );
}

#[test]
fn a_station_with_no_cluster_node_says_so_and_queues_nothing() {
    let mut f = Fixture::new();
    let posted = posters(
        &mut f,
        Ok(propagation::live::pota::SpotAnswer::Posted),
        // The station's own verb's wording when no node is connected.
        Err("no DX cluster connected — set a cluster host in Settings".into()),
    );
    controlling(&f);
    let before = f.engine.lock().unwrap().log_records().len();
    let result = run(&f, &dx_spot(&f, "JA2DEF")).unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "clusterUnavailable");
    assert!(result.get("spot").is_none(), "no per-target report here");
    assert_eq!(f.engine.lock().unwrap().log_records().len(), before);
    // The door was reached exactly once and answered; nothing was retried.
    assert_eq!(posted.lock().unwrap().1.len(), 1);
}

#[test]
fn a_refused_callsign_or_frequency_never_reaches_the_cluster_door() {
    let mut f = Fixture::new();
    let posted = recorder(&mut f);
    controlling(&f);
    // Refused by the change's own rules, after it parses.
    for bad in [
        json!({"kind":"spot","call":"ja2def","freqMhz":14.0765,"comment":"x"}),
        json!({"kind":"spot","call":"W1","freqMhz":14.0765,"comment":"x"}),
        json!({"kind":"spot","call":"JA2DEF","freqMhz":0.0,"comment":"x"}),
        json!({"kind":"spot","call":"JA2DEF","freqMhz":-14.0,"comment":"x"}),
        json!({"kind":"spot","call":"JA2DEF","freqMhz":14.0765,"comment":"x".repeat(31)}),
        json!({"kind":"spot","call":"JA2DEF","freqMhz":14.0765,"comment":"up\n2"}),
    ] {
        assert_eq!(
            run(&f, &change(&f, bad.clone())),
            Err("invalidRecord"),
            "{bad}"
        );
    }
    // Refused by the wire grammar, before it is a change at all.
    for bad in [
        json!({"kind":"spot","call":"JA2DEF","freqMhz":14.0765}),
        json!({"kind":"spot","call":"JA2DEF","freqMhz":14.0765,"comment":"x","reference":"US-0001"}),
        json!({"kind":"spot","call":"JA2DEF","freqMhz":"14.0765","comment":"x"}),
    ] {
        assert!(
            serde_json::from_value::<super::super::logging::Change>(bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert!(nothing_posted(&posted));
    // Positive control: the well-formed request from the same browser is accepted.
    assert_eq!(
        run(&f, &dx_spot(&f, "JA2DEF")).unwrap()["outcome"],
        "applied"
    );
    assert_eq!(posted.lock().unwrap().1.len(), 1);
}

#[test]
fn a_test_build_with_no_poster_refuses_a_spot_instead_of_posting_it() {
    let f = Fixture::new();
    controlling(&f);
    let result = run(&f, &dx_spot(&f, "JA2DEF")).unwrap();
    // No poster installed: the refusal does not name the cluster, so it is an invalid change
    // rather than the `clusterUnavailable` the real door would answer with no node connected.
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["reason"], "invalidChange");
}

/// The shack's log view and a Remote browser are two writers of one log. The view used to
/// address a row by its position at load time; a browser's delete above that row shifted
/// every later one, and the shack's next Delete or Edit went to a DIFFERENT contact, with a
/// toast naming the one the operator meant. The shack now carries the row's key exactly as the
/// browser does, and `locate` turns it into today's position — or refuses.
#[test]
fn the_shacks_row_is_found_by_its_key_after_a_browser_delete_shifts_it() {
    let f = Fixture::new();
    seed(&f);
    f.engine.lock().unwrap().import_adif(
        "<CALL:5>N2XYZ<BAND:3>15m<MODE:3>SSB<FREQ:6>21.300<QSO_DATE:8>20260909<TIME_ON:6>030000<EOR>\n",
    );
    acquire(&f);
    // The shack loaded its list: K1ABC sits at position 1, and the view keeps that row's key.
    let held: Vec<Value> = page_rows(&f);
    assert_eq!(held[1]["call"], "K1ABC");
    let stale_position = 1;
    let held_target: super::super::logging::Target =
        serde_json::from_value(target(&held[1])).unwrap();

    // The browser deletes the row ABOVE it.
    let result = run(
        &f,
        &change(
            &f,
            json!({"kind":"delete","target":target(&row(&f, "W1AW"))}),
        ),
    )
    .unwrap();
    assert_eq!(result["outcome"], "applied");

    let mut e = f.engine.lock().unwrap();
    // The defect, made visible: the position the shack held now names another contact.
    assert_eq!(e.log_records()[stale_position].call, "N2XYZ");
    // The key finds the contact the operator can see, where it is TODAY.
    let found = super::super::logging::locate(&mut e, &held_target)
        .expect("the row the shack holds is still in the log");
    assert_eq!(found, 0);
    assert_eq!(e.log_records()[found].call, "K1ABC");
    assert!(e.delete_qso(found));
    let left: Vec<&str> = e.log_records().iter().map(|r| r.call.as_str()).collect();
    assert_eq!(left, ["N2XYZ"], "K1ABC went and the bystander survived");

    // A key the log no longer holds is refused, not approximated: the deleted row's own key,
    // and a row that was edited since the view loaded (its key changed with its content).
    assert_eq!(super::super::logging::locate(&mut e, &held_target), None);
    let survivor_key: super::super::logging::Target = serde_json::from_value(
        json!({"call":"N2XYZ","whenUnix":e.log_records()[0].when_unix,
            "key":super::super::logging::row_key(&e.log_records()[0])}),
    )
    .unwrap();
    let mut changed = e.log_records()[0].clone();
    changed.comment = Some("changed at the browser".into());
    assert!(e.update_qso(0, changed));
    assert_eq!(super::super::logging::locate(&mut e, &survivor_key), None);
    // Positive control: the key of the row as it is now is found.
    let fresh: super::super::logging::Target = serde_json::from_value(
        json!({"call":"N2XYZ","whenUnix":e.log_records()[0].when_unix,
            "key":super::super::logging::row_key(&e.log_records()[0])}),
    )
    .unwrap();
    assert_eq!(super::super::logging::locate(&mut e, &fresh), Some(0));
}

/// WWFF is a real stored program (an ADIF SIG kept verbatim, as the desktop edit path keeps
/// it). The browser used to narrow the program to SOTA-or-POTA and the station applied it
/// unconditionally, so a Remote grid fix rewrote a WWFF park to POTA.
#[test]
fn a_remote_edit_keeps_a_wwff_park_as_wwff() {
    let f = Fixture::new();
    seed(&f);
    f.engine.lock().unwrap().import_adif(
        "<CALL:6>DL1ABC<BAND:3>20m<MODE:3>SSB<FREQ:6>14.250<QSO_DATE:8>20260909<TIME_ON:6>040000\
         <SIG:4>WWFF<SIG_INFO:9>DLFF-0001<EOR>\n",
    );
    acquire(&f);
    let stored = |f: &Fixture| {
        let e = f.engine.lock().unwrap();
        let r = e.log_records().iter().find(|r| r.call == "DL1ABC").unwrap();
        (
            r.ota.their_program.clone(),
            r.ota.their_ref.clone(),
            r.grid.clone(),
        )
    };
    assert_eq!(
        stored(&f),
        (Some("WWFF".into()), Some("DLFF-0001".into()), None)
    );
    let record = |ota: Value| {
        let mut record = json!({"call":"DL1ABC","grid":"JO31","country":null,"state":null,"band":"20m",
            "freqMhz":14.25,"mode":"SSB","rstSent":null,"rstRcvd":null,"name":null,"qth":null,"comment":null,
            "notes":null,"whenUnix":row(&f, "DL1ABC")["whenUnix"],"confirmed":false,"awardConfirmed":false});
        if !ota.is_null() {
            record["ota"] = ota;
        }
        json!({"kind":"edit","target":target(&row(&f, "DL1ABC")),"record":record})
    };
    // The program the browser read back, exactly as stored.
    let result = run(
        &f,
        &change(
            &f,
            record(json!({"theirProgram":"WWFF","theirRef":"DLFF-0001"})),
        ),
    )
    .unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(
        stored(&f),
        (
            Some("WWFF".into()),
            Some("DLFF-0001".into()),
            Some("JO31".into())
        )
    );
    // A program the log would never hold is an invalid record, refused before it is a change.
    let refused = run(
        &f,
        &change(
            &f,
            record(json!({"theirProgram":"pota","theirRef":"DLFF-0001"})),
        ),
    );
    assert_eq!(refused, Err("invalidRecord"));
    assert_eq!(stored(&f).0.as_deref(), Some("WWFF"));
}
