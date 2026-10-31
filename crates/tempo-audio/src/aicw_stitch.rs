//! The AI CW decode thread's stitch: which characters of one inference pass are new, by
//! absolute stream time — and its [`Gate`]: whether a pass is worth running at all. Pure
//! (std only) so it is unit-tested without the model or a device, and so a replay bench can
//! run the exact rule the app runs.

use std::collections::VecDeque;
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

/// The AI-CW stream rate (the engine's ring, `fed` and the window are all at 12 kHz).
const RATE: u64 = 12_000;
/// The window a pass decodes (15 s, the model's).
const WINDOW: u64 = 15 * RATE;
/// Less new audio than one model hop (15 ms) since the last pass: the window is the one already
/// decoded, so a pass would give exactly what the last one gave.
const MIN_NEW: u64 = RATE * 15 / 1000;
/// The CW detector's frame: 80 ms, so its bins are 12.5 Hz apart, the model's own spacing.
const FRAME: usize = 960;
/// The detector's bins: 300–1300 Hz. The model hears 400–1200 Hz; the extra 100 Hz each side
/// gives the band's edge bins neighbours to be measured against.
const FIRST_BIN: usize = 24;
const BINS: usize = 81;
/// How far either side of a bin its noise reference reaches (±25 to ±100 Hz): clear of the
/// tone's own spread (±1 bin), close enough that a receive filter's slope is nearly flat.
const NEAR: usize = 2;
const FAR: usize = 8;
/// How much audio the detector averages (frames): 2 s. Long enough that keying 10 dB under
/// the weakest the model copies stands well clear of the noise; short enough to open within
/// a second or two of a station starting.
const LISTEN_FRAMES: usize = 25;
/// A bin this far above the noise beside it (power, ×) is CW, or another carrier. Measured on
/// synthetic 20 WPM CW in white noise (SNR in 2500 Hz), the lowest 2 s reading over the keying:
/// −9 dB reads 3.5 or more, −12 dB 2.2 or more; noise alone peaked at 2.1 over six minutes. The
/// model copies 73% of the letters at −6 dB and almost none at −9 dB.
const CW_RATIO: f32 = 2.5;

/// What the decode thread does with one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// No new audio since the last pass: nothing to do.
    Wait,
    /// New audio, but no CW anywhere in the window: a pass would copy nothing, so the model
    /// does not run. The decoder has still listened, up to `heard_until` (stream samples): the
    /// detector's average (2 s) short of the window's end, how far it is sure of, and never
    /// behind a pass already decoded. The stitch cursor stays where the last decoded pass left
    /// it, so a station the detector hears late is still copied from its start.
    Quiet { heard_until: u64 },
    /// Run the model.
    Decode,
}

/// Whether a pass is worth running. An inference pass costs a large part of a core; on a quiet
/// band, or with no audio arriving at all, every one gave nothing new. The model only runs
/// while its window holds audio in which a narrow-band detector heard CW (or another carrier),
/// and never twice on the same window.
///
/// Conservative by construction: the detector hears keying well below the weakest the model
/// copies, every sample of the stream is examined once, and a window that held CW anywhere in
/// its 15 s is decoded exactly as before, so the copy of a real signal is unchanged. A false
/// alarm (a carrier, a narrow receive filter's hump) only costs the CPU the decoder always used.
#[derive(Debug, Default)]
pub struct Gate {
    /// The stream generation this state belongs to.
    epoch: Option<u64>,
    /// Stream position (samples) up to which audio has been framed.
    seen: u64,
    /// The start of a frame still filling.
    carry: Vec<f32>,
    /// The band power of the newest frames, oldest first.
    frames: VecDeque<[f32; BINS]>,
    /// The stream position of the newest frame the detector heard CW in.
    heard_at: Option<u64>,
    /// `fed` at the last pass, decoded or quiet.
    passed_at: Option<u64>,
    /// `fed` at the last decoded pass.
    decoded_at: u64,
}

