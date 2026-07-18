//! Synthetic RGB-sequential encoder for round-trip testing —
//! handles both Scottie (1/2/DX) and Martin (1/2) families.
//!
//! Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
//!
//! **Per-line tone emission order branches on
//! [`crate::modespec::SyncPosition`]:**
//!
//! ```text
//! Scottie (sync_position::Scottie):
//!   [septr 1500 Hz][G pixels 1500-2300 Hz][septr 1500 Hz]
//!   [B pixels 1500-2300 Hz][SYNC 1200 Hz][porch 1500 Hz]
//!   [R pixels 1500-2300 Hz]
//!
//! Martin (sync_position::LineStart):
//!   [SYNC 1200 Hz][porch 1500 Hz][G pixels 1500-2300 Hz]
//!   [septr 1500 Hz][B pixels 1500-2300 Hz][septr 1500 Hz]
//!   [R pixels 1500-2300 Hz]
//! ```
//!
//! Total per line = `LineTime` exactly (defensive pad fills the
//! boundary if float arithmetic rounds short).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use crate::modespec::SstvMode;
use crate::resample::WORKING_SAMPLE_RATE_HZ;
use crate::test_tone::{lum_to_freq, ToneWriter, PORCH_HZ, SEPTR_HZ, SYNC_HZ};

/// Encode an RGB image as continuous-phase FM audio for either
/// Scottie (S1/S2/DX) or Martin (M1/M2). `rgb` is row-major,
/// `line_pixels × image_lines` `[R, G, B]` triples (320×256 for all
/// Scottie modes). Returns f32 PCM at [`WORKING_SAMPLE_RATE_HZ`]
/// (`11_025` Hz).
///
/// The per-line tone emission order branches on `spec.sync_position`:
///
/// - [`crate::modespec::SyncPosition::Scottie`] —
///   `[septr][G][septr][B][SYNC][porch][R]` (sync mid-line).
/// - [`crate::modespec::SyncPosition::LineStart`] —
///   `[SYNC][porch][G][septr][B][septr][R]` (Martin / standard
///   PD-Robot order).
///
/// Panics if `mode` is not one of the five supported variants or if
/// `rgb.len() != line_pixels * image_lines`.
#[must_use]
#[allow(dead_code, clippy::too_many_lines)]
pub(crate) fn encode_scottie(mode: SstvMode, rgb: &[[u8; 3]]) -> Vec<f32> {
    assert!(matches!(
        mode,
        SstvMode::Scottie1
            | SstvMode::Scottie2
            | SstvMode::ScottieDx
            | SstvMode::Martin1
            | SstvMode::Martin2
    ));
    let spec = crate::modespec::for_mode(mode);
    let w = spec.line_pixels;
    let h = spec.image_lines;
    assert_eq!(rgb.len() as u32, w * h);

    let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
    let mut tone = ToneWriter::new();

    let mut t = 0.0_f64;
    let advance = |t: &mut f64, secs: f64| -> usize {
        *t += secs;
        (*t * sr).round() as usize
    };

    for y in 0..h {
        match spec.sync_position {
            crate::modespec::SyncPosition::Scottie => {
                // Septr 1.
                tone.fill_to(SEPTR_HZ, advance(&mut t, spec.septr_seconds));

                // G channel.
                for x in 0..w {
                    let g = rgb[(y * w + x) as usize][1];
                    tone.fill_to(lum_to_freq(g), advance(&mut t, spec.pixel_seconds));
                }

                // Septr 2.
                tone.fill_to(SEPTR_HZ, advance(&mut t, spec.septr_seconds));

                // B channel.
                for x in 0..w {
                    let b = rgb[(y * w + x) as usize][2];
                    tone.fill_to(lum_to_freq(b), advance(&mut t, spec.pixel_seconds));
                }

                // Sync (mid-line, between B and R).
                tone.fill_to(SYNC_HZ, advance(&mut t, spec.sync_seconds));

                // Porch.
                tone.fill_to(PORCH_HZ, advance(&mut t, spec.porch_seconds));

                // R channel.
                for x in 0..w {
                    let r = rgb[(y * w + x) as usize][0];
                    tone.fill_to(lum_to_freq(r), advance(&mut t, spec.pixel_seconds));
                }
            }
            crate::modespec::SyncPosition::LineStart => {
                // Martin layout: sync at line start, then porch, then
                // G/septr/B/septr/R.

                // Sync.
                tone.fill_to(SYNC_HZ, advance(&mut t, spec.sync_seconds));

                // Porch.
                tone.fill_to(PORCH_HZ, advance(&mut t, spec.porch_seconds));

                // G channel.
                for x in 0..w {
                    let g = rgb[(y * w + x) as usize][1];
                    tone.fill_to(lum_to_freq(g), advance(&mut t, spec.pixel_seconds));
                }

                // Septr 1.
                tone.fill_to(SEPTR_HZ, advance(&mut t, spec.septr_seconds));

                // B channel.
                for x in 0..w {
                    let b = rgb[(y * w + x) as usize][2];
                    tone.fill_to(lum_to_freq(b), advance(&mut t, spec.pixel_seconds));
                }

                // Septr 2.
                tone.fill_to(SEPTR_HZ, advance(&mut t, spec.septr_seconds));

                // R channel.
                for x in 0..w {
                    let r = rgb[(y * w + x) as usize][0];
                    tone.fill_to(lum_to_freq(r), advance(&mut t, spec.pixel_seconds));
                }
            }
        }

        // Defensive pad to the line_seconds boundary (existing logic
        // — shape unchanged).
        let line_end_target = f64::from(y + 1) * spec.line_seconds;
        let pad_secs = line_end_target - t;
        if pad_secs > 0.0 {
            tone.fill_to(PORCH_HZ, advance(&mut t, pad_secs));
        }
    }

    tone.into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: `lum_to_freq` endpoint coverage lives in
    // `crate::test_tone::tests::lum_to_freq_endpoints_match_black_and_white`
    // (its canonical home post-#86). Testing it from a consumer is
    // misleading about ownership.

    #[test]
    fn scottie1_encode_total_length() {
        let rgb = vec![[128u8; 3]; 320 * 256];
        let audio = encode_scottie(SstvMode::Scottie1, &rgb);
        let spec = crate::modespec::for_mode(SstvMode::Scottie1);
        let expected_len = (spec.line_seconds
            * f64::from(spec.image_lines)
            * f64::from(WORKING_SAMPLE_RATE_HZ)) as usize;
        // Allow 1-sample rounding slack at end-of-image.
        assert!(audio.len() >= expected_len);
        assert!(audio.len() <= expected_len + 1);
    }
}
