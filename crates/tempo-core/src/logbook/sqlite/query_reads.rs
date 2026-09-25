//! The store's reads behind the UI's log questions — SPEC-2 v3's **C17a** (§4.3, §4.9a).
//!
//! Each is one statement shape an index answers, or a pass in log order over the few columns a
//! question reads, handed back through THE decoder ([`super::record_from_row`]): a narrow row is
//! the whole record's decode with every column the question does not read selected as the value an
//! empty field reads back as, so a field cannot be read one way here and another in `load_all`.
//!
//! A row is named by its **rowid** in here: the handle an order vector holds for as long as the
//! process runs (SPEC-2 v3 P5 — never persisted, never handed to a UI). Log order is rowid order.
//!
//! None of these waits for the writer or fences the Engine lock itself: they run inside a read the
//! caller took through `LogReader` / the app's `StoreReads`, which do both.

use super::{contest_is_empty, record_from_row, Error, LogDb, Result, Rows, QSO_COLUMNS};
use crate::logbook::{QsoRecord, RecordId};
use rusqlite::params_from_iter;
use std::collections::HashMap;

/// The columns the Logbook's order and search read (`logQuery.ts`): the seven searched fields, the
/// sort columns, and the four confirmation channels `confirmed` / `award_confirmed` are read from.
pub const ORDER_COLUMNS: &[&str] = &[
    "call",
    "country",
    "grid",
    "band",
    "mode",
    "freq_mhz",
    "rst_sent",
    "rst_rcvd",
    "ota_their_ref",
    "ota_my_ref",
    "when_unix",
    "qsl_card_rcvd_raw",
    "lotw_rcvd_raw",
    "eqsl_rcvd_raw",
    "qrz_status_raw",
];

/// The columns an entity's slots read: the call (its resolved entity), the stored country (the
/// fallback), and the band, frequency and mode the slots are keyed on.
pub const ENTITY_COLUMNS: &[&str] = &["call", "country", "band", "freq_mhz", "mode"];

/// A rowid as an order vector holds it. SQLite hands them out from 1 upward; a store whose rowids
/// have left `u32` is refused rather than mis-numbered.
fn rowid_u32(rowid: i64) -> Result<u32> {
    u32::try_from(rowid).map_err(|_| Error::Sql(rusqlite::Error::IntegralValueOutOfRange(0, rowid)))
}

impl LogDb {
    /// Every row, newest first, equal times in log order — the Logbook's default order (time
    /// descending, nothing filtered), which is exactly the `qso_recent` index's own order: its
    /// key descending, and within a key the rowid ascending.
    pub fn rowids_newest_first(&self) -> Result<Vec<u32>> {
        let mut stmt = self.conn.prepare(
            "SELECT rowid FROM qso INDEXED BY qso_recent ORDER BY when_unix DESC, rowid ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(rowid_u32(row.get(0)?)?);
        }
        Ok(out)
    }

