//! The FT modes' TX drive (the cockpit's drive slider, `tx_level`), remembered per BAND and MODE.
//!
//! An amplifier is rarely flat across the bands, so the drive that keeps the ALC at zero on 20 m
//! overdrives on 6 m. The operator's own slider move on an FT mode ([`Engine::set_tx_level`]) is
//! remembered for that band and mode on the radio in use ([`RadioProfile::ft_drive`]), and when
//! the station lands on another band, mode or radio the level last used there comes back
//! ([`Engine::settle_ft_drive`]). A band+mode with nothing saved takes the band's last level from
//! any FT mode; a band with nothing saved keeps the drive as it is.
//!
//! ## The rules, and where each is held
//!
//! - **Never while transmitting.** The drive is applied by the radio loop as it changes, so a
//!   restore made mid-over would change the level inside the over. [`Engine::settle_ft_drive`]
//!   runs on every radio-loop tick and does nothing while the loop or the engine reports the
//!   radio on the air ([`Engine::on_air`], the loop's own hold); the place it has not settled stays
//!   pending and is settled on the first tick after the unkey, before the next over keys.
//! - **Only the operator's pick is saved.** [`Engine::set_tx_level`] is the one writer of the
//!   memory; the restore writes the live level only ([`Engine::restore_tx_level`]).
//! - **Only a level the operator set on that band.** A restore brings back a level the operator
//!   chose on that band (in that mode, else in another FT mode), never one from another band or a
//!   default; with nothing saved the drive is left alone. Levels are clamped to the slider's
//!   0–1 range when saved and validated on load (`lenient_ft_drive` in `settings.rs`).
//! - **FT modes only.** The Operate section on a WSJT-X-family mode ([`ft_drive_mode`]); Phone,
//!   CW, RTTY, PSK, JS8 and the Tempo modes neither save nor restore.
//!
//! [`RadioProfile::ft_drive`]: crate::settings::RadioProfile::ft_drive

use super::*;

/// Where the FT drive is remembered: the radio in use, its band and its FT mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FtDriveSlot {
    radio: u32,
    band: String,
    mode: &'static str,
}

/// The FT mode `tier` names for the drive memory, or `None` for a tier that is not one: the
/// Tempo modes and JS8 have cockpits of their own.
fn ft_drive_mode(tier: Tier) -> Option<&'static str> {
    match tier {
        Tier::TempoFast | Tier::TempoDeep | Tier::Js8 => None,
        t => Some(t.label()),
    }
}

impl Engine {
    /// The band, mode and radio the FT drive is remembered for right now, or `None` when the
    /// station is not on an FT mode (another section, a non-FT tier, or off the bands).
    pub(super) fn ft_drive_slot(&self) -> Option<FtDriveSlot> {
        if self.settings.operating_mode != crate::settings::OperatingMode::Digital {
            return None;
        }
        let mode = ft_drive_mode(self.app.tier())?;
        let band = self.settings.band.trim();
        (!band.is_empty()).then(|| FtDriveSlot {
            radio: self.settings.active_radio,
            band: band.to_string(),
            mode,
        })
    }

    /// The operator's own drive pick on an FT mode: remembered for this band and mode on the
    /// radio in use. Called by [`Engine::set_tx_level`] only.
    pub(super) fn remember_ft_drive(&mut self, level: f32) {
        let slot = self.ft_drive_slot();
        if let Some(at) = &slot {
            if let Some(p) = self.settings.radios.iter_mut().find(|p| p.id == at.radio) {
                p.remember_ft_drive(&at.band, at.mode, level);
            }
        }
        // The operator has settled the drive for where the station is: a restore still waiting
        // for an unkey must not put an older level back over this pick.
        self.ft_drive_at = slot;
    }

