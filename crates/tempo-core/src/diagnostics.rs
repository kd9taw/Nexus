//! Silent match-failure diagnostics — "why isn't this QSO confirmed, and what's
//! the one fix?". The differentiator incumbents don't ship: incumbents surface
//! "no matching QSO" errors for *manual* fixing; this turns the data we already
//! have (the reconcile `OrphanConfirmation` list, the source-aware
//! `confirmed`/`award_confirmed` split, the per-QSO credit fields) into a ranked,
//! actionable per-QSO explanation. See `tasks/specs/confirmation-diagnostics.md`.
//!
//! Pure (no network, no clock — `now` is a parameter), like [`crate::reconcile`].
//!
//! **Phase 1a** (this module) covers the reasons derivable with NO schema change:
//! confirmed-on-a-non-award-source (R3), field-mismatch via orphans (R4a/b/c),
//! WAS-blocking missing STATE (R4d, US-family-gated), busted call (R6), possible
//! duplicate (R7). The upload-state reasons (R1 never-uploaded, R9 bounced, the
//! Confident R2) need a new `UploadState` field → Phase 1b.

use crate::logbook::QsoRecord;
use crate::reconcile::{mode_class, OrphanConfirmation, ReconcileSummary};

const SECS_PER_DAY: u64 = 86_400;

/// Tunable diagnostics thresholds.
#[derive(Debug, Clone)]
pub struct DiagCfg {
    /// A QSO unconfirmed for less than this is "recent lag", not a failure (R5).
    pub lag_secs: i64,
    /// Max callsign edit distance for a busted-call suggestion (R6).
    pub busted_max_dist: usize,
    /// Don't fuzzy-match calls shorter than this (R6 false-positive guard).
    pub busted_min_call_len: usize,
}

impl Default for DiagCfg {
    fn default() -> Self {
        Self {
            lag_secs: 14 * 86_400, // 14 days
            busted_max_dist: 2,
            busted_min_call_len: 4,
        }
    }
}

/// Which silent-failure reason. R1/R2/R9 are Phase 1b (kept in the enum so the
/// wire type is stable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonCode {
    R1NeverUploaded,
    R2PartnerHasnt,
    R3WrongSource,
    R4aBandMismatch,
    R4bModeMismatch,
    R4cDateMismatch,
    R4dMissingState,
    R5Lag,
    R6BustedCall,
    R7Duplicate,
    R9UploadBounced,
}

/// How sure we are — `Confident` (decidable from local data) vs `Likely` (needs
/// an assumption, e.g. a fuzzy call match).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Confident,
    Likely,
}

/// The per-(QSO × award) award status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QsoAwardStatus {
    Credited,
    Confirmed,
    ConfirmedWrongSource,
    NeedsAction,
    PendingLag,
}

/// A structured, operator-facing action. (Several variants are Phase 1b; 1a emits
/// `UploadToLotw` as guidance + `FixField`/`CorrectBustedCall`/`MergeDuplicate`.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Guidance only in v1 (there is no in-app LoTW upload path — TQSL is OOB).
    UploadToLotw,
    UploadToQrz,
    UploadToClublog,
    ReUpload {
        source: String,
        detail: Option<String>,
    },
    Reauthenticate {
        source: String,
    },
    NudgePartner {
        call: String,
        source: String,
    },
    FixField {
        field: String,
        found: String,
        expected: String,
    },
    CorrectBustedCall {
        logged: String,
        suggested: String,
    },
    MergeDuplicate {
        other_index: usize,
    },
    Wait {
        until_unix: i64,
    },
    None,
}

#[derive(Debug, Clone)]
pub struct Reason {
    pub code: ReasonCode,
    pub confidence: Confidence,
    pub explanation: String,
    pub action: Action,
}

#[derive(Debug, Clone)]
pub struct QsoDiagnosis {
    /// Index into the logbook (oldest-first) this diagnoses.
    pub index: usize,
    /// The award family this row is about — Phase 1a is the single `"DXCC/WAS"`.
    pub award: String,
    pub status: QsoAwardStatus,
    /// Ranked: the top reason is the single highest-leverage fix.
    pub reasons: Vec<Reason>,
}

