//! Internal rational resampler: caller's audio rate → 11025 Hz working rate.
//!
//! Hand-rolled 64-tap Hann-windowed-sinc polyphase FIR with 256 phase
//! positions. Tap rows are precomputed once in
//! [`Resampler::new`] (~64 KB); the hot path in [`Resampler::process`] is
//! a quantized-phase lookup + 64-tap multiply-accumulate — no
//! transcendentals per output sample.
//!
//! We picked this over `rubato` for zero extra deps and a small file.
//! Quality target is "audible loss < 0.1 dB across SSTV-relevant
//! frequencies (1500-2300 Hz)" — easily met at typical input rates
//! (44.1k, 48k). Translated in spirit from slowrx's implicit resampling
//! inside `pcm.c`'s 44.1 kHz read loop.

use crate::error::{Error, Result};

/// Working sample rate the decoder operates at internally. Any caller
/// sample rate is resampled to this before processing.
pub const WORKING_SAMPLE_RATE_HZ: u32 = 11_025;

/// Maximum supported caller input sample rate.
pub const MAX_INPUT_SAMPLE_RATE_HZ: u32 = 192_000;

/// Number of FIR taps. Higher = sharper transition + more CPU.
/// 64 is the sweet spot at our quality target.
const FIR_TAPS: usize = 64;

/// Number of polyphase positions. Each fractional output sample's
/// `frac` is quantized to one of `NUM_PHASES` precomputed tap rows via
/// round-to-nearest with a clamp at the top edge (so the bucket for
/// `frac` very close to 1.0 is one-sided rather than wrapping). 256
/// gives a max sub-sample position error of `1 / (2·NUM_PHASES) = 1/512`
/// sample across the interior, rising to `1/NUM_PHASES = 1/256` at the
/// top bucket (`frac > (NUM_PHASES − 0.5)/NUM_PHASES`); at 11025 Hz this
/// is ≈ 177 ns typical / 354 ns worst-case time error. RMS phase noise
/// on a 2300 Hz tone (SSTV's highest video frequency) is ≈ −52 dB, well
/// below the audible threshold and SSTV's noise floor. Memory cost:
/// `NUM_PHASES × FIR_TAPS × 4 B` = 64 KB per `Resampler`.
const NUM_PHASES: usize = 256;

/// Polyphase FIR resampler. Stateful — holds a tail buffer to avoid
/// glitches across `process` calls.
///
/// **Group delay:** the 64-tap symmetric FIR has linear-phase group delay
/// of `(FIR_TAPS - 1) / 2 = 31.5` input-rate samples (≈ 715 µs at 44.1 kHz,
/// ≈ 2.86 ms at 11.025 kHz). Output is shifted right by this amount
/// relative to input. SSTV's `find_sync` re-anchors the rate against sync
/// pulses, so this is invisible inside the decoder pipeline; standalone
/// consumers should compensate if they need sample-accurate alignment.
pub struct Resampler {
    input_rate: u32,
    /// `input_rate / WORKING_SAMPLE_RATE_HZ`, expressed as a stride.
    stride: f64,
    /// Position into the input buffer (fractional, accumulates across calls).
    phase: f64,
    /// Carry-over input samples from the previous call.
    tail: Vec<f32>,
    /// 256-phase polyphase tap bank, indexed by `frac` quantized to
    /// 1/256 sub-sample. Built once in [`Resampler::new`] (~64 KB, static
    /// for the resampler's lifetime). Each row is a Hann-windowed sinc at
    /// the corresponding fractional delay. Raw taps (no normalization
    /// pass) — the windowed-sinc form already sums to ~1.0 at typical
    /// `fc` (the audit's D1 claim of "~6 dB attenuation" was a phantom
    /// finding, verified by the
    /// `exact_rate_preserves_amplitude_and_no_attenuation` test).
    taps: Box<[[f32; FIR_TAPS]; NUM_PHASES]>,
}

