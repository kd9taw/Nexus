//! ⭐ **Removing the newest contest contact, and putting it back — never a delete.**
//!
//! The contest log keeps a removed contact as removed
//! ([`FieldDayLog::remove_last`](tempo_core::fieldday::FieldDayLog::remove_last)) and its
//! journal keeps it marked, so the serial and the club seq it was given stay spent and the
//! contest screen can restore it exactly. **Nothing here keys, unkeys or stops a
//! transmission.**
//!
//! ⛔ **Refused while club sync runs**, at a position or at the host. The club log cannot take a
//! contact back: its rows, dupe keys and sections are append-only, and each position's copy is
//! fed everything past its cursor, so removing it here would leave the club's Cabrillo and every
//! position's dupe check holding a contact this station's own files no longer carry.
//!
//! **What a removal cannot reach, it names.** The contact may already have gone to N3FJP (whose
//! API has no delete), the N1MM broadcast, WSJT-X listeners, and — if it was merged — the
//! logbook and the services the logbook uploaded it to. Every answer says which, and nothing is
//! changed there.

use std::collections::HashSet;
use std::sync::Mutex;

use super::{engine_lock, now_unix_secs, Engine, Mode};
use crate::dto::FieldDayQso;
use tempo_core::fieldday::{RemoveRefusal, RemovedQso};
use tempo_core::logbook::{QsoRecord, UploadOutcome, UploadState};

/// Why a removal or a restore changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestRefusal {
    /// The contest log holds no contact.
    Empty,
    /// The newest contact is not the one named — one was logged, removed or restored since it
    /// was shown.
    Changed,
    /// No removed contact has that id.
    NotRemoved,
    /// The same station was worked again since, on that band and mode.
    WorkedAgain,
    /// Club sync runs, and the club log cannot take a contact back.
    ClubSync,
}

impl ContestRefusal {
    /// The refusal as the screen's catalog keys name it.
    pub fn code(self) -> &'static str {
        match self {
            ContestRefusal::Empty => "empty",
            ContestRefusal::Changed => "changed",
            ContestRefusal::NotRemoved => "notRemoved",
            ContestRefusal::WorkedAgain => "workedAgain",
            ContestRefusal::ClubSync => "clubSync",
        }
    }
}

impl From<RemoveRefusal> for ContestRefusal {
    fn from(r: RemoveRefusal) -> Self {
        match r {
            RemoveRefusal::Empty => ContestRefusal::Empty,
            RemoveRefusal::Changed => ContestRefusal::Changed,
            RemoveRefusal::NotRemoved => ContestRefusal::NotRemoved,
            RemoveRefusal::WorkedAgain => ContestRefusal::WorkedAgain,
        }
    }
}

/// Where a contact had already gone when it was removed: the places a removal cannot reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContestSentTo {
    /// The N3FJP host it was pushed to.
    pub n3fjp: Option<String>,
    /// The address of the N1MM broadcast it went out on.
    pub n1mm: Option<String>,
    /// It went to WSJT-X listeners as a "QSO Logged" datagram.
    pub wsjtx: bool,
}

/// One removed contact, as the engine answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestRemoval {
    /// The entry the contest log keeps — the rows exactly as they stood.
    pub entry: RemovedQso,
    /// Its rows as the contest log table shows them.
    pub rows: Vec<FieldDayQso>,
    /// Where it had already gone (a removal only; empty for a restore and for the list).
    pub sent: ContestSentTo,
    /// Each row's merge identity — what the logbook names its copy by, if it was merged.
    pub qids: Vec<String>,
}

/// A removal or restore, as a command answers it: the engine's answer, plus — for a removal —
/// the upload marks on the logbook's copies of the contact, read with the Engine lock released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestRemoved {
    pub removal: ContestRemoval,
    /// `Some` when the logbook holds a merged copy: the services that have taken it (or were
    /// sent it), by the copy's own upload marks. `None` when it was never merged.
    pub logbook: Option<Vec<&'static str>>,
}