impl Gate {
    /// Forget everything (the feature went off; the engine's stream clock restarts).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// This turn's decision for the window whose last sample is at stream position `fed` (as
    /// the engine's `ai_cw_window` gives them).
    pub fn turn(&mut self, epoch: u64, window: &[f32], fed: u64) -> Turn {
        if self.epoch != Some(epoch) || fed < self.seen {
            *self = Self {
                epoch: Some(epoch),
                ..Self::default()
            };
        }
        if self.passed_at.is_some_and(|p| fed - p < MIN_NEW) {
            return Turn::Wait;
        }
        // Frame every sample once. Audio older than the window is gone and can never be in a
        // pass again.
        let new = ((fed - self.seen) as usize).min(window.len());
        self.listen(&window[window.len() - new..], fed);
        self.seen = fed;
        self.passed_at = Some(fed);
        if self.heard_at.is_some_and(|h| fed - h < WINDOW) {
            self.decoded_at = fed;
            Turn::Decode
        } else {
            let lag = (LISTEN_FRAMES * FRAME) as u64;
            Turn::Quiet {
                heard_until: fed.saturating_sub(lag).max(self.decoded_at),
            }
        }
    }

    /// Frame `audio` (its last sample at stream position `end`) and note where CW was heard.
    fn listen(&mut self, audio: &[f32], end: u64) {
        let mut at = end - audio.len() as u64;
        for &s in audio {
            self.carry.push(s);
            at += 1;
            if self.carry.len() == FRAME {
                if self.frames.len() == LISTEN_FRAMES {
                    self.frames.pop_front();
                }
                self.frames.push_back(band_power(&self.carry));
                self.carry.clear();
                // A full two seconds before it judges: a shorter average is noise-prone, and a
                // late opening loses nothing (the first pass reads the whole window).
                if self.frames.len() == LISTEN_FRAMES && cw_heard(&self.frames) {
                    self.heard_at = Some(at);
                }
            }
        }
    }
}

