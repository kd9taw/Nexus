//! Synthetic Robot encoder for round-trip testing. Produces continuous-
//! phase FM audio matching the encoder side of the SSTV protocol.
//!
//! Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
//! Lives in its own file so the production decoder stays under the
//! 500-LOC ceiling.
//!
//! **R36/R24 round-trip constraint:** the source ycrcb buffer must have
//! adjacent rows share chroma (`ycrcb[2k][1] == ycrcb[2k+1][1]` for Cr;
//! `ycrcb[2k+1][2] == ycrcb[2k+2][2]` for Cb), because the decoder
//! duplicates each chroma sample to the neighbor row (slowrx
//! `video.c:424-425`). Source images that violate this constraint cannot
//! round-trip losslessly. R72 has no such constraint (full per-line
//! chroma).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use crate::modespec::SstvMode;
use crate::resample::WORKING_SAMPLE_RATE_HZ;
use crate::test_tone::{lum_to_freq, ToneWriter, PORCH_HZ, SEPTR_HZ, SYNC_HZ};

/// Encode an image as Robot 24 / 36 / 72 audio. `ycrcb` is row-major
/// `[Y, Cr, Cb]` triples of length `width * height`.
///
/// R72: emits Y / U / V sequentially per radio line, with septr between.
/// R36/R24: emits Y / (Cr if y%2==0 else Cb) per radio line, with septr
/// between Y and chroma. The decoder duplicates the chroma sample to
/// the neighbor row, so the source ycrcb buffer must have adjacent rows
/// share chroma (see file-level doc).
#[must_use]
#[allow(dead_code)]
pub(crate) fn encode_robot(mode: SstvMode, ycrcb: &[[u8; 3]]) -> Vec<f32> {
    let spec = crate::modespec::for_mode(mode);
    let w = spec.line_pixels;
    let h = spec.image_lines;
    assert_eq!(ycrcb.len() as u32, w * h);

    let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
    let mut tone = ToneWriter::new();

    let mut t = 0.0_f64;
    let advance = |t: &mut f64, secs: f64| -> usize {
        *t += secs;
        (*t * sr).round() as usize
    };

    match mode {
        SstvMode::Robot72 => encode_r72(&mut tone, &mut t, advance, &spec, ycrcb),
        SstvMode::Robot24 | SstvMode::Robot36 => {
            encode_r36_or_r24(&mut tone, &mut t, advance, &spec, ycrcb);
        }
        // `SstvMode` has variants beyond the three Robot ones (PD, Scottie,
        // Martin) so the wildcard arm is structurally required by the
        // exhaustiveness check — `encode_robot` is only meant to handle the
        // three Robot variants matched above. (audit C17)
        _ => unreachable!("encode_robot called with non-Robot mode {mode:?}"),
    }

    tone.into_vec()
}

fn encode_r72(
    tone: &mut ToneWriter,
    t: &mut f64,
    mut advance: impl FnMut(&mut f64, f64) -> usize,
    spec: &crate::modespec::ModeSpec,
    ycrcb: &[[u8; 3]],
) {
    let w = spec.line_pixels;
    let h = spec.image_lines;
    for y in 0..h {
        // Sync + porch.
        tone.fill_to(SYNC_HZ, advance(t, spec.sync_seconds));
        tone.fill_to(PORCH_HZ, advance(t, spec.porch_seconds));

        // Y channel.
        for x in 0..w {
            let lum = ycrcb[(y * w + x) as usize][0];
            tone.fill_to(lum_to_freq(lum), advance(t, spec.pixel_seconds));
        }

        // Septr between Y and U (Cr).
        tone.fill_to(SEPTR_HZ, advance(t, spec.septr_seconds));

        // U (Cr) channel.
        for x in 0..w {
            let cr = ycrcb[(y * w + x) as usize][1];
            tone.fill_to(lum_to_freq(cr), advance(t, spec.pixel_seconds));
        }

        // Septr between U and V.
        tone.fill_to(SEPTR_HZ, advance(t, spec.septr_seconds));

        // V (Cb) channel.
        for x in 0..w {
            let cb = ycrcb[(y * w + x) as usize][2];
            tone.fill_to(lum_to_freq(cb), advance(t, spec.pixel_seconds));
        }

        // Pad to spec.line_seconds boundary. R72's per-line content
        // (sync + porch + 3 channels + 2 septr) sums to ~297.4 ms but
        // ModeSpec.line_seconds is 300 ms — the decoder advances at
        // 300 ms per line. Without this pad, the synthetic audio drifts
        // from the crate's own timing model and weakens the round-trip
        // as a regression test. Fill the gap with PORCH_HZ (real radio
        // emits a 1500 Hz tone during inter-line idle, not silence).
        let line_end_target = f64::from(y + 1) * spec.line_seconds;
        let pad_secs = line_end_target - *t;
        if pad_secs > 0.0 {
            tone.fill_to(PORCH_HZ, advance(t, pad_secs));
        }
    }
}

