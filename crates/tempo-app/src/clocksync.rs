//! What the radio loop is allowed to believe about the PC clock.
//!
//! The SNTP probe (`tempo_net::sntp::measure`) produces corroborated
//! measurements; `service.rs` steers every TX key and decode window by
//! subtracting one from the system clock. Between those two sits this module,
//! and it exists because the raw number is not enough on its own:
//!
//! - **A measurement goes stale.** Off-grid — a POTA/SOTA operator walking out
//!   of coverage — the probe stops answering. Until 2026-09 the offset was then
//!   set to `None` and every reader fell through `unwrap_or(0)`, so the
//!   correction *silently vanished* on exactly the machines that had needed it,
//!   at exactly the moment nothing could tell the operator. [`ClockOffset`]
//!   holds the last good value with an explicit age and an explicit expiry, so
//!   steering degrades on a schedule the operator can see instead of falling off
//!   a cliff nobody announces.
//! - **The OS steps the clock underneath us.** Windows restores the system clock
//!   from the RTC on resume from sleep — observed stepping by 3 h 06 m on a real
//!   machine — and W32Time can take hours to re-synchronise after that. A held
//!   offset measured before such a step describes a clock that no longer exists,
//!   and applying it makes the station wrong *the other way*.
//!   [`ClockJumpDetector`] watches the wall clock against a monotonic one and
//!   says so within a tick.
//!
//! The whole module is pure: every function takes its clocks as arguments, so
//! the tests below drive a stepped wall clock past a normally-advancing
//! monotonic one without waiting for anything real.

use std::time::{Duration, Instant};

/// The residual error budget the hold window is sized against, in ms.
///
/// TempoFast's decode window is a **cliff, not a slope**: −0.30 s / +0.60 s
/// around the slot (`crates/modes/src/mode.rs`, `crates/tempo-audio/src/slot.rs`).
/// A held offset does not itself decay — the *clock under it* drifts away from
/// the moment it was measured — so this is the drift we are willing to
/// accumulate before the held value stops being worth applying. 250 ms leaves
/// 50 ms of the fast-side budget for everything else in the chain.
const HOLD_BUDGET_MS: f64 = 250.0;

/// Rate error assumed when we have no drift estimate of our own, in ppm.
///
/// The low end of the *undisciplined* regime — a clock whose OS time service is
/// absent, disabled or has never converged (10–50 ppm). Deliberately the
/// pessimistic end: with no second measurement to say otherwise, a machine that
/// has just lost the network is exactly the machine that might have no other
/// discipline either. It puts the default hold at
/// `0.250 s / 50e-6 = 5000 s ≈ 83 minutes`.
const ASSUMED_PPM: f64 = 50.0;

/// Floor on the hold window. A wildly bad drift estimate must not collapse the
/// hold to nothing — that is B2's original bug arriving by a different road.
const HOLD_MIN: Duration = Duration::from_secs(300);

/// Ceiling on the hold window. §2.4's measured converged case (0.8 ppm) would
/// hold for over four days; a day is as far as anyone should steer a
/// transmitter on a measurement nothing has confirmed since.
const HOLD_MAX: Duration = Duration::from_secs(86_400);

/// A drift estimate outside this is not drift — it is a clock STEP that landed
/// between two probes, and sizing a hold window from it would be nonsense.
/// (The undisciplined regime tops out around 50 ppm; 200 leaves headroom for a
/// genuinely awful crystal without admitting a step.)
const MAX_PLAUSIBLE_PPM: f64 = 200.0;

/// Two probe rounds must be at least this far apart before their difference is
/// treated as a drift rate. Closer together, measurement noise (tens of ms
/// between two public servers) dominates the real drift and the "rate" is
/// mostly noise divided by a small number.
const MIN_DRIFT_INTERVAL: Duration = Duration::from_secs(1_800);

