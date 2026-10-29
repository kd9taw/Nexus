//! Doppler correction on the APRS cockpit's ISS channel (145.825 MHz).
//!
//! The ISS digipeater listens and transmits on one frequency, and a pass shifts it by up to
//! about ±3.6 kHz each way: the station hears it HIGH while it approaches and must transmit LOW
//! so its packet lands on the ISS's receiver (the reverse once it recedes). A satellite pass held
//! on one dial can only park on the published channel ([`tempo_core::doppler::tuning`]); an APRS
//! over is short and keyed by the station itself, so here the dial does both legs in turn:
//!
//! - **Between overs** the radio sits on the RECEIVE correction, moved only when it has drifted
//!   [`MIN_STEP_HZ`] or more, and at most once every [`MIN_INTERVAL`].
//! - **For an over** the radio moves to the TRANSMIT correction first, and the frame keys only once
//!   the radio's own frequency read-back says it is there ([`Engine::poll_aprs_tx`] holds it until
//!   then), so the licence and privilege gates there judge the frequency actually keyed. The dial
//!   is held for the whole over and returns to the receive correction after the unkey.
//! - **Never while keyed.** Nothing here moves the dial during an over, the operator's own PTT or
//!   any other transmission; the radio loop also refuses to push a dial while keyed.
//! - **Never against the operator.** A dial this module did not put there (the knob, another band)
//!   ends the correction for that pick of the channel; the dial is never pulled back.
//! - **Off** below the horizon, without current orbital elements, without a grid, without CAT, while
//!   the Satellites tracker steers the radio, for a pass on a radio that rounds what it is sent,
//!   when the tracker has stopped answering, or when the operator turns it off: the radio goes back
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
pub const MIN_INTERVAL: Duration = Duration::from_secs(1);
/// Corrections are written on a 100 Hz grid: far finer than an FM channel needs, and a value a
/// radio's frequency read-back reports back unchanged.
const GRID_HZ: u64 = 100;
/// How long a frame waits for the radio to take the transmit frequency before it is dropped with
/// a notice (never sent later, like every other refused APRS frame).
pub const UPLINK_TAKE: Duration = Duration::from_secs(5);
/// Held past the over's own length before the receive correction may return: covers the radio
/// loop's TX tail and the one-second tick that first sees the over.
const OVER_MARGIN: Duration = Duration::from_millis(1_500);
/// A dial within this of the one this module last put there is still its own; any further off,
/// someone else tuned the radio.
const OWN_DIAL_TOLERANCE_HZ: u64 = 50;
/// A read-back this close to a dial the radio has just acknowledged is the radio rounding the
/// write to its tuning step (half the widest FM step, 25 kHz). Further off, it is a tune.
const ROUNDING_SPAN_HZ: u64 = 12_500;
/// The driver ticks once a second. A sky reading older than this means it has hung (a panic is
/// reported separately): the correction is off as if it had stopped, so no frame keys on the
/// geometry of a moment that has passed.
const SKY_FRESH: Duration = Duration::from_secs(5);

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
    /// A frame waits while the radio moves to the transmit frequency, until `deadline`
    /// (monotonic, stamped when it began waiting). Judged by [`Engine::poll_aprs_tx`] as well as
    /// the tick, so it holds without the driver.
    Staged { deadline: Instant },
    /// A frame `len` long was handed to the radio loop to key. `since` is stamped by the first
    /// tick that sees it.
    OnAir {
        len: Duration,
        since: Option<Instant>,
    },
}

/// The ISS channel's state in the engine.
#[derive(Debug, Default)]
pub struct AprsIss {
    /// The ISS channel is the APRS pick in force (meaningful only while `aprs_fm` holds).
    selected: bool,
    /// The tracker's latest word, `None` until the first tick.
    sky: Option<IssSky>,
    /// When `sky` was last given (the real monotonic clock, as the frame deadlines are).
    sky_at: Option<Instant>,
    /// This module has moved the dial since the channel was picked.
    moved: bool,
    /// The dial this module (or the APRS tune that picked the channel) last put on the engine.
    own_hz: u64,
    /// The last dial the radio acknowledged on the wire…
    acked_hz: Option<u64>,
    /// …and the radio's first frequency read-back after that acknowledgement.
    read_back_hz: Option<u64>,
    /// The radio did not hold a corrected dial: the correction is off until the pass ends.
    rounds: bool,
    /// Why the correction ended for this pick of the channel, for the cockpit.
    ended: Option<&'static str>,
    /// The driver thread stopped: no correction for the rest of the run.
    driver_stopped: bool,
    /// When the receive correction was last written (tick clock).
    last_write: Option<Instant>,
    over: Option<Over>,
}

