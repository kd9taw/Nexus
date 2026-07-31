//! Rotator pointing policy — the decisions an az/el rotator needs that are not
//! "where is the bird".
//!
//! Pure arithmetic, no I/O, so the behaviour that moves a mast full of aluminium
//! is testable on the bench. The loop that owns the wire calls in here.

/// What to do with the antenna when a pass ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PostPass {
    /// Leave it wherever the pass finished (default — never move a mast the
    /// operator didn't ask to move).
    #[default]
    Stop,
    /// Drive to the stow position: wind-safe, usually el 90 or a mast rest.
    Park,
    /// Drive to the pre-armed position for the NEXT pass.
    Ready,
}

/// A rotator's configured positions and its mechanical manners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotatorConfig {
    /// Stow position (deg).
    pub park_az: f64,
    pub park_el: f64,
    /// Waiting position (deg).
    pub ready_az: f64,
    pub ready_el: f64,
    pub post_pass: PostPass,
    /// Don't send a new command until the target differs from what was last
    /// commanded by at least this much. Without a deadband a rotator HUNTS:
    /// the relays chatter for the whole pass, which is audible in the shack and
    /// hard on the controller.
    pub tol_az_deg: f64,
    pub tol_el_deg: f64,
    /// Mechanical trim (deg), added to every command — the difference between
    /// where the controller thinks it is pointing and where the boom actually
    /// points.
    pub cal_az_deg: f64,
    pub cal_el_deg: f64,
    /// May the rotator go past 90° elevation? A flip is how a full az/el mount
    /// takes a high pass without spinning the mast through 180° at the top —
    /// but plenty of rotators physically cannot, so it is opt-in.
    pub allow_flip: bool,
}

impl Default for RotatorConfig {
    fn default() -> Self {
        RotatorConfig {
            park_az: 0.0,
            park_el: 0.0,
            ready_az: 0.0,
            ready_el: 0.0,
            post_pass: PostPass::Stop,
            // 2° is roughly a G-5500's own resolution; below that a command is
            // noise, not motion.
            tol_az_deg: 2.0,
            tol_el_deg: 2.0,
            cal_az_deg: 0.0,
            cal_el_deg: 0.0,
            allow_flip: false,
        }
    }
}

/// Where the rotator should actually be pointed for a look angle.
///
/// # The flip
///
/// A pass straight overhead sweeps azimuth through 180° in seconds. A mount that
/// can drive past 90° elevation instead points at `az + 180°` and keeps going
/// *over the top* — the antenna ends up in the same place in the sky without the
/// mast racing round underneath it. Only when the operator says the rotator can
/// do it; the default is the safe one.
pub fn point_for(az_deg: f64, el_deg: f64, cfg: &RotatorConfig) -> (f64, f64) {
    let (mut az, mut el) = (az_deg, el_deg);
    if cfg.allow_flip && el > 90.0 {
        az += 180.0;
        el = 180.0 - el;
    }
    az = (az + cfg.cal_az_deg).rem_euclid(360.0);
    el = (el + cfg.cal_el_deg).clamp(0.0, 180.0);
    (az, el)
}

/// Is this new target far enough from the last COMMANDED position to be worth a
/// command? The shortest angular distance is used for azimuth, so 359° → 1° is
/// 2° of motion, not 358°.
pub fn worth_moving(
    target: (f64, f64),
    last_commanded: Option<(f64, f64)>,
    cfg: &RotatorConfig,
) -> bool {
    let Some((laz, lel)) = last_commanded else {
        return true; // nothing commanded yet: always point
    };
    az_distance(target.0, laz) >= cfg.tol_az_deg || (target.1 - lel).abs() >= cfg.tol_el_deg
}

/// Where to go when the pass is over, or `None` to stay put.
pub fn post_pass_target(cfg: &RotatorConfig) -> Option<(f64, f64)> {
    match cfg.post_pass {
        PostPass::Stop => None,
        PostPass::Park => Some((cfg.park_az, cfg.park_el)),
        PostPass::Ready => Some((cfg.ready_az, cfg.ready_el)),
    }
}

