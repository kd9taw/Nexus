//! ★ SPEC-2 v3 C19, Part B: every change to one contact is ONE function of that row
//! ([`LogOp::apply_to`]), and that function is the positional body it replaced.
//!
//! Before Part B each by-id op reached a positional body on the log's `Vec` — `update_record`,
//! `mark_qsl_sent`, `mark_qsl_card`, `set_sat_tag`, `delete`, and `apply`'s own stamp arm. The
//! station now plans a change on the row it read from the store, so the body must be a
//! function of the row alone, and the log runs the same function on the row it holds. The
//! oracle here is the code it replaced, VERBATIM (`vec_bodies`, copied from the tree before the
//! change with only `self.records` renamed to a parameter), run on an identical log: seeded
//! random rows and ops, every field an edit merges varied, compared row for row after every op,
//! with what the op reports and the watermarks it moves.

use super::{
    Effects, LogOp, Logbook, QslRcvd, QslSent, QslVia, QsoRecord, UploadOutcome, UploadService,
    UploadState, UploadStatus,
};
use crate::logbook::{ContestFields, Ota};
use std::sync::Arc;

/// The positional bodies Part B replaced, VERBATIM — see the module header. Each changes the
/// row at `index` of `records` and answers whether it did; `records` stands in for the log's
/// `self.records`, whose watermark bookkeeping the test checks on its own.
#[allow(clippy::ptr_arg, clippy::needless_return)]
mod vec_bodies {
    use crate::logbook::{QslRcvd, QslSent, QslVia, QsoRecord, UploadState};
    use std::sync::Arc;

