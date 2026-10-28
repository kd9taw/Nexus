//! Doppler correction on the APRS cockpit's ISS channel (145.825 MHz).
//!
//! The ISS digipeater listens and transmits on one frequency, and a pass shifts it by up to
//! about ±3.6 kHz each way: the station hears it HIGH while it approaches and must transmit LOW
//! so its packet lands on the ISS's receiver (the reverse once it recedes). A satellite pass held
//! on one dial can only park on the published channel ([`tempo_core::doppler::tuning`]); an APRS
//! over is short and keyed by the station itself, so here the dial does both legs in turn:
//!
//! - **Between overs** the radio sits on the RECEIVE correction, moved only when it has drifted
//!   [`MIN_STEP_HZ`] or more, and at most once every [`MIN_INTERVAL_MS`].
//! - **For an over** the radio moves to the TRANSMIT correction first, and the frame keys only once
//!   the radio has acknowledged that frequency ([`Engine::poll_aprs_tx`] holds it until then), so
//!   the licence and privilege gates there judge the frequency actually keyed. The dial is held
//!   for the whole over and returns to the receive correction after the unkey.
//! - **Never while keyed.** Nothing here moves the dial during an over, the operator's own PTT or
//!   any other transmission; the radio loop also refuses to push a dial while keyed.
//! - **Off** below the horizon, without current orbital elements, without a grid, without CAT, while
//!   the Satellites tracker steers the radio, or when the operator turns it off: the radio goes back
//!   to (or stays on) 145.825 and the cockpit says why.
//!
//! The geometry comes from the app's existing satellite tracker and element store (src-tauri's
//! APRS ISS driver feeds [`Engine::aprs_iss_tick`] once a second); this module holds no orbit
//! model. Terrestrial APRS channels are never touched.

use super::*;

/// The ISS APRS digipeater's channel, up and down.
pub const ISS_APRS_HZ: u64 = 145_825_000;
/// A receive correction smaller than this is not worth a CAT write on an FM channel.
pub const MIN_STEP_HZ: u64 = 500;
/// At most one receive correction a second.
pub const MIN_INTERVAL_MS: u64 = 1_000;
/// Corrections are written on a 100 Hz grid: far finer than an FM channel needs, and a value a
/// radio's frequency read-back reports back unchanged.
const GRID_HZ: u64 = 100;
/// How long a frame waits for the radio to acknowledge the transmit frequency before it is
/// dropped with a notice (never sent later, like every other refused APRS frame).
pub const UPLINK_TAKE_MS: u64 = 5_000;
/// Held past the over's own length before the receive correction may return: covers the radio
/// loop's TX tail and the one-second tick that first sees the over.
const OVER_MARGIN_MS: u64 = 1_500;

/// What the tracker says about the ISS from this station, right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IssSky {
    /// Above the horizon; `range_rate_km_s` is positive while it recedes.
    Up {
        range_rate_km_s: f64,
    },
    BelowHorizon,
    /// No orbital elements for the ISS at all.
    NoElements,
    /// Elements past the acting age limit: too old to tune by.
    StaleElements,
    /// No usable grid square, so no observer position.
    NoGrid,
}

/// An APRS over on the ISS channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Over {
    /// A frame waits while the radio moves to the transmit frequency `hz`. `since_ms` is stamped
    /// by the first tick that sees it.
    Staged { hz: u64, since_ms: Option<u64> },
    /// A frame `len_ms` long was handed to the radio loop to key.
    OnAir { len_ms: u64, since_ms: Option<u64> },
}

/// The ISS channel's state in the engine.
#[derive(Debug, Default)]
pub struct AprsIss {
    /// The ISS channel is the APRS pick in force (meaningful only while `aprs_fm` holds).
    selected: bool,
    /// The tracker's latest word, `None` until the first tick.
    sky: Option<IssSky>,
    /// This module has moved the dial since the channel was picked.
    moved: bool,
    /// The last dial the radio acknowledged on the wire.
    rig_on_hz: Option<u64>,
    /// When the receive correction was last written (tick clock).
    last_write_ms: Option<u64>,
    over: Option<Over>,
}

/// The ISS channel's correction, for the APRS cockpit.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsIssDoppler {
    /// The correction is steering the radio.
    pub on: bool,
    /// Why not, when it is not: `switchedOff`, `satTracker`, `noCat`, `starting`, `noGrid`,
    /// `noElements`, `staleElements` or `belowHorizon`.
    pub why_off: Option<&'static str>,
    /// Where the radio listens between overs (Hz), while on.
    pub rx_hz: Option<u64>,
    /// Where an over is sent (Hz), while on.
    pub tx_hz: Option<u64>,
}