    /// Bring back the FT drive last used on the band, mode and radio the station is now on —
    /// once per change of any of the three, and never while the radio is on the air. Called by
    /// the radio loop every tick, with `loop_keyed` its own word that it holds the key (an over's
    /// PTT hold, a tune carrier, the operator's PTT, a radio handoff not yet done). Returns
    /// whether the drive changed.
    pub fn settle_ft_drive(&mut self, loop_keyed: bool) -> bool {
        let slot = self.ft_drive_slot();
        if slot == self.ft_drive_at {
            return false;
        }
        // ⛔ Never inside an over: the place stays unsettled and is settled at the unkey.
        if loop_keyed
            || self.on_air()
            || self.loop_tx_hold_until.is_some_and(|t| Instant::now() < t)
        {
            return false;
        }
        self.ft_drive_at = slot.clone();
        let Some(at) = slot else {
            return false;
        };
        let Some(level) = self
            .settings
            .radios
            .iter()
            .find(|p| p.id == at.radio)
            .and_then(|p| p.ft_drive_for(&at.band, at.mode))
        else {
            return false;
        };
        if (level - self.settings.tx_level).abs() <= f32::EPSILON {
            return false;
        }
        tempo_core::applog::info(
            "tx",
            &format!(
                "FT drive for {} {}: {:.2} (was {:.2})",
                at.band, at.mode, level, self.settings.tx_level
            ),
        );
        self.restore_tx_level(level);
        true
    }

    /// Set the live drive to a remembered level: the flat field the radio loop reads and the
    /// active radio's profile, as [`Engine::set_tx_level`] does, and NOT the memory — a restore is
    /// not the operator's pick.
    fn restore_tx_level(&mut self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        self.settings.tx_level = level;
        let active = self.settings.active_radio;
        if let Some(p) = self.settings.radios.iter_mut().find(|p| p.id == active) {
            p.tx_level = level;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::OperatingMode;

    /// A station on 20 m FT8 in the Operate section, radio 0 in use, drive at 0.5, its first
    /// tick taken so the place it starts on is settled.
    fn station() -> Engine {
        let mut e = Engine::new("K1ABC", "EN50", 0);
        e.settings.operating_mode = OperatingMode::Digital;
        e.set_tier(Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.settings.ensure_radio_profiles();
        e.set_tx_level(0.5);
        e.settle_ft_drive(false);
        e
    }

    fn drive(e: &Engine) -> f32 {
        e.settings().tx_level
    }

    fn profile(e: &Engine) -> &crate::settings::RadioProfile {
        let id = e.settings().active_radio;
        e.settings()
            .radios
            .iter()
            .find(|p| p.id == id)
            .expect("the radio in use has a profile")
    }

    fn qsy(e: &mut Engine, mhz: f64, band: &str) {
        e.set_frequency(mhz, band, "USB");
    }

    #[test]
    fn a_band_change_brings_back_the_drive_last_used_there() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m FT8
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        e.set_tx_level(0.80); // 6 m FT8
        qsy(&mut e, 14.074, "20m");
        assert!(
            e.settle_ft_drive(false),
            "the drive changed on the way back"
        );
        assert!((drive(&e) - 0.30).abs() < 1e-6, "20 m's drive came back");
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        assert!((drive(&e) - 0.80).abs() < 1e-6, "and 6 m's");
        // The profile the next save is built on holds the live level too.
        assert!((profile(&e).tx_level - 0.80).abs() < 1e-6);
    }

    #[test]
    fn a_band_never_driven_keeps_the_drive_as_it_is() {
        let mut e = station();
        e.set_tx_level(0.30);
        qsy(&mut e, 7.074, "40m");
        assert!(
            !e.settle_ft_drive(false),
            "nothing saved for 40 m: nothing restored"
        );
        assert!((drive(&e) - 0.30).abs() < 1e-6);
    }

    #[test]
    fn a_mode_change_brings_back_that_modes_drive_on_the_band() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m FT8
        e.set_tier(Tier::Ft4);
        e.settle_ft_drive(false);
        e.set_tx_level(0.60); // 20 m FT4
        e.set_tier(Tier::Ft8);
        e.settle_ft_drive(false);
        assert!((drive(&e) - 0.30).abs() < 1e-6, "FT8's own 20 m drive");
        e.set_tier(Tier::Ft4);
        e.settle_ft_drive(false);
        assert!((drive(&e) - 0.60).abs() < 1e-6, "FT4's own 20 m drive");
    }

    #[test]
    fn a_mode_never_driven_on_the_band_takes_the_bands_last_ft_drive() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m FT8
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        e.set_tx_level(0.80); // 6 m FT8

        // FST4 has never been driven on 20 m: the band's last FT drive (FT8's) comes back.
        e.set_tier(Tier::Fst4);
        e.settle_ft_drive(false);
        qsy(&mut e, 14.074, "20m");
        e.settle_ft_drive(false);
        assert!(
            (drive(&e) - 0.30).abs() < 1e-6,
            "20 m's last drive, from FT8"
        );
        // CONTROL: a band+mode entry wins over the band's fallback.
        e.set_tx_level(0.45); // 20 m FST4 — now 20 m's last drive too
        e.set_tier(Tier::Ft8);
        e.settle_ft_drive(false);
        assert!(
            (drive(&e) - 0.30).abs() < 1e-6,
            "FT8 keeps its own 20 m drive, not the band's latest"
        );
    }

