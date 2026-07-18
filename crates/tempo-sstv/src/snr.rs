//! SNR estimation for the PD-family per-pixel demod.
//!
//! Translated from slowrx's `video.c` lines 302-343 (SNR-estimator FFT and
//! bandwidth-corrected SNR formula). ISC License — see `NOTICE.md`.
//!
//! `HannBank` / `HANN_LENS` / `window_idx_for_snr{_with_hysteresis}` moved
//! to `crate::demod` (#85).
//!
//! ## Working-rate scaling
//!
//! slowrx operates at `44_100` Hz with `FFTLen = 1024` and Hann lengths
//! `[48, 64, 96, 128, 256, 512, 1024]`. We operate at
//! [`crate::resample::WORKING_SAMPLE_RATE_HZ`] = `11_025` Hz with
//! `FFT_LEN = 1024` — same FFT length, **4× finer Hz/bin**
//! (`11025/1024 ≈ 10.77` Hz/bin vs slowrx's `44100/1024 ≈ 43.07`
//! Hz/bin). The bump produces two coupled DSP changes:
//!
//! - **Per-pixel demod**: `HANN_LENS` stays at slowrx's lengths divided
//!   by 4 (`[12, 16, 24, 32, 64, 128, 256]`), so the Hann is applied
//!   to the first `HANN_LENS[idx]` samples of the FFT input and the
//!   rest is zero-padded. Time-domain support identical to slowrx C;
//!   only the FFT bin density changes (4× more output bins on the
//!   same windowed signal).
//! - **SNR estimator**: `hann_long = build_hann(FFT_LEN)` scales with
//!   `FFT_LEN`, so the SNR-estimator window grows from 256 samples
//!   (~23 ms = slowrx C) to 1024 samples (~93 ms, 4× longer). This
//!   gives a cleaner SNR estimate and is a desirable side-effect.
//!
//! See `docs/intentional-deviations.md::"FFT frequency resolution
//! exceeds slowrx C by 4×"` for the full rationale and revisit
//! triggers.

use rustfft::{num_complex::Complex, FftPlanner};

use crate::demod::FFT_LEN;

/// Per-decoder SNR estimator. Owns its own FFT plan + scratch buffer
/// (separate from the per-pixel demod's plan so concurrent calls never
/// fight over the same scratch slice). One full-length Hann window is
/// pre-computed and reused on every estimate.
///
/// Translated from `video.c:302-343`. Each `estimate` call mirrors one
/// pass through that block: FFT a 1024-sample window, integrate power
/// over `[1500+hedr, 2300+hedr]` Hz (video band) and over
/// `[400+hedr, 800+hedr] ∪ [2700+hedr, 3400+hedr]` Hz (noise band),
/// apply the bandwidth correction in `video.c:336-338`, and return
/// `10·log10(Psignal / Pnoise)` floored at -20 dB.
pub(crate) struct SnrEstimator {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    hann_long: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl SnrEstimator {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_LEN);
        let scratch_len = fft.get_inplace_scratch_len();
        Self {
            fft,
            hann_long: crate::dsp::build_hann(FFT_LEN),
            fft_buf: vec![Complex { re: 0.0, im: 0.0 }; FFT_LEN],
            scratch: vec![Complex { re: 0.0, im: 0.0 }; scratch_len.max(FFT_LEN)],
        }
    }

    /// Estimate SNR in dB for a window of audio centered on
    /// `center_sample`. `hedr_shift_hz` shifts the video band as in
    /// `video.c:316-326`. Out-of-bounds samples zero-pad.
    ///
    /// Returns SNR in dB; floored at -20 dB to match `video.c:340-341`.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    pub fn estimate(&mut self, audio: &[f32], center_sample: i64, hedr_shift_hz: f64) -> f64 {
        // Fill FFT buffer: zero-pad, then window the live samples.
        let half = (FFT_LEN as i64) / 2;
        for i in 0..FFT_LEN {
            let idx = center_sample - half + i as i64;
            let s = if idx >= 0 && (idx as usize) < audio.len() {
                audio[idx as usize]
            } else {
                0.0
            };
            self.fft_buf[i] = Complex {
                re: s * self.hann_long[i],
                im: 0.0,
            };
        }

        self.fft
            .process_with_scratch(&mut self.fft_buf, &mut self.scratch[..]);

        // Bin helper — uses slowrx-equivalent truncation via `crate::dsp::get_bin`.
        // **Do NOT change to `.round()`**: that shifts 5 of the 8 production
        // frequencies by ±1 bin, changing `VideoPlusNoiseBins`, `NoiseOnlyBins`,
        // and `ReceiverBins` by 1–4 % vs. slowrx's values (see round-2 audit
        // Finding 2/3 for the full bandwidth-correction impact).
        let bin_for = |hz: f64| -> usize {
            crate::dsp::get_bin(hz, FFT_LEN, crate::resample::WORKING_SAMPLE_RATE_HZ)
                .min(FFT_LEN / 2 - 1)
        };

        // Integrate power over the video band (1500-2300 Hz, hedr-shifted).
        let video_lo = bin_for(1500.0 + hedr_shift_hz);
        let video_hi = bin_for(2300.0 + hedr_shift_hz);
        let mut p_video_plus_noise = 0.0_f64;
        for n in video_lo..=video_hi {
            p_video_plus_noise += crate::dsp::power(self.fft_buf[n]);
        }

        // Integrate noise band: 400-800 Hz ∪ 2700-3400 Hz (hedr-shifted).
        let n_lo_a = bin_for(400.0 + hedr_shift_hz);
        let n_hi_a = bin_for(800.0 + hedr_shift_hz);
        let n_lo_b = bin_for(2700.0 + hedr_shift_hz);
        let n_hi_b = bin_for(3400.0 + hedr_shift_hz);
        let mut p_noise_only = 0.0_f64;
        for n in n_lo_a..=n_hi_a {
            p_noise_only += crate::dsp::power(self.fft_buf[n]);
        }
        for n in n_lo_b..=n_hi_b {
            p_noise_only += crate::dsp::power(self.fft_buf[n]);
        }

        // Bandwidth corrections — `video.c:329-334` (computed against an
        // un-shifted reference band, matching slowrx).
        let video_plus_noise_bins = bin_for(2300.0) - bin_for(1500.0) + 1;
        let noise_only_bins =
            (bin_for(800.0) - bin_for(400.0) + 1) + (bin_for(3400.0) - bin_for(2700.0) + 1);
        let receiver_bins = bin_for(3400.0) - bin_for(400.0);

        if noise_only_bins == 0 {
            return -20.0;
        }

        // Eq 15 from slowrx (`video.c:336-338`).
        let p_noise = p_noise_only * (receiver_bins as f64) / (noise_only_bins as f64);
        let p_signal = p_video_plus_noise
            - p_noise_only * (video_plus_noise_bins as f64) / (noise_only_bins as f64);

        if p_noise <= 0.0 || p_signal / p_noise < 0.01 {
            -20.0
        } else {
            10.0 * (p_signal / p_noise).log10()
        }
    }
}

