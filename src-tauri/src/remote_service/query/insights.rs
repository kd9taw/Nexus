//! Full-log display summaries. Only short, revision-checked field copies hold
//! the engine lock; award resolution and counting run on the query worker.
//! Nothing here can write contacts, inspect credentials or start a connector.
use super::Collection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, TryLockError};
use std::time::{Duration, Instant};
use tempo_core::logbook::QsoRecord;

const TEXT_BYTES: usize = 256;
const GROUPS: usize = 2048;
const CALLS: usize = 100_000;
const LOG_ROWS: usize = 1_000_000;
// Cumulative copied text, not heap size. This also bounds award accumulator
// input for unusual imported band/grid/IOTA labels; repeated values count too.
const READ_TEXT_BYTES: usize = 32 * 1024 * 1024;
const TIME_CLIP_SECONDS: u64 = 8_640_000_000_000;

// Deliberately excludes names, notes, upload diagnostics and arbitrary ADIF
// extensions. Large irrelevant log fields cannot inflate the transient copy.
struct Row {
    text_bytes: usize,
    call: String,
    band: String,
    mode: String,
    country: Option<String>,
    state: Option<String>,
    grid: Option<String>,
    iota: Option<String>,
    when: u64,
    confirmed: bool,
    award_confirmed: bool,
    card: bool,
    lotw: bool,
    eqsl: bool,
    credited: bool,
    satellite: bool,
}
impl Row {
    fn copy(q: &QsoRecord) -> Result<Self, &'static str> {
        if q.credit_granted.len() > 64
            || q.credit_granted.iter().any(|s| s.len() > TEXT_BYTES)
            || q.prop_mode.as_ref().is_some_and(|s| s.len() > TEXT_BYTES)
        {
            return Err("applicationTooLarge");
        }
        if [&q.call, &q.band, &q.mode]
            .into_iter()
            .chain(
                [
                    q.country.as_ref(),
                    q.state.as_ref(),
                    q.grid.as_ref(),
                    q.ota.iota.as_ref(),
                ]
                .into_iter()
                .flatten(),
            )
            .any(|s| s.len() > TEXT_BYTES)
            || q.when_unix > 9_007_199_254_740_991
        {
            return Err("applicationTooLarge");
        }
        Ok(Self {
            text_bytes: [&q.call, &q.band, &q.mode]
                .into_iter()
                .chain(
                    [
                        q.country.as_ref(),
                        q.state.as_ref(),
                        q.grid.as_ref(),
                        q.ota.iota.as_ref(),
                    ]
                    .into_iter()
                    .flatten(),
                )
                .map(String::len)
                .sum(),
            call: q.call.clone(),
            band: q.band.clone(),
            mode: q.mode.clone(),
            country: q.country.clone(),
            state: q.state.clone(),
            grid: q.grid.clone(),
            iota: q.ota.iota.clone(),
            when: q.when_unix,
            confirmed: q.confirmed,
            award_confirmed: q.award_confirmed,
            card: q.qsl_rcvd.card,
            lotw: q.qsl_rcvd.lotw,
            eqsl: q.qsl_rcvd.eqsl,
            credited: q.credit_granted.iter().any(|c| c.starts_with("DXCC")),
            satellite: crate::qso_is_sat(q.prop_mode.as_deref()),
        })
    }
    fn award(&self, awards: &mut propagation::Awards) {
        // Same inputs as get_awards: ARRL-grade confirmation, granted DXCC
        // credit, paper-card IOTA and distinct satellite credit.
        awards.add_qso(
            &self.call,
            &self.band,
            &self.mode,
            self.award_confirmed,
            self.credited,
            self.card,
            self.state.as_deref(),
            self.grid.as_deref(),
            self.iota.as_deref(),
            self.satellite,
        );
    }
}

