//! Check confirmations: which confirmations up to 1.17.0 put on the wrong contact, decided from a
//! fresh download of the service's own rows (#400).
//!
//! Up to 1.17.0 a report row took the OLDEST contact with its call, band and mode class on its UTC
//! day and never compared times, so of two such contacts the later one's confirmation landed on
//! the earlier one, for good. `pair_report` ends that for every merge from now on. It
//! cannot repair what is stored: a stored confirmation is one flag per service, with no time,
//! date or source row, so a misplaced one looks exactly like a right one. A fresh download can
//! tell, for most of them. [`check_report`] pairs it exactly as the merge does and lists each
//! contact holding the service's confirmation that no confirming row pairs with any more, with
//! its evidence; [`uncheck`] is the change one ticked line makes.
//!
//! Pure: no I/O. Nothing here writes a log, and nothing in it touches a contact's own fields, a
//! paper card or a sync cursor. The caller commits what [`uncheck`] returns.
//!
//! # How a contact is judged
//!
//! 1. **Supported**: a row carrying the service's confirmation pairs with it
//!    (`pair_report`, so a check and a sync never disagree). Nothing more is done; one
//!    lacking the confirmation is a gain, which merging the download adds.
//! 2. **Reach**: the confirming rows with its call, band and mode class on its UTC day or the day
//!    either side, the only rows 1.17.0 could have put on it. A mark no row reaches is not this
//!    defect's (an import, an older call, a paper card an older Nexus filed as LoTW) and is only
//!    counted. So is one on a contact logged under another call than the one the download is
//!    for: the download cannot speak for it.
//! 3. **Replay**: 1.17.0's matcher (`pair_report_1_17`) over the same contacts and rows, in the
//!    service's own row order. It explains the mark when it puts a reaching row here.
//! 4. **Class**: [`LineClass::Contradicted`] when the service holds this contact itself,
//!    unconfirmed; else [`LineClass::Moved`] when the row now pairs with another of the
//!    operator's contacts, the sibling; else [`LineClass::Orphan`].
//! 5. **Decisive**, so the line starts ticked, only when the replay explains it and every time
//!    it rests on has a time of day. Then a Contradicted line is decisive when the service's
//!    record is at this contact's minute. A Moved line is decisive when the row is not exactly as
//!    near this contact as the sibling it went to (a tie the earlier contact took decides
//!    nothing), and either the row is the sibling's own record to the minute (LoTW, QRZ) or this
//!    contact is more than 30 minutes from it, the only form an eQSL card, timed by its sender,
//!    can meet. An Orphan line never is.
//!
//! LoTW's `Accepted` upload marks are checked the same way, against its own-QSO list and a replay
//! of 1.17.0's `promote_own_echo`: a contact marked `Accepted` that no own-list row pairs with is
//! one LoTW holds no upload of, so it was never uploaded again. That pull left out the contacts
//! award-confirmed when it ran, and the replay takes a contact as award-confirmed by a paper card,
//! or by a LoTW confirmation LoTW's download supports. A LoTW mark the download does not support
//! is the one under check: it may be 1.17.0's own, put there after the pull had marked the contact
//! `Accepted` from another contact's upload, so it does not keep the contact out of the replay.
//!
//! QRZ's book is QRZ's own record of the operator's contacts, confirmed or not, and it re-reports
//! what other services hold: its copies of LoTW's, eQSL's and a paper card's confirmation, and of
//! LoTW's credit. Only QRZ's own confirmation (`APP_QRZLOG_STATUS`) is its word, so the copies
//! never support, contradict or tick anything here, and Apply's gains from the book
//! ([`gain_qrz_confirmations`]) carry QRZ's confirmation alone. Neither an eQSL nor a QRZ
//! confirmation ever gave award credit, so their lines take no credit code off.
//!
//! The replay is an approximation. 1.17.0 merged incremental syncs, not one whole download, and
//! read the award state the log had then. Every line is the operator's own tick for that reason.

use super::{build_buckets, closeness, key, mode_class, pair_report, Key, MATCH_WINDOW_SECS};
use crate::logbook::{worked_under, QslRcvd, QsoRecord, RecordId, UploadOutcome};
use std::borrow::Borrow;
use std::collections::HashMap;

/// The service a check is about, named by the channel of [`QslRcvd`] its confirmation lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Lotw,
    Eqsl,
    Qrz,
}

impl Channel {
    /// Whether `q` holds this service's confirmation.
    pub fn held(self, q: &QslRcvd) -> bool {
        match self {
            Channel::Lotw => q.lotw,
            Channel::Eqsl => q.eqsl,
            Channel::Qrz => q.qrz,
        }
    }

    fn clear(self, q: &mut QslRcvd) {
        match self {
            Channel::Lotw => q.lotw = false,
            Channel::Eqsl => q.eqsl = false,
            Channel::Qrz => q.qrz = false,
        }
    }

    /// Whether the service's rows are the operator's own records, at the time the operator logged
    /// (LoTW's report, QRZ's book), rather than cards at the time the other station logged (eQSL).
    fn rows_are_own_records(self) -> bool {
        self != Channel::Eqsl
    }

    /// Whether the service's own record of a contact (LoTW's own-QSO list, QRZ's book) says the
    /// service confirms it. LoTW's list marks a confirmed record with a bare `QSL_RCVD`, which a
    /// parse reads as a card, so any channel will do. QRZ's book also re-reports what other
    /// services hold (its copies of `LOTW_QSL_RCVD`, `EQSL_QSL_RCVD` and `QSL_RCVD`), which is not
    /// QRZ's word: only its own `APP_QRZLOG_STATUS` is.
    fn own_record_confirms(self, q: &QslRcvd) -> bool {
        match self {
            Channel::Lotw => q.any(),
            Channel::Eqsl | Channel::Qrz => self.held(q),
        }
    }

    /// Whether the service's confirmation is award-grade, and so the one whose row brings credit
    /// codes (LoTW's). An eQSL or QRZ confirmation never gave award credit, so its line takes no
    /// code off: a code on a QRZ row is QRZ's copy of LoTW's.
    fn award_grade(self) -> bool {
        self == Channel::Lotw
    }
}

/// What a line takes off its contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// The service's confirmation, with the credit codes the row that put it there brought.
    Confirmation(Channel),
    /// LoTW's `Accepted` upload state.
    LotwUpload,
}

/// What the download says about a listed contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineClass {
    /// The row that reaches this contact now pairs with another of the operator's contacts, the
    /// sibling.
    Moved,
    /// The service holds this contact itself, unconfirmed: its own record pairs with this contact
    /// and carries no confirmation.
    Contradicted,
    /// The row pairs with no contact: the QSO it confirms is not in this log, or this contact's
    /// time is more than 30 minutes from the one the service holds.
    Orphan,
}

/// Why a line does not start ticked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unticked {
    /// A time the line rests on has no time of day (this contact's, the row's, the sibling's or
    /// the service's own record's), so its pairing fell back to the day rule.
    DateOnly,
    /// 1.17.0's matcher, replayed, puts no row here: the defect does not explain the mark.
    NotReplayed,
    /// The row is exactly as near this contact as the sibling it went to; the earlier contact in
    /// the log took it.
    Tie,
    /// The row pairs with no contact.
    Orphan,
    /// The row is within 30 minutes of this contact too, and it is not the sibling's own record
    /// to the minute.
    InsideWindow,
    /// The service's unconfirmed record of this contact is not at this contact's minute.
    NotToTheMinute,
}

/// The operator's contact a line's row pairs with now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sibling {
    /// Its place in the contacts the check was given.
    pub index: usize,
    pub id: Option<RecordId>,
    pub when_unix: u64,
}

/// Award credit codes, as [`QsoRecord::credit_granted`] and [`QsoRecord::credit_submitted`] hold
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Codes {
    pub granted: Vec<String>,
    pub submitted: Vec<String>,
}