    pub(super) fn update_record(records: &mut Vec<Arc<QsoRecord>>, index: usize, mut rec: QsoRecord) -> bool {
        match records.get(index) {
            Some(old) => {
                // An edit is the same row, corrected: it keeps the row's identity.
                rec.id = old.id;
                // A field-edit must not wipe an operator-declared QSL-sent mark —
                // only `mark_qsl_sent` mutates it. (Kept even on a call fix: the
                // card WAS mailed; that's history, not credit.)
                rec.qsl_sent = old.qsl_sent;
                // TRIMMED comparison: an imported record can carry a padded
                // CALL, while the edit form always sends a trimmed one — an
                // untrimmed compare would read every ordinary edit of such a
                // record as a callsign correction and strip its confirmations.
                let call_changed = !rec.call.trim().eq_ignore_ascii_case(old.call.trim());
                if !call_changed {
                    // Ordinary edit (band/grid/name/…): derived state rides along.
                    rec.confirmed = old.confirmed;
                    rec.award_confirmed = old.award_confirmed;
                    rec.qsl_rcvd = old.qsl_rcvd;
                    rec.credit_granted = old.credit_granted.clone();
                    rec.credit_submitted = old.credit_submitted.clone();
                    rec.upload = old.upload.clone();
                } else {
                    // CALLSIGN correction — operator ruling (2026-07-30): the
                    // services hold the OLD call, so clearing the upload stamps
                    // re-queues the corrected QSO to every one of them; and a
                    // confirmation (or granted credit) matched against the busted
                    // call is credit this QSO never earned — stripped, never
                    // silently carried over. (LoTW itself still holds the
                    // old-call record; nothing we send can retract it.)
                    rec.confirmed = false;
                    rec.award_confirmed = false;
                    rec.qsl_rcvd = QslRcvd::default();
                    rec.credit_granted = Vec::new();
                    rec.credit_submitted = Vec::new();
                    rec.upload = UploadState::default();
                    // The RESURRECTION guard: an imported LOTW_QSL_SENT=Y rides
                    // in `extra`, and the parser's fallback would re-derive
                    // upload.lotw = Accepted from it on the next load — quietly
                    // undoing the clear above and excluding the corrected QSO
                    // from the LoTW batch forever. It described the BUSTED
                    // call's upload; it goes with the stamps.
                    rec.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
                    // Call-derived identity re-derives from the NEW call: the
                    // busted call's entity/state must not ride along. (country
                    // refills from the resolver on the save path; dxcc/state
                    // stay empty until a lookup supplies them.)
                    rec.dxcc = None;
                    rec.country = None;
                    rec.state = None;
                }
                // Never clobber a known country/state to None on an ORDINARY
                // edit (the form doesn't carry them) — but on a call correction
                // the old values describe the busted call and stay cleared.
                if !call_changed {
                    if rec.country.is_none() {
                        rec.country = old.country.clone();
                    }
                    if rec.state.is_none() {
                        rec.state = old.state.clone();
                    }
                }
                // An incoming record with no end time means LEAVE ALONE, never "clear it" —
                // preserve the stored TIME_OFF rather than wiping it on a name/grid edit.
                // The Logbook form does carry the field since #329, so an operator can SET
                // and CORRECT an end time here; a producer that knows nothing about it (a
                // per-row connector push, the remote edit, a contest merge) still cannot
                // drop one. Clearing an end time altogether is deliberately not offered.
                if rec.time_off_unix.is_none() {
                    rec.time_off_unix = old.time_off_unix;
                }
                // Nor does it carry the SPLIT receive leg (#163): the form edits one
                // frequency, so a name/RST fix would silently turn a split contact into a
                // simplex one and drop `FREQ_RX` from the record and every future export.
                // Same rule as TIME_OFF and the park refs above.
                if rec.freq_rx_mhz.is_none() {
                    rec.freq_rx_mhz = old.freq_rx_mhz;
                }
                // Preserve the stored POTA/SOTA park refs when the edit leaves them empty (a
                // busted-call/RST fix must not silently drop the park from the record + ADIF).
                let incoming_ota_empty = rec.ota.my_program.is_none()
                    && rec.ota.my_ref.is_none()
                    && rec.ota.their_program.is_none()
                    && rec.ota.their_ref.is_none();
                if incoming_ota_empty {
                    rec.ota = old.ota.clone();
                }
                // Nor the contest block: the edit form carries none of it (the desktop
                // DTO hard-sets `contest: None`), so without this an ordinary edit drops
                // CONTEST_ID, STX/SRX, both exchange vectors and APP_NEXUS_SESSION. The
                // last one is why this is worse than a lost column — it is the key merge
                // idempotence is keyed on. Preserved on a CALLSIGN correction too, unlike
                // country/state/dxcc: the exchange is what went over the air, and a busted
                // call does not change what was sent or received.
                if rec.contest.is_none() {
                    rec.contest = old.contest.clone();
                }
                // The edit form carries none of the import-carried identity —
                // an edit must never bleach it off the record. (dxcc is
                // call-derived: preserved on an ordinary edit only.)
                if !call_changed && rec.dxcc.is_none() {
                    rec.dxcc = old.dxcc;
                }
                if rec.prop_mode.is_none() {
                    rec.prop_mode = old.prop_mode.clone();
                }
                if rec.sat_name.is_none() {
                    rec.sat_name = old.sat_name.clone();
                }
                if rec.operator.is_none() {
                    rec.operator = old.operator.clone();
                }
                if rec.station_callsign.is_none() {
                    rec.station_callsign = old.station_callsign.clone();
                }
                // #239: same rule — an edit that leaves them empty keeps what the record had.
                if rec.my_grid.is_none() {
                    rec.my_grid = old.my_grid.clone();
                }
                if rec.my_rig.is_none() {
                    rec.my_rig = old.my_rig.clone();
                }
                if rec.extra.is_empty() {
                    rec.extra = old.extra.clone();
                    if call_changed {
                        // The preservation must not undo the resurrection guard
                        // above: the busted call's LOTW_QSL_SENT goes, whether
                        // the extra set came from the payload or from `old`.
                        rec.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
                    }
                }
                // The same resurrection, for a park. A side's OTHER programme's reference rides
                // in `extra` (the POTA pair beside a summit, the POTA_REF beside a WWFF park — see
                // `take_ota_side`), and it described the reference the side HAD. An edit that
                // changes the side takes it along: left behind, it is written after the edited
                // reference, a repeated tag's last copy is the one a reader keeps, and the next
                // read would put the old park back over the correction, or bring back a park the
                // operator removed.
                if (&rec.ota.my_program, &rec.ota.my_ref) != (&old.ota.my_program, &old.ota.my_ref)
                {
                    rec.extra.retain(|(k, _)| {
                        !matches!(
                            k.as_str(),
                            "MY_SIG" | "MY_SIG_INFO" | "MY_SOTA_REF" | "MY_POTA_REF"
                        )
                    });
                }
                if (&rec.ota.their_program, &rec.ota.their_ref)
                    != (&old.ota.their_program, &old.ota.their_ref)
                {
                    rec.extra.retain(|(k, _)| {
                        !matches!(k.as_str(), "SIG" | "SIG_INFO" | "SOTA_REF" | "POTA_REF")
                    });
                }
                // An edit that did not touch the TIME OF DAY must not fabricate
                // time-knowledge onto an imported, time-less record — keyed on
                // the time-of-day, not the whole timestamp, so a DATE fix on a
                // date-only import stays honestly time-unknown.
                if rec.when_unix % 86_400 == old.when_unix % 86_400 {
                    rec.time_known = old.time_known;
                }
                // The same row, corrected: it stays where it is, but its keys may have moved.
                records[index] = Arc::new(rec);
                true
            }
            None => false,
        }
    }
    pub(super) fn mark_qsl_sent(
        records: &mut Vec<Arc<QsoRecord>>,
        index: usize,
        via: Option<QslVia>,
        date_unix: u64,
    ) -> bool {
        match records.get_mut(index) {
            Some(rec) => {
                Arc::make_mut(rec).qsl_sent = match via {
                    Some(via) => QslSent {
                        sent: true,
                        via: Some(via),
                        date_unix: Some(date_unix),
                        cleared_unix: None,
                    },
                    None => QslSent {
                        sent: false,
                        via: None,
                        date_unix: None,
                        cleared_unix: Some(date_unix),
                    },
                };
                true
            }
            None => false,
        }
    }
    pub(super) fn mark_qsl_card(records: &mut Vec<Arc<QsoRecord>>, index: usize, received: bool) -> bool {
        match records.get_mut(index) {
            Some(rec) => {
                let rec = Arc::make_mut(rec);
                rec.qsl_rcvd.card = received;
                rec.confirmed = rec.qsl_rcvd.any();
                rec.award_confirmed = rec.qsl_rcvd.award();
                true
            }
            None => false,
        }
    }
    pub(super) fn set_sat_tag(records: &mut Vec<Arc<QsoRecord>>, index: usize, sat_name: Option<&str>) -> bool {
        let name = match sat_name {
            // A tag with no name is the lone `PROP_MODE=SAT` TQSL rejects. Refused here
            // rather than written, for the same reason an empty QSL-sent code is an error
            // and not a withdrawal: an empty control is a non-choice, not a decision.
            Some(n) if n.trim().is_empty() => return false,
            Some(n) => Some(n.trim().to_string()),
            None => None,
        };
        match records.get_mut(index) {
            Some(rec) => {
                let rec = Arc::make_mut(rec);
                match name {
                    Some(n) => {
                        rec.prop_mode = Some("SAT".into());
                        rec.sat_name = Some(n);
                    }
                    None => {
                        rec.sat_name = None;
                        // Only OUR tag. A contact carrying `PROP_MODE=EME`/`MS`/`TEP` and a
                        // stray `SAT_NAME` is making a different claim about how it got
                        // there, and that claim is not this op's to withdraw.
                        if rec
                            .prop_mode
                            .as_deref()
                            .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"))
                        {
                            rec.prop_mode = None;
                        }
                    }
                }
                true
            }
            None => false,
        }
    }
    pub(super) fn delete(records: &mut Vec<Arc<QsoRecord>>, index: usize) -> bool {
        if index < records.len() {
            records.remove(index);
            true
        } else {
            false
        }
    }
    /// `Logbook::apply`'s stamp arm, as it was: the service's slot on the row at `index`.
    pub(super) fn stamp(
        records: &mut Vec<Arc<QsoRecord>>,
        index: usize,
        service: super::UploadService,
        status: super::UploadStatus,
    ) -> bool {
        match records.get_mut(index) {
            Some(rec) => {
                let row = Arc::make_mut(rec);
                let slot = match service {
                    super::UploadService::Lotw => &mut row.upload.lotw,
                    super::UploadService::Eqsl => &mut row.upload.eqsl,
                    super::UploadService::Qrz => &mut row.upload.qrz,
                    super::UploadService::Clublog => &mut row.upload.clublog,
                };
                *slot = Some(status);
                true
            }
            None => false,
        }
    }
}

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
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())].clone()
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn maybe(&mut self, xs: &[&str]) -> Option<String> {
        self.chance(40).then(|| self.pick(xs).to_string())
    }
}

