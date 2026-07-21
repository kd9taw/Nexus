//! SSTV transmitter core — turns an operator RGB image into on-air
//! `f32` PCM.
//!
//! Original Nexus code (MIT, matching the rest of this crate). Produces
//! the full over-the-air transmission: pre-silence, the standard
//! calibration/VIS header, the per-mode scanlines (dispatched on
//! [`ChannelLayout`]), and a trailing pad. Synthesis is direct at the
//! caller's sample rate (production TX passes 12 000 Hz = `tempo_fast::SAMPLE_RATE`)
//! — continuous-phase FM is rate-agnostic, so no 11 025 → 12 000 resample
//! is needed.
//!
//! The per-family scanline emitters live in `crate::encode_pd`,
//! `crate::encode_scottie`, and `crate::encode_robot`; the tone writer in
//! `crate::tone`. This module owns the public API, the VIS header framing,
//! and the RGB → `YCrCb` / chroma-prep colour path.

use crate::error::{Error, Result};
use crate::modespec::{for_mode, ChannelLayout, SstvMode};
use crate::tone::ToneWriter;

/// Standard leader-tone duration (each of the two 1900 Hz segments), seconds.
const LEADER_SECS: f64 = 0.300;
/// Calibration break between the two leader segments (1200 Hz), seconds.
const CAL_BREAK_SECS: f64 = 0.010;
/// Duration of each VIS bit (start, 7 data + parity, stop), seconds.
const VIS_BIT_SECS: f64 = 0.030;
/// Leading silence for PTT / relay settle, seconds.
const PRE_SILENCE_SECS: f64 = 0.100;
/// Trailing silence after the last scanline, seconds.
const TRAILING_SILENCE_SECS: f64 = 0.100;

/// Operator image, already sized to the target mode's exact dimensions,
/// row-major RGB.
#[derive(Clone, Debug)]
pub struct SourceImage {
    /// Image width in pixels — must equal the mode's `line_pixels`.
    pub width: u32,
    /// Image height in pixels — must equal the mode's `image_lines`.
    pub height: u32,
    /// Row-major `[R, G, B]` pixels of length `width * height`.
    pub rgb: Vec<[u8; 3]>,
}

/// Exact on-air duration of a transmission (standard header + scanlines),
/// in seconds. Excludes the pre/trailing silence pads. Used by TX gates
/// and progress math.
#[must_use]
pub fn tx_duration_secs(mode: SstvMode) -> f64 {
    header_secs() + scanline_secs(mode)
}

/// Duration of the standard calibration + VIS header, seconds (≈ 0.910 s):
/// two 300 ms leaders split by a 10 ms break, then 10 × 30 ms VIS bits
/// (start + 7 data + parity + stop).
fn header_secs() -> f64 {
    2.0 * LEADER_SECS + CAL_BREAK_SECS + 10.0 * VIS_BIT_SECS
}

/// Duration of the per-mode scanline body, seconds. PD packs two image
/// rows per radio frame; every other family carries one row per line. Each
/// radio line takes `max(channel content, line_seconds)` — every mode's
/// content is padded up to `line_seconds`, except Scottie DX whose channel
/// sum slightly exceeds its published `LineTime` and is emitted un-padded.
fn scanline_secs(mode: SstvMode) -> f64 {
    let spec = for_mode(mode);
    let w = f64::from(spec.line_pixels);
    let content = match spec.channel_layout {
        // sync + porch + 4 channels (Y_odd, Cr, Cb, Y_even).
        ChannelLayout::PdYcbcr => {
            spec.sync_seconds + spec.porch_seconds + 4.0 * w * spec.pixel_seconds
        }
        // Scottie / Martin: 2 septr + sync + porch + 3 channels (G, B, R).
        ChannelLayout::RgbSequential => {
            2.0 * spec.septr_seconds
                + spec.sync_seconds
                + spec.porch_seconds
                + 3.0 * w * spec.pixel_seconds
        }
        ChannelLayout::RobotYuv => match mode {
            // R72: sync + porch + 2 septr + 3 channels (Y, U, V).
            SstvMode::Robot72 => {
                spec.sync_seconds
                    + spec.porch_seconds
                    + 2.0 * spec.septr_seconds
                    + 3.0 * w * spec.pixel_seconds
            }
            // R36/R24: sync + porch + 1 septr + Y(2×) + 1 chroma = 3 pixel widths.
            _ => {
                spec.sync_seconds
                    + spec.porch_seconds
                    + spec.septr_seconds
                    + 3.0 * w * spec.pixel_seconds
            }
        },
    };
    let radio_frames = match spec.channel_layout {
        ChannelLayout::PdYcbcr => f64::from(spec.image_lines) / 2.0,
        ChannelLayout::RobotYuv | ChannelLayout::RgbSequential => f64::from(spec.image_lines),
    };
    radio_frames * content.max(spec.line_seconds)
}

