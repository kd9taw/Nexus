//! Resolve an operating-section destination without changing the live station.
//!
//! Both the native setter and future Remote radio transactions must use this
//! decision. Preparation cannot bank a memory, arm TX, change a decoder or leave
//! a target for the radio loop to apply after a Remote command has expired.
//! This is the section/frequency decision only, not a CAT permission or a claim
//! that a hardware operation has completed.

use super::Engine;
use crate::settings::OperatingMode;

#[derive(Debug, PartialEq)]
pub(super) struct ModeEntry {
    pub mode: OperatingMode,
    pub frequency: Option<(f64, String)>,
}

impl Engine {
    pub(super) fn prepare_mode_entry(&self, mode: &str, follow_frequency: bool) -> ModeEntry {
        let mode = match mode.to_ascii_lowercase().as_str() {
            "phone" => OperatingMode::Phone,
            "cw" => OperatingMode::Cw,
            "rtty" => OperatingMode::Rtty,
            "keyboard" => OperatingMode::Keyboard,
            _ => OperatingMode::Digital,
        };
        let frequency = if follow_frequency && self.sat_dial_owner.is_none() {
            let band = &self.settings.band;
            // The setter banks the outgoing residency before restoring a cell.
            // On same-mode entry that would replace the cell being read. Peek
            // at that exact replacement without consuming or banking it here.
            let remembered = match self.dial_residency.as_ref() {
                Some(r) if !r.band.is_empty() && &r.band == band && r.mode == mode => {
                    self.resolve_dial_memory(band, mode, r.dial_mhz, r.sideband.clone())
                }
                _ => self.recall_dial_memory(band, mode),
            };
            remembered.or_else(|| self.mode_home(mode))
        } else {
            None
        };
        ModeEntry { mode, frequency }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn station() -> Engine {
        Engine::with_settings(Settings::default())
    }

    #[test]
    fn preparation_does_not_bank_the_dial_or_change_any_live_control() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(14.240, "20m", "USB");
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        let settings = serde_json::to_value(e.settings()).unwrap();
        let generation = e.tx_gate_gen;
        let actuation = e.remote_actuation_context_generation();
        let memories = e.freq_memory.clone();
        let residency = e.dial_residency.as_ref().unwrap().dial_mhz;

        for _ in 0..3 {
            let entry = e.prepare_mode_entry("cw", true);
            assert_eq!(entry.mode, OperatingMode::Cw);
            assert_eq!(entry.frequency.as_ref().unwrap().0, 14.030);
        }

        assert_eq!(serde_json::to_value(e.settings()).unwrap(), settings);
        assert_eq!(e.tx_gate_gen, generation);
        assert_eq!(e.remote_actuation_context_generation(), actuation);
        assert_eq!(e.freq_memory, memories);
        assert_eq!(e.dial_residency.as_ref().unwrap().dial_mhz, residency);
        assert!(!e.tx_enabled());
        assert!(!e.take_immediate_retune());

        // Positive control: executing the native gesture does bank the outgoing
        // dial, assert the new mode and retain native manual-mode arming.
        e.set_operating_mode("cw", true);
        assert_eq!(e.settings().dial_mhz, 14.030);
        assert_eq!(
            e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
            14.240
        );
        assert!(e.tx_enabled());
        assert!(e.take_immediate_retune());
    }