const CALLS: &[&str] = &["W1AW", " w1aw ", "W1AW/P", "K1ABC", "dl1zzz", "JA1XYZ", ""];
const BANDS: &[&str] = &["20m", "40M", "2m", ""];
const MODES: &[&str] = &["FT8", "SSB", "CW", "MFSK"];
const EXTRA: &[(&str, &str)] = &[
    ("LOTW_QSL_SENT", "Y"),
    ("MY_SIG", "POTA"),
    ("MY_SIG_INFO", "US-0001"),
    ("SIG", "SOTA"),
    ("SOTA_REF", "W7A/MN-001"),
    ("POTA_REF", "US-0002"),
    ("APP_OTHER_X", "1"),
];
const T0: u64 = 1_788_000_000;

fn status(rng: &mut Rng) -> UploadStatus {
    UploadStatus {
        outcome: rng.pick(&[
            UploadOutcome::Accepted,
            UploadOutcome::Duplicate,
            UploadOutcome::Rejected,
            UploadOutcome::Pending,
        ]),
        when_unix: T0 as i64 + rng.below(1000) as i64,
        detail: None,
    }
}

/// A contact with every field an edit merges, keeps or clears set at random.
fn row(rng: &mut Rng) -> QsoRecord {
    let mut r = QsoRecord {
        id: None,
        call: rng.pick(CALLS).to_string(),
        grid: rng.maybe(&["FN31", "fn31pr", ""]),
        country: rng.maybe(&["United States", "Japan"]),
        state: rng.maybe(&["MA", "CT"]),
        band: rng.pick(BANDS).to_string(),
        freq_mhz: rng.pick(&[14.074, 0.0, 7.1]),
        freq_rx_mhz: rng.chance(30).then_some(14.080),
        mode: rng.pick(MODES).to_string(),
        rst_sent: rng.maybe(&["-10", "599"]),
        rst_rcvd: rng.maybe(&["-12", "579"]),
        name: rng.maybe(&["Hiram", "Taro"]),
        qth: rng.maybe(&["Newington"]),
        comment: rng.maybe(&["tnx", "gl"]),
        notes: rng.maybe(&["private"]),
        tx_power: rng.chance(30).then_some(100.0),
        // Some on the same second of the day as others, so the time_known rule is exercised
        // both ways.
        when_unix: T0 + rng.pick(&[0, 60, 86_400, 86_460, 3 * 86_400]),
        time_off_unix: rng.chance(40).then_some(T0 + 120),
        confirmed: rng.chance(40),
        award_confirmed: rng.chance(30),
        qsl_rcvd: QslRcvd {
            card: rng.chance(30),
            lotw: rng.chance(30),
            eqsl: rng.chance(30),
            qrz: rng.chance(30),
        },
        qsl_sent: QslSent {
            sent: rng.chance(30),
            via: rng.chance(40).then(|| rng.pick(&[QslVia::Bureau, QslVia::Direct])),
            date_unix: rng.chance(40).then_some(T0),
            cleared_unix: rng.chance(20).then_some(T0 + 5),
        },
        credit_granted: if rng.chance(30) {
            vec!["DXCC".into()]
        } else {
            Vec::new()
        },
        credit_submitted: if rng.chance(30) {
            vec!["WAS".into()]
        } else {
            Vec::new()
        },
        upload: UploadState {
            lotw: rng.chance(30).then(|| status(rng)),
            eqsl: rng.chance(30).then(|| status(rng)),
            qrz: rng.chance(30).then(|| status(rng)),
            clublog: rng.chance(30).then(|| status(rng)),
        },
        ota: Ota {
            my_program: rng.maybe(&["POTA", "SOTA"]),
            my_ref: rng.maybe(&["US-0001", "W7A/MN-001"]),
            their_program: rng.maybe(&["POTA"]),
            their_ref: rng.maybe(&["US-0002", "K-0003"]),
            iota: rng.maybe(&["NA-001"]),
        },
        time_known: rng.chance(80),
        dxcc: rng.chance(40).then_some(291),
        prop_mode: rng.maybe(&["SAT", "EME", " sat "]),
        sat_name: rng.maybe(&["SO-50", "AO-91"]),
        operator: rng.maybe(&["KD9TAW"]),
        my_grid: rng.maybe(&["EN52"]),
        my_rig: rng.maybe(&["FTDX10"]),
        station_callsign: rng.maybe(&["KD9TAW"]),
        extra: Vec::new(),
        contest: rng.chance(30).then(|| {
            Box::new(ContestFields {
                session: "s1".into(),
                contest_id: "ARRL-FIELD-DAY".into(),
                qid: "s1:1:3".into(),
                ..ContestFields::default()
            })
        }),
    };
    for (k, v) in EXTRA {
        if rng.chance(25) {
            r.extra.push((k.to_string(), v.to_string()));
        }
    }
    r
}

