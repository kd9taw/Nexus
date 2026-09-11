//! Native workspace transitions with caller-owned decoder serialization.
//! The ordinary remote radio transaction and a routed handoff execute the same
//! section/area/tier verbs in the same order. Reset callbacks let a handoff keep
//! the existing A7 guard through multiple native QSYs without acquiring it again.
use super::{DecoderMutation, Engine, Tier, Workspace};
use crate::engine::radio_selection::RadioSelection;
use crate::settings::{OperatingMode, RouteMode, Settings};

pub(in crate::engine) struct WorkspacePlan {
    pub selection: RadioSelection,
    pub tier: Tier,
    pub follow_frequency: bool,
    pub routed: bool,
}

impl Engine {
    /// Project each native QSY in order. An intermediate radio may bank a
    /// profile even though only the final radio receives a hardware command.
    pub(in crate::engine) fn prepare_remote_workspace(
        &self,
        workspace: Workspace,
    ) -> WorkspacePlan {
        let follow_frequency =
            workspace != Workspace::Tempo && self.settings.operating_mode != OperatingMode::Digital;
        let mut settings = self.settings.clone();
        settings.operating_mode = OperatingMode::Digital;
        let mut routed = false;
        if let Some((dial, sideband)) = self
            .prepare_mode_entry("digital", follow_frequency)
            .frequency
        {
            let band = settings.band.clone();
            routed |= self.project_workspace_frequency(&mut settings, dial, &band, &sideband);
        } else if settings.sideband.eq_ignore_ascii_case("LSB") {
            settings.sideband = "USB".into();
        }
        // FT entry can restore remembered JS8 through set_area before its
        // explicit FT8 correction. Preserve both channel/profile transitions.
        let first = match workspace {
            Workspace::Ft => self.area_tier("dx"),
            Workspace::Tempo => self.area_tier("msg"),
            Workspace::Js8 => Tier::Js8,
        };
        let second = if workspace == Workspace::Ft && first == Tier::Js8 {
            Tier::Ft8
        } else {
            first
        };
        let mut tier = self.tier();
        for target in [first, second] {
            if target == tier {
                continue;
            }
            tier = target;
            if let Some(channel) =
                self.prepare_tier_frequency_at(tier, &settings.band, settings.dial_mhz)
            {
                routed |= self.project_workspace_frequency(
                    &mut settings,
                    channel.dial_mhz,
                    &channel.band,
                    &channel.mode,
                );
            }
        }
        WorkspacePlan {
            selection: RadioSelection::from_settings(settings),
            tier,
            follow_frequency,
            routed,
        }
    }

    fn project_workspace_frequency(
        &self,
        settings: &mut Settings,
        dial: f64,
        band: &str,
        sideband: &str,
    ) -> bool {
        let band = crate::bandplan::canonical_band(band);
        let mut routed = false;
        if !settings.radio_pegged {
            let id = if band.is_empty() {
                settings.route_radio_bandless(RouteMode::Digital)
            } else {
                settings.route_radio(&band, RouteMode::Digital)
            };
            if let Some(selection) =
                id.and_then(|id| self.preview_radio_selection_from(settings, id))
            {
                *settings = selection.settings().clone();
                routed = true;
            }
        }
        settings.dial_mhz = dial;
        settings.band = band;
        settings.sideband = sideband.into();
        routed
    }

