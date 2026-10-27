//! ⭐ **Editing a contest contact where it stands — the contest screen's Edit and the strip's
//! Ctrl+Y.**
//!
//! A busted call, a mistyped exchange, a contact logged on the wrong band or in the wrong mode,
//! fixed in place ([`FieldDayLog::edit_contact`](tempo_core::fieldday::FieldDayLog::edit_contact)):
//! the contact keeps its time, its serial and its seq, so an edit never spends a number, and the
//! journal holds the edit before the answer, so a restart keeps it. The score, every row's dupe
//! mark and the multipliers follow from the rows, the Cabrillo and the ADIF are written from them,
//! and a merge into the logbook made after the edit writes the contact as it now is. **Nothing
//! here keys, unkeys or stops a transmission.**
//!
//! ⛔ **Never at a club.** Club sync v4 names a row by its position and seq, and the host keeps
//! the row it merged first: the edited row sent again changes nothing there, nor does a take-back
//! and restore around it, so the club's file would keep the busted call with every chip saying
//! Synced. While club sync runs an edit is refused ([`ContestEditRefusal::Club`]), and the screen
//! says how a contact is fixed at a club today: taken back and logged again. Carrying an edit to
//! the club needs a line v4 does not have.
//!
//! **What an edit cannot reach, it names**, as a removal does: N3FJP, the N1MM broadcast and
//! WSJT-X listeners the contact had already gone to, and a copy merged into the logbook before
//! the edit, which keeps what it said until it is edited there.

use std::sync::Mutex;

use super::contest_removal::{holders, ContestSentTo};
use super::{engine_lock, Engine, Mode};
use crate::dto::FieldDayQso;
use tempo_core::fieldday::{ContactEdit, EditRefusal, FieldDayLog, LoggedQso};

/// Why an edit changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestEditRefusal {
    /// The contact no longer reads as the edit opened it: it was edited since it was shown (its
    /// call too, so no contact has that call and time now).
    Changed,
    /// No contact in the log has that time any more: it was removed since it was shown. Every
    /// edit keeps a contact's time, so one whose call was changed still has it and is `Changed`.
    Gone,
    /// The edit names no call, or a band no contest file can carry.
    Incomplete,
    /// The edit gives another number of rows than the contact has (a county line's counties).
    Rows,
    /// A contact worked through a satellite keeps the band of its pass.
    Satellite,
    /// Club sync runs, and an edit cannot reach the club.
    Club,
}

impl ContestEditRefusal {
    /// The refusal as the screen's catalog keys name it.
    pub fn code(self) -> &'static str {
        match self {
            ContestEditRefusal::Changed => "changed",
            ContestEditRefusal::Gone => "gone",
            ContestEditRefusal::Incomplete => "incomplete",
            ContestEditRefusal::Rows => "rows",
            ContestEditRefusal::Satellite => "satellite",
            ContestEditRefusal::Club => "club",
        }
    }
}

impl From<EditRefusal> for ContestEditRefusal {
    fn from(r: EditRefusal) -> Self {
        match r {
            EditRefusal::Changed => ContestEditRefusal::Changed,
            EditRefusal::Incomplete => ContestEditRefusal::Incomplete,
            EditRefusal::Rows => ContestEditRefusal::Rows,
            EditRefusal::Satellite => ContestEditRefusal::Satellite,
        }
    }
}

/// One edited contact, as the engine answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestEdit {
    /// Its rows as the log table showed them before the edit.
    pub before: Vec<FieldDayQso>,
    /// …and as it shows them now.
    pub after: Vec<FieldDayQso>,
    /// Where it had already gone, which the edit does not reach.
    pub sent: ContestSentTo,
    /// Each row's merge identity — what the logbook names its copy by, if it was merged.
    pub qids: Vec<tempo_core::contest::MergeIdentity>,
}

/// An edit as a command answers it: the engine's answer, plus the upload marks of a copy merged
/// into the logbook before the edit, read with the Engine lock released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestEdited {
    pub edit: ContestEdit,
    /// `Some` when the logbook holds a merged copy, which keeps what it said: the services that
    /// have taken it (or were sent it), by the copy's own upload marks. `None` when it was never
    /// merged, or when the logbook could not be read (the app log says so).
    pub logbook: Option<Vec<&'static str>>,
}

