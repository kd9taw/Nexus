//! The Logbook's exports, read from the logbook store — SPEC-2 v3's **C15**.
//!
//! The ADIF and CSV export (whole, or a date range, #98), one operator's contacts (#25), one
//! activation's (POTA's unit of credit), and the two lists those splits are chosen from. Until
//! C15 each was a pass over the log in memory made UNDER the Engine lock — hundreds of
//! milliseconds at 150,000 contacts, with the radio loop waiting on the same lock every 20 ms.
//! Now each reads the store with the lock released, a chunk of whole records at a time
//! ([`LogRows::each_record`]), and runs the export rule it always ran over what it reads.
//!
//! # Byte-identical, by construction and by test
//!
//! The standing ruling is that an export does not change. The rules are not copied here: they
//! are [`Export`], [`Operators`] and [`Activations`] in tempo-core, the very code
//! `Logbook::adif_in_range` and its siblings run over the log in memory. What reaches them is
//! each record as the store's one decoder reads it back, in log order — and the store holds
//! exactly what memory holds (SPEC-2 v3 D2-A saved the country and state fills too).
//!
//! # An export never leaves out a change it was asked after
//!
//! A pass first waits for every change made before it was asked for to reach the store — up to
//! [`EXPORT_WAIT`], the wait an operator's own command gives its change. A store that has not
//! taken them by then (a bulk import still being written, a disk refusing the write) is not
//! exported from: the file would silently lack contacts the Logbook shows, and an operator
//! submitting a log to POTA or ARRL would never know. The export says why instead.
//!
//! ⚠️ Every function here reads the store: never call one under the Engine lock (a debug build
//! panics). Take the rows ([`crate::engine::Engine::log_rows`]) under it, release it, then call.

use std::ops::ControlFlow;
use std::time::Duration;

use tempo_core::logbook::{Activations, Export, ExportKind, LoggedActivation, Operators};

use crate::logstore::{Freshness, LogRows, DURABLE_WAIT};

/// How long an export waits for the changes made before it was asked for to reach the store:
/// as long as an operator's own command waits for its change ([`DURABLE_WAIT`]).
pub const EXPORT_WAIT: Duration = DURABLE_WAIT;

/// The general logbook as `format` — `"csv"` (any case) for CSV, anything else ADIF — bounded to
/// the QSOs starting in `[from_unix, to_unix]` (inclusive; absent = unbounded). The Logbook's
/// Export button (`Logbook::adif_in_range` / `csv_in_range`).
pub fn export_logbook(
    rows: &LogRows,
    format: &str,
    from_unix: Option<u64>,
    to_unix: Option<u64>,
) -> Result<String, String> {
    let kind = match format.to_ascii_lowercase().as_str() {
        "csv" => ExportKind::Csv {
            from: from_unix,
            to: to_unix,
        },
        _ => ExportKind::Adif {
            from: from_unix,
            to: to_unix,
        },
    };
    export(rows, kind)
}

/// ADIF of `operator`'s contacts (`Logbook::adif_for_operator`).
pub fn export_for_operator(rows: &LogRows, operator: &str) -> Result<String, String> {
    export(rows, ExportKind::Operator(operator.to_string()))
}

/// ADIF of ONE activation: `reference`, on the UTC day containing `day_start_unix`, worked under
/// `callsign` (`Logbook::adif_for_activation`).
pub fn export_for_activation(
    rows: &LogRows,
    reference: &str,
    day_start_unix: u64,
    callsign: Option<&str>,
) -> Result<String, String> {
    export(
        rows,
        ExportKind::Activation {
            reference: reference.to_string(),
            day_start_unix,
            callsign: callsign.map(str::to_string),
        },
    )
}

/// Every distinct operator in the log, uppercased and sorted (`Logbook::operators`).
pub fn operators(rows: &LogRows) -> Result<Vec<String>, String> {
    let mut found = Operators::default();
    pass(rows, EXPORT_WAIT, &mut |r| found.add(r))?;
    Ok(found.finish())
}

/// Every distinct activation in the log, newest first (`Logbook::activations`).
pub fn activations(rows: &LogRows) -> Result<Vec<LoggedActivation>, String> {
    let mut found = Activations::default();
    pass(rows, EXPORT_WAIT, &mut |r| found.add(r))?;
    Ok(found.finish())
}

fn export(rows: &LogRows, kind: ExportKind) -> Result<String, String> {
    export_waiting(rows, kind, EXPORT_WAIT)
}

/// [`export`], waiting at most `wait` for the store — what a test shortens.
pub(crate) fn export_waiting(
    rows: &LogRows,
    kind: ExportKind,
    wait: Duration,
) -> Result<String, String> {
    let mut e = Export::new(kind);
    pass(rows, wait, &mut |r| e.add(r))?;
    Ok(e.finish())
}

/// Every record of the log, whole and in log order, through `take` — or why the pass would not
/// be the log the Logbook shows.
fn pass(
    rows: &LogRows,
    wait: Duration,
    take: &mut dyn FnMut(&tempo_core::logbook::QsoRecord),
) -> Result<(), String> {
    let fresh = rows
        .each_record(wait, &mut |r| {
            take(r);
            ControlFlow::Continue(())
        })
        .map_err(|e| format!("The logbook could not be read for the export: {e}"))?;
    match fresh {
        Freshness::Current => Ok(()),
        Freshness::Stale(why) => Err(format!(
            "The logbook has changes it has not saved yet ({why}). The export would leave them \
             out, so it was not made — try again once they are saved."
        )),
    }
}
