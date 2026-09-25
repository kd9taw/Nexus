//! Stage 1's write path, kept as the oracle for after the cut — SPEC-2 v3 C19 Part D.
//!
//! Until C19 every change to the log is made first to the copy held in memory
//! (`StationCore::logbook`) and then carried to the store as the difference
//! (`StationCore::persist_change`). C19 Part B moves the write path onto the store, and the cut
//! deletes the copy — and with it the oracle every test of the log has had: the store holds what
//! memory holds (P6).
//!
//! [`Stage1`] keeps that oracle. Each of its writes is the part of the `StationCore` write of the
//! same name that changes the log, copied VERBATIM from the code before Part B, over a
//! [`Logbook`] of its own. Left out, because each is somewhere a change is carried and not what
//! the change is: the store and its mirror, the hot index and the watermarks it follows, another
//! window's commits (`recover_external_appends`, `refresh_from_store`), the upload queue, and the
//! reconcile summaries a station keeps for its screens. Four differences, so the copy and a
//! station make the SAME change rather than two a clock or a minter would tell apart: a contact
//! arrives carrying the id the station gave it; a QSL-sent mark takes the date the station gave
//! it; a LoTW batch arrives as the ids and fingerprints TQSL was handed, which a station keeps
//! inside [`LotwSigned`]; and the rows a log mints ids for itself (an import's, a download's)
//! take the ids the station minted for them, since every log mints under a nonce of its own
//! (`Minter`'s `Clone`) and no copy can mint a station's — the suite adopts them.
//!
//! The suite below drives the same random writes into a station on a store and into Stage 1, and
//! after every one holds the store's rows to what Stage 1 holds — and every tenth, the `log.adi`
//! its mirror writes. Until the cut that is P6 said twice; once Part B lands, it is the proof that
//! the new write path makes the changes the old one made. It found one divergence in the write
//! path it was copied from, which C19 B2 mended —
//! `an_own_echo_that_restamps_a_contact_on_file_reaches_the_store`.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tempo_core::logbook::{LogOp, Logbook, OpClass, QsoEdit, QsoRecord, RecordId};

use crate::station::{fill_with, DxccResolve, LogFill, LotwStamped, RowRefusal};

/// Stage 1's log, and the resolvers its writes fill contacts from — a station's, set alike.
#[derive(Default)]
pub(crate) struct Stage1 {
    pub(crate) logbook: Logbook,
    pub(crate) dxcc_resolve: Option<Arc<DxccResolve>>,
    #[allow(clippy::type_complexity)]
    pub(crate) state_resolve:
        Option<Box<dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync>>,
}

impl Stage1 {
    /// `StationCore::add_record`: the contact goes in, carrying the id the station gave it.
    pub(crate) fn add_record(&mut self, rec: QsoRecord) -> RecordId {
        let id = rec
            .id
            .expect("a contact reaches Stage 1 with the id the station gave it");
        self.logbook.add(rec);
        id
    }

