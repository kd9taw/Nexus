//! Curating the station's working channel list from a browser: rename a row, move it, drop it, or
//! clear the list. It is the half of the Program view an operator does AFTER the machines are in
//! the list, and with the CHIRP/CSV export beside it (`program_export`) it is the whole
//! curate-then-deliver path from wherever the operator is sitting.
//!
//! **A row is named by its channel ID, never by a position.** A position is stale the moment
//! anything else touches the list, and the browser is looking at a document it fetched seconds ago.
//! The same rule the log changes follow, for the same reason.
//!
//! Every change is checked against the `programming` document revision the browser was SHOWING.
//! The revision is computed by `query::configuration::programming_revision`, which is the read the
//! browser was served, so the two cannot drift apart into a check that always passes.
//!
//! ⛔ What this deliberately does NOT do, because each needs something this path does not have:
//! - **Import a CHIRP CSV.** A file goes UP, in bulk. Remote has no chunked upload anywhere, and an
//!   operation request is 6 KiB; inventing a second byte path for one button is not this change.
//! - **Fetch from a directory.** RepeaterBook needs the station's own API token (credential-gated)
//!   and hearham is a live station-side HTTP fetch; both are acquisition, not curation.
//! - **Save to Memories / star a channel.** The memory bank is owned by the desktop WebView and
//!   only ever published one way into this service, so there is nothing here to write it through.
//!
//! The file is written by `crate::store_radioprog`, the desktop's own writer, so there is exactly
//! one writer of `radioprog.json` exactly as there is one reader of it.
use super::logging::{ChangeReason, ChangeWork};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One curation gesture. `deny_unknown_fields` on a closed set: nothing a browser sends can name a
/// path, a project, or a whole list of channels.
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum Edit {
    /// The operator's own label for the row, as the radio will display it. Not truncated here: the
    /// per-rig cap applies at export time only, exactly as it does on the desktop.
    Rename {
        id: String,
        name: String,
    },
    Remove {
        id: String,
    },
    /// One place up (-1) or down (+1) — the ▲▼ buttons, which is the only reorder the view offers.
    Move {
        id: String,
        by: i8,
    },
    Clear {},
}

impl Edit {
    /// The wire grammar. The ID and name bounds are the `programming` document's own
    /// (`configuration.ts` parses channels with `text(c.id, 256)` and `text(c.name, 1024)`), so a
    /// value that could never have been read out cannot be written back in.
    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Rename { id, name } => {
                channel_id(id) && name.len() <= 1024 && !name.contains(['\n', '\r'])
            }
            Self::Remove { id } => channel_id(id),
            Self::Move { id, by } => channel_id(id) && (*by == -1 || *by == 1),
            Self::Clear {} => true,
        }
    }
}

fn channel_id(id: &str) -> bool {
    (1..=256).contains(&id.len())
}

/// Apply one curation gesture to the working project, against the revision the browser showed.
///
/// ⚠️ NOT under the Engine lock in any meaningful sense: nothing here reads or moves the radio. It
/// runs on the operations path like every other change, and it touches one file.
pub(super) fn prepare(
    path: &Path,
    revision: &str,
    edit: &Edit,
) -> Result<ChangeWork, ChangeReason> {
    // What the operator changed must be what the station still holds. Computed by the SAME function
    // that served the document, so a drifting second implementation cannot make this always pass.
    if super::super::query::programming_revision(path)
        .ok()
        .as_deref()
        != Some(revision)
    {
        return Err(ChangeReason::ContextChanged);
    }
    let mut file = super::program_export::radioprog(path)
        .map_err(|_| ChangeReason::ContextChanged)?
        .ok_or(ChangeReason::InvalidChange)?;
    let Some(project) = file
        .projects
        .iter_mut()
        .find(|p| p.id == super::program_export::WORKING_PROJECT_ID)
    else {
        // The revision matched a file with no working list at all, so the row named is not there.
        return Err(ChangeReason::InvalidChange);
    };
    let at = |channels: &[propagation::memchan::Channel], id: &str| {
        channels.iter().position(|c| c.id == id)
    };
    match edit {
        Edit::Rename { id, name } => {
            let i = at(&project.channels, id).ok_or(ChangeReason::InvalidChange)?;
            project.channels[i].name = name.clone();
        }
        Edit::Remove { id } => {
            let i = at(&project.channels, id).ok_or(ChangeReason::InvalidChange)?;
            project.channels.remove(i);
        }
        Edit::Move { id, by } => {
            let i = at(&project.channels, id).ok_or(ChangeReason::InvalidChange)?;
            // Off either end is not a move. The browser disables those two buttons, so reaching
            // here means the list moved under the operator — refused rather than silently ignored.
            let j = i
                .checked_add_signed(*by as isize)
                .filter(|j| *j < project.channels.len())
                .ok_or(ChangeReason::InvalidChange)?;
            project.channels.swap(i, j);
        }
        Edit::Clear {} => project.channels.clear(),
    }
    project.updated_utc = crate::now_unix();
    // The desktop's own writer, so `radioprog.json` has exactly one writer.
    //
    // A write that failed part way cannot say what the file now holds, so the browser is told
    // UNKNOWN and re-reads — never that the change was rejected, which would be a claim the list
    // is unchanged. Same shape as the settings save beside it.
    Ok(match crate::store_radioprog(path, &file) {
        Ok(()) => ChangeWork::ProgramSaved,
        Err(_) => ChangeWork::ProgramUnconfirmed,
    })
}

#[cfg(test)]
#[path = "program_edit_tests.rs"]
mod tests;