impl Engine {
    /// ⭐ **Remove the newest contest contact** — the strip's Ctrl+D pressed twice, its Remove
    /// last button, and the contest screen's Remove on the newest row — when it is still the
    /// contact the strip named (`call`, `when_unix`). A county line goes with all its rows.
    ///
    /// Kept, never deleted ([`FieldDayLog::remove_last`](tempo_core::fieldday::FieldDayLog::remove_last)):
    /// the journal keeps it marked, its serial and seq stay spent, and
    /// [`Self::contest_restore`] puts it back. `Err` only when no contest runs.
    pub fn contest_remove_last(
        &mut self,
        call: &str,
        when_unix: u64,
    ) -> Result<Result<ContestRemoval, ContestRefusal>, String> {
        let club_sync = self.fd_sync_enabled();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        if club_sync {
            return Ok(Err(ContestRefusal::ClubSync));
        }
        let entry = match station.log.remove_last(call, when_unix, now_unix_secs()) {
            Ok(entry) => entry,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log(); // journal it — the removal must survive a crash too
        let mut removal = self.contest_removal_of(entry);
        removal.sent = self.contest_sent_to(&removal.entry);
        Ok(Ok(removal))
    }

    /// ⭐ **Put a removed contact back exactly as it was** — same time, serial and seq — by its
    /// id ([`RemovedQso::id`]). Refused while club sync runs, as a removal is, and when the
    /// same station was worked again since on that band and mode. `Err` only when no contest
    /// runs.
    pub fn contest_restore(
        &mut self,
        id: u64,
    ) -> Result<Result<ContestRemoval, ContestRefusal>, String> {
        let club_sync = self.fd_sync_enabled();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        if club_sync {
            return Ok(Err(ContestRefusal::ClubSync));
        }
        let entry = match station.log.restore(id) {
            Ok(entry) => entry,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log();
        Ok(Ok(self.contest_removal_of(entry)))
    }

    /// The removed contacts, oldest removal first — the contest screen's Removed list. Empty
    /// outside a contest.
    pub fn contest_removed(&self) -> Vec<ContestRemoval> {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return Vec::new();
        };
        station
            .log
            .removed()
            .iter()
            .map(|entry| self.contest_removal_of(entry.clone()))
            .collect()
    }

    /// The highest seq this contest log has given, its removed contacts' included — where the
    /// slot loop's forwarder starts when a session (re)starts, so nothing restored from the
    /// journal is pushed again, however it is later removed and restored. 0 outside a contest.
    pub fn contest_seq_high_water(&self) -> u64 {
        match &self.mode {
            Mode::FieldDay { station, .. } => station.log.max_seq(),
            _ => 0,
        }
    }

    /// The forwarder's cursor, each slot boundary: every live contest row with a seq at or
    /// below it has been handed to N3FJP, the N1MM broadcast and WSJT-X listeners, as
    /// configured then.
    pub fn note_fd_forwarded(&mut self, seq: u64) {
        self.fd_forwarded_seq = seq;
    }

    /// ⭐ **The contest half of a Logbook delete** — the twin of
    /// [`Self::correct_contest_row`]: a contact merged from the RUNNING contest leaves the
    /// contest log too, as a removal (kept, restorable), so the Cabrillo no longer carries a
    /// contact the operator deleted.
    ///
    /// ⛔ Only a row this session and this position minted: the deleted record's `qid` must be
    /// exactly the one the merge gave the live row ([`tempo_core::contest::qid_for`]), so last
    /// weekend's contact, or another position's, never reaches this log's row with the same
    /// seq. While club sync runs the row stays, as a removal is refused — the logbook delete
    /// itself is the operator's and stands.
    pub(crate) fn contest_row_deleted(&mut self, row: &QsoRecord) {
        let Some(qid) = row.contest.as_deref().map(|c| c.qid.as_str()) else {
            return;
        };
        let club_sync = self.fd_sync_enabled();
        let posid = self.settings.fd_position_id.clone();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return;
        };
        let Some(seq) = tempo_core::contest::seq_from_qid(qid, &station.log.session.id) else {
            return;
        };
        if qid != tempo_core::contest::qid_for(&station.log.session.id, &posid, seq)
            || !station.log.qsos().iter().any(|q| q.seq == seq)
        {
            return;
        }
        if club_sync {
            tempo_core::applog::warn(
                "contest",
                &format!(
                    "{} was deleted from the logbook and stays in the contest log: club sync \
                     is on, and the club log cannot take a contact back",
                    row.call
                ),
            );
            return;
        }
        if station.log.remove_row(seq, now_unix_secs()).is_some() {
            self.persist_fd_log();
            tempo_core::applog::info(
                "contest",
                &format!(
                    "{} was deleted from the logbook, so it left the contest log too \
                     (restore it on the contest screen)",
                    row.call
                ),
            );
        }
    }

