//! One amplifier intent, bound to the station and the actual open links. The
//! amplifier worker owns the wire and supplies later observations for completion.
use super::{Completion, Evidence, Outcome, Permit, Reason};
use crate::engine::Engine;
use crate::remote_monitor::{Amplifier, Observation};
use std::time::Instant;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub enum Target {
    Operate {
        expected: bool,
        desired: bool,
    },
    Band {
        expected: String,
        desired: String,
        direction: i8,
    },
}

pub struct Request {
    pub target: Target,
    pub completion: Completion,
    permit: Permit,
    native_permit: Permit,
    radio: u32,
    generation: u64,
    radio_connection: u64,
    amp_connection: u64,
    prior_read: u64,
    sent_read: Option<u64>,
    attempted: bool,
}

impl Request {
    pub fn new(
        engine: &Engine,
        target: Target,
        radio_connection: u64,
        amp_connection: u64,
        prior_read: u64,
        permit: Permit,
    ) -> Result<Self, Reason> {
        let request = Self {
            native_permit: engine
                .remote_actuation_permit(permit.deadline())
                .ok_or(Reason::ContextChanged)?,
            target,
            completion: Completion::guarded(permit.clone()),
            permit,
            radio: engine.settings().active_radio,
            generation: engine.remote_log_context_generation(),
            radio_connection,
            amp_connection,
            prior_read,
            sent_read: None,
            attempted: false,
        };
        let observation = request.context(engine)?;
        request.idle(engine, &observation)?;
        request.expected(
            observation
                .amplifier
                .as_ref()
                .ok_or(Reason::ReadingUnavailable)?,
        )?;
        Ok(request)
    }

    fn context(&self, engine: &Engine) -> Result<Observation, Reason> {
        if !self.permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if !self.native_permit.valid(Instant::now()) {
            return Err(Reason::ContextChanged);
        }
        if engine.settings().active_radio != self.radio
            || engine.remote_log_context_generation() != self.generation
        {
            return Err(Reason::ContextChanged);
        }
        let o = engine.remote_monitor_observation();
        let cat = o.radio.readings.cat.ok_or(Reason::ReadingUnavailable)?;
        let amp = o.amplifier.as_ref().ok_or(Reason::ReadingUnavailable)?;
        let reading = amp.reading.ok_or(Reason::ReadingUnavailable)?;
        if cat.connection_generation != self.radio_connection
            || reading.connection_generation != self.amp_connection
        {
            return Err(Reason::ContextChanged);
        }
        if o.radio.cat_connected != Some(true)
            || !amp.linked
            || reading.read_sequence < self.prior_read
        {
            return Err(Reason::ReadingUnavailable);
        }
        Ok(o)
    }

    fn idle(&self, engine: &Engine, o: &Observation) -> Result<(), Reason> {
        // Remote amplifier changes require a disarmed, idle exciter. A KPA does
        // not report PTT; a missing amplifier flag never becomes an idle claim.
        if engine.tx_enabled() || o.radio.nexus_busy || o.radio.rig_keyed == Some(true) {
            return Err(Reason::StationBusy);
        }
        let ptt = o.radio.readings.ptt.ok_or(Reason::ReadingUnavailable)?;
        if o.radio.rig_keyed != Some(false)
            || ptt.connection_generation != self.radio_connection
            || ptt.age_ms >= 1000
        {
            return Err(Reason::ReadingUnavailable);
        }
        let amp = o.amplifier.as_ref().ok_or(Reason::ReadingUnavailable)?;
        if amp.transmitting == Some(true) || amp.output_watts.is_some_and(|w| w > 0) {
            return Err(Reason::StationBusy);
        }
        if matches!(self.target, Target::Band { .. }) && amp.follow_band {
            return Err(Reason::StationBusy);
        }
        Ok(())
    }

    fn expected(&self, amp: &Amplifier) -> Result<(), Reason> {
        match &self.target {
            Target::Operate { expected, desired }
                if expected != desired && amp.operate == Some(*expected) =>
            {
                Ok(())
            }
            Target::Band {
                expected,
                desired,
                direction,
            } if expected != desired
                && [-1, 1].contains(direction)
                && amp.band_label.as_deref() == Some(expected.as_str()) =>
            {
                Ok(())
            }
            _ => Err(Reason::ContextChanged),
        }
    }

    /// Called AFTER a new poll and immediately before the worker's write. Never
    /// called for the local intent queue; a Remote intent cannot enter that queue.
    pub fn begin(&mut self, engine: &Engine) -> Result<(), Reason> {
        if self.sent_read.is_some() {
            return Err(Reason::ContextChanged);
        }
        let o = self.context(engine)?;
        self.idle(engine, &o)?;
        let amp = o.amplifier.as_ref().ok_or(Reason::ReadingUnavailable)?;
        self.expected(amp)?;
        let reading = amp.reading.ok_or(Reason::ReadingUnavailable)?;
        if reading.read_sequence <= self.prior_read {
            return Err(Reason::ReadingUnavailable);
        }
        self.sent_read = Some(reading.read_sequence);
        Ok(())
    }

    pub fn confirm(&self, engine: &Engine) {
        let result = (|| {
            let o = self.context(engine)?;
            let amp = o.amplifier.ok_or(Reason::ReadingUnavailable)?;
            let read = amp.reading.ok_or(Reason::ReadingUnavailable)?;
            if self.sent_read.is_none_or(|sent| read.read_sequence <= sent) {
                return Err(Reason::HardwareUnconfirmed);
            }
            match &self.target {
                Target::Operate { desired, .. } if amp.operate == Some(*desired) => Ok(()),
                Target::Band { desired, .. }
                    if amp.band_label.as_deref() == Some(desired.as_str()) =>
                {
                    Ok(())
                }
                _ => Err(Reason::HardwareUnconfirmed),
            }
        })();
        self.completion.finish(match result {
            Ok(()) => Outcome::Applied {
                evidence: Evidence::AmplifierReadback,
            },
            Err(_) => Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed,
            },
        });
    }

    pub fn refuse(&self, reason: Reason) {
        self.completion.finish(if self.attempted {
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed,
            }
        } else {
            Outcome::Rejected { reason }
        });
    }

    /// Last check after releasing the engine and before touching the port.
    pub fn begin_write(&mut self) -> bool {
        if self.attempted
            || self.sent_read.is_none()
            || !self.permit.valid(Instant::now())
            || !self.native_permit.valid(Instant::now())
            || !self.completion.begin_write(Instant::now())
        {
            return false;
        }
        self.attempted = true;
        true
    }
}
