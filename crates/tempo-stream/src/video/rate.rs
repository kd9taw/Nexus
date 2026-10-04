//! How the picture is sent when the link to the page is what limits it: the station follows
//! str0m's estimate of that link (transport-wide congestion control, `Session::estimate`) instead
//! of sending a fixed bit rate the link may not carry. Before 2026-10-03 the internet path sent a
//! constant 4 Mbit/s whatever the link was; over a 1.5 Mbit/s link that lost most of its packets
//! and the page saw no picture at all.
//!
//! - **The bit rate follows the estimate at once, down and up** ([`Rate::plan`]): the video's share
//!   of the link ([`share`]), never above the picture's own budget (`Bound::kbps`, so never above
//!   the path's ceiling: 4 Mbit/s on the internet and a relay, 10 on the shack's network) and never
//!   below [`FLOOR_KBPS`]. A new bit rate retargets the running encoder, with no keyframe.
//! - **Receive audio first.** The share leaves [`AUDIO_KBPS`] for the `audio` channel and
//!   `control`, which str0m's estimate does not count (they are not RTP), and a tenth of the
//!   estimate for what it has not seen coming yet.
//! - **A weak link steps the frame rate down, and the size only for keyframes' sake.**
//!   - The frame rate: at most 30 a second while the share is at least half the picture's
//!     budget, 15 while it is a quarter, else 10 ([`PACES`]), counted as the pipeline counts it. A still window is not affected: it is sent
//!     twice a second whatever the cap.
//!   - The size: the largest of [`SCALES`] whose keyframe still crosses the link in about a
//!     second ([`KEY_PIXELS_PER_KBPS`]). A keyframe follows every loss the page reports and every
//!     new size, and one that takes seconds to cross leaves the page's picture stale for that long.
//!   - Measured on a Nexus-like cockpit with a busy waterfall (2026-10-03): at every bit rate
//!     down to 100 kbit/s the whole picture read better than a smaller one the browser enlarged,
//!     so the size is the last step, taken for the keyframes and not for the text.
//! - **The size waits for the link to be measured.** The first plans are made on str0m's starting
//!   guess ([`MEASURE`]); the frame rate follows it at once, the size only once str0m's first
//!   probes have measured the link.
//! - **Down at once, up only once the link has held**, with a quarter's margin: the size after
//!   [`RISE`], so a link that wavers does not cost a keyframe each way; the frame rate, which costs
//!   nothing to change, after [`PACE_RISE`], which is also how soon the frame rate recovers from
//!   the first frames' plan, made on the estimate's starting guess. Measured (2026-10-03), holding
//!   the size for a second before a step down left the bigger picture's keyframes on a link too
//!   weak for them, and the picture went stale more often, not less.
//! - **Nothing here can stop the picture.** Every plan has a size and a bit rate, however low the
//!   estimate, and its frame interval is at most 100 ms: the pipeline's still-window floor and the
//!   page's freshness rule are untouched, and a blurrier picture is how a weak link shows.
//! - **Without an estimate** (a page whose offer carries no transport-wide feedback) the picture
//!   goes as it always has: the bounded size at the path's fixed budget.
use std::time::{Duration, Instant};

use super::picture::{encoded_size, Bound, MIN_VIEW};
use super::pipeline::{frame_interval, FRAME_INTERVAL};