    #[test]
    fn same_mode_entry_uses_the_pending_residency_instead_of_an_older_cell() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(14.240, "20m", "USB");
        e.bank_dial_memory();
        e.set_frequency(14.285, "20m", "USB");
        let entry = e.prepare_mode_entry("phone", true);
        assert_eq!(entry.frequency, Some((14.285, "USB".into())));
        assert_eq!(
            e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
            14.240
        );
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 14.285);
    }

    #[test]
    fn preparation_and_native_entry_restore_the_same_mode_memories() {
        let mut e = station();
        for (mode, dial, sideband) in [
            ("phone", 14.240, "USB"),
            ("cw", 14.055, "USB"),
            ("rtty", 14.090, "LSB"),
            ("keyboard", 14.071, "USB"),
            ("digital", 14.074, "USB"),
        ] {
            e.set_operating_mode(mode, false);
            e.set_frequency(dial, "20m", sideband);
        }
        for (mode, expected) in [
            ("phone", 14.240),
            ("cw", 14.055),
            ("rtty", 14.090),
            ("keyboard", 14.071),
            ("digital", 14.074),
        ] {
            let entry = e.prepare_mode_entry(mode, true);
            assert_eq!(entry.frequency.as_ref().unwrap().0, expected, "{mode}");
            e.set_operating_mode(mode, true);
            assert_eq!(e.settings().operating_mode, entry.mode);
            assert_eq!(e.settings().dial_mhz, expected, "{mode}");
            assert_eq!(e.settings().sideband, entry.frequency.unwrap().1);
        }
    }

    #[test]
    fn no_follow_or_a_held_pass_never_prepares_a_frequency_change() {
        let mut e = station();
        e.set_frequency(14.240, "20m", "LSB");
        assert!(e.prepare_mode_entry("digital", false).frequency.is_none());
        e.sat_dial_owner = Some(("test-pass".into(), 0));
        assert!(e.prepare_mode_entry("digital", true).frequency.is_none());
        e.set_operating_mode("digital", true);
        assert_eq!(e.settings().dial_mhz, 14.240);
        assert_eq!(e.settings().sideband, "LSB");
        assert!(e.sat_dial_owner.is_some());
    }

    #[test]
    fn a_pending_cell_that_lost_privilege_replaces_an_older_valid_cell() {
        use crate::settings::LicenseClass;
        let mut e = station();
        e.settings.license_class = LicenseClass::Extra;
        e.set_operating_mode("phone", false);
        e.set_frequency(14.240, "20m", "USB");
        e.bank_dial_memory();
        e.set_frequency(14.160, "20m", "USB");
        e.settings.license_class = LicenseClass::General;

        // Native entry banks 14.160 OVER 14.240, refuses that remembered
        // emission under the new class, then uses the General phone home.
        let entry = e.prepare_mode_entry("phone", true);
        assert_eq!(entry.frequency, Some((14.225, "USB".into())));
        assert_eq!(
            e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
            14.240
        );
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 14.225);
    }

    #[test]
    fn a_knob_residency_gets_its_sideband_from_the_existing_band_policy() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(7.200, "40m", "USB");
        e.rig_dial_seen = true;
        e.observe_rig_freq(7_250_000);
        assert!(e.dial_residency.as_ref().unwrap().sideband.is_none());
        let entry = e.prepare_mode_entry("phone", true);
        assert_eq!(entry.frequency, Some((7.250, "LSB".into())));
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 7.250);
        assert_eq!(e.settings().sideband, "LSB");
    }

    #[test]
    fn digital_entry_still_corrects_an_outgoing_lsb_without_homing() {
        let mut e = station();
        e.set_operating_mode("rtty", false);
        e.set_frequency(14.090, "20m", "LSB");
        assert!(e.tx_enabled());
        let entry = e.prepare_mode_entry("digital", false);
        assert!(entry.frequency.is_none());
        assert_eq!(e.settings().sideband, "LSB");
        assert!(
            e.tx_enabled(),
            "preparation cannot lower the local latch either"
        );
        e.set_operating_mode("digital", false);
        assert_eq!(e.settings().dial_mhz, 14.090);
        assert_eq!(e.settings().sideband, "USB");
        assert!(!e.tx_enabled());
    }

    #[test]
    fn an_off_band_residency_does_not_become_a_recall_cell() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(5.000, "", "USB");
        let entry = e.prepare_mode_entry("phone", true);
        assert!(entry.frequency.is_none());
        assert!(e.dial_residency.is_some());
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 5.000);
        assert!(!e
            .freq_memory
            .contains_key(&(String::new(), OperatingMode::Phone)));
    }
}
