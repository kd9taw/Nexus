//! Per-radio ownership while a connection is opened or temporarily leaves the
//! monitor pool. Tokens hold no mutex across I/O; their drop releases the claim.
//! All pool users must check/claim while holding the pool lock, so deciding to
//! open and removing an existing connection cannot race another pool user.
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct RadioClaims(Arc<Mutex<HashSet<u32>>>);

pub(super) struct RadioClaim {
    registry: RadioClaims,
    id: u32,
}

impl RadioClaims {
    pub(super) fn contains(&self, id: u32) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&id)
    }

    pub(super) fn try_claim(&self, id: u32) -> Option<RadioClaim> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id)
            .then(|| RadioClaim {
                registry: self.clone(),
                id,
            })
    }
}

impl Drop for RadioClaim {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_owner_per_radio_and_other_radios_remain_available() {
        let claims = RadioClaims::default();
        let other_owner = claims.clone();
        let first = claims.try_claim(1).unwrap();
        assert!(other_owner.contains(1));
        assert!(other_owner.try_claim(1).is_none());
        let second = other_owner.try_claim(2).unwrap();
        drop(first);
        assert!(!claims.contains(1));
        assert!(other_owner.try_claim(1).is_some());
        assert!(claims.contains(2));
        drop(second);
        assert!(!claims.contains(2));
    }

    #[test]
    fn concurrent_open_attempts_have_one_owner_until_it_releases() {
        let claims = RadioClaims::default();
        let rendezvous = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let claims = claims.clone();
                let rendezvous = rendezvous.clone();
                std::thread::spawn(move || {
                    rendezvous.wait();
                    let ticket = claims.try_claim(7);
                    rendezvous.wait();
                    ticket.is_some()
                })
            })
            .collect();
        let successes = handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum::<usize>();
        assert_eq!(successes, 1);
        assert!(claims.try_claim(7).is_some());
    }

    #[test]
    fn failed_owner_and_poisoned_metadata_release_without_losing_other_claims() {
        let claims = RadioClaims::default();
        let retained = claims.try_claim(1).unwrap();
        let panicking = claims.clone();
        assert!(std::thread::spawn(move || {
            let _ticket = panicking.try_claim(2).unwrap();
            panic!("failed opener");
        })
        .join()
        .is_err());
        assert!(claims.contains(1));
        assert!(!claims.contains(2));
        let poison = claims.clone();
        assert!(std::thread::spawn(move || {
            let _lock = poison.0.lock().unwrap();
            panic!("poison claim metadata");
        })
        .join()
        .is_err());
        assert!(claims.0.is_poisoned());
        assert!(claims.try_claim(1).is_none());
        assert!(claims.try_claim(2).is_some());
        drop(retained);
        assert!(claims.try_claim(1).is_some());
    }
}