/// Encode an operator image as a complete SSTV transmission: pre-silence,
/// the full standard calibration + VIS header, the per-mode scanlines, and
/// a trailing pad. Returns `f32` PCM at `sample_rate_hz` (production passes
/// 12 000).
///
/// # Errors
/// - [`Error::InvalidSampleRate`] if `sample_rate_hz` is 0.
/// - [`Error::ImageDimensionMismatch`] if `img` is not exactly the mode's
///   `line_pixels × image_lines`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn encode_image(mode: SstvMode, img: &SourceImage, sample_rate_hz: u32) -> Result<Vec<f32>> {
    if sample_rate_hz == 0 {
        return Err(Error::InvalidSampleRate { got: 0 });
    }
    let spec = for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    if img.width != w || img.height != h || img.rgb.len() as u64 != u64::from(w) * u64::from(h) {
        return Err(Error::ImageDimensionMismatch {
            mode: spec.name,
            want_w: w,
            want_h: h,
            got_w: img.width,
            got_h: img.height,
            got_len: img.rgb.len(),
        });
    }

    let pre_n = (PRE_SILENCE_SECS * f64::from(sample_rate_hz)).round() as usize;
    let mut tone = ToneWriter::with_pre_silence_samples_at(pre_n, sample_rate_hz);

    emit_vis_header(&mut tone, spec.vis_code);

    match spec.channel_layout {
        ChannelLayout::PdYcbcr => {
            // PD's own emitter averages chroma across each row pair.
            let ycrcb = rgb_image_to_ycbcr(&img.rgb);
            crate::encode_pd::emit_pd_scanlines(&mut tone, mode, &ycrcb);
        }
        ChannelLayout::RobotYuv => {
            let mut ycrcb = rgb_image_to_ycbcr(&img.rgb);
            // R36/R24 subsample chroma across row pairs (Cr on even lines,
            // Cb on odd, each duplicated to the neighbour row). Average to
            // that pairing so the decoder's duplication is lossless.
            if matches!(mode, SstvMode::Robot24 | SstvMode::Robot36) {
                prep_robot_chroma(&mut ycrcb, w as usize, h as usize);
            }
            crate::encode_robot::emit_robot_scanlines(&mut tone, mode, &ycrcb);
        }
        ChannelLayout::RgbSequential => {
            crate::encode_scottie::emit_scottie_scanlines(&mut tone, mode, &img.rgb);
        }
    }

    let mut out = tone.into_vec();
    let tail_n = (TRAILING_SILENCE_SECS * f64::from(sample_rate_hz)).round() as usize;
    out.extend(std::iter::repeat_n(0.0_f32, tail_n));
    Ok(out)
}