    /// An entry with its rows as the log table shows them and their merge identities.
    fn contest_removal_of(&self, entry: RemovedQso) -> ContestRemoval {
        let Mode::FieldDay { station, .. } = &self.mode else {
            return ContestRemoval {
                entry,
                rows: Vec::new(),
                sent: ContestSentTo::default(),
                qids: Vec::new(),
            };
        };
        let log = &station.log;
        let (rs, spec, role) = (log.ruleset(), log.session.exchange, log.session.role());
        ContestRemoval {
            rows: entry
                .rows
                .iter()
                .map(|q| super::field_day_display::field_day_qso(q, rs, spec, role))
                .collect(),
            qids: entry
                .rows
                .iter()
                .map(|q| {
                    tempo_core::contest::qid_for(
                        &log.session.id,
                        &self.settings.fd_position_id,
                        q.seq,
                    )
                })
                .collect(),
            sent: ContestSentTo::default(),
            entry,
        }
    }

    /// Where `entry` had gone before it was removed: each forwarder destination configured
    /// now, when a slot boundary had already passed it. A destination configured after it went
    /// out, or removed before, is answered as configured now — the forwarder keeps no
    /// per-contact record of which destinations it reached.
    fn contest_sent_to(&self, entry: &RemovedQso) -> ContestSentTo {
        let forwarded = entry
            .rows
            .iter()
            .any(|q| q.seq > 0 && q.seq <= self.fd_forwarded_seq);
        if !forwarded {
            return ContestSentTo::default();
        }
        let s = &self.settings;
        let set = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        ContestSentTo {
            n3fjp: set(&s.n3fjp_host),
            n1mm: set(&s.n1mm_addr),
            wsjtx: s.wsjtx_udp,
        }
    }
}

/// The services holding a merged copy, by its upload marks: one that was sent it (pending) or
/// took it (accepted, or already had it) — never one that refused it.
fn holders(marks: &[UploadState]) -> Vec<&'static str> {
    let has = |m: &Option<tempo_core::logbook::UploadStatus>| {
        matches!(
            m.as_ref().map(|s| s.outcome),
            Some(UploadOutcome::Pending | UploadOutcome::Accepted | UploadOutcome::Duplicate)
        )
    };
    let mut out = Vec::new();
    for (name, held) in [
        ("lotw", marks.iter().any(|m| has(&m.lotw))),
        ("qrz", marks.iter().any(|m| has(&m.qrz))),
        ("clublog", marks.iter().any(|m| has(&m.clublog))),
        ("eqsl", marks.iter().any(|m| has(&m.eqsl))),
    ] {
        if held {
            out.push(name);
        }
    }
    out
}

/// What a command makes of an engine answer: for a removal with merge identities, the logbook's
/// copies read with the lock released (the store is never read under it).
fn with_logbook(
    engine: &Mutex<Engine>,
    answer: Result<ContestRemoval, ContestRefusal>,
) -> Result<Result<ContestRemoved, ContestRefusal>, String> {
    let removal = match answer {
        Ok(removal) => removal,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let qids: HashSet<String> = removal.qids.iter().cloned().collect();
    let marks = crate::engine::log_plan(engine).merged_uploads(&qids)?;
    let logbook = (!marks.is_empty()).then(|| holders(&marks));
    Ok(Ok(ContestRemoved { removal, logbook }))
}

/// ★ [`Engine::contest_remove_last`] as a command makes it: the removal under the lock, its
/// journal on disk before the answer (a removal is a change to the contact's only copy on disk
/// until it is merged), and the logbook's copies read after.
///
/// ⚠️ It waits and reads the store: never call it holding the Engine lock.
pub fn remove_last(
    engine: &Mutex<Engine>,
    call: &str,
    when_unix: u64,
) -> Result<Result<ContestRemoved, ContestRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_remove_last(call, when_unix)?;
        (answer, e.journal_mark())
    };
    written.wait();
    with_logbook(engine, answer)
}

/// ★ [`Engine::contest_restore`] as a command makes it: the journal on disk before the answer.
///
/// ⚠️ It waits: never call it holding the Engine lock.
pub fn restore(
    engine: &Mutex<Engine>,
    id: u64,
) -> Result<Result<ContestRemoval, ContestRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_restore(id)?;
        (answer, e.journal_mark())
    };
    written.wait();
    Ok(answer)
}