/// How far the wall clock may diverge from the monotonic clock across one
/// observation before it counts as a step rather than jitter, in ms.
///
/// Ordinary scheduling jitter between two reads in the same tick is
/// sub-millisecond; a slewed correction (W32Time "slows or speeds up the local
/// clock", it does not step) arrives far below this; the resume case steps by
/// minutes to hours. 250 ms is the same budget [`HOLD_BUDGET_MS`] spends, which
/// is the right scale: a divergence we would not tolerate as drift is not one to
/// keep steering through either.
const JUMP_THRESHOLD_MS: f64 = 250.0;

/// How long a measurement stays worth steering by, given what we know about how
/// fast this clock drifts.
///
/// `rate_ppm` is the machine's own measured drift when two probes far enough
/// apart have both landed ([`drift_ppm`]), and `None` otherwise. The window is
/// the time for [`HOLD_BUDGET_MS`] of error to accumulate at that rate, clamped
/// to [`HOLD_MIN`]..=[`HOLD_MAX`]. **Derived, not fixed**: the converged desktop
/// of §2.4 (0.84 ppm) earns the full day, while a machine measured drifting at
/// 100 ppm gets 42 minutes.
pub fn hold_window(rate_ppm: Option<f64>) -> Duration {
    let ppm = rate_ppm
        .map(f64::abs)
        .filter(|p| p.is_finite() && *p > 0.0)
        .unwrap_or(ASSUMED_PPM);
    let secs = HOLD_BUDGET_MS / 1000.0 / (ppm * 1e-6);
    if !secs.is_finite() {
        return HOLD_MAX;
    }
    Duration::from_secs_f64(secs.clamp(HOLD_MIN.as_secs_f64(), HOLD_MAX.as_secs_f64()))
}

/// The drift rate implied by two measurements, in ppm, or `None` when the pair
/// cannot support one.
///
/// Refused when the two probes are closer together than [`MIN_DRIFT_INTERVAL`]
/// (noise-dominated) or when the implied rate exceeds [`MAX_PLAUSIBLE_PPM`] (a
/// step, not drift — and a step must shorten nothing, because
/// [`ClockJumpDetector`] handles those by invalidating outright).
pub fn drift_ppm(older_ms: i64, newer_ms: i64, elapsed: Duration) -> Option<f64> {
    if elapsed < MIN_DRIFT_INTERVAL {
        return None;
    }
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return None;
    }
    let ppm = (newer_ms - older_ms) as f64 / 1000.0 / secs * 1e6;
    (ppm.is_finite() && ppm.abs() <= MAX_PLAUSIBLE_PPM).then_some(ppm)
}

/// A corroborated clock measurement, with everything needed to decide whether it
/// is still worth applying.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockOffset {
    /// Local−UTC in ms (positive = the PC clock is ahead of UTC).
    pub ms: i64,
    /// How many NTP servers agreed on it (`tempo_net::sntp::Measurement::agreeing`).
    pub servers: u8,
    /// When it was taken, on the MONOTONIC clock — the wall clock is the thing
    /// under measurement and cannot be used to age its own measurements.
    pub measured_at: Instant,
    /// How long it stays applicable, from [`hold_window`].
    pub hold_for: Duration,
}

impl ClockOffset {
    /// How long ago this was measured.
    pub fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.measured_at)
    }

    /// Is it still inside its hold window?
    pub fn is_fresh(&self, now: Instant) -> bool {
        self.age(now) < self.hold_for
    }
}

/// Hard ceiling on an offset the station will STEER by, in ms (guard 3).
///
/// Guard 1 has already corroborated anything that gets here, so an offset this
/// large is not a bad packet — it is a genuinely broken clock, and the right
/// answer is to refuse rather than to quietly fly it. A PC a full minute wrong
/// is wrong in ways slot steering does not reach: `tempo_core::logbook` stamps
/// QSOs from the RAW system clock, so a station steered onto the UTC grid while
/// its log records times a minute out puts bad data into other operators'
/// databases and into every upload connector. It also hides a machine that needs
/// an actual repair behind a correction that looks like health. Refuse, surface
/// it, and let the operator (or the Windows repair path) fix the clock itself.
///
/// ⚠️ This is a transmit-path guard. Do not raise it to make a test pass — set
/// the test's skew below it, as `slot.rs`'s hold-clock test does.
pub const MAX_STEER_MS: i64 = 60_000;

