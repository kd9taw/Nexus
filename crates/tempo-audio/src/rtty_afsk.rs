//! Soundcard AFSK RTTY transmit path — the timing-cleanest way to put RTTY on the air.
//!
//! The rig sits in LSB (or a USB data mode with [`AfskConfig::reverse`]) and the app
//! plays a two-tone FSK waveform: mark 2125 Hz / space 2295 Hz (170 Hz shift),
//! 45.45 baud, async Baudot framing (1 start + 5 data + 1.5 stop bits per character).
//! Because the SOUND CARD clocks the bits, timing is jitter-free — this is the robust
//! default TX path (and the IC-9700/USB-D path). Contrast [`crate::rtty_fsk`], whose
//! bit edges come from OS thread scheduling.
//!
//! Two constraints this generator is built around:
//! - **True 45.45 baud, never integerized.** 12000 / 45.45 = 264.026… samples per
//!   bit — not an integer. Bit boundaries are accumulated in fractional samples so the
//!   cumulative error stays under one sample for any message length; rounding each bit
//!   to 264 samples would walk the far end's clock recovery off the straddle point
//!   mid-message.
//! - **Phase continuity + shaped edges.** Mark and space NCOs run continuously and a
//!   bit edge raised-cosine cross-fades between them — no phase steps, no key clicks,
//!   narrow keying sidebands. Generating FSK from two free-running oscillators, and
//!   what the bare switch between them costs in keying sidebands, is the analysis
//!   **Kok Chen, W7AY** published in "FSK Sidebands"
//!   (<http://www.w7ay.net/site/Technical/RTTY%20Sidebands/sidebands.html>); the
//!   shaped cross-fade here is this generator's answer to it. Implemented from that
//!   published design — no W7AY code is used, and this is NOT fldigi's shaped-RTTY
//!   transmit path either (fldigi sums two *independently* sinc-shaped oscillators
//!   and normalizes by a measured peak; here the two weights are complementary, so
//!   the sum is bounded by construction). Contrast `tempo_core::rtty::demod`, whose
//!   receive path IS ported from fldigi — see NOTICE.
//!
//! The ITA2/Baudot encoding itself (LTRS/FIGS, USOS, diddle) lives in
//! `tempo_core::rtty`; this module takes the already-encoded 5-bit stream.
//!
//! TWO ENTRY POINTS, and the difference is which transmissions each can render.
//! [`afsk_samples`] renders a whole transmission at once — the Enter/macro path,
//! like `tempo_core::cw::morse_samples`, and the one that has been on the air
//! since v1. [`AfskStream`] renders the SAME waveform a chunk at a time, carrying
//! the oscillator phase, the cross-fade weight and the fractional bit clock across
//! calls, because continuous ("latched") TX types into a transmission whose end is
//! not known when it starts. Chunking is invisible: the two agree sample for
//! sample on the same bits, which is the contract
//! `a_latched_stream_renders_exactly_what_one_long_transmission_would` pins.

/// Standard mark tone (Hz): the LOWER audio tone, which on LSB lands as the HIGHER RF —
/// the on-air RTTY convention.
pub const MARK_HZ: f32 = 2125.0;
/// Standard space tone (Hz): mark + 170 Hz shift.
pub const SPACE_HZ: f32 = 2295.0;
/// Standard RTTY rate: exactly 45.45 baud (22.0022 ms/bit), NOT 45.
pub const BAUD_45: f64 = 45.45;
/// Raised-cosine cross-fade length at each bit edge (and the key-down/up envelope), s.
pub const EDGE_RAMP_S: f64 = 0.002;

/// AFSK generator settings. `Default` is the on-air standard: 2125/2295 Hz, 45.45 baud,
/// 12 kHz mono, LSB convention.
#[derive(Debug, Clone)]
/// ⚠️ REVERSE IS NOT APPLIED IN THIS STRUCT, and there is deliberately no field for it.
///
/// `mark_hz`/`space_hz` arrive ALREADY in the operator's sense. The one place reverse is applied
/// is `tempo_core::rtty::tone_pair`, which RX and the cockpit's waterfall cursors also call — so
/// all three agree by construction. This config used to carry a `reverse` flag and swap AGAIN in
/// both render paths; two swaps cancel, so through 1.9.0 reverse silently did NOTHING on
/// transmit while RX honoured it, and a reversed rig decoded correctly then answered in the
/// wrong sense. The field is removed rather than left unread, so it cannot be rewired.
pub struct AfskConfig {
    pub mark_hz: f32,
    pub space_hz: f32,
    pub baud: f64,
    pub sample_rate: u32,
}

