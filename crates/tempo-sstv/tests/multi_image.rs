//! Multi-image streaming — back-to-back SSTV transmissions must all decode
//! within a single `SstvDecoder::process` call, not one-per-call (audit #90:
//! A2 + D4). Counterpart to `tests/roundtrip.rs` (single image) and
//! `tests/unknown_vis.rs` (post-unknown-VIS reseed).

#![cfg(feature = "test-support")]
#![allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use tempo_sstv::{SstvDecoder, SstvEvent, SstvMode, WORKING_SAMPLE_RATE_HZ};

/// PD120 == VIS code 0x5F.
const PD120_CODE: u8 = 0x5F;

/// A small synthetic `YCrCb` image for PD120 (luma gradient + smooth chroma
/// stripes — same shape as `tests/roundtrip.rs`'s `test_image`, so the
/// encoder's adjacent-row chroma averaging has something it can reproduce).
fn pd120_test_image() -> Vec<[u8; 3]> {
    let spec = tempo_sstv::for_mode(SstvMode::Pd120);
    let w = spec.line_pixels;
    let h = spec.image_lines;
    let mut ycrcb = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let lum = ((f64::from(x)) / (f64::from(w)) * 255.0) as u8;
            let cr = if y % 4 < 2 { 200 } else { 56 };
            let cb = if (y / 2) % 2 == 0 { 200 } else { 56 };
            ycrcb.push([lum, cr, cb]);
        }
    }
    ycrcb
}

#[test]
fn decoder_decodes_two_back_to_back_images() {
    let img1 = pd120_test_image();
    let img2 = pd120_test_image();

    // Two complete PD120 transmissions, concatenated, then a pad to absorb
    // the resampler FIR group delay so image 2's last line + VIS 2 produce
    // full analysis windows (2048 matches tests/roundtrip.rs's PD padding).
    let mut audio = tempo_sstv::__test_support::vis::synth_vis(PD120_CODE, 0.0);
    audio.extend(tempo_sstv::__test_support::mode_pd::encode_pd(
        SstvMode::Pd120,
        &img1,
    ));
    audio.extend(tempo_sstv::__test_support::vis::synth_vis(PD120_CODE, 0.0));
    audio.extend(tempo_sstv::__test_support::mode_pd::encode_pd(
        SstvMode::Pd120,
        &img2,
    ));
    audio.extend(std::iter::repeat_n(0.0_f32, 2048));

    let mut decoder = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder construct");
    let events = decoder.process(&audio); // ONE call

    let vis_positions: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(
                e,
                SstvEvent::VisDetected {
                    mode: SstvMode::Pd120,
                    ..
                }
            )
        })
        .map(|(i, _)| i)
        .collect();
    let complete_positions: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, SstvEvent::ImageComplete { partial: false, .. }))
        .map(|(i, _)| i)
        .collect();

    assert_eq!(
        vis_positions.len(),
        2,
        "expected 2 VisDetected{{Pd120}} in one process() call, got {} (event count {})",
        vis_positions.len(),
        events.len()
    );
    assert_eq!(
        complete_positions.len(),
        2,
        "expected 2 ImageComplete{{partial:false}} in one process() call, got {}",
        complete_positions.len()
    );
    // Order: VisDetected1 < ImageComplete1 < VisDetected2 < ImageComplete2.
    assert!(
        vis_positions[0] < complete_positions[0]
            && complete_positions[0] < vis_positions[1]
            && vis_positions[1] < complete_positions[1],
        "events out of order: vis@{vis_positions:?} complete@{complete_positions:?}"
    );
    // Both completed images are PD120 (the carry-forward correctly delivered
    // image 2's VIS + video to the re-armed detector).
    for &i in &complete_positions {
        if let SstvEvent::ImageComplete { image, .. } = &events[i] {
            assert_eq!(image.mode, SstvMode::Pd120, "ImageComplete @{i} mode");
        }
    }
}