/// One Hann-windowed frame's power in each detector bin (Goertzel).
fn band_power(frame: &[f32]) -> [f32; BINS] {
    let step = 2.0 * std::f32::consts::PI / FRAME as f32;
    let windowed: Vec<f32> = frame
        .iter()
        .enumerate()
        .map(|(i, &x)| x * (0.5 - 0.5 * (step * i as f32).cos()))
        .collect();
    let mut out = [0.0; BINS];
    for (b, p) in out.iter_mut().enumerate() {
        let coeff = 2.0 * (step * (FIRST_BIN + b) as f32).cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in &windowed {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        *p = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    }
    out
}

/// Whether any bin of the model's band, averaged over `frames`, stands [`CW_RATIO`] above the
/// noise beside it. The reference is the louder side's median, so the edge of a receive filter
/// (passband on one side, stopband on the other) does not read as a signal.
fn cw_heard(frames: &VecDeque<[f32; BINS]>) -> bool {
    peak_ratio(frames) >= CW_RATIO
}

/// The largest bin-to-neighbours power ratio in the model's band (0 on digital silence).
fn peak_ratio(frames: &VecDeque<[f32; BINS]>) -> f32 {
    let mut sum = [0.0f32; BINS];
    for f in frames {
        for (s, p) in sum.iter_mut().zip(f) {
            *s += p;
        }
    }
    let median = |side: &[f32]| {
        let mut v = side.to_vec();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    (FAR..BINS - FAR)
        .map(|b| {
            let noise = median(&sum[b - FAR..=b - NEAR]).max(median(&sum[b + NEAR..=b + FAR]));
            if noise > 0.0 {
                sum[b] / noise
            } else {
                0.0
            }
        })
        .fold(0.0, f32::max)
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

/// Synthetic receive audio for the gate's tests and the model replay: machine-keyed CW at a
/// known SNR in white noise, deterministic.
#[cfg(test)]
mod synth {
    pub const SR: u32 = 12_000;

    /// SplitMix64 → Box-Muller: deterministic Gaussian noise, no dependency.
    pub struct Gauss(u64);
    impl Gauss {
        pub fn new(seed: u64) -> Self {
            Self(seed)
        }
        fn uniform(&mut self) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
        }
        pub fn next(&mut self) -> f32 {
            let (u1, u2) = (self.uniform().max(1e-12), self.uniform());
            ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
        }
    }

    /// The keyed tone's amplitude, and the white-noise sigma that puts it at `snr_db` in the
    /// usual 2500 Hz reference bandwidth.
    pub const AMP: f32 = 0.3;
    pub fn sigma(snr_db: f32) -> f32 {
        let p_tone = AMP * AMP / 2.0;
        let n0 = p_tone / (2500.0 * 10f32.powf(snr_db / 10.0));
        (n0 * SR as f32 / 2.0).sqrt()
    }

    /// `lead` s of noise, `text` keyed at `wpm` and `pitch`, then `tail` s of noise.
    pub fn cw(
        text: &str,
        wpm: u32,
        pitch: f32,
        snr_db: f32,
        lead: f32,
        tail: f32,
        seed: u64,
    ) -> (Vec<f32>, std::ops::Range<usize>) {
        let keyed = tempo_core::cw::morse_samples(text, wpm, pitch, SR);
        let peak = keyed.iter().fold(0f32, |m, x| m.max(x.abs()));
        let start = (lead * SR as f32) as usize;
        let mut audio = vec![0f32; start + keyed.len() + (tail * SR as f32) as usize];
        for (a, k) in audio[start..].iter_mut().zip(&keyed) {
            *a = k / peak * AMP;
        }
        let s = sigma(snr_db);
        let mut g = Gauss::new(seed);
        for a in audio.iter_mut() {
            *a += s * g.next();
        }
        // The keying's span, from its first to its last keyed sample.
        let first = keyed.iter().position(|x| x.abs() > 1e-6).unwrap_or(0);
        let last = keyed.iter().rposition(|x| x.abs() > 1e-6).unwrap_or(0);
        (audio, start + first..start + last + 1)
    }

    /// The engine's window at stream position `fed`: the last 15 s, zero-padded at the front.
    pub fn window(audio: &[f32], fed: usize) -> Vec<f32> {
        let win = 15 * SR as usize;
        let from = fed.saturating_sub(win);
        let mut w = vec![0.0; win - (fed - from)];
        w.extend_from_slice(&audio[from..fed]);
        w
    }
}

#[cfg(test)]
mod gate_tests {
    use super::synth::*;
    use super::*;

    /// A quarter second of audio per turn: the thread's idle poll.
    const STEP: usize = 3_000;

    /// Every turn over `audio`, a quarter second apart: (fed, decision).
    fn run(audio: &[f32]) -> Vec<(usize, Turn)> {
        let mut gate = Gate::default();
        (1..=audio.len() / STEP)
            .map(|i| {
                let fed = i * STEP;
                (fed, gate.turn(1, &window(audio, fed), fed as u64))
            })
            .collect()
    }

    fn decodes(turns: &[(usize, Turn)]) -> usize {
        turns.iter().filter(|(_, t)| *t == Turn::Decode).count()
    }

    /// No audio arriving (a stopped capture device): the window is the one already decoded,
    /// and so is every one after it until audio flows again.
    #[test]
    fn no_new_audio_runs_no_pass() {
        let (audio, _) = cw("CQ CQ", 20, 600.0, 10.0, 0.5, 1.0, 1);
        let mut gate = Gate::default();
        let fed = audio.len();
        let w = window(&audio, fed);
        assert_eq!(gate.turn(1, &w, fed as u64), Turn::Decode);
        for _ in 0..5 {
            assert_eq!(gate.turn(1, &w, fed as u64), Turn::Wait);
        }
        // Less than one model hop more is still the same window.
        assert_eq!(gate.turn(1, &w, fed as u64 + 179), Turn::Wait);
        assert_eq!(gate.turn(1, &w, fed as u64 + 180), Turn::Decode);
    }

    /// Digital silence (a muted or unplugged input) never runs the model, and every turn
    /// still publishes an empty pass, so the decoder's clock keeps up with the stream.
    #[test]
    fn silence_runs_no_pass() {
        let turns = run(&vec![0.0; 60 * SR as usize]);
        assert_eq!(turns.len(), 240);
        for (fed, t) in turns {
            let heard_until = (fed as u64).saturating_sub(2 * RATE);
            assert_eq!(t, Turn::Quiet { heard_until }, "{fed}");
        }
    }

    /// Band noise alone, white or through a CW filter, never runs the model.
    #[test]
    fn band_noise_runs_no_pass() {
        for seed in 1..=3 {
            let (noise, _) = cw("", 20, 600.0, 0.0, 60.0, 0.0, seed);
            assert_eq!(decodes(&run(&noise)), 0, "white noise, seed {seed}");
        }
        // The same noise through a 500 Hz-wide receive filter centred on 700 Hz (an RBJ
        // band-pass biquad): its slopes are inside the detector's band.
        let (noise, _) = cw("", 20, 600.0, 0.0, 60.0, 0.0, 9);
        let (w0, q) = (std::f32::consts::TAU * 700.0 / SR as f32, 700.0 / 500.0);
        let alpha = w0.sin() / (2.0 * q);
        let (b0, a0, a1, a2) = (alpha, 1.0 + alpha, -2.0 * w0.cos(), 1.0 - alpha);
        let (mut x1, mut x2, mut y1, mut y2) = (0f32, 0f32, 0f32, 0f32);
        let filtered: Vec<f32> = noise
            .iter()
            .map(|&x| {
                let y = (b0 * x - b0 * x2 - a1 * y1 - a2 * y2) / a0;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                y
            })
            .collect();
        assert_eq!(decodes(&run(&filtered)), 0, "filtered noise");
    }

    /// Keying far weaker than the model can copy (−15 dB; it copies nothing under about −9 dB)
    /// opens the gate within two seconds of the first element, at any pitch the model hears,
    /// and keeps it open until the keying has left the 15 s window. Then the model rests.
    #[test]
    fn weak_cw_is_decoded_for_as_long_as_the_window_holds_it() {
        for (i, pitch) in [450.0, 700.0, 1150.0].into_iter().enumerate() {
            let (audio, keyed) = cw("CQ CQ DE W1AW K", 20, pitch, -15.0, 5.0, 25.0, i as u64);
            let turns = run(&audio);
            let mut listened = 0;
            for &(fed, t) in &turns {
                // How far the decoder has listened never runs backwards.
                let heard = match t {
                    Turn::Decode => fed as u64,
                    Turn::Quiet { heard_until } => heard_until,
                    Turn::Wait => listened,
                };
                assert!(
                    heard >= listened,
                    "{pitch} Hz, {fed}: {heard} after {listened}"
                );
                listened = heard;
                let in_window =
                    fed > keyed.start && fed.saturating_sub(keyed.end) < WINDOW as usize;
                if fed >= keyed.start + 2 * SR as usize && in_window {
                    assert_eq!(
                        t,
                        Turn::Decode,
                        "{pitch} Hz, {fed}: a window holding the keying"
                    );
                }
                if fed + WINDOW as usize <= keyed.start || fed >= keyed.end + 17 * SR as usize {
                    assert!(
                        matches!(t, Turn::Quiet { .. }),
                        "{pitch} Hz, {fed}: noise only"
                    );
                }
            }
        }
    }

    /// A new stream (a QSY, a toggle) starts the gate over: nothing the old one heard opens it.
    #[test]
    fn a_new_stream_starts_the_gate_over() {
        let (cw_audio, _) = cw("CQ CQ DE W1AW", 20, 600.0, 0.0, 0.5, 0.5, 2);
        let mut gate = Gate::default();
        let fed = cw_audio.len();
        assert_eq!(
            gate.turn(1, &window(&cw_audio, fed), fed as u64),
            Turn::Decode
        );
        let quiet = vec![0.0; SR as usize];
        let t = gate.turn(2, &window(&quiet, SR as usize), SR as u64);
        assert_eq!(t, Turn::Quiet { heard_until: 0 });
    }
}

/// The gate against the real model: a weak signal's copy with the gate is the copy without it.
/// Needs the model, which is an app resource and not in this repo: point `DEEPCW_DIR` at a
/// folder holding `model.onnx` + `model.onnx.json` (the station build stages it in
/// `src-tauri/resources/deepcw`), and run in release, e.g.
/// `cargo test --release -p tempo-audio --features device,serial,ai-cw --lib aicw_stitch::model_replay -- --include-ignored --nocapture`.
/// Without the model each test says so and passes.
#[cfg(all(test, feature = "ai-cw"))]
mod model_replay {
    use super::synth::*;
    use super::*;
    use std::time::Instant;

    fn model() -> Option<deepcw::DeepCw> {
        let dir = std::env::var_os("DEEPCW_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../src-tauri/resources/deepcw")
            });
        match deepcw::DeepCw::load(&dir) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("skipped: the DeepCW model is not installed ({e})");
                None
            }
        }
    }

    /// What one replay gave: the copy (confirmed text, then the provisional tail), the passes
    /// the model ran, and the time they took.
    struct Replay {
        copy: String,
        pending: String,
        passes: usize,
        busy: Duration,
    }

    /// The decode thread's rule over `audio`, one turn every `every` samples, with the gate or
    /// with every turn decoded (the decoder before the gate).
    fn replay(ai: &deepcw::DeepCw, audio: &[f32], every: usize, gated: bool) -> Replay {
        let (mut stitch, mut gate) = (Stitcher::default(), Gate::default());
        let mut r = Replay {
            copy: String::new(),
            pending: String::new(),
            passes: 0,
            busy: Duration::ZERO,
        };
        for fed in (every..=audio.len()).step_by(every) {
            let window = window(audio, fed);
            if gated {
                match gate.turn(1, &window, fed as u64) {
                    Turn::Wait => continue,
                    Turn::Quiet { .. } => {
                        r.pending.clear();
                        continue;
                    }
                    Turn::Decode => {}
                }
            }
            let t0 = Instant::now();
            let chars = ai
                .decode_timed(&deepcw::resample_linear(&window, SR, ai.meta.sample_rate))
                .expect("decode");
            r.busy += t0.elapsed();
            r.passes += 1;
            let p = stitch.pass(1, fed as f64 / SR as f64, 15.0, &chars);
            r.copy.push_str(if r.copy.is_empty() {
                p.confirmed.trim_start()
            } else {
                &p.confirmed
            });
            r.pending = p.pending;
        }
        r
    }

    /// The hard requirement: weak CW, at and just above the weakest the model copies, comes
    /// out letter for letter the same with the gate as without it, and the gate still saves
    /// the passes over the noise around it.
    #[test]
    #[ignore = "needs the DeepCW model (see the module header); run in release"]
    fn a_weak_signal_is_copied_the_same_with_the_gate() {
        let Some(ai) = model() else { return };
        for (i, snr) in [-6.0, -3.0, 0.0].into_iter().enumerate() {
            let (audio, _) = cw(
                "CQ CQ DE W1AW W1AW K",
                20,
                650.0,
                snr,
                20.0,
                20.0,
                40 + i as u64,
            );
            let every = 9_000; // a pass every 0.75 s of audio: a fast PC's cadence
            let plain = replay(&ai, &audio, every, false);
            let gated = replay(&ai, &audio, every, true);
            eprintln!(
                "{snr:>4} dB: {:?} + {:?}, {} passes; gated {:?} + {:?}, {} passes",
                plain.copy, plain.pending, plain.passes, gated.copy, gated.pending, gated.passes
            );
            assert_eq!(gated.copy, plain.copy, "{snr} dB: the copy");
            assert_eq!(
                gated.pending, plain.pending,
                "{snr} dB: the provisional tail"
            );
            assert!(
                !plain.copy.trim().is_empty(),
                "{snr} dB: the model copied something"
            );
            assert!(
                gated.passes < plain.passes,
                "{snr} dB: the noise around it costs no pass"
            );
        }
    }

    /// The measurement behind the fix: inference passes and CPU over a minute of a quiet band
    /// (white noise) and of digital silence, before (every turn decoded, on the thread's own
    /// back-to-back cadence) and after (the gate, polled as the thread polls when idle).
    #[test]
    #[ignore = "needs the DeepCW model (see the module header); run in release"]
    fn measure_passes_and_cpu_on_a_quiet_band() {
        let Some(ai) = model() else { return };
        let minute = 60 * SR as usize;
        let (noise, _) = cw("", 20, 600.0, 0.0, 60.0, 0.0, 77);
        for (name, audio) in [("band noise", noise), ("silence", vec![0.0; minute])] {
            // Before: back to back, each pass followed by `pass_gap`; the audio moves on by
            // the pass plus the rest.
            let (mut fed, mut passes, mut busy) = (SR as usize, 0, Duration::ZERO);
            while fed <= minute {
                let w = window(&audio, fed);
                let t0 = Instant::now();
                let _ = ai.decode_timed(&deepcw::resample_linear(&w, SR, ai.meta.sample_rate));
                let took = t0.elapsed();
                busy += took;
                passes += 1;
                fed += ((took + pass_gap(took)).as_secs_f64() * SR as f64) as usize;
            }
            eprintln!(
                "{name}, before: {passes} passes/min, {:.1}% of a core",
                busy.as_secs_f64() / 60.0 * 100.0
            );
            // After: a turn every quarter second (the idle poll); the model never runs here.
            let mut gate = Gate::default();
            let (mut decoded, mut busy) = (0, Duration::ZERO);
            for fed in (3_000..=minute).step_by(3_000) {
                let w = window(&audio, fed);
                let t0 = Instant::now();
                if gate.turn(1, &w, fed as u64) == Turn::Decode {
                    decoded += 1;
                }
                busy += t0.elapsed();
            }
            eprintln!(
                "{name}, after: {decoded} passes/min, gate {:.2}% of a core",
                busy.as_secs_f64() / 60.0 * 100.0
            );
            assert_eq!(decoded, 0, "{name}");
        }
    }
}