impl Default for AfskConfig {
    fn default() -> Self {
        Self {
            mark_hz: MARK_HZ,
            space_hz: SPACE_HZ,
            baud: BAUD_45,
            sample_rate: 12_000,
        }
    }
}

/// Async Baudot framing: each consecutive group of 5 `data_bits` (one character's data
/// bits, in over-the-air order — ITA2 transmits LSB first) becomes
/// 1 start bit (space) + the 5 data bits + a 1.5-bit stop (mark), as
/// `(mark_level, width_in_bits)` pairs. `data_bits.len()` must be a multiple of 5; a
/// trailing partial group is ignored. Shared by the AFSK generator and the true-FSK
/// keyer schedule ([`crate::rtty_fsk::fsk_schedule`]).
pub fn baudot_frame(data_bits: &[bool]) -> Vec<(bool, f64)> {
    debug_assert!(
        data_bits.len().is_multiple_of(5),
        "5 data bits per Baudot character"
    );
    let mut out = Vec::with_capacity((data_bits.len() / 5) * 7);
    for ch in data_bits.chunks_exact(5) {
        out.push((false, 1.0)); // start bit: space
        for &b in ch {
            out.push((b, 1.0));
        }
        out.push((true, 1.5)); // stop: mark, 1.5 bits
    }
    out
}

/// Render a framed character stream (groups of 5 data bits, see [`baudot_frame`]) to
/// AFSK PCM — mono f32 in -1..1 at `cfg.sample_rate`.
pub fn afsk_char_samples(data_bits: &[bool], cfg: &AfskConfig) -> Vec<f32> {
    afsk_samples(&baudot_frame(data_bits), cfg)
}

/// Render a raw `(mark_level, width_in_bits)` sequence to AFSK PCM. This is the layer
/// under [`afsk_char_samples`] — the seam for non-framed output (diddle idle, tuning
/// carriers, tests).
pub fn afsk_samples(levels: &[(bool, f64)], cfg: &AfskConfig) -> Vec<f32> {
    use std::f64::consts::{PI, TAU};
    debug_assert!(cfg.baud > 0.0);
    let fs = cfg.sample_rate.max(1) as f64;
    let spb = fs / cfg.baud; // fractional samples per bit — kept fractional

    // Pass 1: expand levels to a per-sample tone selection. `t_bits` accumulates the
    // EXACT cumulative bit position; each level run fills to its fractional boundary,
    // so per-bit rounding never accumulates (drift < 1 sample forever).
    let mut tone_mark: Vec<bool> = Vec::new();
    let mut t_bits = 0.0f64;
    for &(mark, width) in levels {
        t_bits += width;
        let target = t_bits * spb;
        while (tone_mark.len() as f64) < target {
            tone_mark.push(mark);
        }
    }
    let n = tone_mark.len();
    if n == 0 {
        return Vec::new();
    }

    // Pass 2: dual continuous NCOs + raised-cosine cross-fade at edges. `w_lin` walks
    // linearly toward the current bit's tone; the raised-cosine of it weights the mark
    // oscillator (space gets the complement, so the summed amplitude never exceeds 1).
    // ⚠️ NO SWAP HERE. `cfg.mark_hz`/`cfg.space_hz` arrive ALREADY in the operator's sense —
    // both TX sites build them with `tempo_core::rtty::tone_pair(centre, shift, reverse)`, which
    // is the one place reverse is applied. Swapping again here applied it TWICE, and two swaps
    // cancel: reverse silently did nothing on transmit through 1.9.0, while RX (which calls
    // `tone_pair` once) honoured it — so a reversed rig decoded correctly and answered in the
    // wrong sense. Bites USB-data rigs; the reverse setting's own doc names the IC-9700.
    let (f_mark, f_space) = (cfg.mark_hz as f64, cfg.space_hz as f64);
    let dm = TAU * f_mark / fs;
    let ds = TAU * f_space / fs;
    let ramp_n = ((fs * EDGE_RAMP_S) as usize).max(1);
    let env_r = ramp_n.min(n / 2).max(1);
    let step = 1.0 / ramp_n as f64;
    let mut pm = 0.0f64;
    let mut ps = 0.0f64;
    let mut w_lin = if tone_mark[0] { 1.0f64 } else { 0.0 };
    let mut out = Vec::with_capacity(n);
    for (i, &mark) in tone_mark.iter().enumerate() {
        pm = (pm + dm) % TAU;
        ps = (ps + ds) % TAU;
        w_lin = if mark {
            (w_lin + step).min(1.0)
        } else {
            (w_lin - step).max(0.0)
        };
        let wm = 0.5 * (1.0 - (PI * w_lin).cos());
        // Raised-cosine key envelope at TX start/end kills the on/off clicks.
        let env = if i < env_r {
            0.5 * (1.0 - (PI * (i + 1) as f64 / env_r as f64).cos())
        } else if i >= n - env_r {
            0.5 * (1.0 - (PI * (n - i) as f64 / env_r as f64).cos())
        } else {
            1.0
        };
        out.push((env * (wm * pm.sin() + (1.0 - wm) * ps.sin())) as f32);
    }
    out
}

