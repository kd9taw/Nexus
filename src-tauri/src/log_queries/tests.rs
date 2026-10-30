//! The engine's answers held to the UI's: C17b's goldens on both of the log's homes (the store,
//! and the 1.13 path's store in memory beside `log.adi`), and a randomized parity against the
//! reference over the engine's own log after every kind of change. Then the revisions, the kept
//! orders, read-your-writes, the Engine lock left free while an answer waits, and the calls the
//! store and the UI fold differently.

use super::*;
use crate::remote_service::stored_log_tests::StoredLog;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tempo_app::engine::engine_try_lock;
use tempo_app::logstore::NOT_ANSWERED;
use tempo_core::logbook::query::{
    reference, BandsInLog, GridCounts, LogStatCounter, LotwBacklog, WorkedGrids,
};
use tempo_core::logbook::{
    adif_header, adif_record_own_log, LogOp, QsoEdit, UploadOutcome, UploadService, UploadStatus,
};

// ── fixtures ────────────────────────────────────────────────────────────────────────────────────

const GOLDEN_LOG: &str = include_str!("../../../ui/src/features/__fixtures__/log-query/log.json");
/// The added columns' orders, the one table both sides are held to.
const SORT_COLUMNS: &str =
    include_str!("../../../ui/src/features/__fixtures__/log-query/sort-columns.json");
const GOLDEN_ANSWERS: &str =
    include_str!("../../../ui/src/features/__fixtures__/log-query/answers.json");
/// The statistics' counts over the golden log — what the engine answers, and the window finishes
/// into the golden answer (`ui/src/features/logStats.countFinish.test.ts` pins that).
const STATISTICS: &str =
    include_str!("../../../ui/src/features/__fixtures__/log-query/statistics.json");

fn golden_statistics() -> Value {
    let all: Vec<Value> = serde_json::from_str(STATISTICS).expect("statistics.json");
    all.into_iter()
        .find(|x| x["name"] == "golden")
        .expect("the golden log's counts")["counts"]
        .clone()
}

