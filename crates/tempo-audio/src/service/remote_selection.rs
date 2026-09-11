//! Radio selection runs on the active connection owner, before local handoff.
//! The incoming connection stays claimed through preparation and installation;
//! failed preparation returns it to read-only monitoring, never a retune queue.
use super::*;
use crate::rig::remote::{Handoff, HandoffReadback, Position, RepeaterConfig, Retune};
use tempo_app::engine::remote_radio::RadioLevel;
use tempo_app::engine::remote_selection::{Configuration, Readback, Request};
use tempo_app::remote_control::{Reason, WritePermission};

struct MonitorPause<'a> {
    flag: &'a std::sync::atomic::AtomicBool,
    prior: bool,
}
impl<'a> MonitorPause<'a> {
    fn new(flag: &'a std::sync::atomic::AtomicBool) -> Self {
        Self {
            flag,
            prior: flag.swap(true, std::sync::atomic::Ordering::Relaxed),
        }
    }
}
impl Drop for MonitorPause<'_> {
    fn drop(&mut self) {
        self.flag
            .store(self.prior, std::sync::atomic::Ordering::Relaxed);
    }
}

impl RadioLoop {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_remote_selection<B: AudioBackend>(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        pool: &MonitorPool,
        rig: &mut Rig,
        backend: &mut B,
        last_active: &mut u32,
        pending: &std::sync::atomic::AtomicBool,
        mut open: impl FnMut(&Transport) -> (Rig, Option<CatDaemon>, Option<bool>),
    ) {
        let (request, configuration) = {
            let mut eng = engine_lock(engine);
            let Some(request) = eng.take_remote_radio_selection() else {
                return;
            };
            let ready = request.configuration(&eng).and_then(|configuration| {
                let mut want = Transport::from_settings(eng.settings());
                self.apply_port_alias(&mut want);
                let now = now_unix_ms();
                if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed)
                    || self.tx_until_ms.is_some()
                    || self.tuning_keyed
                    || self.manual_ptt_applied
                    || rig.keyed
                    || self.cw_busy_until > now
                    || self.rtty_busy_until > now
                    || self.psk_busy_until > now
                {
                    return Err(Reason::StationBusy);
                }
                if self.handoff_deferred
                    || self.cat_hold_active
                    || want != self.applied
                    || self.remote_radio_id != Some(*last_active)
                    || eng.settings().active_radio != *last_active
                    || self
                        .remote_connection
                        .as_ref()
                        .and_then(|connection| eng.remote_radio_read(connection, Instant::now()))
                        .is_none()
                {
                    return Err(Reason::ContextChanged);
                }
                Ok(configuration)
            });
            match ready {
                Ok(configuration) => (request, configuration),
                Err(reason) => {
                    request.refuse(reason);
                    return;
                }
            }
        };
        let _pause = MonitorPause::new(pending);
        let want = Transport::from_settings(request.settings());
        let target =
            match Position::new(request.settings().dial_hz(), &request.settings().rig_mode()) {
                Ok(position) => position,
                Err(reason) => {
                    request.refuse(reason);
                    return;
                }
            };
        let fm = matches!(target.mode(), "FM" | "PKTFM").then(|| {
            (
                request.settings().rptr_shift.clone(),
                request.settings().rptr_offset_hz(),
                request.settings().ctcss_tone_hz,
            )
        });
        let prepared = (|| {
            let (hz, mode) = request.expected();
            let outgoing = Position::new(hz, mode)?;
            self.selection_outgoing_read(engine, rig, &request, &outgoing)?;
            let mut monitor_transport = want.clone();
            monitor_transport.broker_self_port = None;
            let mut incoming = selection_connection::SelectionConnection::acquire(
                pool,
                request.settings().active_radio,
                monitor_transport,
                request.permission(),
                &mut open,
            )?;
            let current = incoming.rig().remote_idle_position(request.permission())?;
            let mut retune = Retune::new(current.position().clone(), target.clone());
            if let Some(limit) = configuration.power_limit {
                retune = retune.with_power_limit(limit)?;
            }
            let repeater = fm
                .as_ref()
                .map(|(shift, offset, tone)| RepeaterConfig::new(shift, *offset, *tone))
                .transpose()?;
            let handoff = Handoff::new(
                retune,
                configuration.levels.clone(),
                configuration.agc,
                repeater,
            )?;
            // Revalidate native intent before physical cleanup. The native
            // owner handles VOX by flushing audio as well as releasing PTT.
            request.validate(&engine_lock(engine))?;
            request.permission().check(Instant::now())?;
            backend.flush_output();
            rig.remote_unkey_idle(outgoing.clone(), request.permission())?;
            self.selection_clear_keyers(rig, request.permission())?;
            incoming.rig().set_ptt_mode(ptt_mode_for(&want));
            incoming
                .rig()
                .remote_unkey_idle(current.position().clone(), request.permission())?;
            let readback = incoming
                .rig()
                .remote_handoff(handoff, request.permission())?;
            // Cold open and configuration may outlive the prior outgoing
            // sample. Refresh on its original token, never the incoming id.
            self.selection_outgoing_read(engine, rig, &request, &outgoing)?;
            Ok::<_, Reason>((incoming, readback))
        })();
        let (incoming, readback) = match prepared {
            Ok(prepared) => prepared,
            Err(reason) => {
                request.refuse(reason);
                return;
            }
        };
        // Keep the unadopted lease outside the closure. If commit refuses,
        // drop it only AFTER releasing the pool mutex it returns into.
        let mut incoming = Some(incoming);
        let mut connections = match pool.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                request.refuse(Reason::StationBusy);
                return;
            }
        };
        let mut eng = engine_lock(engine);
        let Some(decoder) = tempo_app::engine::remote_selection::Ft8A7ResetGuard::try_acquire()
        else {
            request.refuse(Reason::StationBusy);
            return;
        };
        let radio = request.settings().active_radio;
        request.commit_with_install(
            &mut eng,
            Readback {
                radio,
                dial_hz: readback.radio().position().dial_hz(),
                mode: readback.radio().position().mode(),
                sampled_at: readback.radio().sampled_at(),
            },
            decoder,
            |eng| {
                self.rx_tap.retire_receive_audio();
                let (connection, claim) = incoming.take().expect("prepared connection").adopt();
                connections.push(self.install_handoff_connection(
                    rig,
                    connection,
                    &want,
                    *last_active,
                ));
                *last_active = radio;
                self.adopt_selection_readback(eng, &configuration, &readback, fm);
                // Physical cleanup above already serviced these native handoff
                // aborts. Do not send another keyer flush on the incoming radio.
                let _ = eng.take_cw_abort();
                let _ = eng.take_rtty_abort();
                let _ = eng.take_psk_abort();
                let _ = eng.take_sstv_abort();
                drop(claim);
            },
        );
        drop(eng);
        drop(connections);
        drop(incoming);
        // The normal next tick creates the incoming observation token BEFORE
        // its next read. Preparation readings are transaction evidence; they
        // must not be re-stamped as a newly started live-stream measurement.
    }

    fn selection_outgoing_read(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        rig: &mut Rig,
        request: &Request,
        expected: &Position,
    ) -> Result<(), Reason> {
        let read = self.remote_read(engine).ok_or(Reason::ContextChanged)?;
        let actual = rig.remote_idle_position(request.permission())?;
        let mut eng = engine_lock(engine);
        eng.remote_observe_cat(Some(&read), Some(true));
        eng.remote_observe_dial(Some(&read), Some(actual.position().dial_hz()));
        eng.remote_observe_mode(Some(&read), Some(actual.position().mode()));
        eng.remote_observe_ptt(Some(&read), Some(false));
        if actual.position() != expected {
            return Err(Reason::ContextChanged);
        }
        request.validate(&eng)
    }

    fn selection_clear_keyers(
        &mut self,
        rig: &mut Rig,
        permission: &WritePermission,
    ) -> Result<(), Reason> {
        // Preserve native best-effort CAT Morse flushing: not every backend
        // implements it. Final idle readback is required independently.
        let _ = rig.remote_stop_morse(permission);
        permission.check(Instant::now())?;
        #[cfg(feature = "serial")]
        {
            if let Some((_, keyer)) = self.winkeyer.as_mut() {
                // The already-open keyer writes one Clear Buffer byte; there
                // is no intervening open/connect operation after this check.
                permission.begin_write(Instant::now())?;
                keyer.clear().map_err(|_| Reason::HardwareUnconfirmed)?;
            }
            permission.check(Instant::now())?;
            if let Some((_, _, keyer)) = self.serial_keyer.as_ref() {
                keyer.clear();
            }
            if let Some((_, _, keyer)) = self.rtty_keyer.as_ref() {
                keyer.clear();
            }
        }
        Ok(())
    }

    fn adopt_selection_readback(
        &mut self,
        engine: &mut Engine,
        configuration: &Configuration,
        readback: &HandoffReadback,
        fm: Option<(String, i64, f32)>,
    ) {
        self.last_dial = readback.radio().position().dial_hz();
        self.last_mode = readback.radio().position().mode().into();
        self.rig_asserted = true;
        self.cat_ok = Some(true);
        self.last_fm = fm;
        for &(level, desired) in &configuration.levels {
            match level {
                RadioLevel::Power => {
                    self.last_rf_power = Some(desired);
                    engine.set_rf_power(desired);
                }
                RadioLevel::MicGain => self.last_mic_gain = Some(desired),
                RadioLevel::NoiseReduction => self.last_nr_level = Some(desired),
                RadioLevel::Compression => self.last_comp_level = Some(desired),
                RadioLevel::NotchFrequency => self.last_notch_freq_hz = Some(desired),
            }
        }
        for &(level, actual) in readback.levels() {
            match level {
                RadioLevel::Power => engine.observe_rig_power(actual),
                RadioLevel::MicGain => engine.observe_rig_mic_gain(actual),
                RadioLevel::NoiseReduction => engine.observe_rig_nr_level(actual),
                RadioLevel::Compression => engine.observe_rig_comp_level(actual),
                RadioLevel::NotchFrequency => engine.observe_rig_notch_freq_hz(actual),
            }
        }
        if let Some(power) = readback.radio().power() {
            engine.observe_rig_power(power);
        }
        if let Some(agc) = configuration.agc {
            self.last_agc = Some(agc.name().into());
            let _ = engine.agc_to_command();
            engine.observe_rig_agc(agc.name().into());
        }
        engine.observe_rig_mode(self.last_mode.clone());
        engine.observe_rig_ptt(false);
    }
}
