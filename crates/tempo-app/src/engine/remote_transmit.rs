//! Native ownership of an explicitly authorized Remote FT transmission.
//! The existing FT sequencer, slot timing and waveform code remain authoritative.
//! No browser action is exposed by this internal foundation alone.
use super::Engine;
use crate::dto::{SourceKind, Tier};
use crate::remote_control::{transmit::TransmitPermit, Reason};
use crate::settings::OperatingMode;
use std::time::Instant;

/// A clicked decode or roster entry, checked against station-owned data before
/// forwarding the same arguments to the existing native QSO verb.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FtCallSelection {
    pub call: String,
    pub grid: Option<String>,
    pub message: Option<String>,
    pub snr: Option<i32>,
    pub freq: Option<f32>,
}

impl Engine {
    pub fn call_remote_ft_selection(
        &mut self,
        permit: TransmitPermit,
        selection: &FtCallSelection,
    ) -> Result<(), Reason> {
        self.prepare_remote_ft(&permit)?;
        let FtCallSelection {
            call,
            grid,
            message,
            snr,
            freq,
        } = selection;
        if call.len() > 32
            || !tempo_core::message::is_callsign(call)
            || grid.as_ref().is_some_and(|g| g.len() > 16)
            || freq.is_some_and(|f| !f.is_finite())
        {
            return Err(Reason::InvalidAction);
        }
        if let Some(message) = message {
            // Do not accept fabricated report/message context from a stale UI.
            // The native verb still resolves its own slot/parity and QSO state.
            if message.len() > 128 || grid.is_some() || snr.is_none() || freq.is_none() {
                return Err(Reason::InvalidAction);
            }
            let matched = self.decode_history.iter().rev().any(|(_, d)| {
                d.message == *message
                    && Some(d.snr) == *snr
                    && Some(d.freq) == *freq
                    && tempo_core::message::Msg::parse(&d.message)
                        .sender()
                        .is_some_and(|sender| tempo_core::message::same_call(sender, call))
            });
            if !matched {
                return Err(Reason::ContextChanged);
            }
        } else {
            if snr.is_some() {
                return Err(Reason::InvalidAction);
            }
            let matched = self.snapshot().stations.iter().any(|s| {
                tempo_core::message::same_call(&s.call, call)
                    && s.grid == *grid
                    && s.freq_hz.map(|hz| hz as f32) == *freq
                    && s.tier.is_none_or(|tier| tier == self.tier())
            });
            if !matched {
                return Err(Reason::ContextChanged);
            }
        }
        self.call_remote_ft_station(
            permit,
            call,
            grid.as_deref(),
            message.as_deref(),
            *snr,
            *freq,
        )
    }

    pub fn remote_ft_available(&self) -> bool {
        self.source_kind == SourceKind::Native
            && self.settings.operating_mode == OperatingMode::Digital
            && matches!(self.tier(), Tier::Ft8 | Tier::Ft4)
    }

    pub fn remote_ft_tx_owned(&self) -> bool {
        self.tx_enabled()
            && self
                .remote_transmit
                .as_ref()
                .is_some_and(|p| p.valid(Instant::now()))
    }

    /// Arming needs fresh readings from the displayed active radio. TX Off and
    /// Stop use ownership alone so missing CAT cannot prevent their release.
    pub fn validate_remote_ft_radio(
        &self,
        tier: Tier,
        connection: u64,
        permit: &TransmitPermit,
    ) -> Result<(), Reason> {
        if self.tier() != tier {
            return Err(Reason::ContextChanged);
        }
        self.prepare_remote_ft(permit)?;
        let o = self.remote_monitor_observation();
        let cat = o.radio.readings.cat.ok_or(Reason::ReadingUnavailable)?;
        let ptt = o.radio.readings.ptt.ok_or(Reason::ReadingUnavailable)?;
        let dial = o.radio.readings.dial.ok_or(Reason::ReadingUnavailable)?;
        let mode = o.radio.readings.mode.ok_or(Reason::ReadingUnavailable)?;
        if [cat, ptt, dial, mode]
            .iter()
            .any(|r| r.connection_generation != connection)
        {
            return Err(Reason::ContextChanged);
        }
        if o.radio.cat_connected != Some(true)
            || cat.age_ms >= 1000
            || ptt.age_ms >= 1000
            || dial.age_ms >= 1000
            || mode.age_ms >= 10000
        {
            return Err(Reason::ReadingUnavailable);
        }
        if o.radio.rig_keyed != Some(false)
            && !(o.radio.rig_keyed == Some(true)
                && self
                    .remote_transmit
                    .as_ref()
                    .is_some_and(|p| p.valid(Instant::now())))
        {
            return Err(Reason::StationBusy);
        }
        Ok(())
    }

