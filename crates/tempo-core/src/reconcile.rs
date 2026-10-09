//! Reconcile a confirmation/credit report against the local log — the offline
//! core of "clean LoTW sync".
//!
//! A report is just parsed ADIF [`QsoRecord`]s (the same bytes a future live LoTW
//! adapter will download). [`reconcile`] matches each incoming record to a logged
//! QSO and **monotonically** upgrades its confirmation + credit state, then
//! reports confirmations that match **no** logged QSO (the "why is this missing?"
//! diagnostic). Pure: no network, no DXCC resolution, and no merge fabricates or revokes. The
//! one removal is [`check::uncheck`]: a confirmation an earlier matcher misplaced, taken off only
//! once a fresh download shows it and the operator ticks it.

use crate::logbook::{datetime_utc, QsoRecord, RecordId, StoredRecord};
use std::borrow::Borrow;
use std::collections::HashMap;

pub mod check;

/// A confirmation in the report with no matching logged QSO — a log gap, callsign
/// typo, or band/time mismatch worth surfacing (never auto-added).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanConfirmation {
    pub call: String,
    pub band: String,
    pub mode: String,
    pub when_unix: u64,
    pub reason: String,
}

/// What a [`reconcile`] changed (idempotent: a second run yields all-zero counts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileSummary {
    /// Incoming records that matched a logged QSO.
    pub matched: usize,
    /// Matched QSOs newly upgraded to award-eligible confirmed (LoTW/paper).
    pub newly_confirmed: usize,
    /// Matched QSOs newly flipped to confirmed by ANY channel (incl. eQSL). For a
    /// LoTW/paper confirmation of an unconfirmed QSO this is also counted; for an
    /// eQSL confirmation it is the only count (eQSL never bumps `newly_confirmed`).
    pub newly_confirmed_any: usize,
    /// Matched QSOs that gained at least one new granted-credit award.
    pub newly_credited: usize,
    /// Matched QSOs that gained at least one new submitted/applied award.
    pub newly_submitted: usize,
    /// Confirmations with no matching logged QSO.
    pub orphans: Vec<OrphanConfirmation>,
}

/// CW / Phone / Digital bucket for tolerant matching — LoTW reports vary in
/// submode naming, so we match on the mode *class* (and on time within a window).
///
/// The voice vocabulary MUST stay in step with `propagation::ModeClass::from_adif`
/// (tempo-core can't depend on propagation, so this is kept aligned by hand and by
/// test on both sides). When the two disagree, a QSO gets two identities inside one
/// crate: classed Phone by the awards matrix but Digital here, so its LoTW
/// confirmation never matches and the contact sits forever as a phantom
/// "worked, needs a confirmation" need. `PH` is the N3FJP phone token, present in
/// real imported logs. `""` stays "Other" — a mode-less row must not silently
/// match a digital confirmation.
pub fn mode_class(mode: &str) -> &'static str {
    match mode.trim().to_ascii_uppercase().as_str() {
        "CW" => "CW",
        "SSB" | "USB" | "LSB" | "AM" | "FM" | "PHONE" | "PH" | "DV" | "C4FM" | "DIGITALVOICE"
        | "DSTAR" | "FUSION" | "M17" | "FREEDV" => "Phone",
        "" => "Other",
        _ => "Digital", // FT8/FT4/RTTY/JT*/MFSK/PSK/FT1/DX1/… → data
    }
}

/// How far apart in time a report row and the logged contact it lands on may be: LoTW only
/// matches two operators' records when their times agree within 30 minutes, and QRZ calls two
/// records within 30 minutes of each other one contact. A row further than this from every
/// logged contact with the station is none of them.
const MATCH_WINDOW_SECS: u64 = 30 * 60;