/// One contact the check lists.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckLine {
    /// Its place in the contacts the check was given.
    pub index: usize,
    /// The contact as the check read it: what the line shows, and what [`uncheck`] compares.
    pub contact: QsoRecord,
    pub mark: Mark,
    pub class: LineClass,
    /// The time of the row the line rests on: the one 1.17.0's matcher put here, else the
    /// reaching row nearest this contact.
    pub row_unix: u64,
    /// The contact that row pairs with now.
    pub sibling: Option<Sibling>,
    /// The time of the service's own unconfirmed record of this contact (Contradicted only).
    pub own_unix: Option<u64>,
    /// Why the line does not start ticked; `None` when it is decisive.
    pub unticked: Option<Unticked>,
    /// The credit codes the change takes off: those the replayed row brought that the contact
    /// holds. None when the replay put no row here, since then no row is known to have brought
    /// any.
    pub remove: Codes,
}

impl CheckLine {
    /// Whether the evidence decides the line, so it starts ticked.
    pub fn decisive(&self) -> bool {
        self.unticked.is_none()
    }

    /// Whether the contact holds a paper card. The change never touches one, and it keeps the
    /// contact award-confirmed.
    pub fn card_held(&self) -> bool {
        self.contact.qsl_rcvd.card
    }

    /// The credit codes the change leaves.
    pub fn keep(&self) -> Codes {
        let rest = |held: &[String], gone: &[String]| {
            held.iter().filter(|c| !gone.contains(c)).cloned().collect()
        };
        Codes {
            granted: rest(&self.contact.credit_granted, &self.remove.granted),
            submitted: rest(&self.contact.credit_submitted, &self.remove.submitted),
        }
    }
}

/// Marks the check leaves alone and only counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unjudged {
    /// No row reaches the contact, so the mark is not this defect's.
    pub unreached: usize,
    /// The contact was logged under another call than the one the download is for.
    pub out_of_scope: usize,
}

/// What [`check_report`] found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfirmationCheck {
    /// The listed contacts: the confirmation lines, then the upload lines, each in log order.
    pub lines: Vec<CheckLine>,
    /// The contacts a confirming row pairs with that lack the confirmation, which merging the
    /// download adds: places in the contacts the check was given.
    pub gains: Vec<usize>,
    /// The confirmations it leaves alone.
    pub flags: Unjudged,
    /// The LoTW `Accepted` marks it leaves alone.
    pub uploads: Unjudged,
}

/// Check the service's confirmations on `local` against a fresh download of it.
///
/// - `local`: the contacts, in log order (1.17.0's matcher took the oldest by that order), and at
///   least every one with a call the download names. A mark on any other is only counted.
/// - `rows`: the service's rows, read as its merge reads them (a LoTW report with its channel
///   fixed up), in the service's own order.
/// - `own_rows`: the service's own record of the operator's contacts, confirmed or not: LoTW's
///   own-QSO list, or QRZ's book, which is `rows` again. Empty for eQSL, whose cards carry
///   their sender's times. With [`Channel::Lotw`] they also decide the upload marks.
/// - `own_call`: the call the download is for; a contact logged under another is counted, not
///   judged. `None` judges every contact.
pub fn check_report<R: Borrow<QsoRecord>>(
    local: &[R],
    rows: &[QsoRecord],
    own_rows: &[QsoRecord],
    channel: Channel,
    own_call: Option<&str>,
) -> ConfirmationCheck {
    let own_call = own_call
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| !c.is_empty());
    let own_call = own_call.as_deref();
    let confirms = |row: &QsoRecord| channel.held(&row.qsl_rcvd);
    let mut check = ConfirmationCheck::default();

    let own_pairs = pair_report(local, own_rows);
    let own_of = row_of(local.len(), &own_pairs, |_| true);

    let pairs = pair_report(local, rows);
    let supported = row_of(local.len(), &pairs, |j| confirms(&rows[j]));
    let confirmations = Download {
        rows,
        replayed: row_of(local.len(), &pair_report_1_17(local, rows), |j| {
            confirms(&rows[j])
        }),
        reach: index_rows(rows, confirms),
        pairs,
        own_records: channel.rows_are_own_records(),
    };
    for (i, r) in local.iter().enumerate() {
        let held = channel.held(&r.borrow().qsl_rcvd);
        if supported[i].is_some() {
            if !held {
                check.gains.push(i);
            }
            continue;
        }
        if !held {
            continue;
        }
        // The service's own record of this very contact, where it holds it unconfirmed.
        let own = own_of[i]
            .map(|o| &own_rows[o])
            .filter(|o| !channel.own_record_confirms(&o.qsl_rcvd));
        let mark = Mark::Confirmation(channel);
        match judge(local, i, mark, &confirmations, own, own_call) {
            Verdict::Listed(line) => check.lines.push(*line),
            Verdict::Unreached => check.flags.unreached += 1,
            Verdict::OutOfScope => check.flags.out_of_scope += 1,
        }
    }

    if channel == Channel::Lotw {
        // Award-confirmed as 1.17.0's pull could have read it: a paper card, or a LoTW
        // confirmation the download supports.
        let award_then = |i: usize| {
            let q = &local[i].borrow().qsl_rcvd;
            q.card || (q.lotw && supported[i].is_some())
        };
        let uploads = Download {
            rows: own_rows,
            replayed: row_of(
                local.len(),
                &pair_own_echo_1_17(local, own_rows, award_then),
                |_| true,
            ),
            reach: index_rows(own_rows, |_| true),
            pairs: own_pairs,
            own_records: true,
        };
        for (i, r) in local.iter().enumerate() {
            if own_of[i].is_some() || !accepted(r.borrow()) {
                continue;
            }
            match judge(local, i, Mark::LotwUpload, &uploads, None, own_call) {
                Verdict::Listed(line) => check.lines.push(*line),
                Verdict::Unreached => check.uploads.unreached += 1,
                Verdict::OutOfScope => check.uploads.out_of_scope += 1,
            }
        }
    }
    check
}

/// The change one ticked line makes to its contact, or `None` when `rec` no longer holds what the
/// line showed of it: the contact moved on after the check, the operator never saw this version,
/// and nothing changes.
///
/// A confirmation line clears the service's flag, derives `confirmed` and `award_confirmed` again
/// from the four channels, and removes the codes the line names (only a LoTW line names any). It
/// never touches a paper card, so a contact holding one stays award-confirmed. An upload line
/// clears LoTW's upload state, so the contact is owed to LoTW again. Nothing else about the
/// contact moves, and each of a contact's lines still applies once another has gone first.
pub fn uncheck(rec: &QsoRecord, line: &CheckLine) -> Option<QsoRecord> {
    if !still_shows(rec, line) {
        return None;
    }
    let mut now = rec.clone();
    match line.mark {
        Mark::Confirmation(channel) => {
            channel.clear(&mut now.qsl_rcvd);
            now.confirmed = now.qsl_rcvd.any();
            now.award_confirmed = now.qsl_rcvd.award();
            now.credit_granted
                .retain(|c| !line.remove.granted.contains(c));
            now.credit_submitted
                .retain(|c| !line.remove.submitted.contains(c));
        }
        Mark::LotwUpload => now.upload.lotw = None,
    }
    Some(now)
}

/// Apply's gains from QRZ's book: QRZ's own confirmation put on each contact that one of the
/// book's confirming rows pairs with and that lacks it, the rows paired as the check and a sync
/// pair the whole book (`pair_report`), so QRZ's record of an unconfirmed contact still takes that
/// contact. Nothing else a row carries reaches a contact: not QRZ's copies of what LoTW, eQSL or a
/// paper card hold, nor their credit codes or upload marks (QRZ re-reports them, and they are not
/// its word), nor the operator's own fields. No row becomes a contact either: the QSOs the book
/// holds that the log lacks are Sync from QRZ's to add. How many contacts gained it.
pub fn gain_qrz_confirmations<R: crate::logbook::StoredRecord>(
    local: &mut [R],
    book: &[QsoRecord],
) -> usize {
    let mut gained = 0;
    for (row, pair) in book.iter().zip(pair_report(local, book)) {
        let Some(i) = pair else { continue };
        if row.qsl_rcvd.qrz && !local[i].borrow().qsl_rcvd.qrz {
            let rec = local[i].write();
            rec.qsl_rcvd.qrz = true;
            rec.confirmed = true;
            gained += 1;
        }
    }
    gained
}

