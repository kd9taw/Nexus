//! `log_qso`'s duplicate-contact guard, answered from an index instead of a scan of the log.
//!
//! ⛔ **FT HARD GATE — operator yes 2026-09-19, scoped to exactly this:** the guard becomes an
//! index lookup, and the log's append goes through the store, with the behaviour IDENTICAL. The
//! predicate below is the one `Engine::log_qso` has always run, moved here verbatim so there is
//! ONE copy of it; [`DedupIndex`] only narrows which rows it is asked about. The old answer — a
//! scan of every row — is kept as [`scan_for_duplicate`], and it is the ORACLE the parity test
//! holds the index to. Any difference between the two is a behaviour change and a fresh gate.
//!
//! # Why an index can be exact here
//!
//! The predicate's first clause is [`crate::message::same_call`], which is equality of
//! [`crate::message::base_call`] — a pure function of the call. So every row that can satisfy
//! the predicate shares the incoming contact's base call, and a map from base call to the rows
//! holding it hands the predicate exactly those rows: the rest could only have answered no.
//!
//! # Why it never touches the database
//!
//! The FT sequencer logs from the radio loop, under the engine lock, and the whole storage
//! programme exists to keep the disk off that path. The index is built from, and read against,
//! the in-memory [`Logbook`] alone — this module has no store in scope, and nothing here can
//! wait on one.
//!
//! # Keeping it current without a rebuild per contact
//!
//! The index covers the log's first `n` rows at some revision. A later log whose SHAPE has not
//! moved since ([`Logbook::shape_rev`]: no edit, delete, purge, reorder or replacement — see
//! [`super::OpClass`]) holds those same rows, with the same keys, at the same positions, plus
//! whatever was appended; the index is extended by the new rows alone. Anything else rebuilds.
//! Stamps and upgrades leave the shape alone BECAUSE they change no call, band, mode or time —
//! which is precisely what the class honesty of [`super::OpClass`] promises, and what the
//! parity test exercises with every kind of change interleaved.

use super::{Logbook, QsoRecord};
use std::collections::HashMap;

/// How close in time two contacts must be for the second to count as the first logged again.
pub const DEDUP_WINDOW_SECS: u64 = 300;

/// THE guard's predicate: `r`, already in the log, is `rec` logged again — the same station
/// (base call, case-insensitive), band and mode, within [`DEDUP_WINDOW_SECS`]. Verbatim from
/// `Engine::log_qso`; the one copy.
pub fn is_recent_duplicate(r: &QsoRecord, rec: &QsoRecord) -> bool {
    crate::message::same_call(&r.call, &rec.call)
        && r.band.eq_ignore_ascii_case(&rec.band)
        && r.mode.eq_ignore_ascii_case(&rec.mode)
        && rec.when_unix.abs_diff(r.when_unix) <= DEDUP_WINDOW_SECS
}

/// The guard as it always was: every row in the log, asked in turn. O(n) per contact — kept
/// as the ORACLE the index is tested against, and as the reference for what "identical" means.
pub fn scan_for_duplicate(log: &Logbook, rec: &QsoRecord) -> bool {
    log.records().iter().any(|r| is_recent_duplicate(r, rec))
}

/// Base call → the positions of the rows that carry it. See the module header.
#[derive(Debug, Default, Clone)]
pub struct DedupIndex {
    /// The log revision the index was brought up to date at, and how many rows it covers.
    at: Option<(u64, usize)>,
    by_base: HashMap<String, Vec<usize>>,
}

impl DedupIndex {
    /// Whether `rec` is a contact `log` already holds, by the guard's predicate — the same
    /// answer [`scan_for_duplicate`] gives, from the rows sharing `rec`'s base call only.
    pub fn is_duplicate(&mut self, log: &Logbook, rec: &QsoRecord) -> bool {
        self.catch_up(log);
        let rows = log.records();
        self.by_base
            .get(&crate::message::base_call(&rec.call))
            .is_some_and(|at| {
                at.iter()
                    .any(|&i| rows.get(i).is_some_and(|r| is_recent_duplicate(r, rec)))
            })
    }

