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

/// Above this peak elevation a flip-capable mount takes the pass over the top. gpredict and
/// SatPC32 both use ~85°; the number is the operator-visible meaning of "high pass", not a
/// tuning knob.
pub const FLIP_ABOVE_EL_DEG: f64 = 85.0;

/// Does THIS pass want the flipped frame?
///
/// ⭐ **The decision belongs to the PASS, and putting it on the look angle is why "Allow flip"
/// could never fire.** `point_for` used to flip when `el > 90.0`, and elevation above the
/// horizon cannot exceed 90 by construction — `sat::look_at` computes it as
/// `(zenith / range).asin().to_degrees()`, which `asin` bounds to [-90, 90], and even a true
/// zenith pass fails the strict `>`. So the branch was unreachable in the whole tracking path
/// (the unit test that "proved" it fed a synthetic 100° the propagator cannot produce), while
/// Settings told the operator the box would stop the mast racing round at the top of a pass.
/// It did not: the mast raced round on every high pass, exactly as if the box were off.
///
/// The CHOICE is made once, from the pass's peak elevation, and held: turning the flipped frame
/// on and off mid-pass would perform the very 180° azimuth swing it exists to avoid. Which
/// samples the frame then rewrites is [`point_for`]'s business, and it is not "all of them" —
/// see the reference-bearing note there.
///
/// Only high passes, because the flip is not free: it commands elevations past 90°, which many
/// mounts cannot reach at all, and on a low pass there is no mast race to avoid.
pub fn flip_for_pass(max_el_deg: f64, cfg: &RotatorConfig) -> bool {
    cfg.allow_flip && max_el_deg >= FLIP_ABOVE_EL_DEG
}

