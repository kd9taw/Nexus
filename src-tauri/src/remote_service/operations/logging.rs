//! QSO actions use the logging grant, shared native policy and durable receipts.
//! Admission happens under the original controller window. Finishing an append
//! cannot acquire TX authority, retry a write or clear a replacement contact.
//!
//! Log changes (edit, delete, QSL marks) follow the same rules. A row is found again by the key
//! of the exact row the browser's log page showed, never by a position, so a row that changed at
//! the station since that page is refused rather than overwritten. The engine's rewrite does not
//! sync and reports a failed save only to stderr, so "applied" is claimed only after the log file
//! itself has been re-read, shown to hold the change, and synced.
use super::station::Action;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tempo_app::engine::{
    remote_logging::{
        CurrentQsoLogOutcome, JournalSync, LogFailure, PendingJournalWrite, PendingLogConfirmation,
    },
    remote_transmit::FtExchangeContext,
    Engine, LogWriteOutcome,
};
use tempo_app::remote_control::{Evidence, Outcome, Reason};
use tempo_core::logbook::{adif_record, QslVia, QsoRecord};

pub(super) enum Work {
    Append(LogWriteOutcome),
    Pending(PendingJournalWrite),
    Confirm(PendingLogConfirmation),
    Discard(JournalSync),
}