impl Default for SnrEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;
    use crate::resample::WORKING_SAMPLE_RATE_HZ;
    use std::f64::consts::PI;

    fn synth_tone(freq_hz: f64, secs: f64) -> Vec<f32> {
        let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
        let n = (secs * sr).round() as usize;
        (0..n)
            .map(|i| (2.0 * PI * freq_hz * (i as f64) / sr).sin() as f32)
            .collect()
    }

    fn synth_noise(secs: f64, amp: f32, seed: u32) -> Vec<f32> {
        // Deterministic tiny LCG → no rand dep needed for tests.
        let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
        let n = (secs * sr).round() as usize;
        let mut s: u32 = seed.max(1);
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let v = ((s >> 8) & 0xFFFF) as f32 / 32_768.0 - 1.0;
                v * amp
            })
            .collect()
    }

    #[test]
    fn snr_silence_floors_at_minus_twenty() {
        let mut est = SnrEstimator::new();
        let audio = vec![0.0_f32; 1024];
        let snr = est.estimate(&audio, 512, 0.0);
        assert!(
            (snr - -20.0).abs() < 1e-9,
            "silence should floor at -20 dB, got {snr}"
        );
    }

    #[test]
    fn snr_pure_video_tone_is_high() {
        // Pure 1900 Hz tone (mid-video band) → very large p_video_plus_noise,
        // tiny p_noise_only → huge SNR.
        let mut est = SnrEstimator::new();
        let audio = synth_tone(1900.0, 0.100);
        let center = (audio.len() / 2) as i64;
        let snr = est.estimate(&audio, center, 0.0);
        assert!(snr > 25.0, "expected high SNR, got {snr}");
    }

    #[test]
    fn snr_pure_noise_band_is_negative() {
        // Pure 600 Hz (in 400-800 Hz noise band): all power in the noise
        // bins → bandwidth-corrected SNR is very negative (floors at -20).
        let mut est = SnrEstimator::new();
        let audio = synth_tone(600.0, 0.100);
        let center = (audio.len() / 2) as i64;
        let snr = est.estimate(&audio, center, 0.0);
        assert!(snr <= 0.0, "expected ≤ 0 dB SNR, got {snr}");
    }

    #[test]
    fn snr_tone_plus_noise_intermediate() {
        // 1900 Hz tone at amp ~0.3 + white noise at amp ~1.0 →
        // intermediate SNR (signal partially buried).
        let mut est = SnrEstimator::new();
        let mut audio = synth_tone(1900.0, 0.100);
        for (i, n) in synth_noise(0.100, 1.0, 0xCAFE).into_iter().enumerate() {
            if i < audio.len() {
                audio[i] = audio[i] * 0.3 + n;
            }
        }
        let center = (audio.len() / 2) as i64;
        let snr = est.estimate(&audio, center, 0.0);
        assert!(
            (-20.0..30.0).contains(&snr),
            "intermediate SNR expected, got {snr}"
        );
    }

    #[test]
    fn snr_hedr_shift_tracks_band() {
        // 1950 Hz tone with hedr=+50 → tone sits in shifted video band → high SNR.
        // Same tone at hedr=-200 → tone sits OUTSIDE shifted video band but IN
        // shifted noise band (400-800 Hz hedr-shifted to 200-600 ... 2700-3400
        // hedr-shifted to 2500-3200). 1950 is between those, so no power lands
        // in noise; signal still mostly leaks across both bands and is clipped.
        let mut est = SnrEstimator::new();
        let audio = synth_tone(1950.0, 0.100);
        let center = (audio.len() / 2) as i64;
        let snr_aligned = est.estimate(&audio, center, 50.0);
        assert!(snr_aligned > 25.0, "aligned: got {snr_aligned}");
    }

    #[test]
    fn snr_estimator_default_constructs() {
        let _ = SnrEstimator::default();
    }

    /// Verify the SNR-estimator bandwidth correction's bin counts at our
    /// finer-than-slowrx-C frequency resolution. With `FFT_LEN=1024` at
    /// `WORKING_SAMPLE_RATE_HZ=11_025` we get ~10.77 Hz/bin (4× finer
    /// than slowrx C's `1024/44_100`). The integration ranges in Hz are
    /// the same as slowrx C — `[1500, 2300]` for video and
    /// `[400, 800] ∪ [2700, 3400]` for noise — but the `usize` bin
    /// counts are higher in proportion to the resolution.
    ///
    /// At `FFT_LEN=1024, SR=11_025` (slowrx `GetBin` floor truncation):
    ///
    /// | Quantity                  | Expected                          |
    /// |---------------------------|-----------------------------------|
    /// | `video_plus_noise_bins`   | 213 − 139 + 1 = 75                |
    /// | `noise_only_bins`         | (74−37+1) + (315−250+1) = 38+66 = 104 |
    /// | `receiver_bins`           | 315 − 37 = 278                    |
    /// | Pnoise multiplier         | 278/104 ≈ 2.6731                  |
    /// | Psignal subtractor        | 75/104 ≈ 0.7212                   |
    ///
    /// These values differ from what `.round()` would give, so the test
    /// is still a regression guard for the `get_bin` truncation in
    /// `snr.rs::estimate`. It no longer asserts slowrx-C-parity in
    /// `usize` terms (we deliberately exceed slowrx in frequency
    /// resolution; see `docs/intentional-deviations.md`).
    #[test]
    fn snr_bandwidth_correction_bins_at_finer_resolution() {
        let get_bin =
            |hz: f64| crate::dsp::get_bin(hz, FFT_LEN, crate::resample::WORKING_SAMPLE_RATE_HZ);

        let video_lo = get_bin(1500.0);
        let video_hi = get_bin(2300.0);
        let n_lo_a = get_bin(400.0);
        let n_hi_a = get_bin(800.0);
        let n_lo_b = get_bin(2700.0);
        let n_hi_b = get_bin(3400.0);

        let video_plus_noise_bins = video_hi - video_lo + 1;
        let noise_only_bins = (n_hi_a - n_lo_a + 1) + (n_hi_b - n_lo_b + 1);
        let receiver_bins = n_hi_b - n_lo_a;

        assert_eq!(
            video_plus_noise_bins, 75,
            "video+noise bins: got {video_plus_noise_bins}"
        );
        assert_eq!(
            noise_only_bins, 104,
            "noise-only bins: got {noise_only_bins}"
        );
        assert_eq!(receiver_bins, 278, "receiver bins: got {receiver_bins}");

        // Pnoise multiplier ≈ 2.6731 (FFT_LEN=1024) vs 2.5556 (FFT_LEN=256 / slowrx).
        let pnoise_mult = receiver_bins as f64 / noise_only_bins as f64;
        assert!(
            (pnoise_mult - 278.0 / 104.0).abs() < 1e-9,
            "Pnoise mult: {pnoise_mult}"
        );

        // Psignal subtractor ≈ 0.7212 (FFT_LEN=1024) vs 0.7407 (FFT_LEN=256 / slowrx).
        let psignal_sub = video_plus_noise_bins as f64 / noise_only_bins as f64;
        assert!(
            (psignal_sub - 75.0 / 104.0).abs() < 1e-9,
            "Psignal sub: {psignal_sub}"
        );
    }

    /// Verify `sync_target_bin` for 1200 Hz is 27 (slowrx-correct truncation),
    /// not 28 (what `.round()` gives) — round-2 audit Finding 4 / #51.
    #[test]
    fn sync_target_bin_for_1200hz_is_27() {
        // At SYNC_FFT_LEN=256, SR=11025: 1200 * 256 / 11025 = 27.89... → trunc = 27.
        let bin = crate::dsp::get_bin(
            1200.0,
            crate::sync::SYNC_FFT_LEN,
            crate::resample::WORKING_SAMPLE_RATE_HZ,
        );
        assert_eq!(
            bin, 27,
            "sync_target_bin for 1200 Hz should be 27 (trunc), got {bin}"
        );
    }
}
