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
/// `activationExport` is the one-activation ADIF read (see `export.rs`), under the same grant, and
/// `settingsLogging` the logging preferences (see `settings.rs`).
pub(super) const CAPABILITIES: [&str; 6] = [
    "logEdit",
    "qslMarks",
    "otaHunt",
    "otaActivation",
    "activationExport",
    "settingsLogging",
];

/// ⛔ Self-spot posts a PUBLIC spot of the station's own call to pota.app and to the station's DX
/// cluster (policy in `crate::self_spot`). On since the operator signed it off (2026-09-14; both
/// targets 2026-09-14), behind a confirm on every click. Setting this to false makes the station
/// neither advertise nor accept it again.
pub(super) const SELF_SPOT: bool = true;

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
    /// A public spot of the station's own call, on its current dial and mode, naming its activation,
    /// to pota.app and the DX cluster. Carries what the operator's confirm showed, so it is refused
    /// if either has moved since.
    SelfSpot {
        reference: String,
        // `rename_all` above renames variants only, never the fields inside them.
        #[serde(rename = "dialHz")]
        dial_hz: u64,
    },
    /// ⛔ A public DX cluster spot of ANOTHER station: exactly the desktop Spot dialog's three
    /// fields, posted from this station's cluster login. Station control only (see `grants`), and
    /// confirmed on every click at the browser. Nothing here reads or writes the log.
    Spot {
        call: String,
        // `rename_all` above renames variants only, never the fields inside them.
        #[serde(rename = "freqMhz")]
        freq_mhz: f64,
        comment: String,
    },
    /// Operating preferences from the station's allow-list (see `settings.rs`), against the Settings
    /// document revision the browser showed.
    Settings {
        revision: String,
        values: serde_json::Map<String, Value>,
    },
    /// Curating the working channel list (see `program_edit.rs`), against the `programming`
    /// document revision the browser showed. Nothing here reads or moves the radio.
    ProgramEdit {
        revision: String,
        edit: super::program_edit::Edit,
    },
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
            | Self::ClearActivation {}
            | Self::SelfSpot { .. }
            | Self::Spot { .. }
            | Self::Settings { .. }
            | Self::ProgramEdit { .. } => None,
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
            Self::SelfSpot { reference, dial_hz } => {
                (1..=250_000_000_000).contains(dial_hz)
                    && (1..=32).contains(&reference.len())
                    && reference
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'-')
            }
            // The wire grammar only. `crate::post_spot` judges the callsign by the cluster's own
            // rule and refuses when no node is connected; neither is second-guessed here.
            Self::Spot {
                call,
                freq_mhz,
                comment,
            } => {
                (3..=32).contains(&call.len())
                    && call
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
                    && freq_mhz.is_finite()
                    && *freq_mhz > 0.0
                    && *freq_mhz <= 250_000.0
                    // The Spot dialog's own comment field length, printable ASCII only: the line
                    // goes to a public cluster verbatim.
                    && comment.len() <= 30
                    && comment.bytes().all(|b| (0x20..=0x7e).contains(&b))
            }
            Self::Settings { revision, values } => {
                key(revision, 64)
                    && (1..=32).contains(&values.len())
                    && super::settings::grants(values).is_some()
            }
            Self::ProgramEdit { revision, edit } => key(revision, 64) && edit.valid(),
            Self::Delete { .. }
            | Self::QslCard { .. }
            | Self::ClearHunt {}
            | Self::ClearActivation {} => true,
        }
    }
    /// The local grants this change needs, as (logging grant, station control). A log change needs
    /// the logging grant; a preference change needs what its keys need, and the strictest answer for
    /// a key off the allow-list, which `valid` refuses anyway.
    pub(super) fn grants(&self) -> (bool, bool) {
        match self {
            Self::Settings { values, .. } => {
                super::settings::grants(values).unwrap_or((true, true))
            }
            // A public spot of someone else is not a log change: it posts from this station's
            // cluster login, so it needs station control and not the logging grant.
            Self::Spot { .. } => (false, true),
            // Curating the channel list is not a log change either: it writes the station's own
            // programming file, so it is station control and never the logging grant.
            Self::ProgramEdit { .. } => (false, true),
            _ => (true, false),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ChangeEvidence {
    FileSynced,
    StationState,
    /// At least one target took the self-spot; the outcome's `spot` says what each did.
    SpotPosted,
    /// A spot of another station reached the station's DX cluster queue, which sends it.
    ClusterQueued,
    /// Operating preferences saved to the station's settings file, then published.
    SettingsSaved,
    /// The working channel list saved to the station's programming file.
    ProgramSaved,
}

#[derive(Clone, Copy, Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
#[serde(rename_all = "camelCase")]
pub(super) enum ChangeReason {
    ContextChanged,
    InvalidChange,
    /// Neither target took the self-spot; the outcome's `spot` says why, per target.
    SpotNotPosted,
    /// No DX cluster node is connected at the station, so nothing was queued and nothing sent.
    ClusterUnavailable,
    PersistenceUnconfirmed,
}

/// A self-spot's outcome carries `spot`, both targets' results, and so does its receipt: a replay
/// answers with the same two results and posts nothing. Every other outcome has no `spot` key.
#[derive(Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub(super) enum ChangeOutcome {
    Applied {
        evidence: ChangeEvidence,
        #[serde(skip_serializing_if = "Option::is_none")]
        spot: Option<crate::self_spot::Report>,
    },
    Rejected {
        reason: ChangeReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        spot: Option<crate::self_spot::Report>,
    },
    Unknown {
        reason: ChangeReason,
    },
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

/// The identity of a row the SHACK's log view showed: the row itself, exactly as `get_log`
/// handed it out. The station keys it the way a browser keys its page row — the same canonical
/// bytes, the same SHA-256 — and [`locate`] then finds the record whose OWN key matches, so
/// the row is a claim to be matched, never an address to trust. The view computes no hash (the
/// desktop webview has no proven `crypto.subtle`), and `get_log` hands out no keys: keying a
/// whole log on every reload would cost a serialise and a digest per record under the engine
/// lock, where keying one row on one click costs nothing anyone can notice.
pub(crate) fn seen_target(seen: &tempo_app::dto::LoggedQso) -> Target {
    Target {
        call: seen.call.clone(),
        when_unix: seen.when_unix,
        key: serde_json::to_value(seen)
            .map(|v| value_key(&v))
            .unwrap_or_default(),
    }
}

/// The position TODAY of the row a writer saw — the browser's log page or the shack's log
/// view — or `None` when no row holds exactly that content any more. This is the one way
/// either writer turns a row into an index: a position kept from an earlier read is stale the
/// moment the OTHER writer deletes above it, and acting on it deleted or rewrote a different
/// contact. Another instance's appends are folded in first so the index cannot shift under
/// the caller.
pub(crate) fn locate(engine: &mut Engine, target: &Target) -> Option<usize> {
    engine.sync_shared_log_if_changed();
    engine.log_records().iter().position(|r| {
        r.call == target.call && r.when_unix == target.when_unix && row_key(r) == target.key
    })
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
    /// A self-spot read from the station under the Engine lock, posted only after it is released.
    Spot(crate::self_spot::Context),
    /// A spot of another station, posted only after the Engine lock is released. Carries exactly
    /// what the operator confirmed; the station resolves nothing further.
    ClusterSpot {
        call: String,
        freq_mhz: f64,
        comment: String,
    },
    /// Operating preferences saved atomically under the Engine lock, and published.
    SettingsSaved,
    /// A preference save that did not complete: nothing was published, and what the file holds is
    /// unknown.
    SettingsUnconfirmed,
    /// The working channel list written to the station's programming file.
    ProgramSaved,
    /// A channel-list write that did not complete. A save that failed part way cannot say what the
    /// file holds, so the browser is told "unknown" and re-reads rather than being told it worked.
    ProgramUnconfirmed,
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
        Change::SelfSpot { reference, dial_hz } => {
            // What the operator confirmed must still be true: the same activation, the same dial.
            return crate::self_spot::Context::confirmed(engine, reference, *dial_hz)
                .map(ChangeWork::Spot)
                .map_err(|refusal| match refusal {
                    crate::self_spot::Refusal::NoActivation => ChangeReason::InvalidChange,
                    crate::self_spot::Refusal::Moved => ChangeReason::ContextChanged,
                });
        }
        Change::Spot {
            call,
            freq_mhz,
            comment,
        } => {
            // Nothing is read from the station: a spot of another station is the operator's own
            // three fields. Uppercased here exactly as the desktop dialog uppercases them.
            return Ok(ChangeWork::ClusterSpot {
                call: call.to_ascii_uppercase(),
                freq_mhz: *freq_mhz,
                comment: comment.clone(),
            });
        }
        Change::Settings { revision, values } => {
            return super::settings::prepare(engine, revision, values)
        }
        // The programming file, not the log and not the radio: the Engine is not consulted at all.
        Change::ProgramEdit { revision, edit } => {
            return super::program_edit::prepare(&crate::radioprog_path(), revision, edit)
        }
        _ => {}
    }
    let t = change.target().ok_or(ChangeReason::ContextChanged)?;
    let index = locate(engine, t).ok_or(ChangeReason::ContextChanged)?;
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
            | Change::ClearActivation {}
            | Change::SelfSpot { .. }
            | Change::Spot { .. }
            | Change::Settings { .. }
            | Change::ProgramEdit { .. } => false,
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
    /// `spot` is used only by a self-spot and `cluster` only by a spot of another station, each
    /// only here and once per receipt.
    pub(super) fn finish(
        self,
        spot: impl FnOnce(&crate::self_spot::Context) -> crate::self_spot::Report,
        cluster: impl FnOnce(f64, &str, &str) -> Result<(), String>,
    ) -> ChangeOutcome {
        let (path, expected, count) = match self {
            Self::Rewrite {
                path,
                expected,
                count,
            } => (path, expected, count),
            Self::State => {
                return ChangeOutcome::Applied {
                    evidence: ChangeEvidence::StationState,
                    spot: None,
                }
            }
            Self::SettingsSaved => {
                return ChangeOutcome::Applied {
                    evidence: ChangeEvidence::SettingsSaved,
                    spot: None,
                }
            }
            Self::SettingsUnconfirmed | Self::ProgramUnconfirmed => {
                return ChangeOutcome::Unknown {
                    reason: ChangeReason::PersistenceUnconfirmed,
                }
            }
            Self::ProgramSaved => {
                return ChangeOutcome::Applied {
                    evidence: ChangeEvidence::ProgramSaved,
                    spot: None,
                }
            }
            Self::ClusterSpot {
                call,
                freq_mhz,
                comment,
            } => {
                return match cluster(freq_mhz, &call, &comment) {
                    Ok(()) => ChangeOutcome::Applied {
                        evidence: ChangeEvidence::ClusterQueued,
                        spot: None,
                    },
                    // The station's own verb names the cluster only when no node is connected;
                    // anything else is the callsign or the frequency being refused.
                    Err(e) if e.contains("cluster") => ChangeOutcome::Rejected {
                        reason: ChangeReason::ClusterUnavailable,
                        spot: None,
                    },
                    Err(_) => ChangeOutcome::Rejected {
                        reason: ChangeReason::InvalidChange,
                        spot: None,
                    },
                };
            }
            Self::Spot(context) => {
                let report = spot(&context);
                return if report.any_posted() {
                    ChangeOutcome::Applied {
                        evidence: ChangeEvidence::SpotPosted,
                        spot: Some(report),
                    }
                } else {
                    ChangeOutcome::Rejected {
                        reason: ChangeReason::SpotNotPosted,
                        spot: Some(report),
                    }
                };
            }
        };
        let proved = path
            .as_deref()
            .is_some_and(|path| on_disk(path, &expected, count).unwrap_or(false));
        if proved {
            ChangeOutcome::Applied {
                evidence: ChangeEvidence::FileSynced,
                spot: None,
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
