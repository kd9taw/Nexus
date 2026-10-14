//! ⭐ **The station as it was before the contest changed it** — the note the contest-over
//! checklist offers back when Contest mode goes off: the station callsign a club set, the operator
//! at the key, the position, the entry category (MULTI-OP), the entry class, the power category, the
//! band category and Unassisted ([`ContestBefore`]).
//!
//! **When it is taken.** Only for a contest: from a week before the contest picked in Settings
//! until it ends, or while Contest mode is on. The picker's blank default is ARRL Field Day, the
//! contest the mode runs from it (`fdevent::canonical_contest`), so it has Field Day's week. Then,
//! the first time since the last contest ended that one of those eight changes — a Settings save, a
//! seat swap, the Unassisted switch — or Contest mode goes on: the eight as they were just BEFORE
//! that change. A club picks its contest and types its call, its position and MULTI-OP into
//! Settings on the Friday, long before anyone turns the mode on, so a note taken only at the switch
//! would hold the club's call as "before" and offer nothing back; taken at the first change it
//! holds the station's own. The pick is read as the change leaves it, so a save that picks the
//! contest and sets the club's call together takes the note. Outside that week and with the mode
//! off, a change is the station's everyday business, though a contest stays picked after it ends:
//! no note is taken, so no later contest's checklist offers back a callsign changed months ago. A
//! pick the rules data does not carry never counts.
//!
//! **When it is let go.** When it is used: Contest mode goes off with all of it put back. When it
//! is dismissed: Contest mode goes off after its contest has ended with the rest declined, or a
//! change or a launch finds the mode off and the contest over. Declined while the contest is still
//! ahead or running (a rehearsal turning the mode off and on again), it stays, so the checklist at
//! the contest's end still offers the station's own call. (The checklist puts Unassisted back
//! through its own switch BEFORE the save that turns the mode off, so that save finds everything it
//! put back.) A note an earlier build took at an everyday change goes at launch by the same rule.
//!
//! **Who writes it.** Only this. A Settings save carries a copy of every field, and a copy is a
//! snapshot from whenever its surface loaded, so `apply_settings` keeps the engine's own note
//! whatever the payload says: a stale panel can neither write one nor wipe one. A RESTORE takes the
//! backup's, as it takes everything else.
//!
//! **What it never does.** Put anything back. It is a note; the checklist shows it beside today's
//! values and the operator ticks what goes back, through the ordinary Settings save and the
//! Unassisted switch. Nothing here keys, unkeys or stops a transmission.
use super::Engine;
use crate::settings::{ContestBefore, Settings};
use tempo_core::fd_rules;

/// How long before its contest a pick starts to count: the week a club sets up in.
const PICK_LEAD_SECS: u64 = 7 * 24 * 3600;

// A test's clock for the contest's week, per thread like `logstore::SESSION_READS` (the harness runs
// tests in parallel), or None for the wall clock. (A plain comment: doc comments cannot attach
// through `thread_local!`.)
#[cfg(test)]
thread_local! {
    pub(super) static NOW_FOR_TESTS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The time a pick is read at: the wall clock, or a test's.
fn now() -> u64 {
    #[cfg(test)]
    if let Some(t) = NOW_FOR_TESTS.with(|c| c.get()) {
        return t;
    }
    super::now_unix_secs()
}

/// Does the contest picked in Settings count at `now`? From a week before its running until that
/// running ends, by the rules data's dates; the blank default, spaces too, is ARRL Field Day.
fn pick_counts(s: &Settings, now: u64) -> bool {
    let id = crate::fdevent::canonical_contest(&s.fd_event);
    fd_rules::ruleset_by_id(&id, fd_rules::CURRENT_RULES_YEAR).is_some_and(|rs| {
        // The running under way, or else the next: either way `now` is before its end.
        now.saturating_add(PICK_LEAD_SECS) >= rs.next_or_running(now).start_unix
    })
}

impl Engine {
    /// After a change to the settings: `was` is the eight as they were before it, `was_active`
    /// whether Contest mode was on. When the mode went off, lets the note go if it is used or its
    /// contest is over. Otherwise, while a pick counts or the mode is on, takes it, if none is
    /// open, when the mode went on or one of the eight moved; with neither, takes none and lets go
    /// of one whose contest is over.
    pub(super) fn note_contest_before(&mut self, was: ContestBefore, was_active: bool) {
        let active = self.settings.fd_active;
        let picked = pick_counts(&self.settings, now());
        if was_active && !active {
            // Contest mode went off. All of it put back, the note is used; with the contest over,
            // what was declined is dismissed. Declined before the contest ends, it stays for the end.
            let used =
                self.settings.contest_before.as_ref() == Some(&ContestBefore::of(&self.settings));
            if used || !picked {
                self.settings.contest_before = None;
            }
            return;
        }
        if !active && !picked {
            // No contest near and none running: an everyday change. It takes no note, and a note
            // left from a contest that is over goes.
            self.settings.contest_before = None;
            return;
        }
        // None open, and this change is the contest's first: the mode went on, or one of the eight
        // moved. What they were just before it is the station's own.
        let moved = ContestBefore::of(&self.settings) != was;
        if self.settings.contest_before.is_none() && ((active && !was_active) || moved) {
            self.settings.contest_before = Some(was);
        }
    }

    /// At launch, on the stored settings: lets go of a note held with the mode off and no pick that
    /// counts, one whose contest is over or an earlier build's everyday one. One with a pick that
    /// counts or the mode on is kept.
    pub(super) fn forget_an_everyday_note(s: &mut Settings) {
        if !s.fd_active && !pick_counts(s, now()) {
            s.contest_before = None;
        }
    }
}