/// Everything the station knows about its own clock: the offset it is steering
/// by (if any), a measurement it refused to steer by, and whether something has
/// asked for a fresh probe.
#[derive(Debug, Default)]
pub struct ClockState {
    held: Option<ClockOffset>,
    /// The most recent measurement that broke [`MAX_STEER_MS`]. Kept so the UI
    /// can say *why* nothing is being applied — an unexplained absence of
    /// steering is the failure mode B3 exists to end.
    gross_ms: Option<i64>,
    /// Raised when a clock jump invalidated the held offset; drained by the
    /// probe thread, which then measures immediately instead of sleeping out
    /// its interval.
    reprobe: bool,
}

impl ClockState {
    /// Adopt a corroborated measurement taken at `now` (monotonic), sizing its
    /// hold window from `rate_ppm` when the caller has one.
    ///
    /// Guard 3 lives here: an offset beyond [`MAX_STEER_MS`] is recorded for
    /// display and **not** adopted — and it clears any previously held offset,
    /// because a clock now known to be a minute out must not keep being steered
    /// by yesterday's smaller number.
    pub fn publish(&mut self, offset_ms: i64, servers: u8, rate_ppm: Option<f64>, now: Instant) {
        if offset_ms.abs() > MAX_STEER_MS {
            self.gross_ms = Some(offset_ms);
            self.held = None;
            return;
        }
        self.gross_ms = None;
        self.held = Some(ClockOffset {
            ms: offset_ms,
            servers,
            measured_at: now,
            hold_for: hold_window(rate_ppm),
        });
    }

    /// Forget everything — the operator turned the clock check off, or the OS
    /// stepped the clock under us (in which case `reprobe` is raised too).
    pub fn clear(&mut self, reprobe: bool) {
        self.held = None;
        self.gross_ms = None;
        self.reprobe |= reprobe;
    }

    /// The offset to steer by right now, or `None` when there is none, it has
    /// expired, or it was refused by guard 3.
    pub fn offset_ms(&self, now: Instant) -> Option<i64> {
        self.held.filter(|o| o.is_fresh(now)).map(|o| o.ms)
    }

    /// The held measurement whether or not it is still fresh — the UI reports
    /// an expired one along with its age rather than showing nothing.
    pub fn held(&self) -> Option<ClockOffset> {
        self.held
    }

    /// The last measurement refused for exceeding [`MAX_STEER_MS`].
    pub fn gross_ms(&self) -> Option<i64> {
        self.gross_ms
    }

    /// Take the pending re-probe request, clearing it.
    pub fn take_reprobe(&mut self) -> bool {
        std::mem::take(&mut self.reprobe)
    }
}

/// Watches the wall clock against a monotonic one and reports OS clock steps.
///
/// One instance lives in the radio loop and is fed both clocks on every tick.
/// It is deliberately not self-reading: the tests step the wall clock past a
/// normally-advancing monotonic clock by passing the pair in.
#[derive(Debug, Default)]
pub struct ClockJumpDetector {
    last: Option<(f64, Instant)>,
}

