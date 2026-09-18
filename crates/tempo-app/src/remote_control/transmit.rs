//! A separate process-local authority for transmitting. Receiver/CAT write
//! permits cannot be converted into this type. Only the native host may issue
//! it after its distinct local transmit grant and controller checks.
use super::{Permit, Revocation};
use std::sync::Arc;
use std::time::Instant;

#[derive(Default)]
pub struct TransmitAuthority(Revocation);

impl TransmitAuthority {
    pub fn revoke(&self) {
        self.0.revoke();
    }

    pub fn generation(&self) -> u64 {
        self.0.generation()
    }

    /// A handle on this authority's revocation, for the ENGINE to hold.
    ///
    /// The generation behind it is what a browser is shown as its `transmitEpoch`, and what
    /// `permit_generation` checks an arming gesture against. `Engine::halt_tx` is the one verb
    /// every local stand-down funnels through — the desktop's own Stop TX, WSJT-X's UDP HaltTx,
    /// a broker client — and each of those must retire remote transmit authority the way Remote's
    /// own Stop already does. Handing the engine this clone is what lets it, and it is an `Arc`
    /// inside, so both sides move the same counter.
    ///
    /// ⛔ It revokes; it cannot issue. A permit still comes only from the native host after its
    /// own local transmit-grant and controller checks.
    pub fn revocation(&self) -> super::Revocation {
        self.0.clone()
    }

    /// A delayed Stop can retire only the generation it displayed. Atomic
    /// comparison prevents it racing a newer controller or transmission.
    pub fn revoke_generation(&self, expected: u64) -> bool {
        use std::sync::atomic::Ordering;
        expected != u64::MAX
            && self
                .0
                 .0
                .compare_exchange(expected, expected + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }

    /// Bind an arming gesture to the generation the browser displayed. A Stop
    /// racing issuance invalidates the returned permit before Engine admission.
    pub fn permit_generation(&self, expected: u64, deadline: Instant) -> Option<TransmitPermit> {
        let permit = self.permit(deadline)?;
        (permit.0.generation == expected && permit.valid(Instant::now())).then_some(permit)
    }

    pub fn permit(&self, deadline: Instant) -> Option<TransmitPermit> {
        self.0.permit(deadline).map(TransmitPermit)
    }
}

#[derive(Clone)]
pub struct TransmitPermit(Permit);

impl TransmitPermit {
    pub fn valid(&self, now: Instant) -> bool {
        self.0.valid(now)
    }

    /// Identity only; callers still check both deadlines before admission.
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0.owner, &other.0.owner) && self.0.generation == other.0.generation
    }

    /// Renewal cannot resurrect an expired/revoked session or transfer it to
    /// another controller. The host must already have admitted the heartbeat.
    pub fn renew(&mut self, next: Self, now: Instant) -> bool {
        if !self.valid(now)
            || !next.valid(now)
            || !self.same_session(&next)
            || next.0.deadline < self.0.deadline
        {
            return false;
        }
        *self = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn renewal_requires_the_live_original_authority_and_never_revives_expiry() {
        let now = Instant::now();
        let authority = TransmitAuthority::default();
        let other = TransmitAuthority::default();
        let mut permit = authority.permit(now + Duration::from_secs(5)).unwrap();
        assert!(!permit.renew(other.permit(now + Duration::from_secs(8)).unwrap(), now));
        assert!(!permit.renew(authority.permit(now + Duration::from_secs(4)).unwrap(), now));
        assert!(permit.renew(authority.permit(now + Duration::from_secs(8)).unwrap(), now));
        assert!(permit.valid(now + Duration::from_secs(7)));
        assert!(!permit.renew(
            authority.permit(now + Duration::from_secs(12)).unwrap(),
            now + Duration::from_secs(8)
        ));
        assert!(!permit.valid(now + Duration::from_secs(8)));
        let mut permit = authority.permit(now + Duration::from_secs(10)).unwrap();
        authority.revoke();
        assert!(!permit.valid(now));
        assert!(!permit.renew(
            authority.permit(now + Duration::from_secs(12)).unwrap(),
            now
        ));
    }
}