/// A folder of the test's own, gone with the value.
struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Dir {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let p = std::env::temp_dir().join(format!(
            "nexus-logq-{tag}-{}-{}-{nanos}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).expect("scratch");
        Dir(p)
    }
    fn log(&self) -> PathBuf {
        self.0.join("log.adi")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The golden fixture's id `fxNN` as the record id the engine holds it under, and back.
fn golden_id(name: &str) -> RecordId {
    RecordId::Provisional {
        hash: name.trim_start_matches("fx").parse().expect("fxNN"),
        ordinal: 0,
    }
}
fn golden_name(id: &str) -> String {
    match id.parse::<RecordId>() {
        Ok(RecordId::Provisional { hash, .. }) => format!("fx{hash:02}"),
        _ => id.to_string(),
    }
}

/// The golden log as records, and each call's entity as the fixture resolves it.
fn golden_log() -> (Vec<QsoRecord>, HashMap<String, Option<String>>) {
    let rows: Vec<Value> = serde_json::from_str(GOLDEN_LOG).expect("log.json");
    let mut entities = HashMap::new();
    let records = rows
        .into_iter()
        .map(|mut v| {
            let name = v["id"].as_str().expect("id").to_string();
            // The fixture writes a missing park as `null`, which the DTO reads as absent.
            if v["ota"].is_null() {
                v.as_object_mut().expect("a row").remove("ota");
            }
            let entity = v["entity"].as_str().map(str::to_string);
            let dto: LoggedQso = serde_json::from_value(v).expect("a golden row is a LoggedQso");
            let call = dto.call.clone();
            let mut r = QsoRecord::from(dto);
            r.id = Some(golden_id(&name));
            assert_eq!(
                entities
                    .insert(call.clone(), entity.clone())
                    .unwrap_or(entity.clone()),
                entity,
                "{call}: one entity per call"
            );
            r
        })
        .collect();
    (records, entities)
}

fn golden_answers() -> Vec<(Value, Value)> {
    let all: Vec<Value> = serde_json::from_str(GOLDEN_ANSWERS).expect("answers.json");
    all.into_iter()
        .map(|x| (x["q"].clone(), x["a"].clone()))
        .collect()
}

/// `records` written as the operator's `log.adi`. A contact at exactly 00:00:00 UTC whose time of
/// day is known says so in ADIF by its TIME_OFF — the reader takes a bare midnight TIME_ON for a
/// date-only import — so a row like that (the golden log's fx09) is written with one, and both
/// homes of the log hold it as the fixture has it.
fn write_log(d: &Dir, records: &[QsoRecord]) {
    let mut adif = adif_header();
    for r in records {
        let mut r = r.clone();
        if r.time_known && r.when_unix % 86_400 == 0 && r.time_off_unix.is_none() {
            r.time_off_unix = Some(r.when_unix);
        }
        adif.push_str(&adif_record_own_log(&r));
    }
    std::fs::write(d.log(), adif).expect("log.adi");
}

/// An engine whose log is owned by the store, opened from `records`.
fn on_store(d: &Dir, records: &[QsoRecord]) -> SharedEngine {
    write_log(d, records);
    let opened = tempo_app::logstore::open(
        &d.log(),
        Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
        None,
    )
    .expect("the store opens");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.attach_log_store(opened);
    Arc::new(Mutex::new(e))
}

/// An engine on the 1.13 path: the log read from `log.adi` into a store in memory.
fn on_log_file(d: &Dir, records: &[QsoRecord]) -> SharedEngine {
    write_log(d, records);
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_log_path(d.log());
    Arc::new(Mutex::new(e))
}

fn ask(
    queries: &LogQueries,
    engine: &SharedEngine,
    q: Value,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Value {
    let q: LogQuestion = serde_json::from_value(q.clone()).unwrap_or_else(|e| panic!("{q}: {e}"));
    let by_call = |r: &QsoRecord| resolve(&r.call);
    queries
        .answer(engine, &q, &by_call, &by_call, &by_call)
        .unwrap_or_else(|e| panic!("{q:?}: {e}"))
}

/// [`ask`], with the refusal answered rather than a panic.
fn try_ask(
    queries: &LogQueries,
    engine: &SharedEngine,
    q: Value,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Result<Value, String> {
    let q: LogQuestion = serde_json::from_value(q.clone()).unwrap_or_else(|e| panic!("{q}: {e}"));
    let by_call = |r: &QsoRecord| resolve(&r.call);
    queries.answer(engine, &q, &by_call, &by_call, &by_call)
}

/// [`ask`], asked again while it is refused for a change still on its way to disk
/// ([`NOT_ANSWERED`]) — as the window asks again — for up to a test's budget
/// ([`tempo_app::test_util::TEST_WAIT`]). Any other refusal fails, and the first answer is the one
/// returned: never a retry until green.
fn answered(
    queries: &LogQueries,
    engine: &SharedEngine,
    q: Value,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Value {
    let deadline = std::time::Instant::now() + tempo_app::test_util::TEST_WAIT;
    loop {
        match try_ask(queries, engine, q.clone(), resolve) {
            Ok(answer) => return answer,
            Err(why) if why.starts_with(NOT_ANSWERED) && std::time::Instant::now() < deadline => {}
            Err(why) => panic!("{q}: refused: {why}"),
        }
    }
}

/// An answer as the golden file stores it: a row as its id, a page as its numbers and keys.
fn normalise(q: &Value, a: Value) -> Value {
    let id = |row: &Value| -> Value {
        match row {
            Value::Null => Value::Null,
            row => json!(golden_name(
                row["id"].as_str().expect("a row carries its id")
            )),
        }
    };
    match q["kind"].as_str().expect("kind") {
        "page" => json!({
            "total": a["total"],
            "logSize": a["logSize"],
            "offset": a["offset"],
            "keys": a["keys"].as_array().unwrap().iter()
                .map(|k| json!(golden_name(k.as_str().unwrap()))).collect::<Vec<_>>(),
        }),
        "locate" => json!({"index": a["index"]}),
        "callHistory" => {
            let mut a = a;
            a["qsos"] = json!(a["qsos"]
                .as_array()
                .unwrap()
                .iter()
                .map(id)
                .collect::<Vec<_>>());
            a
        }
        "rowsAt" => json!(a.as_array().unwrap().iter().map(id).collect::<Vec<_>>()),
        "row" => id(&a),
        _ => a,
    }
}

// ── the goldens ─────────────────────────────────────────────────────────────────────────────────

/// The answer to a golden question from the reference over `log`, in the form the golden file
/// stores it: a row as its fixture id, a page as its numbers and keys, the statistics as their
/// counts. The UI's reading of each question, so that over the golden log as written it IS the
/// golden answer — which [`every_golden_answer_on_both_homes_of_the_log`] checks first.
fn reference_answer(
    log: &[QsoRecord],
    q: &Value,
    entities: &HashMap<String, Option<String>>,
) -> Value {
    let name = |i: usize| json!(golden_name(&log[i].id.expect("an id").to_string()));
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let by_call = |r: &QsoRecord| resolve(&r.call);
    let calls = || -> Vec<String> { serde_json::from_value(q["calls"].clone()).expect("calls") };
    let query = || -> LogQuery { serde_json::from_value(q["query"].clone()).expect("a query") };
    match q["kind"].as_str().expect("kind") {
        "page" => {
            let order = reference::order(log, &query(), &by_call);
            let (offset, limit) = (
                q["offset"].as_u64().expect("offset") as usize,
                q["limit"].as_u64().expect("limit") as usize,
            );
            json!({
                "total": order.len(),
                "logSize": log.len(),
                "offset": offset,
                "keys": order.iter().skip(offset).take(limit).map(|&i| name(i)).collect::<Vec<_>>(),
            })
        }
        "locate" => {
            let id = golden_id_text(q["id"].as_str().expect("id"));
            let order = reference::order(log, &query(), &by_call);
            json!({"index": order.iter().position(|&i| log[i].id.map(|r| r.to_string()) == Some(id.clone()))})
        }
        "callHistory" => {
            let text = |k: &str| q[k].as_str().expect("text").to_string();
            let h = reference::call_history(
                log,
                &text("call"),
                &text("band"),
                &text("mode"),
                q["matchMode"].as_bool().expect("matchMode"),
            );
            json!({
                "count": h.count, "workedBefore": h.worked_before, "dupeThisBand": h.dupe_this_band,
                "lastUnix": h.last_unix, "confirmedCount": h.confirmed_count,
                "bands": h.bands, "modes": h.modes,
                "qsos": h.qsos.iter().map(|&i| name(i)).collect::<Vec<_>>(),
            })
        }
        "entity" => {
            let entity = q["entity"].as_str().expect("entity");
            serde_json::to_value(reference::entity(log, entity, &by_call)).expect("json")
        }
        "callsSummary" => Value::Object(
            query::calls_summary(log, &calls())
                .into_iter()
                .map(|(c, s)| (c, serde_json::to_value(s).expect("json")))
                .collect(),
        ),
        "workedCalls" => json!(reference::worked_calls(log, &calls())),
        "workedGrids" => json!(reference::fold(log, WorkedGrids::default())),
        "gridPoints" => json!(reference::fold(
            log,
            GridCounts::new(q["band"].as_str().expect("band"))
        )),
        "bandsInLog" => json!(reference::fold(log, BandsInLog::default())),
        "statistics" => json!(reference::fold(log, LogStatCounter::new(&by_call))),
        "lotwBacklog" => json!(reference::fold(log, LotwBacklog::default())),
        "rowsAt" => json!(q["indices"]
            .as_array()
            .expect("indices")
            .iter()
            .map(|i| match i.as_i64() {
                Some(i) if i >= 0 && (i as usize) < log.len() => name(i as usize),
                _ => Value::Null,
            })
            .collect::<Vec<_>>()),
        "row" => {
            let id = q["id"].as_str().expect("id");
            match log
                .iter()
                .position(|r| r.id.map(|i| i.to_string()) == Some(golden_id_text(id)))
            {
                Some(i) => name(i),
                None => Value::Null,
            }
        }
        "logSize" => json!(log.len()),
        other => panic!("a golden question this reading does not know: {other}"),
    }
}

/// A fixture id as the record id text the engine holds it under, or the text itself for an id
/// that is not a fixture's (`no-such-id`).
fn golden_id_text(name: &str) -> String {
    if name.starts_with("fx") {
        golden_id(name).to_string()
    } else {
        name.to_string()
    }
}

/// ★ THE ENGINE ANSWERS WHAT THE UI ANSWERED. Every golden question, sent as the UI sends it (the
/// question JSON, parsed as the command parses it), is answered exactly as the golden file froze
/// it — the statistics as the counts the window finishes into it — with the log in the store, and
/// on the 1.13 path, where since SPEC-2 v3 C19 (D1-A) it is in a store in memory beside
/// `log.adi`.
///
/// ⭐ **Over the log as the engine holds it.** The golden log carries three rows in a form the log
/// no longer holds — fx15's band `20M`, and fx03 and fx04 with the sideband in MODE (`USB`,
/// `LSB`) — and taken in they are `20m`, and SSB with the sideband as SUBMODE
/// (`tempo_core::logbook::canonical`). So each question is answered first by the reference over
/// the log as written, which must give the golden answer (this reading of every question is the
/// UI's), then by the reference over the log as held, which the engine must give. The answers
/// the holding moves are named below; every other one is the frozen golden, unchanged.
#[test]
fn every_golden_answer_on_both_homes_of_the_log() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let held: Vec<QsoRecord> = records
        .iter()
        .cloned()
        .map(|mut r| {
            tempo_core::logbook::canonical::contact(&mut r);
            r
        })
        .collect();
    let mut moved = Vec::new();
    let mut expected = Vec::new();
    for (q, a) in golden_answers() {
        let want = if q["kind"] == "statistics" {
            golden_statistics()
        } else {
            a
        };
        assert_eq!(
            reference_answer(&records, &q, &entities),
            want,
            "the reference over the golden log as written: {q}"
        );
        let as_held = reference_answer(&held, &q, &entities);
        if as_held != want {
            moved.push(q.to_string());
        }
        expected.push((q, as_held));
    }
    // Each for a reason a reader can check against the three rows: fx03 and fx04 sort and list
    // as SSB, and K1ABC's history lists one mode; fx15's square is a 20 m square, its band is one
    // of the log's bands, and the statistics count it under 20 m. The search for "usb" does not
    // move: it finds fx03 by the sideband held as its SUBMODE.
    assert_eq!(
        moved,
        [
            r#"{"kind":"page","limit":24,"offset":0,"query":{"asc":true,"needsConfirmOnly":false,"search":"","sort":"mode"}}"#,
            r#"{"kind":"page","limit":24,"offset":0,"query":{"asc":false,"needsConfirmOnly":false,"search":"","sort":"mode"}}"#,
            r#"{"kind":"page","limit":24,"offset":0,"query":{"asc":false,"needsConfirmOnly":false,"search":"ssb","sort":"mode"}}"#,
            r#"{"band":"40M","call":" k1abc ","kind":"callHistory","matchMode":true,"mode":"SSB"}"#,
            r#"{"band":"40m","call":"K1ABC","kind":"callHistory","matchMode":true,"mode":"CW"}"#,
            r#"{"band":"40m","call":"K1ABC","kind":"callHistory","matchMode":false,"mode":"CW"}"#,
            r#"{"band":"all","kind":"gridPoints"}"#,
            r#"{"band":"20m","kind":"gridPoints"}"#,
            r#"{"kind":"bandsInLog"}"#,
            r#"{"kind":"statistics"}"#,
        ],
        "the answers holding the log in one form moves, and no other"
    );
    for (home, d) in [
        ("store", Dir::new("golden-store")),
        ("1.13 path", Dir::new("golden-mem")),
    ] {
        let engine = if home == "store" {
            on_store(&d, &records)
        } else {
            on_log_file(&d, &records)
        };
        assert_eq!(
            engine_lock(&engine).log_on_file(),
            home != "store",
            "{home}: premise"
        );
        let queries = LogQueries::default();
        let mut answered = 0;
        for (q, want) in &expected {
            // A question naming a row names it by the fixture's id; the engine holds it under
            // the record id that id stands for.
            let mut asked = q.clone();
            if let Some(name) = q["id"].as_str().filter(|n| n.starts_with("fx")) {
                asked["id"] = json!(golden_id(name).to_string());
            }
            let got = ask(&queries, &engine, asked, &resolve);
            assert_eq!(&normalise(q, got), want, "{home}: {q}");
            answered += 1;
        }
        assert_eq!(answered, 78, "{home}: every question in the golden file");
    }
}

/// ★ The Logbook's search finds a phone contact by its sideband. The log holds a phone contact as
/// SSB with the sideband as its SUBMODE, so a search for `usb` finds the contact logged on USB
/// (as Nexus's phone cockpit logs it) and the one another logger wrote as `MODE=SSB SUBMODE=USB`,
/// `lsb` the LSB one, and `ssb` all three; on the store, where the sideband is a row of
/// `qso_extra`, and on the 1.13 path.
#[test]
fn a_search_for_the_sideband_finds_the_phone_contacts_held_as_ssb() {
    let (golden, _) = golden_log();
    let log: Vec<QsoRecord> = [
        ("K1AAA", "USB", None),
        ("K2BBB", "SSB", Some("USB")),
        ("K3CCC", "LSB", None),
        ("K4DDD", "FT8", None),
    ]
    .iter()
    .enumerate()
    .map(|(i, &(call, mode, submode))| {
        let mut r = golden[0].clone();
        r.id = Some(RecordId::Provisional {
            hash: 950 + i as u64,
            ordinal: 0,
        });
        r.call = call.to_string();
        r.mode = mode.to_string();
        r.extra = submode
            .map(|s| vec![("SUBMODE".to_string(), s.to_string())])
            .unwrap_or_default();
        r.when_unix = 1_700_000_000 + i as u64 * 60;
        r
    })
    .collect();
    let resolve = |_: &str| None;
    for (home, d) in [
        ("store", Dir::new("sideband-store")),
        ("1.13 path", Dir::new("sideband-mem")),
    ] {
        let engine = if home == "store" {
            on_store(&d, &log)
        } else {
            on_log_file(&d, &log)
        };
        let queries = LogQueries::default();
        for (search, want) in [
            ("usb", vec!["K1AAA", "K2BBB"]),
            ("lsb", vec!["K3CCC"]),
            ("ssb", vec!["K1AAA", "K2BBB", "K3CCC"]),
        ] {
            let page = ask(
                &queries,
                &engine,
                json!({"kind": "page", "query": {"sort": "call", "asc": true, "search": search,
                    "needsConfirmOnly": false}, "offset": 0, "limit": 10}),
                &resolve,
            );
            let calls: Vec<&str> = page["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .map(|r| r["call"].as_str().expect("a call"))
                .collect();
            assert_eq!(calls, want, "{home}: {search}");
        }
    }
}

/// A page carries what C17b's view reconciles pages by: the query it answers, the revisions it
/// was cut at, and a key per row that is the row's own id.
#[test]
fn a_page_names_its_query_its_revisions_and_its_rows() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("page-shape");
    let engine = on_store(&d, &records);
    let query = json!({"sort": "call", "asc": true, "search": "", "needsConfirmOnly": false});
    let a = ask(
        &LogQueries::default(),
        &engine,
        json!({"kind": "page", "query": query, "offset": 2, "limit": 3}),
        &resolve,
    );
    let (revision, index_rev, content_rev) = {
        let e = engine_lock(&engine);
        (e.log_revision(), e.log_index_rev(), e.log_content_rev())
    };
    assert_eq!(a["query"], query, "the query, echoed");
    assert_eq!(
        (&a["revision"], &a["orderRev"], &a["contentRev"]),
        (&json!(revision), &json!(index_rev), &json!(content_rev))
    );
    let rows = a["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for (row, key) in rows.iter().zip(a["keys"].as_array().unwrap()) {
        assert_eq!(&row["id"], key, "a row's key is its id");
        assert_eq!(
            row["entity"].as_str(),
            resolve(row["call"].as_str().unwrap()).as_deref(),
            "the row carries its live entity, as every row the UI is handed does"
        );
    }
}

/// ★ The Logbook's added columns read what the STORE holds. A page row, read from a real store,
/// carries everything those columns print: the contest block (on the row since the columns came),
/// the IOTA reference, the passthrough tags — county, zones, continent, distance, the QSL
/// received dates — and each service's upload stamp. `ui/src/components/Logbook.columns.test.tsx`
/// renders a row of this shape.
#[test]
fn a_page_row_carries_what_the_logbook_columns_show() {
    let mut records = tempo_core::logbook::parse_adif(
        "<CALL:6>DL1ABC<QSO_DATE:8>20250102<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW<FREQ:6>14.025\
         <CONTEST_ID:8>CQ-WW-CW<STX:2>12<SRX:3>345<SRX_STRING:2>14<IOTA:6>EU-005\
         <CNTY:12>MA,MIDDLESEX<CQZ:2>14<ITUZ:2>28<CONT:2>EU<DISTANCE:6>6512.4\
         <QSLRDATE:8>20250105<LOTW_QSLRDATE:8>20250203<EQSL_QSLRDATE:8>20250304<EOR>",
    );
    records[0].upload.lotw = Some(accepted(1_736_000_000));
    let d = Dir::new("logbook-columns");
    let engine = on_store(&d, &records);
    let a = ask(
        &LogQueries::default(),
        &engine,
        json!({"kind": "page", "query": {"sort": "time", "asc": false, "search": "",
            "needsConfirmOnly": false}, "offset": 0, "limit": 5}),
        &|_| None,
    );
    let row = &a["rows"][0];
    assert_eq!(row["call"], "DL1ABC", "premise: the contact is on the page");
    assert_eq!(row["contest"]["contestId"], "CQ-WW-CW");
    assert_eq!(row["contest"]["stx"], 12);
    assert_eq!(row["contest"]["srx"], 345);
    assert_eq!(row["contest"]["srxString"], "14");
    assert_eq!(row["ota"]["iota"], "EU-005");
    assert_eq!(row["upload"]["lotw"]["outcome"], "accepted");
    let extra: HashMap<String, String> = serde_json::from_value(row["extra"].clone())
        .map(|v: Vec<(String, String)>| v.into_iter().collect())
        .expect("extra pairs");
    for (tag, value) in [
        ("CNTY", "MA,MIDDLESEX"),
        ("CQZ", "14"),
        ("ITUZ", "28"),
        ("CONT", "EU"),
        ("DISTANCE", "6512.4"),
        ("QSLRDATE", "20250105"),
        ("LOTW_QSLRDATE", "20250203"),
        ("EQSL_QSLRDATE", "20250304"),
    ] {
        assert_eq!(extra.get(tag).map(String::as_str), Some(value), "{tag}");
    }
}

/// Whether a change by id with the edit key `key` would be taken now: the contact `id` is still
/// the version whose key it is — asked of the store with the Engine lock released, as a change
/// asks it.
fn key_taken(engine: &SharedEngine, id: RecordId, key: &str) -> bool {
    let view = engine_lock(engine).log_view();
    view.row(id)
        .expect("the log reads")
        .is_some_and(|row| tempo_app::station::StationCore::fresh_row(&row, key).is_ok())
}

/// ★ A PAGE HANDS OUT EACH ROW'S EDIT KEY (SPEC-2 v2 §1): the key a change by id is checked
/// against (`QsoEdit::key`, never computed by the UI). The Logbook sends it back with the row's
/// id, so the key a page gives must be the one the change accepts — on both homes of the log —
/// and once the row has changed, the old key is refused and the next page hands out the new one.
#[test]
fn a_page_hands_out_the_edit_key_a_change_by_id_accepts() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    for (home, d) in [
        ("store", Dir::new("edit-keys-store")),
        ("1.13 path", Dir::new("edit-keys-mem")),
    ] {
        let engine = if home == "store" {
            on_store(&d, &records)
        } else {
            on_log_file(&d, &records)
        };
        let queries = LogQueries::default();
        let page = || {
            ask(
                &queries,
                &engine,
                json!({"kind": "page", "offset": 0, "limit": 6,
                    "query": {"sort": "call", "asc": true, "search": "", "needsConfirmOnly": false}}),
                &resolve,
            )
        };
        let a = page();
        let keys = a["keys"].as_array().unwrap();
        let edit_keys = a["editKeys"]
            .as_array()
            .unwrap_or_else(|| panic!("{home}: the page hands out no edit keys: {a}"));
        assert_eq!(edit_keys.len(), keys.len(), "{home}: one edit key per row");
        for (id, key) in keys.iter().zip(edit_keys) {
            let id: RecordId = id.as_str().unwrap().parse().unwrap();
            let key = key.as_str().expect("an edit key is text");
            assert!(
                key_taken(&engine, id, key),
                "{home}: {id}'s key from the page is refused"
            );
        }
        // The row changes (a paper card arrives, in another window): its old key is refused, and
        // the next page hands out the one that is accepted now.
        let id: RecordId = keys[2].as_str().unwrap().parse().unwrap();
        let before = edit_keys[2].as_str().unwrap().to_string();
        assert!(
            change(&engine, id, &[LogOp::MarkQslCard { id, received: true }]),
            "premise: marked"
        );
        // Asked once the store holds the mark: straight after it, a question on a slow disk waits
        // its two seconds and is refused (P4), as the read-your-writes tests show.
        engine.caught_up();
        assert!(
            !key_taken(&engine, id, &before),
            "{home}: the key of a row that changed since is accepted"
        );
        let after = page();
        assert_eq!(
            after["keys"][2], keys[2],
            "{home}: the same row, in the same place"
        );
        let now = after["editKeys"][2].as_str().unwrap();
        assert_ne!(now, before, "{home}: the page still hands out the old key");
        assert!(key_taken(&engine, id, now));
    }
}

// ── the revisions, and the kept orders ─────────────────────────────────────────────────────────

fn snapshot_tick(engine: &SharedEngine) -> u32 {
    engine_lock(engine).snapshot().log_tick
}

/// The contact `id` as the log holds it — read with the Engine lock released, as every read of
/// the store is.
fn logged(engine: &SharedEngine, id: RecordId) -> QsoRecord {
    let view = engine_lock(engine).log_view();
    QsoRecord::clone(&view.row(id).expect("the log reads").expect("held"))
}

/// `ops` made on the contact `id` as a command makes them: planned with the Engine lock released,
/// made under it ([`tempo_app::logwrite`]). Whether they were made.
fn change(engine: &SharedEngine, id: RecordId, ops: &[LogOp]) -> bool {
    let (made, _) = tempo_app::logwrite::change_ops(engine, id, None, ops, "test");
    matches!(made, Ok(Ok(_)))
}

/// A connector's upload status: accepted at `when_unix`.
fn accepted(when_unix: i64) -> UploadStatus {
    UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix,
        detail: None,
    }
}

/// ★ The Logbook shows a contact under the entity its record's DXCC number names, once the
/// operator sets it by hand — the country column, the column's order and the callsign card's
/// slots alike, read from the store with the app's own resolvers. Two K1N contacts: the 2015
/// Navassa operation, set to 182 by hand, and a US special event that stays the United States.
/// Each answer is asked before the change too, so a kept order or entity index that outlived it
/// would show.
#[test]
fn the_logbook_shows_sorts_and_keys_a_contact_by_the_dxcc_set_by_hand() {
    let d = Dir::new("dxcc-by-hand");
    let rows = [
        "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:4>1234<BAND:3>20m<MODE:2>CW<EOR>",
        "<CALL:3>K1N<QSO_DATE:8>20180704<TIME_ON:4>1500<BAND:3>40m<MODE:3>SSB<EOR>",
        "<CALL:6>XE1ABC<QSO_DATE:8>20200101<TIME_ON:4>1200<BAND:3>15m<MODE:3>FT8<EOR>",
    ];
    let records: Vec<QsoRecord> = rows
        .iter()
        .map(|r| tempo_core::logbook::parse_adif(r).remove(0))
        .collect();
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let app = |q: Value| {
        let q: LogQuestion = serde_json::from_value(q).expect("a question");
        queries
            .answer(&engine, &q, &shown, &on_card, &counted)
            .expect("answered")
    };
    let by_country =
        json!({"sort": "country", "asc": true, "search": "", "needsConfirmOnly": false});
    // Each row as (call, its entity), in the order the column gives.
    let page = || -> Vec<(String, String)> {
        let page = app(json!({"kind": "page", "query": by_country, "offset": 0, "limit": 10}));
        page["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| {
                (
                    r["call"].as_str().expect("call").to_string(),
                    r["entity"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    };
    let navassa = || app(json!({"kind": "entity", "entity": "Navassa Island"}));
    let pair = |call: &str, entity: &str| (call.to_string(), entity.to_string());
    assert_eq!(
        page(),
        [
            pair("XE1ABC", "Mexico"),
            pair("K1N", "United States"),
            pair("K1N", "United States"),
        ],
        "premise: cty.dat places both K1N contacts in the United States"
    );
    assert_eq!(navassa()["newEntity"], json!(true), "premise");

    let id = engine.lock().unwrap().stored_log()[0].id.expect("an id");
    assert!(change(
        &engine,
        id,
        &[LogOp::SetDxcc {
            id,
            dxcc: Some(182),
            country: None,
        }]
    ));
    assert_eq!(
        page(),
        [
            pair("XE1ABC", "Mexico"),
            pair("K1N", "Navassa Island"),
            pair("K1N", "United States"),
        ]
    );
    let card = navassa();
    assert_eq!(card["newEntity"], json!(false));
    assert_eq!(
        (&card["slots"]["bandsWorked"], &card["slots"]["modesWorked"]),
        (&json!(["20M"]), &json!(["CW"])),
        "the 2015 contact's slots, and not the special event's"
    );
    let us = app(json!({"kind": "entity", "entity": "United States"}));
    assert_eq!(us["slots"]["bandsWorked"], json!(["40M"]));
}

// ── a typed call's card asks for its DXCC entity ───────────────────────────────────────────────

/// `q` answered from the store as `ask_log` answers it, with the app's own resolvers, asked again
/// while a change is still on its way to disk, as the window asks again.
fn app_answer(engine: &SharedEngine, q: Value) -> Value {
    let q: LogQuestion = serde_json::from_value(q).expect("a question");
    let deadline = std::time::Instant::now() + tempo_app::test_util::TEST_WAIT;
    loop {
        match with_app_resolvers(&LogQueries::default(), engine, &q) {
            Ok(a) => return a,
            Err(why) if why.starts_with(NOT_ANSWERED) && std::time::Instant::now() < deadline => {}
            Err(why) => panic!("{q:?}: refused: {why}"),
        }
    }
}

/// A typed call's callsign card, as the log form and Operate make it: the entity it asks for
/// (`resolve_entity`), and that entity's answer — (entity, `newEntity`, `bandsWorked`,
/// `modesWorked`).
type Card = (Option<String>, Value, Value, Value);

fn card_of(engine: &SharedEngine, call: &str) -> Card {
    let entity = crate::resolve_entity(call.to_string());
    let a = app_answer(
        engine,
        json!({"kind": "entity", "entity": entity.clone().unwrap_or_default()}),
    );
    let slots = &a["slots"];
    (
        entity,
        a["newEntity"].clone(),
        slots["bandsWorked"].clone(),
        slots["modesWorked"].clone(),
    )
}

/// A card that found its entity worked, on these bands and modes.
fn worked(entity: &str, bands: &[&str], modes: &[&str]) -> Card {
    (
        Some(entity.to_string()),
        json!(false),
        json!(bands),
        json!(modes),
    )
}

/// ★ A typed call in a WAE area asks its card for the DXCC entity its contacts are shown under:
/// IT9XYZ asks for Italy, the entity a Sicily contact carrying Italy's 248 is shown as, so the card
/// finds that contact worked, on its band and in its mode, as the statistics (which count by
/// number) do.
#[test]
fn a_typed_wae_call_asks_its_card_for_the_entity_its_contacts_are_shown_under() {
    let d = Dir::new("card-wae");
    let it9 = tempo_core::logbook::parse_adif(
        "<CALL:6>IT9ABC<QSO_DATE:8>20240101<TIME_ON:4>1200<BAND:3>20m<MODE:2>CW\
         <DXCC:3>248<COUNTRY:6>Sicily<EOR>",
    );
    let engine = on_store(&d, &it9);
    assert_eq!(
        card_of(&engine, "IT9XYZ"),
        worked("Italy", &["20M"], &["CW"])
    );
}

/// ★ Every WAE area a call reaches, with the two kinds of contact a log holds for it: one carrying
/// the parent's DXCC number (an import, or a number a LoTW confirmation filled in) and one logged in
/// Nexus, which is stored with no number and its area's name, and which the Logbook still shows
/// under the area. A typed call in the area and a typed call in the parent both ask their card for
/// the parent, and both see the two contacts. Vienna Intl Ctr is reached by no call: cty.dat places
/// every Vienna call in Austria.
#[test]
fn a_wae_areas_contacts_are_on_its_card_and_its_parents_with_or_without_a_number() {
    let mut got = Vec::new();
    let mut want = Vec::new();
    // (the area's contact, a typed call in the area, a typed call in the parent, number, parent, area)
    for (call, typed, parent_call, code, parent, area) in [
        ("IT9ABC", "IT9XYZ", "I1ABC", 248, "Italy", "Sicily"),
        ("IG9ABC", "IH9XYZ", "I1ABC", 248, "Italy", "African Italy"),
        (
            "2M0BDR",
            "2M0ZET",
            "GM4ABC",
            279,
            "Scotland",
            "Shetland Islands",
        ),
        ("JW0BEA", "JW5RIA", "JW1ABC", 259, "Svalbard", "Bear Island"),
        (
            "TA1ABC",
            "TC1XYZ",
            "TA2ABC",
            390,
            "Asiatic Turkey",
            "European Turkey",
        ),
    ] {
        let placed = |c: &str| propagation::dxcc::resolve(c).map(|i| i.entity);
        assert_eq!(
            (placed(call), placed(typed), placed(parent_call)),
            (Some(area), Some(area), Some(parent)),
            "premise: cty.dat places {call} and {typed} in {area}, {parent_call} in {parent}"
        );
        let d = Dir::new("card-wae-areas");
        write_log(
            &d,
            &tempo_core::logbook::parse_adif(&format!(
                "<CALL:{}>{call}<QSO_DATE:8>20240101<TIME_ON:4>1200<BAND:3>20m<MODE:2>CW\
                 <DXCC:3>{code}<EOR>",
                call.len()
            )),
        );
        let opened = tempo_app::logstore::open(
            &d.log(),
            Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
            None,
        )
        .expect("the store opens");
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.attach_log_store(opened);
        // The station's own country resolvers, as the launch wires them.
        crate::wire_country_resolvers(&mut e);
        let engine: SharedEngine = Arc::new(Mutex::new(e));
        // Logged as the log form logs it.
        engine_lock(&engine).log_qso(
            tempo_core::logbook::parse_adif(&format!(
                "<CALL:{}>{call}<QSO_DATE:8>20250101<TIME_ON:4>1200<BAND:3>40m<MODE:3>SSB<EOR>",
                call.len()
            ))
            .remove(0),
        );
        // Each contact as the Logbook's page holds it, oldest first: its number, its COUNTRY, and
        // the entity its Country column shows.
        let page = app_answer(
            &engine,
            json!({"kind": "page", "offset": 0, "limit": 10, "query":
                {"sort": "time", "asc": true, "search": "", "needsConfirmOnly": false}}),
        );
        let held: Vec<(Value, Value, Value)> = page["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| (r["dxcc"].clone(), r["country"].clone(), r["entity"].clone()))
            .collect();
        got.push((
            area,
            held,
            card_of(&engine, typed),
            card_of(&engine, parent_call),
        ));
        let both = worked(parent, &["20M", "40M"], &["CW", "SSB"]);
        want.push((
            area,
            vec![
                (json!(code), Value::Null, json!(parent)),
                (Value::Null, json!(area), json!(area)),
            ],
            both.clone(),
            both,
        ));
    }
    assert_eq!(got, want);
}

/// A typed call outside the WAE areas asks its card for the entity it always did, and sees what it
/// always saw: Germany's contacts with and without a number, Navassa's K1N carrying 182 on a KP1
/// call's card and not on a US call's, and nothing for a Mexican call.
#[test]
fn a_typed_call_outside_the_wae_areas_asks_its_card_for_its_own_entity() {
    let d = Dir::new("card-non-wae");
    let engine = on_store(
        &d,
        &tempo_core::logbook::parse_adif(
            "<CALL:6>DL1ABC<QSO_DATE:8>20240101<TIME_ON:4>1200<BAND:3>20m<MODE:2>CW<EOR>\
             <CALL:6>DK2ABC<QSO_DATE:8>20240102<TIME_ON:4>1200<BAND:3>40m<MODE:3>SSB<DXCC:3>230<EOR>\
             <CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:4>1234<BAND:3>17m<MODE:4>RTTY<DXCC:3>182<EOR>\
             <CALL:4>W1AW<QSO_DATE:8>20240103<TIME_ON:4>1200<BAND:3>15m<MODE:3>FT8<EOR>",
        ),
    );
    let typed = |call: &str| card_of(&engine, call);
    assert_eq!(
        [
            typed("DL9XYZ"),
            typed("KP1XYZ"),
            typed("K1ABC"),
            typed("XE1ABC")
        ],
        [
            worked("Fed. Rep. of Germany", &["20M", "40M"], &["CW", "SSB"]),
            worked("Navassa Island", &["17M"], &["RTTY"]),
            worked("United States", &["15M"], &["FT8"]),
            (
                Some("Mexico".to_string()),
                json!(true),
                json!([]),
                json!([])
            ),
        ]
    );
}

/// The awards' chase and worked count, the Needed board's worked entities, and the statistics'
/// entity count and entities — what a contact counts toward, as the screens read it from the store.
type Counts = (Vec<String>, usize, Vec<String>, Value, Value);

fn counts(engine: &SharedEngine) -> Counts {
    let tallies = crate::LogTallies::default();
    let awards = crate::log_folds::awards_kept(engine, &tallies).expect("awards");
    let needs = crate::log_folds::needs_kept(engine, &tallies).expect("needs");
    let q: LogQuestion = serde_json::from_value(json!({"kind": "statistics"})).expect("asked");
    let stats = LogQueries::default()
        .answer(engine, &q, &shown, &on_card, &counted)
        .expect("answered");
    let mut worked: Vec<String> = needs.worked_entity_names().iter().cloned().collect();
    worked.sort();
    (
        awards
            .needed
            .iter()
            .map(|n| n.entity.clone())
            .collect::<Vec<_>>(),
        awards.dxcc_worked,
        worked,
        stats["dxccEntities"].clone(),
        stats["entities"].clone(),
    )
}

/// ★ The awards, the Needed board and the statistics count a contact by its DXCC number and never
/// by the COUNTRY beside it, unchanged now that COUNTRY follows the number. A K1N contact carrying
/// 182 counts as Navassa Island whether its COUNTRY says the United States (as an import that
/// contradicts its number leaves it) or Navassa (as the operator's change writes it), and so does
/// one set to 182 by hand, with the app's own resolvers.
#[test]
fn the_awards_the_needed_board_and_the_statistics_count_by_the_number_not_the_country() {
    let k1n = |fields: &str| {
        tempo_core::logbook::parse_adif(&format!(
            "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:4>1234<BAND:3>20m<MODE:2>CW{fields}<EOR>"
        ))
        .remove(0)
    };
    let navassa = (
        vec!["Navassa Island".to_string()],
        1,
        vec!["Navassa Island".to_string()],
        json!(1),
        json!([{"label": "Navassa Island", "count": 1}]),
    );
    for (tag, fields) in [
        ("contradicted", "<DXCC:3>182<COUNTRY:13>United States"),
        ("agreeing", "<DXCC:3>182<COUNTRY:14>Navassa Island"),
    ] {
        let d = Dir::new(&format!("counts-{tag}"));
        let engine = on_store(&d, &[k1n(fields)]);
        assert_eq!(counts(&engine), navassa, "{tag}");
    }

    // Set to 182 by hand on a station wired as the launch wires it, which writes COUNTRY beside
    // the number (`log_by_id`'s tests hold that by value).
    let d = Dir::new("counts-by-hand");
    write_log(&d, &[k1n("<COUNTRY:13>United States")]);
    let opened = tempo_app::logstore::open(
        &d.log(),
        Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
        None,
    )
    .expect("the store opens");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.attach_log_store(opened);
    crate::wire_country_resolvers(&mut e);
    let engine: SharedEngine = Arc::new(Mutex::new(e));
    let row = QsoRecord::clone(&engine.lock().unwrap().stored_log()[0]);
    let key = QsoEdit::project(&row).key();
    let (made, _) = tempo_app::logwrite::until_written(|| {
        tempo_app::logwrite::set_dxcc(&engine, row.id.expect("an id"), &key, Some(182))
    });
    let (_, after) = made.expect("read").expect("made");
    assert_eq!(after.expect("the contact stays").dxcc, Some(182), "premise");
    assert_eq!(counts(&engine), navassa, "set by hand");
}

/// ★ A WAE area's contact counts as it always has, whatever COUNTRY now says beside its number.
/// A Sicily contact carrying Italy's 248 — its COUNTRY "Sicily" as an import left it, or "Italy" as
/// the operator's change now writes it — and one carrying no number count alike. The awards, the
/// Needed board and the statistics count Italy (a WAE area counts as its parent), and the Journey
/// is the same model for all three: it places a contact by its call alone, Sicily and all.
#[test]
fn a_wae_areas_contact_counts_the_same_whatever_country_it_carries() {
    // 2026-10-15 12:00 UTC: the Journey's year, and an hour after the contact.
    const NOW: i64 = 1_792_065_600;
    let with = |fields: &str| {
        let it9 = tempo_core::logbook::parse_adif(&format!(
            "<CALL:6>IT9ABC<QSO_DATE:8>20261015<TIME_ON:4>1100<BAND:3>20m<MODE:2>CW{fields}<EOR>"
        ))
        .remove(0);
        let d = Dir::new("wae-counts");
        let engine = on_store(&d, &[it9]);
        let journey = crate::log_folds::journey_kept(&engine, &crate::LogTallies::default())
            .expect("the Journey");
        let summary = serde_json::to_value(journey.summary(NOW)).expect("a summary");
        (counts(&engine), summary)
    };
    let (as_imported, journey) = with("<DXCC:3>248<COUNTRY:6>Sicily");
    assert_eq!(
        as_imported,
        (
            vec!["Italy".to_string()],
            1,
            vec!["Italy".to_string()],
            json!(1),
            json!([{"label": "Italy", "count": 1}]),
        )
    );
    assert_eq!(
        with("<DXCC:3>248<COUNTRY:5>Italy"),
        (as_imported.clone(), journey.clone()),
        "as the operator's change writes it"
    );
    assert_eq!(
        with("<COUNTRY:6>Sicily"),
        (as_imported, journey),
        "no number"
    );
}

/// ★ `orderRev` IS THE INDEX REVISION. An upload stamp moves the rows' content and not their
/// order: the page's `contentRev` and `revision` move, its `orderRev` does not, the order is not
/// built again, and the stamp is in the rows. An edit moves the order: a new `orderRev`, a new
/// build. And the snapshot's `logTick` — the UI's change feed — moves on both.
#[test]
fn order_rev_survives_a_stamp_and_moves_on_an_edit() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("revs");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let page = |queries: &LogQueries| {
        ask(
            queries,
            &engine,
            json!({"kind": "page", "offset": 0, "limit": 24,
                "query": {"sort": "call", "asc": true, "search": "", "needsConfirmOnly": false}}),
            &resolve,
        )
    };
    let built = |queries: &LogQueries| queries.0.built.load(Ordering::Relaxed);
    let first = page(&queries);
    assert_eq!(built(&queries), 1);
    assert_eq!(page(&queries)["orderRev"], first["orderRev"]);
    assert_eq!(built(&queries), 1, "asked again: kept");

    let tick = snapshot_tick(&engine);
    let target = golden_id("fx05");
    let stamped = {
        let row = logged(&engine, target);
        tempo_app::logwrite::stamp_push(&engine, &row, UploadService::Qrz, accepted(1_790_000_000))
            .0
    };
    assert!(stamped, "premise: the stamp landed");
    assert_ne!(snapshot_tick(&engine), tick, "the change feed moved");
    // Asked once the store holds the stamp: straight after it, a question on a slow disk waits its
    // two seconds and is refused (P4), and a refused build would count here as one.
    engine.caught_up();
    let after_stamp = page(&queries);
    assert_eq!(
        after_stamp["orderRev"], first["orderRev"],
        "a stamp moves no order"
    );
    assert_ne!(after_stamp["contentRev"], first["contentRev"]);
    assert_ne!(after_stamp["revision"], first["revision"]);
    assert_eq!(built(&queries), 1, "the order kept through the stamp");
    let row = after_stamp["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == json!(target.to_string()))
        .expect("the stamped row is on the page");
    assert_eq!(
        row["upload"]["qrz"]["outcome"], "accepted",
        "the rows are read again"
    );

    let tick = snapshot_tick(&engine);
    {
        let mut fixed = logged(&engine, target);
        fixed.call = "AA1AAA".into();
        let edit = LogOp::Edit {
            id: target,
            rec: Box::new(fixed),
        };
        assert!(change(&engine, target, &[edit]));
    }
    assert_ne!(snapshot_tick(&engine), tick, "the change feed moved");
    engine.caught_up();
    let after_edit = page(&queries);
    assert_ne!(
        after_edit["orderRev"], first["orderRev"],
        "an edit moves the order"
    );
    assert_eq!(built(&queries), 2);
    assert_eq!(
        after_edit["keys"][0],
        json!(target.to_string()),
        "the corrected call sorts first now"
    );
}

/// The orders kept are the last four asked; a fifth order pushes out the one asked longest ago,
/// and asking for it again builds it again.
#[test]
fn four_orders_are_kept_and_the_oldest_goes() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("lru");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let page = |sort: &str| {
        ask(
            &queries,
            &engine,
            json!({"kind": "page", "offset": 0, "limit": 1,
                "query": {"sort": sort, "asc": true, "search": "", "needsConfirmOnly": false}}),
            &resolve,
        );
    };
    let built = || queries.0.built.load(Ordering::Relaxed);
    for sort in ["call", "country", "mode", "sent"] {
        page(sort);
    }
    assert_eq!(built(), 4);
    page("call");
    assert_eq!(built(), 4, "one of the four");
    page("rcvd");
    assert_eq!(built(), 5, "a fifth");
    page("country");
    assert_eq!(
        built(),
        6,
        "the oldest (country, since call was asked again) went"
    );
    page("call");
    assert_eq!(built(), 6, "call is still kept");
}

/// ★ A FOLD IS KEPT UNTIL WHAT IT READS MOVES. The squares, the bands, the globe's dots and the
/// statistics read nothing an upload stamp changes: a stamp leaves them kept. The LoTW backlog
/// reads the LoTW stamp: a stamp folds it again, and the answer has the stamp. A logged contact
/// folds every one of them again. Each answer is the reference's, every time.
#[test]
fn a_fold_is_kept_until_what_it_reads_moves() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("folds-kept");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let folded = || queries.0.folded.load(Ordering::Relaxed);
    let questions = [
        json!({"kind": "workedGrids"}),
        json!({"kind": "bandsInLog"}),
        json!({"kind": "gridPoints", "band": "20m"}),
        json!({"kind": "statistics"}),
        json!({"kind": "lotwBacklog"}),
    ];
    let expected = |log: &[QsoRecord]| -> Vec<Value> {
        let of = |call: &str| entities.get(call).cloned().flatten();
        vec![
            json!(reference::fold(log, WorkedGrids::default())),
            json!(reference::fold(log, BandsInLog::default())),
            json!(reference::fold(log, GridCounts::new("20m"))),
            json!(reference::fold(
                log,
                LogStatCounter::new(&|r: &QsoRecord| of(&r.call))
            )),
            json!(reference::fold(log, LotwBacklog::default())),
        ]
    };
    let asked = || -> Vec<Value> {
        questions
            .iter()
            .map(|q| ask(&queries, &engine, q.clone(), &resolve))
            .collect()
    };
    assert_eq!(asked(), expected(&log_of(&engine)));
    assert_eq!(folded(), 5);
    assert_eq!(asked(), expected(&log_of(&engine)));
    assert_eq!(folded(), 5, "asked again: all kept");

    // A LoTW upload stamp on an unsent contact: only the backlog reads it.
    let unsent = log_of(&engine)
        .into_iter()
        .find(|r| !r.award_confirmed && r.upload.lotw.is_none() && r.time_known)
        .expect("premise: an unsent contact");
    let signed = tempo_app::station::LotwSigned::of(&unsent).expect("an id");
    let (done, _) =
        tempo_app::logwrite::stamp_lotw_batch(&engine, &[signed], &accepted(1_790_000_000));
    assert_eq!(done.stamped, 1, "premise: the stamp landed");
    // Asked once the store holds the stamp: straight after it, a question on a slow disk waits its
    // two seconds and is refused (P4), and a refused fold would count here as one.
    engine.caught_up();
    let after_stamp = asked();
    assert_eq!(after_stamp, expected(&log_of(&engine)));
    assert_eq!(folded(), 6, "the backlog alone folded again");
    assert_ne!(
        after_stamp[4],
        json!(reference::fold(&records, LotwBacklog::default()))
    );

    // A logged contact moves them all.
    {
        let mut r = logged(&engine, golden_id("fx01"));
        r.id = None;
        r.call = "N0FOLD".into();
        r.grid = Some("AA00".into());
        r.when_unix = 1_800_000_000;
        engine_lock(&engine).log_qso(r);
    }
    engine.caught_up();
    assert_eq!(asked(), expected(&log_of(&engine)));
    assert_eq!(folded(), 11, "every fold folded again");
}

/// A new order asked for by several windows at once is built once: the others wait for that
/// build instead of passing over the log themselves.
#[test]
fn an_order_asked_for_at_once_is_built_once() {
    let (records, entities) = golden_log();
    let d = Dir::new("single-flight");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let answers: Vec<Value> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..6)
            .map(|i| {
                let (queries, engine, entities) = (&queries, &engine, &entities);
                s.spawn(move || {
                    let resolve = |call: &str| entities.get(call).cloned().flatten();
                    ask(
                        queries,
                        engine,
                        json!({"kind": "page", "offset": i * 4, "limit": 4,
                            "query": {"sort": "park", "asc": false, "search": "", "needsConfirmOnly": false}}),
                        &resolve,
                    )
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(
        queries.0.built.load(Ordering::Relaxed),
        1,
        "one build for six asks"
    );
    assert!(answers.iter().all(|a| a["total"] == 24));
}

// ── gone, and read-your-writes ─────────────────────────────────────────────────────────────────

/// A deleted contact is gone from every answer: its row is null, it has no place, and the pages
/// and the size no longer count it.
#[test]
fn a_deleted_contact_is_gone_from_every_answer() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("gone");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let gone = golden_id("fx07");
    let default = json!({"sort": "time", "asc": false, "search": "", "needsConfirmOnly": false});
    let located = |queries: &LogQueries| {
        ask(
            queries,
            &engine,
            json!({"kind": "locate", "query": default, "id": gone.to_string()}),
            &resolve,
        )["index"]
            .clone()
    };
    assert!(located(&queries).is_number(), "premise: it has a place");
    assert!(change(&engine, gone, &[LogOp::Delete(gone)]));
    // Asked once the store holds the delete: straight after it, a question on a slow disk waits
    // its two seconds and is refused (P4), as the read-your-writes tests show.
    engine.caught_up();
    assert_eq!(located(&queries), Value::Null);
    let row = ask(
        &queries,
        &engine,
        json!({"kind": "row", "id": gone.to_string()}),
        &resolve,
    );
    assert_eq!(row, Value::Null);
    let page = ask(
        &queries,
        &engine,
        json!({"kind": "page", "query": default, "offset": 0, "limit": 100}),
        &resolve,
    );
    assert_eq!(
        (page["total"].clone(), page["logSize"].clone()),
        (json!(23), json!(23))
    );
    assert!(!page["keys"]
        .as_array()
        .unwrap()
        .contains(&json!(gone.to_string())));
}

/// ★ READ YOUR WRITES (P4). A contact logged a moment ago is in the very next answer, though the
/// store takes it on the writer's own thread: the read waits for it. On a disk slower than that
/// wait the read is refused and asked again, as the window asks again — never answered without
/// the contact.
#[test]
fn a_contact_just_logged_is_in_the_next_answer() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("ryw");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let default = json!({"sort": "time", "asc": false, "search": "", "needsConfirmOnly": false});
    for n in 0..5 {
        {
            let mut r = logged(&engine, golden_id("fx01"));
            r.id = None;
            r.call = format!("N{n}NEW");
            r.when_unix = 1_800_000_000 + n;
            engine_lock(&engine).log_qso(r);
        }
        let page = answered(
            &queries,
            &engine,
            json!({"kind": "page", "query": default, "offset": 0, "limit": 1}),
            &resolve,
        );
        assert_eq!(
            page["rows"][0]["call"],
            json!(format!("N{n}NEW")),
            "the newest, at once"
        );
        let history = answered(
            &queries,
            &engine,
            json!({"kind": "callHistory", "call": format!("n{n}new"), "band": "", "mode": "", "matchMode": false}),
            &resolve,
        );
        assert_eq!(history["count"], 1);
    }
}

/// ★ THE ENGINE LOCK IS FREE WHILE AN ANSWER WAITS. With the store's writer stalled behind
/// another program's write lock, a question asked after a contact is logged waits for the writer
/// — and all that time the radio loop's lock can be taken. The wait runs out and the question is
/// refused, never answered without the contact, and nothing it read is kept: asked again at the
/// same revisions once the writer has caught up, it shows the contact.
#[test]
fn the_engine_lock_is_free_while_an_answer_waits_for_the_writer() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("stall");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let question = json!({"kind": "page", "offset": 0, "limit": 2,
        "query": {"sort": "mode", "asc": true, "search": "", "needsConfirmOnly": false}});
    let db = tempo_core::logbook::migrate::database_path(&d.log());
    let hold = tempo_core::logbook::sqlite::WriteHold::take(&db).expect("stall the store");
    {
        let mut r = logged(&engine, golden_id("fx02"));
        r.id = None;
        r.call = "N9STALL".into();
        r.when_unix += 60;
        engine_lock(&engine).log_qso(r);
    }
    let asked_at = engine_lock(&engine).log_index_rev();
    let started = std::time::Instant::now();
    let answer = std::thread::scope(|s| {
        let asking = s.spawn(|| try_ask(&queries, &engine, question.clone(), &resolve));
        let mut free = 0;
        while !asking.is_finished() {
            if engine_try_lock(&engine).is_ok() {
                free += 1;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            free >= 10,
            "the Engine lock was free while the answer waited ({free})"
        );
        asking.join().unwrap()
    });
    assert!(
        started.elapsed() >= READ_WAIT,
        "control: the answer did wait for the stalled writer"
    );
    let why = answer.expect_err("refused, never answered without the contact");
    assert!(why.starts_with(NOT_ANSWERED), "{why}");
    drop(hold);
    assert!(
        lock(&queries.0.orders).is_empty(),
        "an order read before the writer caught up is not kept"
    );
    // Asked once the writer has caught up: letting the hold go only lets it start, and on a loaded
    // machine its commit can outlast the next answer's own wait.
    engine.caught_up();
    let caught_up = ask(&queries, &engine, question, &resolve);
    assert_eq!(
        caught_up["orderRev"],
        json!(asked_at),
        "premise: the same revisions"
    );
    assert_eq!(
        caught_up["total"], 25,
        "the contact logged before the question is in its answer now"
    );
}

/// A fold read before the writer caught up is not kept either: the wait runs out, the fold is
/// refused, and the next question, once the writer has caught up, folds again and has the contact.
#[test]
fn a_fold_read_before_the_writer_caught_up_is_not_kept() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
    let d = Dir::new("stall-fold");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let db = tempo_core::logbook::migrate::database_path(&d.log());
    let hold = tempo_core::logbook::sqlite::WriteHold::take(&db).expect("stall the store");
    {
        let mut r = logged(&engine, golden_id("fx02"));
        r.id = None;
        r.call = "N9STALL".into();
        r.grid = Some("AA00".into());
        r.when_unix += 60;
        engine_lock(&engine).log_qso(r);
    }
    let grids = || try_ask(&queries, &engine, json!({"kind": "workedGrids"}), &resolve);
    let why = grids().expect_err("refused, never answered without the contact");
    assert!(why.starts_with(NOT_ANSWERED), "{why}");
    drop(hold);
    // Asked once the writer has caught up, as above.
    engine.caught_up();
    let caught_up = grids().expect("answered once the store holds the contact");
    assert!(
        caught_up.as_array().unwrap().contains(&json!("AA00")),
        "the contact logged before the question is in its answer now"
    );
    assert_eq!(queries.0.folded.load(Ordering::Relaxed), 2, "folded again");
}

/// ★ P4 WHEN THE WRITER IS BEHIND: every question that reads the store, asked while changes made
/// before it are still on their way to disk — held there past the whole of the read's wait — is
/// REFUSED ([`NOT_ANSWERED`]), never answered without them, and keeps nothing; asked again once
/// the store holds them, each answers with them. What a slow disk did before: the Logbook asked
/// straight after a contact answered from the store as it stood, and left the contact out until
/// the next change. The control: the band map's worked calls read the hot index under the Engine
/// lock, not the store, and have the contact at once.
#[test]
fn a_question_whose_wait_runs_out_is_refused_never_answered_without_the_change() {
    let (records, entities) = golden_log();
    // ZD7AA is St Helena's, which no contact of the golden log is.
    let resolve = |call: &str| match call {
        "ZD7AA" => Some("St Helena".to_string()),
        _ => entities.get(call).cloned().flatten(),
    };
    let d = Dir::new("behind");
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let w1aw = golden_id("fx01");
    let default = json!({"sort": "time", "asc": false, "search": "", "needsConfirmOnly": false});
    let by_mode = json!({"sort": "mode", "asc": true, "search": "", "needsConfirmOnly": false});
    // Every question that reads the store, and what each answered, said by value.
    type Say = fn(&Value) -> String;
    let questions: [(&str, Value, Say); 14] = [
        (
            "the newest page",
            json!({"kind": "page", "query": default, "offset": 0, "limit": 1}),
            |a| format!("{} contacts, newest {}", a["total"], a["rows"][0]["call"]),
        ),
        (
            "a page by mode",
            json!({"kind": "page", "query": by_mode, "offset": 0, "limit": 1}),
            |a| format!("{} contacts", a["total"]),
        ),
        (
            "where W1AW sits",
            json!({"kind": "locate", "query": default, "id": w1aw.to_string()}),
            |a| format!("at {}", a["index"]),
        ),
        (
            "W1AW's row",
            json!({"kind": "row", "id": w1aw.to_string()}),
            |a| format!("comment {}", a["comment"]),
        ),
        (
            "the 25th contact",
            json!({"kind": "rowsAt", "indices": [24]}),
            |a| format!("{}", a[0]["call"]),
        ),
        ("the log's size", json!({"kind": "logSize"}), |a| {
            format!("{a} contacts")
        }),
        (
            "ZD7AA's history",
            json!({"kind": "callHistory", "call": "zd7aa", "band": "", "mode": "", "matchMode": false}),
            |a| format!("{} contacts", a["count"]),
        ),
        (
            "ZD7AA in a roster",
            json!({"kind": "callsSummary", "calls": ["ZD7AA"]}),
            |a| format!("{} contacts", a["ZD7AA"]["count"]),
        ),
        (
            "St Helena's slots",
            json!({"kind": "entity", "entity": "St Helena"}),
            |a| format!("new entity {}", a["newEntity"]),
        ),
        ("the squares worked", json!({"kind": "workedGrids"}), |a| {
            format!("AA00 {}", a.as_array().unwrap().contains(&json!("AA00")))
        }),
        (
            "the globe's 160m squares",
            json!({"kind": "gridPoints", "band": "160m"}),
            |a| format!("{} squares", a.as_array().unwrap().len()),
        ),
        ("the bands in the log", json!({"kind": "bandsInLog"}), |a| {
            format!("160m {}", a.as_array().unwrap().contains(&json!("160m")))
        }),
        ("the statistics", json!({"kind": "statistics"}), |a| {
            format!("{} contacts", a["total"])
        }),
        ("the LoTW backlog", json!({"kind": "lotwBacklog"}), |a| {
            format!("{} unsent", a["unsent"])
        }),
    ];
    let say =
        |q: &Value, show: Say| try_ask(&queries, &engine, q.clone(), &resolve).map(|a| show(&a));
    let w1aw_at = ask(&queries, &engine, questions[2].1.clone(), &resolve)["index"]
        .as_u64()
        .expect("premise: W1AW is in the newest-first order");
    let unsent = ask(&queries, &engine, questions[13].1.clone(), &resolve)["unsent"]
        .as_u64()
        .expect("the LoTW backlog's count");

    let db = tempo_core::logbook::migrate::database_path(&d.log());
    let hold = tempo_core::logbook::sqlite::WriteHold::take(&db).expect("stall the store");
    // Two changes on their way to disk: an edit, made as a command makes it, and a contact logged —
    // newest of all, on a band and in a square the log has no other contact on, from an entity it
    // has never worked.
    {
        let mut edited = logged(&engine, w1aw);
        edited.comment = Some("EDITED".into());
        let mut zd7 = logged(&engine, golden_id("fx02"));
        zd7.id = None;
        zd7.call = "ZD7AA".into();
        zd7.band = "160m".into();
        zd7.freq_mhz = 1.840;
        zd7.grid = Some("AA00".into());
        zd7.when_unix = 1_800_000_000;
        zd7.time_known = true;
        zd7.upload = Default::default();
        assert_eq!(
            lotw_unsent(std::slice::from_ref(&zd7)),
            1,
            "premise: the LoTW backlog counts the new contact"
        );
        let edit = LogOp::Edit {
            id: w1aw,
            rec: Box::new(edited),
        };
        assert!(change(&engine, w1aw, &[edit]), "premise: the edit is made");
        engine_lock(&engine).log_qso(zd7);
    }
    let asked_at = engine_lock(&engine).log_index_rev();
    // Every question at once, each saying what it answered: they all wait on the one held write.
    let asked: Vec<(&str, Result<String, String>)> = std::thread::scope(|s| {
        let asking: Vec<_> = questions
            .iter()
            .map(|(name, q, show)| (*name, s.spawn(move || say(q, *show))))
            .collect();
        asking
            .into_iter()
            .map(|(name, asked)| (name, asked.join().unwrap()))
            .collect()
    });
    let worked = ask(
        &queries,
        &engine,
        json!({"kind": "workedCalls", "calls": ["ZD7AA"]}),
        &resolve,
    );
    drop(hold);
    assert_eq!(
        worked,
        json!(["ZD7AA"]),
        "control: the worked calls read no store, and have the contact at once"
    );
    let answered: Vec<String> = asked
        .iter()
        .filter_map(|(name, a)| a.as_ref().ok().map(|what| format!("{name} ({what})")))
        .collect();
    assert!(
        answered.is_empty(),
        "answered without the changes: {}",
        answered.join(", ")
    );
    for (name, answer) in &asked {
        let why = answer.as_ref().unwrap_err();
        assert!(why.starts_with(NOT_ANSWERED), "{name}: {why}");
    }
    assert!(
        lock(&queries.0.orders)
            .iter()
            .all(|(k, _)| k.index_rev != asked_at),
        "no order read before the writer caught up is kept"
    );
    assert!(
        lock(&queries.0.log_order)
            .as_ref()
            .is_none_or(|(at, _)| *at != asked_at),
        "nor the log's order"
    );
    assert!(
        lock(&queries.0.entities)
            .as_ref()
            .is_none_or(|k| k.index_rev != asked_at),
        "nor the entities' slots"
    );

    engine.caught_up();
    let again: Vec<(&str, String)> = questions
        .iter()
        .map(|(name, q, show)| {
            (
                *name,
                say(q, *show).expect("answered once the store holds the changes"),
            )
        })
        .collect();
    let newer = |n: u64| format!("at {}", n + 1);
    assert_eq!(
        again,
        [
            (
                "the newest page",
                r#"25 contacts, newest "ZD7AA""#.to_string()
            ),
            ("a page by mode", "25 contacts".to_string()),
            ("where W1AW sits", newer(w1aw_at)),
            ("W1AW's row", r#"comment "EDITED""#.to_string()),
            ("the 25th contact", r#""ZD7AA""#.to_string()),
            ("the log's size", "25 contacts".to_string()),
            ("ZD7AA's history", "1 contacts".to_string()),
            ("ZD7AA in a roster", "1 contacts".to_string()),
            ("St Helena's slots", "new entity false".to_string()),
            ("the squares worked", "AA00 true".to_string()),
            ("the globe's 160m squares", "1 squares".to_string()),
            ("the bands in the log", "160m true".to_string()),
            ("the statistics", "25 contacts".to_string()),
            ("the LoTW backlog", format!("{} unsent", unsent + 1)),
        ],
        "asked again once the store holds them, every answer has both changes"
    );
}

/// How many of `records` the Logbook's "Upload to LoTW" counts as unsent ([`LotwBacklog`]'s own
/// fold).
fn lotw_unsent(records: &[QsoRecord]) -> usize {
    let mut backlog = LotwBacklog::default();
    for r in records {
        backlog.add(r);
    }
    backlog.finish().unsent
}

// ── calls the store and the UI fold differently ────────────────────────────────────────────────

/// A log whose calls the store's ASCII fold and the UI's Unicode fold disagree on — `ſ`, `ß`, a
/// no-break space, a trailing blank — asked about the ways the UI asks.
fn odd_log() -> Vec<QsoRecord> {
    let (golden, _) = golden_log();
    let base = &golden[0];
    [
        "dſ1x",
        "DS1X",
        "ß1AA",
        "ss1aa",
        "W1AW\u{A0}",
        "k1abc ",
        " K1ABC",
        "ǆ1Z",
        "\u{FEFF}N0X",
        "N0X",
        "ı5",
        "I5",
    ]
    .iter()
    .enumerate()
    .map(|(i, call)| {
        let mut r = base.clone();
        r.id = Some(RecordId::Provisional {
            hash: 900 + i as u64,
            ordinal: 0,
        });
        r.call = call.to_string();
        r.when_unix = 1_700_000_000 + i as u64 * 60;
        r.name = Some(format!("op{i}"));
        r
    })
    .collect()
}

/// ★ Every call question, on a log of calls the two folds disagree on, is the reference's answer:
/// worked calls from the hot index and the odd calls it names, histories and summaries from the
/// call index and those calls' own rows.
#[test]
fn calls_the_folds_disagree_on_are_answered_as_the_ui_answers() {
    let log = odd_log();
    let resolve = |_: &str| None;
    let asked: Vec<String> = [
        "DS1X",
        "dſ1x",
        "SS1AA",
        "ß1aa",
        "W1AW\u{A0}",
        "w1aw",
        "K1ABC ",
        "k1abc",
        "K1ABC",
        "Ǆ1Z",
        "ǆ1z",
        "N0X",
        "\u{FEFF}n0x",
        "I5",
        "ı5",
        "nobody",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (home, d) in [
        ("store", Dir::new("odd-store")),
        ("1.13 path", Dir::new("odd-mem")),
    ] {
        let engine = if home == "store" {
            on_store(&d, &log)
        } else {
            on_log_file(&d, &log)
        };
        // The reference's log: the store's rows, as `log_of` reads them.
        let held = engine.lock().unwrap().stored_records();
        let queries = LogQueries::default();
        let worked = ask(
            &queries,
            &engine,
            json!({"kind": "workedCalls", "calls": asked}),
            &resolve,
        );
        assert_eq!(
            worked,
            json!(reference::worked_calls(&held, &asked)),
            "{home}"
        );
        for call in &asked {
            let h = ask(
                &queries,
                &engine,
                json!({"kind": "callHistory", "call": call, "band": "20m", "mode": "FT8", "matchMode": false}),
                &resolve,
            );
            let expected = reference::call_history(&held, call, "20m", "FT8", false);
            let ids: Vec<Value> = expected
                .qsos
                .iter()
                .map(|&i| json!(held[i].id.unwrap().to_string()))
                .collect();
            let got: Vec<Value> = h["qsos"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].clone())
                .collect();
            assert_eq!(
                (got, h["count"].clone()),
                (ids, json!(expected.count)),
                "{home}: {call:?}"
            );
        }
        let summary = ask(
            &queries,
            &engine,
            json!({"kind": "callsSummary", "calls": asked}),
            &resolve,
        );
        let expected: serde_json::Map<String, Value> = query::calls_summary(&held, &asked)
            .into_iter()
            .map(|(c, s)| (c, serde_json::to_value(s).unwrap()))
            .collect();
        assert_eq!(summary, Value::Object(expected), "{home}");
    }
}

// ── the randomized parity ───────────────────────────────────────────────────────────────────────

/// A small deterministic generator (xorshift): the seed is printed with every failure.
struct Gen(u64);
impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

const CALLS: [&str; 10] = [
    "W1AW", "w1aw", "K1ABC", "K1ABC ", "dſ1x", "DS1X", "JA1XYZ", "VE3ABC", "ß1AA", "N0CALL",
];
const BANDS: [&str; 7] = ["20m", "40m", "20M", "", "2m", "junk", "15m"];
const MODES: [&str; 7] = ["FT8", "ft8", "USB", "LSB", "CW", "SSB", ""];
const COUNTRIES: [&str; 5] = ["United States", "Canada", "Japan", "ſtraße", ""];

/// The entity the parity test's resolver gives a call.
fn test_entity(call: &str) -> Option<String> {
    match call.trim().to_uppercase().chars().next() {
        Some('W') | Some('K') | Some('N') => Some("United States".into()),
        Some('J') => Some("Japan".into()),
        Some('V') => Some("Canada".into()),
        _ => None,
    }
}

fn random_record(g: &mut Gen, n: u64) -> QsoRecord {
    let (golden, _) = golden_log();
    let mut r = golden[g.below(golden.len())].clone();
    r.id = None;
    r.call = g.pick(&CALLS).to_string();
    r.band = g.pick(&BANDS).to_string();
    r.mode = g.pick(&MODES).to_string();
    r.freq_mhz = [0.0, 14.074, 7.074, 144.2, 21.3][g.below(5)];
    let country = g.pick(&COUNTRIES);
    r.country = (!country.is_empty()).then(|| country.to_string());
    // Twins in time, and the same minute under different keys.
    r.when_unix = 1_700_000_000 + (n % 7) * 60 + (g.below(3) as u64) * 3_600;
    r.qsl_rcvd.card = g.below(4) == 0;
    r.qsl_rcvd.lotw = g.below(4) == 0;
    r.confirmed = r.qsl_rcvd.any();
    r.award_confirmed = r.qsl_rcvd.award();
    r.name = Some(format!("n{n}"));
    r
}

/// The log the reference answers are computed from: the store's rows, in log order
/// ([`StoredLog`]), in place of the copy in memory every window was handed before C17. These tests
/// hold the reference's answers against the engine's over the same rows; whether the store holds
/// what the old write path wrote (P6) is the Stage-1 lockstep suite's job.
fn log_of(engine: &SharedEngine) -> Vec<QsoRecord> {
    engine.lock().unwrap().stored_records()
}

/// Every question this part answers, asked of the engine and of the reference over the log the
/// engine answers from ([`log_of`]).
fn assert_parity(seed: u64, step: &str, engine: &SharedEngine, queries: &LogQueries, g: &mut Gen) {
    let held = log_of(engine);
    let at = |what: &str| format!("seed {seed}, after {step}: {what}");
    let resolve = |call: &str| test_entity(call);
    let id_of = |i: usize| json!(held[i].id.unwrap().to_string());
    let sorts = [
        "call", "country", "band", "freq", "mode", "sent", "rcvd", "time", "park", "qsl",
    ];
    let searches = ["", "w1", "ssb", "usb", "z", "ſ", "20m", "2023-11"];
    for _ in 0..3 {
        let query = json!({
            "sort": g.pick(&sorts), "asc": g.below(2) == 0,
            "search": g.pick(&searches), "needsConfirmOnly": g.below(3) == 0,
        });
        let parsed: LogQuery = serde_json::from_value(query.clone()).unwrap();
        let order = reference::order(&held, &parsed, &|r: &QsoRecord| resolve(&r.call));
        let (offset, limit) = (g.below(held.len() + 2), 1 + g.below(8));
        let page = ask(
            queries,
            engine,
            json!({"kind": "page", "query": query, "offset": offset, "limit": limit}),
            &resolve,
        );
        let keys: Vec<Value> = order
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&i| id_of(i))
            .collect();
        assert_eq!(
            page["keys"],
            json!(keys),
            "{}",
            at(&format!("page {query}"))
        );
        assert_eq!(page["total"], json!(order.len()), "{}", at("total"));
        assert_eq!(page["logSize"], json!(held.len()), "{}", at("logSize"));
        if !held.is_empty() {
            let i = g.below(held.len());
            let located = ask(
                queries,
                engine,
                json!({"kind": "locate", "query": query, "id": held[i].id.unwrap().to_string()}),
                &resolve,
            );
            assert_eq!(
                located["index"],
                json!(order.iter().position(|&p| p == i)),
                "{}",
                at("locate")
            );
        }
    }
    let calls: Vec<String> = (0..4).map(|_| g.pick(&CALLS).to_string()).collect();
    for call in &calls {
        let (band, mode, match_mode) = (g.pick(&BANDS), g.pick(&MODES), g.below(2) == 0);
        let h = ask(
            queries,
            engine,
            json!({"kind": "callHistory", "call": call, "band": band, "mode": mode, "matchMode": match_mode}),
            &resolve,
        );
        let e = reference::call_history(&held, call, band, mode, match_mode);
        let expected = json!({
            "qsos": e.qsos.iter().map(|&i| id_of(i)).collect::<Vec<_>>(),
            "count": e.count, "workedBefore": e.worked_before, "dupeThisBand": e.dupe_this_band,
            "lastUnix": e.last_unix, "confirmedCount": e.confirmed_count,
            "bands": e.bands, "modes": e.modes,
        });
        let mut got = h;
        got["qsos"] = json!(got["qsos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].clone())
            .collect::<Vec<_>>());
        assert_eq!(got, expected, "{}", at(&format!("callHistory {call:?}")));
    }
    let worked = ask(
        queries,
        engine,
        json!({"kind": "workedCalls", "calls": calls}),
        &resolve,
    );
    assert_eq!(
        worked,
        json!(reference::worked_calls(&held, &calls)),
        "{}",
        at("workedCalls")
    );
    let summary = ask(
        queries,
        engine,
        json!({"kind": "callsSummary", "calls": calls}),
        &resolve,
    );
    let expected: serde_json::Map<String, Value> = query::calls_summary(&held, &calls)
        .into_iter()
        .map(|(c, s)| (c, serde_json::to_value(s).unwrap()))
        .collect();
    assert_eq!(summary, Value::Object(expected), "{}", at("callsSummary"));
    for entity in [
        "United States",
        "japan",
        "STRASSE",
        "Canada ",
        "Nowhere",
        "",
    ] {
        let got = ask(
            queries,
            engine,
            json!({"kind": "entity", "entity": entity}),
            &resolve,
        );
        let of = |r: &QsoRecord| test_entity(&r.call);
        let expected = serde_json::to_value(reference::entity(&held, entity, &of)).unwrap();
        assert_eq!(got, expected, "{}", at(&format!("entity {entity:?}")));
    }
    let of = |call: &str| test_entity(call);
    let folds = [
        (
            json!({"kind": "workedGrids"}),
            json!(reference::fold(&held, WorkedGrids::default())),
        ),
        (
            json!({"kind": "bandsInLog"}),
            json!(reference::fold(&held, BandsInLog::default())),
        ),
        (
            json!({"kind": "lotwBacklog"}),
            json!(reference::fold(&held, LotwBacklog::default())),
        ),
        (
            json!({"kind": "statistics"}),
            json!(reference::fold(
                &held,
                LogStatCounter::new(&|r: &QsoRecord| of(&r.call))
            )),
        ),
    ];
    for (q, expected) in folds {
        assert_eq!(
            ask(queries, engine, q.clone(), &resolve),
            expected,
            "{}",
            at(&q.to_string())
        );
    }
    for band in ["all", g.pick(&BANDS)] {
        let got = ask(
            queries,
            engine,
            json!({"kind": "gridPoints", "band": band}),
            &resolve,
        );
        let expected = json!(reference::fold(&held, GridCounts::new(band)));
        assert_eq!(got, expected, "{}", at(&format!("gridPoints {band:?}")));
    }
    let indices = json!([0, held.len() / 2, held.len(), -1]);
    let rows = ask(
        queries,
        engine,
        json!({"kind": "rowsAt", "indices": indices}),
        &resolve,
    );
    let expected: Vec<Value> = [0, held.len() / 2, held.len()]
        .iter()
        .map(|&i| {
            held.get(i)
                .map_or(Value::Null, |r| json!(r.id.unwrap().to_string()))
        })
        .chain([Value::Null])
        .collect();
    let got: Vec<Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.get("id").cloned().unwrap_or(Value::Null))
        .collect();
    assert_eq!(got, expected, "{}", at("rowsAt"));
}

/// ★ PARITY: random logs, every kind of change the Logbook can make, and after each one every
/// question asked of the engine — on the store and on the 1.13 path — against the reference over
/// the engine's own log. And the UI's change feed: a change that moved the log moved `logTick`.
#[test]
fn every_answer_is_the_references_after_every_kind_of_change() {
    for seed in 1..=12u64 {
        for home in ["store", "1.13 path"] {
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let n = 6 + g.below(14);
            let mut log: Vec<QsoRecord> = (0..n).map(|i| random_record(&mut g, i as u64)).collect();
            for (i, r) in log.iter_mut().enumerate() {
                r.id = Some(RecordId::Provisional {
                    hash: 5_000 + i as u64,
                    ordinal: 0,
                });
            }
            let d = Dir::new("parity");
            let engine = if home == "store" {
                on_store(&d, &log)
            } else {
                on_log_file(&d, &log)
            };
            let queries = LogQueries::default();
            assert_parity(seed, &format!("{home} load"), &engine, &queries, &mut g);
            let mut before = (snapshot_tick(&engine), log_of(&engine));
            for step in 0..10 {
                let what = {
                    let held = log_of(&engine);
                    let pick = |g: &mut Gen| held[g.below(held.len())].id.unwrap();
                    // Each change as a command makes it: a change to a row the log holds is
                    // planned with the Engine lock released and made under it.
                    match g.below(7) {
                        0 => {
                            let r = random_record(&mut g, 100 + step);
                            engine_lock(&engine).log_qso(r);
                            "a logged contact"
                        }
                        1 if !held.is_empty() => {
                            let id = pick(&mut g);
                            let mut r = logged(&engine, id);
                            r.call = g.pick(&CALLS).to_string();
                            r.band = g.pick(&BANDS).to_string();
                            r.mode = g.pick(&MODES).to_string();
                            change(
                                &engine,
                                id,
                                &[LogOp::Edit {
                                    id,
                                    rec: Box::new(r),
                                }],
                            );
                            "an edit"
                        }
                        2 if held.len() > 1 => {
                            let id = pick(&mut g);
                            change(&engine, id, &[LogOp::Delete(id)]);
                            "a delete"
                        }
                        3 if !held.is_empty() => {
                            let r = logged(&engine, pick(&mut g));
                            tempo_app::logwrite::stamp_push(
                                &engine,
                                &r,
                                UploadService::Qrz,
                                accepted(1),
                            );
                            "an upload stamp"
                        }
                        4 if !held.is_empty() => {
                            let id = pick(&mut g);
                            change(&engine, id, &[LogOp::MarkQslCard { id, received: true }]);
                            "a paper card"
                        }
                        5 if !held.is_empty() => {
                            let id = pick(&mut g);
                            let stored = QsoEdit::project(&logged(&engine, id));
                            let mut edit = stored.clone();
                            edit.call = g.pick(&CALLS).to_string();
                            edit.mode = g.pick(&MODES).to_string();
                            edit.qsl_card = !edit.qsl_card;
                            tempo_app::logwrite::edit_row(&engine, id, &stored.key(), &edit)
                                .0
                                .expect("the edit is one the form can make")
                                .expect("the row is as it was read");
                            "an edit from the Logbook's form"
                        }
                        _ => {
                            let r = random_record(&mut g, 200 + step);
                            let text = adif_header() + &adif_record_own_log(&r);
                            tempo_app::logwrite::import_adif(&engine, &text)
                                .0
                                .expect("the import is made");
                            "an import"
                        }
                    }
                };
                assert_parity(seed, &format!("{home} {what}"), &engine, &queries, &mut g);
                let after = (snapshot_tick(&engine), log_of(&engine));
                if after.1 != before.1 {
                    assert_ne!(
                        after.0, before.0,
                        "seed {seed}, {home}: {what} changed the log and logTick did not move"
                    );
                }
                before = after;
            }
        }
    }
}

// ── the command ─────────────────────────────────────────────────────────────────────────────────

/// `ask_log` is an async command — it never waits for the engine on the UI thread — registered
/// in `generate_handler!`, with its state managed; and it is the one command here.
#[test]
fn ask_log_is_async_registered_and_its_state_managed() {
    let here = include_str!("../log_queries.rs");
    let lib = include_str!("../lib.rs");
    assert!(here.lines().any(|l| l.starts_with("pub async fn ask_log(")));
    assert_eq!(
        here.lines()
            .filter(|l| l.trim() == "#[tauri::command]")
            .count(),
        1
    );
    let list = lib
        .split_once("tauri::generate_handler![")
        .expect("the handler list")
        .1
        .split_once("])")
        .expect("its end")
        .0;
    assert!(list.lines().any(|l| l.trim() == "log_queries::ask_log,"));
    // Managed with the app's own whole-log tallies, so the award-need filter reads the award fold
    // the Awards view keeps rather than folding a second copy.
    assert!(lib
        .lines()
        .any(|l| l.trim() == ".manage(log_queries::LogQueries::with_tallies(log_tallies))"));
    assert!(lib
        .lines()
        .any(|l| l.trim() == ".manage(log_tallies.clone())"));
}

/// The questions the UI asks parse exactly: a field the question does not have, or a kind that
/// does not exist, is refused rather than answered as something else.
#[test]
fn a_question_is_parsed_exactly() {
    let parse = |v: Value| serde_json::from_value::<LogQuestion>(v);
    assert_eq!(
        parse(json!({"kind": "logSize"})).unwrap(),
        LogQuestion::LogSize {}
    );
    assert!(parse(json!({"kind": "logSize", "extra": 1})).is_err());
    assert!(parse(json!({"kind": "statistics", "extra": 1})).is_err());
    assert!(
        parse(json!({"kind": "gridPoints"})).is_err(),
        "the globe's dots need their band"
    );
    assert!(parse(json!({"kind": "nope"})).is_err());
    assert!(parse(json!({"kind": "row"})).is_err(), "a row needs its id");
    assert!(parse(json!({"kind": "page", "offset": 0, "limit": 1,
        "query": {"sort": "call", "asc": true, "search": "", "needsConfirmOnly": false, "x": 1}}))
    .is_err());
    assert_eq!(
        parse(json!({"kind": "callHistory", "call": "W1AW", "band": "20m", "mode": "FT8", "matchMode": true}))
            .unwrap(),
        LogQuestion::CallHistory {
            call: "W1AW".into(),
            band: "20m".into(),
            mode: "FT8".into(),
            match_mode: true
        }
    );
}

/// A JavaScript index names a row only when it is an integer in range.
#[test]
fn a_javascript_index_is_an_integer_in_range() {
    assert_eq!(position(0.0, 3), Some(0));
    assert_eq!(position(2.0, 3), Some(2));
    assert_eq!(position(3.0, 3), None);
    assert_eq!(position(-1.0, 3), None);
    assert_eq!(position(1.5, 3), None);
    assert_eq!(position(f64::NAN, 3), None);
}

// ── the statistics count each contact as the awards do ─────────────────────────────────────────

/// ★ The Statistics dashboard counts each contact under the entity the awards count it toward —
/// its record's DXCC number (LoTW's first), else its call through cty.dat, a WAE area as the
/// entity it belongs to — read from the store as the whole record holds it, with the app's own
/// resolvers. The entities, their count and the WAS states, by value.
#[test]
fn the_statistics_count_each_contact_as_the_awards_do() {
    let row = |call: &str, date: &str, fields: &str| {
        format!(
            "<CALL:{}>{call}<QSO_DATE:8>{date}<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
             <LOTW_QSL_RCVD:1>Y{fields}<EOR>\n",
            call.len()
        )
    };
    let d = Dir::new("stats-by-record");
    let rows = [
        // Navassa, Baker & Howland (LoTW's number over the log's 291) and Johnston on US calls.
        row("K1N", "20150210", "<DXCC:3>182<CQZ:1>8"),
        row("K1B", "20021201", "<DXCC:3>291<APP_NEXUS_LOTW_DXCC:2>20"),
        row("K3J", "20010101", "<DXCC:3>123"),
        // The German Democratic Republic, deleted in 1990: never the Federal Republic.
        row("Y24UK", "19890101", "<DXCC:3>229"),
        row("K1ABC", "20200101", "<CQZ:1>3<STATE:2>CA"),
        // A KP4 licensee in Florida: the United States, so Florida is a WAS state.
        row("KP4ABC", "20200102", "<DXCC:3>291<STATE:2>FL"),
        // No number: Sicily by its call, which counts as Italy; and the United States.
        row("IT9ABC", "20200103", ""),
        row("W1AW", "20200104", "<STATE:2>CT"),
    ];
    std::fs::write(d.log(), format!("{}{}", adif_header(), rows.concat())).expect("log.adi");
    let opened = tempo_app::logstore::open(
        &d.log(),
        Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
        None,
    )
    .expect("the store opens");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.attach_log_store(opened);
    let engine: SharedEngine = Arc::new(Mutex::new(e));
    let queries = LogQueries::default();
    let q: LogQuestion = serde_json::from_value(json!({"kind": "statistics"})).expect("parses");
    let counts = queries
        .answer(&engine, &q, &shown, &on_card, &counted)
        .expect("answered");
    let entities: Vec<(&str, u64)> = counts["entities"]
        .as_array()
        .expect("entities")
        .iter()
        .map(|t| (t["label"].as_str().unwrap(), t["count"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        entities,
        [
            ("Navassa Island", 1),
            ("Baker & Howland Islands", 1),
            ("Johnston Island", 1),
            ("GERMAN DEMOCRATIC REPUBLIC", 1),
            ("United States", 3),
            ("Italy", 1),
        ]
    );
    assert_eq!(counts["dxccEntities"], 6);
    let states: Vec<&str> = counts["byState"]
        .as_array()
        .expect("byState")
        .iter()
        .map(|t| t["label"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["CA", "FL", "CT"]);
}

// ── the diagnosis names its contacts by id ─────────────────────────────────────────────────────

/// ★ SPEC-2 v2 §3: every contact the desktop's confirmation diagnostics point at is named by its
/// id — each diagnosed row with the call, band, mode and time its list shows, each bucket's rows
/// in their own order, a duplicate's twin — so the Awards view asks for a row by id and uploads
/// by id. The same report UNNAMED, as the Remote sends it, carries none of those keys: the page's
/// parser admits no key it does not know.
#[test]
fn the_diagnosis_names_its_contacts_by_id_and_the_remote_report_is_unchanged() {
    use tempo_app::dto::DiagnosticsReportDto;
    use tempo_core::diagnostics::{diagnose, DiagCfg, DiagRow};
    let (golden, _) = golden_log();
    // An unconfirmed contact, and its field-identical twin that IS award-confirmed: a
    // duplicate to review.
    let mut rows: Vec<QsoRecord> = golden.clone();
    let mut twin = golden[1].clone();
    twin.id = Some(RecordId::Provisional {
        hash: 77,
        ordinal: 0,
    });
    twin.qsl_rcvd.lotw = true;
    twin.confirmed = true;
    twin.award_confirmed = true;
    rows.push(twin);
    let entities: Vec<Option<String>> = rows.iter().map(|_| None).collect();
    let report = diagnose(&rows, &entities, &[], 1_800_000_000, &DiagCfg::default());
    assert!(
        !report.diagnoses.is_empty(),
        "premise: something is diagnosed"
    );

    let unnamed = DiagnosticsReportDto::from(report.clone());
    let wire = serde_json::to_value(&unnamed).unwrap();
    for d in wire["diagnoses"].as_array().unwrap() {
        let keys: Vec<&str> = d.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["award", "index", "reasons", "status"],
            "the Remote's diagnosis"
        );
        for r in d["reasons"].as_array().unwrap() {
            assert!(r["action"].get("otherId").is_none());
        }
    }
    for b in wire["buckets"].as_array().unwrap() {
        let keys: Vec<&str> = b.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["count", "kind", "qsoIndices"], "the Remote's bucket");
    }

    let mut named = unnamed;
    let read: Vec<DiagRow> = rows.iter().map(DiagRow::from).collect();
    let ids: Vec<Option<RecordId>> = rows.iter().map(|r| r.id).collect();
    named.name_rows(&read, &ids);
    for d in &named.diagnoses {
        let r = &rows[d.index];
        assert_eq!(d.id, r.id.map(|id| id.to_string()), "row {}", d.index);
        assert_eq!(
            (
                d.call.as_deref(),
                d.band.as_deref(),
                d.mode.as_deref(),
                d.when_unix
            ),
            (
                Some(r.call.as_str()),
                Some(r.band.as_str()),
                Some(r.mode.as_str()),
                Some(r.when_unix)
            )
        );
    }
    for b in &named.buckets {
        let ids: Vec<Option<String>> = b
            .qso_indices
            .iter()
            .map(|&i| rows[i].id.map(|id| id.to_string()))
            .collect();
        assert_eq!(b.qso_ids.as_ref(), Some(&ids), "{}", b.kind);
    }
    let merge = named
        .diagnoses
        .iter()
        .flat_map(|d| &d.reasons)
        .find(|r| r.action.kind == "mergeDuplicate")
        .expect("premise: the twin is diagnosed as a duplicate");
    let twin_at = merge.action.other_index.expect("a twin");
    assert_eq!(
        merge.action.other_id.as_deref(),
        Some(rows[twin_at].id.unwrap().to_string().as_str())
    );
}

// ── the bench ───────────────────────────────────────────────────────────────────────────────────

// ── the field filters, the saved sort keys ──────────────────────────────────────────────────────

/// Records from ADIF, one per line.
fn adif_rows(rows: &[&str]) -> Vec<QsoRecord> {
    rows.iter()
        .map(|r| tempo_core::logbook::parse_adif(r).remove(0))
        .collect()
}

/// The calls a page of `filter` (newest first) holds, asked with the app's resolvers.
fn filtered_calls(queries: &LogQueries, engine: &SharedEngine, filter: Value) -> Vec<String> {
    let q: LogQuestion = serde_json::from_value(json!({"kind": "page", "offset": 0, "limit": 100,
        "query": {"sort": "call", "asc": true, "search": "", "needsConfirmOnly": false, "filter": filter}}))
    .expect("a question");
    let deadline = std::time::Instant::now() + tempo_app::test_util::TEST_WAIT;
    let page = loop {
        match queries.answer(engine, &q, &shown, &on_card, &counted) {
            Ok(a) => break a,
            Err(why) if why.starts_with(NOT_ANSWERED) && std::time::Instant::now() < deadline => {}
            Err(why) => panic!("{q:?}: refused: {why}"),
        }
    };
    page["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["call"].as_str().unwrap().to_string())
        .collect()
}

/// ★ The fields SQL cannot decide are decided by the engine on the rows SQL hands back: the
/// entity the Logbook SHOWS (a number set by hand wins over the call), the continent and CQ zone
/// the contact counts in (its record's own first) — and every filter combines with AND, the
/// search among them.
#[test]
fn the_engine_decides_entity_continent_and_zone_as_the_logbook_shows_them() {
    let d = Dir::new("filter-decide");
    let records = adif_rows(&[
        "<CALL:6>JA1ABC<QSO_DATE:8>20240101<TIME_ON:4>0100<BAND:3>20m<MODE:3>FT8<EOR>",
        "<CALL:6>DL1ABC<QSO_DATE:8>20240102<TIME_ON:4>0100<BAND:3>20m<MODE:2>CW<EOR>",
        // A W1 licensee in California: zone 3 by the record, not 5 by the prefix.
        "<CALL:5>W1ABC<QSO_DATE:8>20240103<TIME_ON:4>0100<BAND:3>40m<MODE:3>SSB<CQZ:1>3<EOR>",
        // K1N set to Navassa by its number.
        "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:4>1234<BAND:3>20m<MODE:2>CW<DXCC:3>182<EOR>",
        // A CONT the record carries wins over cty.dat's.
        "<CALL:6>VK2ABC<QSO_DATE:8>20240104<TIME_ON:4>0100<BAND:3>15m<MODE:3>FT8<CONT:2>AN<EOR>",
    ]);
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let calls = |f: Value| filtered_calls(&queries, &engine, f);
    assert_eq!(calls(json!({"entity": "japan"})), ["JA1ABC"]);
    assert_eq!(calls(json!({"entity": "Navassa Island"})), ["K1N"]);
    assert_eq!(calls(json!({"entity": "United States"})), ["W1ABC"]);
    assert_eq!(calls(json!({"continent": "EU"})), ["DL1ABC"]);
    assert_eq!(calls(json!({"continent": "AN"})), ["VK2ABC"]);
    assert_eq!(calls(json!({"continent": "OC"})), Vec::<String>::new());
    assert_eq!(calls(json!({"cqZone": 3})), ["W1ABC"]);
    assert_eq!(calls(json!({"cqZone": 25})), ["JA1ABC"]);
    // AND: with SQL's band, and with the search box.
    assert_eq!(calls(json!({"band": "20m", "continent": "NA"})), ["K1N"]);
    let q: LogQuestion = serde_json::from_value(json!({"kind": "page", "offset": 0, "limit": 9,
        "query": {"sort": "time", "asc": false, "search": "cw", "needsConfirmOnly": false,
                  "filter": {"band": "20m"}}}))
    .unwrap();
    let page = queries
        .answer(&engine, &q, &shown, &on_card, &counted)
        .unwrap();
    assert_eq!(page["total"], json!(2), "{page}");
    // A filter on nothing is no filter: the page is the unfiltered one, echoed without it.
    let q: LogQuestion = serde_json::from_value(json!({"kind": "page", "offset": 0, "limit": 9,
        "query": {"sort": "time", "asc": false, "search": "", "needsConfirmOnly": false,
                  "filter": {}, "then": []}}))
    .unwrap();
    let page = queries
        .answer(&engine, &q, &shown, &on_card, &counted)
        .unwrap();
    assert_eq!(page["total"], json!(5));
    assert_eq!(
        page["query"],
        json!({"sort": "time", "asc": false, "search": "", "needsConfirmOnly": false})
    );
}

/// ★ Every Logbook column sorts in the engine, from a real store: a stored column, a passthrough
/// tag, an upload stamp and a contest exchange (each read its own way), both directions, a contact
/// with nothing in the column last both ways, and as a shift-click key — the upload sort among
/// them, whose reads are all the Logbook's own columns but its stamps.
#[test]
fn every_logbook_column_sorts_in_the_engine_with_the_empty_last() {
    let d = Dir::new("sort-all-columns");
    let mut records = adif_rows(&[
        "<CALL:3>AAA<QSO_DATE:8>20240101<TIME_ON:6>010000<TIME_OFF:6>011500<BAND:3>20m<MODE:2>CW\
         <TX_PWR:3>100<CQZ:2>40<COMMENT:7>charlie<CONTEST_ID:8>CQ-WW-CW<SRX_STRING:6>599 33<EOR>",
        "<CALL:3>BBB<QSO_DATE:8>20240101<TIME_ON:6>010100<TIME_OFF:6>010130<BAND:3>20m<MODE:2>CW\
         <TX_PWR:1>9<CQZ:1>9<COMMENT:5>Alpha<CONTEST_ID:8>CQ-WW-CW<SRX_STRING:6>599 05<EOR>",
        "<CALL:3>CCC<QSO_DATE:8>20240101<TIME_ON:6>010200<TIME_OFF:6>010520<BAND:3>20m<MODE:2>CW\
         <TX_PWR:2>10<CQZ:2>10<NOTES:5>Bravo<CONTEST_ID:8>CQ-WW-CW<SRX_STRING:6>599 14<EOR>",
        "<CALL:3>EEE<QSO_DATE:8>20240101<TIME_ON:6>010300<BAND:3>20m<MODE:2>CW<EOR>",
    ]);
    for (r, outcome) in records.iter_mut().zip([
        UploadOutcome::Accepted,
        UploadOutcome::Rejected,
        UploadOutcome::Pending,
    ]) {
        r.upload.lotw = Some(UploadStatus {
            outcome,
            when_unix: 1,
            detail: None,
        });
    }
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let calls = |sort: &str, asc: bool, then: Value| -> Vec<String> {
        let a = answered(
            &queries,
            &engine,
            json!({"kind": "page", "offset": 0, "limit": 10, "query": {"sort": sort, "asc": asc,
                "search": "", "needsConfirmOnly": false, "then": then}}),
            &|_| None,
        );
        a["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|r| r["call"].as_str().unwrap().to_string())
            .collect()
    };
    for sort in ["end", "power", "cqz", "upLotw", "exchRcvd", "notes"] {
        assert_eq!(
            calls(sort, true, json!([])),
            ["BBB", "CCC", "AAA", "EEE"],
            "{sort} ascending"
        );
        assert_eq!(
            calls(sort, false, json!([])),
            ["AAA", "CCC", "BBB", "EEE"],
            "{sort} descending"
        );
    }
    // A second key: the mode ties every contact, so the LoTW mark orders them, the empty last.
    assert_eq!(
        calls("mode", true, json!([{"sort": "upLotw", "asc": false}])),
        ["AAA", "CCC", "BBB", "EEE"]
    );
}

/// ★ The added columns' orders, from the ONE table the Logbook's reference (`logQuery.ts`) is held
/// to as well (`sort-columns.json`): each case's rows through a real store, each order asked as a
/// page, so the engine and the reference cannot drift apart unseen.
#[test]
fn every_added_column_sorts_as_the_shared_table_says() {
    let table: Value = serde_json::from_str(SORT_COLUMNS).expect("sort-columns.json");
    let cases = table["cases"].as_array().expect("cases");
    assert!(cases.len() >= 30, "the table is read");
    for (n, case) in cases.iter().enumerate() {
        let name = case["name"].as_str().expect("a name");
        let records: Vec<QsoRecord> = case["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| {
                let dto: LoggedQso = serde_json::from_value(row.clone())
                    .unwrap_or_else(|e| panic!("{name}: a row is a LoggedQso: {e}"));
                // The DTO's contest block is a display copy the conversion drops (an edit keeps
                // the stored one), so a contest row's block is put on the record here.
                let contest = dto.contest.clone();
                let mut r = QsoRecord::from(dto);
                r.contest = contest.map(|c| {
                    Box::new(tempo_core::logbook::ContestFields {
                        contest_id: c.contest_id,
                        stx: c.stx,
                        srx: c.srx,
                        stx_string: c.stx_string,
                        srx_string: c.srx_string,
                        sent: c.sent,
                        rcvd: c.rcvd,
                        ..Default::default()
                    })
                });
                r
            })
            .collect();
        let d = Dir::new(&format!("sort-table-{n}"));
        let engine = on_store(&d, &records);
        let queries = LogQueries::default();
        for order in case["orders"].as_array().expect("orders") {
            let mut query = order.clone();
            let want = query
                .as_object_mut()
                .expect("an order")
                .remove("expect")
                .expect("expect");
            query["search"] = json!("");
            query["needsConfirmOnly"] = json!(false);
            let a = answered(
                &queries,
                &engine,
                json!({"kind": "page", "offset": 0, "limit": 20, "query": query}),
                &|_| None,
            );
            let got: Vec<Value> = a["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .map(|r| r["call"].clone())
                .collect();
            assert_eq!(Value::Array(got), want, "{name}: {query}");
        }
    }
}

/// The calls of the page `query` gives, and the page's `orderRev`.
fn sorted_page(queries: &LogQueries, engine: &SharedEngine, query: Value) -> (Vec<String>, u64) {
    let a = answered(
        queries,
        engine,
        json!({"kind": "page", "offset": 0, "limit": 20, "query": query}),
        &|_| None,
    );
    let calls = a["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["call"].as_str().unwrap().to_string())
        .collect();
    (calls, a["orderRev"].as_u64().expect("orderRev"))
}

/// Where `id` sits in `query`'s order, and the `orderRev` it was located in.
fn located(
    queries: &LogQueries,
    engine: &SharedEngine,
    query: Value,
    id: RecordId,
) -> (Value, u64) {
    let a = answered(
        queries,
        engine,
        json!({"kind": "locate", "query": query, "id": id.to_string()}),
        &|_| None,
    );
    (
        a["index"].clone(),
        a["orderRev"].as_u64().expect("orderRev"),
    )
}

/// ★ A sort by an upload mark follows the stamp, as the sent filter does
/// (`a_sent_filter_follows_an_upload_stamp`): a stamp moves only `content_rev`, and the order of a
/// column that SHOWS the stamp is built again on it — while an order that reads no stamp is kept.
/// The page names its order by an `orderRev` that moved with it, so the window, which keeps the
/// pages it has by (query, orderRev, offset), never files a page of the new order beside the old
/// order's pages; `locate` names the same order.
#[test]
fn an_upload_sort_follows_an_upload_stamp() {
    let d = Dir::new("sort-upload-stamp");
    let mut records = adif_rows(&[
        "<CALL:6>JA1AAA<QSO_DATE:8>20240101<TIME_ON:4>0100<BAND:3>20m<MODE:3>FT8<EOR>",
        "<CALL:6>DL1AAA<QSO_DATE:8>20240102<TIME_ON:4>0100<BAND:3>20m<MODE:2>CW<EOR>",
        "<CALL:6>VK2AAA<QSO_DATE:8>20240103<TIME_ON:4>0100<BAND:3>20m<MODE:2>CW<EOR>",
    ]);
    for (r, outcome) in records.iter_mut().zip([
        UploadOutcome::Pending,
        UploadOutcome::Pending,
        UploadOutcome::Accepted,
    ]) {
        r.upload.lotw = Some(UploadStatus {
            outcome,
            when_unix: 1,
            detail: None,
        });
    }
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let q = |sort: &str, then: Value| json!({"sort": sort, "asc": true, "search": "", "needsConfirmOnly": false, "then": then});
    let by_mark = q("upLotw", json!([]));
    // The mark as a second key: the mode ties the CW pair.
    let by_mode_then_mark = q("mode", json!([{"sort": "upLotw", "asc": true}]));
    let by_call = q("call", json!([]));
    let (order, mark_rev) = sorted_page(&queries, &engine, by_mark.clone());
    assert_eq!(order, ["JA1AAA", "DL1AAA", "VK2AAA"], "waiting, then done");
    let (order, _) = sorted_page(&queries, &engine, by_mode_then_mark.clone());
    assert_eq!(order, ["DL1AAA", "VK2AAA", "JA1AAA"]);
    let (_, call_rev) = sorted_page(&queries, &engine, by_call.clone());
    let built = || queries.0.built.load(Ordering::Relaxed);
    let before = built();
    let id = engine.lock().unwrap().stored_log()[0].id.expect("an id");
    assert_eq!(logged(&engine, id).call, "JA1AAA");
    let signed = tempo_app::station::LotwSigned::of(&logged(&engine, id)).expect("an id");
    tempo_app::logwrite::stamp_lotw_batch(&engine, &[signed], &accepted(1_790_000_000));
    engine.caught_up();
    let (order, rev) = sorted_page(&queries, &engine, by_mark.clone());
    assert_eq!(
        order,
        ["DL1AAA", "JA1AAA", "VK2AAA"],
        "the stamped contact is done now"
    );
    assert!(
        rev > mark_rev,
        "a new order under a new orderRev: {mark_rev} -> {rev}"
    );
    assert_eq!(
        located(&queries, &engine, by_mark, id),
        (json!(1), rev),
        "locate names the order the page does"
    );
    let (order, _) = sorted_page(&queries, &engine, by_mode_then_mark);
    assert_eq!(order, ["DL1AAA", "VK2AAA", "JA1AAA"]);
    assert_eq!(
        built(),
        before + 2,
        "each order that shows the stamp was built again"
    );
    let (_, rev) = sorted_page(&queries, &engine, by_call);
    assert_eq!(
        rev, call_rev,
        "an order that reads no stamp keeps its orderRev"
    );
    assert_eq!(built(), before + 2, "…and is kept through the stamp");
}

/// The QSL-sent date: marking a card sent moves only `content_rev`, and a sort by the date the
/// card went shows it at once.
#[test]
fn a_qsl_sent_date_sort_follows_a_card_marked_sent() {
    let d = Dir::new("sort-qsl-sent");
    let records = adif_rows(&[
        "<CALL:6>JA1AAA<QSO_DATE:8>20240101<TIME_ON:4>0100<BAND:3>20m<MODE:3>FT8<QSLSDATE:8>20240105<QSL_SENT:1>Y<EOR>",
        "<CALL:6>DL1AAA<QSO_DATE:8>20240102<TIME_ON:4>0100<BAND:3>20m<MODE:2>CW<EOR>",
    ]);
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let by_date =
        json!({"sort": "qslSentDate", "asc": false, "search": "", "needsConfirmOnly": false});
    let (order, before) = sorted_page(&queries, &engine, by_date.clone());
    assert_eq!(order, ["JA1AAA", "DL1AAA"], "no date last");
    let id = engine.lock().unwrap().stored_log()[1].id.expect("an id");
    assert_eq!(logged(&engine, id).call, "DL1AAA");
    assert!(change(
        &engine,
        id,
        &[tempo_app::logwrite::qsl_sent(
            id,
            Some(tempo_core::logbook::QslVia::Bureau)
        )]
    ));
    engine.caught_up();
    let (order, after) = sorted_page(&queries, &engine, by_date);
    assert_eq!(order, ["DL1AAA", "JA1AAA"], "the card sent today first");
    assert!(after > before);
}

/// ★ "Award need": the contacts whose band slot is worked and not yet confirmed, as the award fold
/// counts it — a slot another contact already confirmed is not needed, a satellite contact fills
/// no terrestrial slot, and VUCC's squares are held to the fold's own per-band counts.
#[test]
fn award_need_keeps_the_contacts_whose_slot_is_still_open() {
    let d = Dir::new("filter-need");
    let records = adif_rows(&[
        // Japan on 20 m: confirmed by JA1BBB, so JA1AAA is not needed there.
        "<CALL:6>JA1AAA<QSO_DATE:8>20240101<TIME_ON:4>0100<BAND:3>20m<MODE:3>FT8<EOR>",
        "<CALL:6>JA1BBB<QSO_DATE:8>20240101<TIME_ON:4>0200<BAND:3>20m<MODE:3>FT8<LOTW_QSL_RCVD:1>Y<EOR>",
        // Germany: open on 20 m and on 40 m.
        "<CALL:6>DL1AAA<QSO_DATE:8>20240102<TIME_ON:4>0100<BAND:3>20m<MODE:2>CW<EOR>",
        "<CALL:6>DL1AAA<QSO_DATE:8>20240103<TIME_ON:4>0100<BAND:3>40m<MODE:2>CW<EOR>",
        // WAS: Connecticut open on 6 m; the 6 m grids for VUCC and FFMA.
        "<CALL:5>W1AAA<QSO_DATE:8>20240104<TIME_ON:4>0100<BAND:2>6m<MODE:3>FT8<STATE:2>CT<GRIDSQUARE:6>FN31pr<EOR>",
        "<CALL:5>W1BBB<QSO_DATE:8>20240104<TIME_ON:4>0200<BAND:2>6m<MODE:3>FT8<STATE:2>MA<GRIDSQUARE:4>FN42<QSL_RCVD:1>Y<EOR>",
        "<CALL:5>W1CCC<QSO_DATE:8>20240104<TIME_ON:4>0300<BAND:2>6m<MODE:3>FT8<GRIDSQUARE:4>FN42<EOR>",
        // A satellite contact: no terrestrial DXCC slot, no VUCC band square.
        "<CALL:6>JA2SAT<QSO_DATE:8>20240105<TIME_ON:4>0100<BAND:2>2m<MODE:2>FM<PROP_MODE:3>SAT<GRIDSQUARE:4>PM95<EOR>",
    ]);
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let calls = |need: &str| filtered_calls(&queries, &engine, json!({"need": need}));
    // The United States on 6 m is confirmed by W1BBB's card: neither W1 contact is needed.
    assert_eq!(calls("dxcc"), ["DL1AAA", "DL1AAA"]);
    assert_eq!(calls("was"), ["W1AAA"]);
    assert_eq!(calls("vucc"), ["W1AAA"]);
    assert_eq!(calls("ffma"), ["W1AAA"]);
    // The positive control: JA1AAA is needed once nothing confirms Japan on 20 m.
    let d2 = Dir::new("filter-need-control");
    let engine2 = on_store(&d2, &records[..1]);
    assert_eq!(
        filtered_calls(&LogQueries::default(), &engine2, json!({"need": "dxcc"})),
        ["JA1AAA"]
    );
    // WAZ: zone 25 on 20 m is confirmed, zone 14 (Germany) is open on two bands, and CQ counts a
    // satellite contact's zone (zone 25 on 2 m), as the fold does.
    assert_eq!(calls("waz"), ["DL1AAA", "DL1AAA", "JA2SAT"]);
    // The VUCC squares counted here agree with the award fold, band by band.
    let summary = crate::log_folds::awards_kept(&engine, &crate::LogTallies::default()).unwrap();
    let six = summary
        .vucc
        .awards
        .iter()
        .find(|a| a.band == "6m")
        .expect("6 m");
    let slots = queries.need_slots(&engine, AwardNeed::Vucc).unwrap();
    assert_eq!((six.worked, six.confirmed), (2, 1), "premise");
    assert_eq!(
        slots.open.values().map(HashSet::len).sum::<usize>(),
        six.worked - six.confirmed
    );
}

/// ★ A filter on what was SENT follows an upload stamp: a stamp moves no order (`index_rev`
/// holds), so an order kept for "LoTW not sent" must be keyed on `content_rev` as well, or the
/// contact just uploaded stays listed as unsent.
#[test]
fn a_sent_filter_follows_an_upload_stamp() {
    let d = Dir::new("filter-sent");
    let records = adif_rows(&[
        "<CALL:6>JA1AAA<QSO_DATE:8>20240101<TIME_ON:4>0100<BAND:3>20m<MODE:3>FT8<EOR>",
        "<CALL:6>DL1AAA<QSO_DATE:8>20240102<TIME_ON:4>0100<BAND:3>20m<MODE:2>CW<EOR>",
    ]);
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let unsent = || filtered_calls(&queries, &engine, json!({"qsl": {"lotw": "notSent"}}));
    let by_call = || filtered_calls(&queries, &engine, json!({"call": "JA"}));
    assert_eq!(unsent(), ["DL1AAA", "JA1AAA"]);
    assert_eq!(by_call(), ["JA1AAA"]);
    let built = || queries.0.built.load(Ordering::Relaxed);
    let before = built();
    let id = engine.lock().unwrap().stored_log()[0].id.expect("an id");
    let signed = tempo_app::station::LotwSigned::of(&logged(&engine, id)).expect("an id");
    tempo_app::logwrite::stamp_lotw_batch(&engine, &[signed], &accepted(1_790_000_000));
    engine.caught_up();
    assert_eq!(unsent(), ["DL1AAA"], "the stamped contact is sent now");
    assert_eq!(
        built(),
        before + 1,
        "only the sent filter's order was built again"
    );
    assert_eq!(by_call(), ["JA1AAA"]);
    assert_eq!(
        built(),
        before + 1,
        "an order that reads nothing sent is kept through a stamp"
    );
}

/// The keys after the first and the field filters reach the order: the page is the reference
/// order of the rows the filter keeps, each key in its own direction.
#[test]
fn a_page_orders_by_every_key_it_names() {
    let d = Dir::new("filter-keys");
    let records = adif_rows(&[
        "<CALL:5>K1AAA<QSO_DATE:8>20240101<TIME_ON:4>0100<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<STATE:2>CT<EOR>",
        "<CALL:5>K1BBB<QSO_DATE:8>20240101<TIME_ON:4>0200<BAND:3>40m<FREQ:5>7.074<MODE:3>FT8<STATE:2>MA<EOR>",
        "<CALL:5>K1CCC<QSO_DATE:8>20240101<TIME_ON:4>0300<BAND:3>20m<FREQ:6>14.074<MODE:2>CW<STATE:2>MA<EOR>",
        "<CALL:5>K1DDD<QSO_DATE:8>20240101<TIME_ON:4>0400<BAND:3>40m<FREQ:5>7.074<MODE:2>CW<STATE:2>CT<EOR>",
        "<CALL:5>W9ZZZ<QSO_DATE:8>20240101<TIME_ON:4>0500<BAND:3>40m<FREQ:5>7.074<MODE:2>CW<STATE:2>IL<EOR>",
    ]);
    let engine = on_store(&d, &records);
    let queries = LogQueries::default();
    let page = |query: Value| -> Vec<String> {
        let q: LogQuestion = serde_json::from_value(
            json!({"kind": "page", "offset": 0, "limit": 9, "query": query}),
        )
        .unwrap();
        let a = queries
            .answer(&engine, &q, &shown, &on_card, &counted)
            .unwrap();
        a["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["call"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        page(
            json!({"sort": "state", "asc": true, "search": "", "needsConfirmOnly": false,
            "then": [{"sort": "band", "asc": false}], "filter": {"call": "K1"}})
        ),
        ["K1AAA", "K1DDD", "K1CCC", "K1BBB"]
    );
    assert_eq!(
        page(
            json!({"sort": "mode", "asc": false, "search": "", "needsConfirmOnly": false,
            "then": [{"sort": "state", "asc": true}, {"sort": "time", "asc": false}]})
        ),
        ["K1AAA", "K1BBB", "K1DDD", "W9ZZZ", "K1CCC"]
    );
}

/// A synthetic lifetime log of `n` contacts, written as `log.adi`: a pool of calls worked many
/// times each, the common bands and modes, countries, grids and a spread of years.
fn synthetic_log(d: &Dir, n: usize) {
    use std::fmt::Write as _;
    let bands = [
        ("20m", "14.074"),
        ("40m", "7.074"),
        ("15m", "21.074"),
        ("10m", "28.074"),
        ("2m", "144.174"),
    ];
    let modes = ["FT8", "FT8", "FT8", "CW", "USB", "FT4"];
    let countries = [
        "United States",
        "Canada",
        "Japan",
        "Germany",
        "England",
        "Brazil",
    ];
    let mut adif = adif_header();
    for i in 0..n {
        let call = format!(
            "K{}{}{:03}",
            i % 10,
            (b'A' + (i / 10 % 26) as u8) as char,
            i % 997
        );
        let (band, freq) = bands[i % bands.len()];
        let mode = modes[i / 7 % modes.len()];
        let country = countries[i / 13 % countries.len()];
        let day = 1 + i % 28;
        let (h, m) = (i / 60 % 24, i % 60);
        let year = 2010 + i % 15;
        let grid = format!("FN{:02}", i % 100);
        let _ = writeln!(
            adif,
            "<CALL:{}>{call}<BAND:{}>{band}<MODE:{}>{mode}<FREQ:{}>{freq}<QSO_DATE:8>{year}{:02}{day:02}\
             <TIME_ON:6>{h:02}{m:02}00<COUNTRY:{}>{country}<GRIDSQUARE:4>{grid}<RST_SENT:3>-10<RST_RCVD:3>-12<EOR>",
            call.len(),
            band.len(),
            mode.len(),
            freq.len(),
            1 + i % 12,
            country.len(),
        );
        // What the Logbook's field filters read: a state on every row, a county on a third, a
        // comment on a seventh, a contest on an eleventh, LoTW confirming a fifth.
        adif.pop(); // the newline
        adif.truncate(adif.len() - "<EOR>".len());
        let state = ["CT", "MA", "NY", "IL", "CA", "TX", "OH"][i % 7];
        let _ = write!(adif, "<STATE:2>{state}");
        if i % 3 == 0 {
            let county = format!("{state},COUNTY{:02}", i % 40);
            let _ = write!(adif, "<CNTY:{}>{county}", county.len());
        }
        if i % 7 == 0 {
            let _ = write!(adif, "<COMMENT:14>pileup at dusk");
        }
        if i % 11 == 0 {
            let _ = write!(adif, "<CONTEST_ID:8>CQ-WW-CW");
        }
        if i % 5 == 0 {
            let _ = write!(adif, "<LOTW_QSL_RCVD:1>Y");
        }
        // What the other Logbook columns show, so each sorts over real values: an end time, a
        // name and QTH, power, operator, the station's grid and rig, the zones, continent and
        // distance, the QSL dates, each service's stamp, a contest exchange — on a share of the
        // rows each, the rest left empty, as a lifetime log has them.
        let mut tag = |name: &str, val: &str| {
            let _ = write!(adif, "<{name}:{}>{val}", val.len());
        };
        if i % 2 == 0 {
            tag("TIME_OFF", &format!("{h:02}{:02}30", (m + 1 + i % 7) % 60));
            tag("NAME", ["Ann", "bob", "Carl", "dora", "Ed"][i % 5]);
            tag("TX_PWR", &format!("{}", [5, 10, 50, 100, 500, 1000][i % 6]));
        }
        if i % 3 == 0 {
            tag("QTH", ["Boston", "denver", "Tokyo", "berlin"][i % 4]);
            tag("CQZ", &format!("{}", 1 + i % 40));
            tag("ITUZ", &format!("{}", 1 + i % 75));
            tag("CONT", ["NA", "EU", "AS", "SA", "OC", "AF"][i % 6]);
            tag("DISTANCE", &format!("{}.{}", i % 19_000, i % 10));
        }
        if i % 4 == 0 {
            tag("OPERATOR", ["K2DEF", "w2xyz"][i % 2]);
            tag("MY_GRIDSQUARE", &format!("FN{:02}", i % 50));
            tag("MY_RIG", ["IC-7300", "ft-710", "K3"][i % 3]);
            tag("NOTES", ["met at hamfest", "QSL via buro"][i % 2]);
        }
        if i % 6 == 0 {
            let date = format!("{}{:02}{:02}", 2011 + i % 14, 1 + i % 12, day);
            tag("QSLSDATE", &date);
            tag("QSLRDATE", &date);
            tag("LOTW_QSLRDATE", &date);
            tag("EQSL_QSLRDATE", &date);
        }
        if i % 2 == 1 {
            let outcome = ["accepted", "pending", "rejected", "duplicate"][i % 4];
            let stamp = format!("{outcome}|{}", 1_700_000_000 + i);
            for service in ["LOTW", "EQSL", "QRZ", "CLUBLOG"] {
                tag(&format!("APP_TEMPO_UL_{service}"), &stamp);
            }
        }
        if i % 11 == 0 {
            tag("STX_STRING", &format!("599 {:02}", i % 40));
            tag("SRX_STRING", &format!("599 {:02}", (i / 3) % 40));
        }
        let _ = writeln!(adif, "<EOR>");
    }
    std::fs::write(d.log(), adif).expect("log.adi");
}

// The §4.11 bench's instruments (the lock watcher, SQLite's heap, the load line), by source from
// `crates/tempo-app/tests/support/lock_watch.rs`: the one copy, which tempo-app's
// `tests/log_bench.rs` includes too. Moving either file breaks this path at build time.
#[path = "../../../crates/tempo-app/tests/support/lock_watch.rs"]
mod lock_watch;

/// The engine's log questions timed on a synthetic lifetime log in the store —
/// `cargo test --manifest-path src-tauri/Cargo.toml --lib --features radio --release -- --ignored
/// --nocapture log_query_bench` (`LOG_QUERY_BENCH_ROWS` sets the size; 150,000 by default). In a
/// worktree, which lacks the gitignored AI CW model, a release build with `radio` refuses to build
/// without `NEXUS_ALLOW_MISSING_AICW=1`; the bench needs no model.
///
/// With §4.11's instruments (SPEC-2 v3): each question's longest Engine-lock hold, seen by a
/// watcher thread beside it, against the 5 ms bound on any log path — an answer is read off the
/// store with the lock released, so a hold is the handles alone — and the most SQLite's heap
/// held while it answered. The hold is asserted since the cut removed the log in memory
/// (`AFTER_THE_CUT`). The Rust heap is not counted here: a counting allocator would count every
/// test in this binary, not the bench's; tempo-app's `log_bench` counts the log path's.
#[test]
#[ignore = "a release-build bench, run by hand"]
fn log_query_bench() {
    use lock_watch::{Holds, Watcher};
    use std::cell::RefCell;
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};
    /// Whether the cut has removed the log in memory: the hold bound is asserted from then on.
    const AFTER_THE_CUT: bool = true;
    /// §4.11's bound on an Engine-lock hold in any log path.
    const HOLD_BOUND: Duration = Duration::from_millis(5);
    lock_watch::keep_sqlite_statistics();
    println!("{}", lock_watch::load_line("LOG_QUERY_BENCH"));
    let n: usize = std::env::var("LOG_QUERY_BENCH_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150_000);
    let d = Dir::new("bench");
    synthetic_log(&d, n);
    let opened = tempo_app::logstore::open(
        &d.log(),
        Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
        None,
    )
    .expect("the store opens");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.attach_log_store(opened);
    let engine: SharedEngine = Arc::new(Mutex::new(e));
    println!(
        "LOG_QUERY_BENCH rows={n} SQLite's heap once the log is attached: {}",
        lock_watch::sqlite_heap().map_or("not counted: its statistics are off".into(), |b| {
            format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0))
        })
    );
    let watcher = Watcher::start(&engine);
    let _ = watcher.holds();
    /// One step as the bench saw it: how long it took, what the watcher saw, and the most
    /// SQLite's heap held while it ran.
    struct Step {
        name: &'static str,
        took: f64,
        holds: Holds,
        peak: Option<i64>,
    }
    let steps: RefCell<Vec<Step>> = RefCell::new(Vec::new());
    // The app's row entity ([`shown`]) for a record that carries no DXCC number: its call's.
    let resolve = |call: &str| propagation::dxcc::resolve(call).map(|i| i.entity.to_string());
    let queries = LogQueries::default();
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
    let step = |name: &'static str, f: &mut dyn FnMut() -> Value| -> (Value, f64) {
        let _ = lock_watch::sqlite_heap_peak();
        let t = Instant::now();
        let a = f();
        let took = ms(t);
        steps.borrow_mut().push(Step {
            name,
            took,
            holds: watcher.holds(),
            peak: lock_watch::sqlite_heap_peak(),
        });
        (a, took)
    };
    let run = |name: &'static str, q: Value| -> (Value, f64) {
        step(name, &mut || ask(&queries, &engine, q.clone(), &resolve))
    };
    let query = |sort: &str, asc: bool, search: &str| json!({"sort": sort, "asc": asc, "search": search, "needsConfirmOnly": false});
    let page = |q: Value, offset: usize| json!({"kind": "page", "query": q, "offset": offset, "limit": 100});
    let (first, default_cold) = run("default_order_cold", page(query("time", false, ""), 0));
    let (_, default_warm) = run("default_page_warm", page(query("time", false, ""), n / 2));
    let (_, call_cold) = run("call_order_cold", page(query("call", true, ""), 0));
    let (_, call_warm) = run("call_page_warm", page(query("call", true, ""), n / 2));
    let (_, search_ssb) = run("search_ssb", page(query("time", false, "ssb"), 0));
    let (_, search_z) = run("search_z", page(query("time", false, "z"), 0));
    let (_, search_call) = run("search_call", page(query("time", false, "k3b"), 0));
    let id = first["keys"][50].as_str().unwrap().to_string();
    let (_, locate) = run(
        "locate",
        json!({"kind": "locate", "query": query("call", true, ""), "id": id}),
    );
    let (_, row) = run("row", json!({"kind": "row", "id": id}));
    let (history, call_history) = run(
        "call_history",
        json!({"kind": "callHistory", "call": "K3B003", "band": "20m", "mode": "FT8", "matchMode": false}),
    );
    let calls: Vec<String> = (0..20).map(|i| format!("K{}B{:03}", i % 10, i)).collect();
    let (_, calls_summary) = run(
        "calls_summary",
        json!({"kind": "callsSummary", "calls": calls}),
    );
    let many: Vec<String> = (0..100)
        .map(|i| format!("K{}C{:03}", i % 10, i * 7))
        .collect();
    let (_, worked_calls) = run(
        "worked_calls",
        json!({"kind": "workedCalls", "calls": many}),
    );
    let (_, entity_cold) = run("entity_cold", json!({"kind": "entity", "entity": "Japan"}));
    let (_, entity_warm) = run("entity_warm", json!({"kind": "entity", "entity": "Canada"}));
    let mut appended = Some({
        let mut r = logged(&engine, id.parse().unwrap());
        r.id = None;
        r.call = "JA1NEW".into();
        r.when_unix += 10;
        r
    });
    step("log a contact", &mut || {
        if let Some(r) = appended.take() {
            engine_lock(&engine).log_qso(r);
        }
        Value::Null
    });
    let (_, entity_after_append) = run(
        "entity_after_append",
        json!({"kind": "entity", "entity": "Japan"}),
    );
    let (_, rows_at) = run(
        "rows_at",
        json!({"kind": "rowsAt", "indices": [0, 10, n / 3, n / 2, n - 1]}),
    );
    let (_, log_size) = run("log_size", json!({"kind": "logSize"}));
    let (_, page_after_append) = run(
        "default_page_after_append",
        page(query("time", false, ""), 0),
    );
    // The folds: each cold, then kept; after a LoTW upload stamp only the backlog folds again.
    let (_, grids_cold) = run("worked_grids_cold", json!({"kind": "workedGrids"}));
    let (_, grids_warm) = run("worked_grids_warm", json!({"kind": "workedGrids"}));
    let (_, points_all) = run(
        "grid_points_all",
        json!({"kind": "gridPoints", "band": "all"}),
    );
    let (_, points_band) = run(
        "grid_points_20m",
        json!({"kind": "gridPoints", "band": "20m"}),
    );
    let (_, bands_cold) = run("bands_in_log", json!({"kind": "bandsInLog"}));
    let (_, stats_cold) = run("statistics_cold", json!({"kind": "statistics"}));
    let (_, stats_warm) = run("statistics_warm", json!({"kind": "statistics"}));
    let (_, backlog_cold) = run("lotw_backlog_cold", json!({"kind": "lotwBacklog"}));
    let signed = tempo_app::station::LotwSigned::of(&logged(&engine, id.parse().unwrap()));
    step("a LoTW upload's stamp", &mut || {
        tempo_app::logwrite::stamp_lotw_batch(
            &engine,
            &[signed.expect("an id")],
            &accepted(1_790_000_000),
        );
        Value::Null
    });
    let (_, backlog_after_stamp) = run("lotw_backlog_after_stamp", json!({"kind": "lotwBacklog"}));
    let (_, grids_after_stamp) = run("worked_grids_after_stamp", json!({"kind": "workedGrids"}));
    // The field filters and the extra sort keys, each a cold order (a first page).
    let filtered = |f: Value| {
        page(
            json!({"sort": "time", "asc": false, "search": "", "needsConfirmOnly": false, "filter": f}),
            0,
        )
    };
    let year = 1_420_070_400u64; // 2015-01-01
    let filter_steps: Vec<(&'static str, Value)> = vec![
        ("call_prefix", filtered(json!({"call": "K3B"}))),
        ("call_pattern", filtered(json!({"call": "K?B00*"}))),
        ("band_mode", filtered(json!({"band": "20m", "mode": "FT8"}))),
        (
            "dates_one_year",
            filtered(json!({"fromUnix": year, "toUnix": year + 365 * 86_400})),
        ),
        ("entity", filtered(json!({"entity": "United States"}))),
        ("continent", filtered(json!({"continent": "NA"}))),
        ("cq_zone", filtered(json!({"cqZone": 5}))),
        ("itu_zone", filtered(json!({"ituZone": 8}))),
        ("grid", filtered(json!({"grid": "FN3"}))),
        ("state", filtered(json!({"state": "CT"}))),
        ("county", filtered(json!({"county": "MA,"}))),
        ("iota", filtered(json!({"iota": "NA-001"}))),
        ("reference", filtered(json!({"reference": "K-0001"}))),
        (
            "lotw_confirmed",
            filtered(json!({"qsl": {"lotw": "confirmed"}})),
        ),
        (
            "lotw_not_sent",
            filtered(json!({"qsl": {"lotw": "notSent"}})),
        ),
        ("card_sent", filtered(json!({"qsl": {"card": "sent"}}))),
        ("contest", filtered(json!({"contest": "CQ-WW-CW"}))),
        ("text_ascii", filtered(json!({"text": "pileup"}))),
        ("text_unicode", filtered(json!({"text": "dämmerung"}))),
        ("need_dxcc", filtered(json!({"need": "dxcc"}))),
        ("need_vucc", filtered(json!({"need": "vucc"}))),
        // The fold kept by then, as the Awards view keeps it in the app: the pass alone.
        (
            "need_dxcc_fold_kept",
            filtered(json!({"need": "dxcc", "mode": "FT8"})),
        ),
        (
            "everything",
            filtered(
                json!({"call": "K", "band": "20m", "state": "CT", "qsl": {"lotw": "notSent"}, "need": "was"}),
            ),
        ),
        (
            "sort_three_keys",
            page(
                json!({"sort": "state", "asc": true, "search": "", "needsConfirmOnly": false,
            "then": [{"sort": "band", "asc": false}, {"sort": "call", "asc": true}]}),
                0,
            ),
        ),
        (
            "sort_county",
            page(
                json!({"sort": "county", "asc": true, "search": "", "needsConfirmOnly": false}),
                0,
            ),
        ),
    ];
    // Every sortable column, cold (each its own order, built once): the sixteen that sorted
    // before, then the columns added since. Every added column's cold order is held within ten
    // times the slowest of the sixteen's — the same order of magnitude.
    const BEFORE: [&str; 15] = [
        "call", "country", "band", "freq", "mode", "sent", "rcvd", "time", "park", "qsl", "grid",
        "state", "iota", "contest", "county",
    ];
    const ADDED: [&str; 22] = [
        "end",
        "myGrid",
        "myRig",
        "name",
        "qth",
        "power",
        "operator",
        "cqz",
        "ituz",
        "cont",
        "distance",
        "qslSentDate",
        "qslRcvdDate",
        "lotwRcvdDate",
        "eqslRcvdDate",
        "upLotw",
        "upEqsl",
        "upQrz",
        "upClublog",
        "exchSent",
        "exchRcvd",
        "notes",
    ];
    let mut sort_line = format!("LOG_QUERY_BENCH_SORTS rows={n}");
    let mut slowest_before = 0f64;
    let mut added_took = Vec::new();
    for (i, sort) in BEFORE.iter().chain(ADDED.iter()).enumerate() {
        // Ascending, which the default order is not, so `time` too builds its order.
        let (a, took) = run(sort, page(query(sort, true, ""), 0));
        assert_eq!(
            a["total"], a["logSize"],
            "{sort}: every contact in the order"
        );
        let _ = write!(sort_line, " {sort}={took:.1}ms");
        if i < BEFORE.len() {
            slowest_before = slowest_before.max(took);
        } else {
            added_took.push((*sort, took));
        }
    }
    println!("{sort_line}");
    for (sort, took) in added_took {
        assert!(
            took <= 10.0 * slowest_before,
            "{sort}: {took:.1} ms, more than ten times the slowest column that sorted before \
             ({slowest_before:.1} ms)"
        );
    }
    let mut filter_line = format!("LOG_QUERY_BENCH_FILTERS rows={n}");
    for (name, q) in filter_steps {
        let (a, took) = run(name, q);
        let _ = write!(filter_line, " {name}={took:.1}ms(total={})", a["total"]);
    }
    drop(watcher);
    println!(
        "LOG_QUERY_BENCH_FOLDS rows={n} worked_grids_cold={grids_cold:.1}ms worked_grids_warm={grids_warm:.3}ms \
         grid_points_all={points_all:.1}ms grid_points_20m={points_band:.1}ms bands_in_log={bands_cold:.1}ms \
         statistics_cold={stats_cold:.1}ms statistics_warm={stats_warm:.3}ms lotw_backlog_cold={backlog_cold:.1}ms \
         lotw_backlog_after_stamp={backlog_after_stamp:.1}ms worked_grids_after_stamp={grids_after_stamp:.3}ms"
    );
    println!(
        "LOG_QUERY_BENCH rows={n} default_order_cold={default_cold:.1}ms default_page_warm={default_warm:.1}ms \
         call_order_cold={call_cold:.1}ms call_page_warm={call_warm:.1}ms search_ssb={search_ssb:.1}ms \
         search_z={search_z:.1}ms search_call={search_call:.1}ms locate={locate:.2}ms row={row:.2}ms \
         call_history={call_history:.2}ms(count={}) calls_summary={calls_summary:.2}ms \
         worked_calls={worked_calls:.2}ms entity_cold={entity_cold:.1}ms entity_warm={entity_warm:.3}ms \
         entity_after_append={entity_after_append:.2}ms rows_at={rows_at:.2}ms log_size={log_size:.2}ms \
         default_page_after_append={page_after_append:.1}ms",
        history["count"]
    );
    println!(
        "  longest Engine-lock hold, by step (the bound is {:.0} ms, {}), with its time, how many \
         holds the watcher saw, the longest it did not look (a hold can read short or long by up \
         to that), and the most SQLite's heap held while it ran:",
        HOLD_BOUND.as_secs_f64() * 1000.0,
        if AFTER_THE_CUT {
            "asserted"
        } else {
            "asserted after the cut"
        }
    );
    let mut broken = Vec::new();
    for Step {
        name,
        took,
        holds: h,
        peak,
    } in steps.borrow().iter()
    {
        let over = h.longest > HOLD_BOUND;
        println!(
            "    {:>9.3} ms  {name}{}  [took {took:.2} ms, {} holds, blind at most {:.3} ms, SQLite \
             peak {}]",
            h.longest.as_secs_f64() * 1000.0,
            if over { "  ← over" } else { "" },
            h.holds,
            h.blind.as_secs_f64() * 1000.0,
            peak.map_or("not counted".into(), |b| format!(
                "{:.1} MiB",
                b as f64 / (1024.0 * 1024.0)
            )),
        );
        if AFTER_THE_CUT && over {
            broken.push(format!(
                "{name} held the Engine lock {:.3} ms",
                h.longest.as_secs_f64() * 1000.0
            ));
        }
    }
    println!("{filter_line}");
    assert!(
        broken.is_empty(),
        "§4.11's hold bound, broken at {n} rows:\n  {}",
        broken.join("\n  ")
    );
}

/// ★ The path filter is decided by the engine from each contact's grade: "long" keeps the contacts
/// whose grade names the long path, "polar" those whose graded route crosses the polar cap. A
/// contact with no grade passes neither. CONTROL: with no path filter every contact shows.
#[test]
fn the_path_filter_keeps_the_long_and_the_polar_paths() {
    let rec = |call: &str, grid: &str, route: Option<&str>| {
        let mut s = format!(
            "<EOH><CALL:{}>{call}<QSO_DATE:8>20260507<TIME_ON:6>100000<BAND:3>20m<MODE:3>FT8\
             <GRIDSQUARE:4>{grid}<MY_GRIDSQUARE:4>FN31",
            call.len()
        );
        if let Some(r) = route {
            let why = format!("MARGIN_{r};{grid}/20m");
            s += &format!("<APP_NEXUS_PATH:{}>{r}", r.len());
            s += &format!("<APP_NEXUS_PATH_WHY:{}>{why}", why.len());
        }
        s += "<EOR>";
        tempo_core::logbook::parse_adif(&s).remove(0)
    };
    let records = vec![
        rec("VK2LP", "QF56", Some("L")),
        rec("VK2SP", "QF56", Some("S")),
        rec("JA1PO", "PM95", Some("S")),
        rec("G4EU", "IO91", Some("S")),
        rec("K1NONE", "IO91", None),
    ];
    let d = Dir::new("filter-path");
    let engine = on_store(&d, &records);
    let q = LogQueries::default();
    assert_eq!(
        filtered_calls(&q, &engine, json!({"path": "long"})),
        ["VK2LP"]
    );
    assert_eq!(
        filtered_calls(&q, &engine, json!({"path": "polar"})),
        ["JA1PO"]
    );
    assert_eq!(filtered_calls(&q, &engine, json!({"band": "20m"})).len(), 5);
}
