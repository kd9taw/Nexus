//! Remote uses this existing radio owner, before ordinary settings reconciliation.
//! Only a confirmed QSY enters settings; unknown work never becomes a local retry.
use super::*;
use crate::rig::remote::{Position, Retune};
use tempo_app::remote_control::Reason;

impl RadioLoop {
    pub(super) fn apply_remote_frequency(
        &mut self,
        engine: &Arc<Mutex<Engine>>,
        rig: &mut Rig,
        now: f64,
    ) {
        let request = {
            let mut eng = engine_lock(engine);
            let Some(request) = eng.take_remote_frequency() else {
                return;
            };
            let mut want = Transport::from_settings(eng.settings());
            self.apply_port_alias(&mut want);
            let ready = request.validate(&eng).and_then(|()| {
                if self.tx_until_ms.is_some() || self.tuning_keyed || rig.keyed {
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
        let result = (|| {
            let (hz, mode) = request.expected();
            let expected = Position::new(hz, mode)?;
            let (hz, mode) = request.target();
            rig.remote_retune(
                Retune::new(expected, Position::new(hz, mode)?),
                request.permission(),
            )
        })();
        let readback = match result {
            Ok(readback) => readback,
            Err(reason) => {
                request.refuse(reason);
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
        if request.commit(&mut eng) {
            self.last_dial = position.dial_hz();
            self.last_mode = position.mode().into();
            self.rig_asserted = true;
            self.mode_giveup = None;
            self.mode_fail_count = 0;
            self.mode_saw_reject = false;
            self.dial_giveup = None;
            self.dial_fail_count = 0;
            // The transaction already obtained the immediate hardware sample.
            self.last_rig_poll = now;
            self.last_freq_poll = now;
        }
    }
}