impl ClockJumpDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// A detector whose PREVIOUS observation is already `(wall_ms, mono)`.
    ///
    /// For tests that need the next real observation to look like a step: seed
    /// the baseline an hour behind the real clock and the following
    /// `observe(now_unix_ms(), Instant::now())` reports the jump. It is not a
    /// bypass — it sets the prior sample, which is the only state this type has.
    pub fn seeded(wall_ms: f64, mono: Instant) -> Self {
        Self {
            last: Some((wall_ms, mono)),
        }
    }

    /// Feed one observation of both clocks. Returns `true` when the wall clock
    /// moved by more than [`JUMP_THRESHOLD_MS`] relative to the monotonic clock
    /// since the previous observation — an OS step (resume-from-sleep, a manual
    /// set, a time service stepping rather than slewing).
    ///
    /// The first observation always returns `false`: there is nothing to compare
    /// against, and a start-up "jump" would invalidate the offset the probe had
    /// only just published. After a jump the new pair becomes the baseline, so
    /// one step is reported once and not on every following tick.
    pub fn observe(&mut self, wall_ms: f64, mono: Instant) -> bool {
        let Some((last_wall, last_mono)) = self.last.replace((wall_ms, mono)) else {
            return false;
        };
        // `saturating_duration_since` because a monotonic clock never goes
        // backwards but a caller could hand us an older `Instant`; treating that
        // as zero elapsed makes any wall movement look like a jump, which is the
        // safe direction.
        let mono_ms = mono.saturating_duration_since(last_mono).as_secs_f64() * 1000.0;
        let wall_delta = wall_ms - last_wall;
        (wall_delta - mono_ms).abs() > JUMP_THRESHOLD_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── B2: the hold window is derived, not fixed ─────────────────────────────

    #[test]
    fn no_drift_estimate_holds_for_the_assumed_undisciplined_rate() {
        // 0.250 s of budget at 50 ppm = 5000 s ≈ 83 min.
        let w = hold_window(None);
        assert_eq!(w.as_secs(), 5_000, "83 minutes, from 250 ms at 50 ppm");
    }

    #[test]
    fn a_converged_machine_holds_far_longer_than_a_drifting_one() {
        // §2.4's measured converged desktop, 0.84 ppm → 297_619 s, clamped to a day.
        let converged = hold_window(Some(0.84));
        // A machine measured drifting 20× worse.
        let drifting = hold_window(Some(100.0));
        assert_eq!(converged, HOLD_MAX, "a converged clock earns the ceiling");
        assert_eq!(
            drifting.as_secs(),
            2_500,
            "100 ppm reaches 250 ms in 42 min"
        );
        assert!(
            converged > drifting,
            "the window must track the measurement, not a constant"
        );
    }

    #[test]
    fn the_hold_window_is_clamped_at_both_ends() {
        assert_eq!(
            hold_window(Some(1e9)),
            HOLD_MIN,
            "absurd drift floors, not zero"
        );
        assert_eq!(
            hold_window(Some(1e-9)),
            HOLD_MAX,
            "a perfect clock still expires"
        );
        assert_eq!(
            hold_window(Some(0.0)),
            hold_window(None),
            "0 ppm is no estimate"
        );
        assert_eq!(
            hold_window(Some(f64::NAN)),
            hold_window(None),
            "NaN is no estimate"
        );
    }

    #[test]
    fn a_drift_rate_needs_a_long_enough_baseline() {
        // 30 ms apart over 10 minutes would read as 50 ppm, but 10 minutes is
        // inside the noise floor, so no rate is claimed.
        assert_eq!(drift_ppm(0, 30, Duration::from_secs(600)), None);
        // The same 30 ms over an hour is a rate we will use.
        let ppm = drift_ppm(0, 30, Duration::from_secs(3_600)).expect("an hour is enough");
        assert!((ppm - 8.333).abs() < 0.01, "got {ppm}");
    }

    #[test]
    fn an_implausible_rate_is_a_step_and_is_refused() {
        // 3 hours of error appearing over 2 hours of uptime — the resume case.
        // It must NOT come back as a 1500 ppm "drift" and collapse the window.
        assert_eq!(
            drift_ppm(0, 11_176_113, Duration::from_secs(7_200)),
            None,
            "a step must not be laundered into a drift rate"
        );
        assert_eq!(
            hold_window(None).as_secs(),
            5_000,
            "so the default still applies"
        );
    }

    #[test]
    fn drift_is_signed_but_the_window_is_not() {
        let fast = drift_ppm(0, 60, Duration::from_secs(3_600)).unwrap();
        let slow = drift_ppm(0, -60, Duration::from_secs(3_600)).unwrap();
        assert!(
            fast > 0.0 && slow < 0.0,
            "sign is preserved: {fast} / {slow}"
        );
        assert_eq!(
            hold_window(Some(fast)),
            hold_window(Some(slow)),
            "a clock losing time expires no later than one gaining it"
        );
    }

    // ── B2: an offset expires rather than vanishing ───────────────────────────

    fn offset_aged(age: Duration, hold: Duration) -> ClockOffset {
        ClockOffset {
            ms: 420,
            servers: 3,
            measured_at: Instant::now().checked_sub(age).expect("test clock"),
            hold_for: hold,
        }
    }

    #[test]
    fn a_held_offset_stays_fresh_inside_its_window_and_expires_outside_it() {
        let hold = Duration::from_secs(5_000);
        let young = offset_aged(Duration::from_secs(4_000), hold);
        let old = offset_aged(Duration::from_secs(6_000), hold);
        // AFTER building them: each `offset_aged` reads the clock itself, so a
        // `now` captured first is fractionally EARLIER than their basis and the
        // ages come out a hair short of the ones asked for.
        let now = Instant::now();
        assert!(
            young.is_fresh(now),
            "4000 s into an 83 min hold still steers"
        );
        assert!(!old.is_fresh(now), "6000 s past it does not");
        // The age is reported either way — an expired offset is still evidence
        // for the operator, it is just no longer applied.
        assert!(old.age(now) >= Duration::from_secs(6_000));
    }

    // ── Guard 3: the hard ceiling ─────────────────────────────────────────────

    #[test]
    fn just_under_the_ceiling_steers_and_just_over_refuses() {
        let now = Instant::now();
        let mut s = ClockState::default();

        s.publish(MAX_STEER_MS, 3, None, now);
        assert_eq!(s.offset_ms(now), Some(60_000), "exactly 60 s still steers");
        assert_eq!(s.gross_ms(), None);

        s.publish(MAX_STEER_MS + 1, 3, None, now);
        assert_eq!(s.offset_ms(now), None, "60.001 s does not steer");
        assert_eq!(
            s.gross_ms(),
            Some(60_001),
            "and it is surfaced, not swallowed"
        );

        s.publish(-(MAX_STEER_MS + 1), 3, None, now);
        assert_eq!(s.offset_ms(now), None, "the ceiling is on magnitude");
        assert_eq!(s.gross_ms(), Some(-60_001));
    }

    #[test]
    fn a_gross_measurement_drops_the_offset_it_replaces() {
        // Yesterday's 400 ms is not the safe fallback for a clock now known to
        // be a minute out — it is a smaller wrong answer applied confidently.
        let now = Instant::now();
        let mut s = ClockState::default();
        s.publish(400, 3, None, now);
        assert_eq!(s.offset_ms(now), Some(400));
        s.publish(90_000, 3, None, now);
        assert_eq!(
            s.offset_ms(now),
            None,
            "the good offset must not survive it"
        );
    }

    // ── B2: the offset survives the network going away ────────────────────────

    #[test]
    fn a_held_offset_survives_a_failed_round_and_expires_on_schedule() {
        // THE B2 REGRESSION. Before 2026-09 a failed probe published `None` and
        // every reader fell through `unwrap_or(0)`, so walking out of coverage
        // silently un-steered the transmitter. Now a failed round publishes
        // nothing at all and the held value carries on until its window ends.
        let mut s = ClockState::default();
        let t0 = Instant::now();
        s.publish(400, 3, None, t0);

        // Six failed probe rounds (600 s each) — the loop simply does not call
        // `publish`, so an hour offline changes nothing.
        let an_hour_later = t0 + Duration::from_secs(3_600);
        assert_eq!(
            s.offset_ms(an_hour_later),
            Some(400),
            "an hour off-grid must not un-steer the transmitter"
        );

        // Past the 83-minute default window it stops being applied…
        let past_the_window = t0 + Duration::from_secs(5_001);
        assert_eq!(s.offset_ms(past_the_window), None, "but it does expire");
        // …and is still reportable, with its age, so the UI can say why.
        let held = s.held().expect("the measurement is kept for display");
        assert_eq!(held.ms, 400);
        assert!(held.age(past_the_window) > Duration::from_secs(5_000));
    }

    #[test]
    fn clearing_for_a_jump_asks_for_a_reprobe_and_clearing_for_a_setting_does_not() {
        let now = Instant::now();
        let mut s = ClockState::default();
        s.publish(400, 3, None, now);

        s.clear(false); // operator turned the clock check off
        assert_eq!(s.offset_ms(now), None);
        assert!(!s.take_reprobe(), "a settings change needs no urgent probe");

        s.publish(400, 3, None, now);
        s.clear(true); // the OS stepped the clock
        assert_eq!(
            s.offset_ms(now),
            None,
            "a stale offset must not outlive the step"
        );
        assert!(s.take_reprobe(), "and the probe thread is woken");
        assert!(!s.take_reprobe(), "the request is taken once");
    }

    // ── §8.2(a): the clock-jump detector ──────────────────────────────────────

    /// A wall clock and a monotonic clock advancing together in 100 ms ticks.
    /// `step_ms` is injected into the wall clock alone at the given tick.
    fn run_ticks(ticks: usize, step_at: Option<(usize, f64)>) -> Vec<bool> {
        let mut d = ClockJumpDetector::new();
        let base = Instant::now();
        let mut wall = 1_757_000_000_000.0_f64; // any plausible unix-ms
        let mut out = Vec::new();
        for i in 0..ticks {
            let mono = base + Duration::from_millis(100 * i as u64);
            wall += 100.0;
            if let Some((at, step)) = step_at {
                if i == at {
                    wall += step;
                }
            }
            out.push(d.observe(wall, mono));
        }
        out
    }

    #[test]
    fn normal_operation_never_invalidates() {
        let fired = run_ticks(50, None);
        assert!(
            !fired.iter().any(|f| *f),
            "two clocks advancing together must never look like a step"
        );
    }

    #[test]
    fn the_first_observation_is_never_a_jump() {
        let mut d = ClockJumpDetector::new();
        assert!(
            !d.observe(1_757_000_000_000.0, Instant::now()),
            "nothing to compare against yet"
        );
    }

    /// ⚠️ POSITIVE CONTROL for `normal_operation_never_invalidates`: the same
    /// harness, with the RTC-restore step §2.6 actually observed on Windows
    /// (3 h 06 m 16 s), must trip — and trip exactly once.
    #[test]
    fn a_resume_step_fires_once_on_the_tick_it_happens() {
        let fired = run_ticks(50, Some((20, 11_176_113.0)));
        let hits: Vec<usize> = fired
            .iter()
            .enumerate()
            .filter(|(_, f)| **f)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(hits, vec![20], "one step, one report, on its own tick");
    }

    #[test]
    fn a_backward_step_fires_too() {
        // The resume case can land either way — the kernel steps the system
        // clock TO the RTC, which may be behind.
        let fired = run_ticks(10, Some((5, -37_158.0)));
        assert_eq!(
            fired.iter().filter(|f| **f).count(),
            1,
            "37 s backwards is a step"
        );
    }

    #[test]
    fn a_slewed_correction_is_not_a_step() {
        // W32Time "does not step the time changes, but rather slows or speeds up
        // the local clock". 80 ms of correction spread over 40 ticks is 2 ms a
        // tick and must not invalidate a good offset.
        let mut d = ClockJumpDetector::new();
        let base = Instant::now();
        let mut wall = 1_757_000_000_000.0_f64;
        for i in 0..40 {
            let mono = base + Duration::from_millis(100 * i as u64);
            wall += 102.0; // 2 ms/tick of slew on top of 100 ms of real time
            assert!(!d.observe(wall, mono), "slew is not a step (tick {i})");
        }
    }

    #[test]
    fn a_step_just_under_the_threshold_does_not_fire() {
        let under = run_ticks(10, Some((5, 249.0)));
        let over = run_ticks(10, Some((5, 251.0)));
        assert!(!under.iter().any(|f| *f), "249 ms is jitter");
        assert!(over.iter().any(|f| *f), "251 ms is a step");
    }
}