    #[test]
    fn the_drive_never_changes_while_keyed_and_lands_at_the_unkey() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        e.set_tx_level(0.80); // 6 m
        qsy(&mut e, 14.074, "20m");
        // The loop holds the key (an over still going out): nothing changes, tick after tick.
        assert!(!e.settle_ft_drive(true));
        assert!(!e.settle_ft_drive(true));
        assert!((drive(&e) - 0.80).abs() < 1e-6, "the over keeps its level");
        // The engine's own word that the radio is on the air holds it too (a mic key at the rig).
        e.rig_keyed = true;
        assert!(!e.settle_ft_drive(false));
        assert!((drive(&e) - 0.80).abs() < 1e-6);
        e.rig_keyed = false;
        // The unkey: the first idle tick brings 20 m's drive back.
        assert!(e.settle_ft_drive(false));
        assert!((drive(&e) - 0.30).abs() < 1e-6);
    }

    #[test]
    fn an_operator_pick_made_while_a_restore_waits_is_not_overwritten_at_the_unkey() {
        let mut e = station();
        e.set_tx_level(0.30);
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        e.set_tx_level(0.80);
        qsy(&mut e, 14.074, "20m");
        assert!(!e.settle_ft_drive(true)); // keyed: the 20 m restore waits
        e.set_tx_level(0.20); // the operator trims 20 m by hand meanwhile
        assert!(!e.settle_ft_drive(false), "their pick stands");
        assert!((drive(&e) - 0.20).abs() < 1e-6);
    }

    /// A restore really changes the drive here (20 m FT4, never driven, takes 20 m's 0.30 over the
    /// live 0.80), and still writes nothing to the memory: no "20m/FT4" entry appears and 20 m's
    /// own is untouched. A restore made through the operator's slider path would write both.
    #[test]
    fn a_restore_is_not_saved_as_the_operators_pick() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m FT8
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        e.set_tx_level(0.80); // 6 m FT8
        let before = profile(&e).ft_drive.clone();
        // From 6 m FT8 straight to 20 m FT4, settled once.
        e.set_tier(Tier::Ft4);
        qsy(&mut e, 14.074, "20m");
        assert!(
            e.settle_ft_drive(false),
            "premise: the restore changed the drive"
        );
        assert!((drive(&e) - 0.30).abs() < 1e-6, "premise: 20 m's fallback");
        assert!(!profile(&e).ft_drive.contains_key("20m/FT4"));
        assert_eq!(profile(&e).ft_drive, before, "no restore wrote the memory");
    }

    /// The radio loop's word that it still holds the key on a deadline of its own (a voice
    /// message or an APRS frame it plays, `observe_loop_tx_hold`) holds a restore off, though
    /// nothing else says the radio is on the air; the restore lands once that hold ends.
    #[test]
    fn a_hold_the_loop_reported_keeps_the_drive_until_it_ends() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        e.set_tx_level(0.80); // 6 m
        qsy(&mut e, 14.074, "20m");
        e.observe_loop_tx_hold(Some(5_000.0));
        assert!(!e.on_air(), "premise: nothing else says on the air");
        assert!(!e.settle_ft_drive(false));
        assert!(
            (drive(&e) - 0.80).abs() < 1e-6,
            "the held over keeps its level"
        );
        e.observe_loop_tx_hold(None);
        assert!(
            e.settle_ft_drive(false),
            "the hold ended: 20 m's drive comes back"
        );
        assert!((drive(&e) - 0.30).abs() < 1e-6);
    }

    /// A stored level the slider cannot set (a corrupt or hand-edited entry that got past the
    /// loader) is passed over, never clamped to full drive: with nothing else saved for the band,
    /// the drive stays where it is.
    #[test]
    fn an_out_of_range_stored_level_keeps_the_current_drive() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m FT8
        qsy(&mut e, 50.313, "6m");
        e.settle_ft_drive(false);
        let id = e.settings.active_radio;
        let p = e.settings.radios.iter_mut().find(|p| p.id == id).unwrap();
        p.ft_drive.insert("20m/FT8".into(), 9.0);
        p.ft_drive.insert("20m".into(), -1.0);
        qsy(&mut e, 14.074, "20m");
        assert!(!e.settle_ft_drive(false), "nothing valid to restore");
        assert!(
            (drive(&e) - 0.30).abs() < 1e-6,
            "the drive stays, never full"
        );
    }

    #[test]
    fn outside_the_ft_modes_nothing_is_saved_or_restored() {
        let mut e = station();
        e.set_tx_level(0.30); // 20 m FT8
                              // JS8 has its own cockpit: its drive is not FT's.
        e.set_tier(Tier::Js8);
        e.settle_ft_drive(false);
        e.set_tx_level(0.90);
        assert!(!profile(&e).ft_drive.contains_key("20m/JS8"));
        assert!((profile(&e).ft_drive["20m"] - 0.30).abs() < 1e-6);
        // Phone: no restore on a band change there.
        e.set_tier(Tier::Ft8);
        e.settings.operating_mode = OperatingMode::Phone;
        e.set_tx_level(0.95);
        qsy(&mut e, 50.313, "6m");
        assert!(!e.settle_ft_drive(false));
        assert!((drive(&e) - 0.95).abs() < 1e-6);
        // Back in Operate on 20 m: FT8's 20 m drive.
        e.settings.operating_mode = OperatingMode::Digital;
        qsy(&mut e, 14.074, "20m");
        assert!(e.settle_ft_drive(false));
        assert!((drive(&e) - 0.30).abs() < 1e-6);
    }

    #[test]
    fn each_radio_keeps_its_own_drive_memory() {
        let mut e = station();
        e.set_tx_level(0.30); // radio 0, 20 m FT8
        let mut p1 = profile(&e).clone();
        p1.id = 1;
        p1.ft_drive.clear();
        p1.ft_drive.insert("20m/FT8".into(), 0.70);
        p1.tx_level = 0.30;
        e.settings.radios.push(p1);
        e.settings.active_radio = 1;
        e.settle_ft_drive(false);
        assert!((drive(&e) - 0.70).abs() < 1e-6, "radio 1's own 20 m drive");
    }

    #[test]
    fn a_saved_level_is_clamped_to_the_sliders_range() {
        let mut e = station();
        e.set_tx_level(1.7);
        assert!((profile(&e).ft_drive["20m/FT8"] - 1.0).abs() < 1e-6);
        e.set_tx_level(-0.2);
        assert_eq!(profile(&e).ft_drive["20m/FT8"], 0.0);
    }
}
