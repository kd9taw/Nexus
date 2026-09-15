//! Stop admission stays independent of Engine, file writes and ordinary commands.
//! The mirror is updated only under Core; its own lock never contains I/O or
//! acquires Core/Engine. Atomic permit generation checks reject delayed Stops.
//!
//! STOP ANYTHING (operator decision 2026-09-14). Any browser holding station control may stop, with
//! or without FT8/FT4 transmit permission (that permission is needed only to START), and an admitted
//! Stop stops every transmission at the station, however it started (`stop_station`). A browser
//! without station control is refused and changes nothing.
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
        let owner = self.stop_owner.lock().map_err(|_| "authorityUnavailable")?;
        if owner.as_ref().is_some_and(|o| o.device == device) {
            // Ends this browser's own transmission at once. Its Stop stays: stopping needs only
            // station control, and revoking transmit permission leaves that in place.
            self.transmit.revoke();
        }
        // reconcile removes the queued grant before any subsequent command.
        // Neither lock above acquires Core/Engine or performs external I/O.
        Ok(())
    }

    pub(super) fn sync_stop_owner(&self, c: &Core) {
        let owner = c
            .lease
            .as_ref()
            // Station control, not transmit permission: stopping is always the safe direction.
            .filter(|l| c.control_grants.contains(&l.device))
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
        // This accepts the Stop, not a claim that RF has stopped: `stop_station` raises the
        // engine's stop, and the native radio loop performs the already-tested flush/unkey.
        Ok(())
    }
}

/// Stop anything (operator decision 2026-09-14): what an admitted browser Stop does at the station.
///
/// It runs every stop verb the desktop's own stop controls run, under one Engine lock:
///   · Operate and JS8 Stop TX, and the Phone and SSTV header Stop TX → `halt_tx`
///   · CW Stop TX (and Esc) → `stop_cw` + `halt_tx`
///   · RTTY Stop TX → `rtty_stop` + `halt_tx`; PSK Stop TX → `psk_stop` + `halt_tx`
///   · SSTV's send-bar Stop → `sstv_stop`; the Phone voice keyer's ■ Stop → `stop_voice`
///
/// The station does not know which cockpit the browser is showing, so it runs the union, `halt_tx`
/// last as every combined local stop does. Each verb only clears queues and raises one-shot aborts
/// the radio loop turns into a flush and an unkey; none arms, keys or re-enables anything, and
/// `halt_tx` leaves the TX-enable latch off exactly as a local Stop TX does. The shack's own Stop
/// controls are untouched.
///
/// ⭐ **AND IT ENDS AN ACTIVE SATELLITE TRACK** (`satellite::stop_track` — the rail Stop's own
/// verb: disarm, hand the dial back, halt the mast). This is the ONE place the browser's Stop does
/// more than the desktop's Stop TX does, and it is deliberate. At the shack those are two
/// controls, a metre apart, and the operator picks; a browser has one Stop, and a satellite track
/// is the app's only standing instruction to keep MOVING the radio and the mast by itself. A Stop
/// that left it running would leave the station steering after the operator said stop, which is
/// the failure the rule against moving the radio unattended exists to prevent. Ending it is also
/// the safe direction in the sense every other verb here is: it releases the dial, it commands no
/// new position, and it arms nothing.
///
/// It ends ANY live track, not only one this browser armed. A browser holding station control is
/// the operator, and a Stop that silently declined to stop what is in front of them — because the
/// pass happened to be armed at the shack — would be the worse surprise of the two.
///
/// The acceptance never waits for Engine. When Engine is held (a radio-loop tick, another command),
/// the stop runs on its own thread as soon as Engine is free: the same wait a Stop TX press at the
/// shack has. A poisoned Engine is still stopped.
pub(super) fn stop_station(engine: &crate::SharedEngine) {
    // On its own thread for the same reason the transmit stop below takes one when Engine is
    // contended: the acceptance must never wait for Engine, and the disarm takes that lock. A
    // thread that cannot be spawned runs it here instead — a Stop that quietly declined to stop
    // the track is not an option.
    #[cfg(feature = "radio")]
    {
        let owned = engine.clone();
        if std::thread::Builder::new()
            .name("remote-stop-satellite".into())
            .spawn(move || super::station::satellite::stop_track(&owned))
            .is_err()
        {
            super::station::satellite::stop_track(engine);
        }
    }
    fn stop(e: &mut tempo_app::engine::Engine) {
        tempo_core::applog::info(
            "tx",
            "remote Stop TX: stopping every transmission at the station",
        );
        e.stop_cw();
        e.rtty_stop();
        e.psk_stop();
        e.sstv_stop();
        e.stop_voice();
        e.halt_tx();
    }
    match engine.try_lock() {
        Ok(mut e) => stop(&mut e),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => stop(&mut poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => {
            let engine = engine.clone();
            std::thread::spawn(move || {
                stop(
                    &mut engine
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                )
            });
        }
    }
}