/// Cutoff frequency (Hz) for the resampler, derived from the input rate.
/// `min(input_rate, WORKING_SAMPLE_RATE_HZ) × 0.45`, hard-capped at 4500
/// Hz. The 0.45 factor leaves a small transition band below Nyquist of
/// the lower rate; the 4500 Hz cap pins the absolute cutoff at typical
/// input rates (44.1k / 48k → 4961 Hz uncapped → 4500 Hz capped), so the
/// passband easily covers SSTV's 1500–2300 Hz video band with room for
/// the 64-tap transition rolloff.
fn cutoff_hz(input_rate: u32) -> f64 {
    (f64::from(input_rate.min(WORKING_SAMPLE_RATE_HZ)) * 0.45).min(4500.0)
}

/// Compute one Hann-windowed sinc FIR tap value for a given tap index
/// and fractional phase. Called once per `(phase, tap)` pair from
/// [`Resampler::new`] to populate the polyphase tap bank — never called
/// from the hot path.
///
/// `tap_index` is in 0..`FIR_TAPS`. `frac` is in [0, 1) — the sub-sample
/// offset of the output sample's center from the integer input grid.
/// `fc` is the cutoff normalized to input rate (`cutoff_hz / input_rate`).
///
/// Sinc shifts with `frac`; Hann window stays anchored to the tap grid.
/// This is the standard windowed-sinc fractional-delay formulation —
/// see e.g. Smith, "Digital Audio Resampling Home Page" (CCRMA, 2002).
///
/// The taps already sum to ~1.0 at typical `fc` (the audit's D1 claim of
/// "~6 dB attenuation" was a phantom finding — see the
/// `exact_rate_preserves_amplitude_and_no_attenuation` test for the
/// regression guard).
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn fir_tap(tap_index: usize, frac: f64, fc: f64) -> f32 {
    let m = FIR_TAPS as f64;
    let n = (tap_index as f64) - (m - 1.0) / 2.0 - frac;
    let sinc = if n.abs() < 1e-12 {
        2.0 * fc
    } else {
        (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n)
    };
    let w = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * (tap_index as f64) / (m - 1.0)).cos());
    // Tap values are bounded in [-1, 1]; the f32 cast is exact-enough.
    (sinc * w) as f32
}

impl Resampler {
    /// Construct a resampler converting `input_rate` → [`WORKING_SAMPLE_RATE_HZ`].
    ///
    /// # Errors
    /// Returns [`Error::InvalidSampleRate`] if `input_rate` is 0 or
    /// > [`MAX_INPUT_SAMPLE_RATE_HZ`].
    #[allow(clippy::cast_precision_loss, clippy::large_stack_arrays)]
    pub fn new(input_rate: u32) -> Result<Self> {
        if input_rate == 0 || input_rate > MAX_INPUT_SAMPLE_RATE_HZ {
            return Err(Error::InvalidSampleRate { got: input_rate });
        }
        let cutoff_norm = cutoff_hz(input_rate) / f64::from(input_rate);

        // Build the 256-phase polyphase tap bank — one 64-tap row per
        // quantized fractional phase. Computed once here, looked up in
        // the hot path (no transcendentals per output sample). No
        // normalization pass: raw Hann-windowed-sinc taps already sum
        // to ~1.0 at typical `fc` (audit #87 D1 — phantom finding,
        // verified by `exact_rate_preserves_amplitude_and_no_attenuation`).
        let mut taps: Box<[[f32; FIR_TAPS]; NUM_PHASES]> =
            Box::new([[0.0_f32; FIR_TAPS]; NUM_PHASES]);
        for phase_idx in 0..NUM_PHASES {
            let frac = (phase_idx as f64) / (NUM_PHASES as f64);
            for k in 0..FIR_TAPS {
                taps[phase_idx][k] = fir_tap(k, frac, cutoff_norm);
            }
        }

        Ok(Self {
            input_rate,
            stride: f64::from(input_rate) / f64::from(WORKING_SAMPLE_RATE_HZ),
            phase: 0.0,
            tail: Vec::new(),
            taps,
        })
    }

