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
