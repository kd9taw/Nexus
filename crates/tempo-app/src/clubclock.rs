//! What a club position may say about its clock against the host's.
//!
//! A club log orders club dupes and stamps every Cabrillo line by each position's OWN clock
//! ([`crate::fdevent`]), so a laptop a minute out writes a minute-wrong log and can keep the
//! wrong copy of a dupe. Until this module the position knew its clock only from the host's
//! welcome: whole seconds, once, with no allowance for the time on the wire. Now every timed
//! ping's round trip ([`ClockSample`], `tempo_net::fdsync`) is measured here and turned into
//! the one number the club line and the host's board show.
//!
//! - **The measurement** is the on-wire one NTP uses: the host's clock minus this PC's is
//!   `((t1 − t0) + (t2 − t3)) / 2`, and the time on the wire both ways is
//!   `(t3 − t0) − (t2 − t1)`. Each answer is right to within half its own delay, so the
//!   window keeps the last [`WINDOW`] round trips and believes the one with the least delay.
//! - **The clock was set.** Two answers whose error bounds cannot both hold mean the clock
//!   moved between them, so the window starts again from the newest: the operator who fixes
//!   a wrong clock sees the line follow on the next ping, not a minute later.
//! - **What is shown moves only for a real change.** It follows the measurement only when the
//!   two differ by more than [`DEADBAND_MS`]: WSL2 steps its wall clock by about 85–140 ms
//!   every 30 s, either sign, which would otherwise flip a whole-second line back and forth
//!   whenever the true difference sat near a rounding edge.
//!
//! ⛔ **Shown, never applied.** Nothing here reads, sets, steps or steers a clock (the module
//! is pure: every input is a sample), and nothing downstream may: FT's slot clock steers by
//! the SNTP measurement alone (`crate::clocksync`), and a contact's time is this PC's own
//! clock as it always was. Pinned by `the_club_clock_reaches_no_clock_slot_or_stamp_ft_reads`
//! (`tests/fieldday_loopback.rs`) and, for this file, by the scan in the tests below.

use std::collections::VecDeque;
use tempo_net::fdsync::ClockSample;

/// Round trips kept: the last minute at the pump's 5 s ping (`fdsync::PING_SECS`).
pub const WINDOW: usize = 12;

/// How far the measurement must move before what is shown moves, in ms. Above WSL2's
/// 85–140 ms wall-clock steps; far below the 2 s the club line starts at.
pub const DEADBAND_MS: i64 = 250;

/// How far two answers may disagree, beyond what their delays allow, before the clock is
/// taken to have been SET between them, in ms. A WSL2 step stays inside it; an operator
/// putting a clock right by a second or more does not.
pub const SET_MS: i64 = 250;

/// One round trip, measured: the host's clock minus this PC's, and the time it spent on
/// the wire both ways. Both in ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundTrip {
    pub offset_ms: i64,
    pub delay_ms: i64,
}

impl RoundTrip {
    /// `None` for a sample no real exchange produces: a negative time on the wire, or
    /// stamps too far apart to be one round trip (a hostile or broken host). In `i128`, so
    /// no stamp a peer can send overflows the arithmetic.
    pub fn of(s: ClockSample) -> Option<Self> {
        let (t0, t1, t2, t3) = (s.t0 as i128, s.t1 as i128, s.t2 as i128, s.t3 as i128);
        let offset = ((t1 - t0) + (t2 - t3)) / 2;
        let delay = (t3 - t0) - (t2 - t1);
        if delay < 0 {
            return None;
        }
        Some(Self {
            offset_ms: i64::try_from(offset).ok()?,
            delay_ms: i64::try_from(delay).ok()?,
        })
    }
}

/// The club clock of one session with one host. Reset at every welcome: a rejoin may be to
/// another host.
#[derive(Clone, Debug, Default)]
pub struct ClubClock {
    window: VecDeque<RoundTrip>,
    /// This PC's clock minus the host's, in ms, as shown (see [`DEADBAND_MS`]).
    shown_ms: Option<i64>,
}

impl ClubClock {
    /// Take one answered round trip.
    pub fn add(&mut self, s: ClockSample) {
        let Some(rt) = RoundTrip::of(s) else {
            return;
        };
        if let Some(best) = self.best() {
            // Each answer is within half its delay of the truth; two that cannot both be
            // are about two different clocks.
            let allowed = (rt.delay_ms.saturating_add(best.delay_ms) / 2).saturating_add(SET_MS);
            if rt.offset_ms.abs_diff(best.offset_ms) > allowed.unsigned_abs() {
                self.window.clear();
            }
        }
        if self.window.len() == WINDOW {
            self.window.pop_front();
        }
        self.window.push_back(rt);
        let measured = self.best().map(|b| -b.offset_ms);
        match (self.shown_ms, measured) {
            (Some(shown), Some(m)) if m.abs_diff(shown) <= DEADBAND_MS as u64 => {}
            _ => self.shown_ms = measured,
        }
    }