/// One by-id op against the row at `at`, as the operator's form, a mark, a tag, a stamp or a
/// delete sends it.
fn op_on(lb: &Logbook, at: usize, rng: &mut Rng) -> LogOp {
    let id = lb.records()[at].id.expect("every row the log holds carries an id");
    match rng.below(9) {
        0..=2 => {
            // An edit: a fresh row's fields — sometimes the same call, trimmed or re-cased,
            // sometimes a correction — with some of the "leave alone" fields left empty.
            let mut rec = row(rng);
            if rng.chance(50) {
                rec.call = lb.records()[at].call.trim().to_ascii_lowercase();
            }
            if rng.chance(50) {
                rec.when_unix = lb.records()[at].when_unix;
            }
            LogOp::Edit {
                id,
                rec: Box::new(rec),
            }
        }
        3 => LogOp::MarkQslSent {
            id,
            via: rng.chance(70).then(|| rng.pick(&[QslVia::Bureau, QslVia::Direct, QslVia::Electronic])),
            date_unix: T0 + rng.below(100) as u64,
        },
        4 => LogOp::MarkQslCard {
            id,
            received: rng.chance(50),
        },
        5 => LogOp::SetSatTag {
            id,
            sat_name: rng.pick(&[Some("SO-50"), Some(" AO-91 "), Some("   "), None]).map(str::to_string),
        },
        6 | 7 => LogOp::Stamp {
            id,
            service: rng.pick(&[
                UploadService::Lotw,
                UploadService::Eqsl,
                UploadService::Qrz,
                UploadService::Clublog,
            ]),
            status: status(rng),
        },
        _ => LogOp::Delete(id),
    }
}

