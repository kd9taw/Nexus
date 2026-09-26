//! SPEC-2 v3 C19 Part B's bulk proof: a bulk change planned on the CANDIDATE SUB-LOG — the rows
//! of the calls it brings ([`LogPlan::candidates`]) — decides exactly what the same change decides
//! over the whole log. Seeded logs and reports, with the calls a matcher is easiest to get wrong
//! about: ASCII case, surrounding spaces (the report keys compare a call untrimmed, the store keys
//! it trimmed), characters outside ASCII, and the mode spellings the keys fold (USB and SSB, FT4
//! and MFSK) — on the 1.13 path's log in memory and on the store, whose candidates SQL reads.

use super::*;
use tempo_core::logbook::{adif_header, adif_record_own_log, parse_adif, UploadStatus};

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
        (self.next() % n as u64) as usize
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())]
    }
    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

/// Calls that are one station to a matcher and several to a careless one: case, surrounding
/// spaces, a portable suffix, and letters outside ASCII (ß uppercases to SS in Unicode but not in
/// ASCII, ſ to S, and Ø and é not at all).
const CALLS: [&str; 16] = [
    "W1AW", "w1aw", " W1AW", "W1AW ", "W1AW/P", "DL1AB", "dl1ab", "ß1AA", "SS1AA", "ss1aa", "dſ1x",
    "DS1X", "Ø1XY", "é1AB", "E1AB", "JA1AA",
];
/// A station the reports never name: its rows are never a candidate.
const OTHER: &str = "K9ZZZ";
const BANDS: [&str; 3] = ["20m", "20M", "40m"];
const MODES: [&str; 8] = ["FT8", "FT4", "MFSK", "SSB", "USB", "CW", "PSK31", ""];
const T0: u64 = 1_788_000_000;

/// A contact of `call`, somewhere in four days.
fn contact(g: &mut Rng, call: &str) -> QsoRecord {
    let mut r =
        parse_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>000000<EOR>")
            .remove(0);
    r.call = call.to_string();
    r.band = g.pick(&BANDS).to_string();
    r.mode = g.pick(&MODES).to_string();
    r.when_unix = T0 + g.below(4) as u64 * 86_400 + g.below(6) as u64 * 3_600 + g.below(3) as u64;
    r.time_known = !g.chance(5);
    if g.chance(4) {
        r.upload.lotw = Some(UploadStatus {
            outcome: tempo_core::logbook::UploadOutcome::Pending,
            when_unix: 1,
            detail: None,
        });
    }
    r
}

/// A log of `n` contacts with ids of their own, most of them of the calls the reports name.
fn log_of(g: &mut Rng, n: usize) -> Vec<Arc<QsoRecord>> {
    (0..n)
        .map(|i| {
            let call = if g.chance(5) { OTHER } else { g.pick(&CALLS) };
            let mut r = contact(g, call);
            r.id = Some(RecordId::Provisional {
                hash: 9_000 + i as u64,
                ordinal: 0,
            });
            Arc::new(r)
        })
        .collect()
}

/// A report restating contacts of `log` — its call as the log holds it or in another spelling,
/// maybe on the next day, maybe in another spelling of its mode — with news about them, and
/// contacts the log lacks. Some rows carry an id: one the log holds, a free one, or one twice.
fn report_of(g: &mut Rng, log: &[Arc<QsoRecord>], news: &str) -> String {
    let mut text = adif_header();
    for k in 0..(4 + g.below(10)) {
        let mut r = if !log.is_empty() && !g.chance(3) {
            let mut r = QsoRecord::clone(&log[g.below(log.len())]);
            if g.chance(3) {
                r.call = g.pick(&CALLS).to_string();
            }
            if g.chance(4) {
                r.when_unix += 86_400;
            }
            if g.chance(4) {
                r.mode = match r.mode.as_str() {
                    "SSB" => "USB",
                    "FT4" => "MFSK",
                    m => m,
                }
                .to_string();
            }
            r
        } else {
            let call = g.pick(&CALLS);
            contact(g, call)
        };
        r.id = match g.below(4) {
            0 if !log.is_empty() => log[g.below(log.len())].id,
            1 => Some(RecordId::Provisional {
                hash: 77_000 + k as u64,
                ordinal: 0,
            }),
            2 => Some(RecordId::Provisional {
                hash: 77_000,
                ordinal: 0,
            }),
            _ => None,
        };
        text.push_str(&adif_record_own_log(&r).replace("<EOR>", &format!("{news}<EOR>")));
    }
    text
}