    /// Begin CQ using exactly the native operating verb. Existing local arming
    /// is not permission for a remote browser to take over that transmission.
    /// The host still owns device approval, command context and hardware checks.
    pub fn start_remote_ft_cq(
        &mut self,
        permit: TransmitPermit,
        direction: Option<&str>,
    ) -> Result<(), Reason> {
        self.prepare_remote_ft(&permit)?;
        self.start_cq(direction)
            .map_err(|_| Reason::InvalidAction)?;
        // Native entry may spend time resetting a decoder. Recheck before any
        // future TX plan can observe the newly armed state.
        if !permit.valid(Instant::now()) {
            self.halt_tx();
            return Err(Reason::AuthorityExpired);
        }
        self.remote_transmit = Some(permit);
        Ok(())
    }

    fn require_remote_ft_owner(&self, permit: &TransmitPermit) -> Result<(), Reason> {
        let now = Instant::now();
        if !permit.valid(now) {
            return Err(Reason::AuthorityExpired);
        }
        let current = self.remote_transmit.as_ref().ok_or(Reason::StationBusy)?;
        if !current.valid(now) {
            return Err(Reason::AuthorityExpired);
        }
        if !current.same_session(permit) {
            return Err(Reason::ContextChanged);
        }
        Ok(())
    }

    fn prepare_remote_ft(&self, permit: &TransmitPermit) -> Result<(), Reason> {
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if self.source_kind != SourceKind::Native
            || self.settings.operating_mode != OperatingMode::Digital
            || !matches!(self.tier(), Tier::Ft8 | Tier::Ft4)
        {
            return Err(Reason::UnsupportedAction);
        }
        if self.remote_transmit.is_some() {
            self.require_remote_ft_owner(permit)?;
            if self
                .tx_owner()
                .is_some_and(|owner| owner != super::TxOwner::Slot)
                || self.sstv_in_flight()
            {
                return Err(Reason::StationBusy);
            }
            self.remote_radio_context_idle()
        } else {
            self.remote_radio_idle()
        }
    }

    /// The native call verb owns message choice, parity, offsets and QSO state.
    /// A browser-facing caller must bind its selected decode to station data.
    pub fn call_remote_ft_station(
        &mut self,
        permit: TransmitPermit,
        call: &str,
        grid: Option<&str>,
        message: Option<&str>,
        snr: Option<i32>,
        frequency: Option<f32>,
    ) -> Result<(), Reason> {
        self.prepare_remote_ft(&permit)?;
        // Match the desktop command's entry validation before the native QSO
        // verb can change its target or arm TX. Keying guards remain in place.
        self.structured_tx_ready(true)
            .map_err(|_| Reason::InvalidAction)?;
        let grid = grid.map(str::trim).filter(|s| !s.is_empty());
        let message = message.map(str::trim).filter(|s| !s.is_empty());
        let frequency = frequency.filter(|f| *f > 0.0);
        self.call_station_ctx(call, grid, message, snr, frequency)
            .map_err(|_| Reason::InvalidAction)?;
        if !permit.valid(Instant::now()) {
            self.halt_tx();
            return Err(Reason::AuthorityExpired);
        }
        self.remote_transmit = Some(permit);
        Ok(())
    }

    pub fn set_remote_ft_tx_enabled(
        &mut self,
        permit: TransmitPermit,
        on: bool,
    ) -> Result<(), Reason> {
        if on {
            self.prepare_remote_ft(&permit)?;
            self.set_tx_enabled(true);
            self.remote_transmit = Some(permit);
            if self.poll_remote_transmit(Instant::now()) {
                return Err(Reason::AuthorityExpired);
            }
        } else {
            // TX Off must work during our pending retune or current FT over.
            // Native semantics finish that over; they do not invoke Stop TX.
            self.require_remote_ft_owner(&permit)?;
            self.set_tx_enabled(false);
        }
        Ok(())
    }