fn on_grid(hz: u64) -> u64 {
    (hz + GRID_HZ / 2) / GRID_HZ * GRID_HZ
}

impl Engine {
    /// The APRS tune landed on `dial_hz`: the ISS channel starts fresh; any other channel ends it.
    pub(super) fn aprs_iss_select(&mut self, dial_hz: u64) {
        self.aprs_iss = AprsIss {
            selected: dial_hz == ISS_APRS_HZ,
            ..AprsIss::default()
        };
    }

    /// The ISS channel is the APRS context in force. Any QSY or section change clears `aprs_fm`,
    /// and with it this.
    fn aprs_iss_live(&self) -> bool {
        self.aprs_iss.selected && self.aprs_fm
    }

    /// Whether the driver should look up the ISS for this station: the channel is in force.
    pub fn aprs_iss_wants_sky(&self) -> bool {
        self.aprs_iss_live()
    }

    /// Is a transmission in flight that the engine can see: the operator's PTT on the radio, a
    /// slot over, or anything else that owns the transmitter.
    pub fn aprs_iss_keyed(&self) -> bool {
        self.rig_keyed || self.app.transmitting() || self.tx_owner().is_some()
    }

    /// Why the correction is not steering, or `None` while it is.
    fn aprs_iss_why_off(&self) -> Option<&'static str> {
        if !self.settings.aprs_iss_doppler {
            return Some("switchedOff");
        }
        if self.sat_dial_owner.is_some() {
            return Some("satTracker");
        }
        if self.cat_status.0 != Some(true) {
            return Some("noCat");
        }
        match self.aprs_iss.sky {
            None => Some("starting"),
            Some(IssSky::Up { .. }) => None,
            Some(IssSky::BelowHorizon) => Some("belowHorizon"),
            Some(IssSky::NoElements) => Some("noElements"),
            Some(IssSky::StaleElements) => Some("staleElements"),
            Some(IssSky::NoGrid) => Some("noGrid"),
        }
    }

    /// Both corrected legs on the write grid, while the correction is steering.
    fn aprs_iss_legs(&self) -> Option<(u64, u64)> {
        if !self.aprs_iss_live() || self.aprs_iss_why_off().is_some() {
            return None;
        }
        let Some(IssSky::Up { range_rate_km_s }) = self.aprs_iss.sky else {
            return None;
        };
        let t = tempo_core::doppler::simplex_legs(ISS_APRS_HZ, range_rate_km_s);
        Some((on_grid(t.downlink_hz), on_grid(t.uplink_hz)))
    }

    /// The correction's state for the APRS cockpit; `None` unless the ISS channel is in force.
    pub fn aprs_iss_doppler(&self) -> Option<AprsIssDoppler> {
        if !self.aprs_iss_live() {
            return None;
        }
        let legs = self.aprs_iss_legs();
        Some(AprsIssDoppler {
            on: legs.is_some(),
            why_off: self.aprs_iss_why_off(),
            rx_hz: legs.map(|l| l.0),
            tx_hz: legs.map(|l| l.1),
        })
    }

    /// Move the dial for the ISS correction — not a QSY, so none of a QSY's bookkeeping (which
    /// would also end the APRS context). Same shape as [`Self::steer_sat_dial`]. A committed
    /// remote APRS tune's hold moves with the dial, so the browser that set it is not stranded.
    fn aprs_iss_set_dial(&mut self, hz: u64) {
        let was = self.settings.dial_hz();
        if self.remote_aprs_hold == Some(was) {
            self.remote_aprs_hold = Some(hz);
        }
        let mhz = hz as f64 / 1_000_000.0;
        self.settings.dial_mhz = mhz;
        self.record_dial(DialOrigin::Machinery);
        let (band, sb) = (self.settings.band.clone(), self.settings.sideband.clone());
        self.app.set_radio(mhz, &band, &sb);
        self.aprs_iss.moved = true;
    }

    /// The radio acknowledged a dial on the wire (from [`Self::rig_dial_applied`]).
    pub(super) fn aprs_iss_rig_applied(&mut self, hz: u64) {
        self.aprs_iss.rig_on_hz = Some(hz);
    }

    /// One correction tick from the driver: the tracker's word, the tick clock (ms) and whether a
    /// transmission is in flight. Returns whether the dial was moved.
    ///
    /// ⛔ Never moves the dial while `keyed`, while a frame waits for the transmit frequency, or
    /// while an over is on the air (its length plus a margin, from the first tick that sees it).
    pub fn aprs_iss_tick(&mut self, sky: IssSky, now_ms: u64, keyed: bool) -> bool {
        if !self.aprs_iss_live() {
            if self.aprs_iss.selected {
                self.aprs_iss = AprsIss::default();
            }
            return false;
        }
        self.aprs_iss.sky = Some(sky);
        // The Satellites tracker's pass owns the dial: hands off entirely.
        if self.sat_dial_owner.is_some() {
            return false;
        }
        let mut leaving_over = false;
        match self.aprs_iss.over {
            Some(Over::Staged { hz, since_ms }) => {
                if self.aprs_tx_queue.is_empty() {
                    // Stop TX (or TX Off) dropped the frame while it waited.
                    self.aprs_iss.over = None;
                    leaving_over = true;
                } else {
                    let since = since_ms.unwrap_or(now_ms);
                    self.aprs_iss.over = Some(Over::Staged {
                        hz,
                        since_ms: Some(since),
                    });
                    if now_ms.saturating_sub(since) < UPLINK_TAKE_MS {
                        return false;
                    }
                    tempo_core::applog::info(
                        "tx",
                        "APRS not keyed: the radio did not take the ISS transmit frequency (dropped)",
                    );
                    self.aprs_tx_queue.clear();
                    self.aprs_tx_notice = Some(APRS_ISS_UPLINK_NOT_TAKEN.to_string());
                    self.aprs_iss.over = None;
                    leaving_over = true;
                }
            }
            Some(Over::OnAir { len_ms, since_ms }) => {
                let since = since_ms.unwrap_or(now_ms);
                self.aprs_iss.over = Some(Over::OnAir {
                    len_ms,
                    since_ms: Some(since),
                });
                if keyed || now_ms < since + len_ms + OVER_MARGIN_MS {
                    return false;
                }
                self.aprs_iss.over = None;
                leaving_over = true;
            }
            None => {}
        }
        if keyed {
            return false;
        }
        let want = self.aprs_iss_legs().map_or(ISS_APRS_HZ, |l| l.0);
        let have = self.settings.dial_hz();
        if want == have {
            return false;
        }
        // Back to the channel when the correction goes off, but never move a dial this module
        // did not move.
        let going_home = want == ISS_APRS_HZ;
        if going_home && !self.aprs_iss.moved {
            return false;
        }
        if !(leaving_over || going_home) {
            if have.abs_diff(want) < MIN_STEP_HZ {
                return false;
            }
            if self
                .aprs_iss
                .last_write_ms
                .is_some_and(|t| now_ms.saturating_sub(t) < MIN_INTERVAL_MS)
            {
                return false;
            }
        }
        self.aprs_iss_set_dial(want);
        self.aprs_iss.last_write_ms = Some(now_ms);
        true
    }

    /// May the frame at the head of the APRS queue key now? Always yes off the ISS channel. On it,
    /// only once the dial is the transmit frequency (the correction's, or 145.825 while the
    /// correction is off) and, if this module has moved the dial, the radio has acknowledged it.
    /// Until then the dial is moved there and the frame waits.
    pub(super) fn aprs_iss_ready_to_key(&mut self) -> bool {
        if !self.aprs_iss_live() || self.sat_dial_owner.is_some() {
            return true;
        }
        let target = self.aprs_iss_legs().map_or(ISS_APRS_HZ, |l| l.1);
        let dial = self.settings.dial_hz();
        if dial == target && (!self.aprs_iss.moved || self.aprs_iss.rig_on_hz == Some(target)) {
            return true;
        }
        let since_ms = match self.aprs_iss.over {
            Some(Over::Staged { since_ms, .. }) => since_ms,
            _ => None,
        };
        self.aprs_iss.over = Some(Over::Staged {
            hz: target,
            since_ms,
        });
        if dial != target {
            self.aprs_iss_set_dial(target);
        }
        false
    }

    /// A frame of `samples` (12 kHz) was handed to the radio loop: hold the dial for the over.
    pub(super) fn aprs_iss_keying(&mut self, samples: usize) {
        if !self.aprs_iss_live() || self.sat_dial_owner.is_some() {
            return;
        }
        self.aprs_iss.over = Some(Over::OnAir {
            len_ms: samples as u64 / 12,
            since_ms: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Technician on the ISS channel with CAT up and TX armed.
    fn on_the_iss() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        e.set_license_class("technician");
        e.set_operating_mode("phone", false);
        e.aprs_tune(145.825).unwrap();
        e.cat_status.0 = Some(true);
        assert!(e.tx_enabled(), "precondition: TX is armed");
        e
    }

    /// The loop pushed the dial and the radio acknowledged it.
    fn radio_takes_the_dial(e: &mut Engine) {
        let hz = e.settings().dial_hz();
        e.rig_dial_applied(hz);
    }

    const APPROACHING: IssSky = IssSky::Up {
        range_rate_km_s: -6.0,
    };

    #[test]
    fn approaching_the_radio_listens_high_and_an_over_is_sent_low() {
        let mut e = on_the_iss();
        assert!(e.aprs_iss_tick(APPROACHING, 10_000, false));
        let rx = e.settings().dial_hz();
        // 6 km/s at 145.825 MHz = 2.92 kHz.
        assert!(
            (145_827_800..=145_828_000).contains(&rx),
            "receive correction {rx}"
        );
        let d = e.aprs_iss_doppler().unwrap();
        assert!(d.on && d.why_off.is_none());
        assert_eq!(d.rx_hz, Some(rx));
        let tx = d.tx_hz.unwrap();
        assert!((145_822_000..=145_822_200).contains(&tx), "transmit {tx}");
    }

    #[test]
    fn terrestrial_channels_are_never_corrected() {
        let mut e = on_the_iss();
        e.aprs_tune(144.390).unwrap();
        assert!(!e.aprs_iss_tick(APPROACHING, 10_000, false));
        assert_eq!(e.settings().dial_hz(), 144_390_000);
        assert!(e.aprs_iss_doppler().is_none());
        // Control: back on the ISS, the same tick corrects.
        e.aprs_tune(145.825).unwrap();
        assert!(e.aprs_iss_tick(APPROACHING, 20_000, false));
    }

    /// ⛔ NEVER RETUNE WHILE KEYED. Control: the same tick, unkeyed, moves the dial.
    #[test]
    fn a_correction_due_while_keyed_is_refused_and_lands_after_the_unkey() {
        let mut e = on_the_iss();
        assert!(
            !e.aprs_iss_tick(APPROACHING, 10_000, true),
            "moved while keyed"
        );
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        assert!(
            e.aprs_iss_tick(APPROACHING, 11_000, false),
            "control: unkeyed it moves"
        );
        assert_ne!(e.settings().dial_hz(), ISS_APRS_HZ);
    }

    #[test]
    fn small_or_rapid_corrections_are_not_written() {
        let mut e = on_the_iss();
        assert!(e.aprs_iss_tick(APPROACHING, 10_000, false));
        // ~0.1 km/s change = ~50 Hz: under the step.
        let near = IssSky::Up {
            range_rate_km_s: -5.9,
        };
        assert!(
            !e.aprs_iss_tick(near, 20_000, false),
            "a 50 Hz move was written"
        );
        // A 1.5 kHz move half a second later waits for the interval…
        let far = IssSky::Up {
            range_rate_km_s: -3.0,
        };
        assert!(
            !e.aprs_iss_tick(far, 10_500, false),
            "two writes inside a second"
        );
        // …and lands once it has passed.
        assert!(e.aprs_iss_tick(far, 11_000, false));
    }

    /// The transmit frequency is set BEFORE the key: the frame waits until the radio has taken
    /// it, keys there, and the dial holds through the over, returning to the receive correction
    /// after it.
    #[test]
    fn the_uplink_is_set_before_keying_held_for_the_over_and_released_after() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, 10_000, false);
        radio_takes_the_dial(&mut e);
        let rx = e.settings().dial_hz();
        let tx = e.aprs_iss_doppler().unwrap().tx_hz.unwrap();
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &["ARISS".to_string()])
            .unwrap();
        assert!(e.poll_aprs_tx().is_none(), "keyed before the radio moved");
        assert_eq!(
            e.settings().dial_hz(),
            tx,
            "the dial goes to the uplink first"
        );
        assert!(
            e.poll_aprs_tx().is_none(),
            "keyed before the radio acknowledged the uplink"
        );
        // A receive tick while the frame waits must not drag the dial back.
        assert!(!e.aprs_iss_tick(APPROACHING, 11_000, false));
        assert_eq!(e.settings().dial_hz(), tx);
        radio_takes_the_dial(&mut e);
        let audio = e
            .poll_aprs_tx()
            .expect("keys once the radio is on the uplink");
        assert_eq!(e.settings().dial_hz(), tx, "keyed on the uplink");
        let len_ms = audio.len() as u64 / 12;
        // Through the whole over (and its margin) nothing moves, keyed or not.
        assert!(!e.aprs_iss_tick(APPROACHING, 12_000, true));
        assert!(!e.aprs_iss_tick(APPROACHING, 12_000 + len_ms, false));
        assert_eq!(e.settings().dial_hz(), tx, "the uplink held for the over");
        // After it, back to the receive correction straight away.
        assert!(e.aprs_iss_tick(APPROACHING, 12_000 + len_ms + OVER_MARGIN_MS, false));
        assert_eq!(e.settings().dial_hz(), rx);
    }

    /// The licence gate judges the frequency keyed: on the pass that lets the frame go, the dial
    /// the gate reads is the uplink. Control: a frame refused there is dropped, not keyed later.
    #[test]
    fn the_privilege_gate_sees_the_uplink_and_still_refuses() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, 10_000, false);
        radio_takes_the_dial(&mut e);
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[]).unwrap();
        assert!(e.poll_aprs_tx().is_none());
        radio_takes_the_dial(&mut e);
        // The licence changes while the frame waits on the uplink: the next pass refuses it.
        e.set_license_class("novice");
        assert!(
            !e.tx_allowed(),
            "precondition: a Novice has no 2 m privileges"
        );
        assert!(e.poll_aprs_tx().is_none(), "keyed outside privileges");
        e.set_license_class("technician");
        assert!(e.poll_aprs_tx().is_none(), "the refused frame keyed later");
    }

    #[test]
    fn a_radio_that_never_takes_the_uplink_drops_the_frame_with_a_notice() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, 10_000, false);
        radio_takes_the_dial(&mut e);
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[]).unwrap();
        assert!(e.poll_aprs_tx().is_none());
        e.aprs_iss_tick(APPROACHING, 11_000, false);
        e.aprs_iss_tick(APPROACHING, 11_000 + UPLINK_TAKE_MS - 1, false);
        assert!(e.aprs_tx_notice().is_none(), "gave up early");
        e.aprs_iss_tick(APPROACHING, 11_000 + UPLINK_TAKE_MS, false);
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_UPLINK_NOT_TAKEN));
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_none(), "the dropped frame keyed later");
    }

    /// Off below the horizon (and without elements): back on 145.825, and the reason is given.
    #[test]
    fn below_the_horizon_the_radio_returns_to_the_channel_and_says_why() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, 10_000, false);
        assert_ne!(e.settings().dial_hz(), ISS_APRS_HZ);
        assert!(e.aprs_iss_tick(IssSky::BelowHorizon, 10_200, false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        let d = e.aprs_iss_doppler().unwrap();
        assert_eq!((d.on, d.why_off), (false, Some("belowHorizon")));
        for (sky, why) in [
            (IssSky::NoElements, "noElements"),
            (IssSky::StaleElements, "staleElements"),
            (IssSky::NoGrid, "noGrid"),
        ] {
            assert!(!e.aprs_iss_tick(sky, 20_000, false), "{why}: moved");
            assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
            assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some(why));
        }
        // An over below the horizon keys on 145.825 as before: nothing to wait for.
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[]).unwrap();
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_some());
    }

    #[test]
    fn the_switch_turns_it_off_and_no_cat_means_no_correction() {
        let mut e = on_the_iss();
        let mut s = e.settings().clone();
        s.aprs_iss_doppler = false;
        e.apply_settings(s);
        assert!(!e.aprs_iss_tick(APPROACHING, 10_000, false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("switchedOff"));
        // Off, a beacon keys on 145.825 with no wait (today's behaviour).
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[]).unwrap();
        assert!(e.poll_aprs_tx().is_some());

        let mut e = on_the_iss();
        e.cat_status.0 = None;
        assert!(!e.aprs_iss_tick(APPROACHING, 10_000, false));
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("noCat"));
    }

    #[test]
    fn a_qsy_ends_the_iss_correction() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, 10_000, false);
        e.set_frequency(146.520, "2m", "FM");
        assert!(!e.aprs_iss_tick(APPROACHING, 20_000, false));
        assert_eq!(e.settings().dial_hz(), 146_520_000);
        assert!(e.aprs_iss_doppler().is_none());
    }
}