/// What `op` makes of the WHOLE log `rows`: its answer, and the rows it changed and appended as
/// a plan names them — each appended row keeping the id it brought only where `rows` held that id
/// nowhere (once), which is the rule `op` applied, and `None` where `op` minted one.
fn whole<R>(
    rows: &[Arc<QsoRecord>],
    ids: &[RecordId],
    op: impl FnOnce(&mut Logbook) -> R,
) -> (R, Vec<RowPair>) {
    let mut log = Logbook::new();
    log.replace_rows(rows.to_vec());
    let out = op(&mut log);
    let after = log.records();
    let mut pairs: Vec<RowPair> = rows
        .iter()
        .zip(after)
        .filter(|(b, a)| !Arc::ptr_eq(b, a) && ***b != ***a)
        .map(|(b, a)| (Some(Arc::clone(b)), Some(Arc::clone(a))))
        .collect();
    let held: HashSet<RecordId> = rows.iter().filter_map(|r| r.id).collect();
    let brought: HashSet<RecordId> = ids.iter().copied().collect();
    let mut taken = HashSet::new();
    for a in after.iter().skip(rows.len()) {
        let mut r = QsoRecord::clone(a);
        match r.id {
            Some(id) if brought.contains(&id) && !held.contains(&id) && taken.insert(id) => {}
            _ => r.id = None,
        }
        pairs.push((None, Some(Arc::new(r))));
    }
    (out, pairs)
}

/// The five bulk changes, each as THE implementation runs it, with the text it brings.
enum Op {
    Import,
    Report,
    Download,
    OtaRefs,
    OwnEcho,
}

impl Op {
    const ALL: [Op; 5] = [
        Op::Import,
        Op::Report,
        Op::Download,
        Op::OtaRefs,
        Op::OwnEcho,
    ];

    fn name(&self) -> &'static str {
        match self {
            Op::Import => "import",
            Op::Report => "report",
            Op::Download => "download",
            Op::OtaRefs => "ota refs",
            Op::OwnEcho => "own echo",
        }
    }

    /// The news a report of this kind carries about the contacts it restates.
    fn news(&self) -> &'static str {
        match self {
            Op::Import => "<QSL_RCVD:1>Y<STATE:2>WI",
            Op::Report => "<LOTW_QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
            Op::Download => "<APP_QRZLOG_STATUS:1>C",
            Op::OtaRefs => "<SIG:4>POTA<SIG_INFO:6>K-0001",
            Op::OwnEcho => "",
        }
    }

    /// Plan it on `plan`'s candidates, and run it on the whole of `rows`: both answers (as text),
    /// and both plans' rows.
    fn both(
        &self,
        plan: &LogPlan,
        rows: &[Arc<QsoRecord>],
        text: &str,
    ) -> ((String, Vec<RowPair>), (String, Vec<RowPair>)) {
        let parsed = parse_adif(text);
        let ids: Vec<RecordId> = parsed.iter().filter_map(|r| r.id).collect();
        let calls: Vec<String> = parsed.into_iter().map(|r| r.call).collect();
        macro_rules! run {
            ($class:expr, $ids:expr, $op:expr) => {{
                let (out, planned) =
                    plan_on_candidates(plan, calls, $ids, $class, $op).expect("the plan reads");
                let (w, pairs) = whole(rows, $ids, $op);
                (
                    (format!("{out:?}"), planned.pairs),
                    (format!("{w:?}"), pairs),
                )
            }};
        }
        match self {
            Op::Import => run!(OpClass::Upgrade, &ids, |log: &mut Logbook| {
                let (added, skipped, merged) = log.import_adif(text);
                (added.len(), skipped, merged)
            }),
            Op::Report => run!(OpClass::Upgrade, &[], |log: &mut Logbook| log
                .merge_report(text)),
            Op::Download => run!(OpClass::Upgrade, &ids, |log: &mut Logbook| {
                let (added, summary) = log.merge_downloaded(text);
                (added.len(), summary)
            }),
            Op::OtaRefs => run!(OpClass::Upgrade, &[], |log: &mut Logbook| log
                .stamp_ota_refs(text)),
            Op::OwnEcho => run!(OpClass::Stamp, &[], |log: &mut Logbook| log
                .merge_own_echo(text, 7)),
        }
    }
}