/// A RESUMABLE AFSK generator — the same waveform as [`afsk_samples`], rendered a
/// chunk at a time instead of all at once.
///
/// WHY THIS EXISTS: continuous ("latched") RTTY keys characters as the operator
/// types them, so the generator cannot know the whole transmission up front. It
/// cannot simply be called per character either — everything that makes
/// [`afsk_samples`] correct is a LOCAL that re-initialises on every call:
/// - `pm`/`ps`, the two NCO phases: a fresh call restarts both at 0, so every
///   chunk seam is a phase step — exactly the keying sidebands the W7AY
///   cross-fade in this module exists to prevent;
/// - `w_lin`, the cross-fade weight: restarting it mid-tone is a second step;
/// - the raised-cosine KEY envelope, which ramps both ends of whatever buffer it
///   is given — per character that is a 4 ms hole in the carrier every 165 ms,
///   which reads on the air as a dropout and destroys the far end's clock;
/// - the fractional bit accumulator, which re-rounds 264.026… samples/bit at
///   every call (measured: 8 samples of drift in 10 characters).
///
/// So the state lives here and the arithmetic is unchanged. The contract, pinned
/// by `a_latched_stream_renders_exactly_what_one_long_transmission_would`, is
/// that CHUNKING IS INVISIBLE: chunks concatenated are sample-for-sample the
/// one-shot render of the same bits. The key envelope belongs to the STREAM, not
/// the chunk — ramp up on the first samples emitted, ramp down only on the chunk
/// the caller marks `tail`.
///
/// [`afsk_samples`] is deliberately left alone: the one-shot Enter/macro path is
/// the shipped, on-air-proven TX path and a latch bug must not be able to reach
/// it.
pub struct AfskStream {
    cfg: AfskConfig,
    /// Mark/space NCO phases (rad), carried across chunks — the phase continuity.
    pm: f64,
    ps: f64,
    /// Cross-fade weight walking toward the current bit's tone, carried so a
    /// chunk boundary inside a bit is not an edge.
    w_lin: f64,
    /// EXACT cumulative bit position of the whole stream (never per-chunk), and
    /// the samples emitted for it. Together these are the fractional bit clock:
    /// each chunk fills to `t_bits * spb`, so rounding never accumulates.
    t_bits: f64,
    n_out: u64,
    /// False until the first chunk has picked the starting tone (`afsk_samples`
    /// seeds `w_lin` from its first bit; the stream seeds from its first bit).
    started: bool,
}

impl AfskStream {
    pub fn new(cfg: AfskConfig) -> Self {
        Self {
            cfg,
            pm: 0.0,
            ps: 0.0,
            w_lin: 0.0,
            t_bits: 0.0,
            n_out: 0,
            started: false,
        }
    }

