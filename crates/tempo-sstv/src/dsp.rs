//! Generic DSP primitives used across the decoder.
//!
//! - [`build_hann`] — the canonical zero/one-safe Hann-window builder
//!   (the three pre-#85 copies in `vis.rs` / `sync.rs` / `snr.rs` collapse
//!   here, with the safe `n <= 1` handling).
//! - [`power`] — `|c|²` as `f64`, the replacement for three inline-closure
//!   copies.
//! - [`get_bin`] — frequency → FFT bin index with slowrx C-truncation
//!   semantics (moved from `lib.rs`).
//! - [`goertzel_power`] — single-bin DFT magnitude², with slowrx's
//!   snap-to-nearest-`k` (moved from `vis.rs`).
//! - [`tone_power`] — DFT magnitude² at *exactly* the frequency asked
//!   for. Read its header before reaching for `goertzel_power` in a
//!   frequency scan: on short windows the snapping is not a rounding
//!   error, it is the whole answer.
//!
//! Nothing here is SSTV-specific; per-channel demod machinery lives in
//! [`crate::demod`].

use rustfft::num_complex::Complex;

/// Build a Hann window of length `len`. Used for both the per-pixel demod's
/// [`HannBank`](crate::demod::HannBank) entries (lengths from
/// [`HANN_LENS`](crate::demod::HANN_LENS)) and the
/// [`SnrEstimator`](crate::snr::SnrEstimator)'s
/// [`FFT_LEN`](crate::demod::FFT_LEN)-sample `hann_long`.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn build_hann(len: usize) -> Vec<f32> {
    if len == 0 {
        return Vec::new();
    }
    if len == 1 {
        return vec![0.0_f32];
    }
    (0..len)
        .map(|i| {
            let m = (len - 1) as f32;
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * (i as f32) / m).cos())
        })
        .collect()
}

/// `|c|²` as `f64`. Used wherever an FFT bin's power (= magnitude²) is needed.
#[inline]
pub(crate) fn power(c: Complex<f32>) -> f64 {
    let r = f64::from(c.re);
    let i = f64::from(c.im);
    r * r + i * i
}

/// Translate a frequency in Hz to the nearest FFT bin index using slowrx's
/// C-truncation semantics.
///
/// slowrx's `GetBin` (`common.c:39-41`) is:
/// ```c
/// guint GetBin(double Freq, guint FFTLen) {
///     return (Freq / 44100 * FFTLen);  // implicit double→uint = truncation toward zero
/// }
/// ```
///
/// The implicit `double → guint` cast truncates toward zero.  We replicate
/// this with an `as usize` cast (well-defined for positive doubles: truncates
/// toward zero), which gives the same result as C for all frequencies used
/// in slowrx. **Do NOT change this to `.round()`** — that would deviate from
/// slowrx's bin assignments at 5 of the 8 production frequencies (800, 1200,
/// 1500, 2700, 3400 Hz), shifting SNR-estimator bandwidth divisors and the
/// sync tracker's `Praw`/`Psync` range.
///
/// # Numerical verification (both at slowrx-native 1024/44100 and our 256/11025
/// — same Hz/bin ratio, so bins are identical)
///
/// | Frequency | Expected bin |
/// |-----------|-------------|
/// | 400 Hz    | 9           |
/// | 800 Hz    | 18          |
/// | 1200 Hz   | 27          |
/// | 1500 Hz   | 34          |
/// | 1900 Hz   | 44          |
/// | 2300 Hz   | 53          |
/// | 2700 Hz   | 62          |
/// | 3400 Hz   | 78          |
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[inline]
pub(crate) fn get_bin(hz: f64, fft_len: usize, sample_rate_hz: u32) -> usize {
    (hz * fft_len as f64 / f64::from(sample_rate_hz)) as usize
}

/// Goertzel power on `samples` at `target_hz` (bin power, ~amplitude²).
/// Used by `decoder::estimate_freq` and the resample-quality tests.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn goertzel_power(samples: &[f32], target_hz: f64) -> f64 {
    let n = samples.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let k = (0.5 + n * target_hz / f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ)).floor();
    let coeff = 2.0 * (2.0 * std::f64::consts::PI * k / n).cos();
    let mut s_prev = 0.0_f64;
    let mut s_prev2 = 0.0_f64;
    for &sample in samples {
        let s = f64::from(sample) + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    s_prev2.mul_add(s_prev2, s_prev.mul_add(s_prev, -coeff * s_prev * s_prev2))
}

