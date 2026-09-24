//! Remote split, RIT, XIT and VFO selection.
//!
//! The browser names the value its page displayed and the change it wants. Admission here applies
//! the SAME one-shot verbs the local cockpit buttons call (`request_split`, `request_rit`,
//! `request_xit`, `request_vfo`), which the radio loop writes on its next pass exactly as it does
//! for a local click. Nothing here arms, keys, retunes the dial or saves Settings.
//!
//! Split, XIT and VFO decide where the transmitter goes, so a browser gets two refusals a local
//! click does not: none of them is admitted while the transmitter is enabled, owned, tuning or
//! sending an image, and neither split nor XIT is admitted when the licence would refuse the
//! resulting emission. The desktop's own key-time gate (`tx_allowed`, judging the rig-acknowledged
//! split) is unchanged and still applies to whatever the loop confirms. RIT only moves the receiver.
use super::*;

/// The clarifier range every CAT backend Nexus drives accepts.
const CLARIFIER_LIMIT_HZ: i32 = 9999;

impl Engine {
    /// Shared admission: live authority, the native radio owner, a fresh unkeyed CAT link and no
    /// other radio context a one-shot could silently act beneath.
    fn remote_clarifier_ready(
        &self,
        connection: u64,
        permit: &Permit,
        moves_transmitter: bool,
    ) -> Result<(), Reason> {
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if self.remote_settings_path.is_none() || self.source_kind != crate::dto::SourceKind::Native
        {
            return Err(Reason::UnsupportedAction);
        }
        self.remote_radio_link(connection)?;
        if moves_transmitter
            && (self.tx_enabled()
                || self.tx_owner().is_some()
                || self.tuning()
                || self.sstv_in_flight())
        {
            return Err(Reason::StationBusy);
        }
        if self
            .remote_radio_command
            .as_ref()
            .is_some_and(|r| r.pending())
            || self
                .remote_radio_selection
                .as_ref()
                .is_some_and(|r| r.pending())
        {
            return Err(Reason::StationBusy);
        }
        // A satellite pass owns split for its uplink; APRS, FM channels, parked machinery and a
        // pending retune or radio route each carry their own transaction.
        if self.sat_dial_owner.is_some()
            || self.sat_mode.is_some()
            || self.aprs_fm
            || self.fm_channel
            || self.machinery_park.is_some()
            || self.route_intent.is_some()
            || self.route_target.is_some()
            || self.immediate_retune
            || self.cat_port_hold()
        {
            return Err(Reason::StationBusy);
        }
        Ok(())
    }

    /// The station's own key-time judgement of a transmit carrier — the one `tx_allowed` applies,
    /// Phone and the soundcard sections judged in the mode the rig is actually in — so a split
    /// or XIT is refused here exactly where the station would then refuse to key it.
    fn remote_emission_allowed(&self, mhz: f64) -> bool {
        self.emission_in_use_allowed(mhz)
    }

    pub fn queue_remote_split(
        &mut self,
        expected_tx_mhz: Option<f64>,
        tx_mhz: Option<f64>,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.remote_clarifier_ready(connection, permit, true)?;
        if self.split_dirty {
            return Err(Reason::StationBusy);
        }
        let whole_hz = |mhz: Option<f64>| {
            mhz.map(|m| {
                if m.is_finite() && m > 0.0 && m <= 250_000.0 {
                    Ok((m * 1e6).round() as u64)
                } else {
                    Err(Reason::InvalidAction)
                }
            })
            .transpose()
        };
        let (expected, target) = (whole_hz(expected_tx_mhz)?, whole_hz(tx_mhz)?);
        if expected != self.split_tx_mhz.map(|m| (m * 1e6).round() as u64) {
            return Err(Reason::ContextChanged);
        }
        if expected == target {
            return Err(Reason::InvalidAction);
        }
        if let Some(hz) = target {
            let tx = hz as f64 / 1e6;
            // A manual split stays on the operating band; crossing bands is a QSY.
            if crate::bandplan::band_for_dial(tx) != Some(self.settings.band.as_str()) {
                return Err(Reason::InvalidAction);
            }
            let xit = self.xit_offset_mhz();
            if !self.remote_emission_allowed(tx) || !self.remote_emission_allowed(tx + xit) {
                return Err(Reason::OutsidePrivileges);
            }
        }
        // Clearing a split returns the transmitter to the dial and needs no privilege check.
        self.request_split(target.map(|hz| hz as f64 / 1e6));
        Ok(())
    }

    pub fn queue_remote_xit(
        &mut self,
        expected_hz: i32,
        hz: i32,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.remote_clarifier_ready(connection, permit, true)?;
        // A radio with no XIT (the IC-9700, every radio in `settings::NO_XIT_RIGS`, and any radio
        // behind OmniRig) says so, rather than admitting a change the station's own verb will not
        // make.
        if !self.xit_supported() {
            return Err(Reason::UnsupportedAction);
        }
        if self.xit_dirty {
            return Err(Reason::StationBusy);
        }
        Self::clarifier_change(expected_hz, hz, self.xit_hz)?;
        if hz != 0 {
            // Judge both the frequency the desktop gate judges now and a requested split still
            // awaiting the rig's acknowledgement: the offset rides whichever transmits.
            let offset = f64::from(hz) / 1e6;
            let bases = [Some(self.tx_emission_mhz()), self.split_tx_mhz];
            if bases
                .into_iter()
                .flatten()
                .any(|base| !self.remote_emission_allowed(base + offset))
            {
                return Err(Reason::OutsidePrivileges);
            }
        }
        self.request_xit(hz);
        Ok(())
    }

    pub fn queue_remote_rit(
        &mut self,
        expected_hz: i32,
        hz: i32,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.remote_clarifier_ready(connection, permit, false)?;
        if self.rit_dirty {
            return Err(Reason::StationBusy);
        }
        Self::clarifier_change(expected_hz, hz, self.rit_hz)?;
        self.request_rit(hz);
        Ok(())
    }

    pub fn queue_remote_vfo(
        &mut self,
        expected_vfo_b: bool,
        vfo_b: bool,
        connection: u64,
        permit: &Permit,
    ) -> Result<(), Reason> {
        self.remote_clarifier_ready(connection, permit, true)?;
        // Under a split, selecting the other VFO changes which frequency transmits in a way the
        // station cannot judge before the rig answers.
        if self.vfo_dirty || self.split_tx_mhz.is_some() || self.observed_split.is_some_and(|s| s.0)
        {
            return Err(Reason::StationBusy);
        }
        // Against the EFFECTIVE selection, which is what the browser's page was shown. Comparing
        // the commanded field instead would admit a change judged against a VFO the operator has
        // since moved off at the front panel, and refuse one judged against the VFO on screen.
        if expected_vfo_b != self.active_vfo_b_effective() {
            return Err(Reason::ContextChanged);
        }
        if expected_vfo_b == vfo_b {
            return Err(Reason::InvalidAction);
        }
        self.request_vfo(vfo_b);
        Ok(())
    }

    fn clarifier_change(expected_hz: i32, hz: i32, current: i32) -> Result<(), Reason> {
        let limit = -CLARIFIER_LIMIT_HZ..=CLARIFIER_LIMIT_HZ;
        if !limit.contains(&expected_hz) || !limit.contains(&hz) {
            return Err(Reason::InvalidAction);
        }
        if expected_hz != current {
            return Err(Reason::ContextChanged);
        }
        if expected_hz == hz {
            return Err(Reason::InvalidAction);
        }
        Ok(())
    }
}