    /// Every row's rowid, in log order.
    pub fn rowids_in_log_order(&self) -> Result<Vec<u32>> {
        let mut stmt = self.conn.prepare("SELECT rowid FROM qso ORDER BY rowid")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(rowid_u32(row.get(0)?)?);
        }
        Ok(out)
    }

    /// Hand `each` every row after rowid `after`, in log order, with only `columns` filled (the id
    /// is always read) — one row at a time, never the log's rows held together.
    pub fn each_narrow_after(
        &self,
        columns: &[&str],
        after: u32,
        each: &mut dyn FnMut(u32, &QsoRecord),
    ) -> Result<()> {
        let sql = format!(
            "SELECT {}, q.rowid FROM qso q WHERE q.rowid > ?1 ORDER BY q.rowid",
            self.narrow_projection(columns)?
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([i64::from(after)])?;
        let rowid_at = QSO_COLUMNS.len();
        while let Some(row) = rows.next()? {
            let rowid = rowid_u32(row.get(rowid_at)?)?;
            let mut rec = record_from_row(row)?;
            if rec.contest.as_deref().is_some_and(contest_is_empty) {
                rec.contest = None;
            }
            each(rowid, &rec);
        }
        Ok(())
    }

    /// The rows at these rowids, whole — each exactly as [`LogDb::load_all`] decodes it — aligned
    /// with the rowids asked: `None` where the store no longer holds one (another window deleted
    /// it). Run inside one read transaction by the caller.
    pub fn rows_at(&self, rowids: &[u32]) -> Result<Vec<Option<QsoRecord>>> {
        let mut sorted: Vec<i64> = rowids.iter().map(|&r| i64::from(r)).collect();
        sorted.sort_unstable();
        sorted.dedup();
        let mut by_id: HashMap<RecordId, QsoRecord> = HashMap::new();
        let mut id_at: HashMap<u32, RecordId> = HashMap::new();
        for chunk in sorted.chunks(super::ROWS_PER_STATEMENT) {
            let list = chunk
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT rowid, id FROM qso WHERE rowid IN ({list})"
            ))?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let text: String = row.get(1)?;
                if let Ok(id) = text.parse::<RecordId>() {
                    id_at.insert(rowid_u32(row.get(0)?)?, id);
                }
            }
            for rec in self.decode(Rows::At(chunk), &mut || {})? {
                if let Some(id) = rec.id {
                    by_id.insert(id, rec);
                }
            }
        }
        Ok(rowids
            .iter()
            .map(|r| id_at.get(r).and_then(|id| by_id.get(id)).cloned())
            .collect())
    }

    /// The rowid of the row `id` names, if the store holds it.
    pub fn rowid_of(&self, id: &RecordId) -> Result<Option<u32>> {
        let mut stmt = self.conn.prepare("SELECT rowid FROM qso WHERE id = ?1")?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(rowid_u32(row.get(0)?)?)),
            None => Ok(None),
        }
    }

    /// The rowids whose stored `call_norm` is one of `keys`, in log order — the `qso_callhist`
    /// index. SPEC-2 v3 P2: a caller that means "the same call" passes keys WIDER than its own
    /// test and applies that test to every row these name.
    pub fn rowids_by_call_norm(&self, keys: &[String]) -> Result<Vec<u32>> {
        let mut out = Vec::new();
        for chunk in keys.chunks(super::ROWS_PER_STATEMENT) {
            let places: Vec<String> = (1..=chunk.len()).map(|i| format!("?{i}")).collect();
            let mut stmt = self.conn.prepare(&format!(
                "SELECT rowid FROM qso INDEXED BY qso_callhist WHERE call_norm IN ({})",
                places.join(", ")
            ))?;
            let mut rows = stmt.query(params_from_iter(chunk.iter()))?;
            while let Some(row) = rows.next()? {
                out.push(rowid_u32(row.get(0)?)?);
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// The SELECT list of a narrow read: every column of [`QSO_COLUMNS`] in its place — so
    /// [`record_from_row`] reads the row unchanged — with those `columns` does not name selected
    /// as the value an empty field decodes from ([`LogDb::empty_values`], the one C14's narrow
    /// reads use). The id is always read. A name the schema does not have is an error.
    fn narrow_projection(&self, columns: &[&str]) -> Result<String> {
        if let Some(unknown) = columns.iter().find(|c| !QSO_COLUMNS.contains(c)) {
            return Err(Error::Sql(rusqlite::Error::InvalidColumnName(
                (*unknown).to_string(),
            )));
        }
        let empty = self.empty_values()?;
        let mut projection = Vec::with_capacity(QSO_COLUMNS.len());
        for c in QSO_COLUMNS {
            if *c == "id" || columns.contains(c) {
                projection.push(format!("q.{c}"));
            } else {
                let value = empty
                    .get(*c)
                    .ok_or_else(|| rusqlite::Error::InvalidColumnName((*c).to_string()))?;
                projection.push((*value).to_string());
            }
        }
        Ok(projection.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::query::{order, LogQuery, SortKey};
    use crate::logbook::sqlite::Resolved;
    use crate::logbook::{parse_adif, RecordId};

    /// A store of `n` synthetic contacts with every kind of field the order reads, and the records
    /// `load_all` reads back from it.
    fn store(n: usize) -> (LogDb, Vec<QsoRecord>) {
        let mut records = Vec::new();
        for i in 0..n {
            let mut r = parse_adif(&format!(
                "<CALL:6>K{}Q{:03}<BAND:3>20m<MODE:3>FT8<FREQ:6>14.074<QSO_DATE:8>20260901\
                 <TIME_ON:6>{:02}{:02}00<COUNTRY:6>Canada<GRIDSQUARE:4>FN{:02}<RST_SENT:3>-{:02}<EOR>",
                i % 7,
                i % 997,
                i / 60 % 24,
                // Every seventh minute repeats, so equal times meet in the orders below.
                i % 60 / 7 * 7,
                i % 100,
                i % 30,
            ))
            .remove(0);
            r.id = Some(RecordId::Provisional {
                hash: 10_000 + i as u64,
                ordinal: 0,
            });
            r.qsl_rcvd.lotw = i % 5 == 0;
            r.confirmed = r.qsl_rcvd.any();
            r.award_confirmed = r.qsl_rcvd.award();
            r.ota.their_ref = (i % 9 == 0).then(|| format!("US-{:04}", i % 13));
            records.push(r);
        }
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert_all(records.iter().map(|r| (r, Resolved::default())))
            .unwrap();
        let all = db.load_all().unwrap();
        (db, all)
    }

    /// The default order read off the index IS the order the port gives for the default query —
    /// newest first, equal times in log order — and the index answers it with no sort of its own.
    #[test]
    fn the_newest_first_rowids_are_the_default_order() {
        let (db, all) = store(300);
        let rowids = db.rowids_in_log_order().unwrap();
        let default = LogQuery {
            sort: SortKey::Time,
            asc: false,
            search: String::new(),
            needs_confirm_only: false,
        };
        let expected: Vec<u32> = order(rowids.iter().copied().zip(all.iter()), &default);
        assert_eq!(db.rowids_newest_first().unwrap(), expected);
        let plan: Vec<String> = {
            let mut stmt = db
                .conn
                .prepare(
                    "EXPLAIN QUERY PLAN SELECT rowid FROM qso INDEXED BY qso_recent \
                     ORDER BY when_unix DESC, rowid ASC",
                )
                .unwrap();
            let mut rows = stmt.query([]).unwrap();
            let mut out = Vec::new();
            while let Some(row) = rows.next().unwrap() {
                out.push(row.get::<_, String>(3).unwrap());
            }
            out
        };
        assert!(
            plan.iter().all(|p| !p.contains("TEMP B-TREE")),
            "the index gives the order itself: {plan:?}"
        );
    }

    /// A narrow read hands out exactly the named fields of the whole record, the rest empty, with
    /// the rowid log order gives it — and a column the schema lacks is refused.
    #[test]
    fn a_narrow_read_is_the_whole_decode_of_its_columns() {
        let (db, all) = store(40);
        let rowids = db.rowids_in_log_order().unwrap();
        let mut seen = Vec::new();
        db.each_narrow_after(ORDER_COLUMNS, 0, &mut |rowid, r| {
            seen.push((rowid, r.clone()))
        })
        .unwrap();
        assert_eq!(seen.len(), all.len());
        for ((rowid, narrow), (expected_rowid, whole)) in seen.iter().zip(rowids.iter().zip(&all)) {
            assert_eq!(rowid, expected_rowid);
            assert_eq!(narrow.id, whole.id);
            let text = |r: &QsoRecord| {
                (
                    r.call.clone(),
                    r.country.clone(),
                    r.grid.clone(),
                    r.band.clone(),
                    r.mode.clone(),
                    r.rst_sent.clone(),
                    r.rst_rcvd.clone(),
                    r.ota.their_ref.clone(),
                    r.ota.my_ref.clone(),
                )
            };
            let rest = |r: &QsoRecord| {
                (
                    r.freq_mhz,
                    r.when_unix,
                    r.qsl_rcvd,
                    r.confirmed,
                    r.award_confirmed,
                )
            };
            assert_eq!(text(narrow), text(whole));
            assert_eq!(rest(narrow), rest(whole));
            // What the order does not read is not read.
            assert_eq!(
                (narrow.name.as_deref(), narrow.state.as_deref()),
                (None, None)
            );
            assert!(narrow.extra.is_empty() && narrow.contest.is_none());
        }
        // After a rowid: the rows past it only.
        let mut tail = Vec::new();
        db.each_narrow_after(ENTITY_COLUMNS, rowids[29], &mut |rowid, _| tail.push(rowid))
            .unwrap();
        assert_eq!(tail, rowids[30..]);
        assert!(db
            .each_narrow_after(&["no_such_column"], 0, &mut |_, _| {})
            .is_err());
    }

    /// Rows by rowid come back whole, aligned with the rowids asked — including a rowid asked
    /// twice, in any order, and one the store does not hold.
    #[test]
    fn rows_at_rowids_are_aligned_and_whole() {
        let (db, all) = store(30);
        let rowids = db.rowids_in_log_order().unwrap();
        let asked = [rowids[7], rowids[2], 99_999, rowids[7]];
        let got = db.rows_at(&asked).unwrap();
        assert_eq!(
            got,
            vec![
                Some(all[7].clone()),
                Some(all[2].clone()),
                None,
                Some(all[7].clone())
            ]
        );
        assert_eq!(db.rowid_of(&all[4].id.unwrap()).unwrap(), Some(rowids[4]));
        let absent = RecordId::Provisional {
            hash: 1,
            ordinal: 0,
        };
        assert_eq!(db.rowid_of(&absent).unwrap(), None);
    }

    /// The call index names every row whose stored `call_norm` is asked for, in log order.
    #[test]
    fn rows_by_call_norm_are_the_index_in_log_order() {
        let (db, all) = store(60);
        let rowids = db.rowids_in_log_order().unwrap();
        let keys = vec![
            "K3Q003".to_string(),
            "NOBODY".to_string(),
            "K0Q007".to_string(),
        ];
        let expected: Vec<u32> = all
            .iter()
            .zip(&rowids)
            .filter(|(r, _)| keys.contains(&r.call.trim().to_ascii_uppercase()))
            .map(|(_, &rowid)| rowid)
            .collect();
        assert!(!expected.is_empty(), "premise: the keys name rows");
        assert_eq!(db.rowids_by_call_norm(&keys).unwrap(), expected);
    }
}