    /// `StationCore::take_in_log_file`.
    pub(crate) fn take_in_log_file(&mut self, text: &str) {
        let (country, state) = (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
        let _ = self
            .logbook
            .import_adif_with(text, |r| fill_with(r, country, state));
    }

    /// `StationCore::apply_log_fills`, on a station with a store (the only one it runs on).
    pub(crate) fn apply_log_fills(&mut self, fills: &[LogFill]) -> usize {
        let by_id: HashMap<tempo_core::logbook::RecordId, &LogFill> =
            fills.iter().map(|f| (f.id, f)).collect();
        let hits: Vec<(usize, Option<String>, Option<String>)> = self
            .logbook
            .records()
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let fill = by_id.get(&r.id?)?;
                let country = fill.country.clone().filter(|_| r.country.is_none());
                let state = fill.state.clone().filter(|_| r.state.is_none());
                (country.is_some() || state.is_some()).then_some((i, country, state))
            })
            .collect();
        if !hits.is_empty() {
            // An upgrade, like the fills above: content a fold reads, and no row moves.
            let records = self.logbook.records_mut(OpClass::Upgrade);
            for (i, country, state) in &hits {
                let r = Arc::make_mut(&mut records[*i]);
                if let Some(c) = country {
                    r.country = Some(c.clone());
                }
                if let Some(s) = state {
                    r.state = Some(s.clone());
                }
            }
        }
        hits.len()
    }

    /// The contact the log holds under `id`, if any — `StationCore::row`.
    fn row(&self, id: RecordId) -> Option<Arc<QsoRecord>> {
        self.logbook
            .records()
            .iter()
            .find(|r| r.id == Some(id))
            .cloned()
    }

    /// `StationCore::fresh_row`.
    fn fresh_row(&self, id: RecordId, edit_key: &str) -> Result<Arc<QsoRecord>, RowRefusal> {
        let row = self.row(id).ok_or(RowRefusal::Gone)?;
        if QsoEdit::project(&row).key() == edit_key {
            Ok(row)
        } else {
            Err(RowRefusal::Changed(row))
        }
    }

    /// `StationCore::change_row`.
    fn change_row(&mut self, op: LogOp) -> bool {
        !self.logbook.apply(op).is_empty()
    }

    /// `StationCore::update_qso`.
    pub(crate) fn update_qso(&mut self, id: RecordId, mut rec: QsoRecord) -> bool {
        // Keep country populated on edits (the edit form doesn't carry it).
        if rec.country.is_none() {
            if let Some(resolve) = &self.dxcc_resolve {
                rec.country = resolve(&rec.call);
            }
        }
        !self
            .logbook
            .apply(LogOp::Edit {
                id,
                rec: Box::new(rec),
            })
            .is_empty()
    }

    /// `StationCore::edit_qso`, its QSL-sent mark dated `date_unix`.
    pub(crate) fn edit_qso(
        &mut self,
        id: RecordId,
        edit_key: &str,
        edit: &QsoEdit,
        date_unix: u64,
    ) -> Result<Result<(), RowRefusal>, String> {
        let stored = match self.fresh_row(id, edit_key) {
            Ok(row) => row,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let sent = edit.qsl_sent_change(&stored)?;
        let card = edit.qsl_card_change(&stored);
        let mut rec = edit.record(&stored);
        // Keep country populated on edits, as `update_qso` does: the form does not carry it.
        if let Some(resolve) = &self.dxcc_resolve {
            rec.country = resolve(&rec.call);
        }
        self.logbook.apply(LogOp::Edit {
            id,
            rec: Box::new(rec),
        });
        if let Some(via) = sent {
            self.logbook
                .apply(LogOp::MarkQslSent { id, via, date_unix });
        }
        if let Some(received) = card {
            self.logbook.apply(LogOp::MarkQslCard { id, received });
        }
        Ok(Ok(()))
    }

    /// `StationCore::mark_qsl_sent`, dated `date_unix`.
    pub(crate) fn mark_qsl_sent(
        &mut self,
        id: RecordId,
        via: Option<tempo_core::logbook::QslVia>,
        date_unix: u64,
    ) -> bool {
        self.change_row(LogOp::MarkQslSent { id, via, date_unix })
    }

    /// `StationCore::mark_qsl_card`.
    pub(crate) fn mark_qsl_card(&mut self, id: RecordId, received: bool) -> bool {
        self.change_row(LogOp::MarkQslCard { id, received })
    }

    /// `StationCore::set_sat_tag`.
    pub(crate) fn set_sat_tag(&mut self, id: RecordId, sat_name: Option<&str>) -> bool {
        self.change_row(LogOp::SetSatTag {
            id,
            sat_name: sat_name.map(str::to_string),
        })
    }

    /// `StationCore::delete_qso`.
    pub(crate) fn delete_qso(&mut self, id: RecordId) -> bool {
        self.change_row(LogOp::Delete(id))
    }

    /// `StationCore::clear_logbook`.
    pub(crate) fn clear_logbook(&mut self) -> usize {
        self.logbook.clear()
    }

    /// `StationCore::import_adif`, on a station with a store.
    pub(crate) fn import_adif(&mut self, text: &str) -> (usize, usize, usize, usize) {
        let (country, state) = (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
        let (added, skipped, merged) = self
            .logbook
            .import_adif_with(text, |r| fill_with(r, country, state));
        (added.len(), skipped, merged, self.logbook.len())
    }

    /// `StationCore::merge_lotw_report`.
    pub(crate) fn merge_lotw_report(
        &mut self,
        text: &str,
    ) -> tempo_core::reconcile::ReconcileSummary {
        self.logbook.merge_report(text)
    }

    /// `StationCore::import_pota_log`.
    pub(crate) fn import_pota_log(&mut self, text: &str) -> (usize, usize, usize) {
        self.logbook.stamp_ota_refs(text)
    }

    /// `StationCore::merge_lotw_own_echo`.
    pub(crate) fn merge_lotw_own_echo(&mut self, text: &str, when_unix: i64) -> usize {
        self.logbook.merge_own_echo(text, when_unix)
    }

    /// `StationCore::stamp_qrz_upload`.
    pub(crate) fn stamp_qrz_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.logbook.stamp_qrz_upload(pushed, status)
    }

    /// `StationCore::stamp_clublog_upload`.
    pub(crate) fn stamp_clublog_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.logbook.stamp_clublog_upload(pushed, status)
    }

    /// `StationCore::stamp_eqsl_upload`.
    pub(crate) fn stamp_eqsl_upload(
        &mut self,
        pushed: &QsoRecord,
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> bool {
        let status = tempo_core::logbook::UploadStatus {
            outcome,
            when_unix,
            detail,
        };
        self.logbook.stamp_eqsl_upload(pushed, status)
    }

    /// `StationCore::merge_eqsl_report`.
    pub(crate) fn merge_eqsl_report(
        &mut self,
        text: &str,
    ) -> tempo_core::reconcile::ReconcileSummary {
        self.logbook.merge_report(text)
    }

    /// `StationCore::merge_qrz_report`, on a station with a store.
    pub(crate) fn merge_qrz_report(
        &mut self,
        text: &str,
    ) -> (usize, tempo_core::reconcile::ReconcileSummary) {
        let before = self.logbook.len();
        let (added, summary) = self.logbook.merge_downloaded(text);
        // One change of rows: the merge's adds and upgrades. The merge appends the contacts
        // it adds, and each is filled here, before it is written (SPEC-2 v3 D2-A); the rows
        // the log already held are the fill job's.
        if self.logbook.len() > before {
            let (country, state) = (self.dxcc_resolve.as_deref(), self.state_resolve.as_deref());
            for r in self
                .logbook
                .records_mut(OpClass::Upgrade)
                .iter_mut()
                .skip(before)
            {
                fill_with(Arc::make_mut(r), country, state);
            }
        }
        (added.len(), summary)
    }

    /// `StationCore::stamp_lotw_upload`.
    pub(crate) fn stamp_lotw_upload(
        &mut self,
        ids: &[RecordId],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) {
        let wanted: HashSet<RecordId> = ids.iter().copied().collect();
        let hits: Vec<usize> = self
            .logbook
            .records()
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id.is_some_and(|id| wanted.contains(&id)))
            .map(|(i, _)| i)
            .collect();
        // One classified write for the whole batch — a stamp nothing derived reads, and
        // taking it per row would move the revision once per stamped record.
        let records = self.logbook.records_mut(OpClass::Stamp);
        for i in hits {
            Arc::make_mut(&mut records[i]).upload.lotw = Some(tempo_core::logbook::UploadStatus {
                outcome,
                when_unix,
                detail,
            });
        }
    }

    /// `StationCore::stamp_lotw_batch`, the batch as the ids and fingerprints TQSL was handed.
    pub(crate) fn stamp_lotw_batch(
        &mut self,
        batch: &[(RecordId, u64)],
        outcome: tempo_core::logbook::UploadOutcome,
        when_unix: i64,
        detail: Option<tempo_core::logbook::UploadDetail>,
    ) -> LotwStamped {
        let mut signed: HashMap<tempo_core::logbook::RecordId, u64> =
            batch.iter().copied().collect();
        let mut report = LotwStamped::default();
        let mut hits = Vec::new();
        for (i, r) in self.logbook.records().iter().enumerate() {
            let Some(fingerprint) = r.id.and_then(|id| signed.remove(&id)) else {
                continue;
            };
            if lotw_fingerprint(r) == fingerprint {
                hits.push(i);
            } else {
                report.changed += 1;
            }
        }
        report.gone = signed.len();
        report.stamped = hits.len();
        if !hits.is_empty() {
            // One classified write for the whole batch, as `stamp_lotw_upload` makes.
            let records = self.logbook.records_mut(OpClass::Stamp);
            for i in hits {
                Arc::make_mut(&mut records[i]).upload.lotw =
                    Some(tempo_core::logbook::UploadStatus {
                        outcome,
                        when_unix,
                        detail,
                    });
            }
        }
        report
    }
}

