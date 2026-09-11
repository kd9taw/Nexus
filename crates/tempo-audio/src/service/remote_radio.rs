//! Remote uses this existing radio owner, before ordinary settings reconciliation.
//! Only confirmed dial/mode/power enter settings; unknown work never becomes a local retry.
use super::*;
use crate::rig::remote::{Handoff, Position, RepeaterConfig, Retune};
use tempo_app::engine::remote_radio::{RadioLevel, ReceiverDsp};
use tempo_app::remote_control::Reason;

impl RadioLoop {
    pub(super) fn apply_remote_radio(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        rig: &mut Rig,
        now: f64,
    ) {
        let request = {
            let mut eng = engine_lock(engine);
            let Some(request) = eng.take_remote_radio() else {
                return;
            };
            let mut want = Transport::from_settings(eng.settings());
            self.apply_port_alias(&mut want);
            let ready = request.validate(&eng).and_then(|()| {
                if self.tx_until_ms.is_some()
                    || self.tuning_keyed
                    || rig.keyed
                    || ((request.filter_width().is_some()
                        || request.receiver_dsp().is_some()
                        || request.level().is_some())
                        && !self.rig_asserted)
                {
                    Err(Reason::StationBusy)
                } else if self.handoff_deferred
                    || self.cat_hold_active
                    || want != self.applied
                    || self.remote_radio_id != Some(eng.settings().active_radio)
                    || self
                        .remote_connection
                        .as_ref()
                        .and_then(|c| eng.remote_radio_read(c, Instant::now()))
                        .is_none()
                {
                    Err(Reason::ContextChanged)
                } else {
                    Ok(())
                }
            });
            if let Err(reason) = ready {
                request.refuse(reason);
                return;
            }
            request
        };
        let retuning = request.level().is_none()
            && request.filter_width().is_none()
            && request.receiver_dsp().is_none();
        let result = (|| {
            let (hz, mode) = request.expected();
            let expected = Position::new(hz, mode)?;
            if let Some((level, before, value)) = request.level() {
                return rig.remote_level(expected, level, before, value, request.permission());
            }
            if let Some((before, width)) = request.filter_width() {
                return rig.remote_filter_width(expected, before, width, request.permission());
            }
            if let Some((before, value)) = request.receiver_dsp() {
                return rig.remote_receiver_dsp(expected, before, value, request.permission());
            }
            let (hz, mode) = request.target();
            let mut retune = Retune::new(expected, Position::new(hz, mode)?);
            if let Some(limit) = request.power_limit() {
                retune = retune.with_power_limit(limit)?;
            }
            if let Some((shift, offset, tone)) = request.repeater() {
                let fm = RepeaterConfig::new(shift, offset, tone)?;
                // Reuse the complete native configuration transaction: probe
                // before writes, apply FM after mode/dial, then read everything
                // back under the original permission and deadline.
                return rig
                    .remote_handoff(
                        Handoff::new(retune, Vec::new(), None, Some(fm))?,
                        request.permission(),
                    )
                    .map(|read| read.into_radio());
            }
            rig.remote_retune(retune, request.permission())
        })();
        let readback = match result {
            Ok(readback) => readback,
            Err(reason) => {
                request.refuse(reason);
                if retuning && request.uncertain() {
                    self.remote_retune_uncertain = true;
                }
                return;
            }
        };
        let mut eng = engine_lock(engine);
        let read = self
            .remote_connection
            .as_ref()
            .and_then(|c| eng.remote_radio_read(c, readback.sampled_at()));
        let position = readback.position();
        eng.remote_observe_cat(read.as_ref(), Some(true));
        eng.remote_observe_dial(read.as_ref(), Some(position.dial_hz()));
        eng.remote_observe_mode(read.as_ref(), Some(position.mode()));
        eng.remote_observe_ptt(read.as_ref(), Some(false));
        let filter = request.filter_width().is_some();
        let dsp = request.receiver_dsp().is_some();
        let level = request.level();
        let repeater = request
            .repeater()
            .map(|(shift, offset, tone)| (shift.to_owned(), offset, tone));
        let committed = if level.is_some() {
            request.commit_level_readback(&mut eng, readback.level())
        } else if dsp {
            request.commit_receiver_dsp_readback(&mut eng, readback.receiver_dsp())
        } else if filter {
            request.commit_filter_readback(&mut eng, readback.passband())
        } else {
            request.commit_tuning_readback(
                &mut eng,
                readback.power(),
                readback
                    .repeater()
                    .map(|fm| (fm.shift(), fm.offset_hz(), fm.tone_hz())),
            )
        };
        if retuning {
            // Even a successful CAT exchange can lose authority before the
            // Engine commit. A failed save with confirmed adoption returns true
            // here: it is a durability failure, not unconfirmed radio state.
            self.remote_retune_uncertain = !committed;
        }
        if committed {
            if filter || dsp || level.is_some() {
                if let Some((level, _, desired)) = level {
                    match level {
                        RadioLevel::Power => {
                            self.last_rf_power = Some(desired);
                            self.rf_power_giveup = None;
                        }
                        RadioLevel::MicGain => {
                            self.last_mic_gain = Some(desired);
                            self.mic_gain_giveup = None;
                        }
                        RadioLevel::NoiseReduction => {
                            self.last_nr_level = Some(desired);
                            self.nr_level_giveup = None;
                        }
                        RadioLevel::Compression => {
                            self.last_comp_level = Some(desired);
                            self.comp_level_giveup = None;
                        }
                        RadioLevel::NotchFrequency => {
                            self.last_notch_freq_hz = Some(desired);
                            self.notch_freq_giveup = None;
                        }
                    }
                }
                match readback.receiver_dsp() {
                    Some(ReceiverDsp::Function { func, on }) => {
                        self.func_state[func.index()] = Some(on);
                        self.func_supported[func.index()] = Some(true);
                        self.func_misses[func.index()] = 0;
                    }
                    Some(ReceiverDsp::Agc(speed)) => {
                        self.last_agc = Some(speed.name().into());
                        self.agc_giveup = None;
                    }
                    None => {}
                }
                // Native filter adjustment preserves the physical front-panel
                // mode. Changing the canonical reconciliation belief to that
                // mode would queue a later reassertion of Settings' old mode.
                self.last_rig_poll = now;
                self.last_freq_poll = now;
                return;
            }
            // Cache the requested native tuple, not the quantized wire reading
            // or a preserved hardware offset. Otherwise the next local tick
            // would reapply a completed remote operation outside its authority.
            self.last_fm = repeater;
            self.last_dial = position.dial_hz();
            self.last_mode = position.mode().into();
            self.rig_asserted = true;
            self.mode_giveup = None;
            self.mode_fail_count = 0;
            self.mode_saw_reject = false;
            self.dial_giveup = None;
            self.dial_fail_count = 0;
            if let Some(power) = readback.power() {
                self.last_rf_power = Some(power);
                self.rf_power_giveup = None;
            }
            // The transaction already obtained the immediate hardware sample.
            self.last_rig_poll = now;
            self.last_freq_poll = now;
        }
    }
}
