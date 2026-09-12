//! Complete-log display context for the native JS8 roster. A shared cache is
//! keyed by log identity and the heard-call set, never by an individual browser.
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, TryLockError};
use std::time::{Duration, Instant};

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
    token: Option<Arc<()>>,
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
            match engine.try_lock() {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn engine() -> crate::SharedEngine {
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        let journal = json!({"inbox":[],"heard":[
            {"call":"W1AW","grid":null,"snrDb":-8,"freqHz":1500.0,"speed":"normal","lastMs":1,"lastHb":true,"lastCq":false,"storedMsgs":0},
            {"call":"K2ABC","grid":null,"snrDb":-12,"freqHz":1000.0,"speed":"slow","lastMs":1,"lastHb":false,"lastCq":true,"storedMsgs":0}
        ],"allcallReplied":[],"nextInboxId":1});
        e.js8_load_journal(&journal.to_string());
        assert_eq!(e.js8_heard().len(), 2);
        e.import_adif("<CALL:4>W1AW<BAND:3>40m<MODE:3>SSB<QSO_DATE:8>20260908<TIME_ON:6>010000<GRIDSQUARE:4>FN31<NAME:3>OLD<COMMENT:3>OLD<EOR>");
        let base = e.log_records()[0].clone();
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
        assert_eq!(e.log_records().len(), 2302);
        Arc::new(std::sync::Mutex::new(e))
    }
    #[test]
    fn complete_log_join_preserves_latest_empty_fields_and_reuses_generation() {
        let engine = engine();
        let mut cache = Cache::default();
        let mut chunks = 0;
        let value = cache
            .read_chunks(&engine, |_| {
                assert!(engine.try_lock().is_ok());
                chunks += 1;
            })
            .unwrap();
        assert_eq!(chunks, 18);
        assert_eq!(value["history"]["W1AW"]["count"], 2);
        assert_eq!(value["history"]["W1AW"]["comment"], "");
        assert_eq!(value["history"]["W1AW"]["grid"], "");
        assert_eq!(value["history"]["W1AW"]["name"], "");
        assert_eq!(value["history"]["K2ABC"]["count"], 0);
        assert!(value["history"]["K2ABC"]["lastUnix"].is_null());
        assert!(!value["plan"].as_array().unwrap().is_empty());
        assert_eq!(
            cache
                .read_chunks(&engine, |_| panic!("unchanged log was rescanned"))
                .unwrap(),
            value
        );
        {
            let mut e = engine.lock().unwrap();
            let index = e
                .log_records()
                .iter()
                .enumerate()
                .max_by_key(|(_, q)| q.when_unix)
                .unwrap()
                .0;
            let mut q = e.log_records()[index].clone();
            q.comment = Some("CURRENT".into());
            assert!(e.update_qso(index, q));
        }
        assert_eq!(
            cache.read(&engine).unwrap()["history"]["W1AW"]["comment"],
            "CURRENT"
        );
    }
    #[test]
    fn refuses_mixed_generation_and_oversized_context_instead_of_false_unworked() {
        let engine = engine();
        let result = Cache::default().read_chunks(&engine, |offset| {
            if offset == 0 {
                let mut e = engine.lock().unwrap();
                let mut q = e.log_records()[0].clone();
                q.comment = Some("CHANGED".into());
                assert!(e.update_qso(0, q));
            }
        });
        assert_eq!(result.unwrap_err(), "applicationBusy");
        {
            let mut e = engine.lock().unwrap();
            let mut q = e.log_records()[0].clone();
            q.comment = Some("x".repeat(1025));
            assert!(e.update_qso(0, q));
        }
        assert_eq!(
            Cache::default().read(&engine).unwrap_err(),
            "applicationTooLarge"
        );
    }
}
