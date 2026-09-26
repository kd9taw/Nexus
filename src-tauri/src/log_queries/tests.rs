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
use tempo_core::logbook::query::{
    reference, BandsInLog, GridCounts, LogStatCounter, LotwBacklog, WorkedGrids,
};
use tempo_core::logbook::{
    adif_header, adif_record_own_log, LogOp, QsoEdit, UploadOutcome, UploadService, UploadStatus,
};

// ── fixtures ────────────────────────────────────────────────────────────────────────────────────

const GOLDEN_LOG: &str = include_str!("../../../ui/src/features/__fixtures__/log-query/log.json");
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
    queries
        .answer(engine, &q, resolve)
        .unwrap_or_else(|e| panic!("{q:?}: {e}"))
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

/// ★ THE ENGINE ANSWERS WHAT THE UI ANSWERED. Every golden question, sent as the UI sends it (the
/// question JSON, parsed as the command parses it), is answered exactly as the golden file froze
/// it — the statistics as the counts the window finishes into it — with the log in the store, and
/// on the 1.13 path, where since SPEC-2 v3 C19 (D1-A) it is in a store in memory beside
/// `log.adi`.
#[test]
fn every_golden_answer_on_both_homes_of_the_log() {
    let (records, entities) = golden_log();
    let resolve = |call: &str| entities.get(call).cloned().flatten();
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
        for (q, a) in golden_answers() {
            let want = if q["kind"] == "statistics" {
                golden_statistics()
            } else {
                a
            };
            // A question naming a row names it by the fixture's id; the engine holds it under
            // the record id that id stands for.
            let mut asked = q.clone();
            if let Some(name) = q["id"].as_str().filter(|n| n.starts_with("fx")) {
                asked["id"] = json!(golden_id(name).to_string());
            }
            let got = ask(&queries, &engine, asked, &resolve);
            assert_eq!(normalise(&q, got), want, "{home}: {q}");
            answered += 1;
        }
        assert_eq!(answered, 78, "{home}: every question in the golden file");
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
            json!(reference::fold(log, LogStatCounter::new(&of))),
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
/// store takes it on the writer's own thread: the read waits for it.
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
        let page = ask(
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
        let history = ask(
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
/// — and all that time the radio loop's lock can be taken. The wait runs out, the answer is the
/// store as it stands, and it is not kept: asked again at the same revisions once the writer has
/// caught up, the question shows the contact.
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
    let started = std::time::Instant::now();
    let answer = std::thread::scope(|s| {
        let asking = s.spawn(|| ask(&queries, &engine, question.clone(), &resolve));
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
    assert_eq!(answer["total"], 24, "answered from the store as it stands");
    drop(hold);
    assert!(
        lock(&queries.0.orders).is_empty(),
        "an order read before the writer caught up is not kept"
    );
    let caught_up = ask(&queries, &engine, question, &resolve);
    assert_eq!(
        caught_up["orderRev"], answer["orderRev"],
        "premise: the same revisions"
    );
    assert_eq!(
        caught_up["total"], 25,
        "the contact logged before the question is in its answer now"
    );
}

/// A fold read before the writer caught up is not kept either: the wait runs out, the answer is
/// the store as it stands, and the next question, once the writer has caught up, folds again and
/// has the contact.
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
    let grids = || ask(&queries, &engine, json!({"kind": "workedGrids"}), &resolve);
    let stale = grids();
    assert!(
        !stale.as_array().unwrap().contains(&json!("AA00")),
        "answered from the store as it stands"
    );
    drop(hold);
    let caught_up = grids();
    assert!(
        caught_up.as_array().unwrap().contains(&json!("AA00")),
        "the contact logged before the question is in its answer now"
    );
    assert_eq!(queries.0.folded.load(Ordering::Relaxed), 2, "folded again");
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
    let searches = ["", "w1", "ssb", "z", "ſ", "20m", "2023-11"];
    for _ in 0..3 {
        let query = json!({
            "sort": g.pick(&sorts), "asc": g.below(2) == 0,
            "search": g.pick(&searches), "needsConfirmOnly": g.below(3) == 0,
        });
        let parsed: LogQuery = serde_json::from_value(query.clone()).unwrap();
        let order = reference::order(&held, &parsed);
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
            json!(reference::fold(&held, LogStatCounter::new(&of))),
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
    assert!(lib
        .lines()
        .any(|l| l.trim() == ".manage(log_queries::LogQueries::default())"));
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
    let resolve = |call: &str| cty(call);
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
         holds the watcher saw, the longest it did not look (a hold can read short by up to that), \
         and the most SQLite's heap held while it ran:",
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
    assert!(
        broken.is_empty(),
        "§4.11's hold bound, broken at {n} rows:\n  {}",
        broken.join("\n  ")
    );
}
