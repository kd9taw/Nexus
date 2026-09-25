//! One POTA/SOTA activation file for a Remote browser. The file is exactly what the desktop's
//! per-activation export writes (`tempo_app::logexport::export_for_activation`, tempo-core's
//! `Export` rule), and a browser can name one activation the log lists and nothing else: no date
//! range, no search, never the whole log. It is a READ under the logging grant and that browser's
//! own current lease. It spends no command sequence, writes nothing and never touches the radio.
//!
//! The list and the file come from ONE picture of the logbook store, read with the Engine lock
//! released (`query::picture`, SPEC-2 v3 C18): the activation the list names is the one its file
//! is cut from, whatever commits meanwhile.
//!
//! An operation reply is small, so the file travels in bounded chunks, each carrying the file's
//! length and SHA-256. The station keeps nothing between chunks and rebuilds the file for each one,
//! so a log that changed mid-download arrives as a different file description, and the browser
//! refuses it rather than stitching two logs together.
use super::super::query::picture;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempo_app::dto::LoggedActivationDto;
use tempo_app::logstore::LogRows;
use tempo_core::logbook::sqlite::Narrow;
use tempo_core::logbook::{Activations, Export, ExportKind};

/// Mirrors ACTIVATION_EXPORT_* in ui/src/remote-web/operation-protocol.ts. They bound every export
/// that travels this way, not only an activation's: one ceiling and one chunk size means a browser
/// checks the same two numbers whatever it asked for.
pub(super) const MAX_BYTES: usize = 1024 * 1024;
pub(super) const CHUNK_BYTES: usize = 32 * 1024;
const LISTED: usize = 128;

/// One chunk of a built export file, described by the WHOLE file's length and SHA-256 so a browser
/// can tell that the thing it is stitching together stopped being one file half way down. The
/// station keeps nothing between chunks — each call rebuilds the text — so a file that changed
/// arrives as a different description rather than as a silent splice.
///
/// `operation` names the reply for the browser's own parser: an export is the one operation reply
/// allowed past the ordinary response ceiling, and the client matches the name it asked for.
pub(super) fn chunked(operation: &str, text: &str, index: u32) -> Result<Value, &'static str> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_BYTES {
        return Ok(json!({ "operation": operation, "refused": "tooLarge" }));
    }
    let chunks = bytes.len().div_ceil(CHUNK_BYTES);
    let index = index as usize;
    if index >= chunks {
        return Err("invalidRequest");
    }
    let sha256: String = digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let chunk = &bytes[index * CHUNK_BYTES..bytes.len().min((index + 1) * CHUNK_BYTES)];
    Ok(json!({"operation":operation,
        "file":{"byteLength":bytes.len(),"sha256":sha256,"chunks":chunks},
        "index":index,"base64":crate::b64_encode(chunk)}))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    reference: String,
    day_start_unix: u64,
    /// Null for records that carry no callsign, never omitted: serde fills a MISSING `Option` with
    /// None, which would read an omitted key as "no callsign".
    #[serde(deserialize_with = "Option::deserialize")]
    callsign: Option<String>,
}

impl Selection {
    fn valid(&self) -> bool {
        (1..=32).contains(&self.reference.len())
            && self
                .reference
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/' || b == b'-')
            && self.day_start_unix.is_multiple_of(86_400)
            && self.day_start_unix <= 253_402_214_400
            && self.callsign.as_deref().is_none_or(|c| {
                (3..=32).contains(&c.len())
                    && c.bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
            })
    }
    fn names(&self, a: &LoggedActivationDto) -> bool {
        a.reference == self.reference
            && a.day_start_unix == self.day_start_unix
            && a.callsign == self.callsign
    }
}

/// What the activation list reads of every contact — the callsign it was worked under
/// (`STATION_CALLSIGN`, else `OPERATOR`), its time, your reference and your program — which is also
/// all a file needs to find its contacts: their UTC day.
const ACTIVATIONS: Narrow = Narrow {
    columns: &[
        "station_callsign",
        "operator",
        "when_unix",
        "ota_my_ref",
        "ota_my_program",
    ],
    uploads: false,
};

/// What one read of the log answers.
enum Answer {
    /// Every activation a browser can name back.
    Listed(Vec<LoggedActivationDto>),
    /// The selection names no listed activation.
    NotFound,
    /// The selected activation's file.
    File(String),
}

/// A refused read of the log in an operation's words — the only ones a page accepts in an
/// operation reply (`OPERATION_ERRORS`, ui/src/remote-web/operation-protocol.ts): a writer still
/// behind is `stationBusy`, which a page retries; a store that cannot be read is
/// `stationUnavailable`. The export's reads, and a log change's search for its key target.
pub(super) fn in_operation_words(refused: &'static str) -> &'static str {
    match refused {
        "applicationBusy" => "stationBusy",
        _ => "stationUnavailable",
    }
}

/// The list, or one listed activation's file, from ONE picture of the log: one pass takes every
/// activation and, for a file, keeps its UTC day's contacts — the only ones the export's rule can
/// take (SQL narrows, the rule decides) — and only those are read whole.
fn read(rows: &LogRows, selection: Option<&Selection>) -> Result<Answer, &'static str> {
    picture::read(rows, |log| {
        let mut found = Activations::default();
        let day = selection.map(|s| s.day_start_unix - s.day_start_unix % 86_400);
        let mut picks = Vec::new();
        log.each(ACTIVATIONS, &mut |pick, q| {
            found.add(q);
            if day.is_some_and(|d| q.when_unix >= d && q.when_unix < d + 86_400) {
                picks.push(pick);
            }
            Ok(())
        })?;
        // Only activations a browser can name back. A reference imported with other bytes is left
        // out rather than offered and then refused.
        let listed: Vec<LoggedActivationDto> = found
            .finish()
            .into_iter()
            .map(LoggedActivationDto::from)
            .filter(|a| {
                Selection {
                    reference: a.reference.clone(),
                    day_start_unix: a.day_start_unix,
                    callsign: a.callsign.clone(),
                }
                .valid()
            })
            .collect();
        let Some(selection) = selection else {
            return Ok(Answer::Listed(listed));
        };
        if !listed.iter().any(|a| selection.names(a)) {
            return Ok(Answer::NotFound);
        }
        let mut file = Export::new(ExportKind::Activation {
            reference: selection.reference.clone(),
            day_start_unix: selection.day_start_unix,
            callsign: selection.callsign.clone(),
        });
        for r in log.whole(&picks)? {
            file.add(&r);
        }
        Ok(Answer::File(file.finish()))
    })
    .map_err(in_operation_words)
}

/// The reply to one read: the list (no selection, chunk 0 only), one chunk of one listed
/// activation's file, or a refusal that names why nothing was sent.
///
/// ⚠️ It reads the store: never under the Engine lock. Take `rows` (`Engine::log_rows`) under it.
pub(super) fn respond(
    rows: &LogRows,
    selection: Option<&Selection>,
    index: u32,
) -> Result<Value, &'static str> {
    if selection.is_some_and(|s| !s.valid()) || (selection.is_none() && index != 0) {
        return Err("invalidRequest");
    }
    match read(rows, selection)? {
        Answer::Listed(listed) => Ok(
            json!({"operation":"activationExport","activations":&listed[..listed.len().min(LISTED)]}),
        ),
        Answer::NotFound => Ok(json!({"operation":"activationExport","refused":"notFound"})),
        Answer::File(text) => chunked("activationExport", &text, index),
    }
}

#[cfg(test)]
#[path = "export_parity_tests.rs"]
mod tests;
