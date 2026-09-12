//! Stop admission stays independent of Engine, file writes and ordinary commands.
//! The mirror is updated only under Core; its own lock never contains I/O or
//! acquires Core/Engine. Atomic permit generation checks reject delayed Stops.
use super::*;

pub(super) struct Owner {
    boot: String,
    lease: String,
    session: String,
    device: String,
    until: Instant,
    epoch: u64,
    connection: u64,
    lease_epoch: u64,
}

impl Authority {
    pub(super) fn revoke_transmit_device(&self, device: &str) -> Result<(), &'static str> {
        {
            let mut pending = self
                .transmit_revocations
                .lock()
                .map_err(|_| "authorityUnavailable")?;
            // This local-only queue is bounded independently of a blocked file.
            if pending.len() >= 64 && !pending.contains(device) {
                return Err("remoteBusy");
            }
            pending.insert(device.to_owned());
        }
        let mut owner = self.stop_owner.lock().map_err(|_| "authorityUnavailable")?;
        if owner.as_ref().is_some_and(|o| o.device == device) {
            self.transmit.revoke();
            *owner = None;
        }
        // reconcile removes the queued grant before any subsequent command.
        // Neither lock above acquires Core/Engine or performs external I/O.
        Ok(())
    }

    pub(super) fn sync_stop_owner(&self, c: &Core) {
        let owner = c
            .lease
            .as_ref()
            .filter(|l| {
                c.control_grants.contains(&l.device) && c.transmit_grants.contains(&l.device)
            })
            .and_then(|l| {
                c.boot.as_ref().map(|boot| Owner {
                    boot: boot.clone(),
                    lease: l.id.clone(),
                    session: l.session.clone(),
                    device: l.device.clone(),
                    until: l.until,
                    epoch: c.epoch,
                    connection: c.connection,
                    lease_epoch: c.lease_epoch,
                })
            });
        // No path holding this mutex waits for Core, Engine or an external resource.
        if let Ok(mut current) = self.stop_owner.lock() {
            *current = owner;
        }
    }

    pub fn stop_transmit(
        &self,
        connection: u64,
        session: &str,
        device: &str,
        request: &Request,
        now: Instant,
    ) -> Result<(), &'static str> {
        let Request::StopTransmit {
            request_id,
            station_boot_id,
            lease_id,
            transmit_epoch,
        } = request
        else {
            return Err("invalidRequest");
        };
        if ![
            request_id.as_str(),
            station_boot_id,
            lease_id,
            session,
            device,
        ]
        .into_iter()
        .all(identifier)
            || transmit_epoch.len() != 16
            || !transmit_epoch
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalidRequest");
        }
        let generation = u64::from_str_radix(transmit_epoch, 16).map_err(|_| "invalidRequest")?;
        let owner = self.stop_owner.lock().map_err(|_| "authorityUnavailable")?;
        let owner = owner.as_ref().ok_or("localPermissionRequired")?;
        if owner.connection != connection
            || connection != self.connection.load(Ordering::SeqCst)
            || owner.epoch != self.epoch.load(Ordering::SeqCst)
            || owner.lease_epoch != self.lease_epoch.load(Ordering::SeqCst)
        {
            return Err("staleConnection");
        }
        if owner.boot != *station_boot_id {
            return Err("staleStation");
        }
        if owner.session != session || owner.device != device || owner.lease != *lease_id {
            return Err("notController");
        }
        if now.max(Instant::now()) >= owner.until {
            return Err("leaseExpired");
        }
        if !self.transmit.revoke_generation(generation) {
            return Err("staleContext");
        }
        // This accepts revocation, not a claim that RF has stopped. The native
        // radio loop performs the already-tested halt/flush/unkey sequence.
        Ok(())
    }
}
