//! Process-local permission and completion for station-owned Remote operations.
//!
//! A cloud acknowledgement cannot grant this permission. The native session
//! issues a short-lived permit; a hardware worker rechecks it at each write.
//! The fixed execution deadline never extends when the controller heartbeats.
//! Completion is separate from admission, and an uncertain write is never retried.

use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Instant;

pub mod amplifier;

#[derive(Default)]
pub struct Revocation(Arc<AtomicU64>);

impl Revocation {
    pub fn revoke(&self) {
        // Exhaustion permanently refuses new permits rather than wrapping into
        // an earlier controller's generation.
        let _ = self
            .0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                Some(n.saturating_add(1))
            });
    }

    pub fn permit(&self, deadline: Instant) -> Option<Permit> {
        let generation = self.0.load(Ordering::SeqCst);
        (generation != u64::MAX).then(|| Permit {
            owner: self.0.clone(),
            generation,
            deadline,
        })
    }
}

#[derive(Clone)]
pub struct Permit {
    owner: Arc<AtomicU64>,
    generation: u64,
    deadline: Instant,
}

/// Permission carried to the last byte-write boundary of a CAT operation.
/// Both browser authority and the native station context must still match.
/// This is never a permission to key a transmitter.
pub struct WritePermission {
    authority: Permit,
    native: Permit,
    completion: Completion,
}

impl WritePermission {
    pub fn new(authority: Permit, native: Permit, completion: Completion) -> Self {
        Self {
            authority,
            native,
            completion,
        }
    }

    pub fn check(&self, now: Instant) -> Result<(), Reason> {
        let reason = if !self.authority.valid(now) {
            Some(Reason::AuthorityExpired)
        } else if !self.native.valid(now) {
            Some(Reason::ContextChanged)
        } else if !matches!(self.completion.outcome(), Outcome::Pending) {
            Some(Reason::HardwareUnconfirmed)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.completion.refuse(reason);
            return Err(reason);
        }
        Ok(())
    }

    pub fn begin_write(&self, now: Instant) -> Result<(), Reason> {
        self.check(now)?;
        self.completion
            .begin_write(now)
            .then_some(())
            .ok_or(Reason::HardwareUnconfirmed)
    }
}

impl Permit {
    pub fn valid(&self, now: Instant) -> bool {
        now < self.deadline && self.owner.load(Ordering::SeqCst) == self.generation
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Outcome {
    Pending,
    Applied { evidence: Evidence },
    Rejected { reason: Reason },
    Unknown { reason: Reason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Evidence {
    ReceiverState,
    StationState,
    RadioReadback,
    AmplifierReadback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    AuthorityExpired,
    ContextChanged,
    ReadingUnavailable,
    StationBusy,
    HardwareUnavailable,
    HardwareUnconfirmed,
    UnsupportedAction,
    InvalidAction,
}

/// A bounded receipt shared with the worker, not a second command queue.
/// Once terminal it cannot be promoted to success by a late matching reading.
#[derive(Clone)]
pub struct Completion(Arc<Mutex<Progress>>);

struct Progress {
    outcome: Outcome,
    permit: Option<Permit>,
    attempted: bool,
}

impl Progress {
    fn expire(&mut self, now: Instant) {
        if matches!(self.outcome, Outcome::Pending)
            && self.permit.as_ref().is_some_and(|p| !p.valid(now))
        {
            self.outcome = if self.attempted {
                Outcome::Unknown {
                    reason: Reason::HardwareUnconfirmed,
                }
            } else {
                Outcome::Rejected {
                    reason: Reason::AuthorityExpired,
                }
            };
        }
    }
}

impl Default for Completion {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Progress {
            outcome: Outcome::Pending,
            permit: None,
            attempted: false,
        })))
    }
}

impl Completion {
    pub fn guarded(permit: Permit) -> Self {
        Self(Arc::new(Mutex::new(Progress {
            outcome: Outcome::Pending,
            permit: Some(permit),
            attempted: false,
        })))
    }

    /// Call at the write boundary, after validating the current hardware binding.
    /// Returning false forbids the write, including after an earlier terminal result.
    pub fn begin_write(&self, now: Instant) -> bool {
        let Ok(mut progress) = self.0.lock() else {
            return false;
        };
        progress.expire(now);
        if !matches!(progress.outcome, Outcome::Pending) {
            return false;
        }
        progress.attempted = true;
        true
    }

    pub fn outcome(&self) -> Outcome {
        self.0.lock().map_or(
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed,
            },
            |mut value| {
                value.expire(Instant::now());
                value.outcome.clone()
            },
        )
    }

    pub fn finish(&self, outcome: Outcome) {
        if let Ok(mut value) = self.0.lock() {
            value.expire(Instant::now());
            if matches!(value.outcome, Outcome::Pending) && !matches!(outcome, Outcome::Pending) {
                value.outcome = outcome;
            }
        }
    }

    /// A failure after any attempted write is uncertain, even if a later write
    /// was refused before reaching the wire. Never erase that first attempt.
    pub fn refuse(&self, reason: Reason) {
        if let Ok(mut value) = self.0.lock() {
            value.expire(Instant::now());
            if matches!(value.outcome, Outcome::Pending) {
                value.outcome = if value.attempted {
                    Outcome::Unknown {
                        reason: Reason::HardwareUnconfirmed,
                    }
                } else {
                    Outcome::Rejected { reason }
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_lost_worker_cannot_leave_a_pending_operation_blocking_the_station_forever() {
        let authority = Revocation::default();
        let now = Instant::now();
        let before_wire =
            Completion::guarded(authority.permit(now + Duration::from_secs(2)).unwrap());
        let after_wire =
            Completion::guarded(authority.permit(now + Duration::from_secs(2)).unwrap());
        assert!(after_wire.begin_write(now));
        authority.revoke();
        assert!(!before_wire.begin_write(now));
        assert_eq!(
            before_wire.outcome(),
            Outcome::Rejected {
                reason: Reason::AuthorityExpired
            }
        );
        assert_eq!(
            after_wire.outcome(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed
            }
        );
        after_wire.finish(Outcome::Applied {
            evidence: Evidence::RadioReadback,
        });
        assert_eq!(
            after_wire.outcome(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed
            }
        );
    }

    #[test]
    fn revocation_reaches_a_worker_that_already_took_its_command() {
        let authority = Revocation::default();
        let now = Instant::now();
        let permit = authority.permit(now + Duration::from_secs(2)).unwrap();
        let in_worker = permit.clone();
        assert!(in_worker.valid(now));
        authority.revoke();
        assert!(!in_worker.valid(now));
        assert!(authority
            .permit(now + Duration::from_secs(2))
            .unwrap()
            .valid(now));
    }

    #[test]
    fn the_write_deadline_is_fixed_and_exclusive() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(2);
        let permit = Revocation::default().permit(deadline).unwrap();
        assert!(permit.valid(deadline - Duration::from_nanos(1)));
        assert!(!permit.valid(deadline));
    }

    #[test]
    fn late_readback_cannot_rewrite_an_uncertain_outcome() {
        let completion = Completion::default();
        assert_eq!(completion.outcome(), Outcome::Pending);
        let unknown = Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed,
        };
        completion.finish(unknown.clone());
        completion.finish(Outcome::Applied {
            evidence: Evidence::AmplifierReadback,
        });
        assert_eq!(completion.outcome(), unknown);
        let positive = Completion::default();
        positive.finish(Outcome::Applied {
            evidence: Evidence::AmplifierReadback,
        });
        assert_eq!(
            positive.outcome(),
            Outcome::Applied {
                evidence: Evidence::AmplifierReadback
            }
        );
    }
}
