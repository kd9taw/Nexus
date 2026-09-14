//! #130 — a picture PAINTS while it is still arriving.
//!
//! The corrected decode needs the whole image: `find_sync` fits the slant across every sync
//! pulse, so it runs once, when the buffer is full, and only then emits `LineDecoded`. That left a
//! Scottie 1 black for about two minutes and then complete all at once.
//!
//! `LinePreview` is the provisional half: each line is demodulated at NOMINAL timing (no slant
//! fit) as soon as its audio has arrived, into a SEPARATE preview image with its own demod and SNR
//! state. Three properties are pinned here:
//!   1. rows arrive while the transmission is still coming in, in order, at the mode's geometry;
//!   2. on a clean signal the preview already looks like the picture (measured bound below);
//!   3. the corrected picture is byte-identical to a decode that never previewed — the preview
//!      must not be able to change what is saved.

#![cfg(feature = "test-support")]
#![allow(
    clippy::expect_used,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

use tempo_sstv::{
    encode_image, for_mode, SourceImage, SstvDecoder, SstvEvent, SstvMode, WORKING_SAMPLE_RATE_HZ,
};

fn transmission(mode: SstvMode) -> Vec<f32> {
    let spec = for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let mut rgb = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            rgb.push([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 128u8]);
        }
    }
    let img = SourceImage {
        width: w,
        height: h,
        rgb,
    };
    let mut audio = encode_image(mode, &img, WORKING_SAMPLE_RATE_HZ).expect("encode_image");
    audio.extend(std::iter::repeat_n(
        0.0_f32,
        WORKING_SAMPLE_RATE_HZ as usize * 2,
    ));
    audio
}

fn complete(events: &[SstvEvent]) -> Option<tempo_sstv::SstvImage> {
    events.iter().find_map(|e| match e {
        SstvEvent::ImageComplete { image, .. } => Some(image.clone()),
        _ => None,
    })
}

fn run(mode: SstvMode) {
    let spec = for_mode(mode);
    let audio = transmission(mode);

    // The reference: the same audio in one push.
    let mut one_shot = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
    let reference = complete(&one_shot.process(&audio)).expect("one-shot image");

    // The radio loop's shape: 100 ms at a time.
    let chunk = WORKING_SAMPLE_RATE_HZ as usize / 10;
    let half = audio.len() / 2;
    let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
    let mut early = Vec::new();
    for c in audio[..half].chunks(chunk) {
        early.extend(d.process(c));
    }

    let previews: Vec<(u32, usize)> = early
        .iter()
        .filter_map(|e| match e {
            SstvEvent::LinePreview {
                line_index, pixels, ..
            } => Some((*line_index, pixels.len())),
            _ => None,
        })
        .collect();
    assert!(
        !previews.is_empty(),
        "{mode:?}: no rows painted in the first half of the transmission"
    );
    assert!(
        complete(&early).is_none(),
        "{mode:?}: half a transmission must not complete a picture"
    );
    assert!(
        !early
            .iter()
            .any(|e| matches!(e, SstvEvent::LineDecoded { .. })),
        "{mode:?}: the corrected lines still wait for the whole image"
    );
    for (i, (row, len)) in previews.iter().enumerate() {
        assert!(
            *row < spec.image_lines,
            "{mode:?}: row {row} is off the image"
        );
        assert_eq!(*len, spec.line_pixels as usize, "{mode:?}: row width");
        if i > 0 {
            assert!(
                *row > previews[i - 1].0,
                "{mode:?}: rows paint top to bottom"
            );
        }
    }

    let mut late = Vec::new();
    for c in audio[half..].chunks(chunk) {
        late.extend(d.process(c));
    }

    // What the operator SAW before the corrected picture replaced it: every preview row, in the
    // order it arrived, compared with the corrected picture. A preview that paints garbage is
    // worse than a black canvas, so its likeness is measured, not assumed.
    let w = spec.line_pixels as usize;
    let mut seen = vec![[0u8; 3]; w * spec.image_lines as usize];
    let mut painted = vec![false; spec.image_lines as usize];
    for e in early.iter().chain(late.iter()) {
        if let SstvEvent::LinePreview {
            line_index, pixels, ..
        } = e
        {
            let r = *line_index as usize;
            seen[r * w..(r + 1) * w].copy_from_slice(pixels);
            painted[r] = true;
        }
    }
    let (mut sum, mut n) = (0_u64, 0_u64);
    for (r, _) in painted.iter().enumerate().filter(|(_, p)| **p) {
        for x in 0..w {
            for ch in 0..3 {
                let a = i32::from(seen[r * w + x][ch]);
                let b = i32::from(reference.pixels[r * w + x][ch]);
                sum += u64::from((a - b).unsigned_abs());
                n += 1;
            }
        }
    }
    let rows = painted.iter().filter(|p| **p).count();
    let mean = sum as f64 / n.max(1) as f64;
    // Bounds MEASURED on this clean loopback (2026-09-14): Robot 36 6.14 over 239/240 rows,
    // Scottie 1 3.53 over 255/256, PD120 6.35 over 494/496 — against the < 5.0 the corrected
    // decode meets in tx_loopback.rs. Twice the worst, so a preview that drifts into noise (a
    // wrong line-0 offset, a swapped channel) fails here instead of shipping as a garbled canvas.
    assert!(
        rows + 2 >= spec.image_lines as usize,
        "{mode:?}: only {rows}/{} rows previewed",
        spec.image_lines
    );
    assert!(
        mean < 12.0,
        "{mode:?}: the preview does not look like the picture (mean |diff| {mean:.2})"
    );

    let streamed = complete(&late).expect("the streamed picture completes");
    assert_eq!(
        streamed.pixels, reference.pixels,
        "{mode:?}: previewing changed the corrected picture"
    );
}

#[test]
fn robot36_paints_while_arriving() {
    run(SstvMode::Robot36);
}

#[test]
fn scottie1_paints_while_arriving() {
    run(SstvMode::Scottie1);
}

#[test]
fn pd120_paints_while_arriving() {
    run(SstvMode::Pd120);
}