    pub fn stop_remote_ft(&mut self, permit: &TransmitPermit) -> Result<(), Reason> {
        // Stop needs ownership, not a fresh CAT reading or an idle transmitter.
        // It must never stop a later native transmission after local takeover.
        self.require_remote_ft_owner(permit)?;
        self.halt_tx();
        Ok(())
    }

    pub fn renew_remote_transmit(&mut self, permit: TransmitPermit, now: Instant) -> bool {
        self.remote_transmit
            .as_mut()
            .is_some_and(|current| current.renew(permit, now))
    }

    /// Called by the real radio loop and at both FT planning and commit. A
    /// disconnected host cannot leave the station waiting for another request
    /// to notice expiry. Native halt supplies the existing flush/unkey signals.
    pub fn poll_remote_transmit(&mut self, now: Instant) -> bool {
        if self.remote_transmit.as_ref().is_some_and(|p| !p.valid(now)) {
            self.halt_tx();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_control::transmit::TransmitAuthority;
    use std::time::Duration;

    fn station(tier: Tier) -> Engine {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.set_tier(tier);
        engine.configure_remote_settings_store(
            std::env::temp_dir().join("nexus-remote-ft-authority-test.json"),
        );
        engine.set_tx_enabled(false);
        engine.take_immediate_retune();
        engine.take_slot_tx_abort();
        engine
    }

    #[test]
    fn remote_cq_preserves_native_ft_state_and_expiry_uses_the_existing_halt() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = station(tier);
            let mut remote = station(tier);
            let authority = TransmitAuthority::default();
            let now = Instant::now();
            let deadline = now + Duration::from_secs(5);
            native.start_cq(Some("DX")).unwrap();
            remote
                .start_remote_ft_cq(authority.permit(deadline).unwrap(), Some("DX"))
                .unwrap();
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
            assert_eq!(remote.tx_even(), native.tx_even());
            assert_eq!(remote.tx_gate_gen, native.tx_gate_gen);
            assert_eq!(remote.immediate_tx, native.immediate_tx);
            assert_eq!(remote.immediate_retune, native.immediate_retune);
            assert!(remote.tx_enabled());
            assert!(!remote.poll_remote_transmit(now));
            assert!(remote.poll_remote_transmit(deadline));
            assert!(!remote.tx_enabled());
            assert!(remote.take_slot_tx_abort());
            assert!(!remote.immediate_tx);
            assert!(!remote.renew_remote_transmit(
                authority.permit(deadline + Duration::from_secs(5)).unwrap(),
                deadline
            ));
            assert!(!remote.poll_remote_transmit(deadline));
        }
    }

    #[test]
    fn revoked_authority_cannot_commit_a_previously_planned_ft_over() {
        let mut engine = station(Tier::Ft8);
        let authority = TransmitAuthority::default();
        let permit = authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap();
        engine.start_remote_ft_cq(permit, None).unwrap();
        let slot = if engine.tx_even() { 0 } else { 1 };
        let plan = engine
            .plan_tx(slot)
            .expect("positive control: native CQ plans an over");
        authority.revoke();
        assert!(engine.commit_tx(&plan, vec![0.0; 120], slot).is_empty());
        assert!(!engine.tx_enabled());
        assert!(engine.take_slot_tx_abort());
    }

