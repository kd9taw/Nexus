//! The AI CW decode thread's stitch: which characters of one inference pass are new, by
//! absolute stream time. Pure (std only) so it is unit-tested without the model or a device,
//! and so a replay bench can run the exact rule the app runs.

use std::time::Duration;

/// Don't emit characters from the last half second of a window: the model has no right
/// context there yet; the next (overlapping) window decodes that region reliably. Measured
/// on synthetic contest CW at 20–30 WPM, clean and at 0 dB: 0.5 s copies as cleanly as the
/// 1.0 s it replaced, 0.3 s starts to miss characters, and no guard at all prints garbage.
pub const TAIL_GUARD_SECS: f64 = 0.5;

/// The least rest between two passes, however fast the PC.
pub const MIN_PASS_GAP: Duration = Duration::from_millis(250);

/// The rest after a pass before the next one starts. Passes run back to back rather than on a
/// fixed cadence, so the copy trails the key-up by about the guard plus one pass. The rest is
/// half the pass, never under [`MIN_PASS_GAP`]: the decode thread then holds at most two
/// thirds of one core however slow the PC, and the radio and audio threads always get the CPU
/// back between passes. There is no queue to fall behind on: each pass decodes the newest
/// window, so a pass slower than the audio it covers only makes the next batch larger.
pub fn pass_gap(last_pass: Duration) -> Duration {
    MIN_PASS_GAP.max(last_pass / 2)
}

/// One pass's new text.
#[derive(Debug, Default, PartialEq)]
pub struct Pass {
    /// Characters newer than everything emitted before and older than the tail guard. Final:
    /// never re-emitted or revised by a later pass.
    pub confirmed: String,
    /// The characters inside the tail guard: PROVISIONAL, for display only. The next pass
    /// confirms them or replaces them.
    pub pending: String,
    /// `confirmed` and `pending` again, letter by letter, each with the absolute stream time
    /// the model placed it at: the CW turn cue reads silences and over boundaries from these
    /// (`tempo_core::cw_turn`).
    pub confirmed_at: Vec<(f64, char)>,
    pub pending_at: Vec<(f64, char)>,
}

/// The transcript cursor across passes.
#[derive(Debug, Default)]
pub struct Stitcher {
    /// ABSOLUTE stream seconds (fed samples / 12 kHz): characters at or before this moment
    /// are already emitted. Windows overlap by most of their length; this is what keeps the
    /// overlap from re-printing.
    emitted_until: f64,
    /// The engine's stream generation the cursor belongs to; `None` before the first pass.
    epoch: Option<u64>,
}