/// QSOs collapsed by their top action — "12 QSOs need a LoTW confirmation".
#[derive(Debug, Clone)]
pub struct ActionBucket {
    pub kind: String,
    pub count: usize,
    pub qso_indices: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsReport {
    pub diagnoses: Vec<QsoDiagnosis>,
    pub buckets: Vec<ActionBucket>,
    /// QSOs you've uploaded but the partner hasn't (Phase 1b — 0 in 1a).
    pub waiting_on_partner: usize,
    /// Recently-worked unconfirmed QSOs (lag, not a failure) — counted, not listed.
    pub pending_lag: usize,
}

/// Within-QSO reason rank: lower = higher leverage (shown first). The full order
/// R9>R1>R3>R4*>R6>R7>R2>R5 — 1a uses the R3..R7 subset.
fn rank(code: ReasonCode) -> u8 {
    use ReasonCode::*;
    match code {
        R9UploadBounced => 0,
        R1NeverUploaded => 1,
        R3WrongSource => 2,
        R4aBandMismatch | R4bModeMismatch | R4cDateMismatch | R4dMissingState => 3,
        R6BustedCall => 4,
        R7Duplicate => 5,
        R2PartnerHasnt => 6,
        R5Lag => 7,
    }
}

fn is_us_family(entity: Option<&str>) -> bool {
    matches!(
        entity,
        Some("United States") | Some("Alaska") | Some("Hawaii")
    )
}

/// A STATE is "present" for WAS if it's a non-empty 2-letter alpha code. (A full
/// 50-states+DC validation is a Phase-2 refinement; this catches missing/blank/
/// obviously-malformed.)
fn state_present(state: &Option<String>) -> bool {
    state
        .as_deref()
        .map(|s| {
            let t = s.trim();
            t.len() == 2 && t.bytes().all(|b| b.is_ascii_alphabetic())
        })
        .unwrap_or(false)
}

/// Optimal-string-alignment (restricted Damerau-Levenshtein) distance — handles
/// substitution/insertion/deletion + adjacent transposition (W1AW↔W1WA).
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev2 = vec![0usize; m + 1];
    let mut prev = (0..=m).collect::<Vec<usize>>();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// The match-key components of a logged QSO, in the orphan's normalized shape
/// (call UPPER, band lower, mode-CLASS, UTC day) — so R4/R6 compare like-for-like.
fn key_parts(r: &QsoRecord) -> (String, String, &'static str, u64) {
    (
        r.call.to_ascii_uppercase(),
        r.band.to_ascii_lowercase(),
        mode_class(&r.mode),
        r.when_unix / SECS_PER_DAY,
    )
}

/// Diagnose the log against the latest per-source reconcile summaries. Phase 1a:
/// no `UploadState`. `entities[i]` is the resolved DXCC entity name for
/// `records[i]` (for R4d's US-family gate), `None` if unresolved.
pub fn diagnose(
    records: &[QsoRecord],
    entities: &[Option<String>],
    recents: &[&ReconcileSummary],
    now: i64,
    cfg: &DiagCfg,
) -> DiagnosticsReport {
    // Per-record accumulated reasons (deduped by code).
    let mut reasons: Vec<Vec<Reason>> = vec![Vec::new(); records.len()];
    let push_reason = |slots: &mut Vec<Vec<Reason>>, i: usize, r: Reason| {
        if !slots[i].iter().any(|x| x.code == r.code) {
            slots[i].push(r);
        }
    };

    // --- R3: confirmed only on a non-award source ---
    for (i, r) in records.iter().enumerate() {
        if r.confirmed && !r.award_confirmed {
            push_reason(
                &mut reasons,
                i,
                Reason {
                    code: ReasonCode::R3WrongSource,
                    confidence: Confidence::Confident,
                    explanation: format!(
                        "{} is confirmed on a non-award source (eQSL/QRZ) only — that does NOT count for ARRL DXCC/WAS.",
                        r.call
                    ),
                    action: Action::UploadToLotw,
                },
            );
        }
    }

    // --- R4d: award-confirmed US-family QSO missing STATE (blocks WAS) ---
    for (i, r) in records.iter().enumerate() {
        let entity = entities.get(i).and_then(|e| e.as_deref());
        if r.award_confirmed && is_us_family(entity) && !state_present(&r.state) {
            push_reason(
                &mut reasons,
                i,
                Reason {
                    code: ReasonCode::R4dMissingState,
                    confidence: Confidence::Confident,
                    explanation: format!(
                        "{} is confirmed for DXCC but has no STATE — WAS can't credit it. Set the state.",
                        r.call
                    ),
                    action: Action::FixField {
                        field: "STATE".into(),
                        found: r.state.clone().unwrap_or_default(),
                        expected: "the worked station's US state".into(),
                    },
                },
            );
        }
    }

    // --- R7: a possible duplicate — an unconfirmed record with a field-identical,
    // same-key, award-confirmed twin (the confirmation upgraded only one copy). ---
    use std::collections::HashMap;
    let mut by_key: HashMap<(String, String, &'static str, u64), Vec<usize>> = HashMap::new();
    for (i, r) in records.iter().enumerate() {
        by_key.entry(key_parts(r)).or_default().push(i);
    }
    for group in by_key.values() {
        if group.len() < 2 {
            continue;
        }
        for &i in group {
            if records[i].award_confirmed {
                continue;
            }
            // A field-identical, award-confirmed twin?
            if let Some(&twin) = group.iter().find(|&&j| {
                j != i && records[j].award_confirmed && field_identical(&records[i], &records[j])
            }) {
                push_reason(
                    &mut reasons,
                    i,
                    Reason {
                        code: ReasonCode::R7Duplicate,
                        confidence: Confidence::Likely,
                        explanation: format!(
                            "This looks like a possible duplicate of an already-confirmed {} contact — review before merging.",
                            records[i].call
                        ),
                        action: Action::MergeDuplicate { other_index: twin },
                    },
                );
            }
        }
    }

    // --- Orphan pass: R4a/b/c (exact call, one-dimension diff) else R6 (fuzzy call) ---
    for orphan in recents.iter().flat_map(|s| s.orphans.iter()) {
        if let Some((i, r4)) = best_r4_candidate(records, orphan) {
            push_reason(&mut reasons, i, r4);
        } else if let Some((i, r6)) = best_r6_candidate(records, orphan, cfg) {
            push_reason(&mut reasons, i, r6);
        }
    }

    // --- Build diagnoses + rollup ---
    let mut report = DiagnosticsReport::default();
    let mut buckets: HashMap<&'static str, ActionBucket> = HashMap::new();

    for (i, r) in records.iter().enumerate() {
        let granted = !r.credit_granted.is_empty();
        let mut rs = std::mem::take(&mut reasons[i]);
        rs.sort_by_key(|x| rank(x.code));

        // Status from confirmation state (+ lag), independent of advisory reasons.
        let recent = (now - r.when_unix as i64) < cfg.lag_secs && (now >= r.when_unix as i64);
        let status = if granted {
            QsoAwardStatus::Credited
        } else if r.award_confirmed {
            QsoAwardStatus::Confirmed
        } else if r.confirmed {
            QsoAwardStatus::ConfirmedWrongSource
        } else if recent {
            QsoAwardStatus::PendingLag
        } else {
            QsoAwardStatus::NeedsAction
        };

        // Lag is a muted COUNT, never a per-QSO row (avoids crowding the list).
        if status == QsoAwardStatus::PendingLag && rs.is_empty() {
            report.pending_lag += 1;
            continue;
        }
        // Credited / cleanly-confirmed QSOs with nothing actionable are not shown.
        if rs.is_empty() {
            continue;
        }

        // Bucket by the top reason's action.
        let kind = bucket_kind(rs[0].code);
        let b = buckets.entry(kind).or_insert_with(|| ActionBucket {
            kind: kind.to_string(),
            count: 0,
            qso_indices: Vec::new(),
        });
        b.count += 1;
        b.qso_indices.push(i);

        report.diagnoses.push(QsoDiagnosis {
            index: i,
            award: "DXCC/WAS".into(),
            status,
            reasons: rs,
        });
    }

    // Stable, leverage-first bucket order.
    let mut bs: Vec<ActionBucket> = buckets.into_values().collect();
    bs.sort_by(|a, b| b.count.cmp(&a.count).then(a.kind.cmp(&b.kind)));
    report.buckets = bs;
    report
}

/// Two records are "field-identical" for duplicate detection — same call/band/
/// mode/state (ignoring rst/freq, which often differ between two real contacts).
fn field_identical(a: &QsoRecord, b: &QsoRecord) -> bool {
    a.call.eq_ignore_ascii_case(&b.call)
        && a.band.eq_ignore_ascii_case(&b.band)
        && mode_class(&a.mode) == mode_class(&b.mode)
        && a.state.as_deref().map(|s| s.to_ascii_uppercase())
            == b.state.as_deref().map(|s| s.to_ascii_uppercase())
}

/// Best exact-call R4 candidate for an orphan: among same-call unconfirmed logged
/// QSOs, the one differing in EXACTLY ONE key dimension (band/mode/day).
fn best_r4_candidate(
    records: &[QsoRecord],
    orphan: &OrphanConfirmation,
) -> Option<(usize, Reason)> {
    let o_call = orphan.call.to_ascii_uppercase();
    let o_band = orphan.band.to_ascii_lowercase();
    let o_mode = orphan.mode.as_str(); // already a mode-CLASS
    let o_day = orphan.when_unix / SECS_PER_DAY;

    let mut best: Option<(usize, usize, ReasonCode)> = None; // (index, diffs, code)
    for (i, r) in records.iter().enumerate() {
        if r.award_confirmed || r.call.to_ascii_uppercase() != o_call {
            continue;
        }
        let (_, band, mode, day) = key_parts(r);
        let band_diff = band != o_band;
        let mode_diff = mode != o_mode;
        let day_diff = day.abs_diff(o_day) > 1; // ±1 already tolerated by reconcile
        let diffs = band_diff as usize + mode_diff as usize + day_diff as usize;
        if diffs != 1 {
            continue; // 0 would've matched; ≥2 is too ambiguous to claim
        }
        let code = if band_diff {
            ReasonCode::R4aBandMismatch
        } else if mode_diff {
            ReasonCode::R4bModeMismatch
        } else {
            ReasonCode::R4cDateMismatch
        };
        if best.is_none_or(|(_, d, _)| diffs < d) {
            best = Some((i, diffs, code));
        }
    }

    best.map(|(i, _, code)| {
        let (found, expected, what) = match code {
            ReasonCode::R4aBandMismatch => (
                records[i].band.clone(),
                orphan.band.clone(),
                "band".to_string(),
            ),
            ReasonCode::R4bModeMismatch => (
                mode_class(&records[i].mode).to_string(),
                orphan.mode.clone(),
                "mode".to_string(),
            ),
            _ => ("your logged date".into(), "the confirmed date".into(), "date".into()),
        };
        (
            i,
            Reason {
                code,
                confidence: Confidence::Confident,
                explanation: format!(
                    "{} confirmed on a different {what} than your log — fix the {what} so it matches.",
                    orphan.call
                ),
                action: Action::FixField {
                    field: what.to_ascii_uppercase(),
                    found,
                    expected,
                },
            },
        )
    })
}

/// Best fuzzy-call R6 candidate: an unconfirmed logged QSO on the SAME band+mode+
/// day whose call is within the edit-distance cap of the orphan's call.
fn best_r6_candidate(
    records: &[QsoRecord],
    orphan: &OrphanConfirmation,
    cfg: &DiagCfg,
) -> Option<(usize, Reason)> {
    let o_call = orphan.call.to_ascii_uppercase();
    if o_call.len() < cfg.busted_min_call_len {
        return None;
    }
    let o_band = orphan.band.to_ascii_lowercase();
    let o_mode = orphan.mode.as_str();
    let o_day = orphan.when_unix / SECS_PER_DAY;

    let mut best: Option<(usize, usize)> = None; // (index, distance)
    for (i, r) in records.iter().enumerate() {
        if r.award_confirmed {
            continue;
        }
        let (call, band, mode, day) = key_parts(r);
        if band != o_band || mode != o_mode || day.abs_diff(o_day) > 1 {
            continue;
        }
        if call == o_call || call.len() < cfg.busted_min_call_len {
            continue; // exact match wouldn't be an orphan; guard short calls
        }
        let d = osa_distance(&call, &o_call);
        if d >= 1 && d <= cfg.busted_max_dist && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| {
        (
            i,
            Reason {
                code: ReasonCode::R6BustedCall,
                confidence: Confidence::Likely,
                explanation: format!(
                    "Possible busted call: you logged {}, the confirmation is for {} — if you mis-typed, correct it and re-sync.",
                    records[i].call, orphan.call
                ),
                action: Action::CorrectBustedCall {
                    logged: records[i].call.clone(),
                    suggested: orphan.call.clone(),
                },
            },
        )
    })
}

fn bucket_kind(code: ReasonCode) -> &'static str {
    use ReasonCode::*;
    match code {
        R3WrongSource => "Confirmed elsewhere — not ARRL-eligible (get LoTW/paper)",
        R4aBandMismatch | R4bModeMismatch | R4cDateMismatch => {
            "Field mismatch blocking a confirmation"
        }
        R4dMissingState => "Missing STATE for WAS",
        R6BustedCall => "Possible busted callsign",
        R7Duplicate => "Possible duplicate log entry",
        _ => "Needs action",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(call: &str, band: &str, mode: &str, day: u64) -> QsoRecord {
        QsoRecord {
            call: call.into(),
            grid: None,
            state: None,
            band: band.into(),
            freq_mhz: 14.074,
            mode: mode.into(),
            rst_sent: None,
            rst_rcvd: None,
            when_unix: day * SECS_PER_DAY + 3600,
            confirmed: false,
            award_confirmed: false,
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
        }
    }
    fn orphan(call: &str, band: &str, mode_cls: &str, day: u64) -> OrphanConfirmation {
        OrphanConfirmation {
            call: call.into(),
            band: band.into(),
            mode: mode_cls.into(),
            when_unix: day * SECS_PER_DAY + 3600,
            reason: String::new(),
        }
    }
    // "now" far enough ahead that day-20000 QSOs aren't "recent lag".
    const NOW: i64 = 20_100 * 86_400;

    fn diag(
        records: &[QsoRecord],
        ents: &[Option<String>],
        orphans: Vec<OrphanConfirmation>,
    ) -> DiagnosticsReport {
        let summary = ReconcileSummary {
            orphans,
            ..Default::default()
        };
        diagnose(records, ents, &[&summary], NOW, &DiagCfg::default())
    }

    #[test]
    fn r3_flags_non_award_source_only() {
        let mut r = rec("DL1ABC", "20m", "FT8", 20_000);
        r.confirmed = true; // eQSL-grade: confirmed, not award_confirmed
        let rep = diag(&[r], &[None], vec![]);
        let d = &rep.diagnoses[0];
        assert_eq!(d.status, QsoAwardStatus::ConfirmedWrongSource);
        assert_eq!(d.reasons[0].code, ReasonCode::R3WrongSource);
        assert_eq!(d.reasons[0].confidence, Confidence::Confident);
    }

    #[test]
    fn award_confirmed_qso_is_not_flagged() {
        let mut r = rec("DL1ABC", "20m", "FT8", 20_000);
        r.award_confirmed = true;
        r.confirmed = true;
        let rep = diag(&[r], &[None], vec![]);
        assert!(
            rep.diagnoses.is_empty(),
            "a clean award-confirmed QSO is not a problem"
        );
    }

    #[test]
    fn r4d_fires_only_for_us_family_missing_state() {
        // US QSO, award-confirmed, no STATE → R4d.
        let mut us = rec("W1AW", "20m", "FT8", 20_000);
        us.award_confirmed = true;
        // DX QSO, award-confirmed, no STATE → must NOT fire (the major bug guard).
        let mut dx = rec("DL1ABC", "20m", "FT8", 20_000);
        dx.award_confirmed = true;
        let rep = diag(
            &[us, dx],
            &[Some("United States".into()), Some("Germany".into())],
            vec![],
        );
        assert_eq!(rep.diagnoses.len(), 1, "only the US QSO is flagged");
        assert_eq!(rep.diagnoses[0].index, 0);
        assert_eq!(
            rep.diagnoses[0].reasons[0].code,
            ReasonCode::R4dMissingState
        );
    }

    #[test]
    fn r4d_satisfied_when_state_present() {
        let mut us = rec("W1AW", "20m", "FT8", 20_000);
        us.award_confirmed = true;
        us.state = Some("CT".into());
        let rep = diag(&[us], &[Some("United States".into())], vec![]);
        assert!(rep.diagnoses.is_empty());
    }

    #[test]
    fn r4a_band_mismatch_from_orphan() {
        // Logged 20m unconfirmed; the confirmation orphan is for 40m, same call/mode/day.
        let r = rec("W1AW", "20m", "FT8", 20_000);
        let rep = diag(
            &[r],
            &[None],
            vec![orphan("W1AW", "40m", "Digital", 20_000)],
        );
        let d = &rep.diagnoses[0];
        assert_eq!(d.reasons[0].code, ReasonCode::R4aBandMismatch);
    }

    #[test]
    fn r4b_uses_mode_class_not_raw_mode() {
        // Logged FT8 (Digital). An orphan with the SAME class "Digital" must NOT
        // fire R4b (it's the same class) — only a true class diff (e.g. "Phone").
        let r = rec("W1AW", "20m", "FT8", 20_000);
        let same = diag(
            &[r.clone()],
            &[None],
            vec![orphan("W1AW", "20m", "Digital", 20_000)],
        );
        assert!(
            same.diagnoses.is_empty(),
            "same mode-class is not a mismatch (raw FT8 vs class Digital)"
        );
        let cross = diag(&[r], &[None], vec![orphan("W1AW", "20m", "Phone", 20_000)]);
        assert_eq!(
            cross.diagnoses[0].reasons[0].code,
            ReasonCode::R4bModeMismatch
        );
    }

    #[test]
    fn r6_busted_call_fuzzy_match() {
        // Logged W1AW; the confirmation is for W1AX (edit distance 1), same band/mode/day.
        let r = rec("W1AW", "20m", "FT8", 20_000);
        let rep = diag(
            &[r],
            &[None],
            vec![orphan("W1AX", "20m", "Digital", 20_000)],
        );
        let d = &rep.diagnoses[0];
        assert_eq!(d.reasons[0].code, ReasonCode::R6BustedCall);
        assert_eq!(d.reasons[0].confidence, Confidence::Likely);
    }

    #[test]
    fn r4_preferred_over_r6_when_exact_call_exists() {
        // Exact-call band-mismatch candidate must win over any fuzzy R6.
        let r = rec("W1AW", "20m", "FT8", 20_000);
        let rep = diag(
            &[r],
            &[None],
            vec![orphan("W1AW", "40m", "Digital", 20_000)],
        );
        assert_eq!(
            rep.diagnoses[0].reasons[0].code,
            ReasonCode::R4aBandMismatch
        );
    }

    #[test]
    fn r7_possible_duplicate() {
        // Two identical same-key contacts; one confirmed, one not → the unconfirmed
        // one is a possible duplicate.
        let a = rec("W1AW", "20m", "FT8", 20_000); // unconfirmed
        let mut b = rec("W1AW", "20m", "FT8", 20_000);
        b.award_confirmed = true; // confirmed twin
        let rep = diag(&[a, b], &[None, None], vec![]);
        let d = rep
            .diagnoses
            .iter()
            .find(|d| d.index == 0)
            .expect("dup row");
        assert_eq!(d.reasons[0].code, ReasonCode::R7Duplicate);
        assert!(matches!(
            d.reasons[0].action,
            Action::MergeDuplicate { other_index: 1 }
        ));
    }

    #[test]
    fn recent_unconfirmed_is_lag_not_a_row() {
        // A QSO worked ~1 day ago, unconfirmed, no orphan → counted as lag, not listed.
        let day = (NOW / 86_400) as u64 - 1; // yesterday (within the 14-day lag window)
        let r = rec("W1AW", "20m", "FT8", day);
        let rep = diag(&[r], &[None], vec![]);
        assert!(rep.diagnoses.is_empty());
        assert_eq!(rep.pending_lag, 1);
    }

    #[test]
    fn plain_old_unconfirmed_without_evidence_is_not_listed_in_1a() {
        // Old, unconfirmed, no orphan, no upload-state (1b) → 1a can't diagnose it.
        let r = rec("W1AW", "20m", "FT8", 20_000);
        let rep = diag(&[r], &[None], vec![]);
        assert!(rep.diagnoses.is_empty());
        assert_eq!(rep.pending_lag, 0);
    }

    #[test]
    fn rollup_buckets_group_by_top_action() {
        let mut e1 = rec("DL1A", "20m", "FT8", 20_000);
        e1.confirmed = true; // R3
        let mut e2 = rec("G3X", "20m", "FT8", 20_000);
        e2.confirmed = true; // R3
        let rep = diag(&[e1, e2], &[None, None], vec![]);
        let b = rep
            .buckets
            .iter()
            .find(|b| b.kind.contains("ARRL-eligible"))
            .unwrap();
        assert_eq!(b.count, 2);
    }

    #[test]
    fn empty_log_and_no_reconcile_are_safe() {
        let rep = diagnose(&[], &[], &[], NOW, &DiagCfg::default());
        assert!(rep.diagnoses.is_empty() && rep.buckets.is_empty());
    }

    #[test]
    fn osa_distance_basics() {
        assert_eq!(osa_distance("W1AW", "W1AW"), 0);
        assert_eq!(osa_distance("W1AW", "W1AX"), 1); // substitution
        assert_eq!(osa_distance("W1AW", "W1WA"), 1); // transposition
        assert_eq!(osa_distance("K9XYZ", "W1AW"), 5);
    }
}
