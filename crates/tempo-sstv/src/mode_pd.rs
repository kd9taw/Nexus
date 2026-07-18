//! PD-family mode decoder.
//!
//! PD modes encode each radio "frame" as four channels — Y(odd line),
//! Cr (shared), Cb (shared), Y(even line) — producing two image rows
//! per radio frame with chroma subsampling between them.
//!
//! Translated from slowrx's `video.c` (Oona Räisänen, ISC License).
//! Channel layout: video.c lines 81-93. YUV→RGB matrix: video.c lines
//! 446-451. See `NOTICE.md` for full attribution.

use crate::demod::ChannelDemod;

/// Decode one PD radio frame (`Y(odd)`/`Cr`/`Cb`/`Y(even)`) into two image
/// rows of `image`. Translated from slowrx `video.c:259-486`.
///
/// Closes audit issues:
/// - **#24** time-base alignment: every pixel uses slowrx's exact
///   `Skip + round(rate * (chan_start_sec + pixel_secs * (x + 0.5)))`
///   single-round formula (`video.c:140-142`); `pair_seconds` is folded
///   in here, NOT pre-rounded by the caller, so per-pair rounding
///   error never accumulates.
/// - **#23** FFT-every-N + `StoredLum`: one FFT per working-rate sample
///   produces the latest `Freq`, which fills `StoredLum` at every
///   sample; pixel times read out via `StoredLum[pixel_time]`
///   (`video.c:350-406`). See
///   [`crate::demod::decode_one_channel_into`] for the per-channel
///   inner loop.
/// - **#18** SNR estimator: [`crate::snr::SnrEstimator`] is recomputed
///   every [`crate::demod::SNR_REESTIMATE_STRIDE`] samples
///   (`video.c:302-343`).
///
/// **Deviations from slowrx (deliberate):**
/// - **#44 lifted with hysteresis (0.3.2)**: per-pixel Hann window
///   length is SNR-adaptive (slowrx `video.c:354-367`) plus a 1 dB
///   hysteresis band at each threshold to prevent flip-flop on real-
///   radio SNR fluctuations near boundary values. See
///   [`crate::demod::window_idx_for_snr_with_hysteresis`] and the
///   `SNR hysteresis on adaptive Hann window selection` entry in
///   `docs/intentional-deviations.md`.
///
/// `skip_samples` is the absolute sample index inside `audio` where
/// pair zero's sync pulse begins; `pair_seconds` is `pair_index *
/// line_seconds` (un-rounded); `rate_hz` is the slant-corrected rate
/// from [`crate::sync::find_sync`].
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_arguments
)]
pub(crate) fn decode_pd_line_pair(
    spec: crate::modespec::ModeSpec,
    pair_index: u32,
    audio: &[f32],
    skip_samples: i64,
    pair_seconds: f64,
    rate_hz: f64,
    image: &mut crate::image::SstvImage,
    demod: &mut ChannelDemod,
    snr_est: &mut crate::snr::SnrEstimator,
    hedr_shift_hz: f64,
) {
    let sync_secs = spec.sync_seconds;
    let porch_secs = spec.porch_seconds;
    let pixel_secs = spec.pixel_seconds;
    let septr_secs = spec.septr_seconds;
    let width = spec.line_pixels;

    // PD channel time offsets (seconds from start of line pair):
    // Y(odd) → Cr → Cb → Y(even). Mirrors slowrx video.c:88-92:
    //   ChanStart[n+1] = ChanStart[n] + ChanLen[n] + SeptrTime
    // where ChanLen[n] = PixelTime * ImgWidth.
    // SeptrTime = 0 for the entire PD family (PD120/PD180/PD240, modespec.c),
    // so septr_secs is a no-op for current modes — but having it here
    // prevents a silent break when non-PD modes (Robot, Scottie, Martin —
    // all with non-zero SeptrTime) are added in V2.
    let chan_len = f64::from(width) * pixel_secs;
    let chan_starts_sec = [
        sync_secs + porch_secs,                                     // Y(odd): 0 septr
        sync_secs + porch_secs + chan_len + septr_secs,             // Cr:     1 septr
        sync_secs + porch_secs + 2.0 * chan_len + 2.0 * septr_secs, // Cb:     2 septr
        sync_secs + porch_secs + 3.0 * chan_len + 3.0 * septr_secs, // Y(even):3 septr
    ];

    let row0 = pair_index * 2;
    let row1 = row0 + 1;
    let width_us = width as usize;

    let mut y_odd = vec![0_u8; width_us];
    let mut cr = vec![0_u8; width_us];
    let mut cb = vec![0_u8; width_us];
    let mut y_even = vec![0_u8; width_us];

    let ctx = crate::demod::ChannelDecodeCtx {
        audio,
        skip_samples,
        rate_hz,
        hedr_shift_hz,
        spec,
    };

    let buffers: [&mut [u8]; 4] = [&mut y_odd, &mut cr, &mut cb, &mut y_even];
    for (chan_idx, buf) in buffers.into_iter().enumerate() {
        crate::demod::decode_one_channel_into(
            buf,
            chan_starts_sec[chan_idx],
            pair_seconds,
            &ctx,
            &mut crate::demod::DemodState { demod, snr_est },
        );
    }

    for x in 0..width_us {
        let rgb_odd = crate::demod::ycbcr_to_rgb(y_odd[x], cr[x], cb[x]);
        let rgb_even = crate::demod::ycbcr_to_rgb(y_even[x], cr[x], cb[x]);
        image.put_pixel(x as u32, row0, rgb_odd);
        image.put_pixel(x as u32, row1, rgb_even);
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    /// Verify that with `septr_seconds = 0` the `chan_starts_sec` formula
    /// gives the same values as the pre-#25 direct formula (numeric equivalence).
    /// This confirms the field is a V2 expansion that is a no-op for PD modes.
    #[test]
    fn chan_starts_sec_septr_zero_is_numerically_equivalent_to_old_formula() {
        for spec in [
            crate::modespec::for_mode(crate::modespec::SstvMode::Pd120),
            crate::modespec::for_mode(crate::modespec::SstvMode::Pd180),
            crate::modespec::for_mode(crate::modespec::SstvMode::Pd240),
        ] {
            let sync = spec.sync_seconds;
            let porch = spec.porch_seconds;
            let px = spec.pixel_seconds;
            let septr = spec.septr_seconds;
            let w = f64::from(spec.line_pixels);
            let chan_len = w * px;

            // New formula (with septr_seconds term from slowrx video.c:88-92)
            let new_starts = [
                sync + porch,
                sync + porch + chan_len + septr,
                sync + porch + 2.0 * chan_len + 2.0 * septr,
                sync + porch + 3.0 * chan_len + 3.0 * septr,
            ];
            // Old formula (pre-#25, septr omitted)
            let old_starts = [
                sync + porch,
                sync + porch + w * px,
                sync + porch + 2.0 * w * px,
                sync + porch + 3.0 * w * px,
            ];
            for (n, (n_val, o_val)) in new_starts.iter().zip(old_starts.iter()).enumerate() {
                assert!(
                    (n_val - o_val).abs() < 1e-12,
                    "mode {:?} chan {} new={n_val} old={o_val}",
                    spec.mode,
                    n
                );
            }
        }
    }
}
