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

impl Owner {
    /// This browser's lease RAN OUT, and everything else it was issued under still stands: the same
    /// station boot, connection, permission epoch and lease epoch, with the operator's station
    /// control grant still given to its device. Only then does it keep the stop token.
    ///
    /// A lease that was RELEASED, replaced by another browser, revoked or invalidated has not run
    /// out — `now` is still inside it, or one of the identities above has moved — so the owner is
    /// dropped exactly as before and the refusals for those cases are unchanged.
    fn outlived_its_lease(&self, c: &Core, now: Instant) -> bool {
        c.lease.is_none()
            && now >= self.until
            && c.boot.as_deref() == Some(self.boot.as_str())
            && c.control_grants.contains(&self.device)
            && self.epoch == c.epoch
            && self.connection == c.connection
            && self.lease_epoch == c.lease_epoch
    }
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

    /// The same for a logging or station-control revoke (#318). `reconcile` removes the grant,
    /// and ends the device's lease if it holds one, before any later request consumes Core.
    pub(super) fn revoke_grant_device(
        &self,
        grant: Grant,
        device: &str,
    ) -> Result<(), &'static str> {
        {
            let mut pending = self
                .grant_revocations
                .lock()
                .map_err(|_| "authorityUnavailable")?;
            let entry = (grant, device.to_owned());
            // Bounded like the transmit queue, and for the same reason.
            if pending.len() >= 64 && !pending.contains(&entry) {
                return Err("remoteBusy");
            }
            pending.insert(entry);
        }
        let owner = self.stop_owner.lock().map_err(|_| "authorityUnavailable")?;
        if owner.as_ref().is_some_and(|o| o.device == device) {
            // The controller's in-flight hardware and transmit permits end at once, as they do
            // when Core is free; the lease itself goes at the next reconcile.
            self.revoke_execution();
        }
        Ok(())
    }

    /// The browser that may stop right now, recomputed under Core.
    ///
    /// ⚠️ AN EXPIRED LEASE KEEPS ITS STOP TOKEN (operator ruling, 2026-09-15). `reconcile` drops the
    /// lease the moment it runs out, so without this the ruling below would be inert: the owner
    /// would be gone and the Stop refused `localPermissionRequired` instead of `leaseExpired`. The
    /// worst this allows is an unnecessary unkey of a station that was already transmitting, and
    /// that is a smaller harm than a keyed rig with a Stop button that reported a refusal. It
    /// keeps nothing else alive: every arming path checks the LIVE lease and the transmit grant.
    pub(super) fn sync_stop_owner(&self, c: &Core, now: Instant) {
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
            if owner.is_none()
                && current
                    .as_ref()
                    .is_some_and(|o| o.outlived_its_lease(c, now))
            {
                return;
            }
            *current = owner;
        }
    }

    /// Does this browser hold the station's stop token right now?
    ///
    /// `state` hands it the transmit epoch on the strength of this, a controller and an expired
    /// controller alike. The expired one needs a CURRENT epoch: dropping its lease revoked the
    /// generation it was last shown, so a Stop carrying that one would be refused `staleContext`
    /// and the ruling above would never reach the rig. Replay safety is untouched — a Stop still
    /// retires only the generation it displayed, and a delayed one is still refused.
    pub(super) fn holds_stop_token(&self, session: &str, device: &str) -> bool {
        self.stop_owner.lock().is_ok_and(|o| {
            o.as_ref()
                .is_some_and(|o| o.session == session && o.device == device)
        })
    }

    pub fn stop_transmit(
        &self,
        connection: u64,
        session: &str,
        device: &str,
        request: &Request,
        // Deliberately unread: no clock gates a Stop any more (see the ruling below).
        _now: Instant,
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
        // ⛔ No lease-expiry check, and that is the point (operator ruling, 2026-09-15). A browser
        // that held station control still stops after its lease runs out: an unnecessary unkey is a
        // smaller harm than a keyed rig and a button that reported a refusal. It cannot start
        // anything — every arming path needs the live lease this browser no longer has.
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
/// ⭐ **AND IT ENDS AN ACTIVE SATELLITE TRACK** (`satellite::disarm_track` — the rail Stop's own
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
///
/// ⚠️ **ONE ENGINE ACQUISITION, NOT TWO.** The satellite disarm briefly ran on a thread of its own,
/// which made the Stop contend with itself: the disarm's blocking lock and the ordinary-command
/// path's `try_lock` raced, and the browser's very next request after pressing Stop was refused
/// `stationBusy` by the Stop that preceded it (four times in five, measured). A Stop is never
/// refused that way — its admission touches no Engine — but the request behind it was. Every verb
/// therefore rides the single guard below, and only the mast halt (a blocking socket write, with no
/// Engine in it) is allowed a thread.
pub(super) fn stop_station(engine: &crate::SharedEngine) {
    /// Every stop verb under the one guard, returning the mast halt the caller runs once the guard
    /// is gone. The satellite disarm goes first so `halt_tx` stays last, as it is in every combined
    /// local stop.
    fn stop(e: &mut tempo_app::engine::Engine) -> Option<String> {
        tempo_core::applog::info(
            "tx",
            "remote Stop TX: stopping every transmission at the station",
        );
        #[cfg(feature = "radio")]
        let halt = super::station::satellite::disarm_track(e);
        #[cfg(not(feature = "radio"))]
        let halt = None;
        e.stop_cw();
        e.rtty_stop();
        e.psk_stop();
        e.sstv_stop();
        e.stop_voice();
        e.halt_tx();
        halt
    }
    // A blocking socket write, so the thread that owes the browser its acceptance never runs it. A
    // thread that cannot be spawned runs it here instead — a Stop that quietly declined to halt the
    // mast is not an option.
    fn halt_off_thread(addr: Option<String>) {
        #[cfg(feature = "radio")]
        if let Some(addr) = addr {
            let owned = addr.clone();
            if std::thread::Builder::new()
                .name("remote-stop-rotator".into())
                .spawn(move || super::station::satellite::halt_mast(owned))
                .is_err()
            {
                super::station::satellite::halt_mast(addr);
            }
        }
        #[cfg(not(feature = "radio"))]
        let _ = addr;
    }
    match engine.try_lock() {
        Ok(mut e) => {
            let halt = stop(&mut e);
            drop(e);
            halt_off_thread(halt);
        }
        Err(std::sync::TryLockError::Poisoned(poisoned)) => {
            let mut e = poisoned.into_inner();
            let halt = stop(&mut e);
            drop(e);
            halt_off_thread(halt);
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            let engine = engine.clone();
            std::thread::spawn(move || {
                let halt = stop(
                    &mut engine
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                );
                // Already off the accepting thread; the mast halt needs no second one.
                #[cfg(feature = "radio")]
                if let Some(addr) = halt {
                    super::station::satellite::halt_mast(addr);
                }
                #[cfg(not(feature = "radio"))]
                let _ = halt;
            });
        }
    }
}