/// Robot 36 / Robot 24 channel layout per slowrx `video.c:60-70` (R36/R24 case):
///   `ChanLen[0]` = `pixel_seconds` * width * 2   (Y allocated 2× per-channel time)
///   `ChanLen[1]` = `ChanLen[2]` = `pixel_seconds` * width   (chroma at 1×)
///   `ChanStart[0]` = sync + porch
///   `ChanStart[1]` = `ChanStart[0]` + `ChanLen[0]` + septr
///   `ChanStart[2]` = `ChanStart[1]`   (chroma channel time slot reused — actual
///                                  channel determined by row parity)
///
/// Per radio line N, we emit:
///   - Sync at `SYNC_HZ`
///   - Porch at `PORCH_HZ`
///   - Y for image row N at `pixel_seconds * 2` per pixel (so total Y
///     duration = `pixel_seconds` * 2 * width = `ChanLen[0]`)
///   - Septr at `SEPTR_HZ`
///   - Chroma for image row N at `pixel_seconds` per pixel: Cr if `N%2==0`,
///     Cb if `N%2==1`
fn encode_r36_or_r24(
    tone: &mut ToneWriter,
    t: &mut f64,
    mut advance: impl FnMut(&mut f64, f64) -> usize,
    spec: &crate::modespec::ModeSpec,
    ycrcb: &[[u8; 3]],
) {
    let w = spec.line_pixels;
    let h = spec.image_lines;
    for y in 0..h {
        // Sync + porch.
        tone.fill_to(SYNC_HZ, advance(t, spec.sync_seconds));
        tone.fill_to(PORCH_HZ, advance(t, spec.porch_seconds));

        // Y channel — emit at `pixel_seconds * 2` per source pixel so
        // the total Y allocation equals ChanLen[0] = pixel_seconds *
        // width * 2 per slowrx video.c:60-70 (R36/R24 case).
        for x in 0..w {
            let lum = ycrcb[(y * w + x) as usize][0];
            tone.fill_to(lum_to_freq(lum), advance(t, spec.pixel_seconds * 2.0));
        }

        // Septr between Y and chroma.
        tone.fill_to(SEPTR_HZ, advance(t, spec.septr_seconds));

        // Chroma — Cr (ycrcb index 1) on even rows, Cb (ycrcb index 2)
        // on odd. One sample per `pixel_seconds`.
        let chroma_idx = if y % 2 == 0 { 1_usize } else { 2_usize };
        for x in 0..w {
            let chroma = ycrcb[(y * w + x) as usize][chroma_idx];
            tone.fill_to(lum_to_freq(chroma), advance(t, spec.pixel_seconds));
        }
    }
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

    /// A regression in Robot channel order surfaces here as a pointed
    /// failure instead of a fuzzy roundtrip pixel-diff. Tests Robot72
    /// (simplest of the three Robot variants — 3 channels, no chroma
    /// alternation).
    #[test]
    fn encode_robot72_first_tone_is_sync_hz() {
        let spec = for_mode(SstvMode::Robot72);
        let img = vec![[128_u8, 128, 128]; (spec.line_pixels * spec.image_lines) as usize];
        let audio = encode_robot(SstvMode::Robot72, &img);
        let sync_samples =
            (spec.sync_seconds * f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ)) as usize;
        assert!(audio.len() >= sync_samples, "audio too short");
        let p_sync = crate::dsp::goertzel_power(&audio[..sync_samples], crate::test_tone::SYNC_HZ);
        let p_porch =
            crate::dsp::goertzel_power(&audio[..sync_samples], crate::test_tone::PORCH_HZ);
        assert!(
            p_sync > 10.0 * p_porch,
            "Robot72 line starts with SYNC tone (p_sync={p_sync}, p_porch={p_porch})"
        );
    }

    #[test]
    fn encode_robot72_length_matches_radio_lines() {
        let spec = for_mode(SstvMode::Robot72);
        let img = vec![[0_u8; 3]; (spec.line_pixels * spec.image_lines) as usize];
        let audio = encode_robot(SstvMode::Robot72, &img);
        // Robot: 1 image row per radio line.
        let radio_lines = f64::from(spec.image_lines);
        let expected = (radio_lines
            * spec.line_seconds
            * f64::from(crate::resample::WORKING_SAMPLE_RATE_HZ)) as usize;
        let diff = (audio.len() as i64 - expected as i64).abs();
        assert!(
            diff < 64,
            "Robot72 audio len {} ≉ {expected} (diff {})",
            audio.len(),
            diff
        );
    }
}