    /// Bring the index up to `log`: extend it by the appended rows when the log's shape has not
    /// moved since, rebuild it otherwise.
    fn catch_up(&mut self, log: &Logbook) {
        let from = match self.at {
            Some((revision, rows)) if log.shape_rev() <= revision && rows <= log.len() => rows,
            _ => {
                self.by_base.clear();
                0
            }
        };
        for (i, r) in log.records().iter().enumerate().skip(from) {
            self.by_base
                .entry(crate::message::base_call(&r.call))
                .or_default()
                .push(i);
        }
        self.at = Some((log.revision(), log.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{parse_adif, LogOp, OpClass, QslVia, UploadService};
    use proptest::prelude::*;
    use std::sync::Arc;

    /// Call spellings the base-call rule has to see through: portable in both directions, a
    /// compound, the i3=4 hashed wrapper (and its unresolved form), case, and stray spaces.
    const CALLS: &[&str] = &[
        "W1AW",
        "w1aw",
        "W1AW/P",
        "KH6/W1AW",
        "<W1AW>",
        " W1AW ",
        "K1ABC",
        "K1ABC/MM",
        "VP2E/AA9A",
        "AA9A",
        "<...>",
        "DL1ZZZ/4",
        "dl1zzz",
        "",
    ];
    const BANDS: &[&str] = &["20m", "20M", "40m", "", "2m"];
    const MODES: &[&str] = &["FT8", "ft8", "FT4", "CW", "SSB"];
    const T0: u64 = 1_788_000_000;

    fn contact(call: &str, band: &str, mode: &str, when: u64) -> QsoRecord {
        let mut r = parse_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<EOR>").remove(0);
        r.call = call.to_string();
        r.band = band.to_string();
        r.mode = mode.to_string();
        r.when_unix = when;
        r
    }

    /// A contact drawn from the small spaces above, with a time that sits around the window's
    /// edges — exactly 300 s is IN the window, 301 s is out, and both directions matter.
    fn arb_contact() -> impl Strategy<Value = QsoRecord> {
        (
            0..CALLS.len(),
            0..BANDS.len(),
            0..MODES.len(),
            prop_oneof![
                Just(0i64),
                Just(299),
                Just(300),
                Just(301),
                Just(-300),
                Just(-301),
                -2_000i64..2_000,
            ],
        )
            .prop_map(|(c, b, m, dt)| {
                contact(CALLS[c], BANDS[b], MODES[m], (T0 as i64 + dt) as u64)
            })
    }

    /// Every kind of change the app makes to the log, so the index is exercised across each of
    /// them: appends (one, and an import of several), an edit that moves a row's keys, a
    /// delete, a purge, stamps and upgrades that must NOT move the shape, the report merge and
    /// the disk reconcile that do, and a replacement of the whole log (a reload).
    #[derive(Debug, Clone)]
    enum Step {
        Add(QsoRecord),
        Import(Vec<QsoRecord>),
        Edit(usize, QsoRecord),
        Delete(usize),
        Clear,
        Stamp(usize),
        QslSent(usize),
        QslCard(usize),
        Backfill(usize),
        Merge(Vec<QsoRecord>),
        Reconcile(Vec<QsoRecord>),
        Reload,
    }

    fn arb_step() -> impl Strategy<Value = Step> {
        prop_oneof![
            6 => arb_contact().prop_map(Step::Add),
            2 => prop::collection::vec(arb_contact(), 1..4).prop_map(Step::Import),
            2 => (any::<usize>(), arb_contact()).prop_map(|(i, r)| Step::Edit(i, r)),
            2 => any::<usize>().prop_map(Step::Delete),
            1 => Just(Step::Clear),
            1 => any::<usize>().prop_map(Step::Stamp),
            1 => any::<usize>().prop_map(Step::QslSent),
            1 => any::<usize>().prop_map(Step::QslCard),
            1 => any::<usize>().prop_map(Step::Backfill),
            1 => prop::collection::vec(arb_contact(), 1..3).prop_map(Step::Merge),
            1 => prop::collection::vec(arb_contact(), 1..3).prop_map(Step::Reconcile),
            1 => Just(Step::Reload),
        ]
    }

    fn adif_of(rows: &[QsoRecord]) -> String {
        let mut t = crate::logbook::adif_header();
        for r in rows {
            t.push_str(&crate::logbook::adif_record_own_log(r));
        }
        t
    }

    fn run(log: &mut Logbook, step: Step) {
        let n = log.len();
        match step {
            Step::Add(r) => {
                log.add(r);
            }
            Step::Import(rows) => {
                log.import_adif(&adif_of(&rows));
            }
            Step::Edit(i, r) if n > 0 => {
                log.update_record(i % n, r);
            }
            Step::Delete(i) if n > 0 => {
                log.delete(i % n);
            }
            Step::Clear => {
                log.clear();
            }
            Step::Stamp(i) if n > 0 => {
                let id = log.records()[i % n].id.expect("id");
                log.apply(LogOp::Stamp {
                    id,
                    service: UploadService::Qrz,
                    status: crate::logbook::UploadStatus {
                        outcome: crate::logbook::UploadOutcome::Accepted,
                        when_unix: 1,
                        detail: None,
                    },
                });
            }
            Step::QslSent(i) if n > 0 => {
                log.mark_qsl_sent(i % n, Some(QslVia::Bureau), 1);
            }
            Step::QslCard(i) if n > 0 => {
                log.mark_qsl_card(i % n, true);
            }
            Step::Backfill(i) if n > 0 => {
                let rows = log.records_mut(OpClass::Upgrade);
                Arc::make_mut(&mut rows[i % n]).country = Some("X".into());
            }
            Step::Merge(rows) => {
                log.merge_report(&adif_of(&rows));
            }
            Step::Reconcile(rows) => {
                log.reconcile_disk(&adif_of(&rows));
            }
            Step::Reload => {
                let rows: Vec<QsoRecord> = log.records().iter().map(|r| (**r).clone()).collect();
                *log = Logbook::from_store(rows);
            }
            _ => {}
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

        /// ★ THE FT-GATE PARITY TEST. After every change, for every probe contact, the index
        /// and the old scan give the same accept/reject. The scan is the verbatim old guard —
        /// the oracle; the index is what `log_qso` now asks.
        #[test]
        fn the_index_answers_exactly_as_the_old_scan(
            steps in prop::collection::vec(arb_step(), 1..40),
            probes in prop::collection::vec(arb_contact(), 1..12),
        ) {
            let mut log = Logbook::new();
            let mut index = DedupIndex::default();
            for step in steps {
                run(&mut log, step);
                for p in &probes {
                    prop_assert_eq!(
                        index.is_duplicate(&log, p),
                        scan_for_duplicate(&log, p),
                        "probe {:?} {} {} @{} against {} rows",
                        p.call, p.band, p.mode, p.when_unix, log.len()
                    );
                }
            }
        }
    }

    /// The window's edges, pinned by value rather than left to chance: 300 s is a duplicate,
    /// 301 s is not, in both directions — and a portable, a hashed and a lower-case spelling of
    /// one station are one station.
    #[test]
    fn the_window_edges_and_the_call_spellings_are_the_old_guards() {
        let mut log = Logbook::new();
        log.add(contact("W1AW/P", "20m", "FT8", T0));
        let mut index = DedupIndex::default();
        for (call, band, mode, dt, dup) in [
            ("W1AW", "20m", "FT8", 300i64, true),
            ("W1AW", "20m", "FT8", -300, true),
            ("W1AW", "20m", "FT8", 301, false),
            ("W1AW", "20m", "FT8", -301, false),
            ("<W1AW>", "20M", "ft8", 0, true),
            ("w1aw", "20m", "FT8", 0, true),
            ("KH6/W1AW", "20m", "FT8", 0, true),
            ("W1AW", "40m", "FT8", 0, false),
            ("W1AW", "20m", "FT4", 0, false),
            ("K1ABC", "20m", "FT8", 0, false),
        ] {
            let p = contact(call, band, mode, (T0 as i64 + dt) as u64);
            assert_eq!(
                scan_for_duplicate(&log, &p),
                dup,
                "oracle: {call} {band} {mode} {dt}"
            );
            assert_eq!(
                index.is_duplicate(&log, &p),
                dup,
                "index: {call} {band} {mode} {dt}"
            );
        }
    }

    /// The index is EXTENDED, not rebuilt, by an append — and rebuilt by a change of shape.
    /// Watched through its own bookkeeping: an append leaves the rows it already covered where
    /// they were, and a delete makes it start over.
    #[test]
    fn an_append_extends_the_index_and_a_delete_rebuilds_it() {
        let mut log = Logbook::new();
        let mut index = DedupIndex::default();
        log.add(contact("W1AW", "20m", "FT8", T0));
        assert!(index.is_duplicate(&log, &contact("W1AW", "20m", "FT8", T0)));
        let first_rev = index.at.expect("built").0;
        log.add(contact("K1ABC", "20m", "FT8", T0));
        assert!(index.is_duplicate(&log, &contact("K1ABC", "20m", "FT8", T0)));
        assert_eq!(index.by_base.len(), 2, "extended by the one new row");
        assert!(index.at.expect("built").0 > first_rev);

        // A stamp does not move the shape: still an extension (no rows to add).
        let id = log.records()[0].id.unwrap();
        log.apply(LogOp::Stamp {
            id,
            service: UploadService::Eqsl,
            status: crate::logbook::UploadStatus {
                outcome: crate::logbook::UploadOutcome::Accepted,
                when_unix: 1,
                detail: None,
            },
        });
        assert!(index.is_duplicate(&log, &contact("W1AW", "20m", "FT8", T0)));

        // A delete moves the shape: the deleted row must stop answering.
        log.delete(0);
        assert!(
            !index.is_duplicate(&log, &contact("W1AW", "20m", "FT8", T0)),
            "a deleted contact is no longer a duplicate"
        );
        assert!(index.is_duplicate(&log, &contact("K1ABC", "20m", "FT8", T0)));
    }
}
