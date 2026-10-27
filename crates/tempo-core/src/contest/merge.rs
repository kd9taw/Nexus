//! The general-log seam: a contest log's rows become `QsoRecord`s, once.
//!
//! ⭐ **This is the ONLY path by which a contest contact becomes a general-log record,
//! and that is why it needed a ruling before it could be written.** A Field Day contact
//! never reaches the general upload queue — `fd_log_contact` writes the contest log and
//! nothing else, pinned by a test that says so — and the reason is that the general log
//! path enqueues every record it writes to every connector. Merging 800 contest rows
//! through it would not enqueue 800: both sinks are bounded ring buffers that drop from
//! the front at 256, so the morning's contacts would be discarded silently. A slow
//! upload is a chore; a queue that quietly eats two-thirds of a contest is a data-loss
//! report.
//!
//! So the merge here **writes and does not enqueue**. It hands its caller the records it
//! wrote; whether any of them are queued for upload is the session's
//! [`UploadPolicy`](super::session::UploadPolicy) — per session, default OFF — and the
//! decision belongs to the one caller that owns a connector queue, not to this module.
//!
//! ⭐ **Idempotence, and the mechanism rather than the promise.** Every row carries
//! `APP_NEXUS_QID` = `"<session>:<posid>:<seq>"`. `(posid, seq)` is already the
//! club-sync row identity — monotonic per position and restored across restart — so
//! nothing new had to be invented to make re-running the merge safe. A row whose copy is
//! already in the general log is skipped. This holds **across a restart** only because
//! the ADIF parser consumes the tag back into the record rather than leaving it in
//! `extra`; a merge-twice test that never drops the `Logbook` would pass with no reader
//! at all.
//!
//! ⭐ **The session part names the contest and the running a row was logged in — never where
//! the operator was** ([`session_of`]). It was `"<contest id>:<county, state or section>"`,
//! built from the location in Settings each time the contest was entered, so after "I moved"
//! (or a County edited in Settings) and a restart the same contacts had new names and a second
//! merge wrote every one of them again; and the same contest next year, from the same place,
//! minted last year's names and the merge skipped every contact as already there. Both halves
//! of the new name are facts of the ROW, on it in the journal and never changed: its contest,
//! and the running its time falls in. The copies merged under the old names stay in the
//! Logbook exactly as written, and are still recognised ([`MergeIdentity::names`]).
use super::render::sent_exchange;
use super::spec::FieldKind;
use crate::fd_rules::FdRuleset;
use crate::fieldday::{FieldDayLog, LoggedQso};
use crate::logbook::{ContestFields, Logbook, QslRcvd, QslSent, QsoRecord, UploadState};

/// ⭐ **The merge identity for one contest row** — `"<session>:<posid>:<seq>"`.
///
/// ⚠️ **There is no `qid` on a contest row.** [`LoggedQso`] does not carry one; the
/// contest-side identity is [`LoggedQso::seq`](crate::fieldday::LoggedQso::seq) and the
/// qid is MINTED here, from it. That direction is what makes a correction routable:
/// the general log names a row by qid, and [`merged_row`] turns that back into the
/// seq that [`FieldDayLog::correct_row`](crate::fieldday::FieldDayLog::correct_row)
/// addresses.
///
/// Minting and parsing are one pair of functions for the usual reason: two sites that
/// each build this string by hand are two sites that come to disagree about it. The
/// session is [`session_of`] the row ([`MergeIdentity::of`] mints with both).
pub fn qid_for(session_id: &str, posid: &str, seq: u64) -> String {
    format!("{session_id}:{posid}:{seq}")
}

/// ⭐ **The session a contest row belongs to, as its merge identity names it** —
/// `"<contest id>:<running>"`: the contest the log runs, and the running of it the row's own
/// time falls in ([`FdRuleset::running_of`]), the same answer the journal restore groups a
/// session's rows by.
///
/// ⚠️ **Nothing about where the operator is goes into it, and that is the fix.** This was the
/// session's id, `"<contest id>:<county, state or section>"`, built from Settings whenever the
/// contest was entered — so "I moved", or a County edited in Settings, renamed every contact at
/// the next restart and the merge wrote them all again. The contest and the row's time are
/// journaled with the row and never change; the running is what tells this year's contacts
/// from last year's, whose numbers start at 1 too.
pub fn session_of(log: &FieldDayLog, q: &LoggedQso) -> String {
    running_session(
        &log.session.contest_id,
        log.ruleset().running_of(q.when_unix),
    )
}

/// `"<contest id>:<running>"`: [`session_of`], from the two facts it is made of.
fn running_session(contest_id: &str, running: u16) -> String {
    format!("{contest_id}:{running}")
}

/// ⭐ **One contest row's merge identity** — the qid this build mints for it, and the test of
/// whether a general-log copy is that row's, under that name or under the one an earlier build
/// gave it.
///
/// Owned, so a caller that reads the general log with the Engine lock released can carry it
/// there. Two are equal when they mint the same qid.
#[derive(Clone)]
pub struct MergeIdentity {
    /// [`session_of`] the row.
    session: String,
    /// `"<session>:<posid>:<seq>"`.
    qid: String,
    contest_id: String,
    posid: String,
    seq: u64,
    /// The running the row was logged in, and the rules that say which running a time is in.
    running: u16,
    rules: &'static FdRuleset,
}

impl MergeIdentity {
    /// Row `q` of `log`, merged at position `posid`.
    pub fn of(log: &FieldDayLog, posid: &str, q: &LoggedQso) -> Self {
        Self::of_contest(
            &log.session.contest_id,
            log.ruleset(),
            posid,
            q.seq,
            q.when_unix,
        )
    }

    /// Row `seq` of contest `contest_id`, logged at `when_unix` under `rules`, merged at position
    /// `posid`: [`of`](Self::of) for a row no log holds, one the contest journal keeps past its
    /// window ([`settle_aged`]). One minting for both, so the two cannot come to disagree.
    pub(crate) fn of_contest(
        contest_id: &str,
        rules: &'static FdRuleset,
        posid: &str,
        seq: u64,
        when_unix: u64,
    ) -> Self {
        let running = rules.running_of(when_unix);
        let session = running_session(contest_id, running);
        MergeIdentity {
            qid: qid_for(&session, posid, seq),
            session,
            contest_id: contest_id.to_string(),
            posid: posid.to_string(),
            seq,
            running,
            rules,
        }
    }

    /// The qid this build mints for the row.
    pub fn qid(&self) -> &str {
        &self.qid
    }

    /// ⭐ **Whether the general-log copy carrying `qid`, made at `when_unix`, is this row's** — the
    /// one test every reader of a copy's name uses: the merge (already there?), a Logbook edit and
    /// a Logbook delete (which contest row?), and a removal (which copy did the logbook upload?).
    ///
    /// Either the qid this build mints, or **a name an earlier build gave the same row**:
    /// `"<contest id>:<where the operator was>:<posid>:<seq>"` — the same contest, position and
    /// number, on a copy made in the running this row was logged in. Those copies are in the
    /// operator's Logbook as they were written and are never renamed, so they are recognised
    /// here instead, whatever the place they name: the one the contacts were sent from, or
    /// one a restart read off Settings after a move.
    ///
    /// ⚠️ **The running is what makes the old names safe to read.** They carry no year, and the
    /// numbers start at 1 at every contest, so `"<contest>:KANE:<posid>:1"` names last year's
    /// first contact as well as this year's; the copy's own time says which.
    pub fn names(&self, qid: &str, when_unix: u64) -> bool {
        if qid == self.qid {
            return true;
        }
        let Some((session, posid, seq)) = split_qid(qid) else {
            return false;
        };
        seq == self.seq
            && posid == self.posid
            && session
                .strip_prefix(self.contest_id.as_str())
                .is_some_and(|rest| rest.starts_with(':'))
            && self.rules.running_of(when_unix) == self.running
    }
}

impl PartialEq for MergeIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.qid == other.qid
    }
}

impl Eq for MergeIdentity {}

impl std::fmt::Debug for MergeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.qid)
    }
}

/// The live row of `log` that the general-log copy carrying `qid`, made at `when_unix`, was
/// merged from at position `posid` — its `seq`, for
/// [`FieldDayLog::correct_row`](crate::fieldday::FieldDayLog::correct_row) and
/// [`FieldDayLog::remove_row`](crate::fieldday::FieldDayLog::remove_row). `None` for a copy of
/// another contest's, another running's or another position's row, for a row this log does
/// not hold live, and for anything that is not a merge identity.
///
/// The seq alone names a row in every log the operator has ever run (seqs are per position and
/// restart at 1), so the row it finds is held to the whole identity ([`MergeIdentity::names`]).
pub fn merged_row(log: &FieldDayLog, posid: &str, qid: &str, when_unix: u64) -> Option<u64> {
    let (_, _, seq) = split_qid(qid)?;
    let q = log.qsos().iter().find(|q| q.seq == seq)?;
    MergeIdentity::of(log, posid, q)
        .names(qid, when_unix)
        .then_some(seq)
}

/// A qid's `(session, posid, seq)`.
///
/// ⚠️ **Parsed from the RIGHT, and that is not a style choice.** A session contains a colon of
/// its own (`"CQ-WW-CW:2026"`, and an earlier build's `"CQ-WW-CW:IL"`), so a real qid has four
/// colon-separated parts, and splitting three ways from the left reads the posid as the running
/// and the seq as `"7:42"`. The seq is the last field and the posid the one before it;
/// everything to their left is the session.
///
/// A zero seq is UNSTAMPED, never a row: the merge refuses those rows, so no qid naming one was
/// ever written. Refusing it here keeps that true on the way back.
fn split_qid(qid: &str) -> Option<(&str, &str, u64)> {
    let (head, seq) = qid.rsplit_once(':')?;
    let (session, posid) = head.rsplit_once(':')?;
    let seq = seq.trim().parse::<u64>().ok().filter(|&n| n > 0)?;
    Some((session, posid, seq))
}

/// What one merge did (§3.2).
#[derive(Debug, Clone, Default)]
pub struct MergeReport {
    /// The records actually written, in log order — handed back rather than counted so
    /// the caller can persist them and, if the session says so, queue them, without
    /// re-deriving which rows were new.
    pub written: Vec<QsoRecord>,
    /// Rows whose copy was already in the general log ([`MergeIdentity::names`]). On a
    /// second run of the same merge this is every row, and `written` is empty.
    pub already: usize,
    /// Rows with no stable identity — see [`merge_into_general`]. Reported, never
    /// silently dropped and never silently duplicated.
    pub refused: usize,
}