/// A plan over the 1.13 path's log in memory, `rows`, with nothing on its way.
fn memory_plan(rows: &[Arc<QsoRecord>]) -> LogPlan {
    LogPlan {
        rows: crate::logstore::LogRows::Memory(rows.to_vec()),
        pending: crate::logstore::Pending::default(),
        rev: 0,
        foreign: None,
    }
}

/// ★ ON THE LOG IN MEMORY: every bulk change planned on the candidate sub-log answers and changes
/// exactly what it does over the whole log — 64 seeds of each, each change taking effect (a row
/// upgraded or appended) in most of them.
#[test]
fn every_bulk_change_plans_on_its_candidates_as_on_the_whole_log() {
    let mut changed = [0usize; 5];
    for seed in 0..64u64 {
        for (k, op) in Op::ALL.iter().enumerate() {
            let mut g = Rng(seed * 31 + k as u64);
            let n = 10 + g.below(40);
            let rows = log_of(&mut g, n);
            let text = report_of(&mut g, &rows, op.news());
            let (planned, whole) = op.both(&memory_plan(&rows), &rows, &text);
            assert_eq!(
                planned,
                whole,
                "seed {seed}, {}: the plan on the candidates is the whole log's",
                op.name()
            );
            if !planned.1.is_empty() {
                changed[k] += 1;
            }
        }
    }
    for (k, op) in Op::ALL.iter().enumerate() {
        assert!(
            changed[k] >= 32,
            "{}: only {} of 64 seeds changed anything — a run that changes nothing proves nothing",
            op.name(),
            changed[k]
        );
    }
}

/// ★ ON THE STORE: the same, with the candidates read by SQL — by the `qso_callhist` index one
/// call at a time, and by one pass over the log when a report names too many calls — from a store
/// converted from the same log.
#[test]
fn every_bulk_change_plans_on_the_stores_candidates_as_on_the_whole_log() {
    for seed in 0..12u64 {
        for (k, op) in Op::ALL.iter().enumerate() {
            let mut g = Rng(1_000 + seed * 31 + k as u64);
            let n = 10 + g.below(40);
            let rows = log_of(&mut g, n);
            let d = crate::logstore::tests::Dir::new(&format!("bulk-{seed}-{k}"));
            let mut adif = adif_header();
            for r in &rows {
                adif.push_str(&adif_record_own_log(r));
            }
            std::fs::write(d.log(), adif).unwrap();
            let engine = crate::logstore::tests::engine_on_store(&d);
            crate::logstore::tests::flush(&engine);
            // The rows as the store holds them, which the whole-log run is asked of.
            let stored: Vec<Arc<QsoRecord>> = crate::logstore::tests::stored(&d)
                .into_iter()
                .map(Arc::new)
                .collect();
            let text = report_of(&mut g, &stored, op.news());
            let plan = engine.station().log_view();
            let (planned, whole) = op.both(&plan, &stored, &text);
            assert_eq!(
                planned,
                whole,
                "seed {seed}, {}: the plan on the store's candidates is the whole log's",
                op.name()
            );
            let calls: std::collections::BTreeSet<String> = parse_adif(&text)
                .iter()
                .map(|r| tempo_core::logbook::sqlite::call_norm_of(&r.call))
                .collect();
            assert_eq!(
                plan.candidates_seeking(&calls, 0).expect("one pass"),
                plan.candidates_seeking(&calls, usize::MAX)
                    .expect("a look-up per call"),
                "seed {seed}: one pass over the log finds the rows the look-ups do"
            );
        }
    }
}

