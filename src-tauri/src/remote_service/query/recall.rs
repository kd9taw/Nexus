//! Exact-call recall and entity context from the whole station log, read from one picture of
//! it off the engine lock (see `picture`).
//! Read-only: no disk reconciliation, connector lookup, vault or transmit access.
use super::picture::{Pick, Picture};
use serde_json::{json, Value};
use tempo_core::logbook::sqlite::Narrow;
use tempo_core::logbook::QsoRecord;

const ROWS: usize = 20;
const DISTINCT: usize = 512;

/// What recall reads of every contact: the call it matches on; the band, mode, frequency, time,
/// confirmation and note it counts and lists of a match; and the stored country an unresolvable
/// call's entity falls back to. The rows it lists are then read whole.
const RECALL: Narrow = Narrow {
    columns: &[
        "call",
        "band",
        "mode",
        "freq_mhz",
        "when_unix",
        "country",
        "notes",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
    ],
    uploads: false,
};

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
    selected: Vec<(u64, usize, Pick)>,
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
    fn append(&mut self, pick: Pick, q: &QsoRecord) -> Result<(), &'static str> {
        let index = pick.at;
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
            if self.selected.len() < ROWS || self.selected.last().is_some_and(|r| q.when_unix > r.0)
            {
                self.selected.push((q.when_unix, index, pick));
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
            return Ok(());
        }
        let resolved = propagation::dxcc::resolve(&q.call);
        let key = resolved
            .as_ref()
            .map(|i| i.entity)
            .or(q.country.as_deref())
            .unwrap_or("");
        if key.trim().to_uppercase() != self.entity_key {
            return Ok(());
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
        Ok(())
    }
    /// The capture, with the listed contacts read whole from the picture the pass read.
    fn finish(self, log: &Picture<'_>) -> Result<Capture, &'static str> {
        let picks: Vec<Pick> = self.selected.iter().map(|(_, _, pick)| *pick).collect();
        Ok(Capture {
            rows: log.whole(&picks)?,
            total: self.total,
            meta: json!({ "call": self.call, "entity": self.entity,
                "history": { "count": self.total, "workedBefore": self.total > 0, "lastUnix": (self.total > 0).then_some(self.last),
                    "confirmedCount": self.confirmed, "bands": self.bands, "modes": self.modes },
                "workedBandModes": self.band_modes, "latestNote": self.latest_note.map(|(_, s)| s),
                "slots": { "workedEver": self.worked, "bandUnknown": self.unknown, "bandsWorked": self.entity_bands, "modesWorked": self.entity_modes } }),
        })
    }
}

/// Recall of `call` over one picture of the log: every contact tested, the listed ones read
/// whole from the same picture — within the read's budget (see `picture::within`).
fn recall(
    log: &Picture<'_>,
    call: &str,
    deadline: std::time::Instant,
) -> Result<Capture, &'static str> {
    let mut result = Accumulator::new(call);
    log.each(RECALL, &mut |pick, q| {
        super::picture::within(deadline, pick)?;
        result.append(pick, q)
    })?;
    if std::time::Instant::now() >= deadline {
        return Err("applicationBusy");
    }
    result.finish(log)
}

