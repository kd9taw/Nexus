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
//! Every change this process made before the export was asked for must be in the store. The
//! export takes them as the quit does ([`Unsaved`]: the tickets of the changes still on their
//! way, and the ones the writer gave up on) and first waits for them — up to [`EXPORT_WAIT`],
//! the wait an operator's own command gives its change. If they are not all saved by then (a
//! bulk import still being written, a disk refusing the write), there is no export: the file
//! would silently lack contacts the Logbook shows, and an operator submitting a log to POTA or
//! ARRL would never know. It says why instead.
//!
//! ⚠️ Why not the read's own freshness ([`crate::logstore::Freshness`]): it rests on the writer's
//! durability watermark, which the first change the writer ever gives up on caps for good — so
//! after one refusal, even one saved later by a re-send, every read would count as stale and no
//! export could be made for the rest of the session. The changes' own tickets say exactly what
//! is saved now.
//!
//! ⚠️ Every function here reads the store: never call one under the Engine lock (a debug build
//! panics). Take the [`Source`] under it, release it, then call.

use std::ops::ControlFlow;
use std::time::Duration;

use tempo_core::logbook::{Activations, Export, ExportKind, LoggedActivation, Operators};

use crate::logstore::{LogRows, Unsaved, DURABLE_WAIT, READ_WAIT};

/// How long an export waits for the changes made before it was asked for to reach the store:
/// as long as an operator's own command waits for its change ([`DURABLE_WAIT`]).
pub const EXPORT_WAIT: Duration = DURABLE_WAIT;

/// What an export reads, taken under the Engine lock — handles, no I/O: the log's rows, and this
/// process's changes that were not in the store yet when the export was asked for.
pub struct Source {
    rows: LogRows,
    unsaved: Unsaved,
}

impl Source {
    /// The export source of the log `engine` holds. Take it under the Engine lock, and read it
    /// with the lock released.
    pub fn of(engine: &crate::engine::Engine) -> Source {
        Source {
            rows: engine.log_rows(),
            unsaved: engine.log_unsaved(),
        }
    }

    /// `rows`, with nothing owed to them — the log in memory as a test hands it over.
    #[cfg(test)]
    pub(crate) fn of_rows(rows: LogRows) -> Source {
        Source {
            rows,
            unsaved: Unsaved::default(),
        }
    }
}

/// The general logbook as `format` — `"csv"` (any case) for CSV, anything else ADIF — bounded to
/// the QSOs starting in `[from_unix, to_unix]` (inclusive; absent = unbounded). The Logbook's
/// Export button (`Logbook::adif_in_range` / `csv_in_range`).
pub fn export_logbook(
    from: &Source,
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
    export_waiting(from, kind, EXPORT_WAIT)
}

/// ADIF of `operator`'s contacts (`Logbook::adif_for_operator`).
pub fn export_for_operator(from: &Source, operator: &str) -> Result<String, String> {
    export_waiting(
        from,
        ExportKind::Operator(operator.to_string()),
        EXPORT_WAIT,
    )
}

/// ADIF of ONE activation: `reference`, on the UTC day containing `day_start_unix`, worked under
/// `callsign` (`Logbook::adif_for_activation`).
pub fn export_for_activation(
    from: &Source,
    reference: &str,
    day_start_unix: u64,
    callsign: Option<&str>,
) -> Result<String, String> {
    export_waiting(
        from,
        ExportKind::Activation {
            reference: reference.to_string(),
            day_start_unix,
            callsign: callsign.map(str::to_string),
        },
        EXPORT_WAIT,
    )
}

/// Every distinct operator in the log, uppercased and sorted (`Logbook::operators`).
pub fn operators(from: &Source) -> Result<Vec<String>, String> {
    let mut found = Operators::default();
    pass(from, EXPORT_WAIT, &mut |r| found.add(r))?;
    Ok(found.finish())
}

/// Every distinct activation in the log, newest first (`Logbook::activations`).
pub fn activations(from: &Source) -> Result<Vec<LoggedActivation>, String> {
    let mut found = Activations::default();
    pass(from, EXPORT_WAIT, &mut |r| found.add(r))?;
    Ok(found.finish())
}

/// The export `kind`, waiting at most `wait` for this process's changes to reach the store —
/// what a test shortens.
pub(crate) fn export_waiting(
    from: &Source,
    kind: ExportKind,
    wait: Duration,
) -> Result<String, String> {
    let mut e = Export::new(kind);
    pass(from, wait, &mut |r| e.add(r))?;
    Ok(e.finish())
}

/// Every record of the log, whole and in log order, through `take` — once every change made
/// before the export was asked for is in the store, or why it is not.
fn pass(
    from: &Source,
    wait: Duration,
    take: &mut dyn FnMut(&tempo_core::logbook::QsoRecord),
) -> Result<(), String> {
    let standing = from.unsaved.wait(wait);
    if !standing.saved() {
        let n = standing.pending + standing.retryable + standing.refused;
        let why = standing
            .retry_reason
            .or(standing.reason)
            .map(|w| format!(" ({w})"))
            .unwrap_or_default();
        return Err(format!(
            "The logbook has {n} change(s) not saved to its database yet{why}. The export would \
             leave them out, so it was not made — try again once they are saved."
        ));
    }
    // Every one of them is in: what the read says of its own freshness adds nothing (see the
    // module header).
    from.rows
        .each_record(READ_WAIT, &mut |r| {
            take(r);
            ControlFlow::Continue(())
        })
        .map_err(|e| format!("The logbook could not be read for the export: {e}"))?;
    Ok(())
}
