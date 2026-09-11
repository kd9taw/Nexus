//! QSO actions use the logging grant, shared native policy and durable receipts.
//! Admission happens under the original controller window. Finishing an append
//! cannot acquire TX authority, retry a write or clear a replacement contact.
use super::station::Action;
use tempo_app::engine::{
    remote_logging::{
        CurrentQsoLogOutcome, JournalSync, LogFailure, PendingJournalWrite, PendingLogConfirmation,
    },
    remote_transmit::FtExchangeContext,
    Engine, LogWriteOutcome,
};
use tempo_app::remote_control::{Evidence, Outcome, Reason};

pub(super) enum Work {
    Append(LogWriteOutcome),
    Pending(PendingJournalWrite),
    Confirm(PendingLogConfirmation),
    Discard(JournalSync),
}

fn key(value: &str) -> bool {
    value.len() == 16
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
            if !key(expected_key) {
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
            if !key(expected_key) {
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
            if !key(expected_key) {
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