/// Append the full standard calibration + VIS header to `tone`:
/// `1900 Hz × 300 ms → 1200 Hz × 10 ms → 1900 Hz × 300 ms → 1200 Hz × 30 ms
/// (start) → 7 data bits LSB-first × 30 ms (1100 Hz = 1, 1300 Hz = 0) →
/// even-parity bit × 30 ms → 1200 Hz × 30 ms (stop)`.
///
/// This is the on-air preamble `MMSSTV` / `QSSTV` expect. (The crate's
/// synthetic `crate::vis::tests::synth_vis` omits the first leader + break;
/// production TX must emit the full two-segment header for interop.)
pub(crate) fn emit_vis_header(tone: &mut ToneWriter, vis_code: u8) {
    let leader = crate::vis::LEADER_HZ;
    let break_f = leader + crate::vis::BREAK_HZ_OFFSET; // 1200 Hz
    let bit_freq = |bit: u8| -> f64 {
        leader
            + if bit == 1 {
                crate::vis::BIT_ONE_OFFSET // 1100 Hz
            } else {
                crate::vis::BIT_ZERO_OFFSET // 1300 Hz
            }
    };

    // Two 300 ms leader segments split by a 10 ms calibration break.
    tone.fill_secs(leader, LEADER_SECS);
    tone.fill_secs(break_f, CAL_BREAK_SECS);
    tone.fill_secs(leader, LEADER_SECS);

    // VIS framing: start bit, 7 data bits LSB-first, even parity, stop bit.
    tone.fill_secs(break_f, VIS_BIT_SECS); // start (1200 Hz)
    let mut parity = 0u8;
    for b in 0..7 {
        let bit = (vis_code >> b) & 1;
        parity ^= bit;
        tone.fill_secs(bit_freq(bit), VIS_BIT_SECS);
    }
    // R12BW inverts the parity bit (slowrx `vis.c:116`); harmless for every
    // code we actually transmit, kept so the framing matches the detector.
    let parity_bit = if vis_code == crate::vis::R12BW_VIS_CODE {
        parity ^ 1
    } else {
        parity
    };
    tone.fill_secs(bit_freq(parity_bit), VIS_BIT_SECS);
    tone.fill_secs(break_f, VIS_BIT_SECS); // stop (1200 Hz)
}