pub(super) fn read_engine(
    engine: &crate::SharedEngine,
    call: &str,
) -> Result<Capture, &'static str> {
    use std::sync::TryLockError;
    use std::time::{Duration, Instant};
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
    // The log's handles under the lock. DXCC resolution and summary work cannot hold the
    // engine mutex, and one picture of the log means no mixed log may be reported as complete
    // or used to claim a new entity.
    let rows = lock()?.log_rows();
    super::picture::read(&rows, |log| recall(log, call, deadline))
}
#[cfg(test)]
fn read(records: &[QsoRecord], call: &str) -> Result<Capture, &'static str> {
    let rows: Vec<_> = records.iter().cloned().map(std::sync::Arc::new).collect();
    let unbounded = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    recall(&Picture::Memory(&rows), call, unbounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_service::stored_log_tests::StoredLog;
    fn record(call: &str, when: u64, band: &str, mode: &str) -> QsoRecord {
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        e.import_adif(&format!("<CALL:{}>{call}<BAND:{}>{band}<MODE:{}>{mode}<QSO_DATE:8>20260909<TIME_ON:6>010000<EOR>", call.len(), band.len(), mode.len()));
        let mut q = e.stored_log()[0].as_ref().clone();
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
    /// The pass holds no Engine lock, and an edit landing while it runs mixes into no part of
    /// the answer — the counts or the rows read whole after the pass — nor refuses it: the answer
    /// is the log as the read found it, and the next read has the edit.
    #[test]
    fn the_read_releases_the_engine_and_an_edit_during_it_belongs_to_the_next_read() {
        use super::super::picture::{at_seams, Seam};
        let mut e = tempo_app::engine::Engine::with_settings(Default::default());
        // The 1.13 path: since SPEC-2 v3 C19 (D1-A) its log is in a store in memory, and the
        // read's picture is its read transaction from its first statement on — the pass — so the
        // edit lands after the pass, before the rows it picked are read whole.
        let d = super::super::log_tests::Dir::new("edit-during-recall");
        e.set_log_path(d.memory_log());
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
        assert_eq!(e.stored_log().len(), 270);
        let engine = std::sync::Arc::new(std::sync::Mutex::new(e));
        let (hook, edits) = (engine.clone(), std::rc::Rc::new(std::cell::Cell::new(0)));
        let counted = edits.clone();
        let result = at_seams(
            move |seam| {
                if seam != Seam::Whole {
                    return;
                }
                let mut e = hook
                    .try_lock()
                    .expect("summary work has released station authority");
                let index = e
                    .stored_log()
                    .iter()
                    .position(|q| q.call == "W1AW")
                    .unwrap();
                let mut changed = e.stored_log()[index].as_ref().clone();
                changed.notes = Some("edited during recall".into());
                assert!(e.update_qso(changed.id.unwrap(), changed));
                counted.set(counted.get() + 1);
            },
            || read_engine(&engine, "W1AW"),
        )
        .unwrap()
        .encode()
        .unwrap();
        assert_eq!(
            edits.get(),
            1,
            "premise: the edit landed while the read ran"
        );
        // Counted once the read has ended: the store in memory holds the edit's commit off until
        // then.
        assert_eq!(engine.lock().unwrap().stored_log().len(), 270);
        assert_eq!(result.1, 1);
        assert_eq!(
            result.2["latestNote"],
            Value::Null,
            "the read's own picture: the log before the edit"
        );
        assert_eq!(result.0[0]["notes"], Value::Null, "and its row, read whole");
        let fresh = read_engine(&engine, "W1AW").unwrap().encode().unwrap();
        assert_eq!(fresh.2["latestNote"], "edited during recall");
        assert_eq!(fresh.0[0]["notes"], "edited during recall");
    }

    // ── recall from the store, held to the code before C18 ───────────────────────────────
    //
    // SPEC-2 v3 C18: recall reads one picture of the logbook store now. The oracle is the code
    // before C18, VERBATIM — its accumulator, which held whole records a chunk at a time, and
    // its chunked read of the log in memory — beside the store that log mirrors.

    struct OldAccumulator {
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
    impl OldAccumulator {
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
        fn append<R: std::borrow::Borrow<QsoRecord>>(
            &mut self,
            records: &[R],
            offset: usize,
        ) -> Result<(), &'static str> {
            for (i, q) in records.iter().enumerate() {
                let q: &QsoRecord = std::borrow::Borrow::borrow(q);
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

    fn old_read_chunks(
        engine: &crate::SharedEngine,
        call: &str,
        mut after_chunk: impl FnMut(usize),
    ) -> Result<Capture, &'static str> {
        // The log as the store holds it ([`StoredLog`]), in place of the copy in memory: these
        // tests hold the old algorithm against the new reader over the same rows, and whether the
        // store holds what the old write path wrote (P6) is the Stage-1 lockstep suite's job. One
        // picture, so the old read's log token has nothing left to check.
        let log = engine
            .lock()
            .map_err(|_| "applicationUnavailable")?
            .stored_log();
        let count = log.len();
        let mut result = OldAccumulator::new(call);
        for offset in (0..count).step_by(128) {
            let rows = log[offset..(offset + 128).min(count)].to_vec();
            // DXCC resolution and summary work cannot hold the engine mutex. The
            // token check refuses even same-length edits between chunks; no mixed
            // log may be reported as complete or used to claim a new entity.
            result.append(&rows, offset)?;
            after_chunk(offset);
        }
        Ok(result.finish())
    }

    use super::super::log_tests::{launch, memory, settle, synthetic_log, Dir, Gen};

    /// What a browser may recall (`[A-Z0-9/]{3,32}`): calls worked often and once, portable
    /// and compound spellings, calls a Unicode upper-case folds INTO (`K1ſAB` → `K1SAB`,
    /// `K1ıAB` → `K1IAB`) — which the store's ASCII-only `call_norm` would miss — a call of an
    /// entity with many others in the log, one no table resolves, and one never logged.
    const RECALLS: &[&str] = &[
        "W1AW",
        "K1ABC",
        "K1ABC/P",
        "VP2E/K1ABC",
        "DL1ABC",
        "JA1AA",
        "K1SAB",
        "K1IAB",
        "Q0ZZZ",
        "DL9ZZZ",
        "ZZ9ZZZ",
    ];

    fn bytes(capture: Result<Capture, &'static str>) -> String {
        match capture.and_then(Capture::encode) {
            Ok((rows, total, meta)) => {
                json!({ "rows": rows, "total": total, "meta": meta }).to_string()
            }
            Err(e) => format!("refused: {e}"),
        }
    }

    fn assert_recall_is_the_old_recall(e: &crate::SharedEngine, calls: &[&str], what: &str) {
        for &call in calls {
            let (old, new) = (
                bytes(old_read_chunks(e, call, |_| {})),
                bytes(read_engine(e, call)),
            );
            assert!(
                new == old,
                "{what}: recall of {call} differs\nstore:  {new:.400}\nmemory: {old:.400}"
            );
        }
    }

    /// ★ PARITY: recall of every call above, over 3,000 contacts, read from the store and from
    /// the 1.13 path, is byte for byte the recall the log in memory gave — its counts, its
    /// twenty rows read whole, the latest note, the entity's slots.
    #[test]
    fn recall_read_from_the_store_is_the_recall_of_the_log_in_memory() {
        let text = synthetic_log(3_000, 0x0C18_A2EC);
        let d = Dir::new("recall");
        std::fs::write(d.log(), &text).unwrap();
        let store = launch(&d);
        let w1aw = read_engine(&store, "W1AW").unwrap().encode().unwrap();
        assert!(
            w1aw.1 > 20 && w1aw.0.len() == 20,
            "premise: more W1AW contacts than rows"
        );
        assert!(
            w1aw.2["latestNote"].is_string(),
            "premise: a note to recall"
        );
        let (_, sab, _) = read_engine(&store, "K1SAB").unwrap().encode().unwrap();
        assert!(
            sab > 0,
            "premise: a call outside ASCII recalled by its Unicode upper case"
        );
        assert!(
            w1aw.0
                .iter()
                .any(|r| !r["extra"].as_array().unwrap().is_empty()),
            "premise: rows carry what only a whole record holds"
        );
        assert_recall_is_the_old_recall(&store, RECALLS, "the store");
        assert_recall_is_the_old_recall(&memory(&d, &text), RECALLS, "the 1.13 path");
        settle(&store);
    }

    /// ★ THE PROPERTY: after every one of 24 random changes to each of eight seeded logs, recall
    /// read from the store is the recall of the log in memory.
    #[test]
    fn after_every_change_recall_read_from_the_store_is_the_old_recall() {
        for seed in 1..=8u64 {
            let d = Dir::new(&format!("recall-prop-{seed}"));
            std::fs::write(d.log(), synthetic_log(200, seed * 104_729)).unwrap();
            let e = launch(&d);
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            for step in 0..24u64 {
                super::super::log_tests::random_change(&e, &mut g, step);
                let at = (step as usize * 3) % RECALLS.len();
                let calls: Vec<&str> = RECALLS.iter().cycle().skip(at).take(3).copied().collect();
                assert_recall_is_the_old_recall(&e, &calls, &format!("seed {seed}, step {step}"));
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
        let d = super::super::log_tests::Dir::new("budget");
        let engine = super::super::log_tests::memory(
            &d,
            &super::super::log_tests::synthetic_log(300, 0xB0D6),
        );
        let late = at_seams(
            |seam| {
                if seam == Seam::Each {
                    std::thread::sleep(std::time::Duration::from_millis(2_050));
                }
            },
            || read_engine(&engine, "W1AW"),
        );
        assert!(matches!(late, Err("applicationBusy")));
        assert!(
            read_engine(&engine, "W1AW").is_ok(),
            "control: on time, it answers"
        );
    }
}
