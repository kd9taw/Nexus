//! Narrow, host-owned persistence after Remote authority and configuration
//! preconditions have been checked. A sanitized browser Settings form is never
//! accepted here. Keep native policy, decoder state and other profiles intact.
use super::Engine;
use crate::dto::{SourceKind, Tier};
use crate::remote_control::{Permit, Reason};
use crate::settings::OperatingMode;
use std::sync::TryLockError;
use std::time::Instant;

#[derive(Clone, Copy)]
enum DecoderSetting {
    Js8Speed { expected: u8, speed: u8 },
    Msk144Period { expected: u16, secs: u16 },
}

#[cfg(test)]
mod tests;

impl Engine {
    pub fn save_remote_js8_speed(
        &mut self,
        expected: u8,
        speed: u8,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.save_remote_decoder_setting(
            DecoderSetting::Js8Speed { expected, speed },
            connection,
            permit,
        )
    }

    pub fn save_remote_msk144_period(
        &mut self,
        expected: u16,
        secs: u16,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.save_remote_decoder_setting(
            DecoderSetting::Msk144Period { expected, secs },
            connection,
            permit,
        )
    }

    fn save_remote_decoder_setting(
        &mut self,
        setting: DecoderSetting,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.remote_radio_idle()?;
        self.remote_radio_link(connection)?;
        if self.source_kind != SourceKind::Native
            || self.settings.operating_mode != OperatingMode::Digital
        {
            return Err(Reason::UnsupportedAction);
        }
        let mut next = self.settings.clone();
        match setting {
            DecoderSetting::Js8Speed { expected, speed } => {
                if modes::Js8Speed::from_index(expected).is_none()
                    || modes::Js8Speed::from_index(speed).is_none()
                    || expected == speed
                {
                    return Err(Reason::InvalidAction);
                }
                if self.tier() != Tier::Js8 || self.settings.js8_speed != expected {
                    return Err(Reason::ContextChanged);
                }
                next.js8_speed = speed;
            }
            DecoderSetting::Msk144Period { expected, secs } => {
                let periods = modes::mode::ModeKind::MSK144_PERIODS;
                if !periods.contains(&expected) || !periods.contains(&secs) || expected == secs {
                    return Err(Reason::InvalidAction);
                }
                if self.tier() != Tier::Msk144 || self.settings.msk144_period_s != expected {
                    return Err(Reason::ContextChanged);
                }
                next.msk144_period_s = secs;
            }
        }
        // Speed replacement must use the existing stable source mutex, without
        // parking Engine behind an active decode. MSK144's native narrow period
        // verb does not replace the source or reset any QSO/queue/slot policy.
        let source = self.source.clone();
        let mut slot = if matches!(setting, DecoderSetting::Js8Speed { .. }) {
            Some(match source.try_lock() {
                Ok(slot) => slot,
                Err(TryLockError::Poisoned(error)) => error.into_inner(),
                Err(TryLockError::WouldBlock) => return Err(Reason::StationBusy),
            })
        } else {
            None
        };
        self.remote_radio_link(connection)?;
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        // Save the one-field projection before publishing runtime state. A
        // failed save changes neither the decoder nor the visible preference.
        // Once this atomic save is admitted it may finish after disconnection;
        // it is a saved choice, never a deferred or replayable hardware write.
        next.save(
            self.remote_settings_path
                .as_ref()
                .ok_or(Reason::UnsupportedAction)?,
        )
        .map_err(|_| Reason::PersistenceFailed)?;
        match setting {
            DecoderSetting::Js8Speed { speed, .. } => self
                .js8_set_speed_with_installer(speed, |engine, source| {
                    engine.install_source_into(slot.as_mut().expect("JS8 decoder lock"), source)
                })
                .expect("speed validated before saving"),
            DecoderSetting::Msk144Period { secs, .. } => self.set_msk144_period(secs),
        }
        Ok(())
    }

    pub fn save_remote_amp_follow_band(
        &mut self,
        radio_id: u32,
        expected: bool,
        follow: bool,
        permit: &Permit,
    ) -> Result<(), Reason> {
        if radio_id != self.settings.active_radio
            || self.settings.amp_follow_band != expected
            || self
                .settings
                .active_profile()
                .is_none_or(|p| p.amp_follow_band != expected)
        {
            return Err(Reason::ContextChanged);
        }
        if expected == follow {
            return Err(Reason::InvalidAction);
        }
        let path = self
            .remote_settings_path
            .as_ref()
            .ok_or(Reason::UnsupportedAction)?;
        let mut next = self.settings.clone();
        next.amp_follow_band = follow;
        next.radios
            .iter_mut()
            .find(|p| p.id == radio_id)
            .ok_or(Reason::ContextChanged)?
            .amp_follow_band = follow;
        // Admission ends here. This one persistent choice may finish saving
        // after a disconnect; it does not become a pending hardware command or
        // roll back the operator's preference on lease expiry.
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        self.remote_actuation.revoke();
        next.save(path).map_err(|_| Reason::PersistenceFailed)?;
        // Publish only after the native atomic save succeeds. Do not run the
        // broad form-apply path or copy an old Settings snapshot over live state.
        self.settings.amp_follow_band = follow;
        self.settings
            .radios
            .iter_mut()
            .find(|p| p.id == radio_id)
            .expect("profile checked under the same Engine borrow")
            .amp_follow_band = follow;
        Ok(())
    }
}