/// The op through the bodies it replaced, on `records` — and what `apply` reported for it then.
fn by_the_old_bodies(records: &mut Vec<Arc<QsoRecord>>, op: &LogOp) -> Effects {
    let class = op.class();
    let none = || Effects {
        class,
        added: Vec::new(),
        changed: Vec::new(),
        removed: Vec::new(),
    };
    let id = op.target().expect("a by-id op");
    let Some(i) = records.iter().position(|r| r.id == Some(id)) else {
        return none();
    };
    let done = match op.clone() {
        LogOp::Edit { rec, .. } => vec_bodies::update_record(records, i, *rec),
        LogOp::MarkQslSent { via, date_unix, .. } => {
            vec_bodies::mark_qsl_sent(records, i, via, date_unix)
        }
        LogOp::MarkQslCard { received, .. } => vec_bodies::mark_qsl_card(records, i, received),
        LogOp::SetSatTag { sat_name, .. } => {
            vec_bodies::set_sat_tag(records, i, sat_name.as_deref())
        }
        LogOp::Stamp {
            service, status, ..
        } => vec_bodies::stamp(records, i, service, status),
        LogOp::Delete(_) => {
            return if vec_bodies::delete(records, i) {
                Effects {
                    removed: vec![id],
                    ..none()
                }
            } else {
                none()
            };
        }
        _ => unreachable!("a by-id op"),
    };
    if done {
        Effects {
            changed: vec![id],
            ..none()
        }
    } else {
        none()
    }
}

