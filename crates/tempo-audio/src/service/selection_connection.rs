//! Temporary ownership of an incoming radio during a Remote selection.
//! The connection is returned read-only on cancellation. Successful adoption
//! must retain the claim until the active owner has installed that connection.
use super::monitor_claims::RadioClaim;
use super::{CatDaemon, MonitorConn, MonitorPool, PttMode, Rig, Transport};
use std::time::Instant;
use tempo_app::remote_control::{Reason, WritePermission};

pub(super) struct SelectionConnection {
    pool: MonitorPool,
    claim: Option<RadioClaim>,
    connection: Option<MonitorConn>,
}

impl SelectionConnection {
    /// Called by the active owner, with its native context already checked.
    /// No Engine or pool mutex is held while opening a cold connection. The
    /// opener must be the same station-resolved read-only opener as monitoring.
    pub(super) fn acquire(
        pool: &MonitorPool,
        id: u32,
        transport: Transport,
        permission: &WritePermission,
        mut open: impl FnMut(&Transport) -> (Rig, Option<CatDaemon>, Option<bool>),
    ) -> Result<Self, Reason> {
        permission.check(Instant::now())?;
        let (claim, connection) = {
            let mut connections = match pool.try_lock() {
                Ok(guard) => guard,
                Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return Err(Reason::StationBusy),
            };
            let claim = pool.claims.try_claim(id).ok_or(Reason::StationBusy)?;
            let connection = connections
                .iter()
                .position(|connection| connection.id == id)
                .map(|index| connections.remove(index));
            (claim, connection)
        };
        let mut lease = Self {
            pool: pool.clone(),
            claim: Some(claim),
            connection,
        };
        permission.check(Instant::now())?;
        let reusable = lease.connection.as_mut().is_some_and(|connection| {
            !connection.transport.rig_differs(&transport)
                && connection.rig.has_control()
                && connection
                    .rigctld_proc
                    .as_mut()
                    .is_none_or(CatDaemon::is_alive)
        });
        if !reusable {
            // Finish stale daemon teardown before opening its replacement.
            // A failed attempt stays a monitoring failure, never a queued retune.
            let prior_failures = lease
                .connection
                .as_ref()
                .filter(|connection| !connection.transport.rig_differs(&transport))
                .map_or(0, |connection| connection.open_failures);
            drop(lease.connection.take());
            permission.check(Instant::now())?;
            let (rig, daemon, _) = open(&transport);
            let (failures, retry_after_ms) =
                MonitorConn::open_backoff(rig.has_control(), prior_failures, super::now_unix_ms());
            lease.connection = Some(MonitorConn {
                id,
                transport,
                rig,
                rigctld_proc: daemon,
                last_poll: 0.0,
                ticks: 0,
                smeter_supported: None,
                freq_misses: 0,
                open_failures: failures,
                retry_after_ms,
            });
        }
        permission.check(Instant::now())?;
        if !lease
            .connection
            .as_ref()
            .expect("opened or reused")
            .rig
            .has_control()
        {
            return Err(Reason::HardwareUnavailable);
        }
        Ok(lease)
    }

    pub(super) fn rig(&mut self) -> &mut Rig {
        &mut self.connection.as_mut().expect("owned connection").rig
    }

    /// The caller holds the pool lock while installing the active slot, then
    /// releases the returned claim. Dropping this lease alone returns the radio
    /// to monitoring; only this consuming operation transfers its ownership.
    pub(super) fn adopt(mut self) -> (MonitorConn, RadioClaim) {
        (
            self.connection.take().expect("owned connection"),
            self.claim.take().expect("owned claim"),
        )
    }
}