/// The lowest bit rate the picture is given, in kbit/s, however little the link carries: measured
/// (2026-10-03) the cockpit's text still reads at 150 kbit/s, soft but legible, at 2560×1070.
pub const FLOOR_KBPS: u32 = 150;
/// What the session sends besides the picture, in kbit/s: receive audio (24 kbit/s Opus in 60 ms
/// bundles, about 60 on the wire with its framing) and `control`'s replies.
pub const AUDIO_KBPS: u32 = 64;
/// A keyframe of the busy cockpit at the bottom of the quality range is about 0.2 bit a pixel
/// (measured 2026-10-03: 2560×1070 at 150 kbit/s, 0.18), so one crosses a link in about a second
/// when the picture has at most this many pixels per kbit/s of the video's share.
pub const KEY_PIXELS_PER_KBPS: u64 = 5000;
/// The sizes the picture steps through on a weak link, as fractions of each side.
pub const SCALES: [(u32, u32); 5] = [(1, 1), (3, 4), (1, 2), (3, 8), (1, 4)];
/// The shortest frame interval at each frame-rate step: at most 30, 15 and 10 a second.
pub const PACES: [Duration; 3] = [
    FRAME_INTERVAL,
    Duration::from_micros(66_667),
    Duration::from_millis(100),
];
/// How long a link must hold before the picture's size steps back up.
pub const RISE: Duration = Duration::from_secs(5);
/// How long a link must hold before the frame rate steps back up.
pub const PACE_RISE: Duration = Duration::from_secs(1);
/// How long the first plans on a link keep the picture's size: str0m's first probes go out at once
/// and answer within about a second.
pub const MEASURE: Duration = Duration::from_secs(2);

/// The video's share of a link that carries `estimate` kbit/s: nine tenths of it, less what receive
/// audio and `control` need.
pub fn share(estimate: u32) -> u32 {
    (u64::from(estimate) * 9 / 10).saturating_sub(u64::from(AUDIO_KBPS)) as u32
}

/// The link, in kbit/s, whose share is `kbps`: what to ask str0m's estimation to look for so the
/// whole picture can be sent at its own budget.
pub fn link_for(kbps: u32) -> u32 {
    ((u64::from(kbps) + u64::from(AUDIO_KBPS)) * 10).div_ceil(9) as u32
}

/// What one picture is sent at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub width: u32,
    pub height: u32,
    pub kbps: u32,
    /// The shortest time from this frame to the next.
    pub interval: Duration,
    /// The link the whole picture would need to be sent at its own budget ([`link_for`]).
    pub wanted: u32,
}

/// One step of the picture's size or frame rate, and since when the link has carried a better one.
#[derive(Clone, Copy, Debug, Default)]
struct Step {
    at: usize,
    better_since: Option<Instant>,
}

impl Step {
    /// Down to `fit` at once; up to `fit_up` (the step the link carries with the margin) only once
    /// it has carried a better step than this one for `rise`.
    fn follow(&mut self, fit: usize, fit_up: usize, rise: Duration, now: Instant) -> usize {
        if fit > self.at {
            self.at = fit;
            self.better_since = None;
        } else if fit_up < self.at {
            let since = *self.better_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= rise {
                self.at = fit_up;
                self.better_since = None;
            }
        } else {
            self.better_since = None;
        }
        self.at
    }
}

/// The picture's rate control: what each frame is sent at, from the bound and the link.
#[derive(Clone, Debug, Default)]
pub struct Rate {
    scale: Step,
    pace: Step,
    /// When this picture was first planned on a link.
    since: Option<Instant>,
}

impl Rate {
    /// The plan for a `window` of that size within `bound`, at `now`: `None` only where
    /// `encoded_size` has none (a window too small to encode).
    pub fn plan(&mut self, window: (u32, u32), bound: &Bound, now: Instant) -> Option<Plan> {
        let full = encoded_size(window.0, window.1, bound)?;
        let wanted = link_for(bound.kbps(full.0, full.1));
        let Some(link) = bound.link else {
            *self = Self::default();
            return Some(Plan {
                width: full.0,
                height: full.1,
                kbps: bound.kbps(full.0, full.1),
                interval: frame_interval(full.0, full.1),
                wanted,
            });
        };
        let share = share(link);
        // The margin a step back up needs: the link must carry it with a quarter to spare.
        let spare = share * 4 / 5;
        let since = *self.since.get_or_insert(now);
        let (fit, fit_up) = if now.saturating_duration_since(since) < MEASURE {
            (0, 0)
        } else {
            (scale_for(share, full), scale_for(spare, full))
        };
        let step = self.scale.follow(fit, fit_up, RISE, now);
        let (width, height) = scaled(full, step);
        let budget = bound.kbps(width, height);
        let pace = self.pace.follow(
            pace_for(share, budget),
            pace_for(spare, budget),
            PACE_RISE,
            now,
        );
        Some(Plan {
            width,
            height,
            kbps: share.clamp(FLOOR_KBPS, budget),
            interval: frame_interval(width, height).max(PACES[pace]),
            wanted,
        })
    }
}

