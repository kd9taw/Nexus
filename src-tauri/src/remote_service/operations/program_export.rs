//! The station's working channel list as a CHIRP or spreadsheet CSV, for a browser that wants the
//! file on the machine it is sitting at rather than in the shack's Downloads folder.
//!
//! The text is written by the SAME two writers the desktop's `export_channels` command calls
//! (`propagation::chirp::to_chirp_csv`, `propagation::memchan::to_generic_csv`), from the SAME
//! working project the browser is already looking at through the `programming` configuration
//! collection. Nothing is re-implemented on the browser side: a second CHIRP writer in TypeScript
//! would drift from this one silently, and the file it produced would be wrong in a radio rather
//! than wrong on screen.
//!
//! It is a READ under station control and that browser's own lease. It spends no command sequence,
//! writes nothing, and never touches the radio. The file travels in the same bounded chunks as an
//! activation export, with the same ceiling and the same `tooLarge` refusal (`export::chunked`).
//!
//! ⛔ **No attribution line is written for a remote export.** The desktop stamps the directory the
//! rows were fetched from because it still has the search result on screen; `radioprog.json` does
//! not record where a channel came from, so the station cannot know, and naming either directory
//! would put a claim in a shipped file that nothing checked. An absent credit is honest; a guessed
//! one is not.
use super::export;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

/// The single auto-saved working project — `WORKING_PROJECT_ID` in `RadioProgView.tsx`.
pub(super) const WORKING_PROJECT_ID: &str = "working";
/// `radioprog.json` is read with the same ceiling the `programming` collection reads it with.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// What the operator picked in the deliver row. `chirp` is the CHIRP generic CSV (analog rows
/// only); `csv` is the plain spreadsheet dump of every row.
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    Chirp,
    Csv,
}

/// The per-radio name cap the operator chose (`NAME_CAPS` in `features/radioprog.ts`). Clamped the
/// same way the desktop command clamps it, so a browser cannot ask for a cap no rig has.
fn name_cap(requested: u32) -> usize {
    (requested as usize).clamp(4, 16)
}

/// The reply to one read: one chunk of the rendered file, or a refusal that names why nothing was
/// sent. `notFound` means the station has no working channel list to export — the same state the
/// browser sees as an empty builder, so its own button is already disabled.
pub(super) fn respond(
    path: &Path,
    format: Format,
    cap: u32,
    index: u32,
) -> Result<Value, &'static str> {
    let Some(channels) = working_channels(path)? else {
        return Ok(json!({ "operation": "programExport", "refused": "notFound" }));
    };
    if channels.is_empty() {
        return Ok(json!({ "operation": "programExport", "refused": "notFound" }));
    }
    let text = match format {
        Format::Chirp => propagation::chirp::to_chirp_csv(&channels, name_cap(cap), ""),
        Format::Csv => propagation::memchan::to_generic_csv(&channels, ""),
    };
    export::chunked("programExport", &text, index)
}

/// The working project's channels, or `None` when the file has no working project yet.
fn working_channels(
    path: &Path,
) -> Result<Option<Vec<propagation::memchan::Channel>>, &'static str> {
    Ok(radioprog(path)?.and_then(|file| {
        file.projects
            .into_iter()
            .find(|p| p.id == WORKING_PROJECT_ID)
            .map(|p| p.channels)
    }))
}

/// `radioprog.json` as the Remote service reads it, or `None` when there is no file yet. Shared
/// with the curation path (`program_edit`), so both halves of Program see one file the same way.
///
/// ⚠️ NOT `crate::load_radioprog`: that one turns an unreadable or half-written file into an empty
/// default. Exporting that would hand the operator a blank CSV as though their channels were gone,
/// and writing through it would then SAVE the blank list over them. A file that cannot be read is
/// an error here, and the browser is told so rather than handed nothing.
pub(super) fn radioprog(path: &Path) -> Result<Option<crate::RadioProgFile>, &'static str> {
    // Refuse special files before opening — a FIFO can block in `open` itself.
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() => return Err("applicationUnavailable"),
        Ok(meta) if meta.len() > MAX_FILE_BYTES => return Err("applicationTooLarge"),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("applicationUnavailable"),
    }
    let mut bytes = Vec::new();
    match std::fs::File::open(path) {
        Ok(file) => {
            if !file
                .metadata()
                .map_err(|_| "applicationUnavailable")?
                .is_file()
            {
                return Err("applicationUnavailable");
            }
            file.take(MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "applicationUnavailable")?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err("applicationTooLarge");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("applicationUnavailable"),
    }
    let file: crate::RadioProgFile =
        serde_json::from_slice(&bytes).map_err(|_| "applicationUnavailable")?;
    if file.version != 1 {
        return Err("applicationUnavailable");
    }
    Ok(Some(file))
}

#[cfg(test)]
#[path = "program_export_tests.rs"]
mod tests;