/// Whether `rec` still holds what `line` showed of it: the same contact (every field the check
/// paired and scoped it by), and of its mark what the line's change rewrites and what the line
/// shows. For a confirmation line that is its own service's flag, and for LoTW's, whose
/// confirmation brings award credit, the paper card that keeps the credit and the codes the change
/// takes off; for an upload line, LoTW's upload state. No line compares what another of the
/// contact's lines rewrites (each rewrites its own flag or the upload state, a LoTW line the codes,
/// and the `confirmed`/`award_confirmed` read again from the channels), so a contact's lines, of
/// every service, apply whichever goes first.
fn still_shows(rec: &QsoRecord, line: &CheckLine) -> bool {
    let was = &line.contact;
    rec.id == was.id
        && rec.call == was.call
        && rec.band == was.band
        && rec.mode == was.mode
        && rec.when_unix == was.when_unix
        && rec.time_known == was.time_known
        && rec.station_callsign == was.station_callsign
        && rec.operator == was.operator
        && match line.mark {
            Mark::Confirmation(channel) => {
                channel.held(&rec.qsl_rcvd) == channel.held(&was.qsl_rcvd)
                    && (!channel.award_grade()
                        || (rec.qsl_rcvd.card == was.qsl_rcvd.card
                            && rec.credit_granted == was.credit_granted
                            && rec.credit_submitted == was.credit_submitted))
            }
            Mark::LotwUpload => rec.upload.lotw == was.upload.lotw,
        }
}

/// The rows that speak for one kind of mark (a service's confirmation, or LoTW's upload mark).
struct Download<'a> {
    rows: &'a [QsoRecord],
    /// The merge's pairing of `rows`.
    pairs: Vec<Option<usize>>,
    /// The rows that carry the mark, by the key 1.17.0 bucketed contacts on.
    reach: HashMap<Key, Vec<usize>>,
    /// Per contact: the mark-carrying row 1.17.0's matcher puts on it, replayed.
    replayed: Vec<Option<usize>>,
    /// Whether `rows` are the operator's own records ([`Channel::rows_are_own_records`]).
    own_records: bool,
}

/// Where a mark no row supports stands.
enum Verdict {
    Listed(Box<CheckLine>),
    Unreached,
    OutOfScope,
}

/// Judge the mark on `local[i]`, which no row in `d` supports. `own` is the service's own
/// unconfirmed record of this contact, where it holds one.
fn judge<R: Borrow<QsoRecord>>(
    local: &[R],
    i: usize,
    mark: Mark,
    d: &Download,
    own: Option<&QsoRecord>,
    own_call: Option<&str>,
) -> Verdict {
    let r: &QsoRecord = local[i].borrow();
    if !in_scope(r, own_call) {
        return Verdict::OutOfScope;
    }
    let Some(nearest) =
        reach_of(&d.reach, r).min_by_key(|&j| (r.when_unix.abs_diff(d.rows[j].when_unix), j))
    else {
        return Verdict::Unreached;
    };
    let (j, explained) = match d.replayed[i] {
        Some(j) => (j, true),
        None => (nearest, false),
    };
    let row = &d.rows[j];
    let sibling: Option<(usize, &QsoRecord)> = d.pairs[j].map(|s| (s, local[s].borrow()));
    let class = match (own, sibling) {
        (Some(_), _) => LineClass::Contradicted,
        (None, Some(_)) => LineClass::Moved,
        (None, None) => LineClass::Orphan,
    };
    let date_only = !r.time_known
        || match (own, sibling) {
            (Some(o), _) => !o.time_known,
            (None, Some((_, s))) => !row.time_known || !s.time_known,
            (None, None) => !row.time_known,
        };
    let unticked = if date_only {
        Some(Unticked::DateOnly)
    } else if !explained {
        Some(Unticked::NotReplayed)
    } else {
        match (own, sibling) {
            (Some(o), _) => (!same_minute(o, r)).then_some(Unticked::NotToTheMinute),
            (None, Some((_, s))) => {
                if closeness(r, row) == closeness(s, row) {
                    Some(Unticked::Tie)
                } else if (d.own_records && same_minute(row, s))
                    || r.when_unix.abs_diff(row.when_unix) > MATCH_WINDOW_SECS
                {
                    None
                } else {
                    Some(Unticked::InsideWindow)
                }
            }
            (None, None) => Some(Unticked::Orphan),
        }
    };
    let remove = match mark {
        Mark::Confirmation(channel) if explained && channel.award_grade() => brought(r, row),
        _ => Codes::default(),
    };
    Verdict::Listed(Box::new(CheckLine {
        index: i,
        contact: r.clone(),
        mark,
        class,
        row_unix: row.when_unix,
        sibling: sibling.map(|(index, s)| Sibling {
            index,
            id: s.id,
            when_unix: s.when_unix,
        }),
        own_unix: own.map(|o| o.when_unix),
        unticked,
        remove,
    }))
}

/// Whether the download is for the call `r` was logged under. A contact that names no call of its
/// own cannot be placed under another one, so it is judged; so is every contact when the download
/// names no call. `own_call` is trimmed and uppercased.
fn in_scope(r: &QsoRecord, own_call: Option<&str>) -> bool {
    own_call.is_none_or(|own| worked_under(r).is_none_or(|call| call == own))
}

/// The rows `counts` admits, by the key 1.17.0 bucketed contacts on (call, band, mode class, UTC
/// day).
fn index_rows(rows: &[QsoRecord], counts: impl Fn(&QsoRecord) -> bool) -> HashMap<Key, Vec<usize>> {
    let mut index: HashMap<Key, Vec<usize>> = HashMap::new();
    for (j, row) in rows.iter().enumerate() {
        if counts(row) {
            index.entry(key(row)).or_default().push(j);
        }
    }
    index
}

/// The indexed rows that reach `contact`: the only ones 1.17.0's matcher could have put on it,
/// with its call, band and mode class, on its UTC day or the day either side.
fn reach_of<'a>(
    index: &'a HashMap<Key, Vec<usize>>,
    contact: &QsoRecord,
) -> impl Iterator<Item = usize> + 'a {
    let (call, band, class, day) = key(contact);
    [day.wrapping_sub(1), day, day + 1]
        .into_iter()
        .flat_map(move |d| {
            index
                .get(&(call.clone(), band.clone(), class, d))
                .into_iter()
                .flatten()
                .copied()
        })
}

/// Per contact: the row `pairs` gives it, among the rows `counts` admits.
fn row_of(n: usize, pairs: &[Option<usize>], counts: impl Fn(usize) -> bool) -> Vec<Option<usize>> {
    let mut out = vec![None; n];
    for (j, pair) in pairs.iter().enumerate() {
        if let Some(i) = *pair {
            if counts(j) {
                out[i] = Some(j);
            }
        }
    }
    out
}

/// The codes `contact` holds that `row` brought, which leave with it.
fn brought(contact: &QsoRecord, row: &QsoRecord) -> Codes {
    let both = |held: &[String], came: &[String]| {
        held.iter().filter(|c| came.contains(c)).cloned().collect()
    };
    Codes {
        granted: both(&contact.credit_granted, &row.credit_granted),
        submitted: both(&contact.credit_submitted, &row.credit_submitted),
    }
}

/// Whether two times fall in one minute: how a service's own record of a contact, uploaded from
/// it, still reads when the service keeps no seconds.
fn same_minute(a: &QsoRecord, b: &QsoRecord) -> bool {
    a.when_unix / 60 == b.when_unix / 60
}

