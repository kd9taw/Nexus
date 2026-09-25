//! Complete-log display context for the native JS8 roster, read from one picture of the log
//! off the engine lock (see `picture`). A shared cache is keyed by the log's revision and the
//! heard-call set, never by an individual browser.
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::TryLockError;
use std::time::{Duration, Instant};
use tempo_core::logbook::sqlite::Narrow;

/// What the roster's history reads of every contact: the call it is matched on, the time the
/// latest is chosen by, and the three fields shown from it.
const HISTORY: Narrow = Narrow {
    columns: &["call", "when_unix", "grid", "name", "comment"],
    uploads: false,
};
/// The log the history is read from is bounded, as it has always been.
const LOG_ROWS: usize = 1_000_000;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct History {
    count: usize,
    last_unix: Option<u64>,
    grid: String,
    name: String,
    comment: String,
}
#[derive(Default)]
pub(super) struct Cache {
    /// The log's revision the history was read at.
    revision: Option<u64>,
    calls: Vec<String>,
    history: BTreeMap<String, History>,
}
fn calls(e: &tempo_app::engine::Engine) -> Result<Vec<String>, &'static str> {
    if e.js8_heard().len() > 500 || e.js8_heard().iter().any(|h| h.call.len() > 32) {
        return Err("applicationTooLarge");
    }
    let mut calls: Vec<_> = e
        .js8_heard()
        .iter()
        .map(|h| h.call.trim().to_uppercase())
        .collect();
    calls.sort();
    calls.dedup();
    Ok(calls)
}
impl Cache {
    pub(super) fn read(&mut self, engine: &crate::SharedEngine) -> Result<Value, &'static str> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let lock = || loop {
            if Instant::now() >= deadline {
                return Err("applicationBusy");
            }
            match tempo_app::engine::engine_try_lock(engine) {
                Ok(e) => return Ok(e),
                Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        let (revision, rows, heard, class) = {
            let e = lock()?;
            (
                e.log_revision(),
                e.log_rows(),
                calls(&e)?,
                e.settings().license_class,
            )
        };
        let unchanged = |e: &tempo_app::engine::Engine| e.settings().license_class == class;
        if self.revision != Some(revision) || self.calls != heard {
            let mut history: BTreeMap<_, _> = heard
                .iter()
                .map(|c| (c.clone(), History::default()))
                .collect();
            super::picture::read(&rows, |log| {
                if log.count()? > LOG_ROWS {
                    return Err("applicationTooLarge");
                }
                if heard.is_empty() {
                    return Ok(());
                }
                // Only the four display fields, and only for heard calls. No full
                // QSO/notes/connector data is read.
                log.each(HISTORY, &mut |pick, q| {
                    super::picture::within(deadline, pick)?;
                    if q.call.len() > 128 {
                        return Err("applicationTooLarge");
                    }
                    let call = q.call.trim().to_uppercase();
                    let Some(h) = history.get_mut(&call) else {
                        return Ok(());
                    };
                    let fields = [
                        q.grid.as_deref().unwrap_or(""),
                        q.name.as_deref().unwrap_or(""),
                        q.comment.as_deref().unwrap_or(""),
                    ];
                    if q.when_unix > 9_007_199_254_740_991 || fields.iter().any(|s| s.len() > 1024)
                    {
                        return Err("applicationTooLarge");
                    }
                    let [grid, name, comment] = fields.map(|s| s.trim().to_owned());
                    h.count += 1;
                    // Native callHistory keeps the first row on equal timestamps
                    // and uses the latest contact, even if its fields are empty.
                    if h.last_unix.is_none_or(|last| q.when_unix > last) {
                        h.last_unix = Some(q.when_unix);
                        h.grid = grid;
                        h.name = name;
                        h.comment = comment;
                    }
                    Ok(())
                })
            })?;
            {
                let e = lock()?;
                if !unchanged(&e) || calls(&e)? != heard {
                    return Err("applicationBusy");
                }
            }
            self.revision = Some(revision);
            self.calls = heard.clone();
            self.history = history;
        }
        let plan: Vec<_> = tempo_app::bandplan::js8_band_plan()
            .into_iter()
            .map(|mut c| {
                c.tx = tempo_app::privileges::tx_allowed(
                    class,
                    c.dial_mhz,
                    tempo_app::settings::OperatingMode::Digital,
                );
                c
            })
            .collect();
        {
            let e = lock()?;
            if !unchanged(&e) || calls(&e)? != heard {
                return Err("applicationBusy");
            }
        }
        let value = json!({ "plan": plan, "history": self.history });
        if serde_json::to_vec(&value)
            .map_err(|_| "applicationUnavailable")?
            .len()
            > 192 * 1024
        {
            return Err("applicationTooLarge");
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_service::stored_log_tests::StoredLog;
    use std::sync::Arc;
    fn engine() -> crate::SharedEngine {
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        let journal = json!({"inbox":[],"heard":[
            {"call":"W1AW","grid":null,"snrDb":-8,"freqHz":1500.0,"speed":"normal","lastMs":1,"lastHb":true,"lastCq":false,"storedMsgs":0},
            {"call":"K2ABC","grid":null,"snrDb":-12,"freqHz":1000.0,"speed":"slow","lastMs":1,"lastHb":false,"lastCq":true,"storedMsgs":0}
        ],"allcallReplied":[],"nextInboxId":1});
        e.js8_load_journal(&journal.to_string());
        assert_eq!(e.js8_heard().len(), 2);
        e.import_adif("<CALL:4>W1AW<BAND:3>40m<MODE:3>SSB<QSO_DATE:8>20260908<TIME_ON:6>010000<GRIDSQUARE:4>FN31<NAME:3>OLD<COMMENT:3>OLD<EOR>");
        let base = e.stored_log()[0].as_ref().clone();
        let adif: String = (1..2302)
            .map(|i| {
                let mut q = base.clone();
                q.call = if i == 2301 {
                    "w1aw".into()
                } else {
                    format!("K1T{i}")
                };
                q.when_unix += i * 60;
                q.grid = None;
                q.name = None;
                q.comment = None;
                tempo_core::logbook::adif_record(&q)
            })
            .collect();
        e.import_adif(&adif);
        assert_eq!(e.stored_log().len(), 2302);
        Arc::new(std::sync::Mutex::new(e))
    }
    /// Every contact with a heard call joins its history — the latest one's fields even when
    /// they are empty — from ONE pass, made with the Engine lock free; an unchanged log and
    /// heard set are not read again; a change is.
    #[test]
    fn complete_log_join_preserves_latest_empty_fields_and_reuses_generation() {
        use super::super::picture::{at_seams, Seam};
        let engine = engine();
        let mut cache = Cache::default();
        let (hook, passes) = (engine.clone(), std::rc::Rc::new(std::cell::Cell::new(0)));
        let counted = passes.clone();
        let value = at_seams(
            move |seam| {
                if seam == Seam::Each {
                    assert!(hook.try_lock().is_ok(), "the pass holds no Engine lock");
                    counted.set(counted.get() + 1);
                }
            },
            || cache.read(&engine),
        )
        .unwrap();
        assert_eq!(passes.get(), 1, "one pass over the whole log");
        assert_eq!(value["history"]["W1AW"]["count"], 2);
        assert_eq!(value["history"]["W1AW"]["comment"], "");
        assert_eq!(value["history"]["W1AW"]["grid"], "");
        assert_eq!(value["history"]["W1AW"]["name"], "");
        assert_eq!(value["history"]["K2ABC"]["count"], 0);
        assert!(value["history"]["K2ABC"]["lastUnix"].is_null());
        assert!(!value["plan"].as_array().unwrap().is_empty());
        assert_eq!(
            at_seams(
                |_| panic!("unchanged log was read again"),
                || cache.read(&engine)
            )
            .unwrap(),
            value
        );
        {
            let mut e = engine.lock().unwrap();
            let index = e
                .stored_log()
                .iter()
                .enumerate()
                .max_by_key(|(_, q)| q.when_unix)
                .unwrap()
                .0;
            let mut q = e.stored_log()[index].as_ref().clone();
            q.comment = Some("CURRENT".into());
            assert!(e.update_qso(q.id.unwrap(), q));
        }
        assert_eq!(
            cache.read(&engine).unwrap()["history"]["W1AW"]["comment"],
            "CURRENT"
        );
    }
    /// An edit landing while the history is read is not half in it, and does not refuse it:
    /// the answer is the log the read found, and the next read — the log's revision has moved
    /// — has the edit. An oversized field of a heard call's contact refuses the whole context
    /// rather than showing a station unworked.
    #[test]
    fn an_edit_during_the_read_belongs_to_the_next_read_and_oversized_context_is_refused() {
        use super::super::picture::{at_seams, Seam};
        let engine = engine();
        let mut cache = Cache::default();
        let (hook, edits) = (engine.clone(), std::rc::Rc::new(std::cell::Cell::new(0)));
        let counted = edits.clone();
        let during = at_seams(
            move |seam| {
                if seam != Seam::Each {
                    return;
                }
                // Both halves of the history move: the first W1AW contact becomes another
                // station's, and the latest one gains a comment.
                let mut e = hook.lock().unwrap();
                let last = e.stored_log().len() - 1;
                let mut latest = e.stored_log()[last].as_ref().clone();
                assert_eq!(latest.call, "w1aw", "premise: the latest W1AW contact");
                latest.comment = Some("CHANGED".into());
                assert!(e.update_qso(latest.id.unwrap(), latest));
                let mut first = e.stored_log()[0].as_ref().clone();
                first.call = "K9ZZZ".into();
                assert!(e.update_qso(first.id.unwrap(), first));
                counted.set(counted.get() + 1);
            },
            || cache.read(&engine),
        )
        .unwrap();
        assert_eq!(
            edits.get(),
            1,
            "premise: the edits landed while the read ran"
        );
        assert_eq!(
            (
                &during["history"]["W1AW"]["count"],
                &during["history"]["W1AW"]["comment"]
            ),
            (&json!(2), &json!("")),
            "the history is the log as the read found it, in both halves"
        );
        let after = cache.read(&engine).unwrap();
        assert_eq!(
            (
                &after["history"]["W1AW"]["count"],
                &after["history"]["W1AW"]["comment"]
            ),
            (&json!(1), &json!("CHANGED")),
            "and the next read has both edits"
        );
        {
            let mut e = engine.lock().unwrap();
            let last = e.stored_log().len() - 1;
            let mut q = e.stored_log()[last].as_ref().clone();
            q.comment = Some("x".repeat(1025));
            assert!(e.update_qso(q.id.unwrap(), q));
        }
        assert_eq!(
            Cache::default().read(&engine).unwrap_err(),
            "applicationTooLarge"
        );
    }

    // ── the roster's history from the store, held to the code before C18 ─────────────────
    //
    // SPEC-2 v3 C18: the history reads one picture of the logbook store now. The oracle is the
    // code before C18, VERBATIM — its cache keyed on the log's read token and its chunked read
    // of the log in memory — beside the store that log mirrors.

    #[derive(Default)]
    struct OldCache {
        token: Option<Arc<()>>,
        calls: Vec<String>,
        history: BTreeMap<String, History>,
    }
    impl OldCache {
        fn read(&mut self, engine: &crate::SharedEngine) -> Result<Value, &'static str> {
            self.read_chunks(engine, |_| {})
        }
        fn read_chunks(
            &mut self,
            engine: &crate::SharedEngine,
            mut after_chunk: impl FnMut(usize),
        ) -> Result<Value, &'static str> {
            let deadline = Instant::now() + Duration::from_secs(2);
            let lock = || loop {
                if Instant::now() >= deadline {
                    return Err("applicationBusy");
                }
                match tempo_app::engine::engine_try_lock(engine) {
                    Ok(e) => return Ok(e),
                    Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
                    Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
                }
            };
            let (token, count, heard, class) = {
                let e = lock()?;
                if e.log_records().len() > 1_000_000 {
                    return Err("applicationTooLarge");
                }
                (
                    e.log_read_token(),
                    e.log_records().len(),
                    calls(&e)?,
                    e.settings().license_class,
                )
            };
            let unchanged = |e: &tempo_app::engine::Engine| {
                Arc::ptr_eq(&token, &e.log_read_token()) && e.settings().license_class == class
            };
            if self.token.as_ref().is_none_or(|t| !Arc::ptr_eq(t, &token)) || self.calls != heard {
                let mut history: BTreeMap<_, _> = heard
                    .iter()
                    .map(|c| (c.clone(), History::default()))
                    .collect();
                if !heard.is_empty() {
                    for offset in (0..count).step_by(128) {
                        // Copy only the four display fields, and only for heard calls.
                        // No full QSO/notes/connector data crosses this lock boundary.
                        let rows = {
                            let e = lock()?;
                            if !unchanged(&e) {
                                return Err("applicationBusy");
                            }
                            let mut rows = Vec::new();
                            for q in &e.log_records()[offset..(offset + 128).min(count)] {
                                if q.call.len() > 128 {
                                    return Err("applicationTooLarge");
                                }
                                let call = q.call.trim().to_uppercase();
                                if !history.contains_key(&call) {
                                    continue;
                                }
                                let fields = [
                                    q.grid.as_deref().unwrap_or(""),
                                    q.name.as_deref().unwrap_or(""),
                                    q.comment.as_deref().unwrap_or(""),
                                ];
                                if q.when_unix > 9_007_199_254_740_991
                                    || fields.iter().any(|s| s.len() > 1024)
                                {
                                    return Err("applicationTooLarge");
                                }
                                rows.push((call, q.when_unix, fields.map(|s| s.trim().to_owned())));
                            }
                            rows
                        };
                        for (call, when, [grid, name, comment]) in rows {
                            let h = history.get_mut(&call).ok_or("applicationUnavailable")?;
                            h.count += 1;
                            // Native callHistory keeps the first row on equal timestamps
                            // and uses the latest contact, even if its fields are empty.
                            if h.last_unix.is_none_or(|last| when > last) {
                                h.last_unix = Some(when);
                                h.grid = grid;
                                h.name = name;
                                h.comment = comment;
                            }
                        }
                        after_chunk(offset);
                    }
                }
                {
                    let e = lock()?;
                    if !unchanged(&e) || calls(&e)? != heard {
                        return Err("applicationBusy");
                    }
                }
                self.token = Some(token.clone());
                self.calls = heard.clone();
                self.history = history;
            }
            let plan: Vec<_> = tempo_app::bandplan::js8_band_plan()
                .into_iter()
                .map(|mut c| {
                    c.tx = tempo_app::privileges::tx_allowed(
                        class,
                        c.dial_mhz,
                        tempo_app::settings::OperatingMode::Digital,
                    );
                    c
                })
                .collect();
            {
                let e = lock()?;
                if !unchanged(&e) || calls(&e)? != heard {
                    return Err("applicationBusy");
                }
            }
            let value = json!({ "plan": plan, "history": self.history });
            if serde_json::to_vec(&value)
                .map_err(|_| "applicationUnavailable")?
                .len()
                > 192 * 1024
            {
                return Err("applicationTooLarge");
            }
            Ok(value)
        }
    }