/// ★ Every by-id op, through `Logbook::apply` (the one-row body) and through the positional
/// body it replaced, on identical logs: the same rows after every op, the same report, and the
/// watermarks moved by the op's class exactly when the old body wrote.
#[test]
fn every_one_row_change_is_the_vec_body_it_replaced() {
    let mut ops = 0usize;
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    for seed in 0..64u64 {
        let mut rng = Rng(seed);
        let mut lb = Logbook::new();
        for _ in 0..8 {
            lb.add(row(&mut rng));
        }
        let mut oracle: Vec<Arc<QsoRecord>> = lb.records().to_vec();
        for step in 0..120 {
            if lb.is_empty() || rng.chance(5) {
                let r = row(&mut rng);
                lb.add(r);
                oracle.push(Arc::clone(lb.records().last().expect("just added")));
                continue;
            }
            let at = rng.below(lb.len());
            // Now and then an op for a row that is gone: both must change nothing.
            let op = if rng.chance(5) {
                LogOp::MarkQslCard {
                    id: crate::logbook::RecordId::Provisional {
                        hash: 0xdead,
                        ordinal: rng.below(9) as u32,
                    },
                    received: true,
                }
            } else {
                op_on(&lb, at, &mut rng)
            };
            let what = format!("seed {seed} step {step}: {op:?}");
            *kinds
                .entry(match &op {
                    LogOp::Edit { .. } => "edit",
                    LogOp::MarkQslSent { .. } => "qsl sent",
                    LogOp::MarkQslCard { .. } => "qsl card",
                    LogOp::SetSatTag { .. } => "sat tag",
                    LogOp::Stamp { .. } => "stamp",
                    LogOp::Delete(_) => "delete",
                    _ => "other",
                })
                .or_default() += 1;
            let before = lb.marks();
            let expected = by_the_old_bodies(&mut oracle, &op);
            let effects = lb.apply(op.clone());
            assert_eq!(effects, expected, "{what}: the report");
            let now: Vec<&QsoRecord> = lb.records().iter().map(|r| r.as_ref()).collect();
            let old: Vec<&QsoRecord> = oracle.iter().map(|r| r.as_ref()).collect();
            assert_eq!(now, old, "{what}: the rows");
            let wrote = !expected.is_empty();
            let mut moved = before;
            if wrote {
                moved.mark(op.class());
            }
            let after = lb.marks();
            assert_eq!(
                (
                    after.revision > before.revision,
                    after.content_rev > before.content_rev,
                    after.index_rev > before.index_rev,
                    after.key_rev > before.key_rev,
                    after.shape_rev > before.shape_rev,
                ),
                (
                    moved.revision > before.revision,
                    moved.content_rev > before.content_rev,
                    moved.index_rev > before.index_rev,
                    moved.key_rev > before.key_rev,
                    moved.shape_rev > before.shape_rev,
                ),
                "{what}: the watermarks move as the old body moved them"
            );
            ops += 1;
        }
    }
    // The property reached every kind of op, each many times — a pass that only ever tried one
    // kind would prove nothing about the others.
    for kind in ["edit", "qsl sent", "qsl card", "sat tag", "stamp", "delete"] {
        assert!(
            kinds.get(kind).copied().unwrap_or(0) > 100,
            "{kind} was tried {:?} times",
            kinds.get(kind)
        );
    }
    assert!(ops > 5_000, "{ops}");
}

/// ★ POSITIVE CONTROL for the property above: it can tell the bodies apart. A one-row edit that
/// forgot one of the fields the old body kept (the end time, here) is caught by the same
/// comparison against the same oracle.
#[test]
fn the_parity_check_catches_a_body_that_forgot_a_field() {
    let mut rng = Rng(11);
    let mut caught = false;
    for _ in 0..200 {
        let mut old = row(&mut rng);
        old.id = Some(crate::logbook::RecordId::Provisional { hash: 1, ordinal: 0 });
        old.time_off_unix = Some(T0 + 999);
        let mut rec = row(&mut rng);
        rec.time_off_unix = None;
        let mut oracle = vec![Arc::new(old.clone())];
        vec_bodies::update_record(&mut oracle, 0, rec.clone());
        // The mutation: the one-row edit, then the end time dropped as a body that forgot to
        // keep it would drop it.
        let mut forgot = super::edited(&old, rec);
        forgot.time_off_unix = None;
        if *oracle[0] != forgot {
            caught = true;
            break;
        }
    }
    assert!(caught, "the comparison must see a forgotten field");
}