impl MergeReport {
    /// How many rows this merge wrote.
    pub fn added(&self) -> usize {
        self.written.len()
    }
}

/// Merge a contest log into the general log, skipping every row already there.
///
/// ⚠️ **A free function rather than a method on [`ContestSession`](super::ContestSession),
/// and the deviation from §3.2's sketch is mechanical.** The log already carries its own
/// session (`FieldDayLog.session`), so a method taking both a `&self` session and a log
/// would admit a caller that passes a session the log does not belong to — and the qid
/// this whole function turns on is built from the log's contest. One argument, one session,
/// no way to disagree.
///
/// `posid` is the local position's machine id (the club-sync half of a row identity). It
/// lives in settings, above this crate, so it is passed in.
///
/// **Rows with `seq == 0` are refused, not merged.** Every such row would build the same
/// qid, so merging them is either a silent duplicate on the second run or a silent
/// collapse on the first; refusing and reporting is the only honest third answer. Live
/// logging stamps a sequence and a journal restore backfills one, so this is the
/// defensive path for a journal that has been hand-edited or truncated — which is a file
/// on disk, and this build does not get to assume it is well formed.
pub fn merge_into_general(log: &FieldDayLog, posid: &str, into: &mut Logbook) -> MergeReport {
    let copies: Vec<(String, u64)> = into
        .records()
        .iter()
        .filter_map(|r| {
            r.contest
                .as_deref()
                .filter(|c| !c.qid.is_empty())
                .map(|c| (c.qid.clone(), r.when_unix))
        })
        .collect();
    let mut report = plan_merge(log, posid, copies);
    for rec in &mut report.written {
        // The log mints the row's id; the copy reported as written carries it too.
        rec.id = Some(into.add(rec.clone()));
    }
    report
}

/// [`merge_into_general`]'s decision with the general log's merge identities named rather than
/// read: `copies` is every merged copy the general log holds, as its `APP_NEXUS_QID` and when it
/// was made (`QsoRecord::when_unix`), and the report's `written` are the rows to append, in
/// order, WITHOUT ids — for a caller whose general log is not a [`Logbook`] in hand. The station
/// appends them itself and mints their ids (SPEC-2 v3 C19); the rule for what is merged, refused
/// or already there is this one function either way.
pub fn plan_merge(
    log: &FieldDayLog,
    posid: &str,
    copies: impl IntoIterator<Item = (String, u64)>,
) -> MergeReport {
    // The copies by the number they name: a row is tested against its own number's copies (one,
    // or the few an earlier build's renames wrote), never the whole log.
    let mut by_seq: std::collections::HashMap<u64, Vec<(String, u64)>> =
        std::collections::HashMap::new();
    for (qid, when_unix) in copies {
        if let Some((_, _, seq)) = split_qid(&qid) {
            by_seq.entry(seq).or_default().push((qid, when_unix));
        }
    }
    let mut report = MergeReport::default();
    let numbers = log.qso_numbers();
    for q in log.qsos() {
        if q.seq == 0 {
            report.refused += 1;
            continue;
        }
        let id = MergeIdentity::of(log, posid, q);
        let there = by_seq.entry(q.seq).or_default();
        if there
            .iter()
            .any(|(qid, when_unix)| id.names(qid, *when_unix))
        {
            report.already += 1;
            continue;
        }
        // A copy written by THIS run counts as there too — the set is what the function
        // means by "already there", and a set that only knew about the log it started
        // with would be true of the first duplicate and false of the second.
        there.push((id.qid.clone(), q.when_unix));
        report
            .written
            .push(record_for(log, q, &id, numbers.get(&q.seq).copied()));
    }
    report
}

/// The contacts of one contest that a contest journal keeps past its window and the Logbook does
/// not hold ([`settle_aged`]): how many, and when the first and the last of them were logged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmerged {
    /// The contest's rules-file event id.
    pub event: &'static str,
    pub count: usize,
    pub first_unix: u64,
    pub last_unix: u64,
}

/// ⭐ **The contacts a contest journal keeps past its window, asked of the Logbook** — the ones
/// it no longer needs let go, the ones the Logbook does not hold answered.
///
/// `copies` is every merged copy the Logbook holds, as [`plan_merge`] takes them, and `on_disk`
/// those of them already on disk. A contact whose copy is on disk, made at the contact's own
/// time, is let go ([`FieldDayLog::let_go_aged`]) once its contest's running is over at
/// `now_unix`: it ages out of the journal as a merged contact always did. ⭐ Until then it stays,
/// copy or no copy: a session of its contest loads it whatever its age, and that contest's log
/// and the files exported from it hold the whole running. A contact no copy names is answered,
/// by contest, for the contest screen to offer
/// its merge ([`plan_aged_merge`]). Any other is kept and not offered: its copy is still on its
/// way to disk, or names it at another time — the copy's time was changed in the Logbook, or the
/// copy is another contact's, whose number an earlier build gave this one again.
///
/// ⚠️ **A contact with no merge identity is kept and never offered**: one that carries no number
/// (a journal no build since club sync has written), or names a contest the loaded rules do not
/// hold. Nothing can merge it without giving it a name another contact may hold.
pub fn settle_aged(
    log: &mut FieldDayLog,
    posid: &str,
    copies: &[(String, u64)],
    on_disk: &[(String, u64)],
    now_unix: u64,
) -> Vec<Unmerged> {
    let (copies, on_disk) = (by_seq(copies), by_seq(on_disk));
    let mut gone = Vec::new();
    let mut unmerged: Vec<Unmerged> = Vec::new();
    for (at, c) in log.aged_contacts(posid).into_iter().enumerate() {
        let (Some(id), Some(rules)) = (c.identity, c.rules) else {
            continue;
        };
        let event = rules.event;
        if named(&on_disk, &id, Some(c.when_unix)) {
            if rules.running_of(now_unix) != rules.running_of(c.when_unix) {
                gone.push(at);
            }
        } else if !named(&copies, &id, None) {
            match unmerged.iter_mut().find(|u| u.event == event) {
                Some(u) => {
                    u.count += 1;
                    u.first_unix = u.first_unix.min(c.when_unix);
                    u.last_unix = u.last_unix.max(c.when_unix);
                }
                None => unmerged.push(Unmerged {
                    event,
                    count: 1,
                    first_unix: c.when_unix,
                    last_unix: c.when_unix,
                }),
            }
        }
    }
    log.let_go_aged(&gone);
    unmerged
}

/// ⭐ **The merge the contest screen offers for the contacts of contest `event` that a contest
/// journal keeps past its window and no copy in the Logbook names** ([`settle_aged`]) — through
/// [`plan_merge`], the one merge there is, so a contact already there is never written again.
///
/// Each is read back alone into a log on `session`, a session of that contest
/// ([`FieldDayLog::aged_log`]), so the identity it is merged under is the one [`settle_aged`]
/// asked about. `copies` as [`plan_merge`] takes them. Nothing is let go here: a contact goes
/// once its copy is on disk, the next time the Logbook is asked.
pub fn plan_aged_merge(
    log: &FieldDayLog,
    event: &str,
    session: &super::ContestSession,
    posid: &str,
    copies: Vec<(String, u64)>,
) -> MergeReport {
    let there = by_seq(&copies);
    let at: Vec<usize> = log
        .aged_contacts(posid)
        .into_iter()
        .enumerate()
        .filter(|(_, c)| {
            c.rules.is_some_and(|rs| rs.event == event)
                && c.identity
                    .as_ref()
                    .is_some_and(|id| !named(&there, id, None))
        })
        .map(|(at, _)| at)
        .collect();
    plan_merge(&log.aged_log(&at, session.clone()), posid, copies)
}

/// `copies` by the number each names, so that a row is tested against its own number's copies
/// alone, as [`plan_merge`] tests them.
fn by_seq(copies: &[(String, u64)]) -> std::collections::HashMap<u64, Vec<(&str, u64)>> {
    let mut by: std::collections::HashMap<u64, Vec<(&str, u64)>> = Default::default();
    for (qid, when_unix) in copies {
        if let Some((_, _, seq)) = split_qid(qid) {
            by.entry(seq).or_default().push((qid, *when_unix));
        }
    }
    by
}

/// Whether a copy in `by` is `id`'s ([`MergeIdentity::names`]) — one made at `at` only, when
/// given.
fn named(
    by: &std::collections::HashMap<u64, Vec<(&str, u64)>>,
    id: &MergeIdentity,
    at: Option<u64>,
) -> bool {
    by.get(&id.seq).is_some_and(|there| {
        there
            .iter()
            .any(|&(qid, when)| at.is_none_or(|t| t == when) && id.names(qid, when))
    })
}