impl Drop for SelectionConnection {
    fn drop(&mut self) {
        if let Some(mut connection) = self.connection.take() {
            connection.rig.set_ptt_mode(PttMode::Vox);
            let mut connections = self.pool.lock().unwrap_or_else(|error| error.into_inner());
            // The held claim excludes reconciliation and local adoption. Keep
            // the existing entry if a future caller violates that ownership.
            if !connections.iter().any(|other| other.id == connection.id) {
                connections.push(connection);
            }
        }
        // Field drop releases the claim after the connection is back in the pool.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::MonitorConnections;
    use std::sync::Arc;
    use std::time::Duration;
    use tempo_app::remote_control::{Completion, Revocation};

    fn permission(authority: &Revocation) -> WritePermission {
        let native = Revocation::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        let permit = authority.permit(deadline).unwrap();
        WritePermission::new(
            permit.clone(),
            native.permit(deadline).unwrap(),
            Completion::guarded(permit),
        )
    }

    fn transport() -> Transport {
        Transport::from_settings(&tempo_app::settings::Settings::default())
    }

    fn controlled() -> (Rig, Option<CatDaemon>, Option<bool>) {
        // Constructing this channel performs no network access. These tests
        // exercise ownership, not a simulated claim of physical CAT readback.
        (
            Rig::with_control(Some("127.0.0.1:1".into()), PttMode::Vox),
            None,
            Some(true),
        )
    }

    #[test]
    fn pooled_connection_returns_on_cancellation_and_adoption_retains_its_claim() {
        let pool = Arc::new(MonitorConnections::new(Vec::new()));
        let authority = Revocation::default();
        let allowed = permission(&authority);
        let t = transport();
        let cold =
            SelectionConnection::acquire(&pool, 1, t.clone(), &allowed, |_| controlled()).unwrap();
        assert!(pool.claims.contains(1));
        assert!(pool.lock().unwrap().is_empty());
        drop(cold);
        assert!(!pool.claims.contains(1));
        assert_eq!(pool.lock().unwrap().len(), 1);
        assert_eq!(pool.lock().unwrap()[0].rig.ptt_mode(), &PttMode::Vox);
        let mut warm = SelectionConnection::acquire(&pool, 1, t.clone(), &allowed, |_| {
            panic!("warm connection must be reused")
        })
        .unwrap();
        assert!(warm.rig().has_control());
        assert!(matches!(
            SelectionConnection::acquire(&pool, 1, t, &allowed, |_| panic!(
                "claimed radio must not open"
            )),
            Err(Reason::StationBusy)
        ));
        let (connection, claim) = warm.adopt();
        assert_eq!(connection.id, 1);
        assert!(pool.lock().unwrap().is_empty());
        assert!(
            pool.claims.contains(1),
            "transfer does not release ownership early"
        );
        drop(connection);
        drop(claim);
        assert!(!pool.claims.contains(1));
    }

    #[test]
    fn revoked_cold_open_returns_as_read_only_without_adopting() {
        let pool = Arc::new(MonitorConnections::new(Vec::new()));
        let authority = Revocation::default();
        let allowed = permission(&authority);
        let result = SelectionConnection::acquire(&pool, 1, transport(), &allowed, |_| {
            authority.revoke();
            controlled()
        });
        assert!(matches!(result, Err(Reason::AuthorityExpired)));
        assert!(!pool.claims.contains(1));
        assert_eq!(pool.lock().unwrap().len(), 1);
        assert_eq!(pool.lock().unwrap()[0].rig.ptt_mode(), &PttMode::Vox);
        assert!(matches!(
            SelectionConnection::acquire(&pool, 2, transport(), &allowed, |_| panic!(
                "revoked admission must not open"
            )),
            Err(Reason::AuthorityExpired)
        ));
        assert!(!pool.claims.contains(2));
    }

    #[test]
    fn failed_cold_open_retains_monitor_recovery_state() {
        let pool = Arc::new(MonitorConnections::new(Vec::new()));
        let authority = Revocation::default();
        let allowed = permission(&authority);
        let before = crate::service::now_unix_ms();
        let result = SelectionConnection::acquire(&pool, 1, transport(), &allowed, |_| {
            (Rig::vox(), None, Some(false))
        });
        assert!(matches!(result, Err(Reason::HardwareUnavailable)));
        assert!(!pool.claims.contains(1));
        let connections = pool.lock().unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].open_failures, 1);
        assert!(connections[0].retry_after_ms >= before + 1000.0);
    }

    #[test]
    fn changed_transport_replaces_the_old_connection_and_panic_releases_ownership() {
        let pool = Arc::new(MonitorConnections::new(Vec::new()));
        let authority = Revocation::default();
        let allowed = permission(&authority);
        let t = transport();
        drop(
            SelectionConnection::acquire(&pool, 1, t.clone(), &allowed, |_| controlled()).unwrap(),
        );
        let mut changed = t;
        changed.baud = changed.baud.saturating_add(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _lease = SelectionConnection::acquire(&pool, 1, changed, &allowed, |_| {
                panic!("failed opener")
            });
        }));
        assert!(result.is_err());
        assert!(!pool.claims.contains(1));
        assert!(!pool.connections.is_poisoned());
        drop(
            SelectionConnection::acquire(&pool, 1, transport(), &allowed, |_| controlled())
                .unwrap(),
        );
        assert_eq!(
            pool.lock().unwrap().len(),
            1,
            "subsequent acquisition recovers"
        );
    }
}
