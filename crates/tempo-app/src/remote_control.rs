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
pub mod transmit;

#[derive(Default, Clone)]
pub struct Revocation(Arc<AtomicU64>);

impl Revocation {
    pub fn generation(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

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

/// Does the browser that STARTED a piece of standing station work still hold authority?
///
/// A [`Permit`] cannot answer that: it carries an execution deadline of a few seconds, because it
/// admits one bounded hardware write. Some station work is started by a browser gesture and then
/// runs at the station for minutes on its own — the satellite track loop, which steers the dial
/// and the mast until LOS. That work has to end when the browser that asked for it goes away, and
/// "goes away" is exactly what this generation counts: the lease expiring or being released, the
/// socket closing, the operator taking the station back or revoking the browser all move it.
///
/// It is NOT a permission. It admits nothing and grants nothing; it only answers `held()`. What
/// the work is allowed to do was decided when the gesture was admitted.
#[derive(Clone)]
pub struct Standing {
    owner: Arc<AtomicU64>,
    generation: u64,
}

impl Revocation {
    /// A standing check against this authority's CURRENT generation.
    pub fn standing(&self) -> Standing {
        Standing {
            owner: self.0.clone(),
            generation: self.0.load(Ordering::SeqCst),
        }
    }
}

impl Standing {
    /// True while the authority that issued this is still the live one. Exhaustion
    /// (`u64::MAX`, which permanently refuses new permits) reads as not held.
    pub fn held(&self) -> bool {
        let now = self.owner.load(Ordering::SeqCst);
        now != u64::MAX && now == self.generation
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

    /// Stop a bounded hardware transaction. Any earlier attempted write makes
    /// its result unknown; a refused follow-up cannot erase that attempt.
    pub fn refuse(&self, reason: Reason) {
        self.completion.refuse(reason);
    }
}

impl Permit {
    pub fn valid(&self, now: Instant) -> bool {
        now < self.deadline && self.owner.load(Ordering::SeqCst) == self.generation
    }

    /// The deadline-free half of this permit, for work the gesture STARTS and the station then
    /// runs on its own. See [`Standing`] — it carries no permission, only the authority generation
    /// this gesture was admitted under.
    pub fn standing(&self) -> Standing {
        Standing {
            owner: self.owner.clone(),
            generation: self.generation,
        }
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
    SettingsSaved,
    FileSynced,
    PendingConfirmationSynced,
    PendingDiscarded,
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
    PersistenceFailed,
    NoEligibleContact,
    AlreadyPresent,
    PendingConfirmationRequired,
    /// The requested transmit frequency is outside the operator's licence privileges.
    OutsidePrivileges,
}

/// A bounded receipt shared with the worker, not a second command queue.
/// Once terminal it cannot be promoted to success by a late matching reading.
#[derive(Clone)]
pub struct Completion(Arc<Mutex<Progress>>);

/// Told once, when the outcome stops being pending. Never load-bearing: a reader that polls
/// `outcome` learns the same thing, so a waker that is dropped or never installed costs a
/// browser a poll, not an outcome.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

struct Progress {
    outcome: Outcome,
    permit: Option<Permit>,
    attempted: bool,
    waker: Option<Waker>,
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
            waker: None,
        })))
    }
}

impl Completion {
    pub fn guarded(permit: Permit) -> Self {
        Self(Arc::new(Mutex::new(Progress {
            outcome: Outcome::Pending,
            permit: Some(permit),
            attempted: false,
            waker: None,
        })))
    }

    /// Every path that can settle the outcome goes through here, so the waker fires exactly once,
    /// on the first transition out of `Pending` — whichever of finish, refuse or a lazy expiry
    /// makes it — and after the lock is released, so a waker may take any lock it likes.
    fn settle<T>(&self, now: Instant, f: impl FnOnce(&mut Progress) -> T, fallback: T) -> T {
        let Ok(mut progress) = self.0.lock() else {
            return fallback;
        };
        progress.expire(now);
        let result = f(&mut progress);
        let woken = if matches!(progress.outcome, Outcome::Pending) {
            None
        } else {
            progress.waker.take()
        };
        drop(progress);
        if let Some(wake) = woken {
            wake();
        }
        result
    }

