//! Continuous-phase FM tone generator shared by the encoder core.
//!
//! Promoted from the pre-#86 `test_tone.rs` (which consolidated the four
//! copies in `pd_test_encoder.rs`, `robot_test_encoder.rs`,
//! `scottie_test_encoder.rs`, and `vis.rs::tests`). Now always compiled:
//! the production SSTV transmitter (`crate::encode`) synthesizes directly
//! through this writer, and the synthetic round-trip encoders reuse it at
//! the working rate. The SSTV-specific frequency constants (`SYNC_HZ` =
//! 1200 Hz, the 1500 Hz `PORCH_HZ` / `SEPTR_HZ` / `BLACK_HZ`, the 2300 Hz
//! `WHITE_HZ`) live here too, as does the `lum_to_freq(lum) → Hz` mapping.
//!
//! [`ToneWriter`] owns the output `Vec<f32>` and a running `phase`
//! accumulator. Two emission forms share the same `phase`:
//! [`ToneWriter::fill_to`] (cumulative absolute sample target — used by
//! the scanline emitters; prevents per-tone rounding drift across a
//! multi-channel line) and [`ToneWriter::fill_secs`] (per-tone wall-clock
//! duration — used by VIS header bursts where each tone has a fixed
//! duration).
//!
//! **Sample-rate parametric.** The writer carries its own `sample_rate_hz`
//! so continuous-phase FM is rate-agnostic: the synthetic test paths
//! construct at [`WORKING_SAMPLE_RATE_HZ`] (11 025 Hz) via [`ToneWriter::new`],
//! production TX at 12 000 Hz (`ft1::SAMPLE_RATE`) via
//! [`ToneWriter::with_pre_silence_samples_at`]. Direct synthesis at the
//! caller's rate avoids any 11 025 → 12 000 resample image.

use std::f64::consts::PI;

use crate::resample::WORKING_SAMPLE_RATE_HZ;

pub(crate) const SYNC_HZ: f64 = 1200.0;
pub(crate) const PORCH_HZ: f64 = 1500.0;
/// Same value as `PORCH_HZ` / `BLACK_HZ` (1500 Hz), named for SSTV-spec clarity.
pub(crate) const SEPTR_HZ: f64 = 1500.0;
pub(crate) const BLACK_HZ: f64 = 1500.0;
pub(crate) const WHITE_HZ: f64 = 2300.0;

/// Map an 8-bit luminance value to its FM frequency in Hz.
/// Linear interpolation between [`BLACK_HZ`] (lum=0) and [`WHITE_HZ`] (lum=255).
#[must_use]
pub(crate) fn lum_to_freq(lum: u8) -> f64 {
    BLACK_HZ + (WHITE_HZ - BLACK_HZ) * f64::from(lum) / 255.0
}

/// Continuous-phase FM tone writer. Owns the output `Vec<f32>` and a
/// running `phase` accumulator so consecutive tones produce no audible
/// discontinuity at boundaries.
pub(crate) struct ToneWriter {
    out: Vec<f32>,
    phase: f64,
    sample_rate_hz: u32,
}

impl ToneWriter {
    /// Construct at the crate working rate ([`WORKING_SAMPLE_RATE_HZ`],
    /// 11 025 Hz). Back-compat entry point for the synthetic round-trip
    /// encoders; production TX uses [`Self::with_pre_silence_samples_at`].
    pub fn new() -> Self {
        Self::with_pre_silence_samples_at(0, WORKING_SAMPLE_RATE_HZ)
    }

