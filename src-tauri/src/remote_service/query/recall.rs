//! Exact-call recall and entity context from the full in-memory station log.
//! Read-only: no disk reconciliation, connector lookup, vault or transmit access.
use serde_json::{json, Value};
use tempo_core::logbook::QsoRecord;

const ROWS: usize = 20;
const DISTINCT: usize = 512;

pub(super) struct Capture {
    rows: Vec<QsoRecord>,
    total: usize,
    meta: Value,
}
impl Capture {
    pub(super) fn encode(self) -> Result<(Vec<Value>, usize, Value), &'static str> {
        let rows = self
            .rows
            .into_iter()
            .map(|r| {
                let mut q = tempo_app::dto::LoggedQso::from(r);
                q.entity = propagation::dxcc::resolve(&q.call).map(|i| i.entity.to_string());
                serde_json::to_value(q).map_err(|_| "applicationUnavailable")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, self.total, self.meta))
    }
}
fn distinct<T: PartialEq>(values: &mut Vec<T>, value: T) -> Result<(), &'static str> {
    if !values.contains(&value) {
        if values.len() >= DISTINCT {
            return Err("applicationTooLarge");
        }
        values.push(value);
    }
    Ok(())
}
fn text(value: &str) -> Result<(), &'static str> {
    if value.len() > 128 {
        Err("applicationTooLarge")
    } else {
        Ok(())
    }
}

// These are display identities, including the inclusive endpoints used by
// ui/src/band.ts. Never substitute band_for_dial's CAT/privilege ranges here:
// those deliberately differ at endpoints and on 60/17 m. Conformance tests
// compare the actual native output with the existing UI recall functions.
const BANDS: &[(f64, f64, &str)] = &[
    (1.8, 2.0, "160m"),
    (3.5, 4.0, "80m"),
    (5.3, 5.41, "60m"),
    (7.0, 7.3, "40m"),
    (10.1, 10.15, "30m"),
    (14.0, 14.35, "20m"),
    (18.068, 18.168, "17m"),
    (21.0, 21.45, "15m"),
    (24.89, 24.99, "12m"),
    (28.0, 29.7, "10m"),
    (50.0, 54.0, "6m"),
    (70.0, 71.0, "4m"),
    (144.0, 148.0, "2m"),
    (222.0, 225.0, "1.25m"),
    (420.0, 450.0, "70cm"),
    (902.0, 928.0, "33cm"),
    (1240.0, 1300.0, "23cm"),
    (2300.0, 2450.0, "13cm"),
    (3300.0, 3500.0, "9cm"),
    (5650.0, 5925.0, "6cm"),
    (10000.0, 10500.0, "3cm"),
    (24000.0, 24250.0, "1.25cm"),
];
fn band(q: &QsoRecord) -> Option<String> {
    let token = q.band.trim().to_lowercase();
    BANDS
        .iter()
        .find(|(_, _, b)| *b == token)
        .or_else(|| {
            BANDS
                .iter()
                .find(|(lo, hi, _)| q.freq_mhz >= *lo && q.freq_mhz <= *hi)
        })
        .map(|(_, _, b)| b.to_uppercase())
}
fn mode(value: &str) -> String {
    let value = value.trim().to_uppercase();
    match value.as_str() {
        "USB" | "LSB" => "SSB".into(),
        "BPSK31" => "PSK31".into(),
        "BPSK63" => "PSK63".into(),
        _ => value,
    }
}