    pub(in crate::engine) fn enter_remote_workspace_with_decoder(
        &mut self,
        workspace: Workspace,
        follow_frequency: bool,
        mut decoder: impl FnMut(&mut Self, DecoderMutation),
        mut reset: impl FnMut(),
    ) {
        self.set_operating_mode_with_reset("digital", follow_frequency, false, &mut reset);
        match workspace {
            Workspace::Ft => {
                self.set_area_with_decoder_and_reset("dx", &mut decoder, &mut reset);
                if self.tier() == Tier::Js8 {
                    self.set_tier_with_installer_and_reset(
                        Tier::Ft8,
                        |engine, source| decoder(engine, DecoderMutation::Install(source)),
                        &mut reset,
                    );
                }
            }
            Workspace::Tempo => {
                self.set_area_with_decoder_and_reset("msg", &mut decoder, &mut reset);
            }
            Workspace::Js8 => {
                self.js8_start_session();
                self.set_tier_with_installer_and_reset(
                    Tier::Js8,
                    |engine, source| decoder(engine, DecoderMutation::Install(source)),
                    &mut reset,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::RoutingRule;

    fn station(final_radio: u32) -> Engine {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.settings.ensure_radio_profiles();
        engine.add_radio();
        engine.add_radio();
        engine.set_active_radio(0);
        engine.settings.radio_pegged = true;
        engine.set_tier(Tier::TempoFast);
        engine.set_operating_mode("phone", false);
        engine.set_frequency(14.250, "20m", "USB");
        engine.set_tx_enabled(false);
        engine.last_dx_tier = Some(Tier::Msk144);
        engine.settings.radio_pegged = false;
        engine.settings.routing_rules = vec![
            RoutingRule {
                bands: vec!["6m".into()],
                mode: Some(RouteMode::Digital),
                radio: final_radio,
                ..RoutingRule::default()
            },
            RoutingRule {
                mode: Some(RouteMode::Digital),
                radio: 1,
                ..RoutingRule::default()
            },
        ];
        engine.take_immediate_retune();
        engine
    }

    #[test]
    fn held_workspace_guards_preserve_native_multi_radio_transition_order() {
        for (workspace, final_radio) in [Workspace::Ft, Workspace::Tempo, Workspace::Js8]
            .into_iter()
            .flat_map(|workspace| [0, 2].map(|radio| (workspace, radio)))
        {
            let mut native = station(final_radio);
            let mut guarded = station(final_radio);
            let before = guarded.settings.clone();
            let plan = guarded.prepare_remote_workspace(workspace);
            assert_eq!(guarded.settings, before);
            let follow = workspace != Workspace::Tempo;
            let section_dial = native
                .prepare_mode_entry("digital", follow)
                .frequency
                .map(|f| f.0);
            native.set_operating_mode("digital", follow);
            match workspace {
                Workspace::Ft => {
                    native.set_area("dx");
                    if native.tier() == Tier::Js8 {
                        native.set_tier(Tier::Ft8);
                    }
                }
                Workspace::Tempo => native.set_area("msg"),
                Workspace::Js8 => {
                    native.js8_start_session();
                    native.set_tier(Tier::Js8);
                }
            }
            assert_eq!(plan.tier, native.tier());
            assert_eq!(plan.follow_frequency, follow);
            assert_eq!(
                plan.selection.settings().active_radio,
                native.settings.active_radio
            );
            assert_eq!(
                plan.selection.settings().dial_hz(),
                native.settings.dial_hz()
            );
            assert_eq!(
                plan.selection.settings().rig_mode(),
                native.settings.rig_mode()
            );
            assert_eq!(plan.selection.settings().radios, native.settings.radios);
            assert_eq!(plan.routed, workspace != Workspace::Tempo);
            let source = guarded.source.clone();
            let mut slot = source.lock().unwrap();
            let mut a7 = loop {
                if let Some(guard) = modes::Ft8A7ResetGuard::try_acquire() {
                    break guard;
                }
                std::thread::yield_now();
            };
            let mut resets = 0;
            let modem = std::cell::RefCell::new(&mut a7);
            guarded.enter_remote_workspace_with_decoder(
                workspace,
                follow,
                |engine, mutation| match mutation {
                    DecoderMutation::Install(source) => {
                        engine.install_source_into(&mut slot, source)
                    }
                    DecoderMutation::ResetHarq => modem.borrow_mut().reset_tempo_harq_held(),
                },
                || {
                    resets += 1;
                    modem.borrow_mut().reset_held();
                },
            );
            assert!(modes::Ft8A7ResetGuard::try_acquire().is_none());
            assert_eq!(guarded.settings, native.settings, "{workspace:?}");
            assert_eq!(guarded.freq_memory, native.freq_memory, "{workspace:?}");
            assert_eq!(guarded.source_label, native.source_label);
            assert_eq!(guarded.tier(), native.tier());
            assert_eq!(guarded.last_dx_tier, native.last_dx_tier);
            assert_eq!(guarded.last_msg_tier, native.last_msg_tier);
            assert_eq!(guarded.decode_epoch, native.decode_epoch);
            assert_eq!(guarded.tx_gate_gen, native.tx_gate_gen);
            assert_eq!(guarded.rx_offset_hz, native.rx_offset_hz);
            assert_eq!(guarded.tx_offset_hz, native.tx_offset_hz);
            assert_eq!(guarded.rf_power, native.rf_power);
            assert_eq!(guarded.immediate_retune, native.immediate_retune);
            assert!(!guarded.tx_enabled());
            assert_eq!(guarded.settings.operating_mode, OperatingMode::Digital);
            if workspace == Workspace::Ft {
                assert_eq!(guarded.settings.active_radio, final_radio);
                assert_eq!(guarded.tier(), Tier::Msk144);
                assert_eq!(guarded.settings.dial_hz(), 50_260_000);
                // The intermediate HF radio was selected by section entry,
                // then banked by the specialty decoder's VHF fallback.
                let intermediate = guarded.settings.radios.iter().find(|r| r.id == 1).unwrap();
                assert_eq!(intermediate.last_dial_mhz, section_dial.unwrap());
                assert!(resets >= 2);
            }
            drop(a7);
            drop(slot);
            assert!(std::sync::Arc::ptr_eq(&source, &guarded.source));
        }
    }
}