impl Engine {
    /// ⭐ **Edit a contest contact in place** — the one the screen named (`call`, `when_unix`,
    /// what its log table and the strip show), a county line as one contact. Refused at a club
    /// (see the module). `Err` only when no contest runs.
    ///
    /// ⛔ **Only while it is still the contact the edit opened.** `opened` is its rows as the
    /// form was filled from them. Another window can edit the same contact meanwhile, and a form
    /// filled before that would write its old values back over the change, so a contact whose
    /// call, band, mode or received exchange no longer reads as `opened` is refused as `Changed`,
    /// and nothing changes — or as `Gone` when no contact has its time any more.
    pub fn contest_edit(
        &mut self,
        call: &str,
        when_unix: u64,
        opened: &[FieldDayQso],
        edit: &ContactEdit,
    ) -> Result<Result<ContestEdit, ContestEditRefusal>, String> {
        let club = self.fd_sync_enabled();
        let Mode::FieldDay { station, .. } = &mut self.mode else {
            return Err("Contest mode is not active".into());
        };
        if club {
            return Ok(Err(ContestEditRefusal::Club));
        }
        if when_unix != 0 && !station.log.qsos().iter().any(|q| q.when_unix == when_unix) {
            return Ok(Err(ContestEditRefusal::Gone));
        }
        if !reads_as(&station.log, call, when_unix, opened) {
            return Ok(Err(ContestEditRefusal::Changed));
        }
        let before = match station.log.edit_contact(call, when_unix, edit) {
            Ok(rows) => rows,
            Err(refusal) => return Ok(Err(refusal.into())),
        };
        self.persist_fd_log(); // journal it — the edit must survive a crash too
        Ok(Ok(self.contest_edit_of(&before)))
    }

    /// The answer for a contact whose rows were `before`: both sides as the log table shows them,
    /// where it had gone, and the merge identities of its rows.
    fn contest_edit_of(&self, before: &[LoggedQso]) -> ContestEdit {
        let sent = self.contest_sent_to(before);
        let Mode::FieldDay { station, .. } = &self.mode else {
            return ContestEdit {
                before: Vec::new(),
                after: Vec::new(),
                sent,
                qids: Vec::new(),
            };
        };
        let log = &station.log;
        let shown = shown_of(log);
        let after: Vec<&LoggedQso> = log
            .qsos()
            .iter()
            .filter(|q| before.iter().any(|b| b.seq == q.seq))
            .collect();
        ContestEdit {
            before: before.iter().map(&shown).collect(),
            after: after.iter().map(|q| shown(q)).collect(),
            sent,
            qids: after
                .iter()
                .map(|q| {
                    tempo_core::contest::MergeIdentity::of(log, &self.settings.fd_position_id, q)
                })
                .collect(),
        }
    }
}

/// A row of the log as the log table shows it.
fn shown_of(log: &FieldDayLog) -> impl Fn(&LoggedQso) -> FieldDayQso + '_ {
    let (rs, spec, role) = (log.ruleset(), log.session.exchange, log.session.role());
    let numbers = log.qso_numbers();
    move |q| {
        let nr = numbers.get(&q.seq).copied();
        super::field_day_display::field_day_qso(q, rs, spec, role, nr)
    }
}

/// Does the contact `call` at `when_unix` still read as `opened`, the rows its edit was filled
/// from? Only what an edit can change is compared (`edited`).
fn reads_as(log: &FieldDayLog, call: &str, when_unix: u64, opened: &[FieldDayQso]) -> bool {
    let shown = shown_of(log);
    let now: Vec<FieldDayQso> = log
        .contact_at(call, when_unix)
        .iter()
        .map(|&i| shown(&log.qsos()[i]))
        .collect();
    now.iter().map(edited).eq(opened.iter().map(edited))
}

/// What an edit can change on a row, as the log table shows it: the call, the band, the mode
/// and what was received. The rest of a shown row moves with other contacts (its dupe mark) or
/// is kept by every edit (its time, number and sent exchange).
fn edited(q: &FieldDayQso) -> (&str, &str, &str, &str, &str, &str, &[String]) {
    (
        &q.call, &q.band, &q.mode, &q.submode, &q.class, &q.section, &q.rcvd,
    )
}

/// ★ [`Engine::contest_edit`] as a command makes it: the edit under the lock, its journal on
/// disk before the answer (the journal is the contact's only copy on disk until it is merged),
/// and a merged copy's upload marks read after — or, when the logbook cannot be read, the copy
/// left unnamed: the edit is made either way, and the answer must say so.
///
/// ⚠️ It waits and reads the store: never call it holding the Engine lock.
pub fn edit(
    engine: &Mutex<Engine>,
    call: &str,
    when_unix: u64,
    opened: &[FieldDayQso],
    edit: &ContactEdit,
) -> Result<Result<ContestEdited, ContestEditRefusal>, String> {
    let (answer, written) = {
        let mut e = engine_lock(engine);
        let answer = e.contest_edit(call, when_unix, opened, edit)?;
        (answer, e.journal_mark())
    };
    written.wait();
    let edit = match answer {
        Ok(edit) => edit,
        Err(refusal) => return Ok(Err(refusal)),
    };
    // The edit is made and on disk: a logbook that cannot be read now must not turn it into an
    // answer that says nothing was edited. The copy goes unnamed, and the log says why.
    let logbook = match crate::engine::log_plan(engine).merged_uploads(&edit.qids) {
        Ok(marks) => (!marks.is_empty()).then(|| holders(&marks)),
        Err(e) => {
            tempo_core::applog::error(
                "contest",
                &format!(
                    "a contest contact was edited, and the logbook could not be read to say \
                     whether it holds a copy merged before the edit: {e}"
                ),
            );
            None
        }
    };
    Ok(Ok(ContestEdited { edit, logbook }))
}
