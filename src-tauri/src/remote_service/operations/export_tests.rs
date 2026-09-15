//! One activation file for a Remote browser: the desktop's per-activation ADIF export, read in
//! bounded chunks under the logging grant and the browser's own current lease. Nothing else from
//! the log can be requested this way.
use super::*;

/// 2026-09-09 00:00:00 UTC.
const DAY: u64 = 1_788_912_000;
const CHUNK: usize = 32 * 1024;

fn contact(call: &str, date: &str, time: &str, park: Option<&str>, comment: &str) -> String {
    let field = |name: &str, value: &str| format!("<{name}:{}>{value}", value.len());
    let mut s = [
        field("CALL", call),
        field("BAND", "20m"),
        field("MODE", "FT8"),
        field("FREQ", "14.074"),
        field("QSO_DATE", date),
        field("TIME_ON", time),
        field("STATION_CALLSIGN", "W9XYZ"),
    ]
    .concat();
    if let Some(park) = park {
        s.push_str(&field("MY_SIG", "POTA"));
        s.push_str(&field("MY_SIG_INFO", park));
    }
    if !comment.is_empty() {
        s.push_str(&field("COMMENT", comment));
    }
    s + "<EOR>\n"
}

/// Two contacts from US-1234 on the day, one from a second park that afternoon, one from the same
/// park the next day, and one from home. Only the first two are that activation.
fn seed(f: &Fixture) {
    let text = [
        contact("K1AAA", "20260909", "010000", Some("US-1234"), ""),
        contact("K1BBB", "20260909", "020000", Some("US-1234"), ""),
        contact("K1CCC", "20260909", "150000", Some("US-5678"), ""),
        contact("K1DDD", "20260910", "010000", Some("US-1234"), ""),
        contact("K1EEE", "20260909", "030000", None, ""),
    ]
    .concat();
    f.engine.lock().unwrap().import_adif(&text);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 5);
}

fn run(f: &Fixture, version: u8, request: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, version),
        SESSION,
        DEVICE,
        request,
        &f.engine,
        Instant::now(),
    )
}

fn lease(f: &Fixture) -> Value {
    f.authority.permit(DEVICE, true).unwrap();
    let state = control_state_version(f, Instant::now(), 4);
    run(
        f,
        4,
        &Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        },
    )
    .unwrap()
}

fn raw(state: &Value, selection: Value, index: u64) -> Value {
    json!({"type":"activationExport","requestId":id(),"stationBootId":state["stationBootId"],
        "leaseId":state["leaseId"].as_str().map(String::from).unwrap_or_else(id),
        "selection":selection,"index":index})
}

fn export(state: &Value, selection: Value, index: u64) -> Request {
    serde_json::from_value(raw(state, selection, index)).unwrap()
}

fn selection(reference: &str, day: u64) -> Value {
    json!({"reference":reference,"dayStartUnix":day,"callsign":"W9XYZ"})
}

fn hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Read every chunk the way the browser does; return the bytes and the file description.
fn download(f: &Fixture, state: &Value, selection: &Value) -> (Vec<u8>, Value) {
    let first = run(f, 4, &export(state, selection.clone(), 0)).unwrap();
    let file = first["file"].clone();
    let chunks = file["chunks"].as_u64().unwrap();
    let mut bytes = Vec::new();
    for index in 0..chunks {
        let value = if index == 0 {
            first.clone()
        } else {
            run(f, 4, &export(state, selection.clone(), index)).unwrap()
        };
        assert_eq!(value["file"], file, "every chunk describes the same file");
        assert_eq!(value["index"], index);
        let chunk = crate::b64_decode(value["base64"].as_str().unwrap()).unwrap();
        if index + 1 < chunks {
            assert_eq!(chunk.len(), CHUNK, "only the last chunk is short");
        }
        bytes.extend(chunk);
    }
    assert_eq!(bytes.len() as u64, file["byteLength"].as_u64().unwrap());
    assert_eq!(file["sha256"], hex(&bytes));
    (bytes, file)
}