    /// Resample a chunk of input audio into working-rate output.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::needless_range_loop
    )]
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        // Concatenate carry-over with the new chunk.
        let mut buf = std::mem::take(&mut self.tail);
        buf.extend_from_slice(input);

        let mut out = Vec::new();
        loop {
            // D2b off-by-one fix (#87): the kernel reads indices
            // `floor(phase)..floor(phase) + FIR_TAPS`, so it needs
            // `floor(phase) + FIR_TAPS` samples in `buf`. Pre-#87 this
            // was `(phase + FIR_TAPS).ceil()`, which over-reserved by one
            // sample for fractional `phase`.
            let needed_end = (self.phase.floor() as usize) + FIR_TAPS;
            if needed_end > buf.len() {
                break;
            }
            let frac = self.phase.fract();
            let phase_idx = ((frac * NUM_PHASES as f64).round() as usize).min(NUM_PHASES - 1);
            let taps = &self.taps[phase_idx];
            let start = self.phase.floor() as isize;

            // Convolve using the precomputed taps at this quantized phase.
            // No transcendentals in the hot path.
            let mut acc: f32 = 0.0;
            for k in 0..FIR_TAPS {
                let idx = start + k as isize;
                if (0..buf.len() as isize).contains(&idx) {
                    acc += taps[k] * buf[idx as usize];
                }
            }
            out.push(acc);
            self.phase += self.stride;
        }

        // Keep the trailing samples that the next call will need.
        let drop = self.phase.floor() as usize;
        if drop < buf.len() {
            self.tail = buf[drop..].to_vec();
            self.phase -= drop as f64;
        } else {
            // Reached only when `buf.len() == 0` (empty input on a fresh
            // resampler, or after a prior call drained the tail) —
            // `phase` stays ∈ [0, 1) post-loop, so `drop = floor(phase) = 0`
            // and `drop < buf.len()` is `0 < 0 = false`. For any non-empty
            // `buf` under `MAX_INPUT_SAMPLE_RATE_HZ`, this branch is
            // unreachable. Exercised by the `empty_input_returns_empty`
            // test.
            self.tail.clear();
            self.phase -= buf.len() as f64;
        }
        out
    }

    /// Caller-provided input sample rate.
    #[must_use]
    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Clear FIR tail buffer + phase accumulator so a subsequent call to
    /// `process` starts with a clean state. Keeps the input rate, cutoff,
    /// and stride — the rate doesn't change across `reset_state` calls.
    pub(crate) fn reset_state(&mut self) {
        self.tail.clear();
        self.phase = 0.0;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::float_cmp,
    clippy::expect_used
)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn synth_tone_at(rate: u32, freq_hz: f64, secs: f64) -> Vec<f32> {
        let n = (secs * f64::from(rate)).round() as usize;
        (0..n)
            .map(|i| {
                let t = (i as f64) / f64::from(rate);
                (2.0 * PI * freq_hz * t).sin() as f32
            })
            .collect()
    }

    #[test]
    fn rejects_zero_rate() {
        assert!(matches!(
            Resampler::new(0),
            Err(Error::InvalidSampleRate { got: 0 })
        ));
    }

    #[test]
    fn rejects_oversize_rate() {
        assert!(matches!(
            Resampler::new(MAX_INPUT_SAMPLE_RATE_HZ + 1),
            Err(Error::InvalidSampleRate { .. })
        ));
    }

    #[test]
    fn accepts_common_rates() {
        for rate in [8_000, 11_025, 22_050, 32_000, 44_100, 48_000, 96_000] {
            assert!(Resampler::new(rate).is_ok(), "{rate} should be accepted");
        }
    }

    #[test]
    fn passthrough_when_rate_matches_working_rate() {
        // At equal rates the resampler still applies its FIR (no special-case
        // bypass). Verify the output length is approximately equal to input
        // length and that a 1500 Hz tone survives.
        let mut r = Resampler::new(WORKING_SAMPLE_RATE_HZ).unwrap();
        let in_audio = synth_tone_at(WORKING_SAMPLE_RATE_HZ, 1500.0, 0.1);
        let out = r.process(&in_audio);
        // Allow up to FIR_TAPS samples of length variance (group delay + tail).
        let expected = in_audio.len();
        assert!(
            (out.len() as isize - expected as isize).abs() < 100,
            "len mismatch: out={} expected≈{}",
            out.len(),
            expected
        );
        let p = crate::dsp::goertzel_power(&out, 1500.0);
        let p_off = crate::dsp::goertzel_power(&out, 800.0);
        assert!(p > 10.0 * p_off, "tone should survive: {p} vs {p_off}");
    }

    #[test]
    fn resamples_44100_to_11025_preserves_tone_frequency() {
        // 1 second of 1900 Hz at 44100 Hz → resample → expect 1900 Hz at 11025 Hz.
        let mut r = Resampler::new(44_100).expect("44.1k resampler");
        let in_audio = synth_tone_at(44_100, 1900.0, 1.0);
        let out = r.process(&in_audio);
        // Output should be ~11025 samples (1 second at working rate)
        let expected = WORKING_SAMPLE_RATE_HZ as usize;
        assert!(
            (out.len() as isize - expected as isize).abs() < 200,
            "out.len()={} expected≈{expected}",
            out.len()
        );
        // Goertzel power at 1900 Hz should be much greater than at 1700/2100 Hz.
        let p_target = crate::dsp::goertzel_power(&out, 1900.0);
        let p_off1 = crate::dsp::goertzel_power(&out, 1700.0);
        let p_off2 = crate::dsp::goertzel_power(&out, 2100.0);
        assert!(
            p_target > 10.0 * p_off1.max(p_off2),
            "p1900={p_target} p1700={p_off1} p2100={p_off2}"
        );
    }

    #[test]
    fn resamples_48000_to_11025() {
        let mut r = Resampler::new(48_000).expect("48k resampler");
        let in_audio = synth_tone_at(48_000, 1500.0, 0.5);
        let out = r.process(&in_audio);
        let expected = (WORKING_SAMPLE_RATE_HZ / 2) as usize;
        assert!((out.len() as isize - expected as isize).abs() < 200);
    }

    #[test]
    fn resamples_48000_to_11025_preserves_tone_quality() {
        // 0.5 s of 1900 Hz at 48 kHz, non-integer ratio (4.354...).
        // Pre-fix this test would have shown ~10× signal-to-noise margin
        // around 1900 Hz; with proper polyphase the margin should be 100×+.
        let mut r = Resampler::new(48_000).expect("48k resampler");
        let in_audio = synth_tone_at(48_000, 1900.0, 0.5);
        let out = r.process(&in_audio);
        let p_target = crate::dsp::goertzel_power(&out, 1900.0);
        let p_off1 = crate::dsp::goertzel_power(&out, 1700.0);
        let p_off2 = crate::dsp::goertzel_power(&out, 2100.0);
        // Tighter than the integer-ratio 10× threshold — non-integer
        // ratios with broken polyphase would NOT meet this.
        assert!(
            p_target > 50.0 * p_off1.max(p_off2),
            "p1900={p_target} p1700={p_off1} p2100={p_off2} (polyphase quality)"
        );
    }

    #[test]
    fn streaming_calls_are_consistent() {
        let mut r = Resampler::new(44_100).unwrap();
        let in_audio = synth_tone_at(44_100, 1900.0, 0.5);
        let single = r.process(&in_audio);
        let mut r2 = Resampler::new(44_100).unwrap();
        let mid = in_audio.len() / 2;
        let mut split = r2.process(&in_audio[..mid]);
        split.extend_from_slice(&r2.process(&in_audio[mid..]));
        // Length should match within ±2 samples; per-sample diff should be
        // tiny (filter edge effects).
        assert!((single.len() as isize - split.len() as isize).abs() <= 2);
        let common = single.len().min(split.len());
        let max_diff = (0..common)
            .map(|i| (single[i] - split[i]).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_diff < 0.01, "max_diff={max_diff}");
    }

    /// Unit-gain regression guard (#87). The audit (D1) claimed the 64
    /// Hann-windowed sinc taps weren't normalized to unit DC gain and the
    /// resampler attenuated by ~6 dB. Empirically false — the
    /// windowed-sinc form `2·fc · sin(2π·fc·n)/(π·n)` already sums to
    /// ~1.0 at typical `fc` (the audit appears to have confused the Hann
    /// *window*'s mean (= 0.5) with the Hann-*windowed-sinc*'s DC gain).
    /// This test passes on current code and stays as a guard against any
    /// future change (rate changes, tap-count tweaks, window swaps) that
    /// breaks unit gain unexpectedly. At
    /// `input_rate == WORKING_SAMPLE_RATE_HZ` the stride is exactly 1.0
    /// and every output sample has `frac == 0`, so the fractional-delay
    /// machinery isn't exercised — gain issues show up cleanly.
    #[test]
    fn exact_rate_preserves_amplitude_and_no_attenuation() {
        let mut r = Resampler::new(WORKING_SAMPLE_RATE_HZ).unwrap();
        // 200 samples at amplitude 0.8 — well past the 64-tap kernel ramp-up.
        let amplitude = 0.8_f32;
        let in_audio: Vec<f32> = (0..200)
            .map(|i| {
                let t = f64::from(i) / f64::from(WORKING_SAMPLE_RATE_HZ);
                (f64::from(amplitude) * (2.0 * PI * 1500.0 * t).sin()) as f32
            })
            .collect();
        let out = r.process(&in_audio);
        // Skip the first FIR_TAPS samples — the kernel is ramping up against
        // the left zero-pad and the peak amplitude is reduced there.
        let mid_start = FIR_TAPS.min(out.len());
        let out_peak = out[mid_start..]
            .iter()
            .fold(0.0_f32, |m, &x| m.max(x.abs()));
        let in_peak = in_audio.iter().fold(0.0_f32, |m, &x| m.max(x.abs()));
        // Allow ±5 % of input peak. The audit predicted ~50 % attenuation
        // (taps would sum to ~0.5); empirically the ratio is ~1.0 — the
        // windowed-sinc taps already have unity DC gain.
        let ratio = out_peak / in_peak;
        assert!(
            (ratio - 1.0).abs() < 0.05,
            "expected ~1.0 output peak/input peak ratio (unit gain), got {ratio} (in_peak={in_peak}, out_peak={out_peak})"
        );
    }

    /// F6 (#87). Upsampling 8 kHz → 11025 Hz exercises the `stride < 1`
    /// path that no existing test hits. Output length should be ~11025
    /// samples (1 second at working rate) ±64; Goertzel power at 1500 Hz
    /// should dominate adjacent off-band bins.
    #[test]
    fn upsampling_8khz_to_11025() {
        let mut r = Resampler::new(8_000).unwrap();
        let in_audio = synth_tone_at(8_000, 1500.0, 1.0);
        let out = r.process(&in_audio);
        let expected = WORKING_SAMPLE_RATE_HZ as usize;
        assert!(
            (out.len() as isize - expected as isize).abs() < 200,
            "out.len()={} expected≈{expected}",
            out.len()
        );
        let p_target = crate::dsp::goertzel_power(&out, 1500.0);
        let p_off1 = crate::dsp::goertzel_power(&out, 1200.0);
        let p_off2 = crate::dsp::goertzel_power(&out, 1800.0);
        assert!(
            p_target > 10.0 * p_off1.max(p_off2),
            "p1500={p_target} p1200={p_off1} p1800={p_off2}"
        );
    }

    /// F6 (#87). 192 kHz input — the max supported rate. Stride ≈ 17.41;
    /// many input samples per output. Just verify no panic, output length
    /// is in the right ballpark, and the tone survives.
    #[test]
    fn max_input_rate_192khz() {
        let mut r = Resampler::new(MAX_INPUT_SAMPLE_RATE_HZ).unwrap();
        let in_audio = synth_tone_at(MAX_INPUT_SAMPLE_RATE_HZ, 2000.0, 0.5);
        let out = r.process(&in_audio);
        // 0.5 s at WORKING_SAMPLE_RATE_HZ.
        let expected = (WORKING_SAMPLE_RATE_HZ / 2) as usize;
        assert!(
            (out.len() as isize - expected as isize).abs() < 200,
            "out.len()={} expected≈{expected}",
            out.len()
        );
        let p_target = crate::dsp::goertzel_power(&out, 2000.0);
        let p_off1 = crate::dsp::goertzel_power(&out, 1700.0);
        let p_off2 = crate::dsp::goertzel_power(&out, 2300.0);
        assert!(
            p_target > 10.0 * p_off1.max(p_off2),
            "p2000={p_target} p1700={p_off1} p2300={p_off2}"
        );
    }

    /// F6 (#87). Tiny chunks: each call passes fewer samples than the
    /// 64-tap kernel needs, so the resampler should accumulate them in
    /// `tail` and emit nothing until `tail.len() >= FIR_TAPS`. Verifies
    /// the streaming-buffer carry-over correctness — the production
    /// decoder's per-call audio chunks can be small.
    #[test]
    fn tiny_chunks_emit_nothing_then_catch_up() {
        let mut r = Resampler::new(44_100).unwrap();
        let chunk = [0.5_f32, 0.5, 0.5];
        let mut emitted_before_threshold = 0;
        // 21 chunks of 3 samples = 63 < FIR_TAPS = 64. No output yet.
        for _ in 0..21 {
            let out = r.process(&chunk);
            emitted_before_threshold += out.len();
        }
        assert_eq!(
            emitted_before_threshold, 0,
            "expected no output before FIR_TAPS samples buffered, got {emitted_before_threshold}"
        );
        // One more chunk pushes us past FIR_TAPS — at least one sample emerges.
        let out_after = r.process(&chunk);
        assert!(
            !out_after.is_empty(),
            "expected at least one output sample after crossing the FIR_TAPS threshold"
        );
    }

    /// F6 (#87). Empty input is a no-op — returns an empty Vec and
    /// leaves the resampler state untouched. Plus: an empty call
    /// sandwiched between two non-empty calls doesn't perturb the output
    /// (streaming idempotence).
    #[test]
    fn empty_input_returns_empty() {
        let mut r = Resampler::new(44_100).unwrap();
        assert!(r.process(&[]).is_empty());

        // Sandwich: process(non-empty) → process(empty) → process(non-empty)
        // should produce the same output as process(non-empty ++ non-empty).
        let mut a = Resampler::new(44_100).unwrap();
        let in_audio = synth_tone_at(44_100, 1500.0, 0.2);
        let mid = in_audio.len() / 2;

        let mut sandwiched = a.process(&in_audio[..mid]);
        let empty_call = a.process(&[]);
        assert!(empty_call.is_empty());
        sandwiched.extend_from_slice(&a.process(&in_audio[mid..]));

        let mut b = Resampler::new(44_100).unwrap();
        let combined = b.process(&in_audio);

        // Same length within 1, same per-sample values within tiny tolerance
        // (the empty call shouldn't have moved the FIR's internal state).
        assert!(
            (sandwiched.len() as isize - combined.len() as isize).abs() <= 1,
            "sandwiched.len()={} combined.len()={}",
            sandwiched.len(),
            combined.len()
        );
        let common = sandwiched.len().min(combined.len());
        let max_diff = (0..common)
            .map(|i| (sandwiched[i] - combined[i]).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_diff < 1e-6, "max_diff={max_diff}");
    }
}
