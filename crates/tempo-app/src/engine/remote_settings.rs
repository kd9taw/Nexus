//! Narrow, host-owned persistence after Remote authority and configuration
//! preconditions have been checked. A sanitized browser Settings form is never
//! accepted here. Keep native policy, decoder state and other profiles intact.
use super::Engine;
use crate::remote_control::{Permit, Reason};
use std::time::Instant;

impl Engine {
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