/// `full` at the size step `step`, cut to even.
fn scaled(full: (u32, u32), step: usize) -> (u32, u32) {
    let (n, d) = SCALES[step];
    ((full.0 * n / d) & !1, (full.1 * n / d) & !1)
}

/// The size step a video share carries: the first whose pixels a keyframe crosses in about a
/// second, else the smallest that keeps the picture's shorter side at [`MIN_VIEW`] (or the whole
/// picture's, if that is shorter).
fn scale_for(share: u32, full: (u32, u32)) -> usize {
    let short = full.0.min(full.1).min(MIN_VIEW);
    let last = (0..SCALES.len())
        .rev()
        .find(|&s| {
            let (w, h) = scaled(full, s);
            w.min(h) >= short
        })
        .unwrap_or(0);
    let room = u64::from(share) * KEY_PIXELS_PER_KBPS;
    (0..=last)
        .find(|&s| {
            let (w, h) = scaled(full, s);
            u64::from(w) * u64::from(h) <= room
        })
        .unwrap_or(last)
}

/// The frame-rate step a video share carries for a picture whose own budget is `budget`.
fn pace_for(share: u32, budget: u32) -> usize {
    let (share, budget) = (u64::from(share), u64::from(budget));
    if share * 2 >= budget {
        0
    } else if share * 4 >= budget {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::STILL_FRAME_MS;
    use crate::video::picture::{Path, INTERNET_KBPS, LAN_KBPS};

    /// The operator's wide window.
    const WINDOW: (u32, u32) = (3440, 1440);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A 3440-wide page on the internet: the picture is 3128×1308, and its budget the path's
    /// ceiling, 4 Mbit/s.
    fn far(estimate: Option<u32>) -> Bound {
        Bound::new(Some(Path::Internet), Some((3440, 1440))).within(estimate)
    }

    fn sized(plan: &Plan) -> (u32, u32) {
        (plan.width, plan.height)
    }

    /// No estimate (a page whose offer carries no transport-wide feedback): the plan is exactly
    /// what the picture was sent at before, the bounded size at the path's fixed budget.
    #[test]
    fn without_an_estimate_the_picture_goes_as_before() {
        let mut rate = Rate::default();
        let plan = rate.plan(WINDOW, &far(None), Instant::now()).unwrap();
        assert_eq!(sized(&plan), (3128, 1308));
        assert_eq!(plan.kbps, INTERNET_KBPS);
        assert_eq!(plan.interval, frame_interval(3128, 1308));
    }

    /// ★ The response to an estimate step (2026-10-03): the link falls from wide to 1.5 Mbit/s.
    /// The very next plan is at the video's share of the new link, which leaves receive audio its
    /// room, and is a quarter of the picture's budget or more, so 15 frames a second at full size.
    /// CONTROL: on the wide link the bit rate was the path's ceiling, and never above it.
    #[test]
    fn a_falling_estimate_lowers_the_bit_rate_at_once() {
        let mut rate = Rate::default();
        let t = Instant::now();
        let wide = rate.plan(WINDOW, &far(Some(8000)), t).unwrap();
        assert_eq!(wide.kbps, INTERNET_KBPS);
        assert_eq!(wide.interval, frame_interval(3128, 1308));
        let narrow = rate.plan(WINDOW, &far(Some(1500)), t + ms(10)).unwrap();
        assert_eq!(narrow.kbps, 1286);
        assert_eq!(narrow.kbps, share(1500));
        assert!(narrow.kbps + AUDIO_KBPS < 1500);
        assert_eq!(sized(&narrow), (3128, 1308));
        assert_eq!(narrow.interval, PACES[1]);
    }

    /// Weaker links, once measured: the size steps down only as far as a keyframe needs to cross
    /// in about a second, the frame rate by the share of the stepped picture's budget, and however
    /// low the estimate the picture keeps a size, the floor's bit rate and ten frames a second.
    #[test]
    fn a_weak_link_steps_the_frame_rate_and_then_the_size() {
        let t = Instant::now();
        let cases = [
            // estimate, size, kbps, pace
            (900, (2346, 980), 746, 1),
            // Here 15 a second would fit the stepped picture, but not with the margin a step back
            // up needs, so the frame rate stays where the first plans put it.
            (400, (1564, 654), 296, 2),
            (250, (1172, 490), 161, 2),
            (100, (782, 326), FLOOR_KBPS, 2),
            (0, (782, 326), FLOOR_KBPS, 2),
        ];
        for (estimate, size, kbps, pace) in cases {
            let mut rate = Rate::default();
            // The first plans keep the size while the link is measured; then it steps, and the
            // frame rate settles on the stepped picture's budget.
            for at in [t, t + MEASURE] {
                rate.plan(WINDOW, &far(Some(estimate)), at).unwrap();
            }
            let plan = rate
                .plan(WINDOW, &far(Some(estimate)), t + MEASURE + PACE_RISE)
                .unwrap();
            assert_eq!(sized(&plan), size, "{estimate}");
            assert_eq!(plan.kbps, kbps, "{estimate}");
            assert_eq!(plan.interval, PACES[pace], "{estimate}");
            assert!(plan.interval * 4 < Duration::from_millis(STILL_FRAME_MS));
        }
    }

    /// The link widens again: the bit rate rises at once, to the stepped picture's own budget,
    /// and the size and frame rate come back only once the link has held for [`RISE`], in one
    /// step, with one new size (one keyframe).
    #[test]
    fn a_recovered_link_brings_the_picture_back_once_it_has_held() {
        let mut rate = Rate::default();
        let t = Instant::now() - MEASURE;
        rate.plan(WINDOW, &far(Some(400)), t).unwrap();
        let t = t + MEASURE;
        let weak = rate.plan(WINDOW, &far(Some(400)), t).unwrap();
        assert_eq!(sized(&weak), (1564, 654));
        let wide = rate.plan(WINDOW, &far(Some(8000)), t + ms(10)).unwrap();
        assert_eq!(
            sized(&wide),
            (1564, 654),
            "the size waits for the link to hold"
        );
        assert_eq!(
            wide.kbps,
            far(None).kbps(1564, 654),
            "the bit rate does not"
        );
        let held = rate
            .plan(WINDOW, &far(Some(8000)), t + ms(10) + RISE - ms(1))
            .unwrap();
        assert_eq!(sized(&held), (1564, 654));
        let back = rate
            .plan(WINDOW, &far(Some(8000)), t + ms(10) + RISE)
            .unwrap();
        assert_eq!(sized(&back), (3128, 1308));
        assert_eq!(back.kbps, INTERNET_KBPS);
        assert_eq!(back.interval, frame_interval(3128, 1308));
    }

    /// The frame rate, which costs nothing to change, comes back sooner than the size: the first
    /// frames are planned on the estimate's starting guess, and the frame rate it lowered recovers
    /// a second after the link is measured, where the size would wait [`RISE`].
    #[test]
    fn the_frame_rate_recovers_sooner_than_the_size() {
        let mut rate = Rate::default();
        let t = Instant::now();
        let start = rate.plan(WINDOW, &far(Some(1000)), t).unwrap();
        assert_eq!((sized(&start), start.interval), ((3128, 1308), PACES[2]));
        let measured = rate.plan(WINDOW, &far(Some(6000)), t + ms(500)).unwrap();
        assert_eq!(measured.interval, PACES[2]);
        let soon = rate.plan(WINDOW, &far(Some(6000)), t + ms(1500)).unwrap();
        assert_eq!(soon.interval, frame_interval(3128, 1308));
    }

    /// The first plans are made on str0m's starting guess, not a measurement (2026-10-03: a page
    /// on the shack's network was planned 174 ms in, before the first probes answered, and its
    /// whole 3440×1440 window stepped down for five seconds and a keyframe each way). So for
    /// [`MEASURE`] the size stays whole while the link is measured; the frame rate, which costs
    /// nothing, follows at once. CONTROL: a guess that still stands after that steps it down.
    #[test]
    fn the_size_waits_for_the_link_to_be_measured() {
        let lan = Bound::new(Some(Path::Lan), None).within(Some(1000));
        let mut rate = Rate::default();
        let t = Instant::now();
        let first = rate.plan(WINDOW, &lan, t).unwrap();
        assert_eq!(sized(&first), (3440, 1440));
        assert_eq!(first.interval, PACES[2]);
        let measuring = rate.plan(WINDOW, &lan, t + MEASURE - ms(1)).unwrap();
        assert_eq!(sized(&measuring), (3440, 1440));
        let measured = rate.plan(WINDOW, &lan, t + MEASURE).unwrap();
        assert_eq!(sized(&measured), (2580, 1080));
    }

    /// An estimate that wavers around a step, as str0m's does (it backs off by 15 % on every
    /// sign of a queue and climbs again), steps the picture down once, when the link has been
    /// measured, and leaves it there: the way back needs a quarter's margin as well as time.
    #[test]
    fn a_wavering_link_does_not_flap_the_size() {
        let mut rate = Rate::default();
        let t = Instant::now();
        let mut sizes = Vec::new();
        for n in 0..400u64 {
            let estimate = if n % 2 == 0 { 900 } else { 1100 };
            let plan = rate
                .plan(WINDOW, &far(Some(estimate)), t + ms(100 * n))
                .unwrap();
            if sizes.last() != Some(&sized(&plan)) {
                sizes.push(sized(&plan));
            }
        }
        assert_eq!(sizes, vec![(3128, 1308), (2346, 980)]);
    }

    /// On the shack's own network the picture follows the estimate too, within that path's own
    /// ceiling: a wide link gives the whole window its full budget, as the fixed budget did.
    #[test]
    fn the_shacks_network_follows_the_estimate_within_its_own_ceiling() {
        let lan = |estimate| Bound::new(Some(Path::Lan), None).within(estimate);
        let mut rate = Rate::default();
        let t = Instant::now();
        let wide = rate.plan(WINDOW, &lan(Some(20_000)), t).unwrap();
        assert_eq!((sized(&wide), wide.kbps), ((3440, 1440), 4953));
        let wifi = rate.plan(WINDOW, &lan(Some(3000)), t + ms(10)).unwrap();
        assert_eq!((sized(&wifi), wifi.kbps), ((3440, 1440), share(3000)));
        let mut rate = Rate::default();
        let uhd = rate.plan((3840, 2160), &lan(Some(50_000)), t).unwrap();
        assert_eq!(uhd.kbps, 8294);
        assert!(uhd.kbps <= LAN_KBPS);
    }

    /// What str0m's estimation is asked to look for: the link at which the whole picture gets its
    /// own budget, and no more.
    #[test]
    fn the_wanted_link_carries_the_whole_picture() {
        let mut rate = Rate::default();
        let plan = rate.plan(WINDOW, &far(Some(400)), Instant::now()).unwrap();
        assert_eq!(plan.wanted, link_for(INTERNET_KBPS));
        assert!(share(plan.wanted) >= INTERNET_KBPS);
        assert!(share(plan.wanted - 2) < INTERNET_KBPS);
    }
}
