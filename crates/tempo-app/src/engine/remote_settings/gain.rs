use super::*;

impl Engine {
    /// Save the active capture gain through the native narrow setter. The
    /// displayed flat value is authoritative: a preceding local narrow edit
    /// can legitimately leave the in-memory profile mirror behind its save.
    pub fn save_remote_rx_gain(
        &mut self,
        radio_id: u32,
        expected: f32,
        gain: f32,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.remote_radio_idle()?;
        self.remote_radio_link(connection)?;
        if self.source_kind != SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        if !expected.is_finite()
            || !gain.is_finite()
            || !(1.0..=8.0).contains(&gain)
            || expected == gain
        {
            return Err(Reason::InvalidAction);
        }
        if self.settings.active_radio != radio_id
            || self.settings.rx_gain != expected
            || self.settings.active_profile().is_none()
        {
            return Err(Reason::ContextChanged);
        }
        let mut next = self.settings.clone();
        next.rx_gain = gain;
        next.radios
            .iter_mut()
            .find(|p| p.id == radio_id)
            .expect("active profile checked under this Engine borrow")
            .rx_gain = gain;
        self.remote_radio_link(connection)?;
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        next.save(
            self.remote_settings_path
                .as_ref()
                .ok_or(Reason::UnsupportedAction)?,
        )
        .map_err(|_| Reason::PersistenceFailed)?;
        // Persistence is the admission boundary. The normal audio owner reads
        // this value on its next tick; a receipt does not assert live audio.
        self.set_rx_gain(gain);
        self.settings
            .radios
            .iter_mut()
            .find(|p| p.id == radio_id)
            .expect("active profile checked under this Engine borrow")
            .rx_gain = gain;
        Ok(())
    }
}