#[test]
fn an_activation_file_is_the_desktop_export_byte_for_byte_and_nothing_else_from_the_log() {
    let f = Fixture::new();
    seed(&f);
    let state = lease(&f);
    let listed = run(&f, 4, &export(&state, Value::Null, 0)).unwrap();
    let activations: Vec<tempo_app::dto::LoggedActivationDto> = f
        .engine
        .lock()
        .unwrap()
        .log_activations()
        .into_iter()
        .map(Into::into)
        .collect();
    assert_eq!(activations.len(), 3, "US-1234 on two UTC days, and US-5678");
    assert_eq!(
        listed,
        json!({"operation":"activationExport","activations":activations})
    );
    let (bytes, _) = download(&f, &state, &selection("US-1234", DAY));
    let desktop =
        f.engine
            .lock()
            .unwrap()
            .export_logbook_for_activation("US-1234", DAY, Some("W9XYZ"));
    assert_eq!(
        bytes,
        desktop.as_bytes(),
        "exactly the file the desktop's per-activation export writes"
    );
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("K1AAA") && text.contains("K1BBB"));
    // Positive control: the other contacts really are in the log, and none of them rides along.
    let e = f.engine.lock().unwrap();
    for call in ["K1CCC", "K1DDD", "K1EEE"] {
        assert!(
            e.log_records().iter().any(|r| r.call == call),
            "{call} is in the log"
        );
        assert!(!text.contains(call), "{call} is not in this activation");
    }
    assert!(!e.tx_enabled());
}

#[test]
fn only_a_listed_activation_can_be_named_and_a_read_spends_no_command() {
    let f = Fixture::new();
    seed(&f);
    let state = lease(&f);
    // Well formed, but not an activation this log lists: nothing is exported.
    for missing in [
        selection("US-9999", DAY),
        selection("US-5678", DAY + 86_400),
        json!({"reference":"US-1234","dayStartUnix":DAY,"callsign":null}),
        json!({"reference":"US-1234","dayStartUnix":DAY,"callsign":"K1AAA"}),
    ] {
        assert_eq!(
            run(&f, 4, &export(&state, missing, 0)).unwrap(),
            json!({"operation":"activationExport","refused":"notFound"})
        );
    }
    // Outside the grammar: refused before the log is read.
    for bad in [
        selection("us-1234", DAY),
        selection("US-1234,US-5678", DAY),
        selection("", DAY),
        selection("US-1234", DAY + 1),
        json!({"reference":"US-1234","dayStartUnix":DAY,"callsign":"w9xyz"}),
    ] {
        assert_eq!(run(&f, 4, &export(&state, bad, 0)), Err("invalidRequest"));
    }
    assert_eq!(
        run(&f, 4, &export(&state, Value::Null, 1)),
        Err("invalidRequest")
    );
    assert_eq!(
        run(&f, 4, &export(&state, selection("US-1234", DAY), 1)),
        Err("invalidRequest"),
        "a one-chunk file has no chunk 1"
    );
    // No field can widen the request to a range, a search or the whole log.
    for extra in [
        json!({"from":0}),
        json!({"to":DAY}),
        json!({"search":"K1"}),
        json!({"all":true}),
    ] {
        let extra = extra.as_object().unwrap().clone();
        let mut top = raw(&state, selection("US-1234", DAY), 0);
        top.as_object_mut().unwrap().extend(extra.clone());
        assert!(serde_json::from_value::<Request>(top).is_err());
        let mut inner = raw(&state, selection("US-1234", DAY), 0);
        inner["selection"].as_object_mut().unwrap().extend(extra);
        assert!(serde_json::from_value::<Request>(inner).is_err());
    }
    for key in ["selection", "index", "leaseId"] {
        let mut missing = raw(&state, selection("US-1234", DAY), 0);
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<Request>(missing).is_err(),
            "{key} is required"
        );
    }
    // A callsign may be null, never omitted: a missing key must not read as "no callsign".
    let mut omitted = raw(&state, selection("US-1234", DAY), 0);
    omitted["selection"]
        .as_object_mut()
        .unwrap()
        .remove("callsign");
    assert!(serde_json::from_value::<Request>(omitted).is_err());
    // A read spends no command: the next write still takes the same sequence.
    let after = control_state_version(&f, Instant::now(), 4);
    assert_eq!(after["nextSequence"], state["nextSequence"]);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 5);
}