/// One contest row as a general-log record. `number`: the row's QSO number
/// ([`FieldDayLog::qso_numbers`]), which is its `STX` in a contest that sends no serial.
fn record_for(
    log: &FieldDayLog,
    q: &LoggedQso,
    id: &MergeIdentity,
    number: Option<u32>,
) -> QsoRecord {
    let spec = log.session.exchange;
    // The SENT side comes from the one renderer, which takes a ROW — never from the
    // session, which holds only what is being sent right now.
    let sent = sent_exchange(q, spec);
    // The report each way, when the exchange declares one: CQ WW, CQ WPX and the state
    // QSO parties do. Neither Field Day event does, and those stay `None`. Inventing 599
    // would be a claim about the air that nobody made.
    let (rst_sent, rst_rcvd) = (rst_of(&sent, spec), rst_of(&q.rx, spec));
    // The serial it sent, or in a contest that sends none the number the log gave it.
    let (stx, srx) = (serial_of(&sent, spec).or(number), serial_of(&q.rx, spec));
    let mut contest = ContestFields {
        session: id.session.clone(),
        // A mode-split contest has a distinct id per mode, and a general-log record
        // is one row, so it resolves against ITS OWN class. `Err` is unreachable for
        // an unsplit contest and, for a split one, means the ruleset declares no id
        // for this row's mode — which `cabrillo()` refuses loudly at export, where
        // the operator can see it. A merge must not drop a contact over it.
        contest_id: log
            .session
            .contest_id_for(&[q.mode.as_str()])
            .unwrap_or_else(|_| log.session.contest_id.clone()),
        stx,
        stx_string: joined(sent.iter().map(|v| v.raw.as_str())),
        srx,
        srx_string: joined(q.rx.iter().map(|v| v.raw.as_str())),
        sent: pairs(&sent),
        rcvd: pairs(&q.rx),
        // ⭐ The DIRECTED standard columns, resolved here because this is the only
        // place the exchange spec is in hand (§2.1.1). Downstream sees `(tag, value)`
        // and never has to decide which way round a slot exports.
        //
        // Minus a column the record carries in a field of its own: the reports in
        // `rst_sent`/`rst_rcvd` and the serials in `stx`/`srx`. Kept here as well, the
        // file would write `RST_SENT` or `STX` twice.
        adif: super::adif::directed_columns(q, spec)
            .into_iter()
            .filter(|(tag, _)| match tag.as_str() {
                "RST_SENT" => rst_sent.is_none(),
                "RST_RCVD" => rst_rcvd.is_none(),
                "STX" => stx.is_none(),
                "SRX" => srx.is_none(),
                _ => true,
            })
            .collect(),
        qid: id.qid.clone(),
    };
    // ⭐ THE RECEIVED COLUMNS THE LOGBOOK HOLDS IN A FIELD OF ITS OWN — the grid a VHF contest's
    // exchange carries, a name and a QTH — moved out of the contest columns into that field, as
    // the reports and serials above are. Left there, the value reached the file and never the
    // Logbook's own column: a VHF contest's grids were missing from the Grid column and from
    // everything that reads it until the row was imported again. Moved rather than copied, so
    // the file writes each tag once.
    let mut own = |tag: &str| {
        let at = contest.adif.iter().position(|(t, _)| t == tag)?;
        Some(contest.adif.remove(at).1)
    };
    let (their_grid, name, qth) = (own("GRIDSQUARE"), own("NAME"), own("QTH"));
    // The grid stamped on the row at log time, unless the exchange itself sent one (a VHF
    // contest's `GRID`, a rover's grid at that moment): that is written as `MY_GRIDSQUARE`
    // from the contest's own columns, and wins.
    let my_grid = Some(q.grid.clone()).filter(|g| {
        !g.trim().is_empty() && !contest.adif.iter().any(|(t, _)| t == "MY_GRIDSQUARE")
    });
    // The call the contact went out under: a club's rebuild names a GOTA station's own
    // ([`LoggedQso::station_call`]), else the call on the air when it was logged, else the log's.
    let station_callsign = [&q.station_call, &q.on_air_call, &log.mycall]
        .into_iter()
        .find(|c| !c.trim().is_empty())
        .cloned();
    let mut rec = QsoRecord {
        id: None,
        call: q.call.clone(),
        grid: their_grid,
        country: q.entity.clone(),
        // ⭐ THE STATE THEY SENT, where the exchange carries one (see `received_state` for
        // what counts). Written before the engine's merge-time fill, which resolves only
        // an empty state, so the exchange wins over the callsign resolver: the FCC index
        // answers with a licensee's mailing address, the exchange with where the station
        // is. This is the field every upload service reads (World Radio League's contest
        // log among them); `SRX_STRING` and the private carrier reach none of them.
        state: super::adif::received_state(q, spec).map(str::to_string),
        band: q.band.clone(),
        // ⭐ **THE DIAL THE CONTACT WAS WORKED ON.** This was `0.0` under a comment saying
        // a contest row records the band — which is wrong about the artifact:
        // [`LoggedQso::freq_khz`] is stamped at log time from the dial, the contest log's
        // own Cabrillo writes it in place of the band edge, and its ADIF writes `FREQ`
        // from it. So a whole contest reached the lifetime log — the file that goes to
        // LoTW, QRZ and ClubLog — with `BAND` and nothing else, and on VHF that loses the
        // segment: 144.200 SSB and 146.520 FM are both `2m`.
        //
        // A row that genuinely has no dial keeps `0.0`, and `logbook::adif_record` omits
        // the field rather than emit the `<FREQ:8>0.000000` that DXKeeper and Swisslog
        // reject the whole record over — which is the half of the old comment that was
        // true, and it still holds.
        //
        // ⚠️ Field Day is NOT excepted here, and its own ADIF export still is. That
        // exception (`FieldDayLog::adif`) is about the contest journal and the submitted
        // file, both pinned byte for byte by the §8(a) goldens; the lifetime log is a
        // different artifact with a different job, and an FD contact in it has the same
        // claim to its frequency as any other.
        // ⚠️ THE ON-AIR PAIR, NOT THE DIAL. This read `freq_khz` — the frequency the
        // operator was LISTENING on, rounded to the kHz Cabrillo writes in — and hardcoded
        // `freq_rx_mhz: None`. Two ways that was wrong in the file that goes to LoTW, QRZ
        // and Club Log: a contact worked SPLIT recorded the receive leg as `FREQ` and never
        // said so, and every digital contact recorded the bare dial instead of dial + the
        // TX audio offset, so an operator's FT8 contest rows and their ordinary FT8 rows
        // described the same contact two different ways.
        //
        // Both legs are now stamped at log time from `Engine::log_frequencies`, the general
        // log's own answer, so this copies rather than re-deriving — see
        // [`LoggedQso::on_air_mhz`]. A row with no on-air value falls back to the dial,
        // which is all a pre-pair row ever knew; `logbook::adif_record` still omits a zero
        // rather than emit the `<FREQ:8>0.000000` that DXKeeper and Swisslog reject.
        freq_mhz: if q.on_air_hz > 0 {
            q.on_air_hz as f64 / 1e6
        } else {
            f64::from(q.freq_khz) / 1000.0
        },
        freq_rx_mhz: q.freq_rx_hz.filter(|hz| *hz > 0).map(|hz| hz as f64 / 1e6),
        mode: q.recorded_mode().to_string(),
        rst_sent,
        rst_rcvd,
        name,
        qth,
        // A contest row holds no comment and no private note: the entry strip takes the call
        // and the exchange, nothing else ([`LoggedQso`]).
        comment: None,
        notes: None,
        // The power stamped on the row at log time, as the general log writes it; a row from
        // before it was recorded carries none.
        tx_power: q
            .tx_power
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|w| w.is_finite() && *w > 0.0),
        when_unix: q.when_unix,
        // A legacy row with no stamp keeps the placeholder rather than a fabricated
        // midnight — the same rule the ADIF importer applies.
        time_known: q.when_unix > 0,
        // A contest row records one moment, the one it was logged at, which is when the
        // contact ended: it is the row's end time as well as its start.
        time_off_unix: (q.when_unix > 0).then_some(q.when_unix),
        confirmed: false,
        award_confirmed: false,
        qsl_rcvd: QslRcvd::default(),
        qsl_sent: QslSent::default(),
        credit_granted: Vec::new(),
        credit_submitted: Vec::new(),
        upload: UploadState::default(),
        // MY side: the activation the row was logged during ([`LoggedQso::activation`]), as the
        // general log's funnel stamps an ordinary contact made then — so it counts for the
        // activation too. THEIR side: no exchange carries a park, summit or island, so there is
        // nothing to carry, not something dropped.
        ota: crate::logbook::Ota {
            my_program: Some(q.activation.program.clone())
                .filter(|_| !q.activation.reference.is_empty()),
            my_ref: Some(q.activation.reference.clone()).filter(|r| !r.is_empty()),
            ..crate::logbook::Ota::default()
        },
        dxcc: None,
        // ⭐ **THE BIRD SURVIVES THE MERGE.** These were hardcoded `None`, which was true
        // of every contest row there had ever been — and became a silent data loss the
        // moment the satellite strip could log into a running Field Day session: the
        // contest log is the only copy of that contact, so a pair dropped here is LoTW
        // satellite credit the operator never gets and a grid Nexus's own Satellite VUCC
        // and needs boards (`qso_is_sat` reads `PROP_MODE`) never see. Worse on a metre
        // band than a plain omission — an untagged 2 m pass contact is credited to
        // TERRESTRIAL VUCC, which is a wrong award, not a missing one.
        //
        // ⚠️ **BOTH OR NEITHER, by construction** — one `Option`, read twice. TQSL
        // hard-errors on a lone member and through the `-a compliant` funnel would wedge
        // the whole upload batch as Rejected. [`SatLeg::name`] is `None` for a bird LoTW
        // does not list, and then this row is an ordinary one exactly as before.
        prop_mode: q
            .sat
            .as_ref()
            .and_then(|s| s.name.as_ref())
            .map(|_| "SAT".to_string()),
        sat_name: q.sat.as_ref().and_then(|s| s.name.clone()),
        // ⭐ WHO WAS AT THE KEY, AND THE CALL IT WENT OUT UNDER — both stamped on the row at log
        // time and both dropped here until now: `OPERATOR` was hardcoded `None` and
        // `STATION_CALLSIGN` was the session's call, which after a call change and a restart is
        // not the call the contact was made on. An empty stamp is "nobody named" / "not
        // recorded" (a row from before `on_air_call`), and then the station call goes on: with
        // nobody named the operator is the station, as the general log's own funnel writes it.
        operator: Some(q.operator.clone())
            .filter(|o| !o.trim().is_empty())
            .or_else(|| station_callsign.clone()),
        my_grid,
        // The radio stamped on the row at log time — the general log's own answer then.
        my_rig: Some(q.rig.clone()).filter(|r| !r.trim().is_empty()),
        station_callsign,
        // The worked station's continent, from the country file's reading the row made at log
        // time (ADIF `CONT`), as an ordinary contact carries it.
        extra: q
            .continent
            .iter()
            .map(|c| ("CONT".to_string(), c.clone()))
            .collect(),
        contest: Some(Box::new(contest)),
    };
    // Settings' grid is Nexus's own fill, as the general log marks it, so the LoTW upload leaves
    // it out and TQSL signs from the Station Location as it did before.
    if rec.my_grid.is_some() {
        crate::logbook::mark_self_filled(&mut rec, "MY_GRIDSQUARE");
    }
    // The state the activation was made from, as the funnel writes it on an ordinary activation
    // contact (unmarked: it is the park's, not Settings'), unless the exchange itself sent one —
    // a state QSO party's `MY_STATE` — which is written from the contest's own columns, and wins.
    if !q.activation.state.is_empty()
        && !q.activation.reference.is_empty()
        && !rec
            .contest
            .as_deref()
            .is_some_and(|c| c.adif.iter().any(|(t, _)| t == "MY_STATE"))
    {
        crate::logbook::fill_extra(&mut rec, "MY_STATE", q.activation.state.clone());
    }
    // Into the lifetime log in its one form: a phone row worked on a sideband is SSB, with the
    // sideband as its SUBMODE ([`crate::logbook::canonical`]).
    crate::logbook::canonical::contact(&mut rec);
    rec
}