struct Accumulator {
    call: String,
    entity: Option<String>,
    entity_key: String,
    selected: Vec<(u64, usize, QsoRecord)>,
    total: usize,
    confirmed: usize,
    last: u64,
    bands: Vec<String>,
    modes: Vec<String>,
    band_modes: Vec<(String, String)>,
    worked: bool,
    unknown: bool,
    entity_bands: Vec<String>,
    entity_modes: Vec<String>,
    latest_note: Option<(u64, String)>,
}
impl Accumulator {
    fn new(call: &str) -> Self {
        let entity = propagation::dxcc::resolve(call).map(|i| i.entity.to_string());
        let entity_key = entity.as_deref().unwrap_or("").trim().to_uppercase();
        Self {
            call: call.into(),
            entity,
            entity_key,
            selected: Vec::new(),
            total: 0,
            confirmed: 0,
            last: 0,
            bands: Vec::new(),
            modes: Vec::new(),
            band_modes: Vec::new(),
            worked: false,
            unknown: false,
            entity_bands: Vec::new(),
            entity_modes: Vec::new(),
            latest_note: None,
        }
    }
    fn append(&mut self, records: &[QsoRecord], offset: usize) -> Result<(), &'static str> {
        for (i, q) in records.iter().enumerate() {
            let index = offset + i;
            if q.call.trim().to_uppercase() == self.call {
                text(&q.band)?;
                text(&q.mode)?;
                if q.when_unix > 9_007_199_254_740_991 {
                    return Err("applicationTooLarge");
                }
                self.total += 1;
                self.confirmed += usize::from(q.confirmed);
                self.last = self.last.max(q.when_unix);
                if !q.band.is_empty() {
                    distinct(&mut self.bands, q.band.clone())?;
                }
                if !q.mode.is_empty() {
                    distinct(&mut self.modes, q.mode.clone())?;
                }
                distinct(
                    &mut self.band_modes,
                    (q.band.trim().to_lowercase(), q.mode.trim().to_uppercase()),
                )?;
                if self.selected.len() < ROWS
                    || self.selected.last().is_some_and(|r| q.when_unix > r.0)
                {
                    self.selected.push((q.when_unix, index, q.clone()));
                    self.selected
                        .sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                    self.selected.truncate(ROWS);
                }
                if let Some(note) = q.notes.as_deref().filter(|s| !s.trim().is_empty()) {
                    if self
                        .latest_note
                        .as_ref()
                        .is_none_or(|(when, _)| q.when_unix > *when)
                    {
                        if note.len() > 65536 {
                            return Err("applicationTooLarge");
                        }
                        self.latest_note = Some((q.when_unix, note.into()));
                    }
                }
            }
            if self.entity_key.is_empty() {
                continue;
            }
            let resolved = propagation::dxcc::resolve(&q.call);
            let key = resolved
                .as_ref()
                .map(|i| i.entity)
                .or(q.country.as_deref())
                .unwrap_or("");
            if key.trim().to_uppercase() != self.entity_key {
                continue;
            }
            text(&q.band)?;
            text(&q.mode)?;
            self.worked = true;
            if let Some(b) = band(q) {
                distinct(&mut self.entity_bands, b)?;
            } else if !q.band.trim().is_empty() || q.freq_mhz > 0.0 {
                self.unknown = true;
            }
            let m = mode(&q.mode);
            if !m.is_empty() {
                distinct(&mut self.entity_modes, m)?;
            }
        }
        Ok(())
    }
    fn finish(self) -> Capture {
        Capture {
            rows: self.selected.into_iter().map(|(_, _, q)| q).collect(),
            total: self.total,
            meta: json!({ "call": self.call, "entity": self.entity,
                "history": { "count": self.total, "workedBefore": self.total > 0, "lastUnix": (self.total > 0).then_some(self.last),
                    "confirmedCount": self.confirmed, "bands": self.bands, "modes": self.modes },
                "workedBandModes": self.band_modes, "latestNote": self.latest_note.map(|(_, s)| s),
                "slots": { "workedEver": self.worked, "bandUnknown": self.unknown, "bandsWorked": self.entity_bands, "modesWorked": self.entity_modes } }),
        }
    }
}