    use super::super::log_tests::{launch, memory, settle, synthetic_log, Dir, Gen};

    /// Heard calls: worked often, once, never; spelled as a heard list spells them; and the
    /// ASCII calls that a logged call outside ASCII upper-cases INTO (`K1ſAB` → `K1SAB`).
    fn hear(e: &crate::SharedEngine) {
        let heard: Vec<Value> = [
            "W1AW", "K1ABC", "dl1abc", "JA1AA", "K1SAB", "K1IAB", "ZZ9ZZZ",
        ]
        .iter()
        .map(|c| {
            json!({"call":c,"grid":null,"snrDb":-8,"freqHz":1500.0,"speed":"normal",
                "lastMs":1,"lastHb":true,"lastCq":false,"storedMsgs":0})
        })
        .collect();
        let journal = json!({"inbox":[],"heard":heard,"allcallReplied":[],"nextInboxId":1});
        e.lock().unwrap().js8_load_journal(&journal.to_string());
    }

    fn assert_history_is_the_old_history(e: &crate::SharedEngine, what: &str) {
        let old = OldCache::default().read(e).map(|v| v.to_string());
        let new = Cache::default().read(e).map(|v| v.to_string());
        assert!(
            new == old,
            "{what}: the roster's history differs\nstore:  {new:.600?}\nmemory: {old:.600?}"
        );
    }