    /// The window's least-delayed round trip, the newest of equals.
    pub fn best(&self) -> Option<RoundTrip> {
        self.window.iter().rev().min_by_key(|r| r.delay_ms).copied()
    }

    /// This PC's clock minus the host's, in ms, as shown; `None` before the first answer.
    pub fn skew_ms(&self) -> Option<i64> {
        self.shown_ms
    }

    /// [`Self::skew_ms`] in whole seconds, rounded half away from zero (2.4 s → 2,
    /// −45.6 s → −46): the number the club line says.
    pub fn skew_secs(&self) -> Option<i64> {
        self.shown_ms.map(|ms| (ms + ms.signum() * 500) / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round trip from four stamps.
    fn rt(t0: u64, t1: u64, t2: u64, t3: u64) -> ClockSample {
        ClockSample { t0, t1, t2, t3 }
    }

    /// The round trip a host `ahead_ms` ahead of this PC answers, `up`/`down` ms each way,
    /// sent at this PC's `at`.
    fn trip(at: u64, ahead_ms: i64, up: u64, down: u64) -> ClockSample {
        let t1 = (at as i64 + up as i64 + ahead_ms) as u64;
        rt(at, t1, t1 + 1, at + up + 1 + down)
    }

    #[test]
    fn a_round_trip_is_corrected_for_its_time_on_the_wire() {
        assert_eq!(
            RoundTrip::of(rt(1000, 1530, 1531, 1061)),
            Some(RoundTrip {
                offset_ms: 500,
                delay_ms: 60
            })
        );
        let mut c = ClubClock::default();
        c.add(rt(1000, 1530, 1531, 1061));
        assert_eq!(
            c.skew_ms(),
            Some(-500),
            "the host is 500 ms ahead of this PC"
        );
    }

    /// A scripted ping/pong with a known skew and an ASYMMETRIC path: 40 ms up, 10 ms down,
    /// a host 3 s ahead. The answer is off by half the asymmetry, 15 ms, and that is inside
    /// half the delay: exactly what the method promises and no more.
    #[test]
    fn a_known_skew_over_an_asymmetric_path_is_measured_within_half_the_delay() {
        let s = trip(10_000, 3_000, 40, 10);
        assert_eq!(s, rt(10_000, 13_040, 13_041, 10_051));
        let r = RoundTrip::of(s).unwrap();
        assert_eq!(
            r,
            RoundTrip {
                offset_ms: 3_015,
                delay_ms: 50
            }
        );
        assert!((r.offset_ms - 3_000).abs() <= r.delay_ms / 2);
        let mut c = ClubClock::default();
        c.add(s);
        assert_eq!(c.skew_secs(), Some(-3), "this PC is 3 s behind the host");
    }

    #[test]
    fn the_least_delayed_round_trip_of_the_window_wins() {
        let mut c = ClubClock::default();
        // (offset, delay): (500, 60), (505, 58), (900, 400) — neither the first nor the
        // newest, the one that spent least time on the wire.
        c.add(rt(1000, 1530, 1531, 1061));
        c.add(rt(2000, 2534, 2535, 2059));
        c.add(rt(3000, 4100, 4101, 3401));
        assert_eq!(
            c.best(),
            Some(RoundTrip {
                offset_ms: 505,
                delay_ms: 58
            })
        );
        // The best moved 5 ms, well inside the deadband, so what is shown did not move.
        assert_eq!(c.skew_ms(), Some(-500));
    }

    #[test]
    fn the_window_is_the_last_twelve_round_trips() {
        let mut c = ClubClock::default();
        c.add(trip(1_000, 1_000, 1, 1)); // the best there will be: delay 2
        for i in 0..WINDOW as u64 - 1 {
            c.add(trip(10_000 + i * 5_000, 1_100, 3, 3)); // delay 6
        }
        assert_eq!(c.best().map(|b| b.delay_ms), Some(2), "still in the window");
        c.add(trip(100_000, 1_100, 3, 3));
        assert_eq!(
            c.best().map(|b| (b.offset_ms, b.delay_ms)),
            Some((1_100, 6)),
            "the thirteenth pushed it out"
        );
    }

    #[test]
    fn an_impossible_round_trip_is_no_measurement() {
        let mut c = ClubClock::default();
        // Back before it left: the host claims more time than the whole round trip took.
        c.add(rt(1000, 5000, 5100, 1050));
        // Stamps no round trip produces, from a broken or hostile host.
        c.add(rt(1000, u64::MAX, u64::MAX, 1050));
        assert_eq!(c.skew_ms(), None);
        // Control: a real one after them is taken.
        c.add(rt(1000, 1530, 1531, 1061));
        assert_eq!(c.skew_ms(), Some(-500));
    }

    #[test]
    fn whole_seconds_round_half_away_from_zero() {
        for (ms, secs) in [
            (2_400, 2),
            (-45_600, -46),
            (1_500, 2),
            (-1_500, -2),
            (1_499, 1),
            (0, 0),
            (-400, 0),
        ] {
            let mut c = ClubClock::default();
            c.add(trip(1_000_000, -ms, 2, 2));
            assert_eq!(c.skew_secs(), Some(secs), "{ms} ms");
        }
    }

    /// ⭐ **A WSL2 wall-clock step neither moves the line nor flaps it.** WSL2 steps its wall
    /// clock about every 30 s by 85–140 ms, and the sign flips from one evening to the next,
    /// so this PC's clock against the host's is a sawtooth: a ramp then a step back. Here
    /// the true difference sits ON a rounding edge (1.5 s, where 1 s turns into 2 s), for
    /// ten minutes, in both signs, with the delay wandering. The line says one number the
    /// whole time.
    #[test]
    fn a_wsl2_wall_clock_step_neither_moves_the_line_nor_flaps_it() {
        // A step of +120 ms inside the window moves the measurement by under 130 ms and
        // what is shown not at all.
        let mut c = ClubClock::default();
        for i in 0..6u64 {
            c.add(trip(1_000_000 + i * 5_000, -1_450, 2 + i % 3, 2));
        }
        let (before, shown) = (c.best().unwrap().offset_ms, c.skew_secs());
        for i in 6..12u64 {
            c.add(trip(1_000_000 + i * 5_000, -1_450 - 120, 1 + i % 2, 1));
        }
        let moved = (c.best().unwrap().offset_ms - before).abs();
        assert!(moved < 130, "the measurement moved {moved} ms");
        assert_eq!(c.skew_secs(), shown, "and the line did not move");
        assert!(c.skew_secs().unwrap().abs() <= 30, "no warning");

        for sign in [1i64, -1] {
            let mut c = ClubClock::default();
            let mut said = Vec::new();
            for i in 0..120u64 {
                let ramp = ((i % 6) as i64) * 24; // 0..120 ms over 30 s, then back
                let truth = sign * (1_500 + ramp - 60);
                c.add(trip(
                    1_000_000 + i * 5_000,
                    -truth,
                    1 + (i * 7) % 5,
                    1 + (i * 3) % 4,
                ));
                said.push(c.skew_secs().unwrap());
            }
            said.dedup();
            assert_eq!(said.len(), 1, "sign {sign}: the line said {said:?}");
        }
    }

    /// The operator puts a wrong clock right: the line follows on the next answer, not when
    /// the minute's window has aged out. Without the restart the window's least-delayed
    /// answer, about the old clock, would hold it for up to a minute.
    #[test]
    fn setting_the_clock_restarts_the_window() {
        let mut c = ClubClock::default();
        for i in 0..WINDOW as u64 {
            c.add(trip(1_000_000 + i * 5_000, 45_000, 1, 1)); // 45 s behind, delay 2
        }
        assert_eq!(c.skew_secs(), Some(-45));
        c.add(trip(2_000_000, 0, 4, 4)); // put right; a worse delay than any before it
        assert_eq!(c.skew_secs(), Some(0), "followed at once");
        // Control: a congested answer that its own delay explains is not a clock set.
        let mut c = ClubClock::default();
        c.add(trip(1_000_000, 5_000, 1, 1));
        c.add(trip(1_005_000, 5_000, 800, 40)); // 380 ms off, which its 840 ms delay explains
        assert_eq!(
            c.best().map(|b| b.delay_ms),
            Some(2),
            "the old answer still counts"
        );
    }

    /// The module reads no clock and writes none: every number in it comes from a sample.
    /// A scan of its code (above the tests) for the APIs that would read or set one, with a
    /// control that the scan sees a planted call.
    #[test]
    fn nothing_here_reads_or_sets_a_clock() {
        const FORBIDDEN: &[&str] = &[
            "SystemTime",
            "Instant",
            "UNIX_EPOCH",
            "clock_offset",
            "set_clock",
            "SetSystemTime",
            "SetLocalTime",
            "settimeofday",
            "clock_settime",
            "adjtime",
            "w32tm",
            "timedatectl",
        ];
        let calls = |src: &str| -> Vec<String> {
            let code = src.split("#[cfg(test)]").next().unwrap_or("");
            code.lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .filter(|l| FORBIDDEN.iter().any(|f| l.contains(f)))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(calls(include_str!("clubclock.rs")), Vec::<String>::new());
        let planted = "fn f() { let _ = std::time::SystemTime::now(); }\n#[cfg(test)]";
        assert_eq!(
            calls(planted).len(),
            1,
            "control: the scan sees a planted read"
        );
    }
}