    #[test]
    fn remote_ft_tx_off_and_stop_preserve_the_native_distinction() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = station(tier);
            let mut remote = station(tier);
            let authority = TransmitAuthority::default();
            let permit = || {
                authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap()
            };
            native.start_cq(None).unwrap();
            remote.start_remote_ft_cq(permit(), None).unwrap();
            native.set_tx_enabled(false);
            remote.set_remote_ft_tx_enabled(permit(), false).unwrap();
            assert!(!native.take_slot_tx_abort());
            assert!(
                !remote.take_slot_tx_abort(),
                "TX Off lets the current FT over finish"
            );
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
            native.take_immediate_retune();
            remote.take_immediate_retune();
            native.set_tx_enabled(true);
            remote.set_remote_ft_tx_enabled(permit(), true).unwrap();
            native.halt_tx();
            remote.stop_remote_ft(&permit()).unwrap();
            assert!(remote.take_slot_tx_abort());
            assert!(native.take_slot_tx_abort());
            assert!(!remote.tx_enabled());
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
        }
    }

    #[test]
    fn remote_ft_call_uses_native_qso_state_and_keeps_ownership_after_rearm() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = station(tier);
            let mut remote = station(tier);
            let authority = TransmitAuthority::default();
            let permit = || {
                authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap()
            };
            native
                .call_station_ctx("W1AW", Some("FN31"), None, None, Some(950.0))
                .unwrap();
            remote
                .call_remote_ft_station(permit(), "W1AW", Some("FN31"), None, None, Some(950.0))
                .unwrap();
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
            assert_eq!(remote.tx_even(), native.tx_even());
            assert_eq!(remote.tx_offset_hz, native.tx_offset_hz);
            assert_eq!(remote.rx_offset_hz, native.rx_offset_hz);
            assert_eq!(remote.immediate_tx, native.immediate_tx);
            assert!(remote.tx_enabled());
            authority.revoke();
            assert!(remote.poll_remote_transmit(Instant::now()));
            assert!(!remote.tx_enabled());
        }
    }

    #[test]
    fn remote_call_checks_desktop_identity_before_changing_qso() {
        for invalid in ["call", "grid"] {
            let mut engine = station(Tier::Ft8);
            if invalid == "call" {
                engine.settings.mycall.clear();
            } else {
                engine.settings.mygrid.clear();
            }
            assert!(engine.structured_tx_ready(true).is_err());
            let before = engine.snapshot().qso;
            let authority = TransmitAuthority::default();
            assert_eq!(
                engine.call_remote_ft_station(
                    authority
                        .permit(Instant::now() + Duration::from_secs(5))
                        .unwrap(),
                    "W1AW",
                    Some("FN31"),
                    None,
                    None,
                    Some(950.0),
                ),
                Err(Reason::InvalidAction),
                "{invalid}"
            );
            assert_eq!(engine.snapshot().qso, before);
            assert!(!engine.tx_enabled());
        }
    }

    #[test]
    fn remote_call_preserves_manual_arming_preference() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = station(tier);
            let mut remote = station(tier);
            native.settings.double_click_sets_tx = false;
            remote.settings.double_click_sets_tx = false;
            native
                .call_station_ctx("W1AW", Some("FN31"), None, None, Some(950.0))
                .unwrap();
            let authority = TransmitAuthority::default();
            remote
                .call_remote_ft_station(
                    authority
                        .permit(Instant::now() + Duration::from_secs(5))
                        .unwrap(),
                    "W1AW",
                    Some("FN31"),
                    None,
                    None,
                    Some(950.0),
                )
                .unwrap();
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
            assert_eq!(remote.tx_even(), native.tx_even());
            assert_eq!(remote.rx_offset_hz, native.rx_offset_hz);
            assert_eq!(remote.tx_offset_hz, native.tx_offset_hz);
            assert!(!remote.tx_enabled());
            assert!(!remote.remote_ft_tx_owned());
        }
    }

    #[test]
    fn remote_selected_decode_uses_native_message_parity_and_offsets() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = station(tier);
            let mut remote = station(tier);
            let decode = modes::Decode {
                message: "KD9TAW W1AW -07".into(),
                sync: 1.0,
                snr: -12,
                dt: 0.1,
                freq: 950.0,
                nap: 0,
                qual: 1.0,
                rv: None,
                mode: None,
                raw: None,
            };
            native.ingest_decodes_for_test(std::slice::from_ref(&decode), 8);
            remote.ingest_decodes_for_test(std::slice::from_ref(&decode), 8);
            let selection = FtCallSelection {
                call: "W1AW".into(),
                grid: None,
                message: Some(decode.message.clone()),
                snr: Some(decode.snr),
                freq: Some(decode.freq),
            };
            let authority = TransmitAuthority::default();
            let permit = || {
                authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap()
            };
            for cause in ["message", "snr", "frequency", "call"] {
                let mut changed = selection.clone();
                match cause {
                    "message" => changed.message = Some("KD9TAW W1AW R-07".into()),
                    "snr" => changed.snr = Some(-3),
                    "frequency" => changed.freq = Some(1000.0),
                    "call" => changed.call = "K2ABC".into(),
                    _ => unreachable!(),
                }
                assert_eq!(
                    remote.call_remote_ft_selection(permit(), &changed),
                    Err(Reason::ContextChanged),
                    "{cause}"
                );
                assert!(!remote.tx_enabled());
            }
            native
                .call_station_ctx(
                    "W1AW",
                    None,
                    Some(&decode.message),
                    Some(decode.snr),
                    Some(decode.freq),
                )
                .unwrap();
            remote
                .call_remote_ft_selection(permit(), &selection)
                .unwrap();
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
            assert_eq!(remote.tx_even(), native.tx_even());
            assert_eq!(remote.tx_offset_hz, native.tx_offset_hz);
            assert_eq!(remote.rx_offset_hz, native.rx_offset_hz);
            assert_eq!(remote.immediate_tx, native.immediate_tx);
            assert!(remote.remote_ft_tx_owned());
        }
    }

    #[test]
    fn remote_selected_roster_and_retired_decode_context() {
        let mut remote = station(Tier::Ft8);
        let decode = modes::Decode {
            message: "CQ W1AW FN31".into(),
            sync: 1.0,
            snr: -10,
            dt: 0.1,
            freq: 1250.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
            raw: None,
        };
        remote.ingest_decodes_for_test(std::slice::from_ref(&decode), 8);
        let station = remote
            .snapshot()
            .stations
            .into_iter()
            .find(|s| s.call == "W1AW")
            .unwrap();
        let selection = FtCallSelection {
            call: station.call,
            grid: station.grid,
            message: None,
            snr: None,
            freq: station.freq_hz.map(|hz| hz as f32),
        };
        let authority = TransmitAuthority::default();
        let permit = || {
            authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap()
        };
        remote
            .call_remote_ft_selection(permit(), &selection)
            .unwrap();
        assert!(remote.remote_ft_tx_owned());
        remote.halt_tx();
        remote.clear_decode_context();
        remote.take_immediate_retune();
        let old = FtCallSelection {
            call: "W1AW".into(),
            grid: None,
            message: Some(decode.message),
            snr: Some(-10),
            freq: Some(1250.0),
        };
        assert_eq!(
            remote.call_remote_ft_selection(permit(), &old),
            Err(Reason::ContextChanged)
        );
        assert!(!remote.tx_enabled());
    }

    #[test]
    fn another_remote_owner_and_old_session_cannot_stop_native_or_current_ft() {
        let mut remote = station(Tier::Ft8);
        let authority = TransmitAuthority::default();
        let other = TransmitAuthority::default();
        let permit = authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap();
        remote.start_remote_ft_cq(permit.clone(), None).unwrap();
        let foreign = other
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(remote.stop_remote_ft(&foreign), Err(Reason::ContextChanged));
        assert!(remote.tx_enabled());
        remote.set_tx_enabled(true);
        assert_eq!(remote.stop_remote_ft(&permit), Err(Reason::StationBusy));
        assert_eq!(
            remote.set_remote_ft_tx_enabled(permit, false),
            Err(Reason::StationBusy)
        );
        assert!(remote.tx_enabled());
    }

    #[test]
    fn native_rearm_takes_ownership_and_cannot_be_stopped_by_an_old_remote_session() {
        let mut engine = station(Tier::Ft8);
        let authority = TransmitAuthority::default();
        engine
            .start_remote_ft_cq(
                authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
                None,
            )
            .unwrap();
        engine.set_tx_enabled(true);
        authority.revoke();
        assert!(!engine.poll_remote_transmit(Instant::now()));
        assert!(engine.tx_enabled());
        assert!(!engine.renew_remote_transmit(
            authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
            Instant::now()
        ));
    }
}
