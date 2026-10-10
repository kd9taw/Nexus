//! ⭐ **ONE CONTACT IN PROGRESS, IN TWO WINDOWS** — the contest entry strip shared by the main
//! window and the contest logger window, so a second person can log at a second monitor and
//! keyboard while the operator works the radio.
//!
//! While the logger window is open, every contest strip on screen shows the entry held here:
//! what is typed in either window is put here and shown in the other at its next snapshot. The
//! shell says when the window opens and closes ([`Engine::contest_entry_share`], from
//! `open_panel_window` and the window's `Destroyed` event), so a crash or a restart cannot leave
//! it shared. With the window closed there is no shared entry and every strip is its own,
//! exactly as before.
//!
//! **What the engine keeps, and what it reads.** The call and the received boxes, which it
//! compares when a contact is logged; and the strip's marks (the call-history fill, the
//! take-back line, the mode class the operator's strip logs under), which it keeps for the
//! windows and never reads. Every change bumps [`ContestEntryDto::rev`], so a window can tell
//! an entry newer than the one it shows from its own.
//!
//! ⭐ **ENTER LOGS IT ONCE.** A window logging the shared contact names the entry it saw
//! ([`EntryClaim`]), and [`Engine::contest_log_entry`] checks it and runs the log in the same
//! hold of the engine lock. Two Enters pressed at the same moment in two windows log one
//! contact: the second finds the entry already logged ([`ENTRY_LOGGED`]) or changed since it
//! was shown ([`ENTRY_CHANGED`]), and logs nothing. With the window closed a claim checks
//! nothing, so closing it can never stop the main window logging.
//!
//! **Nothing here keys, unkeys or stops a transmission.** It holds text, and the logging it
//! guards is the strip's own.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Engine;

/// The refusal a claim gets when the contact it names was logged since it was shown — from the
/// other window, as a rule. The strip says so and logs nothing.
pub const ENTRY_LOGGED: &str = "contestEntryLogged";
/// The refusal a claim gets when the entry changed since the window that claims it last showed
/// it: the other window typed over it. The strip says so and logs nothing.
pub const ENTRY_CHANGED: &str = "contestEntryChanged";
/// A put while no logger window is open: there is no shared entry to put to.
pub const ENTRY_CLOSED: &str = "contestEntryClosed";
/// A put larger than any strip writes.
pub const ENTRY_TOO_LARGE: &str = "contestEntryTooLarge";

/// The most a call may hold. The longest real ones (`VE3ABC/W9`, `K1ABC/VP2V/P`) are a third of it.
const CALL_MAX: usize = 32;
/// The most boxes an entry may hold: the rules validator refuses a role receiving more than five.
const FIELDS_MAX: usize = 16;
/// The most a slot id may hold (`SECTION` is the longest today).
const KEY_MAX: usize = 16;
/// The most a box may hold: a county line of five counties is about thirty.
const VALUE_MAX: usize = 128;
/// The most the marks may hold, serialised: a few hundred bytes in practice.
const MARKS_MAX: usize = 8 * 1024;

/// The shared entry, as the snapshot carries it (`AppSnapshot::contest_entry`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestEntryDto {
    /// Bumped by every change — a put, a log. A window shows the entry as of this rev.
    pub rev: u64,
    /// The Call box, as typed.
    pub call: String,
    /// The received boxes, by slot id, as typed. A box nobody has touched is absent, which is
    /// how a window tells it from one that was emptied.
    pub fields: BTreeMap<String, String>,
    /// The strip's marks, kept for the windows and never read here: which boxes hold a
    /// call-history fill, the take-back line, and the mode class the operator's strip logs
    /// under. `null` until a window puts some.
    #[serde(default)]
    pub marks: serde_json::Value,
}

/// What a window logging the shared contact saw: the rev of the entry it last agreed with, and
/// the call and boxes it is logging.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryClaim {
    pub rev: u64,
    pub call: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

/// The call and boxes as a log compares them: trimmed, in capitals.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    call: String,
    fields: BTreeMap<String, String>,
}

impl Shown {
    fn of(call: &str, fields: &BTreeMap<String, String>) -> Shown {
        Shown {
            call: call.trim().to_uppercase(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.clone(), v.trim().to_uppercase()))
                .collect(),
        }
    }
}

/// The entry while the logger window is open.
#[derive(Debug, Default)]
pub(super) struct SharedEntry {
    entry: ContestEntryDto,
    /// The last contact logged from the entry, and the rev that log left it at. A claim older
    /// than that rev was made by a window that had not yet seen the log.
    logged: Option<(Shown, u64)>,
}