/// The numeric serial in a field vector, when the exchange declares one — ADIF's `STX`
/// and `SRX` are numbers, and a contest whose exchange is not a serial has neither.
fn serial_of(vals: &[super::spec::FieldValue], spec: &super::spec::ExchangeSpec) -> Option<u32> {
    vals.iter()
        .find(|v| {
            matches!(
                spec.field(v.key).map(|f| &f.kind),
                Some(FieldKind::Serial { .. })
            )
        })
        .and_then(|v| v.raw.trim().parse().ok())
}

/// The signal report in a field vector, when the exchange declares an RST slot — what
/// ADIF's `RST_SENT`/`RST_RCVD` hold. `None` for an exchange with no report, and for an
/// empty one.
///
/// The contest log's own submitted ADIF writes its `RST_SENT` with this too, so the two files
/// cannot disagree ([`FieldDayLog::submission_adif`]).
pub(crate) fn rst_of(
    vals: &[super::spec::FieldValue],
    spec: &super::spec::ExchangeSpec,
) -> Option<String> {
    vals.iter()
        .find(|v| {
            matches!(
                spec.field(v.key).map(|f| &f.kind),
                Some(FieldKind::Rst { .. })
            )
        })
        .map(|v| v.raw.trim().to_string())
        .filter(|r| !r.is_empty())
}

/// An exchange as `STX_STRING`/`SRX_STRING`: the raw values, space separated, empties
/// dropped. `None` rather than an empty tag when there is nothing to say.
fn joined<'a>(raws: impl Iterator<Item = &'a str>) -> Option<String> {
    let s = raws
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!s.is_empty()).then_some(s)
}