    /// Construct at [`WORKING_SAMPLE_RATE_HZ`] with `n` zero samples already
    /// in `out` (VIS pre-silence for the synthetic bursts). Phase starts at 0.
    /// Test-only: production TX passes its rate via
    /// [`Self::with_pre_silence_samples_at`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_pre_silence_samples(n: usize) -> Self {
        Self::with_pre_silence_samples_at(n, WORKING_SAMPLE_RATE_HZ)
    }

    /// Construct at an arbitrary output rate with `n` leading zero samples.
    /// Production TX passes `sample_rate_hz = 12_000` (`ft1::SAMPLE_RATE`).
    /// Phase starts at 0.
    pub fn with_pre_silence_samples_at(n: usize, sample_rate_hz: u32) -> Self {
        Self {
            out: vec![0.0; n],
            phase: 0.0,
            sample_rate_hz,
        }
    }

    /// Output sample rate this writer synthesizes at.
    #[must_use]
    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    /// Emit samples up to absolute output index `target_n` (exclusive) at
    /// `freq_hz`. Cumulative-target form — call repeatedly with increasing
    /// `target_n` across a multi-channel line; per-pixel rounding error
    /// never compounds.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub fn fill_to(&mut self, freq_hz: f64, target_n: usize) {
        let dphi = 2.0 * PI * freq_hz / f64::from(self.sample_rate_hz);
        while self.out.len() < target_n {
            self.out.push(self.phase.sin() as f32);
            self.phase += dphi;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
    }

    /// Emit `secs` seconds at `freq_hz`. Per-tone-duration form. Used by
    /// VIS header bursts where each tone has a fixed wall-clock duration.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn fill_secs(&mut self, freq_hz: f64, secs: f64) {
        let n = (secs * f64::from(self.sample_rate_hz)).round() as usize;
        let target = self.out.len() + n;
        self.fill_to(freq_hz, target);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.out.len()
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<f32> {
        self.out
    }
}

impl Default for ToneWriter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    #[test]
    fn lum_to_freq_endpoints_match_black_and_white() {
        assert_eq!(lum_to_freq(0), BLACK_HZ);
        assert_eq!(lum_to_freq(255), WHITE_HZ);
        let mid = lum_to_freq(128);
        let target = f64::midpoint(BLACK_HZ, WHITE_HZ);
        assert!((mid - target).abs() < 5.0, "mid={mid} ≉ {target}");
    }

    #[test]
    fn fill_to_advances_to_exact_target() {
        let mut tone = ToneWriter::new();
        tone.fill_to(1200.0, 100);
        assert_eq!(tone.len(), 100);
        tone.fill_to(1500.0, 250);
        assert_eq!(tone.len(), 250);
    }

    #[test]
    fn fill_to_and_fill_secs_are_equivalent_for_matching_durations() {
        let secs = 100.0 / f64::from(WORKING_SAMPLE_RATE_HZ);
        let mut a = ToneWriter::new();
        a.fill_secs(1200.0, secs);
        let mut b = ToneWriter::new();
        b.fill_to(1200.0, 100);
        let av = a.into_vec();
        let bv = b.into_vec();
        assert_eq!(av.len(), bv.len());
        for (i, (&x, &y)) in av.iter().zip(bv.iter()).enumerate() {
            assert!((x - y).abs() < 1e-6, "sample {i}: {x} vs {y}");
        }
    }

    #[test]
    fn phase_is_continuous_across_tone_boundaries() {
        let mut tone = ToneWriter::new();
        tone.fill_to(1200.0, 100);
        tone.fill_to(1500.0, 200);
        let v = tone.into_vec();
        for w in v.windows(2) {
            let delta = (w[1] - w[0]).abs();
            assert!(
                delta < 1.0,
                "sample-to-sample delta {delta} > 1.0 — phase discontinuity"
            );
        }
    }

    #[test]
    fn sample_rate_carries_through_fill_secs() {
        // A writer at 12 kHz produces 12 000 samples for a 1 s tone.
        let mut tone = ToneWriter::with_pre_silence_samples_at(0, 12_000);
        assert_eq!(tone.sample_rate_hz(), 12_000);
        tone.fill_secs(1500.0, 1.0);
        assert_eq!(tone.len(), 12_000);
    }
}