/// Where the rotator should actually be pointed for a look angle.
///
/// # The flip
///
/// A pass straight overhead sweeps azimuth through 180° in seconds. A mount that can drive past
/// 90° elevation instead points at `az + 180°` and keeps going *over the top* — the antenna
/// ends up in the same place in the sky without the mast racing round underneath it. Only when
/// the operator says the rotator can do it (`allow_flip`) and only on a pass that needs it
/// ([`flip_for_pass`]); the default is the safe one.
///
/// `flip_ref` is that decision: `None` for the ordinary frame, `Some(bearing)` to track this
/// pass over the top about that REFERENCE BEARING — the azimuth the pass rises on.
///
/// ⭐ **The reference is what makes the flip do anything, and it is subtle enough to get wrong
/// twice.** Mapping every sample to `az + 180` is not a flip: an overhead pass swings azimuth
/// 180° at the peak, and a uniformly rotated frame swings by exactly the same 180°. What
/// removes the swing is flipping only the HALF of the pass that lies on the far side of the
/// rise bearing. A pass rising at 045° and setting at 225° is then commanded 045° for its whole
/// length, with elevation running 0° → 90° → 180° straight through the zenith, and the mast
/// never turns at all. That is the behaviour the setting promises.
///
/// The elevation ceiling follows the frame: 90° in the ordinary one, 180° in the flipped one.
/// It has to, and the old code missed it twice over — `point_line_azel` then clamped every
/// command to 90 on the wire, so even a flip that fired would have been clamped away.
pub fn point_for(
    az_deg: f64,
    el_deg: f64,
    cfg: &RotatorConfig,
    flip_ref: Option<f64>,
) -> (f64, f64) {
    let (mut az, mut el) = (az_deg, el_deg);
    if flip_ref.is_some_and(|r| az_distance(az_deg, r) > 90.0) {
        az += 180.0;
        el = 180.0 - el;
    }
    az = (az + cfg.cal_az_deg).rem_euclid(360.0);
    el = (el + cfg.cal_el_deg).clamp(0.0, if flip_ref.is_some() { 180.0 } else { 90.0 });
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
    /// The bearing this pass rises on, when it is being tracked over the top; `None` in the
    /// ordinary frame. Decided once, at construction, from the pass's peak elevation — see
    /// [`flip_for_pass`] and [`point_for`].
    flip_ref: Option<f64>,
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
    /// A driver for the ordinary frame — no flip. What every caller that has no pass in hand
    /// wants, and what `for_pass` reduces to on a low pass or a mount that cannot flip.
    pub fn new(cfg: RotatorConfig) -> Self {
        TrackDriver {
            cfg,
            flip_ref: None,
            last_cmd: None,
            last_aim: None,
            azel_ok: true,
            az_only_ticks: 0,
            misses: 0,
        }
    }

    /// A driver for ONE pass, which is what decides whether the flip applies — see
    /// [`flip_for_pass`]. `aos_az_deg` is the bearing the pass rises on, which is the reference
    /// the flipped frame turns about. Fixed here and never changed mid-pass.
    pub fn for_pass(cfg: RotatorConfig, max_el_deg: f64, aos_az_deg: f64) -> Self {
        TrackDriver {
            flip_ref: flip_for_pass(max_el_deg, &cfg).then_some(aos_az_deg),
            ..TrackDriver::new(cfg)
        }
    }

    /// Is this pass being tracked over the top?
    pub fn flipped(&self) -> bool {
        self.flip_ref.is_some()
    }

    /// The last pair actually COMMANDED, in the controller's frame — what a position read back
    /// off the controller is comparable with (`last_aim` is the boresight angle, which differs
    /// by the calibration trim and by the flip).
    pub fn last_cmd(&self) -> Option<(f64, f64)> {
        self.last_cmd
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
        let (az, el) = point_for(look_az, look_el, &self.cfg, self.flip_ref);
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
        let (az, el) = point_for(look_az, look_el, &self.cfg, self.flip_ref);
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

/// How far the mast may sit from its last command before that is worth saying out loud.
const LAG_LIMIT_DEG: f64 = 15.0;
/// Below this, a controller has not moved between two reads — a G-5500 reports whole degrees
/// and a slewing mast covers several per tick, so anything under a degree is standing still.
const STUCK_MOVE_DEG: f64 = 1.0;
/// Consecutive stuck-and-far reads before the operator is told. At the loop's 3 s cadence this
/// is ~9 s of a mast that is neither where it was told to go nor on its way there.
const LAG_STRIKES: u32 = 3;

/// Is the mast actually following? Pure, so the answer is testable without one.
///
/// ⚠️ **A pass never read the rotator's position back at all** — the deadband compares the new
/// target against the last COMMAND, so it self-satisfies, and the badge draws the commanded
/// pair. A rotator that accepted every command and then jammed, lost a belt, or hit a stop was
/// invisible for the whole pass; the sky dome's "pointing error" was computed from two numbers
/// that agreed by construction.
///
/// The rule is deliberately NOT "the reading is far from the target": a legitimate slew to AOS
/// is 180° of gap and takes a G-5500 half a minute. It is **far AND not moving** — a mast that
/// is neither there nor on its way. Reported once per episode, and forgiven the moment it moves
/// again, because the operator does not need a second line about the same jam.
#[derive(Debug, Clone, Copy, Default)]
pub struct LagWatch {
    last_seen_az: Option<f64>,
    strikes: u32,
    reported: bool,
}

impl LagWatch {
    /// Feed one position read against the azimuth last commanded (both in the CONTROLLER's
    /// frame). `Some(gap)` exactly once per episode, when the mast has stopped short.
    pub fn observe(&mut self, commanded_az: f64, measured_az: f64) -> Option<f64> {
        let gap = az_distance(commanded_az, measured_az);
        let moved = self
            .last_seen_az
            .is_none_or(|prev| az_distance(prev, measured_az) >= STUCK_MOVE_DEG);
        self.last_seen_az = Some(measured_az);
        if gap < LAG_LIMIT_DEG || moved {
            self.strikes = 0;
            self.reported = false;
            return None;
        }
        self.strikes += 1;
        (self.strikes >= LAG_STRIKES && !self.reported).then(|| {
            self.reported = true;
            gap
        })
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
        let cfg = RotatorConfig::default();
        // Not flipped: azimuth untouched and elevation held at the mount's 90° stop. A rotator
        // that cannot flip must never be commanded past it.
        assert_eq!(point_for(100.0, 88.0, &cfg, None), (100.0, 88.0));
        assert_eq!(point_for(100.0, 100.0, &cfg, None), (100.0, 90.0));

        // Flipped about a rise bearing of 100°: the half of the pass on THAT side is commanded
        // unchanged, and the half on the far side is turned over the top — the same patch of
        // sky, reached without the mast crossing under it.
        let rise = Some(100.0);
        assert_eq!(
            point_for(100.0, 80.0, &cfg, rise),
            (100.0, 80.0),
            "the near half is not rewritten"
        );
        assert_eq!(
            point_for(280.0, 80.0, &cfg, rise),
            (100.0, 100.0),
            "the far half comes over the top"
        );
        assert_eq!(
            point_for(280.0, 0.0, &cfg, rise),
            (100.0, 180.0),
            "…all the way to the far horizon"
        );
        // 90° away is the boundary and stays on the near side: an ambiguous sample must not
        // flap between the two frames.
        assert_eq!(point_for(190.0, 45.0, &cfg, rise), (190.0, 45.0));
    }

    #[test]
    fn allow_flip_can_actually_fire_now_and_only_on_a_high_pass() {
        // ⭐ THE DEFECT, pinned. The flip used to be gated on `el > 90.0`, and a look angle
        // above the horizon cannot exceed 90 — `sat::look_at`'s elevation is an `asin`, bounded
        // to [-90, 90]. So the branch was dead in every pass the propagator can produce, while
        // Settings promised the mast would not race round at the top of a high one. It did.
        //
        // Old test's synthetic input, for the record: `point_for(100.0, 100.0, …)`. A 100°
        // elevation is not a thing.
        let mut cfg = RotatorConfig::default();
        assert!(!cfg.allow_flip);
        for max_el in [0.0, 45.0, 84.9, 85.0, 89.9, 90.0] {
            assert!(
                !flip_for_pass(max_el, &cfg),
                "with the box off nothing flips, however high the pass"
            );
        }

        cfg.allow_flip = true;
        // A real zenith pass — the case the setting exists for — now flips, and so does one at
        // exactly the threshold.
        assert!(flip_for_pass(90.0, &cfg));
        assert!(flip_for_pass(85.0, &cfg));
        assert!(flip_for_pass(88.7, &cfg));
        // …and an ordinary pass does not: flipping a low pass would spin the mast for nothing.
        assert!(!flip_for_pass(84.9, &cfg));
        assert!(!flip_for_pass(30.0, &cfg));
    }

    #[test]
    fn a_flipped_pass_crosses_the_zenith_without_the_mast_racing_round() {
        // The behaviour the operator was promised, over a pass that goes overhead: in the plain
        // frame the azimuth swings ~180° in the seconds around the peak (this is the mast race);
        // in the flipped frame it does not move at all, and the elevation walks up through 90
        // and back down.
        let cfg = RotatorConfig {
            allow_flip: true,
            ..RotatorConfig::default()
        };
        // A near-zenith pass sampled through the peak: it rises at 010°, climbs to 89°, goes
        // over the top and the same track continues on the opposite bearing, 190°.
        let pass = [(10.0, 86.0), (10.0, 89.0), (190.0, 89.0), (190.0, 86.0)];

        let plain: Vec<f64> = pass
            .iter()
            .map(|&(az, el)| point_for(az, el, &cfg, None).0)
            .collect();
        assert!(
            az_distance(plain[1], plain[2]) > 170.0,
            "the unflipped frame really does swing the mast round at the top: {plain:?}"
        );

        // 010° is where it rose, so that is the reference the flipped frame turns about.
        let flipped: Vec<(f64, f64)> = pass
            .iter()
            .map(|&(az, el)| point_for(az, el, &cfg, Some(10.0)))
            .collect();
        for w in flipped.windows(2) {
            assert!(
                az_distance(w[0].0, w[1].0) < 1.0,
                "a flipped pass must not swing the azimuth at all: {flipped:?}"
            );
        }
        assert!(flipped.iter().all(|&(az, _)| (az - 10.0).abs() < 1e-9));
        // Elevation instead walks straight up through 90 and out the far side — which is the
        // whole point, and what the wire's old hard 90° clamp would have thrown away.
        let els: Vec<f64> = flipped.iter().map(|&(_, el)| el).collect();
        assert_eq!(els, vec![86.0, 89.0, 91.0, 94.0]);
    }

    #[test]
    fn a_driver_built_for_a_pass_holds_one_frame_for_the_whole_pass() {
        let cfg = RotatorConfig {
            allow_flip: true,
            ..RotatorConfig::default()
        };
        // Rises at 010°, peaks at 89° — a flip pass.
        let mut d = TrackDriver::for_pass(cfg, 89.0, 10.0);
        assert!(d.flipped());
        // Still on the rise bearing: commanded as-is.
        assert_eq!(
            d.step(10.0, 20.0),
            RotStep::PointAzEl { az: 10.0, el: 20.0 }
        );
        d.record(
            RotStep::PointAzEl { az: 10.0, el: 20.0 },
            RotOutcome::AzElOk,
            10.0,
            20.0,
        );
        // Past the zenith, on the far bearing: the mast does not follow, the elevation does.
        assert_eq!(
            d.step(190.0, 20.0),
            RotStep::PointAzEl {
                az: 10.0,
                el: 160.0
            }
        );
        d.record(
            RotStep::PointAzEl {
                az: 10.0,
                el: 160.0,
            },
            RotOutcome::AzElOk,
            190.0,
            20.0,
        );
        // …and the reported AIM is still the boresight look angle, so nothing the operator
        // reads moves because of the flip.
        assert_eq!(d.last_aim(), Some((190.0, 20.0)));
        assert_eq!(d.last_cmd(), Some((10.0, 160.0)));

        // A low pass on the same station keeps the plain frame.
        assert!(!TrackDriver::for_pass(cfg, 40.0, 10.0).flipped());
        assert!(!TrackDriver::new(cfg).flipped());
    }

    #[test]
    fn a_mast_that_stopped_short_is_noticed_and_a_slewing_one_is_not() {
        // A legitimate slew is a huge gap that closes: 180° at AOS takes a G-5500 half a minute,
        // and calling that a fault would cry wolf on every pass.
        let mut w = LagWatch::default();
        let mut az = 10.0;
        for _ in 0..10 {
            az += 18.0; // ~6°/s over a 3 s tick
            assert_eq!(w.observe(190.0, az), None, "a mast on its way is not stuck");
        }
        // Now it stops, 40° short. The first read after a move only ESTABLISHES the position;
        // it takes a second to know nothing changed, and two more to call it a jam.
        for _ in 0..3 {
            assert_eq!(w.observe(190.0, 150.0), None);
        }
        let gap = w
            .observe(190.0, 150.0)
            .expect("a jammed mast must be reported");
        assert!(
            (gap - 40.0).abs() < 1e-9,
            "and it says how far short: {gap}"
        );
        // Said once, not once a tick.
        assert_eq!(w.observe(190.0, 150.0), None);
        assert_eq!(w.observe(190.0, 150.0), None);

        // It frees itself, moves, and stops short again: the episode ended, so the second jam
        // gets its own report.
        for _ in 0..3 {
            assert_eq!(w.observe(190.0, 165.0), None);
        }
        assert!(
            w.observe(190.0, 165.0).is_some(),
            "a second jam is a second report"
        );
    }

    #[test]
    fn a_mast_sitting_exactly_where_it_was_told_is_never_reported() {
        // The positive control's other half: a healthy rotator holds position for tick after
        // tick, and that is not-moving too. Only the GAP separates it from a jam.
        let mut w = LagWatch::default();
        for _ in 0..20 {
            assert_eq!(w.observe(190.0, 190.4), None);
        }
        // …and within the limit, still nothing, however long it sits.
        for _ in 0..20 {
            assert_eq!(w.observe(190.0, 180.0), None);
        }
    }

    #[test]
    fn calibration_trim_applies_to_every_command_and_wraps() {
        let cfg = RotatorConfig {
            cal_az_deg: -5.0,
            cal_el_deg: 1.5,
            ..RotatorConfig::default()
        };
        assert_eq!(point_for(10.0, 20.0, &cfg, None), (5.0, 21.5));
        // Trim across the 0° boundary must wrap, not go negative.
        assert_eq!(point_for(2.0, 20.0, &cfg, None).0, 357.0);
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