type Key = (String, String, &'static str, u64);
fn key(r: &QsoRecord) -> Key {
    (
        r.call.to_ascii_uppercase(),
        r.band.to_ascii_lowercase(),
        mode_class(&r.mode),
        r.when_unix / 86_400,
    )
}

/// Add any codes in `incoming` missing from `existing` (kept sorted+deduped).
/// Returns true if anything new was added.
fn merge_codes(existing: &mut Vec<String>, incoming: &[String]) -> bool {
    let mut changed = false;
    for c in incoming {
        if !existing.contains(c) {
            existing.push(c.clone());
            changed = true;
        }
    }
    if changed {
        existing.sort();
        existing.dedup();
    }
    changed
}

fn fmt_day(unix: u64) -> String {
    let (y, m, d, ..) = datetime_utc(unix);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Identity of one of OUR OWN records: the same call / band / mode-CLASS as [`key`],
/// but the **exact** contact second instead of the UTC day. Used only when the
/// "incoming" rows came from our own [`crate::logbook::Logbook::save`] — there both
/// sides carry the same `when_unix` to the second, so no tolerance is wanted and any
/// tolerance is a hazard (see [`merge_own_disk`]). Mode stays CLASS-keyed because
/// spelling is the one thing that legitimately drifts between our own writes
/// (a row stored `MFSK` by an older build reads back `FT4` today); two genuinely
/// distinct contacts with one station on one band in one mode class cannot share a
/// second, so class costs no discrimination here.
///
/// That last sentence holds ONLY while the second is a MEASURED one. A row whose
/// source carried no `TIME_ON` parks at 00:00:00 UTC and is flagged
/// [`time_known`](QsoRecord::time_known)`= false`; every date-only contact of that
/// day then shares this key, and the key stops being an identity. [`take_own_disk`]
/// is where that is handled — never by loosening the key.
fn exact_key(r: &QsoRecord) -> Key {
    (
        r.call.to_ascii_uppercase(),
        r.band.to_ascii_lowercase(),
        mode_class(&r.mode),
        r.when_unix,
    )
}

/// Is `b` the same CONTACT as `a` — the same record, possibly with the sync state
/// [`apply_match`] merges moved on?
///
/// Compares everything that merge does NOT own, which for two copies of one of OUR
/// OWN records is everything that cannot legitimately differ between instances.
/// Written as "clone, blank what merge owns, compare" rather than a field list on
/// purpose: a field added to `QsoRecord` is compared by default, so the check can
/// never silently rot. If a future field is added to `apply_match` and not blanked
/// here, two copies of one record read as two contacts — a visible duplicate, which
/// is the failure this module already prefers over a silent loss.
///
/// Two fields are blanked on top of that because they are OURS and still differ
/// across our own write/read cycle, so neither can discriminate anything:
///
/// - `mode`, whose SPELLING legitimately drifts between our own writes
///   (`MFSK` ↔ `FT4`) — [`exact_key`] already pins the mode class;
/// - `time_known`, which is not a property of the contact at all but of the ADIF ROW
///   it was read from, inferred by the fabricated-midnight migration in
///   [`crate::logbook`]. A contact logged natively at exactly 00:00:00 with no
///   off-time is written `time_known = true` and parses back `false` — the
///   documented false-negative of that heuristic. Compare it and a record stops
///   being equal to its own round-trip.
fn same_contact(a: &QsoRecord, b: &QsoRecord) -> bool {
    fn strip(r: &QsoRecord) -> QsoRecord {
        let mut r = r.clone();
        // Everything `apply_match` writes.
        r.confirmed = false;
        r.award_confirmed = false;
        r.qsl_rcvd = Default::default();
        r.qsl_sent = Default::default();
        r.credit_granted.clear();
        r.credit_submitted.clear();
        r.upload = Default::default();
        r.state = None;
        r.country = None;
        // ...plus the two that do not survive our own round-trip intact.
        r.mode.clear();
        r.time_known = false;
        // The id is not part of what makes this the same contact: the two sides may have
        // assigned different ones, and pairing is exactly what lets them converge
        // (`adopt_id`).
        r.id = None;
        r
    }
    strip(a) == strip(b)
}

/// Consume-once lookup for [`merge_own_disk`]: which local record IS this row of our
/// own file?
///
/// Both sides came out of our own `save()`, so the answer is normally "the one with
/// the same [`exact_key`]", and normally there is exactly one. When there is more
/// than one, order alone cannot tell them apart — memory and file order diverge
/// precisely when another instance appended something we never loaded — so prefer
/// the candidate that IS this record by content ([`same_contact`]).
///
/// The fallback when nothing matches by content is the whole distinction between the
/// two ways a bucket comes to hold twins:
///
/// - a MEASURED second (`time_known`) is an identity, so a content mismatch means the
///   record changed under us (another instance edited it) rather than that we are
///   looking at a different contact. Pair by order, as before, and let the monotonic
///   merge do its work.
/// - a FABRICATED second — a date-only import parked at 00:00:00 — is not an identity
///   at all. Pairing by order there is the mis-pairing this matcher exists to end: it
///   folds one contact's confirmation onto another and drops the row we never held.
///   A row we cannot recognise is a row we do not have, which is exactly what the
///   recovery appends.
fn take_own_disk<R: Borrow<QsoRecord>>(
    buckets: &mut HashMap<Key, Vec<usize>>,
    local: &[R],
    inc: &QsoRecord,
) -> Option<usize> {
    let bucket = buckets.get_mut(&exact_key(inc))?;
    // Buckets are reversed, so scanning from the BACK consumes oldest-first, the
    // same order `pop()` does.
    if let Some(pos) = bucket
        .iter()
        .rposition(|&i| same_contact(local[i].borrow(), inc))
    {
        return Some(bucket.remove(pos));
    }
    if inc.time_known {
        bucket.pop()
    } else {
        None
    }
}

/// Index local QSOs by `key_of`; each bucket reversed so `pop()` consumes in log
/// order (oldest first), so two same-key contacts reconcile against distinct rows.
fn build_buckets_by<R: Borrow<QsoRecord>>(
    local: &[R],
    key_of: impl Fn(&QsoRecord) -> Key,
) -> HashMap<Key, Vec<usize>> {
    let mut buckets: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, r) in local.iter().enumerate() {
        buckets.entry(key_of(r.borrow())).or_default().push(i);
    }
    for v in buckets.values_mut() {
        v.reverse();
    }
    buckets
}

/// Index local QSOs by the fuzzy report key ([`key`]).
fn build_buckets<R: Borrow<QsoRecord>>(local: &[R]) -> HashMap<Key, Vec<usize>> {
    build_buckets_by(local, key)
}

/// How near a report row is to a logged contact, as [`pair_report`] ranks pairs (smallest
/// first), or `None` when the row cannot be that contact.
///
/// Two measured times: the seconds between them, at most [`MATCH_WINDOW_SECS`]. A side with
/// no measured time (`time_known == false`: a date-only import parked at 00:00 UTC) has no
/// time to be near, so it keeps the day rule this matcher had before it compared times (the
/// same UTC day, else the day either side), and it ranks after every pair of two measured
/// times, because a fabricated midnight is no evidence of nearness.
fn closeness(local: &QsoRecord, inc: &QsoRecord) -> Option<(bool, u64)> {
    if local.time_known && inc.time_known {
        let apart = local.when_unix.abs_diff(inc.when_unix);
        (apart <= MATCH_WINDOW_SECS).then_some((false, apart))
    } else {
        let days = (local.when_unix / 86_400).abs_diff(inc.when_unix / 86_400);
        (days <= 1).then_some((true, days))
    }
}

/// Pair each report row with the logged contact it confirms: `out[j]` is the index in `local`
/// that row `j` lands on, or `None` when it is none of them. Consume-once both ways: no contact
/// takes two rows, and no row two contacts.
///
/// A row and a contact are candidates when they share call, band and mode class and their
/// times are within [`MATCH_WINDOW_SECS`] ([`closeness`]). The day either side of the row's own
/// is searched too, so a pair split by midnight (23:58 and 00:03) is still a pair. Every
/// candidate pair in the report is then settled NEAREST FIRST: a row never takes a contact that
/// another row is nearer to, and the order the service lists its rows in (LoTW's is newest
/// match first) decides nothing. Equal distances go to the earlier row, then to the earlier
/// contact in the log.
///
/// Up to 1.17.0 a row took the oldest contact on its own UTC day (else the day either side)
/// without looking at the time, so of two contacts with one station on one band in one day,
/// the confirmation of the later one landed on the earlier, for good (#400).
fn pair_report<R: Borrow<QsoRecord>>(local: &[R], incoming: &[QsoRecord]) -> Vec<Option<usize>> {
    let buckets = build_buckets(local);
    let mut candidates = Vec::new();
    for (j, inc) in incoming.iter().enumerate() {
        let (call, band, mc, day) = key(inc);
        for d in [day.wrapping_sub(1), day, day + 1] {
            let Some(bucket) = buckets.get(&(call.clone(), band.clone(), mc, d)) else {
                continue;
            };
            for &i in bucket {
                if let Some(near) = closeness(local[i].borrow(), inc) {
                    candidates.push((near, j, i));
                }
            }
        }
    }
    candidates.sort_unstable();
    let mut taken = vec![false; local.len()];
    let mut out = vec![None; incoming.len()];
    for (_, j, i) in candidates {
        if out[j].is_none() && !taken[i] {
            out[j] = Some(i);
            taken[i] = true;
        }
    }
    out
}

/// Monotonically upgrade a matched local record from an incoming report row (only
/// ever adds confirmation/credit) and tally the change in `sum`.
/// (`pub(crate)`: [`crate::logbook::Logbook::import_adif`] upgrades a deduped row
/// through this same merge, so "import" and "sync" can't disagree about what a
/// confirmation row means.)
pub(crate) fn apply_match(rec: &mut QsoRecord, inc: &QsoRecord, sum: &mut ReconcileSummary) {
    sum.matched += 1;
    // `newly_confirmed_any` counts a plain confirmed flip from any channel (incl.
    // eQSL/QRZ); `newly_confirmed` counts only award-grade (LoTW/paper) upgrades.
    // An award confirmation of a previously unconfirmed QSO bumps both.
    if inc.confirmed && !rec.confirmed {
        rec.confirmed = true;
        sum.newly_confirmed_any += 1;
    }
    if inc.award_confirmed && !rec.award_confirmed {
        rec.award_confirmed = true;
        rec.confirmed = true;
        sum.newly_confirmed += 1;
    }
    // Per-source truth merges monotonically alongside the derived booleans (which
    // channel confirmed — LoTW vs card vs eQSL vs QRZ).
    rec.qsl_rcvd.merge(inc.qsl_rcvd);
    if merge_codes(&mut rec.credit_granted, &inc.credit_granted) {
        sum.newly_credited += 1;
    }
    if merge_codes(&mut rec.credit_submitted, &inc.credit_submitted) {
        sum.newly_submitted += 1;
    }
    // A granted award is no longer merely "applied" — drop it from the submitted
    // set so applied/granted stay mutually exclusive.
    if !rec.credit_submitted.is_empty() {
        let granted = rec.credit_granted.clone();
        rec.credit_submitted.retain(|c| !granted.contains(c));
    }
    // Location enrich: a report often carries STATE/COUNTRY the logged QSO lacked —
    // fill it so WAS/DXCC can credit it. Monotonic: never overwrites an existing value.
    if rec.state.is_none() {
        if let Some(st) = &inc.state {
            rec.state = Some(st.clone());
        }
    }
    if rec.country.is_none() {
        if let Some(c) = &inc.country {
            rec.country = Some(c.clone());
        }
    }
    // Outbound state — merged so a two-instance shared log doesn't lose an upload stamp or a
    // QSL-sent mark one instance wrote when the other rewrites the whole file. (A LoTW report
    // carries neither, so this is inert for the report path; it matters for recover-from-disk.)
    rec.upload.merge_recent(&inc.upload);
    rec.qsl_sent.merge(&inc.qsl_sent);
}

/// Merge a confirmation/credit report into `local`, in place. Each incoming
/// record lands on at most one logged QSO, the one nearest its time within 30
/// minutes (`pair_report`), and no QSO takes two records (so two same-day/band/mode
/// contacts with one call reconcile against two distinct report rows). Unmatched
/// confirmations become orphans (a "why is this missing?" diagnostic) — they are
/// NOT added, because a LoTW/eQSL confirmation of a QSO we never logged is a gap to
/// surface, not a contact to fabricate.
pub fn reconcile<R: StoredRecord>(local: &mut [R], incoming: &[QsoRecord]) -> ReconcileSummary {
    let pairs = pair_report(local, incoming);
    let mut sum = ReconcileSummary::default();
    for (inc, pair) in incoming.iter().zip(pairs) {
        match pair {
            Some(i) => apply_match(local[i].write(), inc, &mut sum),
            // Only a row that actually carries a confirmation/credit is a
            // meaningful "missing" diagnostic; a plain unconfirmed QSO row is not.
            None if inc.confirmed
                || inc.award_confirmed
                || !inc.credit_granted.is_empty()
                || !inc.credit_submitted.is_empty() =>
            {
                let mc = mode_class(&inc.mode);
                let call_u = inc.call.to_ascii_uppercase();
                let band_l = inc.band.to_ascii_lowercase();
                let (.., h, mi, _) = datetime_utc(inc.when_unix);
                let reason = format!(
                    "no logged QSO with {call_u} on {band_l} ({mc}) within 30 minutes of {} \
                     {h:02}{mi:02}Z",
                    fmt_day(inc.when_unix),
                );
                sum.orphans.push(OrphanConfirmation {
                    call: call_u,
                    band: band_l,
                    mode: mc.to_string(),
                    when_unix: inc.when_unix,
                    reason,
                });
            }
            None => {}
        }
    }
    sum
}

/// Two-way merge of a DOWNLOADED logbook (a QRZ Logbook FETCH — the operator's own
/// book pulled back down). ONE consume-once pass paired exactly as [`reconcile`] pairs
/// (`pair_report`: call / band / mode-CLASS, the nearest contact within 30 minutes): a row
/// that pairs with a local QSO upgrades its confirmation monotonically; a row that pairs
/// with NOTHING (a row more than 30 minutes from every logged contact with the station is
/// one) is APPENDED as a genuinely-new QSO, and is never a match for a later row in the
/// same batch. The single shared pairing is the whole point —
/// a separate full-mode import + class-reconcile disagree on "same QSO" whenever the
/// mode spelling differs (local `SSB` vs a re-uploaded `USB`, `FT4` vs `MFSK`), which
/// double-logs the contact. Returns the newly-added records (so the caller persists
/// exactly those) plus the reconcile summary.
///
/// A row confirms with QRZ's own confirmation alone and brings no LoTW upload mark
/// (`qrzs_own_word`), on a contact the log holds and on one the download adds.
pub fn merge_and_add<R: StoredRecord>(
    local: &mut Vec<R>,
    incoming: Vec<QsoRecord>,
) -> (Vec<QsoRecord>, ReconcileSummary) {
    let incoming: Vec<QsoRecord> = incoming.into_iter().map(qrzs_own_word).collect();
    let pairs = pair_report(local, &incoming);
    merge_pass(local, incoming, pairs, |_, _| {})
}

/// What a row of QRZ's book is evidence of: QRZ's own confirmation (`APP_QRZLOG_STATUS`), and
/// none of what the book repeats from other services, its copies of LoTW's, eQSL's and a paper
/// card's confirmation and of LoTW's credit codes. They are not QRZ's word. Taken as evidence, a
/// copy put back a confirmation Logbook ▸ Check confirmations had taken off, with the award credit
/// only LoTW or a card can give. [`check`] judges the book by the same rule.
///
/// Nor is the book LoTW's word on an upload: its `LOTW_QSL_SENT`, or the `APP_TEMPO_UL_LOTW` stamp
/// a Nexus push carried to QRZ, if QRZ hands one back. Either reads as a LoTW upload mark, and a
/// contact holding one is not owed to LoTW, so a copy put back the mark Check confirmations' upload
/// line had taken off and the contact was never uploaded again. The row brings no LoTW mark, and
/// `LOTW_QSL_SENT` goes with it, or a contact the download adds would get the mark back whenever
/// its ADIF is read again (an export or a backup imported). Only LoTW's mark goes: the stamps of
/// QRZ's, Club Log's and eQSL's uploads a row may hand back still merge, as they decide what the
/// catch-up sweep sends those services. The rest of the row is as QRZ sent it.
fn qrzs_own_word(mut row: QsoRecord) -> QsoRecord {
    let qrz = row.qsl_rcvd.qrz;
    row.qsl_rcvd = Default::default();
    row.qsl_rcvd.qrz = qrz;
    row.confirmed = row.qsl_rcvd.any();
    row.award_confirmed = row.qsl_rcvd.award();
    row.credit_granted.clear();
    row.credit_submitted.clear();
    row.upload.lotw = None;
    row.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
    row
}

/// Two-way merge of OUR OWN on-disk log back into memory — the two-instance recovery
/// behind [`crate::logbook::Logbook::reconcile_disk`]. Same consume-once shape as
/// [`merge_and_add`] (match → monotonic upgrade, no match → append), but matched on
/// [`exact_key`]: call / band / mode-class / **the exact contact second**.
///
/// # Why not the report matcher
///
/// [`merge_and_add`]'s match is deliberately tolerant — the nearest contact within 30
/// minutes, across midnight — because a report's timestamps can legitimately differ
/// from ours. The rows here are not a report: they came out of our
/// own `save()` and carry our own `when_unix` to the second. Applied to them a
/// tolerance is not slack, it is a mis-pairing: with two contacts with one station on
/// one band close together (routine FT8), whenever file order and memory order diverge —
/// another instance appended a QSO we never loaded — a tolerant key can pair the wrong
/// two rows. Observed with the day-keyed matcher this function once shared: memory
/// holding only the 18:00 contact, disk holding 06:00
/// (award-confirmed) and 18:00, recovered to *two* 18:00 rows with the confirmation on
/// the wrong contact and the 06:00 QSO gone. Exact identity cannot pair them; a row
/// that fails to match is genuinely a row we do not hold, which is exactly what the
/// recovery exists to append.
///
/// # What it deliberately does NOT do
///
/// It cannot see an EDIT. A record has no stable id in the ADIF, so a correction
/// another instance made to a keyed field — the mis-logged time 12:00 → 12:05, a
/// busted call — carries a different [`exact_key`] and is appended as a contact we do
/// not hold. The operator is then holding two rows for one QSO, permanently and in
/// both copies of the file, and both are eligible for upload. **That is the chosen
/// trade, not an oversight**: the only key that could pair 12:00 with 12:05 is the
/// fuzzy one this matcher replaced, and it pays for the duplicate by silently
/// REVERTING the correction on the next full rewrite. A duplicate is on screen and one
/// delete away; a reverted edit is invisible. See the contract on
/// `StationCore::recover_external_appends`, which is the caller that must run first.
///
/// The same applies to an edit that keeps the key and changes a field the merge does
/// not own (a `COMMENT`, a `NAME`): the rows still pair, and OUR copy — the older
/// text — is what the rewrite writes back.
pub fn merge_own_disk<R: StoredRecord>(
    local: &mut Vec<R>,
    incoming: Vec<QsoRecord>,
) -> (Vec<QsoRecord>, ReconcileSummary) {
    let mut buckets = build_buckets_by(local, exact_key);
    let pairs = incoming
        .iter()
        .map(|inc| take_own_disk(&mut buckets, local, inc))
        .collect();
    merge_pass(local, incoming, pairs, adopt_id)
}

/// Our memory and our own file hold one row under two ids (two instances each gave it one):
/// both settle on [`RecordId::adopt`], so the instances converge without talking.
fn adopt_id(local: &mut QsoRecord, disk: &QsoRecord) {
    local.id = match (local.id, disk.id) {
        (Some(a), Some(b)) => Some(RecordId::adopt(a, b)),
        (a, b) => a.or(b),
    };
}

/// The shared body of the two-way merges: each incoming row upgrades the local QSO that
/// `pairs` names for it, monotonically, or is appended as new. The pairing is the whole
/// difference between them — a report's nearest-in-time match vs. our own exact identity —
/// and it is settled before anything is appended.
fn merge_pass<R: StoredRecord>(
    local: &mut Vec<R>,
    incoming: Vec<QsoRecord>,
    pairs: Vec<Option<usize>>,
    on_pair: impl Fn(&mut QsoRecord, &QsoRecord),
) -> (Vec<QsoRecord>, ReconcileSummary) {
    let mut sum = ReconcileSummary::default();
    let mut added = Vec::new();
    for (inc, pair) in incoming.into_iter().zip(pairs) {
        match pair {
            Some(i) => {
                let held = local[i].write();
                apply_match(held, &inc, &mut sum);
                on_pair(held, &inc);
            }
            None => {
                // New contact from the download — append it. It is never a match for a later
                // row in this batch (the pairing was settled first): a later same-key row is a
                // DISTINCT QSO (consume-once, exactly like `reconcile`). Matching it to the
                // appended slot broke re-sync idempotency — the slot got popped by a same-key
                // row, leaving its twin to re-append on every fetch (phantom-duplicate
                // accretion). Because the pairing is rebuilt from the grown log next sync, each
                // row then pairs with its own match and nothing re-adds.
                added.push(inc.clone());
                local.push(R::from(inc));
            }
        }
    }
    (added, sum)
}

/// Promote a logged QSO's own LoTW upload state to `Accepted` when it appears in
/// the **own-QSO report** (LoTW's `qso_qsl=no` — the records LoTW holds from your
/// uploads). That membership is proof LoTW has your side on
/// file, which is exactly what turns a "Pending" (awaiting echo) or never-marked
/// upload into the "waiting on the other operator" (R2) state, and clears a false
/// "never uploaded" (R1) for QSOs uploaded out-of-band (e.g. plain TQSL).
///
/// Paired exactly as [`reconcile`] pairs a confirmation report (`pair_report`: the nearest
/// contact with the same call, band and mode class within 30 minutes, settled nearest first,
/// no contact taking two rows). Up to 1.17.0 a row took the oldest contact on its UTC day
/// without comparing times, so the upload of the later of two contacts with one station on one
/// band that day marked the earlier one `Accepted`, and a contact marked `Accepted` is no
/// longer owed to LoTW, so it was never uploaded. What it writes is an upload state, never a
/// confirmation. Award-confirmed QSOs keep their place in the pairing, so their own row never
/// lands on a neighbour, but are never written. Idempotent: an already-Accepted/Duplicate QSO
/// is re-stamped harmlessly and not counted. Returns the number *newly* promoted.
pub fn promote_own_echo<R: StoredRecord>(
    local: &mut [R],
    own: &[QsoRecord],
    when_unix: i64,
) -> usize {
    use crate::logbook::{UploadOutcome, UploadStatus};

    let mut promoted = 0usize;
    for i in pair_report(local, own).into_iter().flatten() {
        let r: &QsoRecord = local[i].borrow();
        if r.award_confirmed {
            continue;
        }
        let already_on_file = matches!(
            r.upload.lotw.as_ref().map(|s| s.outcome),
            Some(UploadOutcome::Accepted) | Some(UploadOutcome::Duplicate)
        );
        local[i].write().upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix,
            detail: None,
        });
        if !already_on_file {
            promoted += 1;
        }
    }
    promoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{QsoRecord, UploadOutcome, UploadState, UploadStatus};

    pub(super) fn rec(call: &str, band: &str, mode: &str, day: u64) -> QsoRecord {
        QsoRecord {
            id: None,
            call: call.into(),
            grid: None,
            country: None,
            state: None,
            band: band.into(),
            freq_mhz: 14.074,
            freq_rx_mhz: None,
            mode: mode.into(),
            rst_sent: None,
            rst_rcvd: None,
            name: None,
            qth: None,
            comment: None,
            notes: None,
            tx_power: None,
            when_unix: day * 86_400 + 3600,
            time_off_unix: None,
            confirmed: false,
            award_confirmed: false,
            qsl_rcvd: Default::default(),
            qsl_sent: Default::default(),
            credit_granted: Vec::new(),
            credit_submitted: Vec::new(),
            upload: Default::default(),
            ota: Default::default(),
            time_known: true,
            dxcc: None,
            prop_mode: None,
            sat_name: None,
            operator: None,
            my_grid: None,
            my_rig: None,
            station_callsign: None,
            extra: Vec::new(),
            contest: None,
        }
    }

    pub(super) fn with_lotw(mut r: QsoRecord, outcome: UploadOutcome) -> QsoRecord {
        r.upload = UploadState {
            lotw: Some(UploadStatus {
                outcome,
                when_unix: 0,
                detail: None,
            }),
            ..Default::default()
        };
        r
    }

    pub(super) fn lotw_outcome(r: &QsoRecord) -> Option<UploadOutcome> {
        r.upload.lotw.as_ref().map(|s| s.outcome)
    }

    /// A W1AW 20m FT8 contact, or report row, at `h:m` UTC on `day`.
    pub(super) fn w1aw_at(day: u64, h: u64, m: u64) -> QsoRecord {
        let mut r = rec("W1AW", "20m", "FT8", day);
        r.when_unix = day * 86_400 + h * 3600 + m * 60;
        r
    }

    /// `r` as a LoTW confirmation granting `credit`, so a test can tell BY VALUE which row a
    /// contact received.
    fn confirms(mut r: QsoRecord, credit: &str) -> QsoRecord {
        r.confirmed = true;
        r.award_confirmed = true;
        r.credit_granted = vec![credit.into()];
        r
    }

    #[test]
    fn own_echo_promotes_pending_to_accepted() {
        let mut log = vec![with_lotw(
            rec("W1AW", "20m", "FT8", 20_000),
            UploadOutcome::Pending,
        )];
        // Own-QSO report row (submode differs: MFSK→Digital), ±0 day.
        let own = vec![rec("w1aw", "20M", "MFSK", 20_000)];
        let n = promote_own_echo(&mut log, &own, 99);
        assert_eq!(n, 1);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
        assert_eq!(log[0].upload.lotw.as_ref().unwrap().when_unix, 99);
    }

    #[test]
    fn own_echo_clears_false_never_uploaded() {
        // A QSO LoTW holds but we never marked (uploaded out-of-band) → Accepted.
        let mut log = vec![rec("W1AW", "20m", "FT8", 20_000)]; // upload state = none
        let own = vec![rec("W1AW", "20m", "FT8", 20_000)];
        let n = promote_own_echo(&mut log, &own, 7);
        assert_eq!(n, 1);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
    }

    #[test]
    fn own_echo_skips_award_confirmed_and_is_consume_once() {
        let mut award = rec("K2AA", "20m", "FT8", 20_000);
        award.award_confirmed = true;
        let mut log = vec![
            award, // must NOT be touched
            with_lotw(rec("W1AW", "20m", "FT8", 20_000), UploadOutcome::Pending),
            with_lotw(rec("W1AW", "20m", "FT8", 20_000), UploadOutcome::Pending), // twin
        ];
        // One own-echo row for the W1AW key → consumes exactly one of the twins.
        let own = vec![rec("W1AW", "20m", "FT8", 20_000)];
        let n = promote_own_echo(&mut log, &own, 1);
        assert_eq!(n, 1, "one own-echo row promotes one twin");
        assert_eq!(lotw_outcome(&log[0]), None, "award-confirmed untouched");
        let promoted = log[1..]
            .iter()
            .filter(|r| lotw_outcome(r) == Some(UploadOutcome::Accepted))
            .count();
        let still_pending = log[1..]
            .iter()
            .filter(|r| lotw_outcome(r) == Some(UploadOutcome::Pending))
            .count();
        assert_eq!((promoted, still_pending), (1, 1));
    }

    #[test]
    fn own_echo_already_accepted_not_double_counted() {
        let mut log = vec![with_lotw(
            rec("W1AW", "20m", "FT8", 20_000),
            UploadOutcome::Accepted,
        )];
        let own = vec![rec("W1AW", "20m", "FT8", 20_000)];
        let n = promote_own_echo(&mut log, &own, 5);
        assert_eq!(n, 0, "already on file — re-stamp is not a new promotion");
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
    }

    #[test]
    fn own_echo_no_match_leaves_state_untouched() {
        let mut log = vec![with_lotw(
            rec("W1AW", "20m", "FT8", 20_000),
            UploadOutcome::Pending,
        )];
        let own = vec![rec("K9XYZ", "40m", "FT8", 19_000)]; // different QSO
        let n = promote_own_echo(&mut log, &own, 1);
        assert_eq!(n, 0);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Pending));
    }

    /// An own-QSO row is proof about ONE upload. It used to promote the oldest contact with the
    /// station that UTC day whatever its time, so the 18:00 upload marked the 06:00 contact
    /// Accepted: that one no longer looked owed and was never uploaded, and 18:00 stayed Pending.
    #[test]
    fn own_echo_promotes_the_contact_nearest_its_time() {
        let mut log = vec![
            with_lotw(w1aw_at(20_000, 6, 0), UploadOutcome::Pending),
            with_lotw(w1aw_at(20_000, 18, 0), UploadOutcome::Pending),
        ];
        assert_eq!(promote_own_echo(&mut log, &[w1aw_at(20_000, 18, 0)], 9), 1);
        assert_eq!(
            lotw_outcome(&log[0]),
            Some(UploadOutcome::Pending),
            "06:00 was not listed"
        );
        assert_eq!(lotw_outcome(&log[1]), Some(UploadOutcome::Accepted));
        // Both inside the window, so only the nearest rule (not the window) picks 18:00.
        let mut log = vec![
            with_lotw(w1aw_at(20_000, 17, 45), UploadOutcome::Pending),
            with_lotw(w1aw_at(20_000, 18, 0), UploadOutcome::Pending),
        ];
        assert_eq!(promote_own_echo(&mut log, &[w1aw_at(20_000, 17, 58)], 9), 1);
        assert_eq!(
            lotw_outcome(&log[0]),
            Some(UploadOutcome::Pending),
            "17:45 is 13 min off"
        );
        assert_eq!(lotw_outcome(&log[1]), Some(UploadOutcome::Accepted));
    }

    #[test]
    fn own_echo_31_minutes_from_every_contact_promotes_nothing() {
        let mut log = vec![with_lotw(w1aw_at(20_000, 10, 0), UploadOutcome::Pending)];
        assert_eq!(promote_own_echo(&mut log, &[w1aw_at(20_000, 10, 31)], 9), 0);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Pending));
        // The edge is inclusive, as the confirmation match's is: 30:00 still pairs.
        assert_eq!(promote_own_echo(&mut log, &[w1aw_at(20_000, 10, 30)], 9), 1);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
    }

    /// Settled nearest first across the whole report, never in the order LoTW lists its rows:
    /// the 10:01 row is A's, so the 10:19 row, though nearer A than B, is B's.
    #[test]
    fn own_echo_settles_the_nearer_row_first() {
        let mut log = vec![
            with_lotw(w1aw_at(20_000, 10, 0), UploadOutcome::Pending), // A
            with_lotw(w1aw_at(20_000, 10, 40), UploadOutcome::Pending), // B
        ];
        let own = [w1aw_at(20_000, 10, 19), w1aw_at(20_000, 10, 1)];
        assert_eq!(promote_own_echo(&mut log, &own, 9), 2);
        assert_eq!(lotw_outcome(&log[1]), Some(UploadOutcome::Accepted), "B");
    }

    /// No contact takes two rows: the 10:02 row is not the 10:00 contact's a second time.
    #[test]
    fn own_echo_never_gives_one_contact_two_rows() {
        let mut log = vec![
            with_lotw(w1aw_at(20_000, 10, 0), UploadOutcome::Pending),
            with_lotw(w1aw_at(20_000, 10, 20), UploadOutcome::Pending),
        ];
        let own = [w1aw_at(20_000, 10, 0), w1aw_at(20_000, 10, 2)];
        assert_eq!(promote_own_echo(&mut log, &own, 9), 2);
        assert_eq!(lotw_outcome(&log[1]), Some(UploadOutcome::Accepted));
    }

    /// A confirmed contact still owns its own row. `qso_qsl=no` lists every upload LoTW holds,
    /// matched or not (`QSL_RCVD` is "Y" or "N"), and leaving confirmed contacts out of the
    /// pairing handed a confirmed contact's row to its unconfirmed neighbour.
    #[test]
    fn own_echo_never_hands_a_confirmed_contacts_row_to_its_neighbour() {
        let mut confirmed = w1aw_at(20_000, 10, 0);
        confirmed.award_confirmed = true;
        let mut log = vec![
            confirmed,
            with_lotw(w1aw_at(20_000, 10, 20), UploadOutcome::Pending),
        ];
        assert_eq!(promote_own_echo(&mut log, &[w1aw_at(20_000, 10, 0)], 9), 0);
        assert_eq!(
            lotw_outcome(&log[0]),
            None,
            "a confirmed contact is still never written"
        );
        assert_eq!(lotw_outcome(&log[1]), Some(UploadOutcome::Pending));
    }

    /// A contact imported with no time of day keeps the day rule here too, as it does for a
    /// confirmation: its 00:00 is no time to be 30 minutes from.
    #[test]
    fn own_echo_a_date_only_contact_still_pairs_on_its_day() {
        let mut date_only = with_lotw(w1aw_at(20_000, 0, 0), UploadOutcome::Pending);
        date_only.time_known = false;
        let mut log = vec![date_only];
        assert_eq!(promote_own_echo(&mut log, &[w1aw_at(20_000, 14, 0)], 9), 1);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
    }

    #[test]
    fn upgrades_matched_qso_monotonically_and_is_idempotent() {
        let mut log = vec![rec("W1AW", "20m", "FT8", 20_000)];
        // Report confirms + grants DXCC for that QSO (submode differs: MFSK→Digital).
        let mut report = rec("w1aw", "20M", "MFSK", 20_000);
        report.award_confirmed = true;
        report.confirmed = true;
        report.credit_granted = vec!["DXCC".into()];

        let s1 = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!(
            (s1.matched, s1.newly_confirmed, s1.newly_credited),
            (1, 1, 1)
        );
        assert!(log[0].award_confirmed && log[0].credit_granted == vec!["DXCC".to_string()]);
        assert!(s1.orphans.is_empty());

        // Idempotent: re-running the same report changes nothing.
        let s2 = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s2.newly_confirmed, s2.newly_credited), (0, 0));
        assert_eq!(s2.matched, 1);
    }

    #[test]
    fn eqsl_grade_confirmation_counts_any_not_award_and_is_idempotent() {
        let mut log = vec![rec("DL1ABC", "40m", "FT8", 20_100)];
        // An eQSL-grade confirmation: confirmed but NOT award-eligible.
        let mut report = rec("dl1abc", "40M", "FT8", 20_100);
        report.confirmed = true; // award_confirmed stays false (eQSL)

        let s1 = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!(s1.matched, 1);
        assert_eq!(
            s1.newly_confirmed_any, 1,
            "a new confirmation (eQSL channel)"
        );
        assert_eq!(s1.newly_confirmed, 0, "but NOT award-grade");
        assert!(log[0].confirmed && !log[0].award_confirmed);

        // Idempotent: a re-pulled eQSL card (the inclusive RcvdSince boundary) does
        // not inflate the count.
        let s2 = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s2.newly_confirmed_any, s2.newly_confirmed), (0, 0));

        // A later LoTW (award) confirmation of the SAME already-confirmed QSO bumps
        // the award count, not the any-count (it was already confirmed).
        let mut lotw = rec("dl1abc", "40M", "FT8", 20_100);
        lotw.confirmed = true;
        lotw.award_confirmed = true;
        let s3 = reconcile(&mut log, std::slice::from_ref(&lotw));
        assert_eq!((s3.newly_confirmed_any, s3.newly_confirmed), (0, 1));
        assert!(log[0].award_confirmed);
    }

    #[test]
    fn unmatched_confirmation_becomes_an_orphan() {
        let mut log = vec![rec("W1AW", "20m", "FT8", 20_000)];
        let mut report = rec("K9XYZ", "40m", "CW", 20_001); // not in the log
        report.award_confirmed = true;
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!(s.matched, 0);
        assert_eq!(s.orphans.len(), 1);
        assert!(s.orphans[0].reason.contains("K9XYZ"));
        assert!(s.orphans[0].reason.contains("40m"));
    }

    #[test]
    fn two_same_day_qsos_consume_distinct_report_rows() {
        // Two CW QSOs with the same station, same band+day (e.g. dupe/relog).
        let mut log = vec![
            rec("DL1AA", "20m", "CW", 20_000),
            rec("DL1AA", "20m", "CW", 20_000),
        ];
        let mut r1 = rec("DL1AA", "20m", "CW", 20_000);
        r1.award_confirmed = true;
        let mut r2 = rec("DL1AA", "20m", "CW", 20_000);
        r2.award_confirmed = true;
        let s = reconcile(&mut log, &[r1, r2]);
        assert_eq!(s.matched, 2);
        assert!(log[0].award_confirmed && log[1].award_confirmed);
        assert!(s.orphans.is_empty());
    }

    #[test]
    fn matches_across_a_midnight_boundary_within_one_day() {
        // Logged at 23:59 one UTC day; report timestamped 00:01 the next.
        let mut logged = rec("DL1XX", "20m", "FT8", 20_000);
        logged.when_unix = 20_000 * 86_400 + 86_399; // 23:59:59
        let mut log = vec![logged];
        let mut report = rec("DL1XX", "20m", "FT8", 20_001);
        report.when_unix = 20_001 * 86_400 + 60; // 00:01:00 next day
        report.award_confirmed = true;
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!(s.matched, 1, "±1 day tolerance matches the same QSO");
        assert!(log[0].award_confirmed);
        assert!(s.orphans.is_empty());
    }

    #[test]
    fn granting_a_credit_clears_it_from_submitted() {
        let mut logged = rec("W1AW", "20m", "FT8", 20_000);
        logged.credit_submitted = vec!["DXCC".into()]; // previously applied
        let mut log = vec![logged];
        let mut report = rec("W1AW", "20m", "FT8", 20_000);
        report.award_confirmed = true;
        report.credit_granted = vec!["DXCC".into()]; // now granted
        reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!(log[0].credit_granted, vec!["DXCC".to_string()]);
        assert!(
            log[0].credit_submitted.is_empty(),
            "granted ⇒ no longer applied"
        );
    }

    #[test]
    fn report_fills_missing_state_but_never_overwrites() {
        let a = rec("W1AW", "20m", "FT8", 20_000); // logged without state
        let mut b = rec("K5XYZ", "20m", "FT8", 20_000);
        b.state = Some("TX".into()); // logged WITH state
        let mut log = vec![a, b];

        let mut r1 = rec("W1AW", "20m", "FT8", 20_000);
        r1.award_confirmed = true;
        r1.state = Some("CT".into()); // report supplies the missing state
        let mut r2 = rec("K5XYZ", "20m", "FT8", 20_000);
        r2.award_confirmed = true;
        r2.state = Some("OK".into()); // report DISAGREES — must not overwrite

        reconcile(&mut log, &[r1, r2]);
        assert_eq!(log[0].state.as_deref(), Some("CT"), "missing state filled");
        assert_eq!(
            log[1].state.as_deref(),
            Some("TX"),
            "existing state preserved"
        );
    }

    #[test]
    fn report_fills_missing_country_but_never_overwrites() {
        let a = rec("DL1XYZ", "20m", "FT8", 20_000); // logged without country
        let mut b = rec("F5RXL", "20m", "FT8", 20_000);
        b.country = Some("France".into()); // logged WITH country
        let mut log = vec![a, b];

        let mut r1 = rec("DL1XYZ", "20m", "FT8", 20_000);
        r1.award_confirmed = true;
        r1.country = Some("Germany".into()); // report supplies the missing country
        let mut r2 = rec("F5RXL", "20m", "FT8", 20_000);
        r2.award_confirmed = true;
        r2.country = Some("Wrong".into()); // report DISAGREES — must not overwrite

        reconcile(&mut log, &[r1, r2]);
        assert_eq!(
            log[0].country.as_deref(),
            Some("Germany"),
            "missing country filled"
        );
        assert_eq!(
            log[1].country.as_deref(),
            Some("France"),
            "existing country preserved"
        );
    }

    #[test]
    fn plain_unconfirmed_report_row_is_not_an_orphan() {
        // A report row with no confirmation/credit that matches nothing isn't a
        // "missing confirmation" — don't surface it as a diagnostic.
        let mut log = vec![rec("W1AW", "20m", "FT8", 20_000)];
        let plain = rec("K9ZZZ", "40m", "CW", 20_000); // no confirmed/credit
        let s = reconcile(&mut log, std::slice::from_ref(&plain));
        assert_eq!(s.matched, 0);
        assert!(s.orphans.is_empty(), "an unconfirmed row is not an orphan");
    }

    #[test]
    fn phone_and_digital_same_call_band_day_do_not_cross_match() {
        let mut log = vec![rec("JA1AA", "20m", "SSB", 20_000)];
        let mut digi = rec("JA1AA", "20m", "FT8", 20_000); // Digital ≠ Phone
        digi.award_confirmed = true;
        let s = reconcile(&mut log, std::slice::from_ref(&digi));
        assert_eq!(s.matched, 0, "Digital report must not match a Phone QSO");
        assert_eq!(s.orphans.len(), 1);
        assert!(!log[0].award_confirmed);
    }

    #[test]
    fn a_confirmation_lands_on_the_contact_nearest_its_time() {
        // #400. Two contacts with one station on one band in one UTC day: a QSO that broke
        // down and was redone, or FT8 then FT4 (one mode class). LoTW confirms only the later
        // one. The day-keyed matcher handed it to the EARLIER contact, by its place in the log,
        // and a merge only ever adds, so the false confirmation counted toward awards for good.
        let mut log = vec![w1aw_at(20_000, 6, 0), w1aw_at(20_000, 18, 0)];
        let report = confirms(w1aw_at(20_000, 18, 2), "DXCC");
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s.matched, s.orphans.len()), (1, 0));
        assert!(
            log[1].award_confirmed,
            "the 18:00 contact, two minutes away"
        );
        assert_eq!(log[1].credit_granted, vec!["DXCC".to_string()]);
        assert!(
            !log[0].confirmed && !log[0].award_confirmed && log[0].credit_granted.is_empty(),
            "and nothing on the 06:00 one"
        );

        // Both inside the window (a redo fifteen minutes later): still the nearer one.
        let mut log = vec![w1aw_at(20_000, 17, 45), w1aw_at(20_000, 18, 0)];
        let report = confirms(w1aw_at(20_000, 17, 58), "DXCC");
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s.matched, s.orphans.len()), (1, 0));
        assert!(log[1].award_confirmed, "18:00 is two minutes away");
        assert!(!log[0].award_confirmed, "17:45 is thirteen");
    }

    #[test]
    fn a_confirmation_31_minutes_from_every_contact_does_not_merge() {
        // LoTW only matches two operators' times within 30 minutes, so a confirmation further
        // than that from every contact in the log is none of them: it stays out, an orphan,
        // exactly like one for a contact that was never logged. Either side of the contact.
        let day = 20_000;
        let mut log = vec![w1aw_at(day, 12, 0)];
        let after = confirms(w1aw_at(day, 12, 31), "DXCC");
        let before = confirms(w1aw_at(day, 11, 29), "DXCC");
        let s = reconcile(&mut log, &[after, before]);
        assert_eq!(s.matched, 0);
        assert!(!log[0].confirmed && !log[0].award_confirmed && log[0].credit_granted.is_empty());
        let mut orphaned: Vec<u64> = s.orphans.iter().map(|o| o.when_unix).collect();
        orphaned.sort_unstable();
        assert_eq!(
            orphaned,
            vec![
                day * 86_400 + 11 * 3600 + 29 * 60,
                day * 86_400 + 12 * 3600 + 31 * 60
            ]
        );

        // Thirty minutes exactly is inside the window.
        let edge = confirms(w1aw_at(day, 12, 30), "DXCC");
        let s = reconcile(&mut log, std::slice::from_ref(&edge));
        assert_eq!((s.matched, s.orphans.len()), (1, 0));
        assert!(log[0].award_confirmed);
    }

    #[test]
    fn two_confirmations_never_share_a_contact_and_the_nearer_one_takes_it() {
        // One contact, logged at 10:00, and two confirmations for the station that day: 10:01
        // (this contact) and 10:24 (another QSO, logged somewhere else and never here). LoTW
        // lists the newest match first, so the 10:24 row can arrive first. Taken in report
        // order it would claim the contact (24 minutes is inside the window) and orphan the
        // row that is really this contact's. The nearer pair settles first.
        let mut log = vec![w1aw_at(20_000, 10, 0)];
        let elsewhere = confirms(w1aw_at(20_000, 10, 24), "DXCC");
        let this_one = confirms(w1aw_at(20_000, 10, 1), "WAS");
        let s = reconcile(&mut log, &[elsewhere, this_one]);
        assert_eq!(s.matched, 1, "one contact takes one confirmation");
        assert_eq!(
            log[0].credit_granted,
            vec!["WAS".to_string()],
            "the 10:01 row's credit, not the 10:24 row's"
        );
        assert_eq!(s.orphans.len(), 1);
        assert_eq!(
            s.orphans[0].when_unix,
            20_000 * 86_400 + 10 * 3600 + 24 * 60,
            "the 10:24 row is the contact this log lacks"
        );
    }

    #[test]
    fn a_near_pair_split_by_midnight_still_pairs() {
        // 23:58 and 00:03 are five minutes apart on two UTC days. The day-keyed matcher took
        // anything on the confirmation's own day first (here a 00:20 contact with the same
        // station) and only looked across midnight when that day had nothing. Time decides.
        let mut log = vec![w1aw_at(20_000, 23, 58), w1aw_at(20_001, 0, 20)];
        let report = confirms(w1aw_at(20_001, 0, 3), "DXCC");
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s.matched, s.orphans.len()), (1, 0));
        assert!(
            log[0].award_confirmed,
            "the 23:58 contact, five minutes away"
        );
        assert!(
            !log[1].award_confirmed,
            "not the 00:20 one, seventeen minutes away"
        );
    }

    #[test]
    fn a_date_only_contact_still_pairs_on_its_day() {
        // A contact imported with no time of day (`time_known == false`, parked at 00:00) has
        // no time to be near. Nexus never signs one for LoTW, so a confirmation for it comes
        // from an upload made elsewhere, with the real time: 14:23 here. It pairs by its date,
        // as it always has.
        let mut log = vec![date_only("W1AW", "20m", "FT8", 20_000, None)];
        let report = confirms(w1aw_at(20_000, 14, 23), "DXCC");
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s.matched, s.orphans.len()), (1, 0));
        assert!(log[0].award_confirmed);
    }

    #[test]
    fn a_measured_time_in_the_window_beats_a_date_only_contact() {
        // Confirmation at 00:10. The date-only contact's midnight is ten minutes from it and the
        // contact logged at 00:35 is twenty-five, but only one of those times was measured.
        let mut log = vec![
            date_only("W1AW", "20m", "FT8", 20_000, None),
            w1aw_at(20_000, 0, 35),
        ];
        let report = confirms(w1aw_at(20_000, 0, 10), "DXCC");
        let s = reconcile(&mut log, std::slice::from_ref(&report));
        assert_eq!((s.matched, s.orphans.len()), (1, 0));
        assert!(log[1].award_confirmed, "the contact logged at 00:35");
        assert!(!log[0].award_confirmed, "not the one with no time of day");
    }

    // ---- merge_and_add (two-way download sync) ----

    #[test]
    fn merge_add_does_not_double_log_across_mode_spelling() {
        // Local SSB QSO; the download re-reports the SAME contact as USB (a phone app
        // uploaded it that way). The old two-pass import (full-mode key) would have
        // seen USB as new and appended a phantom; merge_and_add's single class-keyed
        // pass matches it, upgrades the confirmation, and adds nothing.
        let mut log = vec![rec("W1AW", "20m", "SSB", 20_000)];
        let mut usb = rec("W1AW", "20m", "USB", 20_000);
        usb.qsl_rcvd.qrz = true; // QRZ-native confirmation
        usb.confirmed = true;
        let (added, sum) = merge_and_add(&mut log, vec![usb]);
        assert!(
            added.is_empty(),
            "USB must match the SSB QSO, not add a phantom"
        );
        assert_eq!(log.len(), 1);
        assert!(log[0].confirmed);
        assert_eq!(sum.matched, 1);
        assert!(sum.orphans.is_empty());
    }

    #[test]
    fn merge_add_appends_new_and_is_idempotent() {
        let mut log = vec![rec("W1AW", "20m", "FT8", 20_000)];
        let mut newq = rec("K5NEW", "40m", "CW", 20_000);
        newq.qsl_rcvd.qrz = true;
        newq.confirmed = true;
        // First sync: K5NEW is new → added; W1AW row (unconfirmed) matches, no change.
        let (added, _) = merge_and_add(
            &mut log,
            vec![rec("W1AW", "20m", "FT8", 20_000), newq.clone()],
        );
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].call, "K5NEW");
        assert_eq!(log.len(), 2);
        // Second identical sync: nothing new, nothing re-confirmed.
        let (added2, sum2) = merge_and_add(&mut log, vec![rec("W1AW", "20m", "FT8", 20_000), newq]);
        assert!(added2.is_empty(), "re-sync must add no duplicates");
        assert_eq!(log.len(), 2);
        assert_eq!(sum2.newly_confirmed_any, 0, "already confirmed");
    }

    #[test]
    fn merge_add_keeps_distinct_same_key_rows_and_is_idempotent() {
        // Two rows collapsing to one (call, band, mode-class, day) key are DISTINCT QSOs —
        // consume-once, exactly like reconcile — so BOTH add on the first sync...
        let mut log: Vec<QsoRecord> = Vec::new();
        let a = rec("DL1XYZ", "15m", "FT8", 20_100);
        let (added, _) = merge_and_add(&mut log, vec![a.clone(), a.clone()]);
        assert_eq!(added.len(), 2, "two same-key rows are two records");
        assert_eq!(log.len(), 2);
        // ...and re-fetching the SAME batch adds nothing (idempotent — each row pops its own
        // match). The old within-batch "dedup" re-indexed the appended slot and broke exactly
        // this: the twin re-appended a phantom every sync.
        let (added2, _) = merge_and_add(&mut log, vec![a.clone(), a]);
        assert!(
            added2.is_empty(),
            "re-sync of the same batch adds no phantom"
        );
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn merge_add_still_tolerates_a_download_timestamp_inside_the_window() {
        // THE REASON the download/report matcher is tolerant, pinned: a downloaded copy of our
        // QSO can carry a time a little off ours (another program logged or uploaded it). Inside
        // the 30-minute window, and across midnight, it is the SAME contact — matched and
        // upgraded, never appended as a phantom. (`merge_own_disk` is exact precisely because
        // its rows are not this.) Hours off is another contact since 2026-10-07 (#400):
        // `merge_add_appends_a_row_31_minutes_from_every_contact`.
        let mut logged = rec("W1AW", "20m", "FT8", 20_000);
        logged.when_unix = 20_000 * 86_400 + 6 * 3600; // we logged 06:00
        let mut log = vec![logged];

        let mut same_day = rec("W1AW", "20m", "FT8", 20_000);
        same_day.when_unix = 20_000 * 86_400 + 6 * 3600 + 20 * 60; // they report 06:20
        same_day.qsl_rcvd.qrz = true;
        same_day.confirmed = true;
        let (added, sum) = merge_and_add(&mut log, vec![same_day]);
        assert!(
            added.is_empty(),
            "a re-timed row is the same QSO, not a new one"
        );
        assert_eq!((log.len(), sum.matched), (1, 1));
        assert!(log[0].confirmed);

        // ...and across the midnight boundary.
        let mut late = rec("W1AW", "20m", "FT8", 20_000);
        late.when_unix = 20_000 * 86_400 + 86_399; // we logged 23:59:59
        let mut log = vec![late];
        let mut next_day = rec("W1AW", "20m", "FT8", 20_001);
        next_day.when_unix = 20_001 * 86_400 + 60; // they report 00:01 the next day
        next_day.qsl_rcvd.qrz = true;
        next_day.confirmed = true;
        let (added2, sum2) = merge_and_add(&mut log, vec![next_day]);
        assert!(added2.is_empty(), "across midnight it still matches");
        assert_eq!((log.len(), sum2.matched), (1, 1));
        assert!(log[0].qsl_rcvd.qrz);
    }

    #[test]
    fn merge_add_lands_a_downloaded_confirmation_on_the_nearest_contact() {
        // #400 through the QRZ fetch: two contacts with the station that day, QRZ shows only
        // the later one confirmed.
        let mut log = vec![w1aw_at(20_000, 6, 0), w1aw_at(20_000, 18, 0)];
        let mut row = w1aw_at(20_000, 18, 2);
        row.qsl_rcvd.qrz = true;
        row.confirmed = true; // QRZ's own confirmation: confirmed, never award-grade
        let (added, sum) = merge_and_add(&mut log, vec![row]);
        assert!(added.is_empty(), "it is the 18:00 contact, not a new one");
        assert_eq!((log.len(), sum.matched), (2, 1));
        assert!(log[1].confirmed, "the 18:00 contact QRZ confirmed");
        assert!(!log[0].confirmed, "and not the 06:00 one");
    }

    #[test]
    fn merge_add_appends_a_row_31_minutes_from_every_contact() {
        // In the QRZ fetch a row that is none of the logged contacts is a contact the log lacks,
        // so beyond the window it is added as one, never folded onto the same-day QSO.
        let mut log = vec![w1aw_at(20_000, 12, 0)];
        let mut row = w1aw_at(20_000, 12, 31);
        row.qsl_rcvd.qrz = true;
        row.confirmed = true;
        let (added, sum) = merge_and_add(&mut log, vec![row]);
        assert_eq!(sum.matched, 0);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].when_unix, 20_000 * 86_400 + 12 * 3600 + 31 * 60);
        assert_eq!(log.len(), 2);
        assert!(!log[0].confirmed, "the 12:00 contact is untouched");
    }

    #[test]
    fn merge_add_settles_the_nearer_row_first() {
        // Through the QRZ fetch: the 10:24 row arrives first, and it is the contact the log lacks
        // (added), not the 10:00 contact's; the 10:01 row, which QRZ confirms, is the 10:00 one's.
        let mut log = vec![w1aw_at(20_000, 10, 0)];
        let elsewhere = w1aw_at(20_000, 10, 24);
        let mut this_one = w1aw_at(20_000, 10, 1);
        this_one.qsl_rcvd.qrz = true;
        this_one.confirmed = true;
        let (added, sum) = merge_and_add(&mut log, vec![elsewhere, this_one]);
        assert_eq!(sum.matched, 1);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].when_unix, 20_000 * 86_400 + 10 * 3600 + 24 * 60);
        assert_eq!(log.len(), 2);
        assert!(log[0].qsl_rcvd.qrz, "the 10:00 contact took the 10:01 row");
        assert!(!added[0].qsl_rcvd.qrz, "and the 10:24 row is the one added");
    }

    #[test]
    fn merge_add_takes_qrzs_own_confirmation_and_none_of_its_copies() {
        // QRZ's book repeats what LoTW, eQSL and a paper card hold, and LoTW's credit, beside
        // QRZ's own confirmation. 06:00 holds nothing (Check confirmations took LoTW's off it),
        // 18:00 holds LoTW's own confirmation and its DXCC, and K5NEW is not in the log. Only
        // QRZ's own confirmation reaches a contact, held or added.
        let copies = |mut r: QsoRecord, qrz: bool| {
            r.qsl_rcvd = crate::logbook::QslRcvd {
                card: true,
                lotw: true,
                eqsl: true,
                qrz,
            };
            r.confirmed = true;
            r.award_confirmed = true;
            r.credit_granted = vec!["DXCC".into()];
            r.credit_submitted = vec!["WAS".into()];
            r
        };
        let early = w1aw_at(20_000, 6, 0);
        let mut late = w1aw_at(20_000, 18, 0);
        late.qsl_rcvd.lotw = true;
        late.confirmed = true;
        late.award_confirmed = true;
        late.credit_granted = vec!["DXCC".into()];
        let mut log = vec![early.clone(), late.clone()];
        let book = vec![
            copies(w1aw_at(20_000, 6, 0), false),
            copies(w1aw_at(20_000, 18, 0), true),
            copies(rec("K5NEW", "40m", "CW", 20_000), true),
        ];
        let (added, sum) = merge_and_add(&mut log, book);
        // 18:00 gains QRZ's confirmation, though the same row says a card confirms it too.
        late.qsl_rcvd.qrz = true;
        let mut new = rec("K5NEW", "40m", "CW", 20_000);
        new.qsl_rcvd.qrz = true;
        new.confirmed = true;
        assert_eq!(log, vec![early, late, new.clone()]);
        assert_eq!(added, vec![new]);
        assert_eq!(
            (sum.matched, sum.newly_confirmed, sum.newly_submitted),
            (2, 0, 0)
        );
    }

    #[test]
    fn merge_add_takes_no_lotw_upload_mark_from_qrzs_book() {
        use crate::logbook::{adif_record, parse_adif, UploadDetail};
        // QRZ's book also says what LoTW holds of the upload: its `LOTW_QSL_SENT`, and the
        // `APP_TEMPO_UL_LOTW` stamp a Nexus push carried to QRZ, if QRZ hands it back. 06:00 holds
        // no LoTW mark (Check confirmations took it off), 18:00 holds an upload LoTW bounced, and
        // K5NEW is not in the log. No row's mark reaches a contact, held or added, and QRZ's own
        // confirmation on the same row still does.
        let row = |call: &str, hhmmss: &str, more: &str| {
            format!(
                "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20241004\
                 <TIME_ON:6>{hhmmss}{more}<EOR>\n",
                call.len()
            )
        };
        let book = parse_adif(
            &[
                row("W1AW", "060000", "<APP_QRZLOG_STATUS:1>C<LOTW_QSL_SENT:1>Y"),
                row(
                    "W1AW",
                    "180000",
                    "<APP_TEMPO_UL_LOTW:20>accepted|1728100000|",
                ),
                row(
                    "K5NEW",
                    "090000",
                    "<APP_QRZLOG_STATUS:1>C<LOTW_QSL_SENT:1>Y",
                ),
            ]
            .concat(),
        );
        let declared = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 0,
            detail: Some(UploadDetail::OperatorDeclared),
        });
        let echoed = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_728_100_000,
            detail: None,
        });
        assert_eq!(
            book.iter()
                .map(|r| r.upload.lotw.clone())
                .collect::<Vec<_>>(),
            [declared.clone(), echoed, declared],
            "premise: each row reads as a LoTW upload mark"
        );
        let early = w1aw_at(20_000, 6, 0);
        let mut late = w1aw_at(20_000, 18, 0);
        late.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Rejected,
            when_unix: 1_728_050_000,
            detail: Some(UploadDetail::RecordRefused),
        });
        let mut log = vec![early.clone(), late.clone()];
        let (added, sum) = merge_and_add(&mut log, book);
        let mut confirmed = early;
        confirmed.qsl_rcvd.qrz = true;
        confirmed.confirmed = true;
        // K5NEW as QRZ's row would be without its word on LoTW.
        let new = parse_adif(&row("K5NEW", "090000", "<APP_QRZLOG_STATUS:1>C")).remove(0);
        assert_eq!(log, vec![confirmed, late, new.clone()]);
        assert_eq!(added, vec![new]);
        assert_eq!((sum.matched, sum.newly_confirmed_any), (2, 1));
        assert_eq!(
            parse_adif(&adif_record(&added[0]))[0].upload.lotw,
            None,
            "the added contact does not read a LoTW mark back from its own ADIF"
        );
    }

    // ---- merge_own_disk (recover OUR OWN file before a full rewrite) ----

    #[test]
    fn merge_own_disk_pairs_by_the_exact_second_not_the_utc_day() {
        // Two QSOs with one station on one band inside one UTC day (routine FT8). Another
        // instance appended the 06:00 one — award-confirmed — after we loaded, so our memory
        // holds ONLY the 18:00 one and disk order differs from memory order. The day-keyed
        // report matcher popped our 18:00 slot for the incoming 06:00 row: the confirmation
        // landed on the wrong contact, the 06:00 QSO was lost and the 18:00 one duplicated.
        let day = 20_000 * 86_400;
        let mut early = rec("W1AW", "20m", "FT8", 20_000);
        early.when_unix = day + 6 * 3600;
        early.award_confirmed = true;
        early.confirmed = true;
        let mut late = rec("W1AW", "20m", "FT8", 20_000);
        late.when_unix = day + 18 * 3600;

        let mut mem = vec![late.clone()];
        let (added, sum) = merge_own_disk(&mut mem, vec![early.clone(), late.clone()]);
        assert_eq!(added.len(), 1, "only the row we lack is new");
        assert_eq!(added[0].when_unix, day + 6 * 3600);
        assert_eq!(sum.matched, 1, "our own 18:00 row matched itself");

        let mut times: Vec<u64> = mem.iter().map(|r| r.when_unix).collect();
        times.sort_unstable();
        assert_eq!(times, vec![day + 6 * 3600, day + 18 * 3600]);
        let got_early = mem.iter().find(|r| r.when_unix == day + 6 * 3600).unwrap();
        let got_late = mem.iter().find(|r| r.when_unix == day + 18 * 3600).unwrap();
        assert!(
            got_early.award_confirmed,
            "confirmation on the contact that earned it"
        );
        assert!(
            !got_late.award_confirmed,
            "and not folded onto the other one"
        );

        // Idempotent: recovering the same file again adds nothing (each row pops its own
        // exact match out of the regrown log).
        let (added2, _) = merge_own_disk(&mut mem, vec![early, late]);
        assert!(added2.is_empty());
        assert_eq!(mem.len(), 2);
    }

    #[test]
    fn merge_own_disk_pairs_across_adjacent_days() {
        // The same defect 30 hours apart: the report matcher's ±1-day midnight tolerance
        // paired two of OUR OWN rows. Our own file never needs that tolerance.
        let mut early = rec("W1AW", "20m", "FT8", 20_000);
        early.when_unix = 20_000 * 86_400 + 6 * 3600;
        early.award_confirmed = true;
        let mut late = rec("W1AW", "20m", "FT8", 20_001);
        late.when_unix = 20_001 * 86_400 + 12 * 3600;

        let mut mem = vec![late.clone()];
        merge_own_disk(&mut mem, vec![early, late]);
        let mut times: Vec<u64> = mem.iter().map(|r| r.when_unix).collect();
        times.sort_unstable();
        assert_eq!(
            times,
            vec![20_000 * 86_400 + 6 * 3600, 20_001 * 86_400 + 12 * 3600]
        );
        assert!(mem.iter().filter(|r| r.award_confirmed).count() == 1);
        assert!(
            mem.iter()
                .find(|r| r.when_unix == 20_000 * 86_400 + 6 * 3600)
                .unwrap()
                .award_confirmed
        );
    }

    #[test]
    fn merge_own_disk_unions_state_and_survives_mode_spelling_drift() {
        // Same contact, same second, spelled MFSK on disk (written by an older build) and
        // FT4 in memory — the one thing that legitimately drifts between OUR OWN writes, so
        // the key stays mode-CLASS. It must upgrade in place, not append a twin.
        let mut mine = rec("DL1ABC", "20m", "FT4", 20_000);
        mine.upload.clublog = Some(UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 50,
            detail: None,
        });
        let mut theirs = rec("DL1ABC", "20m", "MFSK", 20_000);
        theirs.award_confirmed = true;
        theirs.confirmed = true;

        let mut mem = vec![mine];
        let (added, sum) = merge_own_disk(&mut mem, vec![theirs]);
        assert!(added.is_empty(), "MFSK/FT4 is one QSO, not two");
        assert_eq!((mem.len(), sum.matched), (1, 1));
        assert!(
            mem[0].award_confirmed,
            "the other instance's confirmation folded in"
        );
        assert!(
            mem[0].upload.clublog.is_some(),
            "our own upload stamp is not clobbered"
        );
    }

    #[test]
    fn merge_own_disk_keeps_two_records_that_share_a_second() {
        // Date-only imports park at midnight, so two rows CAN share an exact key. They are
        // still distinct records: consume-once pairs them one-for-one and adds nothing.
        let mut mem = vec![date_only("K5AA", "20m", "CW", 20_000, None); 2];
        let disk = vec![date_only("K5AA", "20m", "CW", 20_000, None); 2];
        let (added, sum) = merge_own_disk(&mut mem, disk);
        assert!(added.is_empty(), "two on disk, two in memory — nothing new");
        assert_eq!((mem.len(), sum.matched), (2, 2));
    }

    /// A row from a DATE-ONLY source: no `TIME_ON`, so the parser parks it at
    /// 00:00:00 UTC and marks it time-UNKNOWN (`logbook.rs`, `time_known`).
    /// `rst` is what tells two such contacts apart, exactly as it does in the
    /// paper log the ADIF came from.
    fn date_only(call: &str, band: &str, mode: &str, day: u64, rst: Option<&str>) -> QsoRecord {
        let mut r = rec(call, band, mode, day);
        r.when_unix = day * 86_400; // fabricated midnight
        r.time_known = false;
        r.rst_sent = rst.map(str::to_string);
        r
    }

    #[test]
    fn merge_own_disk_keeps_two_date_only_contacts_that_share_a_fabricated_midnight() {
        // The exact key rests on "two distinct contacts with one station on one band in one
        // mode class cannot share a second". A date-only import breaks that assumption at the
        // source: with no TIME_ON, every row of that day parks at 00:00:00, so two genuinely
        // distinct contacts DO share the key — and the day-keyed mis-pairing the exact key was
        // built to end came straight back in its own shape.
        //
        // Disk holds both (the 599 one is another instance's append, award-confirmed); memory
        // holds only the 339 one. Consume-once popped OUR 339 row for the incoming 599 row —
        // the confirmation landed on the wrong contact, the 599 contact never entered memory,
        // and the 339 row was appended a second time. Two copies of one QSO, the other one
        // destroyed by the next full rewrite.
        let day = 20_000;
        let mut a = date_only("W1AW", "20m", "CW", day, Some("599"));
        a.award_confirmed = true;
        a.confirmed = true;
        let b = date_only("W1AW", "20m", "CW", day, Some("339"));

        let mut mem = vec![b.clone()];
        let (added, sum) = merge_own_disk(&mut mem, vec![a.clone(), b.clone()]);
        assert_eq!(added.len(), 1, "only the contact we lack is new");
        assert_eq!(added[0].rst_sent.as_deref(), Some("599"));
        assert_eq!(sum.matched, 1, "our own 339 row matched itself");

        let by_rst = |r: &str| -> Vec<&QsoRecord> {
            mem.iter()
                .filter(|q| q.rst_sent.as_deref() == Some(r))
                .collect()
        };
        assert_eq!(by_rst("599").len(), 1, "the 599 contact survives, once");
        assert_eq!(
            by_rst("339").len(),
            1,
            "and is not a second copy of the 339 one"
        );
        assert!(
            by_rst("599")[0].award_confirmed,
            "confirmation on the contact that earned it"
        );
        assert!(
            !by_rst("339")[0].award_confirmed,
            "and not folded onto the other one"
        );

        // Idempotent: recovering the same file again adds nothing.
        let (added2, _) = merge_own_disk(&mut mem, vec![a, b]);
        assert!(added2.is_empty());
        assert_eq!(mem.len(), 2);
    }

    #[test]
    fn merge_own_disk_never_mistakes_a_real_midnight_qso_for_a_date_only_row() {
        // The other half of the same confusion: a REAL 00:00:00 UTC contact (time known —
        // the native writer records an off-time too) shares its second with a date-only row
        // for the same station and band. They are two contacts, not one.
        let day = 20_000;
        let mut real = rec("K5AA", "40m", "CW", day);
        real.when_unix = day * 86_400; // genuinely worked at midnight
        real.time_off_unix = Some(day * 86_400 + 120);
        let imported = date_only("K5AA", "40m", "CW", day, Some("559"));

        let mut mem = vec![real.clone()];
        let (added, _) = merge_own_disk(&mut mem, vec![imported.clone(), real.clone()]);
        assert_eq!(
            added.len(),
            1,
            "the imported row is a contact we do not hold"
        );
        assert_eq!(mem.len(), 2);
        assert_eq!(
            mem.iter().filter(|r| r.time_known).count(),
            1,
            "the real midnight QSO is still exactly one record"
        );
    }

    #[test]
    fn merge_own_disk_leaves_a_cross_instance_edit_as_a_visible_duplicate() {
        // THE DOCUMENTED TRADE, pinned so it cannot change by accident. Instance A corrects a
        // mis-logged time (12:00 → 12:05) and rewrites the file. We still hold the 12:00 copy.
        // The correction carries a different exact key, so it matches nothing here and is
        // appended: we end up holding BOTH, permanently, and both are eligible for upload.
        //
        // That is deliberate. The alternative — a key loose enough to see 12:05 as "the 12:00
        // row, edited" — is the fuzzy pairing `exact_key` exists to end, and it pays for the
        // duplicate with a SILENT revert of the operator's correction on the next rewrite.
        // A duplicate is on screen and one delete away; a reverted edit is invisible.
        // See `merge_own_disk` and `StationCore::recover_external_appends` for the contract,
        // and the 1.0.2 CHANGELOG for what the operator is told.
        let mut original = rec("W1AW", "20m", "SSB", 20_000);
        original.when_unix = 20_000 * 86_400 + 12 * 3600;
        let mut corrected = original.clone();
        corrected.when_unix += 5 * 60;

        let mut mem = vec![original];
        let (added, sum) = merge_own_disk(&mut mem, vec![corrected]);
        assert_eq!(added.len(), 1, "the correction reads as a contact we lack");
        assert_eq!(sum.matched, 0);
        assert_eq!(mem.len(), 2, "VISIBLE duplicate — never a silent revert");
        let mut times: Vec<u64> = mem.iter().map(|r| r.when_unix).collect();
        times.sort_unstable();
        assert_eq!(
            times,
            vec![
                20_000 * 86_400 + 12 * 3600,
                20_000 * 86_400 + 12 * 3600 + 300
            ],
            "the operator's corrected time is the one that survives a rewrite"
        );
    }
}
