//! Native radio-selection policy shared by local handoff and Remote preparation.
//! A preview owns only a temporary Settings projection. It changes no live engine,
//! decoder, dial memory or hardware and is neither authorization nor readback.
//! Never apply the projected Settings wholesale: the native handoff owns its
//! context retirement, dial banking and current-profile synchronization.
use super::{Engine, RadioLive};
use crate::settings::Settings;

pub struct RadioSelection {
    settings: Settings,
}

impl RadioSelection {
    /// Local owner input for transport/tune preparation; never a browser DTO.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

impl Engine {
    /// Resolve exactly the profile and monitored-tune policy used by local
    /// selection, without executing a selection. Recompute at commit: a monitor
    /// read, profile edit or local gesture can supersede this projection.
    pub fn preview_radio_selection(&self, id: u32) -> Option<RadioSelection> {
        if id == self.settings.active_radio || !self.settings.radios.iter().any(|p| p.id == id) {
            return None;
        }
        let mut settings = self.settings.clone();
        bank_outgoing_profile(&mut settings);
        settings.active_radio = id;
        settings.sync_flat_from_active();
        incoming_tune(&settings, self.radio_live.get(&id)).apply(&mut settings);
        Some(RadioSelection { settings })
    }
}

pub(super) fn bank_outgoing_profile(settings: &mut Settings) {
    // Preserve unsaved flat CAT/audio edits on the outgoing profile, then
    // de-conflict daemon ports before the incoming profile is mirrored.
    settings.sync_active_from_flat();
    settings.ensure_distinct_radio_ports();
    let current = settings.active_radio;
    if let Some(profile) = settings.radios.iter_mut().find(|p| p.id == current) {
        profile.last_dial_mhz = settings.dial_mhz;
        profile.last_band = settings.band.clone();
        profile.last_sideband = settings.sideband.clone();
    }
}

pub(super) struct Tune {
    pub dial_mhz: f64,
    pub band: String,
    pub sideband: String,
    pub monitored: bool,
}

impl Tune {
    pub fn apply(&self, settings: &mut Settings) {
        settings.dial_mhz = self.dial_mhz;
        settings.band = self.band.clone();
        settings.sideband = self.sideband.clone();
    }
}

pub(super) fn incoming_tune(settings: &Settings, live: Option<&RadioLive>) -> Tune {
    if let Some(live) = live.filter(|l| l.dial_mhz.is_some()) {
        let dial_mhz = live.dial_mhz.expect("filtered monitored dial");
        return Tune {
            dial_mhz,
            band: live
                .band
                .as_ref()
                .filter(|b| !b.is_empty())
                .cloned()
                .or_else(|| crate::bandplan::band_for_dial(dial_mhz).map(str::to_string))
                .unwrap_or_else(|| settings.band.clone()),
            sideband: live
                .sideband
                .clone()
                .unwrap_or_else(|| settings.sideband.clone()),
            monitored: true,
        };
    }
    if let Some(profile) = settings
        .active_profile()
        .filter(|p| !p.last_band.is_empty())
    {
        return Tune {
            dial_mhz: profile.last_dial_mhz,
            band: profile.last_band.clone(),
            sideband: profile.last_sideband.clone(),
            monitored: false,
        };
    }
    Tune {
        dial_mhz: settings.dial_mhz,
        band: settings.band.clone(),
        sideband: settings.sideband.clone(),
        monitored: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station() -> (Engine, u32) {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.settings.ensure_radio_profiles();
        let incoming = engine.add_radio();
        engine.set_active_radio(0);
        engine.settings.audio_in = "outgoing-live-input".into();
        engine.settings.amp_follow_band = true;
        engine.settings.dial_mhz = 7.123;
        engine.settings.band = "40m".into();
        engine.settings.sideband = "LSB".into();
        let profile = engine
            .settings
            .radios
            .iter_mut()
            .find(|p| p.id == incoming)
            .unwrap();
        profile.audio_in = "incoming-input".into();
        profile.amp_follow_band = false;
        profile.last_dial_mhz = 14.225;
        profile.last_band = "20m".into();
        profile.last_sideband = "USB".into();
        (engine, incoming)
    }

    #[test]
    fn preview_is_passive_and_native_selection_preserves_both_profiles() {
        let (mut engine, incoming) = station();
        let before = serde_json::to_value(engine.settings()).unwrap();
        let retune = engine.immediate_retune;
        let selection = engine.preview_radio_selection(incoming).unwrap();
        assert_eq!(serde_json::to_value(engine.settings()).unwrap(), before);
        assert_eq!(engine.immediate_retune, retune);
        assert_eq!(selection.settings().dial_mhz, 14.225);
        assert_eq!(selection.settings().audio_in, "incoming-input");
        let outgoing = selection
            .settings()
            .radios
            .iter()
            .find(|p| p.id == 0)
            .unwrap();
        assert_eq!(outgoing.audio_in, "outgoing-live-input");
        assert!(outgoing.amp_follow_band);
        assert_eq!(outgoing.last_dial_mhz, 7.123);
        engine.set_active_radio(incoming);
        assert_eq!(
            serde_json::to_value(engine.settings()).unwrap(),
            serde_json::to_value(selection.settings()).unwrap()
        );
        assert!(engine.immediate_retune);
        assert!(!engine.settings.amp_follow_band);
    }

    #[test]
    fn monitor_tune_precedes_saved_profile_with_native_fallbacks() {
        for (dial, band, expected_band, sideband, expected_sideband) in [
            (50.125, Some("6m"), "6m", Some("USB"), "USB"),
            (50.125, Some(""), "6m", None, "LSB"),
            (50.125, None, "6m", Some("FM"), "FM"),
            (1.0, None, "40m", None, "LSB"),
        ] {
            let (mut engine, incoming) = station();
            engine.radio_live.insert(
                incoming,
                RadioLive {
                    dial_mhz: Some(dial),
                    band: band.map(str::to_owned),
                    sideband: sideband.map(str::to_owned),
                    ..Default::default()
                },
            );
            let selection = engine.preview_radio_selection(incoming).unwrap();
            assert_eq!(selection.settings().dial_mhz, dial);
            assert_eq!(selection.settings().band, expected_band);
            assert_eq!(selection.settings().sideband, expected_sideband);
            engine.set_active_radio(incoming);
            assert_eq!(
                serde_json::to_value(engine.settings()).unwrap(),
                serde_json::to_value(selection.settings()).unwrap()
            );
        }
    }

    #[test]
    fn missing_monitor_dial_and_empty_profile_keep_native_fallback() {
        let (mut engine, incoming) = station();
        engine.radio_live.insert(
            incoming,
            RadioLive {
                band: Some("2m".into()),
                sideband: Some("FM".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            engine
                .preview_radio_selection(incoming)
                .unwrap()
                .settings()
                .dial_mhz,
            14.225
        );
        engine
            .settings
            .radios
            .iter_mut()
            .find(|p| p.id == incoming)
            .unwrap()
            .last_band
            .clear();
        let selection = engine.preview_radio_selection(incoming).unwrap();
        assert_eq!(selection.settings().dial_mhz, 7.123);
        assert_eq!(selection.settings().band, "40m");
        assert_eq!(selection.settings().sideband, "LSB");
        engine.set_active_radio(incoming);
        assert_eq!(
            serde_json::to_value(engine.settings()).unwrap(),
            serde_json::to_value(selection.settings()).unwrap()
        );
    }

    #[test]
    fn preview_deconflicts_ports_without_mutating_live_settings() {
        let (mut engine, incoming) = station();
        let outgoing_port = engine.settings.rigctld_port;
        engine
            .settings
            .radios
            .iter_mut()
            .find(|p| p.id == incoming)
            .unwrap()
            .rigctld_port = outgoing_port;
        let before = engine.settings.clone();
        let selection = engine.preview_radio_selection(incoming).unwrap();
        assert_eq!(engine.settings, before);
        let ports: std::collections::HashSet<_> = selection
            .settings()
            .radios
            .iter()
            .map(|p| p.rigctld_port)
            .collect();
        assert_eq!(ports.len(), selection.settings().radios.len());
        assert_eq!(
            selection.settings().rigctld_port,
            selection.settings().active_profile().unwrap().rigctld_port
        );
        engine.set_active_radio(incoming);
        assert_eq!(&engine.settings, selection.settings());
    }

    #[test]
    fn no_op_and_unknown_radio_do_not_prepare_or_change_settings() {
        let (mut engine, _) = station();
        let before = serde_json::to_value(engine.settings()).unwrap();
        for id in [0, u32::MAX] {
            assert!(engine.preview_radio_selection(id).is_none());
            engine.set_active_radio(id);
            assert_eq!(serde_json::to_value(engine.settings()).unwrap(), before);
        }
    }

    fn decoder_guard() -> modes::Ft8A7ResetGuard {
        loop {
            if let Some(guard) = modes::Ft8A7ResetGuard::try_acquire() {
                return guard;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn guarded_selection_keeps_native_profiles_context_and_stop_effects() {
        let (mut local, incoming) = station();
        let (mut guarded, guarded_incoming) = station();
        assert_eq!(incoming, guarded_incoming);
        for engine in [&mut local, &mut guarded] {
            engine.aprs_fm = true;
            engine.fm_channel = true;
            engine.sideband_override = Some("LSB".into());
            engine.observed_split = Some((true, Some(7_124_000), 0));
            engine.split_tx_mhz = Some(7.124);
            engine.immediate_retune = false;
            engine.slot_tx_abort = false;
            engine.cw_abort = false;
            engine.rtty_abort = false;
            engine.psk_abort = false;
            engine.sstv_abort = false;
        }
        let epoch = guarded.decode_epoch;
        let generation = guarded.tx_gate_gen;
        local.set_active_radio(incoming);
        let guard = decoder_guard();
        assert!(modes::Ft8A7ResetGuard::try_acquire().is_none());
        guarded.set_active_radio_with_decoder_guard(incoming, guard);
        assert_eq!(guarded.settings, local.settings);
        assert_eq!(guarded.decode_epoch, epoch.wrapping_add(1));
        assert_eq!(guarded.decode_epoch, local.decode_epoch);
        assert_eq!(guarded.tx_gate_gen, generation.wrapping_add(1));
        assert_eq!(guarded.tx_gate_gen, local.tx_gate_gen);
        assert!(!guarded.tx_enabled());
        assert!(!guarded.aprs_fm && !guarded.fm_channel);
        assert!(guarded.sideband_override.is_none());
        assert!(guarded.split_tx_mhz.is_none() && guarded.observed_split.is_none());
        assert!(guarded.split_dirty && guarded.immediate_retune);
        assert!(guarded.slot_tx_abort && guarded.cw_abort && guarded.rtty_abort);
        assert!(guarded.psk_abort && guarded.sstv_abort);
        // A complete reverse switch proves the consumed guard was released,
        // and retains the outgoing radio's banked live edits.
        guarded.set_active_radio_with_decoder_guard(0, decoder_guard());
        local.set_active_radio(0);
        assert_eq!(guarded.settings, local.settings);
        assert_eq!(guarded.settings.audio_in, "outgoing-live-input");
        assert!(guarded.settings.amp_follow_band);
    }

    #[test]
    fn guarded_no_op_does_not_retire_context_or_reset_decoder() {
        let (mut engine, _) = station();
        let before = engine.settings.clone();
        let (epoch, generation) = (engine.decode_epoch, engine.tx_gate_gen);
        for id in [0, u32::MAX] {
            engine.set_active_radio_with_reset(id, || panic!("no-op must not reset FT8"));
            engine.set_active_radio_with_decoder_guard(id, decoder_guard());
            assert_eq!(engine.settings, before);
            assert_eq!(engine.decode_epoch, epoch);
            assert_eq!(engine.tx_gate_gen, generation);
        }
    }
}