fn key(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn reason(failure: LogFailure) -> Reason {
    match failure {
        LogFailure::StalePending => Reason::ContextChanged,
        LogFailure::InvalidEdits => Reason::InvalidAction,
        LogFailure::AlreadyPresent => Reason::AlreadyPresent,
        LogFailure::PersistenceUnconfirmed => Reason::PersistenceFailed,
    }
}

pub(super) fn prepare(engine: &mut Engine, action: &Action) -> Result<Work, Reason> {
    match action {
        Action::QsoLogCurrent {
            expected_key,
            expected_tier,
            expected_qso,
        } => {
            if !key(expected_key, 32) {
                return Err(Reason::InvalidAction);
            }
            if engine.current_qso_log_key().as_deref() != Some(expected_key)
                || engine.tier() != *expected_tier
                || engine
                    .snapshot()
                    .qso
                    .as_ref()
                    .map(FtExchangeContext::from)
                    .as_ref()
                    != Some(expected_qso)
            {
                return Err(Reason::ContextChanged);
            }
            match engine.log_current_qso_for_sync() {
                CurrentQsoLogOutcome::NoEligibleContact => Err(Reason::NoEligibleContact),
                CurrentQsoLogOutcome::PendingExists => Err(Reason::PendingConfirmationRequired),
                CurrentQsoLogOutcome::Append(outcome) => Ok(Work::Append(outcome)),
                CurrentQsoLogOutcome::Pending(write) => Ok(Work::Pending(write)),
            }
        }
        Action::QsoConfirm {
            expected_key,
            edits,
        } => {
            if !key(expected_key, 16) {
                return Err(Reason::InvalidAction);
            }
            let pending = engine
                .pending_log_identity()
                .ok_or(Reason::ContextChanged)?;
            if pending.key().as_deref() != Some(expected_key) {
                return Err(Reason::ContextChanged);
            }
            engine
                .confirm_pending_log_for_sync(pending, edits.clone())
                .map(Work::Confirm)
                .map_err(reason)
        }
        Action::QsoDiscard { expected_key } => {
            if !key(expected_key, 16) {
                return Err(Reason::InvalidAction);
            }
            let pending = engine
                .pending_log_identity()
                .ok_or(Reason::ContextChanged)?;
            if pending.key().as_deref() != Some(expected_key) {
                return Err(Reason::ContextChanged);
            }
            engine
                .discard_pending_log_for_sync(&pending)
                .map(Work::Discard)
                .map_err(reason)
        }
        _ => Err(Reason::UnsupportedAction),
    }
}

impl Work {
    /// Engine is unlocked throughout storage sync. A short, guarded Engine
    /// completion may wait for its owner, but never holds Engine while waiting
    /// for disk. Once append begins, revocation cannot roll it back.
    pub(super) fn finish(self, shared: &crate::SharedEngine) -> Outcome {
        let result = (|| -> Result<Evidence, Reason> {
            match self {
                Self::Append(LogWriteOutcome::PendingSync(receipts)) if !receipts.is_empty() => {
                    receipts
                        .into_iter()
                        .try_for_each(|r| r.sync())
                        .map_err(|_| Reason::PersistenceFailed)?;
                    Ok(Evidence::FileSynced)
                }
                Self::Append(LogWriteOutcome::Duplicate) => Err(Reason::AlreadyPresent),
                Self::Append(_) => Err(Reason::PersistenceFailed),
                Self::Pending(write) => {
                    let prepared = write.prepare().map_err(|_| Reason::PersistenceFailed)?;
                    let receipt = shared
                        .lock()
                        .map_err(|_| Reason::StationBusy)?
                        .publish_pending_qso_journal(prepared)
                        .map_err(reason)?;
                    receipt.sync().map_err(|_| Reason::PersistenceFailed)?;
                    Ok(Evidence::PendingConfirmationSynced)
                }
                Self::Confirm(append) => {
                    let confirmed = append.sync().map_err(reason)?;
                    let clear = shared
                        .lock()
                        .map_err(|_| Reason::StationBusy)?
                        .finish_pending_log_confirmation(confirmed);
                    match clear {
                        Ok(receipt) => receipt.sync().map_err(|_| Reason::PersistenceFailed)?,
                        // The append is proved. A newer local hold belongs to
                        // another action and must remain untouched.
                        Err(LogFailure::StalePending) => {}
                        Err(error) => return Err(reason(error)),
                    }
                    Ok(Evidence::FileSynced)
                }
                Self::Discard(receipt) => {
                    receipt.sync().map_err(|_| Reason::PersistenceFailed)?;
                    Ok(Evidence::PendingDiscarded)
                }
            }
        })();
        match result {
            Ok(evidence) => Outcome::Applied { evidence },
            Err(Reason::AlreadyPresent) => Outcome::Rejected {
                reason: Reason::AlreadyPresent,
            },
            Err(reason) => Outcome::Unknown { reason },
        }
    }
}

/// Station hints for log changes. Offered with the logging grant at operation v4, inside
/// `controls.capabilities`, which older hosted pages filter; `actions` never changes.
pub(super) const CAPABILITIES: [&str; 4] = ["logEdit", "qslMarks", "otaHunt", "otaActivation"];

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    call: String,
    when_unix: u64,
    key: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Change {
    Edit {
        target: Target,
        record: Box<super::ManualRecord>,
    },
    Delete {
        target: Target,
    },
    QslSent {
        target: Target,
        /// The ADIF QSL_SENT_VIA letter, or null to withdraw the mark. Only an explicit null
        /// withdraws, exactly as the desktop's `mark_qsl_sent` command rules. serde fills a
        /// MISSING `Option` field with None, which would read an omitted `via` as a withdrawal;
        /// `deserialize_with` turns that fallback off, so a missing key is a parse error.
        #[serde(deserialize_with = "Option::deserialize")]
        via: Option<String>,
    },
    QslCard {
        target: Target,
        received: bool,
    },
    /// Station context, not a row: the engine's own funnel tags the next contact logged with
    /// `call` with this reference, then clears the hunt.
    Hunt {
        call: String,
        program: String,
        reference: String,
    },
    ClearHunt {},
    /// Station context too: while an activation is on, the engine's funnel stamps your reference
    /// on every contact it logs.
    Activation {
        program: String,
        reference: String,
    },
    ClearActivation {},
}

impl Change {
    fn target(&self) -> Option<&Target> {
        match self {
            Self::Edit { target, .. }
            | Self::Delete { target }
            | Self::QslSent { target, .. }
            | Self::QslCard { target, .. } => Some(target),
            Self::Hunt { .. }
            | Self::ClearHunt {}
            | Self::Activation { .. }
            | Self::ClearActivation {} => None,
        }
    }
    /// An edit states when the contact happened; "station time" only means something for a new entry.
    pub(super) fn valid(&self, now_unix: u64) -> bool {
        self.target().is_none_or(|t| {
            !t.call.is_empty()
                && t.call.len() <= 32
                && t.when_unix <= 253_402_300_799
                && key(&t.key, 64)
        }) && match self {
            Self::Edit { record, .. } => record.when_unix.is_some() && record.valid(now_unix),
            // Exactly the menu's letters. The empty placeholder is a non-choice, never a clear.
            Self::QslSent { via, .. } => {
                via.as_deref().is_none_or(|v| ["B", "D", "E"].contains(&v))
            }
            // The wire grammar only; the engine normalizes the reference for its program.
            Self::Hunt {
                call,
                program,
                reference,
            } => {
                (3..=32).contains(&call.len())
                    && call
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
                    && ["POTA", "SOTA"].contains(&program.as_str())
                    && (1..=32).contains(&reference.len())
                    && reference
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'-')
            }
            Self::Activation { program, reference } => {
                ["POTA", "SOTA"].contains(&program.as_str())
                    && (1..=32).contains(&reference.len())
                    && reference
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'-')
            }
            Self::Delete { .. }
            | Self::QslCard { .. }
            | Self::ClearHunt {}
            | Self::ClearActivation {} => true,
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ChangeEvidence {
    FileSynced,
    StationState,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ChangeReason {
    ContextChanged,
    InvalidChange,
    PersistenceUnconfirmed,
}

#[derive(Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub(super) enum ChangeOutcome {
    Applied { evidence: ChangeEvidence },
    Rejected { reason: ChangeReason },
    Unknown { reason: ChangeReason },
}

pub(super) fn change_value(id: &str, outcome: &ChangeOutcome) -> Value {
    let mut value = serde_json::to_value(outcome)
        .unwrap_or_else(|_| json!({"outcome":"unknown","reason":"persistenceUnconfirmed"}));
    value["operation"] = json!("logChange");
    value["operationId"] = json!(id);
    value
}

/// The bytes a row key is the SHA-256 of. Mirrors `logRowCanonical` in
/// ui/src/remote-web/operation-protocol.ts; one vector in both test suites holds them together.
/// Numbers are compared in millionths, so a float and the same integer agree on both sides.
pub(super) fn row_canonical(value: &Value) -> String {
    fn walk(value: &Value, out: &mut String) {
        match value {
            Value::Null => out.push('z'),
            Value::Bool(b) => out.push(if *b { 't' } else { 'f' }),
            Value::Number(n) => {
                let x = n.as_f64().unwrap_or(0.0);
                let micro = (x.abs() * 1e6).round();
                let sign = if x < 0.0 && micro != 0.0 { "-" } else { "" };
                let _ = write!(out, "n{sign}{micro:.0};");
            }
            Value::String(s) => {
                let _ = write!(out, "s{}:{s}", s.len());
            }
            Value::Array(items) => {
                let _ = write!(out, "a{}[", items.len());
                items.iter().for_each(|item| walk(item, out));
                out.push(']');
            }
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let _ = write!(out, "o{}{{", keys.len());
                for k in keys {
                    let _ = write!(out, "s{}:{k}", k.len());
                    walk(&map[k], out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    walk(value, &mut out);
    out
}

pub(super) fn value_key(row: &Value) -> String {
    digest(&SHA256, row_canonical(row).as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The key of a stored record, built as the log page builds its row (query.rs `Collection::Log`:
/// the DTO plus the resolved entity). If that row shape changes, keys stop matching and every
/// change is refused as stale — safe, and caught by the page-row test.
pub(super) fn row_key(record: &QsoRecord) -> String {
    let mut q = tempo_app::dto::LoggedQso::from(record.clone());
    q.entity = propagation::dxcc::resolve(&q.call).map(|i| i.entity.to_string());
    serde_json::to_value(q)
        .map(|v| value_key(&v))
        .unwrap_or_default()
}

pub(super) enum ChangeWork {
    /// A proven rewrite: the file must hold exactly `count` copies of `expected` afterwards.
    Rewrite {
        path: Option<PathBuf>,
        expected: String,
        count: usize,
    },
    /// In-memory station context (a hunt), applied under the Engine lock. Nothing to sync.
    State,
}

/// Apply under the Engine lock. Only the fields a remote edit carries change; everything else
/// (confirmations, uploads, park refs, the split leg) is carried from the stored record, and the
/// engine's own edit policy still applies on top (a callsign fix clears upload stamps).
pub(super) fn prepare_change(
    engine: &mut Engine,
    change: &Change,
) -> Result<ChangeWork, ChangeReason> {
    match change {
        // The engine validates and normalizes the reference for its program; a refusal changes nothing.
        Change::Hunt {
            call,
            program,
            reference,
        } => {
            return engine
                .set_hunt_target(call, program, reference)
                .map(|()| ChangeWork::State)
                .map_err(|_| ChangeReason::InvalidChange)
        }
        Change::ClearHunt {} => {
            engine.clear_hunt_target();
            return Ok(ChangeWork::State);
        }
        Change::Activation { program, reference } => {
            return engine
                .set_activation(program, reference)
                .map(|_| ChangeWork::State)
                .map_err(|_| ChangeReason::InvalidChange)
        }
        Change::ClearActivation {} => {
            engine.clear_activation();
            return Ok(ChangeWork::State);
        }
        _ => {}
    }
    // Fold in another instance's appends first, so the index found below cannot shift under it.
    engine.sync_shared_log_if_changed();
    let t = change.target().ok_or(ChangeReason::ContextChanged)?;
    let index = engine
        .log_records()
        .iter()
        .position(|r| r.call == t.call && r.when_unix == t.when_unix && row_key(r) == t.key)
        .ok_or(ChangeReason::ContextChanged)?;
    let stored = engine.log_records()[index].clone();
    let copies = |records: &[QsoRecord], of: &QsoRecord, text: &str| {
        records
            .iter()
            .filter(|r| r.call == of.call && r.when_unix == of.when_unix && adif_record(r) == text)
            .count()
    };
    let (expected, count) = if let Change::Delete { .. } = change {
        let text = adif_record(&stored);
        if !engine.delete_qso(index) {
            return Err(ChangeReason::ContextChanged);
        }
        let count = copies(engine.log_records(), &stored, &text);
        (text, count)
    } else {
        // The row stays at `index`; prove the record the engine actually wrote there.
        let applied = match change {
            Change::Edit { record, .. } => engine.update_qso(index, edited(record, &stored)),
            // `valid` admitted only B/D/E or null, so a letter always parses here.
            Change::QslSent { via, .. } => {
                engine.mark_qsl_sent(index, via.as_deref().and_then(QslVia::from_code))
            }
            Change::QslCard { received, .. } => engine.mark_qsl_card(index, *received),
            Change::Delete { .. }
            | Change::Hunt { .. }
            | Change::ClearHunt {}
            | Change::Activation { .. }
            | Change::ClearActivation {} => false,
        };
        if !applied {
            return Err(ChangeReason::ContextChanged);
        }
        let written = engine
            .log_records()
            .get(index)
            .cloned()
            .ok_or(ChangeReason::ContextChanged)?;
        let text = adif_record(&written);
        let count = copies(engine.log_records(), &written, &text);
        (text, count)
    };
    Ok(ChangeWork::Rewrite {
        path: engine.log_path().map(Path::to_path_buf),
        expected,
        count,
    })
}

fn edited(record: &super::ManualRecord, stored: &QsoRecord) -> QsoRecord {
    let mut next = stored.clone();
    next.call = record.call.clone();
    next.grid = record.grid.clone();
    next.state = record.state.clone();
    next.band = record.band.clone();
    next.freq_mhz = record.freq_mhz;
    next.mode = record.mode.clone();
    next.rst_sent = record.rst_sent.clone();
    next.rst_rcvd = record.rst_rcvd.clone();
    next.name = record.name.clone();
    next.qth = record.qth.clone();
    next.comment = record.comment.clone();
    next.notes = record.notes.clone();
    if let Some(at) = record.when_unix.filter(|at| *at != stored.when_unix) {
        next.when_unix = at;
        next.time_known = true; // the operator stated the time
    }
    if let Some(ota) = &record.ota {
        next.ota.their_program = Some(ota.their_program.clone());
        next.ota.their_ref = Some(ota.their_ref.clone());
    }
    next
}

impl ChangeWork {
    /// Runs with Engine unlocked. Never retries and never writes the log itself.
    pub(super) fn finish(self) -> ChangeOutcome {
        let Self::Rewrite {
            path,
            expected,
            count,
        } = self
        else {
            return ChangeOutcome::Applied {
                evidence: ChangeEvidence::StationState,
            };
        };
        let proved = path
            .as_deref()
            .is_some_and(|path| on_disk(path, &expected, count).unwrap_or(false));
        if proved {
            ChangeOutcome::Applied {
                evidence: ChangeEvidence::FileSynced,
            }
        } else {
            ChangeOutcome::Unknown {
                reason: ChangeReason::PersistenceUnconfirmed,
            }
        }
    }
}

/// Read the file the log path names now, check it holds the change, then sync it and (on Unix) its
/// directory, so the rename that published it survives a crash. A write handle: Windows flushes
/// only through one. Anything short of that is unknown, never applied.
fn on_disk(path: &Path, expected: &str, count: usize) -> std::io::Result<bool> {
    use std::io::Read;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    if text.matches(expected).count() != count {
        return Ok(false);
    }
    file.sync_all()?;
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(true)
}