    /// Ask to be told when this settles. An outcome already terminal is told at once — the
    /// caller registering late (its reply built before the worker finished) must not wait for
    /// a second transition that will never come.
    pub fn on_finish(&self, waker: Waker) {
        // Installed unconditionally: `settle` fires it on the way out if the outcome is
        // already terminal, and keeps it for the transition otherwise.
        self.settle(Instant::now(), |progress| progress.waker = Some(waker), ())
    }

    /// Call at the write boundary, after validating the current hardware binding.
    /// Returning false forbids the write, including after an earlier terminal result.
    pub fn begin_write(&self, now: Instant) -> bool {
        self.settle(
            now,
            |progress| {
                if !matches!(progress.outcome, Outcome::Pending) {
                    return false;
                }
                progress.attempted = true;
                true
            },
            false,
        )
    }

    pub fn outcome(&self) -> Outcome {
        self.settle(
            Instant::now(),
            |value| value.outcome.clone(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed,
            },
        )
    }

    pub fn finish(&self, outcome: Outcome) {
        self.settle(
            Instant::now(),
            |value| {
                if matches!(value.outcome, Outcome::Pending) && !matches!(outcome, Outcome::Pending)
                {
                    value.outcome = outcome;
                }
            },
            (),
        )
    }

    /// A failure after any attempted write is uncertain, even if a later write
    /// was refused before reaching the wire. Never erase that first attempt.
    pub fn refuse(&self, reason: Reason) {
        self.settle(
            Instant::now(),
            |value| {
                if matches!(value.outcome, Outcome::Pending) {
                    value.outcome = if value.attempted {
                        Outcome::Unknown {
                            reason: Reason::HardwareUnconfirmed,
                        }
                    } else {
                        Outcome::Rejected { reason }
                    };
                }
            },
            (),
        )
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
    fn the_waker_fires_exactly_once_on_the_first_settlement_however_it_happens() {
        use std::sync::atomic::AtomicUsize;
        let counter = || Arc::new(AtomicUsize::new(0));
        let waker = |count: &Arc<AtomicUsize>| -> Waker {
            let count = count.clone();
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })
        };
        // Finished after registration: told once, and a second finish tells nobody.
        let finished = Completion::default();
        let told = counter();
        finished.on_finish(waker(&told));
        assert_eq!(told.load(Ordering::SeqCst), 0, "pending: nothing to tell");
        finished.finish(Outcome::Applied {
            evidence: Evidence::RadioReadback,
        });
        finished.finish(Outcome::Rejected {
            reason: Reason::StationBusy,
        });
        finished.refuse(Reason::StationBusy);
        assert_eq!(told.load(Ordering::SeqCst), 1);
        // Registered late, after the worker had already finished: told at once.
        let early = Completion::default();
        early.finish(Outcome::Applied {
            evidence: Evidence::StationState,
        });
        let told = counter();
        early.on_finish(waker(&told));
        assert_eq!(told.load(Ordering::SeqCst), 1);
        // Settled by a lazy expiry (nobody called finish): the read that notices it tells.
        let authority = Revocation::default();
        let now = Instant::now();
        let expired =
            Completion::guarded(authority.permit(now + Duration::from_millis(1)).unwrap());
        let told = counter();
        expired.on_finish(waker(&told));
        assert_eq!(told.load(Ordering::SeqCst), 0);
        std::thread::sleep(Duration::from_millis(5));
        assert!(matches!(expired.outcome(), Outcome::Rejected { .. }));
        assert_eq!(told.load(Ordering::SeqCst), 1);
        assert!(matches!(expired.outcome(), Outcome::Rejected { .. }));
        assert_eq!(told.load(Ordering::SeqCst), 1, "a second read tells nobody");
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