/// Power of the tone at **exactly** `target_hz` in `samples` — the DFT
/// magnitude² evaluated off the bin grid.
///
/// [`goertzel_power`] cannot answer this question. It snaps `target_hz`
/// to an integer Goertzel `k`, i.e. to a multiple of `sample_rate / n`,
/// which is fine at the window lengths slowrx uses it at and useless at
/// the length of a sync pulse: over a 36-sample pulse interior a bin is
/// 306 Hz wide, so a 2 Hz scan across ±250 Hz evaluates **three distinct
/// frequencies**, reports a plateau, and returns whichever end of it the
/// loop reached first. That is exactly how the blind decoder's mistuning
/// estimate came to answer −128 Hz for a perfectly tuned signal.
///
/// The `goertzel_power` snapping is slowrx-faithful and stays where
/// slowrx relies on it; this is for scans that need the frequency they
/// asked for.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn tone_power(samples: &[f32], target_hz: f64) -> f64 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let omega =
        2.0 * std::f64::consts::PI * target_hz / f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ);
    // Phase rotator rather than a `sin`/`cos` per sample; the windows here
    // are short enough that no renormalisation is warranted.
    let (step_c, step_s) = (omega.cos(), omega.sin());
    let (mut rot_c, mut rot_s) = (1.0_f64, 0.0_f64);
    let (mut i_acc, mut q_acc) = (0.0_f64, 0.0_f64);
    for &sample in samples {
        let v = f64::from(sample);
        i_acc += v * rot_c;
        q_acc += v * rot_s;
        let next_c = rot_c.mul_add(step_c, -(rot_s * step_s));
        rot_s = rot_c.mul_add(step_s, rot_s * step_c);
        rot_c = next_c;
    }
    i_acc.mul_add(i_acc, q_acc * q_acc)
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    #[test]
    fn get_bin_matches_slowrx_truncation() {
        let cases: &[(f64, usize)] = &[
            (400.0, 9),
            (800.0, 18),
            (1190.0, 27),
            (1200.0, 27),
            (1500.0, 34),
            (1900.0, 44),
            (2300.0, 53),
            (2700.0, 62),
            (3400.0, 78),
        ];
        for &(hz, expected) in cases {
            let bin_ours = get_bin(hz, 256, 11025);
            let bin_slowrx = get_bin(hz, 1024, 44100);
            assert_eq!(
                bin_ours, expected,
                "get_bin({hz}, 256, 11025) = {bin_ours}, expected {expected}"
            );
            assert_eq!(
                bin_slowrx, expected,
                "get_bin({hz}, 1024, 44100) = {bin_slowrx}, expected {expected}"
            );
        }
    }

    #[test]
    fn build_hann_zero_and_one_length_safe() {
        assert!(build_hann(0).is_empty());
        let one = build_hann(1);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0], 0.0);
    }

    #[test]
    fn build_hann_endpoints_are_zero_and_middle_is_one() {
        let h = build_hann(256);
        assert!(h[0].abs() < 1e-6);
        assert!(h[h.len() - 1].abs() < 1e-6);
        let mid = h.len() / 2;
        assert!((h[mid] - 1.0).abs() < 1e-2, "middle ≈ 1, got {}", h[mid]);
    }

    #[test]
    fn goertzel_empty_input_returns_zero_power() {
        assert_eq!(goertzel_power(&[], 1900.0), 0.0);
    }

    /// The trap, pinned in the two functions' own numbers.
    ///
    /// Over a 36-sample window — a Martin sync pulse interior — a 2 Hz
    /// scan across ±250 Hz gets **three** distinct answers out of
    /// `goertzel_power` and 251 out of `tone_power`. The scan that reads
    /// a radio's mistuning off its sync pulses ran on the former, and so
    /// reported the low edge of a 300 Hz plateau instead of the tone.
    #[test]
    fn tone_power_is_not_snapped_to_a_bin_the_way_goertzel_is() {
        let sr = f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ);
        let window: Vec<f32> = (0..36)
            .map(|i| (2.0 * std::f64::consts::PI * 1200.0 * f64::from(i) / sr).sin() as f32)
            .collect();

        let scan: Vec<f64> = (0..=250)
            .map(|i| 1200.0 - 250.0 + 2.0 * f64::from(i))
            .collect();
        let distinct = |f: fn(&[f32], f64) -> f64| {
            let mut v: Vec<u64> = scan.iter().map(|&hz| f(&window, hz).to_bits()).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        assert!(
            distinct(goertzel_power) <= 4,
            "goertzel_power should snap to a handful of bins here, got {}",
            distinct(goertzel_power)
        );
        assert!(
            distinct(tone_power) > 200,
            "tone_power must evaluate the frequency it was asked for, got {} distinct",
            distinct(tone_power)
        );

        // And the peak lands on the tone rather than on a plateau edge.
        let peak = |f: fn(&[f32], f64) -> f64| {
            scan.iter()
                .copied()
                .fold((f64::NEG_INFINITY, 0.0), |(bp, bh), hz| {
                    let p = f(&window, hz);
                    if p > bp {
                        (p, hz)
                    } else {
                        (bp, bh)
                    }
                })
                .1
        };
        assert!(
            (peak(tone_power) - 1200.0).abs() <= 10.0,
            "tone_power peak {} Hz, expected ≈1200",
            peak(tone_power)
        );
        assert!(
            (peak(goertzel_power) - 1200.0).abs() > 100.0,
            "the bug being guarded against: goertzel_power peaked at {} Hz",
            peak(goertzel_power)
        );
    }

    #[test]
    fn tone_power_matches_goertzel_on_a_bin_centre() {
        let sr = f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ);
        // 441 samples at 11025 Hz puts 1200 Hz within a fraction of a bin.
        let window: Vec<f32> = (0..441)
            .map(|i| (2.0 * std::f64::consts::PI * 1200.0 * f64::from(i) / sr).sin() as f32)
            .collect();
        let g = goertzel_power(&window, 1200.0);
        let t = tone_power(&window, 1200.0);
        assert!(
            (g - t).abs() / g.max(t) < 0.05,
            "on a long window the two must agree: goertzel {g}, tone {t}"
        );
    }

    #[test]
    fn tone_power_empty_input_returns_zero() {
        assert_eq!(tone_power(&[], 1900.0), 0.0);
    }

    #[test]
    fn goertzel_handcomputed_quarter_cycle() {
        let samples = [1.0_f32, 0.0, -1.0, 0.0];
        let target = f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ) / 4.0;
        let p = goertzel_power(&samples, target);
        assert!((p - 4.0).abs() < 1e-9, "expected 4.0, got {p}");
    }
}