/// ★ POSITIVE CONTROL for the two proofs above: a sub-log that leaves out ONE row a report can
/// match — the row logged `w1aw`, which the report's `W1AW` matches up to ASCII case and the
/// store's `call_norm` keys with it — plans differently from the whole log. So the comparison can
/// see a candidate set that is short by one row.
#[test]
fn a_sub_log_short_of_one_matching_row_plans_differently() {
    let mut g = Rng(5);
    let mut rows = log_of(&mut g, 6);
    let mut lower = contact(&mut g, "w1aw");
    lower.band = "2m".into(); // a key no other row of the log shares
    lower.id = Some(RecordId::Provisional {
        hash: 1,
        ordinal: 0,
    });
    rows.push(Arc::new(lower.clone()));
    let mut restated = lower.clone();
    restated.id = None;
    restated.call = "W1AW".into();
    let text =
        adif_header() + &adif_record_own_log(&restated).replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
    let (on_all, _) = whole(&rows, &[], |log: &mut Logbook| log.merge_report(&text));
    let short: Vec<Arc<QsoRecord>> = rows.iter().filter(|r| r.id != lower.id).cloned().collect();
    let (on_short, _) = whole(&short, &[], |log: &mut Logbook| log.merge_report(&text));
    assert!(
        on_all.matched >= 1,
        "premise: the report matches the row logged w1aw"
    );
    assert_ne!(
        format!("{on_all:?}"),
        format!("{on_short:?}"),
        "without that one row the plan differs"
    );
    // And the candidates hold it: its call keys with the report's.
    let calls: std::collections::BTreeSet<String> = parse_adif(&text)
        .iter()
        .map(|r| tempo_core::logbook::sqlite::call_norm_of(&r.call))
        .collect();
    assert!(memory_plan(&rows)
        .candidates(&calls)
        .expect("reads")
        .iter()
        .any(|r| r.id == lower.id));
}

/// A bulk change moves the watermarks of what it did: an import that upgrades a contact the log
/// holds and appends one it lacks moves an upgrade's (`content_rev`, `index_rev`) and then an
/// append's (`key_rev`: new rows, new keys), and never `shape_rev` (no row edited or removed); an
/// import that only appends stays an append — the rows held before are, unchanged and in order,
/// the first rows held now, which is what the companion import's one contact must keep.
#[test]
fn a_bulk_change_moves_the_watermarks_of_what_it_did() {
    let mut g = Rng(11);
    let mut sc = StationCore::new();
    let held = contact(&mut g, "W1AW");
    let held = sc.append(vec![held], false).0.remove(0);

    let before = sc.marks();
    let lacking = contact(&mut g, "DL1AB");
    let (added, _, merged, _) = sc.import_adif(&(adif_header() + &adif_record_own_log(&lacking)));
    assert_eq!((added, merged), (1, 0), "premise: only appended");
    let after = sc.marks();
    assert!(
        after.appended_only_since(before.revision),
        "an import that only appends is an append"
    );
    assert!(after.key_rev > before.key_rev, "new keys");

    let before = sc.marks();
    let mut restated = held.clone();
    restated.id = None;
    let text = adif_header()
        + &adif_record_own_log(&restated).replace("<EOR>", "<QSL_RCVD:1>Y<EOR>")
        + &adif_record_own_log(&contact(&mut g, "JA1AA"));
    let (added, _, merged, _) = sc.import_adif(&text);
    assert_eq!(
        (added, merged),
        (1, 1),
        "premise: one upgraded, one appended"
    );
    let after = sc.marks();
    assert!(
        !after.appended_only_since(before.revision),
        "an upgrade is not an append"
    );
    assert!(
        after.index_rev > before.index_rev,
        "the upgrade's index watermark"
    );
    assert!(after.key_rev > before.key_rev, "the append's key watermark");
    assert_eq!(
        after.shape_rev, before.shape_rev,
        "no row edited or removed"
    );
}