    /// ★ PARITY: the history of every heard call, over 3,000 contacts, read from the store and
    /// from the 1.13 path, is byte for byte the history the log in memory gave — counts, the
    /// latest contact's fields (empty ones too), ties in the same second.
    #[test]
    fn the_history_read_from_the_store_is_the_history_of_the_log_in_memory() {
        let text = synthetic_log(3_000, 0x0C18_A758);
        let d = Dir::new("js8");
        std::fs::write(d.log(), &text).unwrap();
        let (store, mem) = (launch(&d), memory(&text));
        hear(&store);
        hear(&mem);
        let value = Cache::default().read(&store).unwrap();
        assert!(
            value["history"]["W1AW"]["count"].as_u64().unwrap() > 20,
            "premise"
        );
        assert!(
            value["history"]["K1SAB"]["count"].as_u64().unwrap() > 0,
            "premise: ſ folds"
        );
        assert_eq!(
            value["history"]["ZZ9ZZZ"]["count"], 0,
            "premise: a call never logged"
        );
        assert_history_is_the_old_history(&store, "the store");
        assert_history_is_the_old_history(&mem, "the 1.13 path");
        settle(&store);
    }

    /// ★ THE PROPERTY: after every one of 24 random changes to each of eight seeded logs, the
    /// history read from the store is the history of the log in memory — and a cache kept
    /// across the changes answers as a fresh read does.
    #[test]
    fn after_every_change_the_history_read_from_the_store_is_the_old_history() {
        for seed in 1..=8u64 {
            let d = Dir::new(&format!("js8-prop-{seed}"));
            std::fs::write(d.log(), synthetic_log(200, seed * 15_485_863)).unwrap();
            let e = launch(&d);
            hear(&e);
            let mut kept = Cache::default();
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            for step in 0..24u64 {
                super::super::log_tests::random_change(&e, &mut g, step);
                let what = format!("seed {seed}, step {step}");
                assert_history_is_the_old_history(&e, &what);
                assert_eq!(
                    kept.read(&e).map(|v| v.to_string()),
                    OldCache::default().read(&e).map(|v| v.to_string()),
                    "{what}: the kept cache"
                );
            }
            settle(&e);
        }
    }

    /// ★ THE BUDGET IS KEPT: a read of the log still has the two seconds the chunked read had,
    /// from before it takes the Engine lock. One whose pass would start past them — held up
    /// here at the start of its pass — is refused as busy, the answer the chunked read gave one
    /// that ran long, not answered late. The control: the same read, on time, answers.
    #[test]
    fn a_read_past_its_budget_is_refused_as_busy() {
        use super::super::picture::{at_seams, Seam};
        let engine = engine();
        let late = at_seams(
            |seam| {
                if seam == Seam::Each {
                    std::thread::sleep(std::time::Duration::from_millis(2_050));
                }
            },
            || Cache::default().read(&engine),
        );
        assert!(matches!(late, Err("applicationBusy")));
        assert!(
            Cache::default().read(&engine).is_ok(),
            "control: on time, it answers"
        );
    }
}