/// The ISS channel's correction, for the APRS cockpit.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AprsIssDoppler {
    /// The correction is steering the radio.
    pub on: bool,
    /// Why not, when it is not: `switchedOff`, `stopped`, `satTracker`, `noCat`, `radioRounds`,
    /// `starting`, `noGrid`, `noElements`, `staleElements`, `belowHorizon`, or `tunedAway` (the
    /// radio was tuned off the channel, which ended the correction).
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
            own_hz: dial_hz,
            driver_stopped: self.aprs_iss.driver_stopped,
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
        if self.aprs_iss.driver_stopped
            || self
                .aprs_iss
                .sky_at
                .is_some_and(|t| t.elapsed() > SKY_FRESH)
        {
            return Some("stopped");
        }
        if self.sat_dial_owner.is_some() {
            return Some("satTracker");
        }
        if self.cat_status.0 != Some(true) {
            return Some("noCat");
        }
        if self.aprs_iss.rounds {
            return Some("radioRounds");
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

    /// The correction's state for the APRS cockpit; `None` unless the ISS channel is in force (or
    /// was, until the radio was tuned off it, which the cockpit says while APRS stays on).
    pub fn aprs_iss_doppler(&self) -> Option<AprsIssDoppler> {
        if let Some(why) = self.aprs_iss.ended.filter(|_| self.aprs_fm) {
            return Some(AprsIssDoppler {
                on: false,
                why_off: Some(why),
                rx_hz: None,
                tx_hz: None,
            });
        }
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

    /// Is the dial someone else's: off the one this module last put there, or off 2 m? The
    /// operator's knob (adopted by [`Self::observe_rig_freq`], which leaves the APRS context up)
    /// or anything else that tuned the radio.
    fn aprs_iss_dial_taken(&self) -> bool {
        self.settings.dial_hz().abs_diff(self.aprs_iss.own_hz) > OWN_DIAL_TOLERANCE_HZ
            || !self.settings.band.eq_ignore_ascii_case("2m")
    }

    /// End the correction for this pick of the channel, `why` for the cockpit. The dial stays
    /// where it is. Anything still queued was meant for the ISS, so it is dropped with `notice`,
    /// never sent on whatever frequency the radio is on now. A remote APRS hold ends with it: the
    /// channel the browser set is no longer the one in force.
    fn aprs_iss_end(&mut self, why: &'static str, notice: &str) {
        if !self.aprs_tx_queue.is_empty() {
            tempo_core::applog::info(
                "tx",
                &format!("APRS not keyed: the ISS Doppler correction ended ({why}) (dropped)"),
            );
            self.aprs_tx_queue.clear();
            self.aprs_tx_notice = Some(notice.to_string());
        }
        self.remote_aprs_hold = None;
        self.aprs_iss = AprsIss {
            ended: Some(why),
            driver_stopped: self.aprs_iss.driver_stopped,
            ..AprsIss::default()
        };
    }

    /// A frame that has waited out [`UPLINK_TAKE`] for the radio to take the transmit frequency
    /// is dropped with a notice that names the cause, never sent later. Judged on the monotonic
    /// clock against the frame's own deadline, from [`Self::poll_aprs_tx`] as well as the tick,
    /// so it holds even if the driver has stopped. Returns whether it dropped.
    fn aprs_iss_drop_expired(&mut self) -> bool {
        let Some(Over::Staged { deadline }) = self.aprs_iss.over else {
            return false;
        };
        if Instant::now() < deadline {
            return false;
        }
        let (why, notice) = if self.cat_status.0 != Some(true) {
            ("the CAT link is down", APRS_ISS_NO_CAT)
        } else {
            (
                "the radio did not take the ISS transmit frequency",
                APRS_ISS_UPLINK_NOT_TAKEN,
            )
        };
        tempo_core::applog::info("tx", &format!("APRS not keyed: {why} (dropped)"));
        self.aprs_tx_queue.clear();
        self.aprs_tx_notice = Some(notice.to_string());
        self.aprs_iss.over = None;
        true
    }

    /// Move the dial for the ISS correction — not a QSY, so none of a QSY's bookkeeping (which
    /// would also end the APRS context). Same shape as [`Self::steer_sat_dial`]. A committed
    /// remote APRS tune's hold moves with the dial, so the browser that set it is not stranded.
    /// Any confirmation the radio gave is for the dial before this move: only an ack and a
    /// read-back that come after it count.
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
        self.aprs_iss.own_hz = hz;
        self.aprs_iss.acked_hz = None;
        self.aprs_iss.read_back_hz = None;
    }

    /// The radio acknowledged a dial on the wire (from [`Self::rig_dial_applied`]). Only its
    /// read-back ([`Self::aprs_iss_rig_read`]) says it is there: a radio may round the write.
    pub(super) fn aprs_iss_rig_applied(&mut self, hz: u64) {
        self.aprs_iss.acked_hz = Some(hz);
        self.aprs_iss.read_back_hz = None;
    }

    /// The radio's own frequency read-back (from [`Self::rig_freq_read`]). The first read after
    /// it acknowledged a dial this module wrote either confirms that dial or, close by, says the
    /// radio rounds what it is sent: then the correction is off until the pass ends (a frame
    /// waiting for the transmit frequency is dropped, never keyed off target), and the dial the
    /// radio chose is taken as this module's own, so the cockpit does not call it a tune.
    pub(super) fn aprs_iss_rig_read(&mut self, hz: u64) {
        let Some(acked) = self.aprs_iss.acked_hz else {
            return;
        };
        if self.aprs_iss.read_back_hz.is_some() {
            return;
        }
        self.aprs_iss.read_back_hz = Some(hz);
        if hz == acked
            || !self.aprs_iss_live()
            || self.sat_dial_owner.is_some()
            || !self.aprs_iss.moved
            || acked != self.aprs_iss.own_hz
            || hz.abs_diff(acked) > ROUNDING_SPAN_HZ
        {
            return;
        }
        tempo_core::applog::info(
            "cat",
            &format!(
                "ISS Doppler correction off for this pass: the radio was sent {acked} Hz and reads \
                 back {hz} Hz"
            ),
        );
        if acked == ISS_APRS_HZ {
            // It does not hold the channel itself either: nothing left to steer back to.
            self.aprs_iss_end("radioRounds", APRS_ISS_RADIO_ROUNDS);
            return;
        }
        if matches!(self.aprs_iss.over, Some(Over::Staged { .. })) {
            tempo_core::applog::info(
                "tx",
                "APRS not keyed: the radio did not hold the ISS transmit frequency (dropped)",
            );
            self.aprs_tx_queue.clear();
            self.aprs_tx_notice = Some(APRS_ISS_RADIO_ROUNDS.to_string());
            self.aprs_iss.over = None;
        }
        self.aprs_iss.rounds = true;
        self.aprs_iss.own_hz = hz;
        // …and a remote APRS hold follows it, as it follows every dial this module sets.
        if self.remote_aprs_hold == Some(acked) {
            self.remote_aprs_hold = Some(hz);
        }
    }

    /// The driver thread stopped (it panicked): the correction is off for the rest of the run, and
    /// a frame waiting for the transmit frequency is dropped, never keyed later. A frame after
    /// this goes out on 145.825 once the radio reads back there, as with the correction off.
    pub fn aprs_iss_driver_stopped(&mut self) {
        self.aprs_iss.driver_stopped = true;
        if matches!(self.aprs_iss.over, Some(Over::Staged { .. })) {
            tempo_core::applog::info(
                "tx",
                "APRS not keyed: the ISS Doppler correction stopped (dropped)",
            );
            self.aprs_tx_queue.clear();
            self.aprs_tx_notice = Some(APRS_ISS_STOPPED.to_string());
            self.aprs_iss.over = None;
        }
    }

    /// One correction tick from the driver: the tracker's word, the tick clock (monotonic) and
    /// whether a transmission is in flight. Returns whether the dial was moved.
    ///
    /// ⛔ Never moves the dial while `keyed`, while a frame waits for the transmit frequency, or
    /// while an over is on the air (its length plus a margin, from the first tick that sees it).
    /// ⛔ Never moves a dial someone else tuned: that ends the correction instead.
    pub fn aprs_iss_tick(&mut self, sky: IssSky, now: Instant, keyed: bool) -> bool {
        if !self.aprs_iss_live() {
            if self.aprs_iss.selected {
                self.aprs_iss = AprsIss {
                    driver_stopped: self.aprs_iss.driver_stopped,
                    ..AprsIss::default()
                };
            }
            return false;
        }
        self.aprs_iss.sky = Some(sky);
        self.aprs_iss.sky_at = Some(Instant::now());
        if !matches!(sky, IssSky::Up { .. }) {
            // The pass is over: the next one tries the radio again.
            self.aprs_iss.rounds = false;
        }
        // The Satellites tracker's pass owns the dial: hands off entirely.
        if self.sat_dial_owner.is_some() {
            return false;
        }
        if self.aprs_iss_dial_taken() {
            self.aprs_iss_end("tunedAway", APRS_ISS_TUNED_AWAY);
            return false;
        }
        let mut leaving_over = false;
        match self.aprs_iss.over {
            Some(Over::Staged { .. }) => {
                if self.aprs_tx_queue.is_empty() {
                    // Stop TX (or TX Off) dropped the frame while it waited.
                    self.aprs_iss.over = None;
                    leaving_over = true;
                } else if self.aprs_iss_drop_expired() {
                    leaving_over = true;
                } else {
                    return false;
                }
            }
            Some(Over::OnAir { len, since }) => {
                let since = since.unwrap_or(now);
                self.aprs_iss.over = Some(Over::OnAir {
                    len,
                    since: Some(since),
                });
                if keyed || now < since + len + OVER_MARGIN {
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
                .last_write
                .is_some_and(|t| now.saturating_duration_since(t) < MIN_INTERVAL)
            {
                return false;
            }
        }
        self.aprs_iss_set_dial(want);
        self.aprs_iss.last_write = Some(now);
        true
    }

    /// May the frame at the head of the APRS queue key now? Always yes off the ISS channel. On it,
    /// only once the dial is the transmit frequency (the correction's, or 145.825 while the
    /// correction is off) and, if this module has moved the dial, the radio's own read-back says
    /// it is there. Until then the dial is moved there and the frame waits, at most
    /// [`UPLINK_TAKE`]. A dial someone else tuned ends the correction and drops the frame.
    pub(super) fn aprs_iss_ready_to_key(&mut self) -> bool {
        if !self.aprs_iss_live() || self.sat_dial_owner.is_some() {
            return true;
        }
        if self.aprs_iss_dial_taken() {
            self.aprs_iss_end("tunedAway", APRS_ISS_TUNED_AWAY);
            return false;
        }
        if self.aprs_iss_drop_expired() {
            return false;
        }
        let target = self.aprs_iss_legs().map_or(ISS_APRS_HZ, |l| l.1);
        let dial = self.settings.dial_hz();
        let radio_there =
            self.aprs_iss.acked_hz == Some(target) && self.aprs_iss.read_back_hz == Some(target);
        if dial == target && (!self.aprs_iss.moved || radio_there) {
            return true;
        }
        let deadline = match self.aprs_iss.over {
            Some(Over::Staged { deadline }) => deadline,
            _ => Instant::now() + UPLINK_TAKE,
        };
        self.aprs_iss.over = Some(Over::Staged { deadline });
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
            len: Duration::from_millis(samples as u64 / 12),
            since: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// The tick clock in these tests: `ms` after a fixed start.
    fn at(ms: u64) -> Instant {
        static T0: OnceLock<Instant> = OnceLock::new();
        *T0.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

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

    /// The loop pushed the dial, the radio acknowledged it, and its next read-back says the same.
    fn radio_takes_the_dial(e: &mut Engine) {
        let hz = e.settings().dial_hz();
        e.rig_dial_applied(hz);
        e.rig_freq_read(hz);
    }

    /// The loop pushed the dial and the radio acknowledged it, but it reads back `step`-rounded,
    /// in the order the loop reports a read: the read-back first, then the knob adoption.
    fn radio_rounds_the_dial(e: &mut Engine, step: u64) -> u64 {
        let hz = e.settings().dial_hz();
        let read = (hz + step / 2) / step * step;
        assert_ne!(read, hz, "precondition: the radio rounds this dial");
        e.rig_dial_applied(hz);
        e.rig_freq_read(read);
        e.observe_rig_freq(read);
        read
    }

    /// The waiting frame's 5 s have run out (the monotonic deadline, moved instead of slept).
    fn the_uplink_wait_runs_out(e: &mut Engine) {
        match e.aprs_iss.over.as_mut() {
            Some(Over::Staged { deadline }) => {
                *deadline = Instant::now() - Duration::from_millis(1)
            }
            other => panic!("no frame waiting: {other:?}"),
        }
    }

    fn beacon(e: &mut Engine) {
        e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[]).unwrap();
    }

    const APPROACHING: IssSky = IssSky::Up {
        range_rate_km_s: -6.0,
    };

    #[test]
    fn approaching_the_radio_listens_high_and_an_over_is_sent_low() {
        let mut e = on_the_iss();
        assert!(e.aprs_iss_tick(APPROACHING, at(10_000), false));
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
        assert!(!e.aprs_iss_tick(APPROACHING, at(10_000), false));
        assert_eq!(e.settings().dial_hz(), 144_390_000);
        assert!(e.aprs_iss_doppler().is_none());
        // Control: back on the ISS, the same tick corrects.
        e.aprs_tune(145.825).unwrap();
        assert!(e.aprs_iss_tick(APPROACHING, at(20_000), false));
    }

    /// ⛔ NEVER RETUNE WHILE KEYED. Control: the same tick, unkeyed, moves the dial.
    #[test]
    fn a_correction_due_while_keyed_is_refused_and_lands_after_the_unkey() {
        let mut e = on_the_iss();
        assert!(
            !e.aprs_iss_tick(APPROACHING, at(10_000), true),
            "moved while keyed"
        );
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        assert!(
            e.aprs_iss_tick(APPROACHING, at(11_000), false),
            "control: unkeyed it moves"
        );
        assert_ne!(e.settings().dial_hz(), ISS_APRS_HZ);
    }

    #[test]
    fn small_or_rapid_corrections_are_not_written() {
        let mut e = on_the_iss();
        assert!(e.aprs_iss_tick(APPROACHING, at(10_000), false));
        // ~0.1 km/s change = ~50 Hz: under the step.
        let near = IssSky::Up {
            range_rate_km_s: -5.9,
        };
        assert!(
            !e.aprs_iss_tick(near, at(20_000), false),
            "a 50 Hz move was written"
        );
        // A 1.5 kHz move half a second later waits for the interval…
        let far = IssSky::Up {
            range_rate_km_s: -3.0,
        };
        assert!(
            !e.aprs_iss_tick(far, at(10_500), false),
            "two writes inside a second"
        );
        // …and lands once it has passed.
        assert!(e.aprs_iss_tick(far, at(11_000), false));
    }

    /// The transmit frequency is set BEFORE the key: the frame waits until the radio reads back
    /// there, keys there, and the dial holds through the over, returning to the receive
    /// correction after it.
    #[test]
    fn the_uplink_is_set_before_keying_held_for_the_over_and_released_after() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
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
        assert!(!e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert_eq!(e.settings().dial_hz(), tx);
        radio_takes_the_dial(&mut e);
        let audio = e
            .poll_aprs_tx()
            .expect("keys once the radio is on the uplink");
        assert_eq!(e.settings().dial_hz(), tx, "keyed on the uplink");
        let len_ms = audio.len() as u64 / 12;
        // Through the whole over (and its margin) nothing moves, keyed or not.
        assert!(!e.aprs_iss_tick(APPROACHING, at(12_000), true));
        assert!(!e.aprs_iss_tick(APPROACHING, at(12_000 + len_ms), false));
        assert_eq!(e.settings().dial_hz(), tx, "the uplink held for the over");
        // After it, back to the receive correction straight away.
        let margin = OVER_MARGIN.as_millis() as u64;
        assert!(e.aprs_iss_tick(APPROACHING, at(12_000 + len_ms + margin), false));
        assert_eq!(e.settings().dial_hz(), rx);
    }

    /// The radio's ACKNOWLEDGEMENT of the uplink is not enough: rigctld acks a write the radio
    /// rounds. The frame keys only on the radio's read-back of the exact uplink.
    #[test]
    fn the_uplink_is_confirmed_by_the_radios_read_back_not_its_ack() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        let tx = e.settings().dial_hz();
        e.rig_dial_applied(tx);
        assert!(e.poll_aprs_tx().is_none(), "keyed on the ack alone");
        // Control: the read-back says the same, and it keys there.
        e.rig_freq_read(tx);
        assert!(
            e.poll_aprs_tx().is_some(),
            "read back on the uplink, not keyed"
        );
        assert_eq!(e.settings().dial_hz(), tx);
    }

    /// A radio that rounds the uplink it is sent never keys off target: the waiting frame is
    /// dropped and the cockpit says why, the correction is off for the pass, the radio goes back
    /// to 145.825 ONCE (no CAT write a second), and the next packet keys there, the gate judging
    /// 145.825.
    /// A confirmation counts only for the dial it confirmed. After an over the receive
    /// correction's write is never acknowledged (a link fault), and the next frame's uplink is
    /// the same 100 Hz as the last: the frame must not key on the previous over's ack and
    /// read-back. Control: a fresh ack and read-back of the uplink keys it.
    #[test]
    fn a_frame_never_keys_on_a_confirmation_from_before_the_dial_last_moved() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        let rx = e.settings().dial_hz();
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        let tx = e.settings().dial_hz();
        radio_takes_the_dial(&mut e);
        let audio = e.poll_aprs_tx().expect("keys on the confirmed uplink");
        let len_ms = audio.len() as u64 / 12;
        assert!(!e.aprs_iss_tick(APPROACHING, at(12_000), true));
        let margin = OVER_MARGIN.as_millis() as u64;
        assert!(e.aprs_iss_tick(APPROACHING, at(12_000 + len_ms + margin), false));
        assert_eq!(e.settings().dial_hz(), rx, "precondition: back on receive");
        // The radio never acknowledges the receive write. The next frame moves the dial back.
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        assert_eq!(e.settings().dial_hz(), tx, "precondition: the same uplink");
        assert!(
            e.poll_aprs_tx().is_none(),
            "keyed on the previous over's confirmation"
        );
        radio_takes_the_dial(&mut e);
        assert!(
            e.poll_aprs_tx().is_some(),
            "a fresh confirmation did not key"
        );
        assert_eq!(e.settings().dial_hz(), tx);
    }

    /// A driver that hangs (no panic, no more ticks) must not leave frames keying on a frozen
    /// correction: once its reading is older than [`SKY_FRESH`] the correction is off and says it
    /// stopped, and the next frame goes out on 145.825. Control: a fresh reading turns it back on.
    #[test]
    fn a_sky_reading_the_driver_stopped_refreshing_turns_the_correction_off() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        assert!(e.aprs_iss_doppler().unwrap().on, "precondition: on");
        e.aprs_iss.sky_at = Some(Instant::now() - SKY_FRESH - Duration::from_millis(1));
        let d = e.aprs_iss_doppler().unwrap();
        assert_eq!((d.on, d.why_off), (false, Some("stopped")));
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        assert_eq!(
            e.settings().dial_hz(),
            ISS_APRS_HZ,
            "a frame went to a correction the driver no longer updates"
        );
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_some());
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        e.aprs_iss_tick(APPROACHING, at(20_000), false);
        assert!(
            e.aprs_iss_doppler().unwrap().on,
            "a fresh reading stayed off"
        );
    }

    #[test]
    fn a_radio_that_rounds_the_uplink_drops_the_frame_and_the_correction_stops() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        let rounded = radio_rounds_the_dial(&mut e, 5_000);
        assert!(e.poll_aprs_tx().is_none(), "keyed off target");
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_RADIO_ROUNDS));
        let d = e.aprs_iss_doppler().unwrap();
        assert_eq!((d.on, d.why_off), (false, Some("radioRounds")));
        assert_eq!(e.settings().dial_hz(), rounded, "not a tune: nothing ended");
        // Back to the channel once, and then no more writes for the pass.
        assert!(e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        radio_takes_the_dial(&mut e);
        for i in 2..12 {
            assert!(
                !e.aprs_iss_tick(APPROACHING, at(10_000 + i * 1_000), false),
                "a CAT write at tick {i}"
            );
        }
        beacon(&mut e);
        assert!(
            e.poll_aprs_tx().is_some(),
            "the channel keys as with the correction off"
        );
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
    }

    /// The receive correction on a rounding radio: one write, one return, then quiet; the next
    /// pass tries the radio again (control: the correction is not off for good).
    #[test]
    fn a_radio_that_rounds_the_receive_correction_is_not_written_every_second() {
        let mut e = on_the_iss();
        let mut writes = 0;
        for i in 0..10 {
            if e.aprs_iss_tick(APPROACHING, at(10_000 + i * 1_000), false) {
                writes += 1;
                if e.settings().dial_hz() == ISS_APRS_HZ {
                    radio_takes_the_dial(&mut e);
                } else {
                    radio_rounds_the_dial(&mut e, 5_000);
                }
            }
        }
        assert_eq!(writes, 2, "one correction and one return, not one a second");
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("radioRounds"));
        e.aprs_iss_tick(IssSky::BelowHorizon, at(30_000), false);
        assert!(
            e.aprs_iss_tick(APPROACHING, at(31_000), false),
            "the next pass tries again"
        );
    }

    /// A radio that cannot hold 145.825 itself either: nothing to steer back to, so the
    /// correction ends for this pick; no further writes.
    #[test]
    fn a_radio_that_rounds_the_channel_too_ends_the_correction() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_rounds_the_dial(&mut e, 5_000);
        assert!(e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        let read = radio_rounds_the_dial(&mut e, 10_000);
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("radioRounds"));
        assert!(!e.aprs_iss_wants_sky());
        assert!(!e.aprs_iss_tick(APPROACHING, at(12_000), false));
        assert_eq!(e.settings().dial_hz(), read);
    }

    /// ⛔ NEVER FIGHT THE KNOB. The operator tunes off the channel during a pass: the correction
    /// ends, the cockpit says so, a remote hold on the channel ends, and the dial is never pulled
    /// back.
    #[test]
    fn a_knob_move_in_band_ends_the_correction_and_is_never_pulled_back() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        e.remote_aprs_hold = Some(e.settings().dial_hz());
        e.rig_freq_read(146_520_000);
        e.observe_rig_freq(146_520_000);
        for i in 1..5 {
            assert!(!e.aprs_iss_tick(APPROACHING, at(10_000 + i * 1_000), false));
        }
        assert_eq!(e.settings().dial_hz(), 146_520_000, "pulled back");
        let d = e.aprs_iss_doppler().unwrap();
        assert_eq!((d.on, d.why_off), (false, Some("tunedAway")));
        assert!(!e.aprs_iss_wants_sky());
        assert_eq!(
            e.remote_aprs_hold, None,
            "the remote hold outlived the channel"
        );
        // A packet now goes out on the operator's dial, judged there, with no ISS wait.
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_some());
        assert_eq!(e.settings().dial_hz(), 146_520_000);
    }

    #[test]
    fn a_knob_move_to_another_band_is_never_pulled_back_to_2m() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        e.observe_rig_freq(446_000_000);
        assert!(!e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert!(!e.aprs_iss_tick(IssSky::BelowHorizon, at(12_000), false));
        assert_eq!(
            e.settings().dial_hz(),
            446_000_000,
            "pulled back across bands"
        );
        assert!(!e.aprs_iss_wants_sky());
    }

    /// The knob moves while a frame waits for the uplink: the frame was meant for the ISS, so it
    /// is dropped with a notice, not sent on the operator's new frequency.
    #[test]
    fn a_knob_move_while_a_frame_waits_drops_it() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        e.observe_rig_freq(146_520_000);
        assert!(e.poll_aprs_tx().is_none(), "keyed on the knob's frequency");
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_TUNED_AWAY));
        assert_eq!(e.settings().dial_hz(), 146_520_000);
        assert!(e.poll_aprs_tx().is_none(), "the dropped frame keyed later");
    }

    /// ⛔ NEVER MOVE A DIAL THIS MODULE DID NOT MOVE. The radio reads the channel back a few
    /// hertz off (still the channel): with the correction off, nothing "fixes" it. Control: once
    /// this module has moved the dial, going off returns it to the channel.
    #[test]
    fn a_dial_this_module_never_moved_is_never_sent_home() {
        let mut e = on_the_iss();
        e.observe_rig_freq(ISS_APRS_HZ + 30);
        assert!(!e.aprs_iss_tick(IssSky::BelowHorizon, at(10_000), false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ + 30);
        assert_eq!(
            e.aprs_iss_doppler().unwrap().why_off,
            Some("belowHorizon"),
            "a few hertz is not a tune"
        );
        assert!(e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert!(e.aprs_iss_tick(IssSky::BelowHorizon, at(12_000), false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
    }

    /// Stop TX while a frame waits for the uplink: the dial returns to the receive correction on
    /// the next tick (not 5 s later), no "radio did not move" notice follows, and a frame queued
    /// after it gets its own full wait.
    #[test]
    fn stop_tx_while_a_frame_waits_returns_the_dial_to_receive() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        let rx = e.settings().dial_hz();
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        assert_ne!(e.settings().dial_hz(), rx, "precondition: on the uplink");
        e.halt_tx();
        assert!(e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert_eq!(
            e.settings().dial_hz(),
            rx,
            "left on the uplink after Stop TX"
        );
        assert_ne!(e.aprs_tx_notice(), Some(APRS_ISS_UPLINK_NOT_TAKEN));
        e.set_tx_enabled(true);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        match e.aprs_iss.over {
            Some(Over::Staged { deadline }) => assert!(
                deadline > Instant::now() + UPLINK_TAKE - Duration::from_secs(1),
                "the new frame inherited the old wait"
            ),
            other => panic!("the new frame is not waiting: {other:?}"),
        }
    }

    /// The licence gate judges the frequency keyed: on the pass that lets the frame go, the dial
    /// the gate reads is the uplink. Control: a frame refused there is dropped, not keyed later.
    #[test]
    fn the_privilege_gate_sees_the_uplink_and_still_refuses() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
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
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        e.aprs_iss_tick(APPROACHING, at(11_000), false);
        assert!(e.poll_aprs_tx().is_none());
        assert!(e.aprs_tx_notice().is_none(), "gave up early");
        the_uplink_wait_runs_out(&mut e);
        e.aprs_iss_tick(APPROACHING, at(12_000), false);
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_UPLINK_NOT_TAKEN));
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_none(), "the dropped frame keyed later");
    }

    /// ⛔ DROPPED, NEVER HELD, without the driver too: the wait is judged in the poll against the
    /// frame's own monotonic deadline, so a frame keys late even if no tick ever comes.
    #[test]
    fn a_frame_past_its_wait_is_dropped_by_the_poll_without_any_tick() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        the_uplink_wait_runs_out(&mut e);
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_none(), "keyed after its wait ran out");
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_UPLINK_NOT_TAKEN));
    }

    /// CAT lost mid-pass: a waiting frame is dropped with a notice that names CAT, not one that
    /// blames the radio for not moving.
    #[test]
    fn cat_lost_mid_pass_drops_the_frame_naming_cat() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        e.cat_status.0 = Some(false);
        assert!(e.aprs_iss_tick(APPROACHING, at(11_000), false));
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("noCat"));
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        the_uplink_wait_runs_out(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_NO_CAT));
    }

    /// The driver thread stopped: a waiting frame is dropped (never keyed later), the cockpit
    /// says the correction stopped, and the next packet goes out on 145.825.
    #[test]
    fn the_driver_stopping_drops_a_waiting_frame_and_turns_the_correction_off() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        radio_takes_the_dial(&mut e);
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        e.aprs_iss_driver_stopped();
        assert_eq!(e.aprs_tx_notice(), Some(APRS_ISS_STOPPED));
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_none(), "the waiting frame keyed later");
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("stopped"));
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_none());
        assert_eq!(
            e.settings().dial_hz(),
            ISS_APRS_HZ,
            "the next packet goes home first"
        );
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_some());
    }

    /// Off below the horizon (and without elements): back on 145.825, and the reason is given.
    #[test]
    fn below_the_horizon_the_radio_returns_to_the_channel_and_says_why() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        assert_ne!(e.settings().dial_hz(), ISS_APRS_HZ);
        assert!(e.aprs_iss_tick(IssSky::BelowHorizon, at(10_200), false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        let d = e.aprs_iss_doppler().unwrap();
        assert_eq!((d.on, d.why_off), (false, Some("belowHorizon")));
        for (sky, why) in [
            (IssSky::NoElements, "noElements"),
            (IssSky::StaleElements, "staleElements"),
            (IssSky::NoGrid, "noGrid"),
        ] {
            assert!(!e.aprs_iss_tick(sky, at(20_000), false), "{why}: moved");
            assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
            assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some(why));
        }
        // An over below the horizon keys on 145.825 as before: nothing to wait for.
        beacon(&mut e);
        radio_takes_the_dial(&mut e);
        assert!(e.poll_aprs_tx().is_some());
    }

    #[test]
    fn the_switch_turns_it_off_and_no_cat_means_no_correction() {
        let mut e = on_the_iss();
        let mut s = e.settings().clone();
        s.aprs_iss_doppler = false;
        e.apply_settings(s);
        assert!(!e.aprs_iss_tick(APPROACHING, at(10_000), false));
        assert_eq!(e.settings().dial_hz(), ISS_APRS_HZ);
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("switchedOff"));
        // Off, a beacon keys on 145.825 with no wait (today's behaviour).
        beacon(&mut e);
        assert!(e.poll_aprs_tx().is_some());

        let mut e = on_the_iss();
        e.cat_status.0 = None;
        assert!(!e.aprs_iss_tick(APPROACHING, at(10_000), false));
        assert_eq!(e.aprs_iss_doppler().unwrap().why_off, Some("noCat"));
    }

    #[test]
    fn a_qsy_ends_the_iss_correction() {
        let mut e = on_the_iss();
        e.aprs_iss_tick(APPROACHING, at(10_000), false);
        e.set_frequency(146.520, "2m", "FM");
        assert!(!e.aprs_iss_tick(APPROACHING, at(20_000), false));
        assert_eq!(e.settings().dial_hz(), 146_520_000);
        assert!(e.aprs_iss_doppler().is_none());
    }
}