/// Shortest angular distance between two azimuths (0..180).
pub fn az_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// What to do with the rotator on one tick of a pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotStep {
    /// Inside the deadband — send nothing. The antenna stays where it was last
    /// told to go, which is what makes the reported pointing error real.
    Hold,
    /// Command azimuth AND elevation.
    PointAzEl { az: f64, el: f64 },
    /// Command azimuth only: this rotator has refused elevation, so claiming
    /// one would be a command we never issued.
    PointAz { az: f64 },
}

/// What actually happened on the wire, fed back so the driver can learn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotOutcome {
    /// The az/el command was accepted.
    AzElOk,
    /// az/el was REFUSED but plain azimuth was accepted — an az-only rotator.
    AzOnly,
    /// Azimuth alone was accepted (already in az-only mode).
    AzOk,
    /// Nothing reached the rotator.
    Failed,
}

/// ~60 s of recovery probing at the loop's 3 s cadence.
const PROBE_AFTER_TICKS: u32 = 20;
/// Consecutive failures before a pass gives up on the rotator.
pub const MISS_LIMIT: u32 = 5;

/// The per-tick tracking policy: deadband, az-only fallback and its recovery
/// probe, and the miss counter.
///
/// This lives here, away from the loop that owns the socket, for the same
/// reason everything else in this module does: it decides how a mast full of
/// aluminium moves, and it has to be testable without one. The loop is then
/// only I/O — ask [`Self::step`] what to do, do it, report back with
/// [`Self::record`].
#[derive(Debug, Clone)]
pub struct TrackDriver {
    cfg: RotatorConfig,
    /// What the rotator was last actually TOLD, in the CONTROLLER's frame. The
    /// deadband compares against this.
    last_cmd: Option<(f64, f64)>,
    /// The same command as the BORESIGHT angle it was aiming at — the frame a
    /// sky dome draws in, which is the controller frame minus the calibration
    /// trim. Kept alongside rather than derived so the two can never drift.
    last_aim: Option<(f64, f64)>,
    /// False once the rotator has refused an elevation.
    azel_ok: bool,
    az_only_ticks: u32,
    misses: u32,
}

impl TrackDriver {
    pub fn new(cfg: RotatorConfig) -> Self {
        TrackDriver {
            cfg,
            last_cmd: None,
            last_aim: None,
            azel_ok: true,
            az_only_ticks: 0,
            misses: 0,
        }
    }

    /// Where the antenna was last aimed, as a boresight look angle, and whether
    /// an elevation was part of it. `None` until something has actually been
    /// sent — the armed phase commands nothing, and reporting a position then
    /// would claim a command deliberately withheld.
    pub fn last_aim(&self) -> Option<(f64, f64)> {
        self.last_aim
    }

    /// True while the rotator is still accepting elevation.
    pub fn azel_ok(&self) -> bool {
        self.azel_ok
    }

    /// Has the rotator stopped answering?
    pub fn gave_up(&self) -> bool {
        self.misses >= MISS_LIMIT
    }

    /// Decide this tick from the bird's boresight look angle.
    pub fn step(&mut self, look_az: f64, look_el: f64) -> RotStep {
        let (az, el) = point_for(look_az, look_el, &self.cfg);
        if !worth_moving((az, el), self.last_cmd, &self.cfg) {
            return RotStep::Hold;
        }
        if self.azel_ok {
            return RotStep::PointAzEl { az, el };
        }
        // In az-only mode, periodically re-offer elevation: the original
        // refusal may have been a transient comms error rather than a rotator
        // that genuinely has no elevation axis, and a whole pass tracked flat
        // because of one dropped reply is a bad trade.
        //
        // The counter advances HERE, on ticks that actually reach the wire —
        // not on every loop iteration. Counting suppressed ticks made the
        // "~60 s" probe arrive after a minute of wall time on a fast-moving
        // bird and never at all on a slow one near the horizon, where almost
        // every tick is inside the deadband.
        self.az_only_ticks += 1;
        if self.az_only_ticks >= PROBE_AFTER_TICKS {
            self.az_only_ticks = 0;
            RotStep::PointAzEl { az, el }
        } else {
            RotStep::PointAz { az }
        }
    }

