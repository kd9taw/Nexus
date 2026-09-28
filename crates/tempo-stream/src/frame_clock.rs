//! How old is the picture the page last showed? (S9: blind means no authority.)
//!
//! The station stamps every video frame it sends with an RTP timestamp from its own 90 kHz video
//! clock, which starts at zero when the session's video starts. A page's heartbeat echoes the
//! timestamp of the last frame it showed (`decodedFrameAt`), and this maps it back to the station
//! instant that frame was captured. No wall clock crosses the boundary, so a browser whose clock is
//! minutes off cannot make a stale picture read as fresh.
//!
//! ⛔ AN ECHO MUST NAME A FRAME THE STATION SENT. The clock remembers the timestamps of the frames
//! it recently sent, and a `decodedFrameAt` that matches none of them is not fresh, whatever its
//! value. So a page cannot keep transmit presence alive by sending an estimate of "now" instead of
//! the frame it actually showed, and a garbled or replayed value reads as stale, never as fresh.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::protocol::{FRESH_FRAME_MS, VIDEO_CLOCK_HZ};

/// How many sent frames are remembered: over eight seconds at 30 frames a second, and minutes at
/// the still-window floor, against a freshness window of two seconds.
const REMEMBERED: usize = 256;

/// One session's video clock.
pub struct VideoClock {
    start: Instant,
    sent: VecDeque<(u32, Instant)>,
}

impl VideoClock {
    pub fn new(start: Instant) -> Self {
        Self {
            start,
            sent: VecDeque::with_capacity(REMEMBERED),
        }
    }

    /// The media time, in 90 kHz ticks since the clock started, of a frame captured at `at`. A
    /// capture instant before the start reads as the start.
    pub fn ticks(&self, at: Instant) -> u64 {
        let elapsed = at.saturating_duration_since(self.start);
        (elapsed.as_nanos() * u128::from(VIDEO_CLOCK_HZ) / 1_000_000_000) as u64
    }

    /// Record a frame captured at `at` as sent, and return its media time. The RTP timestamp the
    /// page will see is this, truncated to 32 bits: the transport writes it unchanged.
    pub fn sent(&mut self, at: Instant) -> u64 {
        let ticks = self.ticks(at);
        if self.sent.len() >= REMEMBERED {
            self.sent.pop_front();
        }
        self.sent.push_back((ticks as u32, at));
        ticks
    }

    /// When the frame a page echoed was captured, if the station sent it and remembers it. The
    /// newest match wins, which only matters thirteen hours in, when the 32-bit timestamp wraps.
    pub fn captured_at(&self, rtp: u32) -> Option<Instant> {
        self.sent
            .iter()
            .rev()
            .find(|(sent, _)| *sent == rtp)
            .map(|(_, at)| *at)
    }

    /// S9: is the picture the page last showed fresh enough to renew transmit presence at `now`?
    /// `None` (nothing shown yet) is not.
    pub fn fresh(&self, decoded: Option<u32>, now: Instant) -> bool {
        decoded
            .and_then(|rtp| self.captured_at(rtp))
            .is_some_and(|at| {
                now.saturating_duration_since(at) <= Duration::from_millis(FRESH_FRAME_MS)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_frame_is_stamped_on_the_90_khz_clock() {
        let t0 = Instant::now();
        let mut clock = VideoClock::new(t0);
        assert_eq!(clock.sent(t0), 0);
        assert_eq!(clock.sent(t0 + ms(1000)), 90_000);
        assert_eq!(clock.sent(t0 + ms(33)), 2970);
    }

    /// ★ A7's clock half: an echo 0.5 s old is fresh (the control); one 2.5 s old is not.
    #[test]
    fn a_half_second_old_picture_is_fresh_and_a_two_and_a_half_second_old_one_is_not() {
        let t0 = Instant::now();
        let mut clock = VideoClock::new(t0);
        let frame = clock.sent(t0 + ms(100)) as u32;
        assert!(
            clock.fresh(Some(frame), t0 + ms(600)),
            "0.5 s old read as stale"
        );
        assert!(
            clock.fresh(Some(frame), t0 + ms(2100)),
            "exactly 2 s is still fresh"
        );
        assert!(
            !clock.fresh(Some(frame), t0 + ms(2101)),
            "just past 2 s read as fresh"
        );
        assert!(
            !clock.fresh(Some(frame), t0 + ms(2600)),
            "2.5 s old read as fresh"
        );
    }

    #[test]
    fn nothing_shown_is_not_fresh() {
        let t0 = Instant::now();
        let mut clock = VideoClock::new(t0);
        clock.sent(t0);
        assert!(!clock.fresh(None, t0));
    }

    /// ⛔ An echo that names no frame the station sent is not fresh, however plausible its value.
    #[test]
    fn an_estimate_of_now_is_not_a_frame() {
        let t0 = Instant::now();
        let mut clock = VideoClock::new(t0);
        let frame = clock.sent(t0) as u32;
        let now = t0 + ms(5000);
        // A page that stopped showing frames but "estimates" the current timestamp.
        let estimate = clock.ticks(now) as u32;
        assert!(
            !clock.fresh(Some(estimate), now),
            "an estimate kept presence alive"
        );
        // CONTROL: the frame it really showed is known, and simply old.
        assert_eq!(clock.captured_at(frame), Some(t0));
        assert!(!clock.fresh(Some(frame), now));
        // …and a fresh frame sent now, echoed, is fresh.
        let live = clock.sent(now) as u32;
        assert!(clock.fresh(Some(live), now + ms(300)));
    }

    #[test]
    fn only_the_recent_frames_are_remembered() {
        let t0 = Instant::now();
        let mut clock = VideoClock::new(t0);
        let first = clock.sent(t0) as u32;
        for i in 1..=REMEMBERED as u64 {
            clock.sent(t0 + ms(i * 33));
        }
        assert_eq!(
            clock.captured_at(first),
            None,
            "a frame out of the window was remembered"
        );
    }

    /// Thirteen hours in, the 32-bit timestamp wraps; the newest frame with that value wins.
    #[test]
    fn the_newest_frame_wins_across_a_wrap() {
        let t0 = Instant::now();
        let mut clock = VideoClock::new(t0);
        // The wrap period, rounded UP to a whole nanosecond so the tick count lands exactly on it.
        let hz = u128::from(VIDEO_CLOCK_HZ);
        let wrap = Duration::from_nanos((((1u128 << 32) * 1_000_000_000).div_ceil(hz)) as u64);
        let early = t0 + ms(10);
        let a = clock.sent(early) as u32;
        let later = early + wrap;
        let b = clock.sent(later) as u32;
        assert_eq!(a, b, "premise: the two frames share a timestamp");
        assert_eq!(clock.captured_at(a), Some(later));
        assert!(clock.fresh(Some(a), later + ms(100)));
    }
}