fn tally(map: &mut BTreeMap<String, usize>, value: &str) -> Result<(), &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if !map.contains_key(value) && map.len() >= GROUPS {
        return Err("applicationTooLarge");
    }
    *map.entry(value.to_owned()).or_default() += 1;
    Ok(())
}
fn tallies(map: BTreeMap<String, usize>) -> Value {
    json!(map
        .into_iter()
        .map(|(label, count)| json!({ "label": label, "count": count }))
        .collect::<Vec<_>>())
}
#[derive(Default)]
struct Statistics {
    total: usize,
    calls: BTreeSet<String>,
    entities: BTreeMap<String, (String, usize)>,
    bands: BTreeMap<String, usize>,
    modes: BTreeMap<String, usize>,
    years: BTreeMap<String, usize>,
    states: BTreeMap<String, usize>,
    hours: [usize; 24],
    hour_unknown: usize,
    confirmed: usize,
    award_confirmed: usize,
    card: usize,
    lotw: usize,
    eqsl: usize,
}
impl Statistics {
    fn append(&mut self, q: &Row) -> Result<(), &'static str> {
        self.total += 1;
        let call = q.call.trim().to_uppercase();
        if !self.calls.contains(&call) && self.calls.len() >= CALLS {
            return Err("applicationTooLarge");
        }
        self.calls.insert(call);
        let entity = propagation::dxcc::resolve(&q.call)
            .map(|i| i.entity)
            .or(q.country.as_deref());
        if let Some(entity) = entity.map(str::trim).filter(|v| !v.is_empty()) {
            let key = entity.to_uppercase();
            if !self.entities.contains_key(&key) && self.entities.len() >= GROUPS {
                return Err("applicationTooLarge");
            }
            self.entities
                .entry(key)
                .or_insert_with(|| (entity.to_owned(), 0))
                .1 += 1;
        }
        tally(&mut self.bands, &q.band)?;
        tally(&mut self.modes, &q.mode)?;
        // Existing UI Statistics gates WAS by the stored country, separately
        // from the resolved-entity headline. Do not silently change that rule.
        if matches!(
            q.country
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_uppercase()
                .as_str(),
            "UNITED STATES" | "ALASKA" | "HAWAII"
        ) {
            let state = q.state.as_deref().unwrap_or("").trim().to_uppercase();
            if propagation::awards::valid_state(&state).is_some() {
                tally(&mut self.states, &state)?;
            }
        }
        if q.when.is_multiple_of(86_400) {
            self.hour_unknown += 1;
        } else if q.when <= TIME_CLIP_SECONDS {
            self.hours[((q.when % 86_400) / 3600) as usize] += 1;
        }
        if q.when <= TIME_CLIP_SECONDS {
            tally(
                &mut self.years,
                &tempo_core::logbook::datetime_utc(q.when).0.to_string(),
            )?;
        }
        self.confirmed += usize::from(q.confirmed);
        self.award_confirmed += usize::from(q.award_confirmed);
        self.card += usize::from(q.card);
        self.lotw += usize::from(q.lotw);
        self.eqsl += usize::from(q.eqsl);
        Ok(())
    }
    fn finish(self) -> Value {
        // The browser sorts labels with its existing localeCompare comparator,
        // then takes the top twelve entities. Rust lexical order cannot choose
        // the same tie slice for accented/legacy country labels.
        json!({ "total": self.total, "uniqueCalls": self.calls.len(), "dxccEntities": self.entities.len(),
            "confirmed": self.confirmed, "awardConfirmed": self.award_confirmed,
            "byBand": tallies(self.bands), "byMode": tallies(self.modes), "byYear": tallies(self.years),
            "byState": tallies(self.states), "topEntities": self.entities.into_values().map(|(label,count)| json!({ "label": label, "count": count })).collect::<Vec<_>>(),
            "hourUtc": self.hours, "hourUnknown": self.hour_unknown,
            "qsl": { "card": self.card, "lotw": self.lotw, "eqsl": self.eqsl } })
    }
}