    /// Render the next framed character chunk (groups of 5 data bits, see
    /// [`baudot_frame`]). `tail` = this is the last chunk of the transmission, so
    /// close it with the key-down ramp.
    pub fn char_chunk(&mut self, data_bits: &[bool], tail: bool) -> Vec<f32> {
        self.chunk(&baudot_frame(data_bits), tail)
    }

    /// Render the next raw `(mark_level, width_in_bits)` chunk — the seam under
    /// [`Self::char_chunk`], mirroring [`afsk_samples`].
    pub fn chunk(&mut self, levels: &[(bool, f64)], tail: bool) -> Vec<f32> {
        use std::f64::consts::{PI, TAU};
        debug_assert!(self.cfg.baud > 0.0);
        let fs = self.cfg.sample_rate.max(1) as f64;
        let spb = fs / self.cfg.baud;

        // Pass 1, as in `afsk_samples` — except `t_bits` and the emitted-sample
        // count are the STREAM's, so a chunk fills to the exact fractional
        // boundary the whole transmission is at, not to its own rounded one.
        let mut tone_mark: Vec<bool> = Vec::new();
        for &(mark, width) in levels {
            self.t_bits += width;
            let target = self.t_bits * spb;
            while ((self.n_out + tone_mark.len() as u64) as f64) < target {
                tone_mark.push(mark);
            }
        }
        let n = tone_mark.len();
        if n == 0 {
            return Vec::new();
        }

        // Pass 2: the same dual-NCO cross-fade, reading and writing the carried
        // oscillator state instead of locals.
        // No swap — see the one-shot path above. The pair arrives already in the operator's
        // sense from `tone_pair`; this is the reader that actually keys on air, and it was
        // double-applying reverse right alongside the other one.
        let (f_mark, f_space) = (self.cfg.mark_hz as f64, self.cfg.space_hz as f64);
        let dm = TAU * f_mark / fs;
        let ds = TAU * f_space / fs;
        let ramp_n = ((fs * EDGE_RAMP_S) as usize).max(1);
        let step = 1.0 / ramp_n as f64;
        if !self.started {
            self.w_lin = if tone_mark[0] { 1.0 } else { 0.0 };
            self.started = true;
        }
        let mut out = Vec::with_capacity(n);
        for (i, &mark) in tone_mark.iter().enumerate() {
            self.pm = (self.pm + dm) % TAU;
            self.ps = (self.ps + ds) % TAU;
            self.w_lin = if mark {
                (self.w_lin + step).min(1.0)
            } else {
                (self.w_lin - step).max(0.0)
            };
            let wm = 0.5 * (1.0 - (PI * self.w_lin).cos());
            // The key envelope is the STREAM's: ramp up over the first samples
            // ever emitted, ramp down only out of the chunk that ends the
            // transmission. Interior chunk boundaries get neither — that is the
            // whole difference from calling `afsk_samples` per character.
            let k = self.n_out + i as u64; // absolute sample index in the stream
            let mut env = 1.0;
            if k < ramp_n as u64 {
                env = 0.5 * (1.0 - (PI * (k + 1) as f64 / ramp_n as f64).cos());
            }
            if tail && i + ramp_n >= n {
                env = env.min(0.5 * (1.0 - (PI * (n - i) as f64 / ramp_n as f64).cos()));
            }
            out.push((env * (wm * self.pm.sin() + (1.0 - wm) * self.ps.sin())) as f32);
        }
        self.n_out += n as u64;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_is_start_five_data_and_one_and_a_half_stop() {
        let f = baudot_frame(&[true, false, false, true, true]);
        assert_eq!(f.len(), 7);
        assert_eq!(f[0], (false, 1.0)); // start = space
        let data: Vec<bool> = f[1..6].iter().map(|&(b, _)| b).collect();
        assert_eq!(data, vec![true, false, false, true, true]);
        assert!(f[1..6].iter().all(|&(_, w)| w == 1.0));
        assert_eq!(f[6], (true, 1.5)); // stop = mark, 1.5 bits
        let total: f64 = f.iter().map(|&(_, w)| w).sum();
        assert!((total - 7.5).abs() < 1e-12);
    }

    #[test]
    fn bit_clock_never_drifts_more_than_one_sample() {
        // 12000/45.45 = 264.026… samples/bit. An integerized bit would be short by
        // 0.026 samples — 26 samples off by bit 1000; the fractional accumulator must
        // stay within 1 sample of the exact product at every prefix.
        let levels: Vec<(bool, f64)> = (0..1000).map(|i| (i % 2 == 0, 1.0)).collect();
        let cfg = AfskConfig::default();
        let spb = 12000.0 / BAUD_45;
        for k in [1usize, 3, 45, 100, 454, 1000] {
            let n = afsk_samples(&levels[..k], &cfg).len() as f64;
            assert!((n - k as f64 * spb).abs() < 1.0, "k={k} n={n}");
        }
    }

    fn measured_hz(x: &[f32], fs: f64) -> f64 {
        let crossings = x.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
        crossings as f64 * fs / (2.0 * x.len() as f64)
    }

    #[test]
    fn mark_and_space_tones_via_zero_crossings() {
        let cfg = AfskConfig::default();
        let mark = afsk_samples(&[(true, 400.0)], &cfg);
        let space = afsk_samples(&[(false, 400.0)], &cfg);
        assert!((measured_hz(&mark, 12000.0) - 2125.0).abs() < 5.0);
        assert!((measured_hz(&space, 12000.0) - 2295.0).abs() < 5.0);
    }

    /// The modulator emits the pair IT WAS GIVEN, in order — it does not reverse anything.
    ///
    /// This test used to set a `reverse` flag on the config and assert the modulator swapped.
    /// That contract conflicted with `tone_pair`, which the callers (and RX, and the waterfall
    /// cursors) already use to apply reverse — so reverse was applied twice on transmit and
    /// cancelled. Now there is one convention: whoever builds the pair decides the sense, and
    /// this renders it faithfully. A REVERSED pair is therefore just a pair whose mark is the
    /// higher tone.
    #[test]
    fn the_modulator_renders_the_pair_it_was_given_without_reversing_it() {
        // Normal sense.
        let cfg = AfskConfig::default();
        let mark = afsk_samples(&[(true, 400.0)], &cfg);
        let space = afsk_samples(&[(false, 400.0)], &cfg);
        assert!((measured_hz(&mark, 12000.0) - 2125.0).abs() < 5.0);
        assert!((measured_hz(&space, 12000.0) - 2295.0).abs() < 5.0);

        // The REVERSED pair, exactly as `tone_pair(2210, 170, true)` returns it: mark is the
        // higher tone. The modulator must key it that way round and not "helpfully" swap back.
        let rev = AfskConfig {
            mark_hz: 2295.0,
            space_hz: 2125.0,
            ..AfskConfig::default()
        };
        let rmark = afsk_samples(&[(true, 400.0)], &rev);
        let rspace = afsk_samples(&[(false, 400.0)], &rev);
        assert!(
            (measured_hz(&rmark, 12000.0) - 2295.0).abs() < 5.0,
            "a reversed pair must key mark HIGH — a second swap here is the 1.9.0 bug"
        );
        assert!((measured_hz(&rspace, 12000.0) - 2125.0).abs() < 5.0);
    }

    /// Render `chars` one character per call and concatenate — the naive way to
    /// "stream" with the one-shot generator, and the POSITIVE CONTROL for the
    /// stream test below: every measurement that must pass on `AfskStream` has to
    /// FAIL here, or the measurement is not measuring anything.
    fn naive_per_char(chars: usize, cfg: &AfskConfig) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..chars {
            out.extend_from_slice(&afsk_char_samples(&[true; 5], cfg));
        }
        out
    }