pub(super) fn read_engine(
    engine: &crate::SharedEngine,
    call: &str,
) -> Result<Capture, &'static str> {
    read_chunks(engine, call, |_| {})
}
fn read_chunks(
    engine: &crate::SharedEngine,
    call: &str,
    mut after_chunk: impl FnMut(usize),
) -> Result<Capture, &'static str> {
    use std::sync::{Arc, TryLockError};
    use std::time::{Duration, Instant};
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
    let (token, count) = {
        let e = lock()?;
        (e.log_read_token(), e.log_records().len())
    };
    let mut result = Accumulator::new(call);
    for offset in (0..count).step_by(128) {
        let rows = {
            let e = lock()?;
            if !Arc::ptr_eq(&token, &e.log_read_token()) {
                return Err("applicationBusy");
            }
            e.log_records()[offset..(offset + 128).min(count)].to_vec()
        };
        // DXCC resolution and summary work cannot hold the engine mutex. The
        // token check refuses even same-length edits between chunks; no mixed
        // log may be reported as complete or used to claim a new entity.
        result.append(&rows, offset)?;
        after_chunk(offset);
    }
    if !Arc::ptr_eq(&token, &lock()?.log_read_token()) {
        return Err("applicationBusy");
    }
    Ok(result.finish())
}
#[cfg(test)]
fn read(records: &[QsoRecord], call: &str) -> Result<Capture, &'static str> {
    let mut result = Accumulator::new(call);
    result.append(records, 0)?;
    Ok(result.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(call: &str, when: u64, band: &str, mode: &str) -> QsoRecord {
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        e.import_adif(&format!("<CALL:{}>{call}<BAND:{}>{band}<MODE:{}>{mode}<QSO_DATE:8>20260909<TIME_ON:6>010000<EOR>", call.len(), band.len(), mode.len()));
        let mut q = e.log_records()[0].clone();
        q.when_unix = when;
        q
    }
    #[test]
    fn recall_counts_the_whole_log_and_keeps_only_recent_exact_call_rows() {
        let base = record("W1AW", 0, "40m", "FT8");
        let mut log: Vec<_> = (0..2300)
            .map(|i| {
                let mut q = base.clone();
                q.when_unix = i;
                q
            })
            .collect();
        log[0].confirmed = true;
        log[0].notes = Some("older note beyond the display window".into());
        log.push(record("W1AW/P", 5000, "20m", "SSB"));
        let before = log.clone();
        let (rows, total, meta) = read(&log, "W1AW").unwrap().encode().unwrap();
        assert_eq!(total, 2300);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[0]["whenUnix"], 2299);
        assert_eq!(meta["history"]["confirmedCount"], 1);
        assert_eq!(meta["latestNote"], "older note beyond the display window");
        assert_eq!(meta["workedBandModes"], json!([["40m", "FT8"]]));
        assert!(
            meta["slots"]["bandsWorked"]
                .as_array()
                .unwrap()
                .contains(&json!("20M")),
            "other calls in the entity count toward entity slots"
        );
        assert_eq!(
            log, before,
            "recall cannot mutate contacts or connector outcomes"
        );
    }
    #[test]
    fn entity_truth_is_resolved_and_unknown_bands_never_claim_new_slots() {
        let mut q = record("DL1ABC", 1, "odd-import", "LSB");
        q.country = Some("Germany".into());
        q.freq_mhz = 0.0;
        let (rows, total, meta) = read(&[q.clone()], "DL2ABC").unwrap().encode().unwrap();
        assert!(rows.is_empty());
        assert_eq!(total, 0);
        assert_eq!(meta["entity"], "Fed. Rep. of Germany");
        assert_eq!(meta["slots"]["workedEver"], true);
        assert_eq!(meta["slots"]["bandUnknown"], true);
        assert_eq!(meta["slots"]["modesWorked"], json!(["SSB"]));
        q.freq_mhz = 7.142;
        let (_, _, meta) = read(&[q], "DL2ABC").unwrap().encode().unwrap();
        assert_eq!(meta["slots"]["bandUnknown"], false);
        assert_eq!(meta["slots"]["bandsWorked"], json!(["40M"]));
        let (_, _, meta) = read(&[], "000").unwrap().encode().unwrap();
        assert_eq!(meta["entity"], Value::Null);
        assert_eq!(meta["history"]["workedBefore"], false);
    }
    #[test]
    fn large_log_recall_stays_bounded_and_equal_timestamps_keep_desktop_order() {
        let base = record("W1AW", 100, "20m", "FT8");
        let log: Vec<_> = (0..100_000)
            .map(|i| {
                let mut q = base.clone();
                q.call = if i < 30 {
                    "W1AW".into()
                } else {
                    format!("K1T{i}")
                };
                q.comment = Some(i.to_string());
                q
            })
            .collect();
        let started = std::time::Instant::now();
        let capture = read(&log, "W1AW").unwrap();
        eprintln!(
            "recall 100000-record immutable scan: {:?}",
            started.elapsed()
        );
        let (rows, total, _) = capture.encode().unwrap();
        assert_eq!(total, 30);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[0]["comment"], "0");
        assert_eq!(rows[19]["comment"], "19");
    }
    #[test]
    fn chunks_release_the_engine_and_refuse_a_same_length_log_edit() {
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        let adif: String = (0..270)
            .map(|i| {
                let call = if i == 0 {
                    "W1AW".into()
                } else {
                    format!("K1T{i}")
                };
                tempo_core::logbook::adif_record(&record(&call, i, "20m", "FT8"))
            })
            .collect();
        e.import_adif(&adif);
        assert_eq!(e.log_records().len(), 270);
        let engine = std::sync::Arc::new(std::sync::Mutex::new(e));
        let mut chunks = 0;
        let result = read_chunks(&engine, "W1AW", |_| {
            assert!(
                engine.try_lock().is_ok(),
                "summary work has released station authority"
            );
            chunks += 1;
        })
        .unwrap()
        .encode()
        .unwrap();
        assert_eq!(chunks, 3);
        assert_eq!(result.1, 1);
        let changed = read_chunks(&engine, "W1AW", |offset| {
            if offset == 0 {
                let mut e = engine.try_lock().unwrap();
                let count = e.log_records().len();
                let index = e
                    .log_records()
                    .iter()
                    .position(|q| q.call == "W1AW")
                    .unwrap();
                let mut changed = e.log_records()[index].clone();
                changed.notes = Some("edited during recall".into());
                assert!(e.update_qso(index, changed));
                assert_eq!(e.log_records().len(), count);
            }
        });
        assert!(
            matches!(changed, Err("applicationBusy")),
            "a mixed log must never claim complete recall truth"
        );
        let fresh = read_engine(&engine, "W1AW").unwrap().encode().unwrap();
        assert_eq!(fresh.2["latestNote"], "edited during recall");
    }
}