pub(super) fn read_engine(
    engine: &crate::SharedEngine,
    collection: Collection,
) -> Result<Value, &'static str> {
    read_chunks(engine, collection, |_| {})
}
fn read_chunks(
    engine: &crate::SharedEngine,
    collection: Collection,
    mut after_chunk: impl FnMut(usize),
) -> Result<Value, &'static str> {
    if !matches!(collection, Collection::Awards | Collection::Statistics) {
        return Err("applicationUnsupported");
    }
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
    let (token, count, my_call) = {
        let e = lock()?;
        if e.log_records().len() > LOG_ROWS || e.settings().mycall.len() > TEXT_BYTES {
            return Err("applicationTooLarge");
        }
        (
            e.log_read_token(),
            e.log_records().len(),
            e.settings().mycall.clone(),
        )
    };
    let mut awards = propagation::Awards::new();
    awards.set_home_call(&my_call);
    let mut stats = Statistics::default();
    let mut geo = propagation::stats::LogStatsAccumulator::new(&my_call);
    let mut text_bytes = 0;
    for offset in (0..count).step_by(128) {
        let rows = {
            let e = lock()?;
            if !Arc::ptr_eq(&token, &e.log_read_token()) || e.settings().mycall != my_call {
                return Err("applicationBusy");
            }
            e.log_records()[offset..(offset + 128).min(count)]
                .iter()
                .map(Row::copy)
                .collect::<Result<Vec<_>, _>>()?
        };
        for row in rows {
            text_bytes += row.text_bytes;
            if text_bytes > READ_TEXT_BYTES {
                return Err("applicationTooLarge");
            }
            if collection == Collection::Awards {
                row.award(&mut awards)
            } else {
                stats.append(&row)?;
                geo.add(&row.call)
            }
        }
        after_chunk(offset);
    }
    {
        let e = lock()?;
        if !Arc::ptr_eq(&token, &e.log_read_token()) || e.settings().mycall != my_call {
            return Err("applicationBusy");
        }
    }
    let result = if collection == Collection::Awards {
        json!({ "logCount": count, "awards": awards.summary() })
    } else {
        json!({ "logCount": count, "statistics": stats.finish(), "geography": geo.summary() })
    };
    if Instant::now() >= deadline {
        return Err("applicationBusy");
    }
    // One indivisible result: no truncation can masquerade as full-log totals.
    if result.to_string().len() > super::PAGE_BYTES / 2 {
        return Err("applicationTooLarge");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn engine(count: usize) -> crate::SharedEngine {
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        let adif: String = (0..count).map(|i| {
            let call = if i == 0 { "JA1ABC".into() } else { format!("K1T{i}") };
            format!("<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260909<TIME_ON:6>120000<LOTW_QSL_RCVD:1>Y<EOR>\n",call.len())
        }).collect();
        e.import_adif(&adif);
        assert_eq!(e.log_records().len(), count);
        Arc::new(Mutex::new(e))
    }
    #[test]
    fn full_log_summaries_match_native_awards_and_geography_beyond_the_display_window() {
        let engine = engine(2301);
        let (log, settings, snapshot) = {
            let e = engine.lock().unwrap();
            (e.log_records().to_vec(), e.settings().clone(), e.snapshot())
        };
        let mut chunks = 0;
        let awards = read_chunks(&engine, Collection::Awards, |_| {
            assert!(
                engine.try_lock().is_ok(),
                "the read must release the engine between chunks"
            );
            chunks += 1;
        })
        .unwrap();
        assert_eq!(chunks, 18);
        assert_eq!(awards["logCount"], 2301);
        assert_eq!(
            awards["awards"],
            serde_json::to_value(crate::awards_for_records(&log, &settings.mycall)).unwrap()
        );
        let stats = read_engine(&engine, Collection::Statistics).unwrap();
        assert_eq!(stats["statistics"]["total"], 2301);
        assert_eq!(stats["statistics"]["uniqueCalls"], 2301);
        assert_eq!(stats["statistics"]["confirmed"], 2301);
        assert_eq!(stats["statistics"]["hourUtc"][12], 2301);
        assert_eq!(
            stats["geography"],
            serde_json::to_value(propagation::compute_log_stats(
                &log.iter().map(|q| &q.call).collect::<Vec<_>>(),
                &settings.mycall
            ))
            .unwrap()
        );
        assert!(stats.to_string().len() < 16_384);
        let e = engine.lock().unwrap();
        assert_eq!(e.log_records(), log);
        assert_eq!(e.settings(), &settings);
        assert_eq!(e.snapshot().radio.tx_enabled, snapshot.radio.tx_enabled);
    }
    #[test]
    fn both_summaries_refuse_same_length_edits_and_changed_home_identity() {
        for kind in [Collection::Awards, Collection::Statistics] {
            let engine = engine(270);
            let changed = read_chunks(&engine, kind, |offset| {
                if offset == 0 {
                    let mut e = engine.try_lock().unwrap();
                    let mut q = e.log_records()[0].clone();
                    q.notes = Some("edited while reading".into());
                    assert!(e.update_qso(0, q));
                    assert_eq!(e.log_records().len(), 270);
                }
            });
            assert!(matches!(changed, Err("applicationBusy")));
            let changed = read_chunks(&engine, kind, |offset| {
                if offset == 0 {
                    let mut e = engine.try_lock().unwrap();
                    let mut settings = e.settings().clone();
                    settings.mycall = "JA1ABC".into();
                    e.apply_settings(settings);
                }
            });
            assert!(matches!(changed, Err("applicationBusy")));
            assert_eq!(read_engine(&engine, kind).unwrap()["logCount"], 270);
        }
    }
    #[test]
    fn irrelevant_large_notes_do_not_enter_the_read_and_oversized_labels_are_refused() {
        let engine = engine(1);
        {
            let mut e = engine.lock().unwrap();
            let mut q = e.log_records()[0].clone();
            q.notes = Some("contact note ".repeat(100_000));
            q.comment = Some("private comment".into());
            assert!(e.update_qso(0, q));
        }
        for kind in [Collection::Awards, Collection::Statistics] {
            let read = read_engine(&engine, kind).unwrap().to_string();
            assert!(!read.contains("contact note") && !read.contains("private comment"));
        }
        {
            let mut e = engine.lock().unwrap();
            let mut q = e.log_records()[0].clone();
            q.country = Some("X".repeat(TEXT_BYTES + 1));
            assert!(e.update_qso(0, q));
        }
        assert!(matches!(
            read_engine(&engine, Collection::Statistics),
            Err("applicationTooLarge")
        ));
        assert!(matches!(
            read_engine(&engine, Collection::Log),
            Err("applicationUnsupported")
        ));
        // These are inspected under the engine lock, even though only boolean
        // credit/satellite flags leave it. Bound the inspection as well as copies.
        let original = engine.lock().unwrap().log_records()[0].clone();
        for satellite in [false, true] {
            let mut q = original.clone();
            q.country = None;
            if satellite {
                q.prop_mode = Some("SAT ".repeat(TEXT_BYTES));
            } else {
                q.credit_granted = vec!["DXCC".into(); 65];
            }
            assert!(engine.lock().unwrap().update_qso(0, q));
            assert!(matches!(
                read_engine(&engine, Collection::Awards),
                Err("applicationTooLarge")
            ));
        }
    }
    #[test]
    fn busy_engine_returns_a_bounded_refusal_then_recovers() {
        let engine = engine(1);
        let held = engine.lock().unwrap();
        let start = Instant::now();
        assert!(matches!(
            read_engine(&engine, Collection::Awards),
            Err("applicationBusy")
        ));
        assert!(start.elapsed() < Duration::from_secs(3));
        drop(held);
        assert_eq!(
            read_engine(&engine, Collection::Awards).unwrap()["logCount"],
            1
        );
    }
}