/// Convert one `[R, G, B]` pixel to a `[Y, Cr, Cb]` triple — the exact
/// inverse of [`crate::demod::ycbcr_to_rgb`]. Full-range `BT.601`:
/// ```text
/// Y  = 0.299 R + 0.587 G + 0.114 B
/// Cr = 0.5 R - 0.418688 G - 0.081312 B + 128
/// Cb = -0.168736 R - 0.331264 G + 0.5 B + 128
/// ```
/// (Triple order is `[Y, Cr, Cb]`, matching the rest of the crate.) Each
/// component is clamped to `[0, 255]` and rounded to nearest.
#[must_use]
pub(crate) fn rgb_to_ycbcr(r: u8, g: u8, b: u8) -> [u8; 3] {
    let rf = f64::from(r);
    let gf = f64::from(g);
    let bf = f64::from(b);
    let y = 0.299 * rf + 0.587 * gf + 0.114 * bf;
    let cr = 0.5 * rf - 0.418_688 * gf - 0.081_312 * bf + 128.0;
    let cb = -0.168_736 * rf - 0.331_264 * gf + 0.5 * bf + 128.0;
    [clamp_round(y), clamp_round(cr), clamp_round(cb)]
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn clamp_round(v: f64) -> u8 {
    v.clamp(0.0, 255.0).round() as u8
}

/// Whole-image RGB → `[Y, Cr, Cb]`.
fn rgb_image_to_ycbcr(rgb: &[[u8; 3]]) -> Vec<[u8; 3]> {
    rgb.iter().map(|&[r, g, b]| rgb_to_ycbcr(r, g, b)).collect()
}

/// Average R36/R24 chroma to the decoder's row-pairing convention so the
/// chroma duplication is lossless: Cr is shared across rows `(2k, 2k+1)`
/// (sent on the even line, duplicated down); Cb across `(2k+1, 2k+2)`
/// (sent on the odd line, duplicated down). Row 0's Cb is never sent —
/// the decoder zero-inits it, a one-row colour cast (slowrx behaviour).
pub(crate) fn prep_robot_chroma(ycrcb: &mut [[u8; 3]], w: usize, h: usize) {
    // Cr over (2k, 2k+1).
    let mut k = 0;
    while 2 * k + 1 < h {
        let (r0, r1) = (2 * k, 2 * k + 1);
        for x in 0..w {
            let cr = u8::midpoint(ycrcb[r0 * w + x][1], ycrcb[r1 * w + x][1]);
            ycrcb[r0 * w + x][1] = cr;
            ycrcb[r1 * w + x][1] = cr;
        }
        k += 1;
    }
    // Cb over (2k+1, 2k+2).
    let mut k = 0;
    while 2 * k + 2 < h {
        let (r0, r1) = (2 * k + 1, 2 * k + 2);
        for x in 0..w {
            let cb = u8::midpoint(ycrcb[r0 * w + x][2], ycrcb[r1 * w + x][2]);
            ycrcb[r0 * w + x][2] = cb;
            ycrcb[r1 * w + x][2] = cb;
        }
        k += 1;
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::expect_used
)]
mod tests {
    use super::*;
    use crate::dsp::goertzel_power;
    use crate::modespec::ALL_SPECS;
    use crate::resample::WORKING_SAMPLE_RATE_HZ;

    /// Every mode's `tx_duration_secs` is header (0.910 s) + scanlines.
    #[test]
    fn header_secs_is_910ms() {
        assert!(
            (header_secs() - 0.910).abs() < 1e-9,
            "got {}",
            header_secs()
        );
    }

    /// The full standard header, synthesized at the working rate so
    /// `goertzel_power` (which is hard-wired to `WORKING_SAMPLE_RATE_HZ`)
    /// can probe each segment: leader / calibration break / second leader /
    /// start bit, plus a couple of data bits confirming LSB-first framing.
    #[test]
    fn header_anatomy_leader_break_start_and_bits() {
        let mut tone = ToneWriter::new(); // 11 025 Hz
        emit_vis_header(&mut tone, 0x5F); // PD120: bits LSB-first = 1,1,1,1,1,0,1
        let audio = tone.into_vec();
        let s = |sec: f64| (sec * f64::from(WORKING_SAMPLE_RATE_HZ)).round() as usize;
        let seg = |a: f64, b: f64| &audio[s(a)..s(b)];

        // First leader (1900 Hz) dominates 1200 across an inset of [0, 0.3).
        let l1 = seg(0.05, 0.25);
        assert!(goertzel_power(l1, 1900.0) > 10.0 * goertzel_power(l1, 1200.0));
        // Calibration break (1200 Hz) across [0.30, 0.31).
        let brk = seg(0.302, 0.308);
        assert!(goertzel_power(brk, 1200.0) > 5.0 * goertzel_power(brk, 1900.0));
        // Second leader (1900 Hz) across [0.31, 0.61).
        let l2 = seg(0.35, 0.55);
        assert!(goertzel_power(l2, 1900.0) > 10.0 * goertzel_power(l2, 1200.0));
        // Start bit (1200 Hz) across [0.61, 0.64).
        let start = seg(0.615, 0.635);
        assert!(goertzel_power(start, 1200.0) > 5.0 * goertzel_power(start, 1300.0));

        // Data bits are LSB-first, 1 = 1100 Hz, 0 = 1300 Hz. Data begins at
        // 0.64 s. For 0x5F: bit0 = 1 (1100 Hz), bit5 = 0 (1300 Hz).
        let bit = |k: usize| {
            seg(
                0.64 + 0.030 * k as f64 + 0.005,
                0.64 + 0.030 * (k as f64 + 1.0) - 0.005,
            )
        };
        let b0 = bit(0);
        assert!(
            goertzel_power(b0, 1100.0) > 3.0 * goertzel_power(b0, 1300.0),
            "bit0 should be 1 (1100 Hz)"
        );
        let b5 = bit(5);
        assert!(
            goertzel_power(b5, 1300.0) > 3.0 * goertzel_power(b5, 1100.0),
            "bit5 should be 0 (1300 Hz)"
        );
    }

    /// `rgb_to_ycbcr` is a tight inverse of the decoder's `ycbcr_to_rgb`.
    /// Worst-case per-channel round-trip error is 3 LSB (near-black blue),
    /// driven by the decoder's approximate (1.40/0.71/0.33/1.78) inverse
    /// coefficients plus 8-bit clamp/round; mean is < 1.
    #[test]
    fn rgb_to_ycbcr_inverts_ycbcr_to_rgb() {
        let mut max_diff = 0i32;
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let [y, cr, cb] = rgb_to_ycbcr(r, g, b);
                    let [r2, g2, b2] = crate::demod::ycbcr_to_rgb(y, cr, cb);
                    for (a, c) in [(r, r2), (g, g2), (b, b2)] {
                        max_diff = max_diff.max((i32::from(a) - i32::from(c)).abs());
                    }
                }
            }
        }
        assert!(
            max_diff <= 3,
            "rgb round-trip max per-channel diff {max_diff} > 3"
        );
    }

    /// `encode_image` length ≈ (pre + header + scanlines + trailing) × rate
    /// for every mode, at 12 kHz. Catches a missing channel / dropped line.
    #[test]
    fn encode_image_length_matches_duration_for_all_modes() {
        let rate = 12_000u32;
        for spec in ALL_SPECS {
            let img = SourceImage {
                width: spec.line_pixels,
                height: spec.image_lines,
                rgb: vec![[64, 128, 192]; (spec.line_pixels * spec.image_lines) as usize],
            };
            let audio = encode_image(spec.mode, &img, rate).expect("encode");
            let expected =
                ((PRE_SILENCE_SECS + tx_duration_secs(spec.mode) + TRAILING_SILENCE_SECS)
                    * f64::from(rate))
                .round() as i64;
            let diff = (audio.len() as i64 - expected).abs();
            assert!(
                diff < 256,
                "{:?}: len {} ≉ expected {expected} (diff {diff})",
                spec.mode,
                audio.len()
            );
        }
    }

    #[test]
    fn encode_image_rejects_wrong_dimensions() {
        let img = SourceImage {
            width: 10,
            height: 10,
            rgb: vec![[0, 0, 0]; 100],
        };
        assert!(matches!(
            encode_image(SstvMode::Pd120, &img, 12_000),
            Err(Error::ImageDimensionMismatch { .. })
        ));
    }

    #[test]
    fn encode_image_rejects_zero_sample_rate() {
        let spec = for_mode(SstvMode::Robot36);
        let img = SourceImage {
            width: spec.line_pixels,
            height: spec.image_lines,
            rgb: vec![[0, 0, 0]; (spec.line_pixels * spec.image_lines) as usize],
        };
        assert!(matches!(
            encode_image(SstvMode::Robot36, &img, 0),
            Err(Error::InvalidSampleRate { got: 0 })
        ));
    }

    #[test]
    fn prep_robot_chroma_pairs_share_chroma() {
        // 4×4 image, varied chroma; after prep, Cr[(2k,2k+1)] and
        // Cb[(2k+1,2k+2)] must match.
        let (w, h) = (4usize, 4usize);
        let mut ycrcb: Vec<[u8; 3]> = (0..(w * h))
            .map(|i| [128, (i * 7) as u8, (i * 13) as u8])
            .collect();
        prep_robot_chroma(&mut ycrcb, w, h);
        for x in 0..w {
            // Cr shared over rows 0,1 and rows 2,3.
            assert_eq!(ycrcb[x][1], ycrcb[w + x][1]);
            assert_eq!(ycrcb[2 * w + x][1], ycrcb[3 * w + x][1]);
            // Cb shared over rows 1,2.
            assert_eq!(ycrcb[w + x][2], ycrcb[2 * w + x][2]);
        }
    }
}