fn accepted(r: &QsoRecord) -> bool {
    matches!(
        r.upload.lotw.as_ref().map(|s| s.outcome),
        Some(UploadOutcome::Accepted)
    )
}

/// 1.17.0's matcher, replayed: `out[j]` is the contact in `local` that 1.17.0's `reconcile` put
/// report row `j` on. The DEFECT, kept on purpose as the check's oracle (#400), and never used to
/// merge.
///
/// Each row took the OLDEST contact left with its call, band and mode class on its own UTC day,
/// else the day before, else the day after, and never compared times. [`take_match`] is 1.17.0's,
/// verbatim (`v1.17.0:crates/tempo-core/src/reconcile.rs`); the buckets it pops
/// ([`build_buckets`], [`key`], [`mode_class`]) are unchanged since that tag.
fn pair_report_1_17<R: Borrow<QsoRecord>>(
    local: &[R],
    incoming: &[QsoRecord],
) -> Vec<Option<usize>> {
    let mut buckets = build_buckets(local);
    incoming
        .iter()
        .map(|inc| take_match(&mut buckets, inc))
        .collect()
}

/// 1.17.0's own-QSO pull (`promote_own_echo`), replayed: the same lookup over only the contacts
/// that were not award-confirmed, which it left out of its buckets (copied from the tag; its row
/// loop is [`take_match`]'s, written out inline there). It wrote `Accepted` on each contact it
/// returns. `award_confirmed` says which contacts were award-confirmed when it ran: the log's state
/// then, which no stored field records, so the caller says how it reads it.
fn pair_own_echo_1_17<R: Borrow<QsoRecord>>(
    local: &[R],
    own: &[QsoRecord],
    award_confirmed: impl Fn(usize) -> bool,
) -> Vec<Option<usize>> {
    // Index award-unconfirmed local QSOs by match key; reversed so pop() consumes
    // in log order (oldest first), mirroring `reconcile`.
    let mut buckets: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, r) in local.iter().enumerate() {
        if !award_confirmed(i) {
            buckets.entry(key(r.borrow())).or_default().push(i);
        }
    }
    for v in buckets.values_mut() {
        v.reverse();
    }
    own.iter()
        .map(|inc| take_match(&mut buckets, inc))
        .collect()
}

/// 1.17.0's, verbatim, and only ever the replay's (see [`pair_report_1_17`]):
///
/// Consume-once lookup of the local QSO matching `inc`: exact UTC day preferred,
/// then ±1 day — tolerates a report timestamped across midnight from the logged
/// QSO (clock skew / the other op's minute), which would otherwise falsely orphan
/// the same contact. Returns the matched local index and removes it from the bucket.
fn take_match(buckets: &mut HashMap<Key, Vec<usize>>, inc: &QsoRecord) -> Option<usize> {
    let call_u = inc.call.to_ascii_uppercase();
    let band_l = inc.band.to_ascii_lowercase();
    let mc = mode_class(&inc.mode);
    let day = inc.when_unix / 86_400;
    for d in [day, day.wrapping_sub(1), day + 1] {
        if let Some(v) = buckets.get_mut(&(call_u.clone(), band_l.clone(), mc, d)) {
            if let Some(i) = v.pop() {
                return Some(i);
            }
        }
    }
    None
}

/// 1.17.0's merges, replayed onto a log: how a test, in this crate or another, builds the state
/// 1.17.0 left by running its matcher, never by setting a flag by hand (#400). Built only for
/// tests: in this crate's, and with `test-util` in another crate's.
#[cfg(any(test, feature = "test-util"))]
pub mod replay {
    use super::{pair_own_echo_1_17, pair_report_1_17};
    use crate::logbook::{QsoRecord, UploadOutcome, UploadStatus};
    use crate::reconcile::{apply_match, ReconcileSummary};

    /// `log` as 1.17.0's `reconcile` left it after `rows`: its matcher's pairing, and the merge
    /// every version makes of a paired row (`apply_match`). `rows` are a report's rows as its
    /// merge reads them (`crate::logbook::report_rows`).
    pub fn left_by_1_17(mut log: Vec<QsoRecord>, rows: &[QsoRecord]) -> Vec<QsoRecord> {
        let mut sum = ReconcileSummary::default();
        for (row, pair) in rows.iter().zip(pair_report_1_17(&log, rows)) {
            if let Some(i) = pair {
                apply_match(&mut log[i], row, &mut sum);
            }
        }
        log
    }

    /// `log` as 1.17.0's own-QSO pull left it after `own`, LoTW's own-QSO list: `Accepted` on
    /// each contact its matcher gave a row, the contacts award-confirmed as it ran left out.
    pub fn echoed_by_1_17(mut log: Vec<QsoRecord>, own: &[QsoRecord]) -> Vec<QsoRecord> {
        let pairs = pair_own_echo_1_17(&log, own, |i| log[i].award_confirmed);
        for i in pairs.into_iter().flatten() {
            log[i].upload.lotw = Some(UploadStatus {
                outcome: UploadOutcome::Accepted,
                when_unix: 7,
                detail: None,
            });
        }
        log
    }
}

#[cfg(test)]
mod tests {
    use super::replay::{echoed_by_1_17, left_by_1_17};
    use super::*;
    use crate::logbook::UploadStatus;
    use crate::reconcile::tests::{lotw_outcome, rec, w1aw_at, with_lotw};
    use crate::reconcile::{apply_match, reconcile, ReconcileSummary};
    use proptest::prelude::*;

    /// The day every scene happens on (2024-10-04 UTC).
    const D: u64 = 20_000;
    /// The call each download is for.
    const OWN: Option<&str> = Some("KD9TAW");

    /// `r` as LoTW's confirmation of it, granting `credit`: a LoTW report's row with its channel
    /// fixed up, as the merge reads it.
    fn lotw(mut r: QsoRecord, credit: &[&str]) -> QsoRecord {
        r.qsl_rcvd.lotw = true;
        r.confirmed = true;
        r.award_confirmed = true;
        r.credit_granted = credit.iter().map(|c| c.to_string()).collect();
        r
    }

    /// `r` as an eQSL card: a confirmation, never an award-grade one.
    fn eqsl(mut r: QsoRecord) -> QsoRecord {
        r.qsl_rcvd.eqsl = true;
        r.confirmed = true;
        r
    }

    /// `row` merged into `log[i]` the way an ADIF import upgrades a contact it already holds.
    fn imported(log: &mut [QsoRecord], i: usize, row: &QsoRecord) {
        apply_match(&mut log[i], row, &mut ReconcileSummary::default());
    }

    /// W1AW on 20 m FT8 at `18:00:00 + secs`.
    fn w1aw_after_1800(secs: u64) -> QsoRecord {
        let mut r = w1aw_at(D, 18, 0);
        r.when_unix += secs;
        r
    }

    /// #400's pair as 1.17.0 left it: W1AW on 20 m FT8 at 06:00 and 18:00, and LoTW's confirmation
    /// of the 18:00 contact (its row at 18:02, granting DXCC) on the 06:00 one.
    fn the_400_pair() -> (Vec<QsoRecord>, Vec<QsoRecord>) {
        let rows = vec![lotw(w1aw_at(D, 18, 2), &["DXCC"])];
        let log = left_by_1_17(vec![w1aw_at(D, 6, 0), w1aw_at(D, 18, 0)], &rows);
        assert!(
            log[0].qsl_rcvd.lotw && !log[1].qsl_rcvd.lotw,
            "1.17.0 put the 18:00 contact's confirmation on the 06:00 one"
        );
        (log, rows)
    }

