//! PD-family scanline emitter. Produces the continuous-phase FM image
//! audio (no leader/VIS header — that is `crate::encode::emit_vis_header`)
//! for PD50 / PD90 / PD120 / PD160 / PD180 / PD240 / PD290.
//!
//! Promoted from the pre-#86 `pd_test_encoder.rs`: the per-line emission
//! loop is now always-compiled production code driven by
//! [`crate::encode::encode_image`], parametric on the target
//! [`ToneWriter`]'s sample rate. Lives in its own file so the encoder core
//! stays under the crate's 500-LOC-per-file ceiling.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use crate::modespec::SstvMode;
use crate::tone::{lum_to_freq, ToneWriter, PORCH_HZ, SYNC_HZ};

/// Append PD-family scanline audio to `tone`. `ycrcb` is row-major
/// `[Y, Cr, Cb]` triples of length `width * height`. Pairs of rows share
/// averaged chroma, matching how the decoder recovers them. Synthesis is
/// phase-continuous with whatever `tone` already holds (leader + VIS
/// header) and runs at `tone`'s own sample rate.
pub(crate) fn emit_pd_scanlines(tone: &mut ToneWriter, mode: SstvMode, ycrcb: &[[u8; 3]]) {
    assert!(matches!(
        mode,
        SstvMode::Pd50
            | SstvMode::Pd90
            | SstvMode::Pd120
            | SstvMode::Pd160
            | SstvMode::Pd180
            | SstvMode::Pd240
            | SstvMode::Pd290
    ));
    let spec = crate::modespec::for_mode(mode);
    let w = spec.line_pixels;
    let h = spec.image_lines;
    assert_eq!(ycrcb.len() as u32, w * h);
    assert_eq!(h % 2, 0);

    let sr = f64::from(tone.sample_rate_hz());
    // Position-independent: targets are `base + round(t * sr)` so appending
    // after a header keeps phase continuous without re-basing `t`.
    let base = tone.len();

    // Cumulative time tracker (seconds). Targets are computed as
    // `base + (running_t * sr).round()` so per-event rounding doesn't drift.
    let mut t = 0.0_f64;
    let advance = |t: &mut f64, secs: f64| -> usize {
        *t += secs;
        base + (*t * sr).round() as usize
    };

    for y_pair in 0..h / 2 {
        tone.fill_to(SYNC_HZ, advance(&mut t, spec.sync_seconds));
        tone.fill_to(PORCH_HZ, advance(&mut t, spec.porch_seconds));

        // Y(odd row).
        for x in 0..w {
            let lum = ycrcb[((y_pair * 2) * w + x) as usize][0];
            tone.fill_to(lum_to_freq(lum), advance(&mut t, spec.pixel_seconds));
        }
        // Cr (averaged across pair).
        for x in 0..w {
            let cr_a = ycrcb[((y_pair * 2) * w + x) as usize][1];
            let cr_b = ycrcb[((y_pair * 2 + 1) * w + x) as usize][1];
            let cr = u8::midpoint(cr_a, cr_b);
            tone.fill_to(lum_to_freq(cr), advance(&mut t, spec.pixel_seconds));
        }
        // Cb (averaged).
        for x in 0..w {
            let cb_a = ycrcb[((y_pair * 2) * w + x) as usize][2];
            let cb_b = ycrcb[((y_pair * 2 + 1) * w + x) as usize][2];
            let cb = u8::midpoint(cb_a, cb_b);
            tone.fill_to(lum_to_freq(cb), advance(&mut t, spec.pixel_seconds));
        }
        // Y(even row).
        for x in 0..w {
            let lum = ycrcb[((y_pair * 2 + 1) * w + x) as usize][0];
            tone.fill_to(lum_to_freq(lum), advance(&mut t, spec.pixel_seconds));
        }
    }
}

/// Synthetic PD encoder at [`crate::resample::WORKING_SAMPLE_RATE_HZ`]
/// (11 025 Hz) — the round-trip test path. Byte-identical to the pre-#86
/// `pd_test_encoder::encode_pd`; surfaced through
/// `crate::__test_support::mode_pd::encode_pd`.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub(crate) fn encode_pd(mode: SstvMode, ycrcb: &[[u8; 3]]) -> Vec<f32> {
    let mut tone = ToneWriter::new();
    emit_pd_scanlines(&mut tone, mode, ycrcb);
    tone.into_vec()
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;
    use crate::modespec::{for_mode, SstvMode};

    /// A regression in channel order surfaces here as a pointed failure
    /// instead of a fuzzy roundtrip pixel-diff.
    #[test]
    fn encode_pd120_first_tone_is_sync_hz() {
        let spec = for_mode(SstvMode::Pd120);
        let img = vec![[128_u8, 128, 128]; (spec.line_pixels * spec.image_lines) as usize];
        let audio = encode_pd(SstvMode::Pd120, &img);
        let sync_samples =
            (spec.sync_seconds * f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ)) as usize;
        assert!(audio.len() >= sync_samples, "audio too short");
        let p_sync = crate::dsp::goertzel_power(&audio[..sync_samples], crate::tone::SYNC_HZ);
        let p_porch = crate::dsp::goertzel_power(&audio[..sync_samples], crate::tone::PORCH_HZ);
        assert!(
            p_sync > 10.0 * p_porch,
            "PD line starts with SYNC tone (p_sync={p_sync}, p_porch={p_porch})"
        );
    }

    /// Catches structural drift — extra/missing septr, wrong channel count —
    /// without round-tripping.
    #[test]
    fn encode_pd120_length_matches_radio_frames() {
        let spec = for_mode(SstvMode::Pd120);
        let img = vec![[0_u8; 3]; (spec.line_pixels * spec.image_lines) as usize];
        let audio = encode_pd(SstvMode::Pd120, &img);
        // PD packs 2 image rows / radio frame.
        let radio_frames = f64::from(spec.image_lines) / 2.0;
        let expected = (radio_frames
            * spec.line_seconds
            * f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ)) as usize;
        let diff = (audio.len() as i64 - expected as i64).abs();
        assert!(
            diff < 64,
            "PD120 audio len {} ≉ {expected} (diff {})",
            audio.len(),
            diff
        );
    }
}