/// A field vector as the general log's `(slot, raw)` pairs — the domain deliberately
/// dropped, because the general log does not score and ADIF has nowhere to put one.
fn pairs(vals: &[super::spec::FieldValue]) -> Vec<(String, String)> {
    vals.iter()
        .map(|v| (v.key.to_string(), v.raw.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::{ContestSession, DupeRule};
    use crate::fieldday::FdEvent;

    const FD_RULE: DupeRule = DupeRule {
        by_call: true,
        by_band: true,
        by_mode_class: true,
        by_fields: &[],
        by_sent_fields: &[],
        mode_class_groups: &[],
        log_dupes: false,
        satellite_is_a_band: false,
        fm_satellite_once: false,
    };

    /// A unique scratch path under the OS temp dir — the shape `logbook.rs`'s own
    /// tests use, so the restart leg re-reads a REAL file through the real loader.
    fn scratch_adi() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tempo_mergetest_{}_{n}.adi", std::process::id()))
    }

    fn fd_log() -> FieldDayLog {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_800));
        log
    }

    #[test]
    fn a_merged_row_carries_both_sides_of_its_own_exchange() {
        let log = fd_log();
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "a1b2c3d4", &mut lb);
        assert_eq!((r.added(), r.already, r.refused), (2, 0, 0));
        let rec = &lb.records()[0];
        let c = rec.contest.as_deref().expect("a merged row has provenance");
        assert_eq!(c.contest_id, "ARRL-FIELD-DAY");
        // The contest and the running the contact was made in, never where it was made from.
        assert_eq!(c.session, "ARRL-FIELD-DAY:2026");
        assert_eq!(c.qid, "ARRL-FIELD-DAY:2026:a1b2c3d4:1");
        assert_eq!(c.stx_string.as_deref(), Some("3A WI"), "what I sent");
        assert_eq!(c.srx_string.as_deref(), Some("2A EMA"), "what they sent");
        assert_eq!(
            c.sent,
            vec![
                ("CLASS".to_string(), "3A".to_string()),
                ("SECTION".to_string(), "WI".to_string())
            ]
        );
        assert_eq!(
            c.rcvd,
            vec![
                ("CLASS".to_string(), "2A".to_string()),
                ("SECTION".to_string(), "EMA".to_string())
            ]
        );
        assert_eq!(rec.mode, "CW");
        assert_eq!(rec.station_callsign.as_deref(), Some("W9XYZ"));
    }

    /// ⛔ **THE LEG THAT GOES OUT IS THE ONE THAT WAS TRANSMITTED** (operator review of
    /// 1.15.0, finding L1 — a regression introduced by the commit that added `FREQ` here).
    ///
    /// Two defects in one line, both of them a confident wrong number in the file that goes
    /// to LoTW, QRZ and Club Log — and the previous behaviour was `0.0`, so this range
    /// turned "absent" into "wrong", which is worse:
    ///
    /// 1. A contact worked **SPLIT** recorded the frequency the operator was LISTENING on
    ///    as ADIF `FREQ`, and emitted no `FREQ_RX` at all, so nothing said it was a split
    ///    contact and the one frequency it did state was the wrong one.
    /// 2. A **digital** contact recorded the bare dial rather than the dial plus the TX
    ///    audio offset, so the same operator's FT8 contest rows and their ordinary FT8 rows
    ///    described one contact two different ways.
    ///
    /// Both legs are stamped at log time from `Engine::log_frequencies` — the general log's
    /// own answer, so the two emitters cannot disagree — and this function copies them.
    #[test]
    fn a_split_contact_logs_the_transmit_leg_and_says_it_was_split() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        // Worked split: listening on 14.025000, transmitting on 14.028500. The dial is the
        // RECEIVE leg, which is exactly why writing it as FREQ was wrong.
        log.dial_khz = 14_025;
        log.on_air_hz = 14_028_500;
        log.on_air_rx_hz = Some(14_025_000);
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));

        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!(r.added(), 1);

        assert_eq!(
            r.written[0].freq_mhz, 14.0285,
            "the leg that was TRANSMITTED"
        );
        let adi = crate::logbook::adif_record(&r.written[0]);
        assert!(adi.contains("<FREQ:9>14.028500"), "{adi}");

        // ⚠️ ITS OWN CONTROL. The two claims below fail for a DIFFERENT reason than the two
        // above — `freq_rx_mhz` was hardcoded `None`, so a run that fixed only `FREQ` would
        // still ship a split contact that never says it was split. Asserted after the FREQ
        // pair deliberately, and verified to red on its own by reverting only that field.
        assert_eq!(
            r.written[0].freq_rx_mhz,
            Some(14.025),
            "the leg that was LISTENED on — without it nothing says this was worked split"
        );
        assert!(adi.contains("<FREQ_RX:9>14.025000"), "{adi}");

        // And the CABRILLO dial is untouched: the sponsor's convention is the VFO readout,
        // so the QSO line still says 14025 even though the transmission was 3.5 kHz up.
        assert_eq!(
            log.qsos()[0].freq_khz,
            14_025,
            "the Cabrillo dial is a different fact and must not follow the on-air value"
        );
    }

    /// The digital half of L1, split out so its control reports its OWN claim: a red on the
    /// split test above says nothing about this one.
    ///
    /// A simplex FT8 contact transmits on the dial PLUS the TX audio offset (minus it on
    /// LSB). Recording the bare dial made a contest row and an ordinary row describe the
    /// same contact two different ways, and a simplex row must still emit no `FREQ_RX` —
    /// `<FREQ_RX>` equal to `<FREQ>` is a claim that the QSO was worked split.
    #[test]
    fn a_digital_contact_logs_the_dial_plus_its_tx_audio_offset() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.dial_khz = 14_074;
        log.on_air_hz = 14_075_500; // 1500 Hz up, USB
        log.on_air_rx_hz = None;
        assert!(log.log_submode_at("W1AW", "1D", "CT", "DIG", "FT8", 0, 1_782_583_560));

        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!(r.added(), 1);
        assert_eq!(
            r.written[0].freq_mhz, 14.0755,
            "dial + the TX audio offset, which is what the general log writes"
        );
        assert_eq!(
            r.written[0].freq_rx_mhz, None,
            "a simplex contact must not claim a receive leg"
        );
        let adi = crate::logbook::adif_record(&r.written[0]);
        assert!(adi.contains("<FREQ:9>14.075500"), "{adi}");
        assert!(!adi.contains("FREQ_RX"), "{adi}");
        assert_eq!(
            log.qsos()[0].freq_khz,
            14_074,
            "Cabrillo still writes the dial"
        );
    }

    /// ⭐ **THE DIAL THE CONTACT WAS WORKED ON REACHES THE LIFETIME LOG** (operator review
    /// of 1.14.0).
    ///
    /// This wrote `freq_mhz: 0.0` under a comment saying a contest row records the band —
    /// which is wrong about the artifact. [`LoggedQso::freq_khz`] is stamped at log time
    /// from the dial (`Engine::sync_fd_band`), the contest log's own Cabrillo writes it in
    /// place of the band edge, and its ADIF writes `FREQ` from it. So a weekend's contacts
    /// reached the file that goes to LoTW, QRZ and ClubLog carrying `BAND` and nothing
    /// else — and on VHF that loses the segment: 144.200 SSB and 146.520 FM are both `2m`.
    ///
    /// The row that genuinely has no dial still writes no `FREQ`: `adif_record` omits a
    /// zero rather than emit the `<FREQ:8>0.000000` that Swisslog and DXKeeper reject the
    /// whole record over.
    #[test]
    fn a_merged_row_carries_the_frequency_it_was_worked_on() {
        // A 2 m FM contact and a 2 m SSB one — the pair the band alone cannot tell apart.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "2m",
        );
        log.dial_khz = 144_200;
        assert!(log.log_submode_at("K1ABC", "2A", "EMA", "PH", "USB", 0, 1_782_583_500));
        log.dial_khz = 146_520;
        assert!(log.log_submode_at("W1AW", "1D", "CT", "PH", "FM", 0, 1_782_583_560));
        // …and one row that never knew its dial, which must stay silent rather than zero.
        log.dial_khz = 0;
        assert!(log.log_mode_at("K2DEF", "1D", "MN", "CW", 0, 1_782_583_620));

        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((r.added(), r.already, r.refused), (3, 0, 0));
        let freqs: Vec<f64> = r.written.iter().map(|w| w.freq_mhz).collect();
        assert_eq!(freqs, vec![144.2, 146.52, 0.0], "the dial per row");

        let adi: Vec<String> = r.written.iter().map(crate::logbook::adif_record).collect();
        assert!(adi[0].contains("<FREQ:10>144.200000"), "{}", adi[0]);
        assert!(adi[1].contains("<FREQ:10>146.520000"), "{}", adi[1]);
        assert!(
            !adi[2].contains("<FREQ:"),
            "a row with no dial must not export a zero frequency: {}",
            adi[2]
        );
        // BAND still rides beside it — FREQ is an addition, not a replacement.
        for a in &adi {
            assert!(a.contains("<BAND:2>2m"), "{a}");
        }

        // ⭐ **AND FIELD DAY'S OWN ADIF NOW CARRIES IT TOO** — this assertion was the
        // opposite until 2026-09-20, and it was inverted by an operator ruling, not to make
        // anything go green.
        //
        // What it used to say was that the Field Day export "must not grow a column"
        // because journal and export "are pinned byte for byte by the §8(a) goldens".
        // ⚠️ That was measured and is FALSE: every row in the golden fixtures carries
        // `dial_khz == 0`, so all ten pass either way and they never pinned this at all.
        // And the exception had a real cost, because that one function is BOTH artifacts:
        // `merge_adif` restores `freq_khz` by reading `FREQ` back, so withholding it here
        // silently zeroed the dial on every logged contact whenever Nexus restarted
        // mid-event. See `fieldday::tests::a_field_day_contact_keeps_its_dial_across_a_restart`.
        // ⭐ SIX DECIMALS, matching the general log's own writer. `FREQ` is the frequency
        // RADIATED, so it has to be able to carry a digital mode's TX audio offset; three
        // decimals rounded that away and was the reason a contest row and an ordinary row
        // described one contact two different ways.
        assert!(
            log.adif().contains("<FREQ:10>144.200000"),
            "the Field Day export lost the dial: {}",
            log.adif()
        );
        // And the Cabrillo dial rides in its own tag now that FREQ no longer carries it —
        // without this a restart would rebuild the QSO line from the on-air value.
        assert!(
            log.adif().contains("<APP_NEXUS_DIALKHZ:6>144200"),
            "the Cabrillo dial is not journalled: {}",
            log.adif()
        );
    }

    /// …and the same for a contest that is NOT Field Day, which is the half the exception
    /// above could otherwise be read as covering. A QSO party's own ADIF writes `FREQ`
    /// today; the merged record must agree with it rather than drop to the band.
    #[test]
    fn a_non_field_day_contest_row_agrees_with_its_own_export_about_the_frequency() {
        let spec = crate::contest::exchanges::qso_party_shaped();
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = spec;
        let mut log = FieldDayLog::new("W4TN", session, "20m");
        log.dial_khz = 14_253;
        let v = |key: &'static str, raw: &str| crate::contest::FieldValue {
            key: spec.field(key).expect("declared slot").key,
            raw: raw.to_string(),
            domain: None,
        };
        assert!(log.log_exchange_at(
            "W1AW",
            vec![v("RST", "59"), v("QTH", "CT")],
            vec![v("RST", "59"), v("QTH", "WIL")],
            "PH",
            "USB",
            0,
            1_782_583_500,
        ));
        // The contest log's own export carries the dial…
        assert!(
            log.adif().contains("<FREQ:9>14.253000"),
            "the contest export lost the dial: {}",
            log.adif()
        );
        // …and so does the record the lifetime log keeps, to the same kHz.
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!(r.added(), 1);
        assert_eq!(r.written[0].freq_mhz, 14.253);
        assert!(
            crate::logbook::adif_record(&r.written[0]).contains("<FREQ:9>14.253000"),
            "{}",
            crate::logbook::adif_record(&r.written[0])
        );
    }

    /// ⭐ **A merged PHONE row says which sideband, and an FM one says FM.**
    ///
    /// The mode a merged record carries is [`LoggedQso::recorded_mode`], and it reaches
    /// the file through the general log's own writer — so the pairing is the one
    /// `logbook::adif_submode` has used since 2026-09-15 and the two exports of one
    /// contact cannot disagree.
    ///
    /// ⚠️ **This one was GREEN before the fix, and saying so is the point.** It hands the
    /// row a submode itself, and this layer always carried one through; the defect was
    /// that no row ever GOT one — `Engine::fd_log_manual` passed `submode: None` for
    /// every phone contact. The test that was red is
    /// `engine::tests::a_contest_phone_contact_records_the_mode_that_was_actually_on_the_air`.
    /// What this pins is the seam below it: that the merge keeps reading `recorded_mode`
    /// and the writer keeps pairing it, so a phone row cannot lose its mode BETWEEN the
    /// contest log and the lifetime log.
    #[test]
    fn a_merged_phone_row_carries_the_sideband_the_contact_was_worked_on() {
        for (band, submode, golden) in [
            ("40m", "LSB", "<MODE:3>SSB<SUBMODE:3>LSB"),
            ("20m", "USB", "<MODE:3>SSB<SUBMODE:3>USB"),
            ("2m", "FM", "<MODE:2>FM"),
        ] {
            let mut log = FieldDayLog::new(
                "W9XYZ",
                ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
                band,
            );
            assert!(log.log_submode_at("K1ABC", "2A", "EMA", "PH", submode, 0, 1_782_583_500));
            let mut lb = Logbook::new();
            let r = merge_into_general(&log, "pos", &mut lb);
            assert_eq!(r.added(), 1);
            let out = crate::logbook::adif_record(&r.written[0]);
            assert!(
                out.contains(golden),
                "{submode} must ride as {golden}: {out}"
            );
            assert!(
                !out.contains("<MODE:3>USB") && !out.contains("<MODE:3>LSB"),
                "{submode} put the bare sideband in MODE: {out}"
            );
        }
        // POSITIVE CONTROL: a row with no recorded phone mode still merges as plain SSB
        // with no submode — the legacy answer, unchanged, and what makes the three above
        // a statement about the ROW rather than about a writer that always adds one.
        let log = fd_log(); // K1ABC on CW, W1AW on PH with no submode
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        let out = crate::logbook::adif_record(&r.written[1]);
        assert!(out.contains("<MODE:3>SSB"), "{out}");
        assert!(
            !out.contains("<SUBMODE:"),
            "no sideband was ever observed: {out}"
        );
    }

    /// A contest contact reaches the lifetime log in the form every contact there is held in: a
    /// phone row worked on a sideband is SSB, with the sideband as its SUBMODE.
    #[test]
    fn a_merged_phone_row_is_held_as_ssb_with_its_sideband_as_the_submode() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "40m",
        );
        assert!(log.log_submode_at("K1ABC", "2A", "EMA", "PH", "LSB", 0, 1_782_583_500));
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        let rec = &r.written[0];
        let submode = rec.extra.iter().find(|(k, _)| k == "SUBMODE");
        assert_eq!(
            (rec.band.as_str(), rec.mode.as_str(), submode),
            (
                "40m",
                "SSB",
                Some(&("SUBMODE".to_string(), "LSB".to_string()))
            )
        );
    }

    /// ⭐ §3.2 — merging twice writes nothing the second time.
    /// ⭐ **WHO WAS AT THE KEY, AND THE CALL IT WENT OUT UNDER, SURVIVE THE MERGE**.
    ///
    /// Each contest row is stamped at log time with the operator setting
    /// ([`LoggedQso::operator`]) and the call on the air ([`LoggedQso::on_air_call`]), and the
    /// merge wrote `OPERATOR` as nothing and `STATION_CALLSIGN` as the session's call — so a
    /// club's rotation of operators reached the Cabrillo and never the Logbook, and a contact
    /// merged after the call was changed and Nexus restarted was signed under the new call.
    /// Read back through the file too: the Logbook is what reaches every upload.
    #[test]
    fn a_merged_row_keeps_its_operator_and_the_call_it_was_logged_on() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.operator = "K9OP".into();
        log.on_air_call = "K9OLD".into();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        // Nobody named, and a row from before the call on the air was recorded: both keep
        // what the merge always wrote.
        log.operator = String::new();
        log.on_air_call = String::new();
        assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_800));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        let path = scratch_adi();
        lb.save(&path).expect("write log.adi");
        let reborn = Logbook::load(&path);
        let _ = std::fs::remove_file(&path);
        let by = |call: &str| {
            reborn
                .records()
                .iter()
                .find(|r| r.call == call)
                .cloned()
                .expect("merged")
        };
        let named = by("K1ABC");
        assert_eq!(named.operator.as_deref(), Some("K9OP"));
        assert_eq!(named.station_callsign.as_deref(), Some("K9OLD"));
        let plain = by("W1AW");
        assert_eq!(
            plain.operator.as_deref(),
            Some("W9XYZ"),
            "nobody named: the operator is the station, never \"\""
        );
        assert_eq!(plain.station_callsign.as_deref(), Some("W9XYZ"));
    }

    /// **A merged contact carries the radio and the power it was made with, and an end
    /// time** — the row's own `MY_RIG` and `TX_PWR`, stamped at log time, and `TIME_OFF` = the
    /// moment it was logged, the one moment a contest row records — plus the continent its call
    /// resolved to. Read back through the file, which is what reaches every upload. A row from
    /// before the stamps carries no rig and no power.
    #[test]
    fn a_merged_row_carries_its_radio_power_and_end_time() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.rig = "Icom IC-7300".into();
        log.tx_power = "100".into();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        log.rig = String::new();
        log.tx_power = String::new();
        assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_800));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        let path = scratch_adi();
        lb.save(&path).expect("write log.adi");
        let reborn = Logbook::load(&path);
        let _ = std::fs::remove_file(&path);
        let by = |call: &str| {
            reborn
                .records()
                .iter()
                .find(|r| r.call == call)
                .cloned()
                .expect("merged")
        };
        let kit = by("K1ABC");
        assert_eq!(kit.my_rig.as_deref(), Some("Icom IC-7300"));
        assert_eq!(kit.tx_power, Some(100.0));
        assert_eq!(kit.time_off_unix, Some(1_782_583_500));
        let none = by("W1AW");
        assert_eq!(none.my_rig, None);
        assert_eq!(none.tx_power, None);
        assert_eq!(none.time_off_unix, Some(1_782_583_800));
    }

    /// **A merged contact carries the grid it was made from, marked as Nexus's own** — the
    /// row's grid stamped at log time becomes `MY_GRIDSQUARE`, which every service is sent and
    /// the LoTW upload leaves out ([`crate::logbook::without_self_filled`]), so TQSL signs it
    /// from the Station Location as before. Read back through the file, which is what reaches
    /// every upload. A row with none gets none.
    #[test]
    fn a_merged_row_carries_its_grid_which_lotw_never_sees() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.grid = "EN52".into();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        log.grid = String::new();
        assert!(log.log_mode_at("W1AW", "1D", "CT", "PH", 0, 1_782_583_800));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        let path = scratch_adi();
        lb.save(&path).expect("write log.adi");
        let reborn = Logbook::load(&path);
        let _ = std::fs::remove_file(&path);
        let by = |call: &str| {
            reborn
                .records()
                .iter()
                .find(|r| r.call == call)
                .cloned()
                .expect("merged")
        };
        let placed = by("K1ABC");
        assert_eq!(placed.my_grid.as_deref(), Some("EN52"));
        assert_eq!(crate::logbook::self_filled(&placed), ["MY_GRIDSQUARE"]);
        let sent = crate::logbook::adif_record(&placed);
        assert!(sent.contains("<MY_GRIDSQUARE:4>EN52"), "{sent}");
        assert!(!sent.contains("SELF_FILLED"), "{sent}");
        let lotw = crate::logbook::without_self_filled(&placed);
        assert_eq!(lotw.my_grid, None);
        assert!(
            !crate::logbook::adif_record(&lotw).contains("MY_GRIDSQUARE"),
            "the LoTW copy"
        );
        let none = by("W1AW");
        assert_eq!(none.my_grid, None);
        assert!(crate::logbook::self_filled(&none).is_empty());
    }

    /// A VHF contest's row SENDS its grid, and the exchange's `MY_GRIDSQUARE` is the one the
    /// merged contact keeps — a rover's grid at that moment — written once, never doubled by
    /// the Settings grid stamped on the row.
    #[test]
    fn a_vhf_rows_sent_grid_wins_over_the_stamped_one() {
        let rs = crate::fd_rules::ruleset_by_id("arrlvhf_jan", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = crate::contest::StationData {
            mycall: "W9XYZ".into(),
            mygrid: "EN52".into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        let mut log = FieldDayLog::new("W9XYZ", session, "6m");
        log.session
            .move_to(&[("GRID", "EN53")])
            .expect("a rover moves");
        log.grid = "EN52".into();
        let rx = [("GRID".to_string(), "FN31".to_string())];
        assert!(log.log_fields_at("K1ABC", &rx, "PH", "", 0, 1_782_583_500));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 1);
        let r = &lb.records()[0];
        let out = crate::logbook::adif_record(r);
        assert_eq!(out.matches("<MY_GRIDSQUARE:").count(), 1, "{out}");
        assert!(out.contains("<MY_GRIDSQUARE:4>EN53"), "{out}");
        assert!(crate::logbook::self_filled(r).is_empty());
    }

    /// ⭐ **EVERY FIELD A CONTEST ROW HOLDS REACHES THE LOGBOOK, BY VALUE.** The row is
    /// destructured with no `..`, so a field added to [`LoggedQso`] does not compile here until
    /// this test says where it lands in the Logbook, or why it has no place there. Every value is
    /// distinct from every other and from what the log would stamp by itself, so a field read
    /// from the wrong place fails rather than agreeing by accident.
    ///
    /// ARRL January VHF because its exchange RECEIVES a grid: the one shipped exchange column the
    /// Logbook models in a field of its own, which the merge left in the contest columns only, so
    /// a VHF contest's grids never reached the Logbook's grid column nor anything that reads it.
    #[test]
    fn every_field_a_contest_row_holds_reaches_the_logbook_by_value() {
        let rs = crate::fd_rules::ruleset_by_id("arrlvhf_jan", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        let data = crate::contest::StationData {
            mycall: "W9XYZ".into(),
            mygrid: "EN53".into(),
            ..Default::default()
        };
        let session = ContestSession::for_ruleset(rs, &data).expect("the session builds");
        let mut log = FieldDayLog::new("W9XYZ", session, "2m");
        log.operator = "K9OP".into();
        log.on_air_call = "K9AIR".into();
        log.rig = "IC-9700".into();
        log.tx_power = "50".into();
        log.grid = "EN51".into();
        log.activation = crate::fieldday::RowActivation {
            program: "POTA".into(),
            reference: "US-0001,US-0002".into(),
            state: "ND".into(),
        };
        let rx = [("GRID".to_string(), "FN31".to_string())];
        assert!(log.log_fields_at("K1ABC", &rx, "PH", "USB", 7, 1_782_583_500));
        {
            // What the log path stamps from the radio, the country file and a club's rebuild.
            let q = &mut log.qsos_mut()[0];
            q.entity = Some("United States".into());
            q.continent = Some("NA".into());
            q.freq_khz = 144_200;
            q.on_air_hz = 144_201_500;
            q.freq_rx_hz = Some(144_210_000);
            q.sat = Some(crate::fieldday::SatLeg {
                band: "2m".into(),
                down_hz: 145_960_000,
                bird: "IO-117".into(),
                name: Some("IO-117".into()),
                ..Default::default()
            });
            q.station_call = "K9GOTA".into();
        }
        let row = log.qsos()[0].clone();
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 1);
        let rec = lb.records()[0].clone();
        let c = rec.contest.as_deref().expect("the contest provenance");
        let path = scratch_adi();
        lb.save(&path).expect("write log.adi");
        let reborn = Logbook::load(&path).records()[0].clone();
        let _ = std::fs::remove_file(&path);
        // …and through the logbook database, which (unlike the file) gives the contest columns
        // back as contest columns: the store a restart reads.
        let mut db = crate::logbook::sqlite::LogDb::open_in_memory().unwrap();
        db.insert(&rec, crate::logbook::sqlite::Resolved::default())
            .unwrap();
        let stored = db.load_all().unwrap().remove(0);
        let (out, back, kept) = (
            crate::logbook::adif_record(&rec),
            crate::logbook::adif_record(&reborn),
            crate::logbook::adif_record(&stored),
        );
        // Once, with this value, in the file every upload is made from, before and after a restart.
        let once = |tag: &str, val: &str| {
            let want = format!("<{tag}:{}>{val}", val.len());
            [&out, &back, &kept]
                .iter()
                .all(|f| f.contains(&want) && f.matches(&format!("<{tag}:")).count() == 1)
        };
        let LoggedQso {
            call,
            rx,
            tx,
            role,
            entity,
            continent,
            prefix,
            band,
            freq_khz,
            on_air_hz,
            freq_rx_hz,
            mode,
            submode,
            slot,
            when_unix,
            seq,
            dupe,
            sat,
            operator,
            station_call,
            on_air_call,
            rig,
            tx_power,
            grid,
            activation,
        } = row.clone();
        let checks: Vec<(&str, bool)> = vec![
            ("call", rec.call == call && reborn.call == call),
            (
                "rx",
                c.rcvd == pairs(&rx)
                    && rec.grid.as_deref() == Some("FN31")
                    && reborn.grid.as_deref() == Some("FN31")
                    && stored.grid.as_deref() == Some("FN31")
                    && once("GRIDSQUARE", "FN31"),
            ),
            (
                "tx",
                c.sent == pairs(&tx)
                    && tx.iter().any(|v| v.raw == "EN53")
                    && once("MY_GRIDSQUARE", "EN53"),
            ),
            // Which slots the row sent and received, and so which columns the two above write.
            (
                "role",
                crate::contest::role_for(&row, log.session.exchange)
                    .id
                    .eq(&role)
                    && c.sent.len()
                        == crate::contest::role_for(&row, log.session.exchange)
                            .sends
                            .len(),
            ),
            ("entity", entity.is_some() && rec.country == entity),
            (
                "continent",
                once("CONT", continent.as_deref().unwrap_or("?")),
            ),
            // Derived from the call by the sponsor's rule, for scoring; the call is the fact.
            ("prefix", prefix.is_some()),
            ("band", rec.band == band && once("BAND", &band)),
            // The dial, which the on-air frequency below supersedes; a row with none falls back
            // to it (`a_merged_row_carries_the_frequency_it_was_worked_on`).
            ("freq_khz", freq_khz == 144_200),
            ("on_air_hz", rec.freq_mhz == on_air_hz as f64 / 1e6),
            (
                "freq_rx_hz",
                rec.freq_rx_mhz == freq_rx_hz.map(|hz| hz as f64 / 1e6),
            ),
            ("mode", mode == "PH" && rec.mode == "SSB"),
            ("submode", once("SUBMODE", &submode)),
            // The digital slot the row was decoded in: a property of the decode, not the contact.
            ("slot", slot == 7),
            (
                "when_unix",
                rec.when_unix == when_unix && reborn.when_unix == when_unix,
            ),
            ("seq", c.qid == qid_for(&session_of(&log, &row), "pos", seq)),
            // A fact about the contest LOG, re-derived from its other rows; the contact is real.
            ("dupe", !dupe),
            (
                "sat",
                once(
                    "SAT_NAME",
                    sat.and_then(|s| s.name).as_deref().unwrap_or("?"),
                ) && once("PROP_MODE", "SAT"),
            ),
            ("operator", once("OPERATOR", &operator)),
            ("station_call", once("STATION_CALLSIGN", &station_call)),
            // Shadowed above by the club's call; its own leg is below.
            ("on_air_call", on_air_call == "K9AIR"),
            ("rig", once("MY_RIG", &rig)),
            ("tx_power", rec.tx_power == tx_power.parse().ok()),
            // The exchange's own sent grid wins over this stamp (`a_vhf_rows_sent_grid_wins…`).
            ("grid", grid == "EN51" && !out.contains("EN51")),
            (
                "activation",
                // A two-fer writes as the general log writes one: its first park as
                // `MY_SIG_INFO`, both as `MY_POTA_REF`.
                once("MY_SIG", &activation.program)
                    && once("MY_SIG_INFO", "US-0001")
                    && once("MY_POTA_REF", &activation.reference)
                    && once("MY_STATE", &activation.state)
                    && [&rec, &reborn, &stored]
                        .iter()
                        .all(|r| r.ota.my_ref.as_deref() == Some("US-0001,US-0002")),
            ),
        ];
        let failed: Vec<&str> = checks
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(f, _)| *f)
            .collect();
        assert!(
            failed.is_empty(),
            "lost or changed: {failed:?}\n{out}\n{back}"
        );

        // The call it went out under, for the row a club's rebuild did not re-sign.
        log.qsos_mut()[0].station_call.clear();
        let mut own = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut own).added(), 1);
        assert_eq!(own.records()[0].station_callsign.as_deref(), Some("K9AIR"));

        // A contest row holds no park or summit of THEIRS, nor a comment, note or name of the
        // other operator outside its exchange: the entry strip takes the call and the exchange,
        // nothing else. So the Logbook row has none, rather than having had one taken away. Its
        // own side is the activation it was logged during (`activation` above).
        assert_eq!(
            rec.ota,
            crate::logbook::Ota {
                my_program: Some("POTA".into()),
                my_ref: Some("US-0001,US-0002".into()),
                ..crate::logbook::Ota::default()
            }
        );
        assert_eq!(
            (&rec.name, &rec.qth, &rec.comment, &rec.notes),
            (&None, &None, &None, &None)
        );

        // Twice is once.
        let again = merge_into_general(&log, "pos", &mut lb);
        assert_eq!(
            (again.added(), again.already, lb.records().len()),
            (0, 1, 1)
        );
    }

    /// A row logged during an activation carries the park's state as `MY_STATE`, unless its own
    /// exchange SENT a state — a QSO party worked from outside it — which is the one the merged
    /// contact keeps, written once. A row logged with none carries no park and no state.
    #[test]
    fn a_rows_sent_state_wins_over_the_activations() {
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = crate::contest::exchanges::qso_party_shaped();
        let spec = session.exchange;
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        log.activation = crate::fieldday::RowActivation {
            program: "POTA".into(),
            reference: "US-0001".into(),
            state: "ND".into(),
        };
        assert!(log.log_mode_at("K9AAA", "", "", "CW", 0, 1_792_000_000));
        log.activation = crate::fieldday::RowActivation::default();
        assert!(log.log_mode_at("K9BBB", "", "", "CW", 0, 1_792_000_060));
        // What the first row SENT: its state, under the arm that exports as `MY_STATE`.
        log.qsos_mut()[0].tx = vec![
            crate::contest::FieldValue {
                key: spec.field("RST").expect("declared slot").key,
                raw: "599".into(),
                domain: None,
            },
            crate::contest::FieldValue {
                key: spec.field("QTH").expect("declared slot").key,
                raw: "TN".into(),
                domain: Some("us_ca"),
            },
        ];
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        let of = |call: &str| {
            lb.records()
                .iter()
                .find(|r| r.call == call)
                .cloned()
                .unwrap()
        };
        let (park, home) = (of("K9AAA"), of("K9BBB"));
        let adif = crate::logbook::adif_record(&park);
        assert_eq!(adif.matches("<MY_STATE:").count(), 1, "{adif}");
        assert!(adif.contains("<MY_STATE:2>TN"), "the exchange's: {adif}");
        assert!(
            !park.extra.iter().any(|(k, _)| k == "MY_STATE"),
            "no second, disagreeing state on the record: {:?}",
            park.extra
        );
        assert_eq!(park.ota.my_ref.as_deref(), Some("US-0001"));
        assert_eq!(home.ota, crate::logbook::Ota::default());
        assert!(!home.extra.iter().any(|(k, _)| k == "MY_STATE"));
    }

    /// A name and a QTH the other station SENT, in an exchange that carries them, are the
    /// Logbook's own Name and QTH — not a contest column the Name column never shows — and are
    /// written once.
    #[test]
    fn a_name_and_qth_they_sent_are_the_logbooks_name_and_qth() {
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = crate::contest::exchanges::casual();
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        let rx = [
            ("RST".to_string(), "579".to_string()),
            ("NAME".to_string(), "BOB".to_string()),
            ("QTH".to_string(), "BOSTON".to_string()),
        ];
        assert!(log.log_fields_at("K1ABC", &rx, "CW", "", 0, 1_782_583_500));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 1);
        let rec = &lb.records()[0];
        assert_eq!(
            (rec.name.as_deref(), rec.qth.as_deref()),
            (Some("BOB"), Some("BOSTON"))
        );
        let out = crate::logbook::adif_record(rec);
        assert_eq!(out.matches("<NAME:").count(), 1, "{out}");
        assert_eq!(out.matches("<QTH:").count(), 1, "{out}");
    }

    /// ★ The CQ zone the other station SENT in a CQ WW exchange is the zone the contact carries,
    /// wherever it is read: on the merged record, back from the logbook database, and from the
    /// narrow read the award folds make. It stays a contest column (the database keeps those
    /// apart from the passthrough, `sqlite.rs` says why), so a reader of the passthrough alone
    /// counted the country file's zone instead: a W1 call in California sending zone 3 was
    /// credited zone 5.
    #[test]
    fn the_cq_zone_they_sent_is_the_zone_the_contact_carries() {
        use crate::logbook::record_cq_zone;
        use crate::logbook::sqlite::{LogDb, Narrow, Order, Resolved, Scope};
        let rs = crate::fd_rules::ruleset_by_id("cqww_cw", crate::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
        // CQ WW's exchange on a session this test binary can build: CQ WW itself refuses a build
        // with no country file to score by, and the merge reads only the exchange.
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = rs.exchange;
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        let rx = [
            ("RST".to_string(), "599".to_string()),
            ("ZN".to_string(), "3".to_string()),
        ];
        assert!(log.log_fields_at("W1AW", &rx, "CW", "", 0, 1_782_583_500));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 1);
        let rec = lb.records()[0].clone();
        // The premise: the zone is a contest column and not in the passthrough.
        assert!(rec
            .contest
            .as_deref()
            .is_some_and(|c| c.adif.contains(&("CQZ".to_string(), "3".to_string()))));
        assert_eq!(crate::logbook::extra_field(&rec, "CQZ"), None);

        assert_eq!(record_cq_zone(&rec), Some(3), "the merged record");
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert(&rec, Resolved::default()).unwrap();
        let stored = db.load_all().unwrap().remove(0);
        assert_eq!(record_cq_zone(&stored), Some(3), "back from the database");
        let mut narrow = Vec::new();
        db.each_narrow_with(
            Narrow {
                columns: &["call"],
                uploads: false,
            },
            crate::logbook::AWARD_EXTRAS,
            Scope::All,
            Order::Log,
            &mut |r| {
                narrow.push(record_cq_zone(r));
                std::ops::ControlFlow::Continue(())
            },
        )
        .unwrap();
        assert_eq!(narrow, [Some(3)], "the award folds' narrow read");
        let out = crate::logbook::adif_record(&stored);
        assert_eq!(out.matches("<CQZ:").count(), 1, "{out}");
        assert!(out.contains("<CQZ:1>3"), "{out}");
    }

    /// A contact edited in the contest log before the merge reaches the Logbook as edited.
    #[test]
    fn a_contact_edited_before_the_merge_merges_as_edited() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABD", "2A", "EMA", "CW", 0, 1_782_583_500));
        let edit = crate::fieldday::ContactEdit {
            call: "K1ABC".into(),
            rows: vec![vec![
                ("CLASS".to_string(), "3F".to_string()),
                ("SECTION".to_string(), "CT".to_string()),
            ]],
            band: "40m".into(),
            mode: "USB".into(),
        };
        log.edit_contact("K1ABD", 1_782_583_500, &edit)
            .expect("the edit applies");
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 1);
        let rec = &lb.records()[0];
        let c = rec.contest.as_deref().expect("contest");
        assert_eq!(
            (rec.call.as_str(), rec.band.as_str(), rec.mode.as_str()),
            ("K1ABC", "40m", "SSB")
        );
        assert_eq!(
            c.rcvd,
            [
                ("CLASS".to_string(), "3F".to_string()),
                ("SECTION".to_string(), "CT".to_string())
            ]
        );
        assert!(c
            .adif
            .contains(&("ARRL_SECT".to_string(), "CT".to_string())));
    }

    #[test]
    fn a_second_merge_writes_nothing() {
        let log = fd_log();
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        let again = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((again.added(), again.already), (0, 2));
        assert_eq!(lb.len(), 2, "the general log did not grow");
        // POSITIVE CONTROL: the skip is the QID and not a merge that never writes. A
        // DIFFERENT position id is a different row identity, and those DO land.
        let other = merge_into_general(&log, "other-pos", &mut lb);
        assert_eq!(other.added(), 2);
        assert_eq!(lb.len(), 4);
    }

    /// ⭐ §3.5(3) — the idempotence survives a RESTART, which is the leg that proves
    /// the ADIF reader exists. The in-memory leg above would pass with no reader at all.
    #[test]
    fn the_merge_is_idempotent_across_a_restart() {
        let log = fd_log();
        let path = scratch_adi();
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "pos", &mut lb).added(), 2);
        lb.save(&path).expect("write log.adi");
        // RESTART: drop the Logbook and re-read the file, exactly as launch does.
        drop(lb);
        let mut reborn = Logbook::load(&path);
        assert_eq!(reborn.len(), 2, "the re-read log has both rows");
        let after = merge_into_general(&log, "pos", &mut reborn);
        assert_eq!((after.added(), after.already), (0, 2));
        assert_eq!(reborn.len(), 2, "no duplicate after a restart");

        // POSITIVE CONTROL: with the qid stripped from the file on disk, the SAME
        // merge DOES duplicate — so the green above is the reader working, not a merge
        // that cannot write.
        let text = std::fs::read_to_string(&path).expect("read back");
        let stripped = strip_tag(&text, "APP_NEXUS_QID");
        assert!(
            !stripped.contains("APP_NEXUS_QID"),
            "the control has to actually remove the tag"
        );
        let blind_path = scratch_adi();
        std::fs::write(&blind_path, &stripped).expect("write stripped");
        let mut blind = Logbook::load(&blind_path);
        assert_eq!(blind.len(), 2);
        assert_eq!(merge_into_general(&log, "pos", &mut blind).added(), 2);
        assert_eq!(blind.len(), 4, "no qid, no idempotence — the control trips");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&blind_path);
    }

    /// Remove one `<TAG:len>value` field from ADIF text, value and all.
    fn strip_tag(text: &str, tag: &str) -> String {
        let needle = format!("<{tag}:");
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find(&needle) {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            rest = match after.find('<') {
                Some(e) => &after[e..],
                None => "",
            };
        }
        out.push_str(rest);
        out
    }

    /// A row with no club-sync sequence has no stable identity, so it is refused and
    /// counted rather than merged twice or collapsed onto one.
    ///
    /// The shape has to be built by hand, because every production path stamps a
    /// sequence — including a 1.x journal restore, which backfills one (and backfills
    /// even over a journaled `0`). That is the guard being defensive rather than
    /// load-bearing, and it is worth saying so out loud rather than leaving an
    /// untested branch in a merge that writes to the operator's logbook.
    #[test]
    fn an_unstamped_row_is_refused_and_reported() {
        let mut log = fd_log();
        log.qsos_mut()[0].seq = 0;
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((r.added(), r.already, r.refused), (1, 0, 1));
        assert_eq!(lb.len(), 1, "the stamped row still merged");
        // POSITIVE CONTROL: with the sequence back, the same row merges — so the
        // refusal is the missing identity and not a merge that drops the first row.
        let whole = fd_log();
        let mut lb2 = Logbook::new();
        assert_eq!(merge_into_general(&whole, "pos", &mut lb2).added(), 2);
    }

    /// …and the real journal path never produces one: a 1.x row with no `APP_NEXUS_QSEQ`
    /// backfills a sequence and merges, so a restart onto a pre-sync journal does not
    /// silently lose the morning's contacts to the guard above.
    #[test]
    fn a_1x_journal_row_backfills_its_sequence_and_merges() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.merge_adif(
            "<EOH>\n<CALL:5>K1ABC<CLASS:2>2A<ARRL_SECT:3>EMA<BAND:3>20m<MODE:2>CW\
             <QSO_DATE:8>20260627<TIME_ON:6>180500<EOR>\n",
            0,
        );
        assert_ne!(log.qsos()[0].seq, 0);
        let mut lb = Logbook::new();
        let r = merge_into_general(&log, "pos", &mut lb);
        assert_eq!((r.added(), r.refused), (1, 0));
    }

    /// §18.1 — the destination is a property of the SESSION, and it is OFF.
    #[test]
    fn a_fresh_session_uploads_nowhere() {
        let s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        assert!(!s.upload.enabled, "default OFF, on every new session");
        assert!(s.upload_destinations().is_empty());
        // Turning the switch on with nowhere to send is still nowhere — a switch and a
        // destination are both required.
        let mut on = s.clone();
        on.upload.enabled = true;
        assert!(on.upload_destinations().is_empty());
        // …and a destination with the switch off is nowhere too.
        let mut named = s.clone();
        named.upload.destinations = vec!["wrl".into()];
        assert!(
            named.upload_destinations().is_empty(),
            "a destination alone must not opt anybody in"
        );
        // POSITIVE CONTROL: both halves together DO name a destination, so the three
        // empties above are the policy and not an accessor that always answers empty.
        let mut both = s;
        both.upload.enabled = true;
        both.upload.destinations = vec!["wrl".into()];
        assert_eq!(both.upload_destinations(), ["wrl".to_string()]);
    }

    /// The hint the operator reads beside that switch must say the one thing the
    /// switch does not control (§18.1): ClubLog's catch-up sweep.
    #[test]
    fn the_session_control_names_the_clublog_exception() {
        let hint = super::super::UPLOAD_CLUBLOG_SWEEP_HINT;
        assert!(hint.contains("ClubLog"), "{hint}");
        assert!(hint.to_lowercase().contains("password"), "{hint}");
    }

    /// §3.1's own test: a merged row is visible to the session-scoped sweep under the
    /// contest's rule, and an ordinary contact is not confused with one.
    #[test]
    fn merged_rows_reach_the_session_scoped_sweep() {
        let log = fd_log();
        let mut lb = Logbook::new();
        merge_into_general(&log, "pos", &mut lb);
        let w = lb.worked_keys_since(1_782_583_000, &FD_RULE);
        assert!(w.exact.contains(&vec![
            "K1ABC".to_string(),
            "20M".to_string(),
            "CW".to_string()
        ]));
        assert!(w.worked_this_session.contains("W1AW"));
        // …and the cutoff really bounds it.
        let none = lb.worked_keys_since(1_900_000_000, &FD_RULE);
        assert!(none.exact.is_empty() && none.worked_this_session.is_empty());
    }

    /// ⭐ §10 — **the sent side has a DIRECTED ADIF home**, all the way to the bytes
    /// the general log writes. Before batch 6 a merged Field Day row carried no
    /// standard exchange column at all: only `APP_NEXUS_MYEX`/`EX`, which no other
    /// logger reads.
    #[test]
    fn a_merged_row_writes_the_exchange_under_its_directed_adif_tags() {
        let log = fd_log();
        let mut lb = Logbook::new();
        merge_into_general(&log, "pos", &mut lb);
        let out = crate::logbook::adif_record(&lb.records()[0]);
        assert!(
            out.contains("<MY_ARRL_SECT:2>WI"),
            "the section I SENT is mine: {out}"
        );
        assert!(
            out.contains("<ARRL_SECT:3>EMA"),
            "the section they sent is theirs: {out}"
        );
        assert!(out.contains("<CLASS:2>2A"), "their class: {out}");
        // ⭐ THE FALLBACK, on a shipped exchange. `MY_CLASS` is not an ADIF field
        // (checked 2026-09-09), so the class I sent gets no column — and its value is
        // still recoverable, on the private carrier.
        assert!(
            !out.contains("MY_CLASS"),
            "an uncorroborated tag must not be invented: {out}"
        );
        assert!(
            !out.contains("<CLASS:2>3A"),
            "…and it must not ride the CONTACTED station's column either: {out}"
        );
        assert!(
            out.contains("<APP_NEXUS_MYEX:21>CLASS::3A;SECTION::WI"),
            "the sent class rides the private carrier: {out}"
        );
    }

    /// ⭐ §10's own control — a row where the OTHER station sent a county exports
    /// `CNTY`, and one where I am the mobile exports `MY_CNTY` and no `CNTY` at all.
    /// The two together are what prove the assertion is about DIRECTION rather than
    /// about a tag that happens to be absent everywhere.
    #[test]
    fn a_mobiles_county_and_a_worked_states_state_land_in_different_columns() {
        let mut lb = Logbook::new();
        let rec = merged_party_row(&mut lb, ("WIL", "tn_counties"), ("CT", "us_ca"));
        let out = crate::logbook::adif_record(&rec);
        assert!(out.contains("<MY_CNTY:3>WIL"), "the county I was in: {out}");
        assert!(out.contains("<STATE:2>CT"), "the state they sent: {out}");
        assert!(
            !out.contains("<CNTY:"),
            "the contacted station is in Connecticut and has no county: {out}"
        );
        // POSITIVE CONTROL: worked from inside the state, they DO send a county.
        let mut lb2 = Logbook::new();
        let rec = merged_party_row(&mut lb2, ("WIL", "tn_counties"), ("DAV", "tn_counties"));
        let out = crate::logbook::adif_record(&rec);
        assert!(out.contains("<CNTY:3>DAV"), "{out}");
        assert!(out.contains("<MY_CNTY:3>WIL"), "{out}");
    }

    /// ⭐ §2.1.1 ruling 3 — **two writers, one tag, and the EXCHANGE WINS.**
    /// `QsoRecord::state` is the DXCC/callbook resolver's guess; a received `QTH`
    /// matched from `us_ca` is what the other operator told me on the air. Exactly one
    /// `<STATE>` reaches the file, and it is theirs.
    #[test]
    fn a_resolved_state_and_a_received_one_emit_exactly_one_state_field() {
        let mut lb = Logbook::new();
        let mut rec = merged_party_row(&mut lb, ("WIL", "tn_counties"), ("CT", "us_ca"));
        // The resolver ran and guessed something else entirely.
        rec.state = Some("NY".to_string());
        let out = crate::logbook::adif_record(&rec);
        assert_eq!(
            out.matches("<STATE:").count(),
            1,
            "a duplicate hands TQSL undefined territory: {out}"
        );
        assert!(out.contains("<STATE:2>CT"), "the exchange wins: {out}");
        assert!(
            !out.contains("NY"),
            "the resolver's guess is dropped: {out}"
        );

        // POSITIVE CONTROL: with no contest-sourced STATE, the resolver's value IS
        // written — so the guard is the collision and not a writer that lost `state`.
        let mut plain = rec.clone();
        plain.contest = None;
        let out = crate::logbook::adif_record(&plain);
        assert_eq!(out.matches("<STATE:").count(), 1, "{out}");
        assert!(out.contains("<STATE:2>NY"), "{out}");
    }

    /// One merged row of the QSO-party shape: `(raw, domain)` for what I sent and for
    /// what they sent. Returns the record, which is also in `lb`.
    fn merged_party_row(
        lb: &mut Logbook,
        sent: (&str, &'static str),
        rcvd: (&str, &'static str),
    ) -> QsoRecord {
        let spec = crate::contest::exchanges::qso_party_shaped();
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = spec;
        let mut log = FieldDayLog::new("W4TN", session, "20m");
        let v = |key: &'static str, raw: &str, domain: Option<&'static str>| {
            crate::contest::FieldValue {
                key: spec.field(key).expect("declared slot").key,
                raw: raw.to_string(),
                domain,
            }
        };
        assert!(log.log_exchange_at(
            "W1AW",
            vec![v("RST", "599", None), v("QTH", rcvd.0, Some(rcvd.1))],
            vec![v("RST", "599", None), v("QTH", sent.0, Some(sent.1))],
            "CW",
            "",
            0,
            1_782_583_500,
        ));
        let r = merge_into_general(&log, "pos", lb);
        assert_eq!(r.added(), 1);
        r.written[0].clone()
    }
}