/// `station::lotw_fingerprint`, verbatim: the contact as TQSL signs it, without its connector
/// stamps.
pub(crate) fn lotw_fingerprint(r: &QsoRecord) -> u64 {
    use std::hash::Hasher;
    let mut row = r.clone();
    row.upload = Default::default();
    let mut h = std::hash::DefaultHasher::new();
    h.write(tempo_core::logbook::adif_record(&row).as_bytes());
    h.finish()
}

mod lockstep {
    //! ★ The same random writes into a station on a store and into Stage 1. After every one the
    //! store's rows are Stage 1's log — and so is the copy in memory, while it exists — and every
    //! few writes the `log.adi` the store's mirror writes is the one Stage 1's log pictures
    //! ([`mirror_adif`]), byte for byte.
    use super::*;
    use crate::station::{LotwSigned, StationCore};
    use tempo_core::logbook::mirror::mirror_adif;
    use tempo_core::logbook::sqlite::Resolved;
    use tempo_core::logbook::{
        adif_header, adif_record, adif_record_own_log, QslVia, UploadOutcome,
    };

    /// SplitMix64 — a deterministic stream, so a failing seed replays.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
            xs[self.below(xs.len())]
        }
        fn chance(&mut self, percent: u64) -> bool {
            self.next() % 100 < percent
        }
    }

    const CALLS: &[&str] = &[
        "W1AW",
        "w1aw",
        "W1AW/P",
        "K1ABC",
        "VP2E/AA9A",
        "DL1ZZZ",
        "JA1XYZ",
        "K1ſAB",
        "OH2ÖÖ",
    ];
    const BANDS: &[&str] = &["20m", "40m", "2m", ""];
    const MODES: &[&str] = &["FT8", "CW", "SSB"];
    const T0: u64 = 1_788_000_000;

    /// The station's resolvers, and Stage 1's: a country from the call, a state for W calls.
    fn country(call: &str) -> Option<String> {
        (call.trim().len() >= 3).then(|| call.trim()[..2].to_ascii_uppercase())
    }
    fn state(call: &str, _grid: Option<&str>) -> Option<String> {
        call.starts_with('W').then(|| "MA".into())
    }

    fn contact(rng: &mut Rng) -> QsoRecord {
        let mut parsed = Logbook::default();
        parsed.import_adif(
            "<CALL:4>W1AW<QSO_DATE:8>20260901<TIME_ON:6>120000<BAND:3>20m<MODE:3>FT8<EOR>",
        );
        let mut r = QsoRecord::clone(&parsed.records()[0]);
        r.id = None;
        r.call = rng.pick(CALLS).to_string();
        r.band = rng.pick(BANDS).to_string();
        r.mode = rng.pick(MODES).to_string();
        r.when_unix = T0 + 60 * rng.below(200) as u64;
        r.grid = rng
            .pick(&[None, Some("FN31"), Some("jo62qm")])
            .map(str::to_string);
        r.comment = rng
            .pick(&[None, Some("tnx"), Some("Größe")])
            .map(str::to_string);
        r.notes = rng
            .pick(&[None, None, Some("private: 599\nrig 2")])
            .map(str::to_string);
        r.ota.my_ref = rng.pick(&[None, None, Some("US-1234")]).map(str::to_string);
        r.country = rng.pick(&[None, None, Some("Germany")]).map(str::to_string);
        r
    }

    /// A folder of the test's own, gone with the value.
    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "nexus-stage1-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn log(&self) -> std::path::PathBuf {
            self.0.join("log.adi")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A station on a store of its own — the launch's open path — and Stage 1 beside it with the
    /// same resolvers.
    fn lockstep(d: &Dir) -> (StationCore, Stage1) {
        let opened = crate::logstore::open_with(
            &d.log(),
            Arc::new(|_| Resolved::default()),
            None,
            tempo_core::logbook::mirror::MirrorOptions {
                debounce: std::time::Duration::from_millis(5),
                max_delay: std::time::Duration::from_millis(50),
                accepted: None,
            },
        )
        .expect("the store opens");
        let mut sc = StationCore::new();
        sc.set_dxcc_resolver(country);
        sc.set_state_resolver(state);
        let _ = sc.attach_store(opened);
        // Stage 1 starts as the station does, on an empty log.
        let stage1 = Stage1 {
            logbook: Logbook::default(),
            dxcc_resolve: Some(Arc::new(country)),
            state_resolve: Some(Box::new(state)),
        };
        (sc, stage1)
    }

    /// Every contact the store holds, once every change submitted is written — waiting longer
    /// than a screen would, so a busy box is not a divergence.
    fn stored(sc: &StationCore) -> Vec<QsoRecord> {
        let reads = sc.store.as_ref().expect("the station has a store").reads();
        let (rows, fresh) = reads
            .rows(std::time::Duration::from_secs(30))
            .expect("the store reads");
        assert_eq!(
            fresh,
            crate::logstore::Freshness::Current,
            "the writer caught up"
        );
        rows
    }

    /// The date a station gave the QSL-sent mark of `id` — or its withdrawal, which is dated
    /// too — as its store holds it.
    fn sent_date(sc: &StationCore, id: RecordId) -> u64 {
        let row = stored(sc).into_iter().find(|r| r.id == Some(id));
        row.and_then(|r| match r.qsl_sent.sent {
            true => r.qsl_sent.date_unix,
            false => r.qsl_sent.cleared_unix,
        })
        .unwrap_or(0)
    }

    /// One row of the log, as a report restates it, with `extra` tags.
    fn restated(row: &QsoRecord, extra: &str) -> String {
        let text = adif_record(row);
        let at = text.rfind("<EOR>").expect("a record ends");
        format!("{}{}{extra}{}", adif_header(), &text[..at], &text[at..])
    }

    /// One write, made to the station and to Stage 1 alike. What it was, for a failure.
    fn change(sc: &mut StationCore, s1: &mut Stage1, rng: &mut Rng) -> &'static str {
        let log: Vec<QsoRecord> = s1
            .logbook
            .records()
            .iter()
            .map(|r| QsoRecord::clone(r))
            .collect();
        let n = log.len();
        let any = |rng: &mut Rng| log[rng.below(n)].clone();
        match rng.below(22) {
            0..=3 => {
                // `Engine::log_qso`'s append: the station mints the contact's id.
                let (mut rows, _) = sc.append(vec![contact(rng)], false);
                s1.add_record(rows.pop().expect("one contact in, one out"));
                "log a contact"
            }
            4 if n > 0 => {
                // Mostly an ordinary correction, which keeps the call; sometimes a busted call.
                let (row, mut rec) = (any(rng), contact(rng));
                if rng.chance(70) {
                    rec.call = row.call;
                }
                let id = row.id.unwrap();
                assert_eq!(sc.update_qso(id, rec.clone()), s1.update_qso(id, rec));
                "edit"
            }
            5 if n > 0 => {
                // The Logbook form's edit, against the version it shows — or one it showed before.
                let row = any(rng);
                let id = row.id.unwrap();
                let key = if rng.chance(20) {
                    "0000000000000000".to_string()
                } else {
                    QsoEdit::project(&row).key()
                };
                let mut edit = QsoEdit::project(&row);
                edit.comment = Some(format!("form {}", rng.below(9)));
                edit.band = rng.pick(BANDS).to_string();
                edit.qsl_sent_via = rng
                    .pick(&[None, Some("B"), Some("E"), Some("SENT")])
                    .map(str::to_string);
                edit.qsl_card = rng.chance(50);
                if rng.chance(20) {
                    edit.call = rng.pick(CALLS).to_string();
                }
                let station = sc.edit_qso(id, &key, &edit).map(|done| done.is_ok());
                let date = sent_date(sc, id);
                let copy = s1.edit_qso(id, &key, &edit, date).map(|done| done.is_ok());
                assert_eq!(station, copy, "the form edit");
                "form edit"
            }
            6 if n > 0 => {
                let id = any(rng).id.unwrap();
                assert_eq!(sc.delete_qso(id), s1.delete_qso(id));
                "delete"
            }
            7 if rng.chance(15) => {
                assert_eq!(sc.clear_logbook(), s1.clear_logbook());
                "purge"
            }
            8 => {
                // New contacts, and sometimes one the log holds restated (a dupe, or an upgrade).
                let mut text = adif_header();
                for _ in 0..1 + rng.below(3) {
                    text.push_str(&adif_record_own_log(&contact(rng)));
                }
                if n > 0 && rng.chance(50) {
                    text.push_str(&adif_record(&any(rng)));
                }
                assert_eq!(sc.import_adif(&text), s1.import_adif(&text));
                "import"
            }
            9 if n > 0 => {
                let report = restated(&any(rng), "<LOTW_QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC");
                assert_eq!(sc.merge_lotw_report(&report), s1.merge_lotw_report(&report));
                "LoTW confirmation"
            }
            10 if n > 0 => {
                let report = restated(&any(rng), "<EQSL_QSL_RCVD:1>Y");
                assert_eq!(sc.merge_eqsl_report(&report), s1.merge_eqsl_report(&report));
                "eQSL confirmation"
            }
            11 => {
                // QRZ's book: a row the log holds, confirmed, and one it does not.
                let mut text = adif_record(&contact(rng));
                if n > 0 {
                    text = restated(&any(rng), "<QSL_RCVD:1>Y") + &text;
                }
                assert_eq!(sc.merge_qrz_report(&text), s1.merge_qrz_report(&text));
                "QRZ download"
            }
            12 if n > 0 => {
                let mut r = any(rng);
                r.ota.their_program = Some("POTA".into());
                r.ota.their_ref = Some("US-0002".into());
                let text = format!("{}{}", adif_header(), adif_record(&r));
                assert_eq!(sc.import_pota_log(&text), s1.import_pota_log(&text));
                "POTA park stamp"
            }
            13 if n > 0 => {
                let pushed = any(rng);
                let (outcome, when) = (
                    rng.pick(&[UploadOutcome::Accepted, UploadOutcome::Rejected]),
                    1 + rng.below(9) as i64,
                );
                match rng.below(3) {
                    0 => assert_eq!(
                        sc.stamp_qrz_upload(&pushed, outcome, when, None),
                        s1.stamp_qrz_upload(&pushed, outcome, when, None)
                    ),
                    1 => assert_eq!(
                        sc.stamp_clublog_upload(&pushed, outcome, when, None),
                        s1.stamp_clublog_upload(&pushed, outcome, when, None)
                    ),
                    _ => assert_eq!(
                        sc.stamp_eqsl_upload(&pushed, outcome, when, None),
                        s1.stamp_eqsl_upload(&pushed, outcome, when, None)
                    ),
                };
                "connector stamp"
            }
            14 if n > 0 => {
                let ids: Vec<RecordId> = (0..1 + rng.below(3))
                    .map(|_| any(rng).id.unwrap())
                    .collect();
                sc.stamp_lotw_upload(&ids, UploadOutcome::Pending, 5, None);
                s1.stamp_lotw_upload(&ids, UploadOutcome::Pending, 5, None);
                "LoTW stamp"
            }
            15 if n > 0 => {
                // A batch TQSL signed; the log may move under it before the stamp.
                let rows: Vec<QsoRecord> = (0..1 + rng.below(3)).map(|_| any(rng)).collect();
                let signed: Vec<LotwSigned> = rows.iter().filter_map(LotwSigned::of).collect();
                let pairs: Vec<(RecordId, u64)> = rows
                    .iter()
                    .map(|r| (r.id.unwrap(), lotw_fingerprint(r)))
                    .collect();
                if rng.chance(30) {
                    let (id, rec) = (rows[0].id.unwrap(), contact(rng));
                    sc.update_qso(id, rec.clone());
                    s1.update_qso(id, rec);
                } else if rng.chance(20) {
                    let id = rows[0].id.unwrap();
                    sc.delete_qso(id);
                    s1.delete_qso(id);
                }
                assert_eq!(
                    sc.stamp_lotw_batch(&signed, UploadOutcome::Accepted, 6, None),
                    s1.stamp_lotw_batch(&pairs, UploadOutcome::Accepted, 6, None),
                    "the batch's report"
                );
                "LoTW batch"
            }
            16 if n > 0 => {
                let id = any(rng).id.unwrap();
                if rng.chance(50) {
                    let via = rng.pick(&[Some(QslVia::Bureau), Some(QslVia::Direct), None]);
                    let station = sc.mark_qsl_sent(id, via);
                    let date = sent_date(sc, id);
                    assert_eq!(station, s1.mark_qsl_sent(id, via, date));
                    "QSL sent"
                } else {
                    let received = rng.chance(70);
                    assert_eq!(
                        sc.mark_qsl_card(id, received),
                        s1.mark_qsl_card(id, received)
                    );
                    "QSL card"
                }
            }
            17 if n > 0 => {
                let id = any(rng).id.unwrap();
                let sat = rng.chance(50).then_some("SO-50");
                assert_eq!(sc.set_sat_tag(id, sat), s1.set_sat_tag(id, sat));
                "satellite tag"
            }
            18 if n > 0 => {
                // The fill job's findings for contacts without a country or a state.
                let fills: Vec<LogFill> = log
                    .iter()
                    .filter(|r| r.country.is_none() || r.state.is_none())
                    .map(|r| LogFill {
                        id: r.id.unwrap(),
                        country: Some("Filled".into()),
                        state: rng.chance(50).then(|| "ST".into()),
                    })
                    .collect();
                assert_eq!(sc.apply_log_fills(&fills, 3), s1.apply_log_fills(&fills));
                "fills"
            }
            19 => {
                // Another window's `log.adi`, taken in.
                let mut text = adif_header() + &adif_record_own_log(&contact(rng));
                if n > 0 {
                    text.push_str(&adif_record_own_log(&any(rng)));
                }
                sc.take_in_log_file(&text, None);
                s1.take_in_log_file(&text);
                "take in a log file"
            }
            20 if n > 0 => {
                let text = restated(&any(rng), "");
                assert_eq!(
                    sc.merge_lotw_own_echo(&text, 7),
                    s1.merge_lotw_own_echo(&text, 7)
                );
                "LoTW own echo"
            }
            _ => "nothing",
        }
    }

    /// The rows a write added at `from` and after, whose ids Stage 1's log minted, take the ids
    /// the station minted for them (the fourth difference in the module header). Only where the
    /// id on each side is new — one Stage 1 did not hold before the write — so a row that kept an
    /// id, or took one the log already held, is left for [`assert_same`] to report.
    fn adopt_minted(sc: &StationCore, s1: &mut Stage1, from: usize, held: &HashSet<RecordId>) {
        let station = stored(sc);
        if station.len() != s1.logbook.len() {
            return;
        }
        let new = |id: Option<RecordId>| id.is_some_and(|id| !held.contains(&id));
        let minted: Vec<(usize, RecordId)> = s1
            .logbook
            .records()
            .iter()
            .zip(&station)
            .enumerate()
            .skip(from)
            .filter(|(_, (mine, theirs))| mine.id != theirs.id && new(mine.id) && new(theirs.id))
            .map(|(i, (_, theirs))| (i, theirs.id.unwrap()))
            .collect();
        if !minted.is_empty() {
            let records = s1.logbook.records_mut(OpClass::IdOnly);
            for (i, id) in minted {
                Arc::make_mut(&mut records[i]).id = Some(id);
            }
        }
    }

    /// Where two logs first part: a count, or the first contact that differs — each side from
    /// the field the two first differ in.
    fn difference(a: &[QsoRecord], b: &[QsoRecord]) -> Option<String> {
        if a.len() != b.len() {
            return Some(format!("{} contacts against {}", a.len(), b.len()));
        }
        let (i, (x, y)) = a.iter().zip(b).enumerate().find(|(_, (x, y))| x != y)?;
        let (x, y) = (format!("{x:?}"), format!("{y:?}"));
        let same: String = x
            .chars()
            .zip(y.chars())
            .take_while(|(p, q)| p == q)
            .map(|(p, _)| p)
            .collect();
        let from = same.rfind(", ").map_or(0, |at| at + 2);
        let near = |s: &str| s[from..].chars().take(120).collect::<String>();
        Some(format!("contact {i}: {} / {}", near(&x), near(&y)))
    }

    /// The store is Stage 1's log — and, while it lives, so is the copy in memory. Every contact
    /// in it carries an id of its own, so no id the station minted was adopted twice.
    fn assert_same(sc: &StationCore, s1: &Stage1, what: &str) {
        let want: Vec<QsoRecord> = s1
            .logbook
            .records()
            .iter()
            .map(|r| QsoRecord::clone(r))
            .collect();
        let ids: HashSet<RecordId> = want.iter().filter_map(|r| r.id).collect();
        assert_eq!(
            ids.len(),
            want.len(),
            "{what}: a contact without an id of its own"
        );
        let held: Vec<QsoRecord> = sc
            .logbook
            .records()
            .iter()
            .map(|r| QsoRecord::clone(r))
            .collect();
        if let Some(d) = difference(&held, &want) {
            panic!("{what}: the copy in memory is not Stage 1's log: {d}");
        }
        if let Some(d) = difference(&stored(sc), &want) {
            panic!("{what}: the store is not Stage 1's log: {d}");
        }
    }

    /// A LoTW own echo that finds a contact already on file stamps it again — the echo's
    /// time, and a Duplicate becomes Accepted — without counting it as promoted. Until C19 B2
    /// the station carried an echo to the store only when that count moved, so the store kept
    /// the stamp the copy in memory had replaced: the divergence this oracle found. B2 plans the
    /// echo by content, which carries it.
    #[test]
    fn an_own_echo_that_restamps_a_contact_on_file_reaches_the_store() {
        let d = Dir::new("echo");
        let (mut sc, mut s1) = lockstep(&d);
        let (mut rows, _) = sc.append(vec![contact(&mut Rng(7))], false);
        let r = rows.pop().expect("one contact in, one out");
        s1.add_record(r.clone());
        let id = r.id.unwrap();
        sc.stamp_lotw_upload(&[id], UploadOutcome::Duplicate, 6, None);
        s1.stamp_lotw_upload(&[id], UploadOutcome::Duplicate, 6, None);
        assert_same(&sc, &s1, "the upload stamp");
        let echo = restated(&r, "");
        assert_eq!(
            sc.merge_lotw_own_echo(&echo, 7),
            0,
            "premise: nothing newly promoted"
        );
        assert_eq!(s1.merge_lotw_own_echo(&echo, 7), 0);
        let stamp = s1.logbook.records()[0]
            .upload
            .lotw
            .as_ref()
            .map(|s| (s.outcome, s.when_unix));
        assert_eq!(
            stamp,
            Some((UploadOutcome::Accepted, 7)),
            "premise: stamped again"
        );
        assert_same(&sc, &s1, "the own echo");
    }

    /// ★ 16 seeds × 40 random writes of every kind: after each, the store and the copy are Stage
    /// 1's log; every tenth, the mirror's `log.adi` is Stage 1's log, byte for byte. Premise: every
    /// kind of write ran. `NEXUS_STAGE1_SEEDS=300` runs deeper, for a write path just changed.
    #[test]
    fn every_write_leaves_the_store_as_stage_1_would_have_left_the_log() {
        let seeds: usize = std::env::var("NEXUS_STAGE1_SEEDS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(16);
        let (mut ran, mut compared): (HashMap<&'static str, usize>, usize) = (HashMap::new(), 0);
        for seed in 1..=seeds as u64 {
            let d = Dir::new(&format!("s{seed}"));
            let (mut sc, mut s1) = lockstep(&d);
            let mut rng = Rng(seed.wrapping_mul(0x0C19_D5E1));
            let (mut history, mut mirrored) = (Vec::new(), false);
            for step in 0..40 {
                let (from, held) = (
                    s1.logbook.len(),
                    s1.logbook.records().iter().filter_map(|r| r.id).collect(),
                );
                let what = change(&mut sc, &mut s1, &mut rng);
                adopt_minted(&sc, &mut s1, from, &held);
                *ran.entry(what).or_insert(0) += 1;
                history.push(what);
                assert_same(&sc, &s1, &format!("seed {seed} step {step}: {what}"));
                if step % 10 == 9 {
                    let store = sc.store.as_ref().unwrap();
                    store
                        .flush(std::time::Duration::from_secs(60))
                        .expect("written");
                    match std::fs::read_to_string(d.log()) {
                        Ok(text) => {
                            (mirrored, compared) = (true, compared + 1);
                            assert_eq!(
                                text,
                                mirror_adif(s1.logbook.records()),
                                "seed {seed} step {step}: the mirror's log.adi is not Stage 1's log"
                            );
                        }
                        // The mirror writes a change: a log nothing has changed has no file yet.
                        Err(e)
                            if e.kind() == std::io::ErrorKind::NotFound
                                && !mirrored
                                && s1.logbook.is_empty() => {}
                        Err(e) => panic!("seed {seed} step {step}: no log.adi ({e}): {history:?}"),
                    }
                }
            }
            sc.store
                .as_ref()
                .unwrap()
                .flush(std::time::Duration::from_secs(60))
                .expect("written");
        }
        for kind in [
            "log a contact",
            "edit",
            "form edit",
            "delete",
            "purge",
            "import",
            "LoTW confirmation",
            "eQSL confirmation",
            "QRZ download",
            "POTA park stamp",
            "connector stamp",
            "LoTW stamp",
            "LoTW batch",
            "QSL sent",
            "QSL card",
            "satellite tag",
            "fills",
            "take in a log file",
            "LoTW own echo",
        ] {
            assert!(
                ran.get(kind).copied().unwrap_or(0) > 0,
                "premise: {kind} ran: {ran:?}"
            );
        }
        assert!(
            compared >= 2 * seeds,
            "premise: the mirror's log.adi was compared ({compared} of {})",
            4 * seeds
        );
    }
}