impl Stitcher {
    /// Forget the cursor (the feature went off; the engine's stream clock restarts).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Stitch one pass. `chars` are the model's (seconds into the window, text); the window
    /// ends at `end_abs` stream seconds and is `win_secs` long. A window longer than the
    /// stream so far is zero-padded at the front, so its start is before zero.
    pub fn pass(
        &mut self,
        epoch: u64,
        end_abs: f64,
        win_secs: f64,
        chars: &[(f32, String)],
    ) -> Pass {
        let start_abs = end_abs - win_secs;
        // A new stream (a QSY or a toggle restarted the engine's clock), or more time since
        // the last pass than the window holds: nothing before this window is ours. Nor is
        // anything before the stream's own start: that is the zero padding.
        if self.epoch != Some(epoch)
            || self.emitted_until < start_abs
            || self.emitted_until > end_abs
        {
            self.emitted_until = start_abs.max(0.0);
            self.epoch = Some(epoch);
        }
        let cutoff = end_abs - TAIL_GUARD_SECS;
        let mut out = Pass::default();
        for (t_rel, ch) in chars {
            let t_abs = start_abs + *t_rel as f64;
            if t_abs > self.emitted_until {
                let (text, timed) = if t_abs <= cutoff {
                    (&mut out.confirmed, &mut out.confirmed_at)
                } else {
                    (&mut out.pending, &mut out.pending_at)
                };
                text.push_str(ch);
                timed.extend(ch.chars().map(|c| (t_abs, c)));
            }
        }
        self.emitted_until = cutoff.max(self.emitted_until);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: f64 = 15.0;

    fn chars(v: &[(f32, &str)]) -> Vec<(f32, String)> {
        v.iter().map(|(t, c)| (*t, c.to_string())).collect()
    }

    /// The first pass after a QSY sees a window mostly made of zero padding. A character the
    /// model places in the padding cannot be a signal the new stream carried.
    #[test]
    fn a_partial_window_emits_only_the_streams_own_audio() {
        let mut s = Stitcher::default();
        // 3 s into a fresh stream: the window spans −12 s … 3 s.
        let p = s.pass(
            1,
            3.0,
            WIN,
            &chars(&[(11.5, "E"), (13.2, "K"), (14.6, "T")]),
        );
        // t_abs: −0.5 (padding), 1.2 (ours), 2.6 (inside the tail guard).
        assert_eq!(p.confirmed, "K");
    }

    /// Two QSYs a second or two apart: the cursor the old stream left sits INSIDE the new
    /// stream's time range. Without the stream generation the new stream's first characters
    /// are taken as already emitted and silently dropped.
    #[test]
    fn a_new_stream_never_inherits_the_old_streams_cursor() {
        let mut s = Stitcher::default();
        let _ = s.pass(1, 4.0, WIN, &chars(&[]));
        // The old stream's cursor is now 3.5 (4.0 − guard). A QSY restarts the clock; the
        // new stream's first pass ends at 5.0 and has characters at 1.5 and 2.5.
        let p = s.pass(2, 5.0, WIN, &chars(&[(11.5, "C"), (12.5, "Q")]));
        assert_eq!(p.confirmed, "CQ");
    }

    /// Back to back on a fast PC, and never more than two thirds of a core on a slow one.
    #[test]
    fn the_rest_between_passes_bounds_the_cpu_share() {
        assert_eq!(pass_gap(Duration::from_millis(300)), MIN_PASS_GAP);
        assert_eq!(pass_gap(Duration::from_secs(1)), Duration::from_millis(500));
        for ms in [50, 300, 500, 1000, 3000] {
            let pass = Duration::from_millis(ms);
            let share = pass.as_secs_f64() / (pass + pass_gap(pass)).as_secs_f64();
            assert!(
                share <= 2.0 / 3.0 + 1e-9,
                "{ms} ms pass holds {share:.2} of a core"
            );
        }
    }

    /// A character half a second clear of the window's end is copied on this pass; one
    /// inside the last half second waits for the next.
    #[test]
    fn the_tail_guard_holds_back_only_the_last_half_second() {
        let mut s = Stitcher::default();
        // Window ends at 20.0: K at 19.4 (0.6 s clear), T at 19.6 (0.4 s clear).
        let p = s.pass(1, 20.0, WIN, &chars(&[(14.4, "K"), (14.6, "T")]));
        assert_eq!(p.confirmed, "K");
    }

    /// The guarded tail is shown at once as provisional; the next pass confirms what it still
    /// hears there and may replace the rest.
    #[test]
    fn the_guarded_tail_is_provisional_until_a_later_pass_confirms_it() {
        let mut s = Stitcher::default();
        // Window ends at 20.0: C at 15.0 (confirmed), K at 19.7 and Q at 19.9 (inside the guard).
        let a = s.pass(
            1,
            20.0,
            WIN,
            &chars(&[(10.0, "C"), (14.7, "K"), (14.9, "Q")]),
        );
        assert_eq!((a.confirmed.as_str(), a.pending.as_str()), ("C", "KQ"));
        // 0.6 s later the model hears the K again, but the Q was really a G, and an E follows.
        let b = s.pass(
            1,
            20.6,
            WIN,
            &chars(&[(9.4, "C"), (14.1, "K"), (14.3, "G"), (15.0, "E")]),
        );
        assert_eq!(
            b.confirmed, "KG",
            "the next pass's own reading is what is confirmed"
        );
        assert_eq!(
            b.pending, "E",
            "the provisional tail is this pass's, the old one replaced"
        );
        // With more right context the E (20.6) reads as an I.
        let c = s.pass(
            1,
            21.2,
            WIN,
            &chars(&[(13.7, "G"), (14.4, "I"), (14.8, "T")]),
        );
        assert_eq!((c.confirmed.as_str(), c.pending.as_str()), ("I", "T"));
    }

    /// Once confirmed, a letter is final: a later pass that reads the same moment differently
    /// changes nothing already confirmed.
    #[test]
    fn a_confirmed_letter_is_never_reissued_or_revised() {
        let mut s = Stitcher::default();
        let a = s.pass(1, 20.0, WIN, &chars(&[(13.0, "K")]));
        assert_eq!(a.confirmed, "K");
        let b = s.pass(1, 20.6, WIN, &chars(&[(12.4, "R"), (14.0, "E")]));
        assert_eq!((b.confirmed.as_str(), b.pending.as_str()), ("E", ""));
    }

    /// The same stream across passes: the overlap is emitted once.
    #[test]
    fn the_overlap_of_two_passes_is_emitted_once() {
        let mut s = Stitcher::default();
        let a = s.pass(1, 20.0, WIN, &chars(&[(10.0, "C"), (13.0, "Q")]));
        // C at 15.0, Q at 18.0: both confirmed (cutoff 19.5).
        assert_eq!(a.confirmed, "CQ");
        // Next window ends at 22.0: the same C and Q again (shifted 2 s), plus D at 20.5.
        let b = s.pass(
            1,
            22.0,
            WIN,
            &chars(&[(8.0, "C"), (11.0, "Q"), (13.5, "D")]),
        );
        assert_eq!(b.confirmed, "D");
    }

    /// A pass slower than the audio it covers never builds a backlog: the next pass decodes
    /// the newest window, and a gap longer than a window skips ahead rather than queueing.
    #[test]
    fn a_long_gap_skips_ahead_instead_of_queueing() {
        let mut s = Stitcher::default();
        let _ = s.pass(1, 20.0, WIN, &chars(&[]));
        // 40 s later (the cursor, 19.5, is now before this window's start, 45.0).
        let p = s.pass(1, 60.0, WIN, &chars(&[(1.0, "A"), (12.0, "B")]));
        assert_eq!(
            p.confirmed, "AB",
            "everything in the newest window that is guard-clear"
        );
        let q = s.pass(
            1,
            61.0,
            WIN,
            &chars(&[(0.0, "A"), (11.0, "B"), (13.8, "C")]),
        );
        assert_eq!(q.confirmed, "C", "and the cursor follows the newest window");
    }

    /// Every letter a pass gives carries the stream time it was heard at, in the same order as
    /// the text, the provisional ones included: what the turn cue measures silences with.
    #[test]
    fn a_pass_gives_each_letter_its_stream_time() {
        let mut s = Stitcher::default();
        // The window spans 5 s … 20 s; the tail guard starts at 19.5 s.
        let p = s.pass(
            1,
            20.0,
            WIN,
            &chars(&[(1.0, "K"), (1.25, " "), (14.5, "N"), (14.8, "?")]),
        );
        assert_eq!((p.confirmed.as_str(), p.pending.as_str()), ("K N", "?"));
        assert_eq!(p.confirmed_at, vec![(6.0, 'K'), (6.25, ' '), (19.5, 'N')]);
        // The model's times are f32 seconds into the window.
        assert_eq!(p.pending_at, vec![(5.0 + f64::from(14.8f32), '?')]);
    }
}