impl SharedEntry {
    /// Does this claim name the entry as it stands? Either nothing changed it since the window
    /// last agreed with it, or what changed it left the call and the boxes as the window shows
    /// them (the other window's marks, or the same boxes written again). A claim older than the
    /// last log is refused whatever it shows: that window had not seen the log.
    fn admits(&self, claim: &EntryClaim) -> Result<(), &'static str> {
        let shown = Shown::of(&claim.call, &claim.fields);
        if let Some((logged, at)) = &self.logged {
            if claim.rev < *at {
                return Err(if *logged == shown {
                    ENTRY_LOGGED
                } else {
                    ENTRY_CHANGED
                });
            }
        }
        if claim.rev == self.entry.rev || Shown::of(&self.entry.call, &self.entry.fields) == shown {
            Ok(())
        } else {
            Err(ENTRY_CHANGED)
        }
    }

    /// The contact `claim` names is in the log: the entry moves on. The call and the marks go;
    /// the boxes stay, as they stay in a strip after a contact (the windows put the next
    /// contact's carried-over exchange).
    fn consume(&mut self, claim: &EntryClaim) {
        self.entry.rev += 1;
        self.entry.call.clear();
        self.entry.marks = serde_json::Value::Null;
        self.logged = Some((Shown::of(&claim.call, &claim.fields), self.entry.rev));
    }
}

impl Engine {
    /// The contest logger window opened (`on`) or closed. Opening it shares an empty entry —
    /// the main window's strip puts what it already holds — and opening it again while it is
    /// open changes nothing. Closing it ends the sharing: every strip keeps what it shows.
    ///
    /// A new sharing starts its revs past every rev the last one handed out, so a window still
    /// holding one from before can never pass for current.
    pub fn contest_entry_share(&mut self, on: bool) {
        if !on {
            if let Some(shared) = self.contest_entry.take() {
                self.contest_entry_revs = shared.entry.rev;
            }
            return;
        }
        if self.contest_entry.is_none() {
            let mut shared = SharedEntry::default();
            shared.entry.rev = self.contest_entry_revs + 1;
            self.contest_entry = Some(shared);
        }
    }

    /// The shared entry for the snapshot: `None` while the logger window is closed.
    pub(super) fn contest_entry_dto(&self) -> Option<ContestEntryDto> {
        self.contest_entry.as_ref().map(|s| s.entry.clone())
    }

    /// A window's strip changed: what it now shows becomes the entry, and the answer is the
    /// entry's new rev. The last writer wins — two people typing into one contact at one moment
    /// is the one case that has no better answer, and a log is checked again on its own
    /// ([`Self::contest_log_entry`]).
    ///
    /// Refused ([`ENTRY_CLOSED`]) with the logger window closed, and ([`ENTRY_TOO_LARGE`]) for an
    /// entry larger than any strip writes.
    pub fn contest_entry_put(
        &mut self,
        call: &str,
        fields: BTreeMap<String, String>,
        marks: serde_json::Value,
    ) -> Result<u64, String> {
        let Some(shared) = self.contest_entry.as_mut() else {
            return Err(ENTRY_CLOSED.into());
        };
        let too_large = call.len() > CALL_MAX
            || fields.len() > FIELDS_MAX
            || fields
                .iter()
                .any(|(k, v)| k.len() > KEY_MAX || v.len() > VALUE_MAX)
            || serde_json::to_vec(&marks).map_or(true, |m| m.len() > MARKS_MAX);
        if too_large {
            return Err(ENTRY_TOO_LARGE.into());
        }
        let entry = &mut shared.entry;
        entry.rev += 1;
        entry.call = call.to_string();
        entry.fields = fields;
        entry.marks = marks;
        Ok(entry.rev)
    }

    /// ⭐ **Log the shared contact once.** `log` is the strip's own log command and `landed`
    /// reads its answer: did the contact enter the log (a dupe refused by the ruleset did not).
    ///
    /// With a `claim` and the logger window open, the claim is checked first — refused with
    /// [`ENTRY_LOGGED`] or [`ENTRY_CHANGED`] and nothing logged — and a contact that landed
    /// moves the entry on, all in this one hold of the engine lock, which is what makes two
    /// windows pressing Enter at the same moment log one contact. With no claim, or the window
    /// closed, this is `log` and nothing else.
    pub fn contest_log_entry<T>(
        &mut self,
        claim: Option<&EntryClaim>,
        log: impl FnOnce(&mut Engine) -> Result<T, String>,
        landed: impl FnOnce(&T) -> bool,
    ) -> Result<T, String> {
        if let (Some(claim), Some(shared)) = (claim, self.contest_entry.as_ref()) {
            shared.admits(claim).map_err(String::from)?;
        }
        let out = log(self)?;
        if let (Some(claim), Some(shared)) = (claim, self.contest_entry.as_mut()) {
            if landed(&out) {
                shared.consume(claim);
            }
        }
        Ok(out)
    }
}