    /// Worst sample-for-sample disagreement between two renderings of the same
    /// bits (shorter length counts as total disagreement).
    fn max_diff(a: &[f32], b: &[f32]) -> f64 {
        if a.len() != b.len() {
            return f64::INFINITY;
        }
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max) as f64
    }

    #[test]
    fn a_latched_stream_renders_exactly_what_one_long_transmission_would() {
        // THE FEATURE THIS EXISTS FOR: continuous ("latched") RTTY keys character
        // by character as the operator types, so the generator must be RESUMABLE.
        // The contract is the strongest one available and needs no new metric —
        // chunking must be invisible: N characters rendered one chunk at a time
        // must come out SAMPLE FOR SAMPLE identical to the one-shot render of the
        // same N characters, which is the waveform this app already ships and the
        // four tests above already pin. Anything that differs — a phase step, a
        // re-applied envelope, a re-rounded bit clock — shows up here.
        let cfg = AfskConfig::default();
        let mut bits = Vec::new();
        for _ in 0..10 {
            bits.extend_from_slice(&[true; 5]); // LTRS ×10 — the diddle idle stream
        }
        let one_shot = afsk_char_samples(&bits, &cfg);

        // POSITIVE CONTROL: the one-shot generator called ONCE PER CHARACTER — the
        // naive way to stream, and the reason this struct exists. It re-initialises
        // `pm`/`ps`/`w_lin` and re-ramps BOTH ends of every buffer, so it must
        // disagree grossly. If this ever stops failing, the comparison is broken,
        // not the world.
        let naive = naive_per_char(10, &cfg);
        assert!(
            max_diff(&naive, &one_shot) > 0.5,
            "the per-character control must NOT match one long transmission — it \
             re-inits the oscillators and re-ramps every buffer (diff={})",
            max_diff(&naive, &one_shot)
        );

        // THE STREAM: the same ten characters, one chunk each, one carrier. `tail`
        // on the last chunk is the key-DOWN ramp — the stream's own end.
        let mut s = AfskStream::new(cfg.clone());
        let mut streamed = Vec::new();
        for i in 0..10 {
            streamed.extend_from_slice(&s.char_chunk(&[true; 5], i == 9));
        }
        assert_eq!(
            streamed.len(),
            one_shot.len(),
            "chunking changed the duration"
        );
        assert!(
            max_diff(&streamed, &one_shot) < 1e-6,
            "chunking is not invisible: worst sample disagreement={}",
            max_diff(&streamed, &one_shot)
        );

        // …and chunk boundaries that do NOT fall on character boundaries are just
        // as invisible: the same bits split 3 + 7 must render the same waveform.
        let mut s2 = AfskStream::new(cfg);
        let mut split = s2.char_chunk(&bits[..15], false);
        split.extend_from_slice(&s2.char_chunk(&bits[15..], true));
        assert!(
            max_diff(&split, &one_shot) < 1e-6,
            "an uneven chunk split is visible in the waveform"
        );
    }

    #[test]
    fn a_streamed_bit_clock_never_drifts_more_than_one_sample() {
        // The fractional accumulator must survive CHUNKING: re-rounding 264.026…
        // samples/bit at every chunk boundary is the drift `afsk_samples` was
        // written to avoid, and a latched over is far longer than a one-shot one.
        let cfg = AfskConfig::default();
        let spb = 12000.0 / BAUD_45;
        let mut s = AfskStream::new(cfg);
        let mut n = 0usize;
        for k in 1..=200 {
            n += s.char_chunk(&[true; 5], false).len();
            // 7.5 bits per framed character, k characters in.
            assert!(
                (n as f64 - k as f64 * 7.5 * spb).abs() < 1.0,
                "drift at char {k}: n={n}"
            );
        }
    }

    #[test]
    fn no_phase_discontinuity_at_bit_edges() {
        use std::f64::consts::PI;
        // Max-transition data (alternating bits) across 20 chars. A continuous tone's
        // largest sample-to-sample step is its phase step (|sin a − sin b| ≤ |a − b|);
        // the cross-fade adds at most π/ramp_n. A hard phase reset would jump ~2.
        let mut bits = Vec::new();
        for _ in 0..20 {
            bits.extend_from_slice(&[true, false, true, false, true]);
        }
        let cfg = AfskConfig::default();
        let x = afsk_char_samples(&bits, &cfg);
        let max_step = x
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max) as f64;
        let ramp_n = (12000.0 * EDGE_RAMP_S).round();
        let bound = 2.0 * PI * 2295.0 / 12000.0 + PI / ramp_n + 0.05;
        assert!(max_step < bound, "max_step={max_step} bound={bound}");
        assert!(bound < 1.6); // a discontinuity-sized jump must still fail
    }
}