#[test]
fn an_export_needs_the_logging_grant_this_browsers_current_lease_and_operation_v4() {
    let f = Fixture::new();
    seed(&f);
    let pick = selection("US-1234", DAY);
    f.authority.permit(DEVICE, true).unwrap();
    let idle = control_state_version(&f, Instant::now(), 4);
    assert_eq!(
        run(&f, 4, &export(&idle, pick.clone(), 0)),
        Err("leaseExpired")
    );
    let state = lease(&f);
    assert!(run(&f, 4, &export(&state, pick.clone(), 0)).unwrap()["file"].is_object());
    let mut other = state.clone();
    other["leaseId"] = json!(id());
    assert_eq!(
        run(&f, 4, &export(&other, pick.clone(), 0)),
        Err("notController")
    );
    let mut stale = state.clone();
    stale["stationBootId"] = json!(id());
    assert_eq!(
        run(&f, 4, &export(&stale, pick.clone(), 0)),
        Err("staleStation")
    );
    // Another browser session cannot read through this lease.
    assert_eq!(
        f.authority.handle_version(
            (f.connection, 4),
            &id(),
            DEVICE,
            &export(&state, pick.clone(), 0),
            &f.engine,
            Instant::now()
        ),
        Err("notController")
    );
    for version in [1, 2, 3] {
        assert_eq!(
            run(&f, version, &export(&state, pick.clone(), 0)),
            Err("stationUnsupported")
        );
    }
    // Station control is not the logging grant.
    f.authority.permit_station(DEVICE, true).unwrap();
    f.authority.permit(DEVICE, false).unwrap();
    assert_eq!(
        run(&f, 4, &export(&state, pick, 0)),
        Err("localPermissionRequired")
    );
    assert!(!f.engine.lock().unwrap().tx_enabled());
}

#[test]
fn a_long_activation_arrives_in_whole_chunks_and_a_file_over_the_bound_is_refused() {
    let at = |i: usize| format!("{:02}{:02}{:02}", i / 3600, i / 60 % 60, i % 60);
    let f = Fixture::new();
    let note = "x".repeat(200);
    let long: String = (0..400)
        .map(|i| {
            contact(
                &format!("K{}A{}", i / 100, i % 100),
                "20260909",
                &at(i),
                Some("US-1234"),
                &note,
            )
        })
        .collect();
    f.engine.lock().unwrap().import_adif(&long);
    assert_eq!(f.engine.lock().unwrap().log_records().len(), 400);
    let state = lease(&f);
    let (bytes, file) = download(&f, &state, &selection("US-1234", DAY));
    assert!(
        file["chunks"].as_u64().unwrap() > 1,
        "positive control: this file really spans chunks"
    );
    assert_eq!(
        bytes,
        f.engine
            .lock()
            .unwrap()
            .export_logbook_for_activation("US-1234", DAY, Some("W9XYZ"))
            .as_bytes()
    );

    let big = Fixture::new();
    let note = "x".repeat(1000);
    let huge: String = (0..1100)
        .map(|i| {
            contact(
                &format!("K{}A{}", i / 100, i % 100),
                "20260909",
                &at(i),
                Some("US-1234"),
                &note,
            )
        })
        .collect();
    big.engine.lock().unwrap().import_adif(&huge);
    let desktop =
        big.engine
            .lock()
            .unwrap()
            .export_logbook_for_activation("US-1234", DAY, Some("W9XYZ"));
    assert!(
        desktop.len() > 1024 * 1024,
        "positive control: the desktop file really is over the bound"
    );
    let state = lease(&big);
    assert_eq!(
        run(&big, 4, &export(&state, selection("US-1234", DAY), 0)).unwrap(),
        json!({"operation":"activationExport","refused":"tooLarge"})
    );
}

#[test]
fn the_list_is_the_logs_newest_activations_and_bounded() {
    let f = Fixture::new();
    let parks: String = (0..130)
        .map(|i| {
            contact(
                &format!("K{}A{}", i / 100, i % 100),
                "20260909",
                "010000",
                Some(&format!("US-{i:04}")),
                "",
            )
        })
        .collect();
    f.engine.lock().unwrap().import_adif(&parks);
    let state = lease(&f);
    let listed = run(&f, 4, &export(&state, Value::Null, 0)).unwrap();
    let all: Vec<tempo_app::dto::LoggedActivationDto> = f
        .engine
        .lock()
        .unwrap()
        .log_activations()
        .into_iter()
        .map(Into::into)
        .collect();
    assert_eq!(all.len(), 130, "positive control: more than the bound");
    assert_eq!(listed["activations"], json!(all[..128]));
}