    #[test]
    fn the_400_pair_lists_the_earlier_contact_and_ticks_it() {
        let (log, rows) = the_400_pair();
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!(
            (line.index, line.mark, line.class),
            (0, Mark::Confirmation(Channel::Lotw), LineClass::Moved)
        );
        assert_eq!(line.row_unix, w1aw_at(D, 18, 2).when_unix);
        assert_eq!(
            line.sibling.map(|s| (s.index, s.when_unix)),
            Some((1, w1aw_at(D, 18, 0).when_unix))
        );
        assert_eq!(
            line.unticked, None,
            "06:00 is twelve hours from LoTW's 18:02"
        );
        assert_eq!(line.remove.granted, ["DXCC"]);
        assert_eq!(
            check.gains,
            [1],
            "18:00 gains the confirmation LoTW holds for it"
        );
        // The change puts 06:00 back exactly as it was before 1.17.0's merge.
        let after = uncheck(&log[0], line).expect("06:00 still holds what the line shows");
        assert_eq!(after, w1aw_at(D, 6, 0));
    }

    #[test]
    fn a_contact_lotw_holds_unconfirmed_is_contradicted_and_ticked() {
        // The confirmed QSO, at 15:00, was logged in another program and is not in this log;
        // 1.17.0 put its confirmation on the only W1AW 20 m FT8 contact that day.
        let rows = vec![lotw(w1aw_at(D, 15, 0), &["DXCC"])];
        let log = left_by_1_17(vec![w1aw_at(D, 10, 0)], &rows);
        assert!(
            log[0].qsl_rcvd.lotw,
            "1.17.0 put the 15:00 confirmation on 10:00"
        );
        // LoTW's own-QSO list holds both uploads: 10:00 unconfirmed, 15:00 confirmed (its plain
        // QSL_RCVD=Y, which a parse reads as a card).
        let mut confirmed_upload = w1aw_at(D, 15, 0);
        confirmed_upload.qsl_rcvd.card = true;
        confirmed_upload.confirmed = true;
        let own = vec![w1aw_at(D, 10, 0), confirmed_upload];
        let check = check_report(&log, &rows, &own, Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!((line.index, line.class), (0, LineClass::Contradicted));
        assert_eq!(line.own_unix, Some(w1aw_at(D, 10, 0).when_unix));
        assert_eq!(line.row_unix, w1aw_at(D, 15, 0).when_unix);
        assert_eq!(
            line.unticked, None,
            "LoTW's own record of this contact, to the minute"
        );
        // An unconfirmed record five minutes off (the time edited after the upload) is still this
        // contact's by the pairing, but no longer to the minute.
        let own = vec![w1aw_at(D, 10, 5)];
        let check = check_report(&log, &rows, &own, Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        assert_eq!(check.lines[0].class, LineClass::Contradicted);
        assert_eq!(check.lines[0].unticked, Some(Unticked::NotToTheMinute));
    }

    #[test]
    fn a_confirmation_of_an_unlogged_qso_is_listed_unticked() {
        let rows = vec![lotw(w1aw_at(D, 15, 0), &["DXCC"])];
        let log = left_by_1_17(vec![w1aw_at(D, 10, 0)], &rows);
        assert!(
            log[0].qsl_rcvd.lotw,
            "1.17.0 put the 15:00 confirmation on 10:00"
        );
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!((line.class, line.sibling), (LineClass::Orphan, None));
        assert_eq!(line.row_unix, w1aw_at(D, 15, 0).when_unix);
        assert_eq!(
            line.unticked,
            Some(Unticked::Orphan),
            "the confirmed QSO may be this one, logged five hours off"
        );
        assert!(check.gains.is_empty());
    }

    #[test]
    fn a_date_only_contact_is_never_ticked() {
        let mut date_only = rec("W1AW", "20m", "FT8", D);
        date_only.when_unix = D * 86_400;
        date_only.time_known = false;
        let rows = vec![lotw(w1aw_at(D, 18, 2), &[])];
        let log = left_by_1_17(vec![date_only, w1aw_at(D, 18, 0)], &rows);
        assert!(
            log[0].qsl_rcvd.lotw && !log[1].qsl_rcvd.lotw,
            "1.17.0 put 18:00's confirmation on the date-only contact"
        );
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!((line.index, line.class), (0, LineClass::Moved));
        assert_eq!(line.unticked, Some(Unticked::DateOnly));
    }

    #[test]
    fn a_tie_is_never_ticked() {
        // A QSO logged twice, 40 seconds apart, and LoTW's row exactly between them at 18:00:30.
        // LoTW lists the newest match first; 1.17.0 gave the 17:20 row to the older contact and
        // the 18:00:30 row to the younger. Now the tie goes to the older contact, and nothing
        // says which of the two the row is. A tie can decide a line only inside one minute:
        // anywhere else the row is not the sibling's to the minute, or not more than 30 minutes
        // from this contact.
        let rows = vec![
            lotw(w1aw_at(D, 17, 20), &[]),
            lotw(w1aw_after_1800(30), &[]),
        ];
        let log = left_by_1_17(vec![w1aw_after_1800(10), w1aw_after_1800(50)], &rows);
        assert!(
            log[1].qsl_rcvd.lotw,
            "1.17.0 gave 18:00:30's row to the 18:00:50 contact"
        );
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!((line.index, line.class), (1, LineClass::Moved));
        assert_eq!(line.sibling.map(|s| s.index), Some(0));
        assert_eq!(line.unticked, Some(Unticked::Tie));
    }

    #[test]
    fn an_eqsl_card_inside_the_window_of_both_contacts_is_never_ticked() {
        // Contacts at 18:00 and 18:20, and a card the sender timed 18:20, which 1.17.0 put on
        // 18:00. A card carries the other station's time, so meeting the 18:20 contact's minute
        // proves nothing, and 18:00 is only 20 minutes from it.
        let rows = vec![eqsl(w1aw_at(D, 18, 20))];
        let log = left_by_1_17(vec![w1aw_at(D, 18, 0), w1aw_at(D, 18, 20)], &rows);
        assert!(
            log[0].qsl_rcvd.eqsl && !log[1].qsl_rcvd.eqsl,
            "1.17.0 put the card on 18:00"
        );
        let check = check_report(&log, &rows, &[], Channel::Eqsl, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        assert_eq!(check.lines[0].class, LineClass::Moved);
        assert_eq!(check.lines[0].unticked, Some(Unticked::InsideWindow));
        // The same evidence from LoTW, whose row is the 18:20 contact's own record, decides it.
        let rows = vec![lotw(w1aw_at(D, 18, 20), &[])];
        let log = left_by_1_17(vec![w1aw_at(D, 18, 0), w1aw_at(D, 18, 20)], &rows);
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        assert_eq!(check.lines[0].unticked, None);
    }

    #[test]
    fn a_flag_the_1_17_matcher_could_not_have_put_there_is_unticked() {
        // Contacts at 06:00, 12:00 and 18:00, and LoTW's confirmation of 18:00 (its row 18:01),
        // which 1.17.0 put on 06:00. 12:00's LoTW mark came with an import: 1.17.0's matcher,
        // which always took the oldest contact, could not have put the 18:01 row there.
        let rows = vec![lotw(w1aw_at(D, 18, 1), &["DXCC"])];
        let mut log = left_by_1_17(
            vec![w1aw_at(D, 6, 0), w1aw_at(D, 12, 0), w1aw_at(D, 18, 0)],
            &rows,
        );
        imported(&mut log, 1, &lotw(w1aw_at(D, 12, 0), &[]));
        assert!(log[0].qsl_rcvd.lotw && log[1].qsl_rcvd.lotw && !log[2].qsl_rcvd.lotw);
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        let listed: Vec<usize> = check.lines.iter().map(|l| l.index).collect();
        assert_eq!(listed, [0, 1]);
        assert_eq!(check.lines[0].unticked, None, "06:00's is the replay's");
        let noon = &check.lines[1];
        assert_eq!(
            (noon.class, noon.sibling.map(|s| s.index)),
            (LineClass::Moved, Some(2))
        );
        assert_eq!(noon.unticked, Some(Unticked::NotReplayed));
        assert_eq!(
            noon.remove,
            Codes::default(),
            "no row is known to have brought a code"
        );
        assert_eq!(check.gains, [2]);
    }

    #[test]
    fn a_flag_no_row_reaches_is_counted_not_listed() {
        // An imported LoTW mark on 06:00, and a download whose only W1AW 20 m FT8 row is two days
        // later: beyond 1.17.0's reach (the row's own day and the day either side).
        let mut log = vec![w1aw_at(D, 6, 0)];
        imported(&mut log, 0, &lotw(w1aw_at(D, 6, 0), &[]));
        let rows = vec![lotw(w1aw_at(D + 2, 6, 0), &[])];
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert!(check.lines.is_empty(), "{:?}", check.lines);
        assert_eq!(
            check.flags,
            Unjudged {
                unreached: 1,
                out_of_scope: 0
            }
        );
        // A row the day after is in reach, and the same mark is listed.
        let rows = vec![lotw(w1aw_at(D + 1, 6, 0), &[])];
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        assert_eq!(check.flags, Unjudged::default());
    }

    #[test]
    fn a_paper_card_stays_and_keeps_award_credit() {
        let rows = vec![lotw(w1aw_at(D, 18, 2), &["DXCC"])];
        let mut log = vec![w1aw_at(D, 6, 0), w1aw_at(D, 18, 0)];
        let mut card = w1aw_at(D, 6, 0);
        card.qsl_rcvd.card = true;
        card.confirmed = true;
        card.award_confirmed = true;
        imported(&mut log, 0, &card);
        let log = left_by_1_17(log, &rows);
        assert!(log[0].qsl_rcvd.card && log[0].qsl_rcvd.lotw);
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert!(line.card_held() && line.decisive());
        let after = uncheck(&log[0], line).expect("06:00 still holds what the line shows");
        assert!(!after.qsl_rcvd.lotw, "LoTW's confirmation goes");
        assert!(after.qsl_rcvd.card, "the paper card stays");
        assert!(
            after.award_confirmed && after.confirmed,
            "and keeps the contact award-confirmed"
        );
    }

    #[test]
    fn credit_codes_leave_only_with_the_row_that_brought_them() {
        let mut row = lotw(w1aw_at(D, 18, 2), &["DXCC"]);
        row.credit_submitted = vec!["DXCC_BAND".into()];
        let rows = vec![row];
        let mut log = vec![w1aw_at(D, 6, 0), w1aw_at(D, 18, 0)];
        let mut was = w1aw_at(D, 6, 0);
        was.credit_granted = vec!["WAS".into()];
        imported(&mut log, 0, &was);
        let log = left_by_1_17(log, &rows);
        assert_eq!(log[0].credit_granted, ["DXCC", "WAS"]);
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!(line.remove.granted, ["DXCC"]);
        assert_eq!(line.remove.submitted, ["DXCC_BAND"]);
        assert_eq!(line.keep().granted, ["WAS"]);
        let after = uncheck(&log[0], line).expect("06:00 still holds what the line shows");
        assert_eq!(after.credit_granted, ["WAS"], "the import's WAS stays");
        assert!(after.credit_submitted.is_empty());
    }

    #[test]
    fn a_contact_logged_under_another_call_is_left_out() {
        // 06:00 was logged under another of the operator's calls, and the download is KD9TAW's,
        // so it cannot say whether that contact is confirmed. Both its marks are 1.17.0's: the
        // own-QSO pull gave it the 18:00 upload's echo, then a sync 18:00's confirmation.
        let mut other_call = w1aw_at(D, 6, 0);
        other_call.station_callsign = Some("N0OLD".into());
        let mut ours = with_lotw(w1aw_at(D, 18, 0), UploadOutcome::Pending);
        ours.station_callsign = Some("KD9TAW".into());
        let own = vec![w1aw_at(D, 18, 0)];
        let rows = vec![lotw(w1aw_at(D, 18, 2), &["DXCC"])];
        let log = left_by_1_17(echoed_by_1_17(vec![other_call, ours], &own), &rows);
        assert!(log[0].qsl_rcvd.lotw);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
        let check = check_report(&log, &rows, &own, Channel::Lotw, OWN);
        assert!(check.lines.is_empty(), "{:?}", check.lines);
        let one_out = Unjudged {
            unreached: 0,
            out_of_scope: 1,
        };
        assert_eq!((check.flags, check.uploads), (one_out, one_out));
        // A download for every call judges both marks.
        let check = check_report(&log, &rows, &own, Channel::Lotw, None);
        let marks: Vec<(usize, Mark)> = check.lines.iter().map(|l| (l.index, l.mark)).collect();
        assert_eq!(
            marks,
            [
                (0, Mark::Confirmation(Channel::Lotw)),
                (0, Mark::LotwUpload)
            ]
        );
    }

    #[test]
    fn uncheck_changes_nothing_when_the_contact_moved_on() {
        let (log, rows) = the_400_pair();
        let check = check_report(&log, &rows, &[], Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        // What happened to the contact after the check: versions the operator never saw.
        let mut card = log[0].clone();
        card.qsl_rcvd.card = true;
        let mut credited = log[0].clone();
        credited.credit_granted.push("WAS".into());
        let mut retimed = log[0].clone();
        retimed.when_unix += 300;
        let mut cleared = log[0].clone();
        cleared.qsl_rcvd.lotw = false;
        for moved_on in [card, credited, retimed, cleared] {
            assert!(uncheck(&moved_on, line).is_none(), "{moved_on:?}");
        }
        // An upload stamp is not what a confirmation line shows, so the change still applies.
        let restamped = with_lotw(log[0].clone(), UploadOutcome::Accepted);
        assert!(uncheck(&restamped, line).is_some_and(|r| !r.qsl_rcvd.lotw));
        assert!(uncheck(&log[0], line).is_some_and(|r| !r.qsl_rcvd.lotw));
    }

    #[test]
    fn pair_report_1_17_still_misplaces_the_400_pair() {
        let pair = [w1aw_at(D, 6, 0), w1aw_at(D, 18, 0)];
        let row = [w1aw_at(D, 18, 2)];
        assert_eq!(
            pair_report_1_17(&pair, &row),
            [Some(0)],
            "1.17.0: the oldest contact on the row's day"
        );
        assert_eq!(pair_report(&pair, &row), [Some(1)], "now: the nearest");
        // Two rows, two contacts, consumed oldest first whatever the times.
        assert_eq!(
            pair_report_1_17(&pair, &[w1aw_at(D, 18, 2), w1aw_at(D, 6, 1)]),
            [Some(0), Some(1)]
        );
        // Its own UTC day first: 00:03 took that day's 00:20 over 23:58, five minutes earlier;
        // with nothing on its day, the day before; then the day after.
        let midnight = [w1aw_at(D - 1, 23, 58), w1aw_at(D, 0, 20)];
        assert_eq!(pair_report_1_17(&midnight, &[w1aw_at(D, 0, 3)]), [Some(1)]);
        let either_side = [w1aw_at(D - 1, 12, 0), w1aw_at(D + 1, 12, 0)];
        assert_eq!(
            pair_report_1_17(&either_side, &[w1aw_at(D, 12, 0)]),
            [Some(0)]
        );
        // Its own-QSO twin skipped the contacts award-confirmed as it ran.
        let mut confirmed = w1aw_at(D, 6, 0);
        confirmed.award_confirmed = true;
        assert_eq!(
            pair_own_echo_1_17(&pair, &row, |i| pair[i].award_confirmed),
            [Some(0)]
        );
        let skipped = [confirmed, w1aw_at(D, 18, 0)];
        assert_eq!(
            pair_own_echo_1_17(&skipped, &row, |i| skipped[i].award_confirmed),
            [Some(1)]
        );
    }

    #[test]
    fn an_upload_mark_lotw_does_not_hold_is_listed() {
        // 06:00 was never uploaded; 18:00 was, and LoTW's own-QSO list holds only it. 1.17.0's
        // pull marked the oldest contact Accepted, so 06:00 was never owed to LoTW again.
        let own = vec![w1aw_at(D, 18, 0)];
        let log = echoed_by_1_17(
            vec![
                w1aw_at(D, 6, 0),
                with_lotw(w1aw_at(D, 18, 0), UploadOutcome::Pending),
            ],
            &own,
        );
        assert_eq!(
            (lotw_outcome(&log[0]), lotw_outcome(&log[1])),
            (Some(UploadOutcome::Accepted), Some(UploadOutcome::Pending))
        );
        let check = check_report(&log, &[], &own, Channel::Lotw, OWN);
        assert_eq!(check.lines.len(), 1, "{:?}", check.lines);
        let line = &check.lines[0];
        assert_eq!(
            (line.index, line.mark, line.class),
            (0, Mark::LotwUpload, LineClass::Moved)
        );
        assert_eq!(line.sibling.map(|s| s.index), Some(1));
        assert_eq!(
            line.unticked, None,
            "LoTW's record is the 18:00 contact's own, to the minute"
        );
        // Not uploaded: owed to LoTW again, exactly as before 1.17.0's pull.
        let after = uncheck(&log[0], line).expect("06:00 still holds what the line shows");
        assert_eq!(after, w1aw_at(D, 6, 0));
    }

    #[test]
    fn a_contact_both_marks_were_misplaced_on_is_ticked_when_lotw_proves_it() {
        // 06:00 was never uploaded; 18:00 was, and LoTW's own-QSO list holds only it. 1.17.0's
        // pull marked 06:00 Accepted from 18:00's upload while 06:00 was unconfirmed, and a later
        // sync put 18:00's confirmation on it: 06:00 is award-confirmed now only by a LoTW mark
        // LoTW's download does not support.
        let own = vec![w1aw_at(D, 18, 0)];
        let rows = vec![lotw(w1aw_at(D, 18, 2), &["DXCC"])];
        let before = vec![
            w1aw_at(D, 6, 0),
            with_lotw(w1aw_at(D, 18, 0), UploadOutcome::Pending),
        ];
        let log = left_by_1_17(echoed_by_1_17(before.clone(), &own), &rows);
        assert!(log[0].qsl_rcvd.lotw && log[0].award_confirmed);
        assert_eq!(lotw_outcome(&log[0]), Some(UploadOutcome::Accepted));
        let check = check_report(&log, &rows, &own, Channel::Lotw, OWN);
        let ticked: Vec<(usize, Mark, Option<Unticked>)> = check
            .lines
            .iter()
            .map(|l| (l.index, l.mark, l.unticked))
            .collect();
        assert_eq!(
            ticked,
            [
                (0, Mark::Confirmation(Channel::Lotw), None),
                (0, Mark::LotwUpload, None)
            ],
            "the confirmation LoTW does not support leaves 06:00 in the pull's replay"
        );
        // Both changes put 06:00 back as it was before 1.17.0: owed to LoTW again.
        let after = check.lines.iter().fold(log[0].clone(), |r, line| {
            uncheck(&r, line).expect("06:00 still holds what each line shows")
        });
        assert_eq!(after, before[0]);

        // A paper card kept a contact out of the pull, and keeps it out of the replay: the same
        // marks on a contact holding one leave its upload line unticked.
        let mut card = w1aw_at(D, 6, 0);
        card.qsl_rcvd.card = true;
        card.confirmed = true;
        card.award_confirmed = true;
        let mut carded = echoed_by_1_17(before, &own);
        imported(&mut carded, 0, &card);
        let carded = left_by_1_17(carded, &rows);
        assert!(carded[0].qsl_rcvd.card && carded[0].qsl_rcvd.lotw);
        let check = check_report(&carded, &rows, &own, Channel::Lotw, OWN);
        let upload = check
            .lines
            .iter()
            .find(|l| l.mark == Mark::LotwUpload)
            .expect("the upload mark LoTW does not hold is listed");
        assert_eq!(
            (upload.index, upload.unticked),
            (0, Some(Unticked::NotReplayed))
        );
    }

    /// `r` as QRZ's record of it, confirmed by QRZ itself (`APP_QRZLOG_STATUS=C`): never an
    /// award-grade confirmation.
    fn qrz(mut r: QsoRecord) -> QsoRecord {
        r.qsl_rcvd.qrz = true;
        r.confirmed = true;
        r
    }

    /// `row` as QRZ's book can carry it: with QRZ's copies of what LoTW, eQSL and a paper card
    /// hold, and of LoTW's credit (its `LOTW_QSL_RCVD`, `EQSL_QSL_RCVD`, `QSL_RCVD` and
    /// `CREDIT_GRANTED`), beside whatever QRZ itself says.
    fn with_copies(mut row: QsoRecord) -> QsoRecord {
        row.qsl_rcvd.lotw = true;
        row.qsl_rcvd.eqsl = true;
        row.qsl_rcvd.card = true;
        row.confirmed = true;
        row.award_confirmed = true;
        row.credit_granted = vec!["DXCC".into()];
        row
    }

    /// `call` on 20 m FT8 at `h:m` UTC on the scenes' day.
    fn on_20m(call: &str, h: u64, m: u64) -> QsoRecord {
        let mut r = w1aw_at(D, h, m);
        r.call = call.into();
        r
    }

    /// QRZ's book, newest first as 1.17.0's merge read it, and the log that merge left: #400's
    /// pair on W1AW, 18:00's confirmation on 06:00, which QRZ holds to the minute, unconfirmed;
    /// K1ABC at 10:00 holding the confirmation of a 15:00 QSO this log lacks, while QRZ holds 10:00
    /// five minutes off, unconfirmed; and N0SUP at 12:00, which QRZ confirms. 06:00 also holds a
    /// LoTW confirmation of its own, with its DXCC.
    fn the_qrz_scenes() -> (Vec<QsoRecord>, Vec<QsoRecord>) {
        let book = vec![
            qrz(w1aw_at(D, 18, 0)),
            w1aw_at(D, 6, 0),
            qrz(on_20m("K1ABC", 15, 0)),
            on_20m("K1ABC", 10, 5),
            qrz(on_20m("N0SUP", 12, 0)),
        ];
        let mut log = vec![
            w1aw_at(D, 6, 0),
            w1aw_at(D, 18, 0),
            on_20m("K1ABC", 10, 0),
            on_20m("N0SUP", 12, 0),
        ];
        imported(&mut log, 0, &lotw(w1aw_at(D, 6, 0), &["DXCC"]));
        let log = left_by_1_17(log, &book);
        let held: Vec<bool> = log.iter().map(|r| r.qsl_rcvd.qrz).collect();
        assert_eq!(
            held,
            [true, false, true, true],
            "1.17.0 put 18:00's confirmation on 06:00, and 15:00's on 10:00"
        );
        (log, book)
    }

    #[test]
    fn a_qrz_rows_copies_of_other_services_never_tick_or_move_a_line() {
        let (log, book) = the_qrz_scenes();
        let check = check_report(&log, &book, &book, Channel::Qrz, OWN);
        let seen: Vec<(usize, LineClass, Option<Unticked>, Codes)> = check
            .lines
            .iter()
            .map(|l| (l.index, l.class, l.unticked, l.remove.clone()))
            .collect();
        assert_eq!(
            seen,
            [
                (0, LineClass::Contradicted, None, Codes::default()),
                (
                    2,
                    LineClass::Contradicted,
                    Some(Unticked::NotToTheMinute),
                    Codes::default()
                ),
            ],
            "QRZ's own unconfirmed records decide both, and neither takes a code off"
        );
        assert_eq!(check.gains, [1], "18:00 gains QRZ's confirmation");
        // The same book, its rows carrying QRZ's copies of LoTW's, eQSL's and a card's
        // confirmation and of LoTW's credit: not one line moves, ticks or takes a code off.
        let copied: Vec<QsoRecord> = book.into_iter().map(with_copies).collect();
        assert_eq!(
            check_report(&log, &copied, &copied, Channel::Qrz, OWN),
            check,
            "QRZ's copies of other services' word are not QRZ's"
        );
        // The change takes off QRZ's confirmation alone: 06:00 keeps its own LoTW confirmation
        // and its DXCC.
        let after = uncheck(&log[0], &check.lines[0]).expect("06:00 holds what the line shows");
        assert!(!after.qsl_rcvd.qrz);
        assert!(
            after.qsl_rcvd.lotw && after.award_confirmed && after.credit_granted == ["DXCC"],
            "{after:?}"
        );
    }

    #[test]
    fn a_contacts_lines_of_every_service_apply_whichever_goes_first() {
        // #400's pair, with every service's confirmation of 18:00 put on 06:00 by 1.17.0's
        // matcher: LoTW's (its row at 18:02, granting DXCC), eQSL's (the card its sender timed
        // 18:01) and QRZ's (its record of 18:00; it holds 06:00 unconfirmed).
        let lotw_rows = vec![lotw(w1aw_at(D, 18, 2), &["DXCC"])];
        let eqsl_rows = vec![eqsl(w1aw_at(D, 18, 1))];
        let book = vec![qrz(w1aw_at(D, 18, 0)), w1aw_at(D, 6, 0)];
        let before = vec![w1aw_at(D, 6, 0), w1aw_at(D, 18, 0)];
        let log = left_by_1_17(
            left_by_1_17(left_by_1_17(before.clone(), &lotw_rows), &eqsl_rows),
            &book,
        );
        assert!(log[0].qsl_rcvd.lotw && log[0].qsl_rcvd.eqsl && log[0].qsl_rcvd.qrz);
        let lines: Vec<CheckLine> = [
            check_report(&log, &lotw_rows, &[], Channel::Lotw, OWN),
            check_report(&log, &eqsl_rows, &[], Channel::Eqsl, OWN),
            check_report(&log, &book, &book, Channel::Qrz, OWN),
        ]
        .into_iter()
        .flat_map(|c| c.lines)
        .collect();
        let marks: Vec<(usize, Mark, bool)> = lines
            .iter()
            .map(|l| (l.index, l.mark, l.decisive()))
            .collect();
        assert_eq!(
            marks,
            [
                (0, Mark::Confirmation(Channel::Lotw), true),
                (0, Mark::Confirmation(Channel::Eqsl), true),
                (0, Mark::Confirmation(Channel::Qrz), true),
            ]
        );
        // Each line still holds once another of the contact's lines has gone first, in either
        // order: 06:00 ends as it was before 1.17.0.
        for order in [lines.clone(), lines.iter().rev().cloned().collect()] {
            let after = order
                .iter()
                .try_fold(log[0].clone(), |r, line| uncheck(&r, line));
            assert_eq!(after.as_ref(), Some(&before[0]));
        }
    }

    #[test]
    fn apply_gives_a_contact_qrzs_own_confirmation_and_nothing_else_its_book_holds() {
        let (log, book) = the_qrz_scenes();
        let copied: Vec<QsoRecord> = book.into_iter().map(with_copies).collect();
        let mut after = log.clone();
        assert_eq!(gain_qrz_confirmations(&mut after, &copied), 1);
        // 18:00 gains QRZ's confirmation, and nothing else its row re-reports; 06:00 and 10:00,
        // which QRZ holds unconfirmed, take nothing from their rows; and K1ABC's 15:00 QSO stays
        // out of the log, for Sync from QRZ to add.
        let mut gained = log.clone();
        gained[1].qsl_rcvd.qrz = true;
        gained[1].confirmed = true;
        assert_eq!(after, gained);
    }

    /// A contact or a row: two stations, two bands, three modes in two classes, over three days,
    /// with times bunched so rows fall within 30 minutes of two contacts, across midnight and
    /// inside one minute, and some with no time of day; any confirmations, calls, upload marks
    /// and codes.
    fn arb_qso() -> impl Strategy<Value = QsoRecord> {
        (
            prop::sample::select(vec!["W1AW", "K1ABC"]),
            prop::sample::select(vec!["20m", "40m"]),
            prop::sample::select(vec!["FT8", "FT4", "CW"]),
            0u64..3,
            prop::sample::select(vec![0u64, 1, 15, 29, 30, 31, 45, 60, 720, 1439]),
            0u64..3,
            0u8..8,
            any::<[bool; 4]>(),
            0usize..3,
            0usize..3,
            any::<[bool; 2]>(),
        )
            .prop_map(
                |(call, band, mode, day, minute, third, known, q, station, upload, codes)| {
                    let mut r = rec(call, band, mode, D + day);
                    r.time_known = known > 0;
                    r.when_unix = (D + day) * 86_400;
                    if r.time_known {
                        r.when_unix += minute * 60 + third * 20;
                    }
                    r.qsl_rcvd = QslRcvd {
                        card: q[0],
                        lotw: q[1],
                        eqsl: q[2],
                        qrz: q[3],
                    };
                    r.confirmed = r.qsl_rcvd.any();
                    r.award_confirmed = r.qsl_rcvd.award();
                    r.station_callsign =
                        [None, Some("KD9TAW"), Some("N0OLD")][station].map(String::from);
                    r.upload.lotw = [
                        None,
                        Some(UploadOutcome::Pending),
                        Some(UploadOutcome::Accepted),
                    ][upload]
                        .map(|outcome| UploadStatus {
                            outcome,
                            when_unix: 1,
                            detail: None,
                        });
                    r.credit_granted = [("DXCC", codes[0]), ("WAS", codes[1])]
                        .iter()
                        .filter(|c| c.1)
                        .map(|c| c.0.to_string())
                        .collect();
                    r
                },
            )
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

        /// ★ A check never disagrees with the merge. A contact is supported, and never listed,
        /// exactly when the merge pairs a row carrying the service's confirmation with it; one
        /// lacking the confirmation is then a gain; and every other contact holding it is listed
        /// or counted, once.
        #[test]
        fn a_check_never_disagrees_with_the_merge(
            log in prop::collection::vec(arb_qso(), 0..10),
            rows in prop::collection::vec(arb_qso(), 0..10),
            own in prop::collection::vec(arb_qso(), 0..6),
            channel in prop::sample::select(vec![Channel::Lotw, Channel::Eqsl, Channel::Qrz]),
            scoped in any::<bool>(),
        ) {
            let check = check_report(&log, &rows, &own, channel, scoped.then_some("KD9TAW"));
            // What the merge pairs, read off the merge itself: from a copy holding no
            // confirmation, exactly the contacts a confirming row lands on come out holding one.
            let mut bare: Vec<QsoRecord> = log
                .iter()
                .cloned()
                .map(|mut r| {
                    r.qsl_rcvd = QslRcvd::default();
                    r
                })
                .collect();
            reconcile(&mut bare, &rows);
            let mark = Mark::Confirmation(channel);
            let mut unsupported = 0;
            for (i, r) in log.iter().enumerate() {
                let merged = channel.held(&bare[i].qsl_rcvd);
                let held = channel.held(&r.qsl_rcvd);
                let listed = check.lines.iter().filter(|l| l.index == i && l.mark == mark).count();
                prop_assert_eq!(check.gains.contains(&i), merged && !held, "gain {}", i);
                prop_assert!(listed <= 1, "{} listed twice", i);
                prop_assert!(listed == 0 || (held && !merged), "{} listed", i);
                if held && !merged {
                    unsupported += 1;
                }
            }
            let lines = check.lines.iter().filter(|l| l.mark == mark).count();
            prop_assert_eq!(lines + check.flags.unreached + check.flags.out_of_scope, unsupported);
        }
    }
}