    /// Feed back what the wire actually did. `step_sent` is the command the
    /// loop attempted, so the driver records what was really issued rather than
    /// what was wanted.
    pub fn record(&mut self, step: RotStep, outcome: RotOutcome, look_az: f64, look_el: f64) {
        let (az, el) = point_for(look_az, look_el, &self.cfg);
        match outcome {
            RotOutcome::Failed => {
                self.misses += 1;
                return;
            }
            RotOutcome::AzElOk => self.azel_ok = true,
            RotOutcome::AzOnly => {
                // Learned just now that elevation is refused. Restart the probe
                // window so the next retry is a full interval away.
                self.azel_ok = false;
                self.az_only_ticks = 0;
            }
            RotOutcome::AzOk => {}
        }
        self.misses = 0;
        // Record what REACHED the rotator. On an az-only send the elevation was
        // never issued, so the stored pair keeps the elevation the rotator is
        // still physically at — which is what the deadband must compare against
        // if a pure-elevation change is not to trigger pointless azimuth
        // commands for the rest of the pass.
        let el_reached = match (step, outcome) {
            (RotStep::PointAzEl { .. }, RotOutcome::AzElOk) => el,
            _ => self.last_cmd.map(|c| c.1).unwrap_or(el),
        };
        self.last_cmd = Some((az, el_reached));
        self.last_aim = Some((look_az, look_el));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deadband_stops_the_rotator_hunting() {
        // Without this the relays chatter for the whole pass — audible in the
        // shack and hard on the controller.
        let cfg = RotatorConfig::default(); // 2° az/el
        assert!(
            worth_moving((100.0, 30.0), None, &cfg),
            "first point always goes"
        );
        assert!(!worth_moving((100.5, 30.5), Some((100.0, 30.0)), &cfg));
        assert!(worth_moving((103.0, 30.0), Some((100.0, 30.0)), &cfg));
        assert!(worth_moving((100.0, 33.0), Some((100.0, 30.0)), &cfg));
    }

    #[test]
    fn azimuth_wrap_is_the_short_way_round() {
        // 359° → 1° is 2° of motion. Treating it as 358° would both spin the
        // mast the long way and defeat the deadband at the wrap point.
        assert!((az_distance(359.0, 1.0) - 2.0).abs() < 1e-9);
        assert!((az_distance(1.0, 359.0) - 2.0).abs() < 1e-9);
        assert!((az_distance(10.0, 200.0) - 170.0).abs() < 1e-9);
        let cfg = RotatorConfig::default();
        assert!(!worth_moving((359.5, 20.0), Some((0.5, 20.0)), &cfg));
    }

    #[test]
    fn the_flip_is_opt_in_and_takes_the_pass_over_the_top() {
        let mut cfg = RotatorConfig::default();
        // Default: no flip. A >90° look angle is clamped, never mirrored —
        // a rotator that cannot flip must not be commanded past its stop.
        let (az, el) = point_for(100.0, 100.0, &cfg);
        assert_eq!(az, 100.0);
        assert!(
            el > 90.0,
            "no flip ⇒ elevation is passed through, az untouched"
        );

        cfg.allow_flip = true;
        let (az, el) = point_for(100.0, 100.0, &cfg);
        assert_eq!(az, 280.0, "flip swings azimuth 180°");
        assert_eq!(el, 80.0, "…and elevation comes back down the far side");
        // Below the flip point nothing changes.
        assert_eq!(point_for(100.0, 80.0, &cfg), (100.0, 80.0));
    }

    #[test]
    fn calibration_trim_applies_to_every_command_and_wraps() {
        let cfg = RotatorConfig {
            cal_az_deg: -5.0,
            cal_el_deg: 1.5,
            ..RotatorConfig::default()
        };
        assert_eq!(point_for(10.0, 20.0, &cfg), (5.0, 21.5));
        // Trim across the 0° boundary must wrap, not go negative.
        assert_eq!(point_for(2.0, 20.0, &cfg).0, 357.0);
    }

    #[test]
    fn the_driver_holds_inside_the_deadband_and_moves_outside_it() {
        let mut d = TrackDriver::new(RotatorConfig::default()); // 2°
        assert_eq!(
            d.last_aim(),
            None,
            "nothing commanded yet is not a position"
        );
        let s = d.step(100.0, 30.0);
        assert_eq!(
            s,
            RotStep::PointAzEl {
                az: 100.0,
                el: 30.0
            }
        );
        d.record(s, RotOutcome::AzElOk, 100.0, 30.0);
        assert_eq!(d.last_aim(), Some((100.0, 30.0)));
        // Sub-deadband motion sends nothing at all.
        assert_eq!(d.step(100.5, 30.5), RotStep::Hold);
        assert_eq!(d.last_aim(), Some((100.0, 30.0)), "a hold moves nothing");
        // Past it, a command goes out.
        assert_eq!(
            d.step(103.0, 30.0),
            RotStep::PointAzEl {
                az: 103.0,
                el: 30.0
            }
        );
    }

    #[test]
    fn the_aim_is_the_boresight_angle_not_the_controller_command() {
        // The trim's definition is that the controller's numbers are offset
        // from where the boom points. A display drawing the controller number
        // against the satellite's true position would show a permanent error
        // equal to the trim — a fault on a correctly calibrated station.
        let cfg = RotatorConfig {
            cal_az_deg: -4.0,
            ..RotatorConfig::default()
        };
        let mut d = TrackDriver::new(cfg);
        let s = d.step(100.0, 30.0);
        assert_eq!(
            s,
            RotStep::PointAzEl { az: 96.0, el: 30.0 },
            "the WIRE gets the trimmed angle"
        );
        d.record(s, RotOutcome::AzElOk, 100.0, 30.0);
        assert_eq!(
            d.last_aim(),
            Some((100.0, 30.0)),
            "but the reported aim is where the BOOM points — no phantom 4° error"
        );
    }

    #[test]
    fn an_az_only_rotator_is_learned_and_then_re_probed() {
        let mut d = TrackDriver::new(RotatorConfig::default());
        let s = d.step(10.0, 20.0);
        // The wire refused elevation but took azimuth.
        d.record(s, RotOutcome::AzOnly, 10.0, 20.0);
        assert!(!d.azel_ok(), "elevation is off the table for now");

        // Subsequent ticks are azimuth-only...
        let mut probes = 0;
        let mut az = 10.0;
        for _ in 0..PROBE_AFTER_TICKS {
            az += 5.0; // always outside the deadband, so every tick reaches the wire
            match d.step(az, 20.0) {
                RotStep::PointAz { .. } => {}
                RotStep::PointAzEl { .. } => probes += 1,
                RotStep::Hold => panic!("5° a tick is not a hold"),
            }
            d.record(RotStep::PointAz { az }, RotOutcome::AzOk, az, 20.0);
        }
        assert_eq!(
            probes, 1,
            "…until exactly one recovery probe after the interval"
        );
    }

    #[test]
    fn the_probe_counts_ticks_that_reached_the_wire_not_suppressed_ones() {
        // A bird crawling along near the horizon spends most ticks inside the
        // deadband. Counting those would push the "~60 s" probe out to minutes
        // — or, on a slow enough pass, past LOS, so a rotator downgraded by one
        // dropped reply would track flat for the whole pass and never retry.
        let mut d = TrackDriver::new(RotatorConfig::default());
        let s = d.step(10.0, 20.0);
        d.record(s, RotOutcome::AzOnly, 10.0, 20.0);

        // 100 ticks of sub-deadband drift: all held, none counted.
        for i in 0..100 {
            assert_eq!(d.step(10.0 + (i as f64) * 0.001, 20.0), RotStep::Hold);
        }
        // Now real motion. The probe is still a full interval away.
        let mut az = 10.0;
        let mut probes = 0;
        for _ in 0..(PROBE_AFTER_TICKS - 1) {
            az += 5.0;
            if matches!(d.step(az, 20.0), RotStep::PointAzEl { .. }) {
                probes += 1;
            }
            d.record(RotStep::PointAz { az }, RotOutcome::AzOk, az, 20.0);
        }
        assert_eq!(probes, 0, "the held ticks must not have advanced the probe");
    }

    #[test]
    fn an_az_only_send_does_not_record_an_elevation_it_never_issued() {
        // Otherwise a pure-elevation change looks like motion the deadband
        // should act on, and the loop fires azimuth commands that repeat the
        // azimuth the rotator is already at — the relay chatter the deadband
        // exists to prevent.
        let mut d = TrackDriver::new(RotatorConfig::default());
        let s = d.step(10.0, 20.0);
        d.record(s, RotOutcome::AzOnly, 10.0, 20.0);
        // Elevation climbs 20° with azimuth unchanged. Nothing was ever sent to
        // an elevation axis, so the stored elevation must not have followed it.
        let s2 = d.step(10.0, 40.0);
        assert!(matches!(s2, RotStep::PointAz { .. }));
        d.record(s2, RotOutcome::AzOk, 10.0, 40.0);
        // A further pure-elevation change is still not azimuth motion.
        assert_eq!(d.step(10.0, 60.0), RotStep::PointAz { az: 10.0 });
    }

    #[test]
    fn the_driver_gives_up_only_after_repeated_silence() {
        let mut d = TrackDriver::new(RotatorConfig::default());
        for i in 1..MISS_LIMIT {
            let s = d.step(10.0 + i as f64 * 5.0, 20.0);
            d.record(s, RotOutcome::Failed, 10.0 + i as f64 * 5.0, 20.0);
            assert!(!d.gave_up(), "one dropped reply is not a dead rotator");
        }
        let s = d.step(90.0, 20.0);
        d.record(s, RotOutcome::Failed, 90.0, 20.0);
        assert!(d.gave_up());
        assert_eq!(d.last_aim(), None, "nothing ever reached the rotator");
    }

    #[test]
    fn one_success_forgives_the_misses() {
        let mut d = TrackDriver::new(RotatorConfig::default());
        for i in 1..MISS_LIMIT {
            let s = d.step(10.0 + i as f64 * 5.0, 20.0);
            d.record(s, RotOutcome::Failed, 10.0 + i as f64 * 5.0, 20.0);
        }
        let s = d.step(90.0, 20.0);
        d.record(s, RotOutcome::AzElOk, 90.0, 20.0);
        assert!(!d.gave_up());
        for i in 1..MISS_LIMIT {
            let s = d.step(90.0 + i as f64 * 5.0, 20.0);
            d.record(s, RotOutcome::Failed, 90.0 + i as f64 * 5.0, 20.0);
            assert!(!d.gave_up(), "the counter restarted at the success");
        }
    }

    #[test]
    fn post_pass_defaults_to_leaving_the_mast_alone() {
        let mut cfg = RotatorConfig::default();
        assert_eq!(post_pass_target(&cfg), None, "never move a mast unasked");
        cfg.post_pass = PostPass::Park;
        cfg.park_az = 180.0;
        cfg.park_el = 90.0;
        assert_eq!(post_pass_target(&cfg), Some((180.0, 90.0)));
        cfg.post_pass = PostPass::Ready;
        cfg.ready_az = 45.0;
        cfg.ready_el = 5.0;
        assert_eq!(post_pass_target(&cfg), Some((45.0, 5.0)));
    }
}
