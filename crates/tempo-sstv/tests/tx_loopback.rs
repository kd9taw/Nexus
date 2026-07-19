//! Production TX ↔ RX self-loopback — the Phase-1 anchor.
//!
//! For every mode: build a known RGB test image at the mode's exact
//! dimensions, run it through the **production** `encode_image` at 12 kHz
//! (`ft1::SAMPLE_RATE`), then feed the very same `Vec<f32>` into the real
//! `SstvDecoder::new(12_000)` — VIS auto-detect + per-line decode +
//! `ImageComplete`. The recovered image must match the source to a mean
//! per-channel diff < 5.0 (the established `tests/roundtrip.rs` bar). This
//! proves the full transmitter — standard two-segment leader/VIS header,
//! RGB → `YCrCb` conversion, R36/R24 chroma-pair prep, and 12 kHz synthesis
//! through the RX resampler — end-to-end with zero radio.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::many_single_char_names,
    clippy::panic
)]

use tempo_sstv::{encode_image, for_mode, SourceImage, SstvDecoder, SstvEvent, SstvMode};

/// Nexus TX/RX audio rate (Hz) — `ft1::SAMPLE_RATE`.
const RATE: u32 = 12_000;

/// A known test image at the mode's exact geometry: horizontal red gradient,
/// smooth vertical green gradient, constant blue. The vertical gradient keeps
/// chroma slowly varying so PD / Robot vertical chroma subsampling stays
/// (near) lossless — the same discipline `roundtrip.rs` uses for its chroma
/// stripes.
fn source_image(w: u32, h: u32) -> SourceImage {
    let mut rgb = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = (x * 255 / (w - 1)) as u8;
            let g = (y * 255 / (h - 1)) as u8;
            let b = 128u8;
            rgb.push([r, g, b]);
        }
    }
    SourceImage {
        width: w,
        height: h,
        rgb,
    }
}

/// Encode `mode` through the production encoder, decode the same samples,
/// and return `(mean_per_channel_diff, max_per_channel_diff)` versus the
/// source RGB. Panics if the decoder does not recover a complete image of
/// the right mode/geometry.
fn run_loopback(mode: SstvMode) -> (f64, u8) {
    let spec = for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let img = source_image(w, h);

    let mut audio = encode_image(mode, &img, RATE).expect("encode_image");
    // Trailing runway so the decoder's find-sync buffer fills and the final
    // line's FFT look-ahead + resampler group delay are covered (mirrors the
    // trailing pad in roundtrip.rs / the 2 s flush in nexus_acceptance.rs).
    audio.extend(std::iter::repeat_n(0.0_f32, RATE as usize));

    let mut dec = SstvDecoder::new(RATE).expect("decoder");
    let events = dec.process(&audio);

    let recovered = events
        .iter()
        .find_map(|e| match e {
            SstvEvent::ImageComplete {
                image,
                partial: false,
            } => Some(image.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "{mode:?}: no ImageComplete; events={:?}",
                event_kinds(&events)
            )
        });

    assert_eq!(recovered.mode, mode, "{mode:?}: wrong decoded mode");
    assert_eq!(
        (recovered.width, recovered.height),
        (w, h),
        "{mode:?}: wrong dims"
    );
    assert_eq!(
        recovered.pixels.len(),
        img.rgb.len(),
        "{mode:?}: pixel count"
    );

    let mut max_diff = 0u8;
    let mut sum: u64 = 0;
    let mut n: u64 = 0;
    for (src, dec) in img.rgb.iter().zip(recovered.pixels.iter()) {
        for ch in 0..3 {
            let d = (i32::from(src[ch]) - i32::from(dec[ch])).unsigned_abs() as u8;
            max_diff = max_diff.max(d);
            sum += u64::from(d);
            n += 1;
        }
    }
    (sum as f64 / n as f64, max_diff)
}

/// Compact event-kind list for panic diagnostics.
fn event_kinds(events: &[SstvEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            SstvEvent::VisDetected { .. } => "VisDetected",
            SstvEvent::UnknownVis { .. } => "UnknownVis",
            SstvEvent::LineDecoded { .. } => "LineDecoded",
            SstvEvent::ImageComplete { .. } => "ImageComplete",
            SstvEvent::FskId { .. } => "FskId",
            // `SstvEvent` is `#[non_exhaustive]`; this test is an external crate.
            _ => "?",
        })
        .collect()
}

/// Established mean-quality bar from `roundtrip.rs` — max-diff is waived per
/// the documented #44 synthetic instant-frequency-step artifact.
const MEAN_BAR: f64 = 5.0;

macro_rules! loopback_test {
    ($name:ident, $mode:expr) => {
        #[test]
        fn $name() {
            let (mean, max) = run_loopback($mode);
            println!("{:?}: mean={mean:.3} max={max}", $mode);
            assert!(
                mean < MEAN_BAR,
                "{:?}: mean per-channel diff {mean:.3} >= {MEAN_BAR} (max={max})",
                $mode
            );
        }
    };
}

loopback_test!(loopback_pd50, SstvMode::Pd50);
loopback_test!(loopback_pd90, SstvMode::Pd90);
loopback_test!(loopback_pd120, SstvMode::Pd120);
loopback_test!(loopback_pd160, SstvMode::Pd160);
loopback_test!(loopback_pd180, SstvMode::Pd180);
loopback_test!(loopback_pd240, SstvMode::Pd240);
loopback_test!(loopback_pd290, SstvMode::Pd290);
loopback_test!(loopback_robot24, SstvMode::Robot24);
loopback_test!(loopback_robot36, SstvMode::Robot36);
loopback_test!(loopback_robot72, SstvMode::Robot72);
loopback_test!(loopback_scottie1, SstvMode::Scottie1);
loopback_test!(loopback_scottie2, SstvMode::Scottie2);
loopback_test!(loopback_scottie_dx, SstvMode::ScottieDx);
loopback_test!(loopback_martin1, SstvMode::Martin1);
loopback_test!(loopback_martin2, SstvMode::Martin2);
