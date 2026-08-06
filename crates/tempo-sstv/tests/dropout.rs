//! A picture must survive losing the signal in the middle of it.
//!
//! **Origin.** On 2026-08-05/06 the decoder gained a second way out of
//! its `Decoding` state: an end-of-transmission trigger that fired once
//! no sync pulse had been heard for three line periods. It made a
//! truncated picture emit sooner, and it broke normal reception, because
//! *a carrier that stopped and a carrier momentarily lost are the same
//! observation*. Measured on the commit that shipped it, against the
//! commit before (`78ac6092`):
//!
//! | input | before | with the trigger |
//! |---|---|---|
//! | Robot 36, 0.6 s fade at 50 % | one image, 240 rows | one image, **120 rows** |
//! | Robot 72, 0.95 s fade at 50 % | one image, 240 rows | **two images**, 120 + 111 rows |
//! | Scottie 1, 4 s QSB null at 40 % | one image, 256 rows | **two images**, 102 + 145 rows |
//! | PD-120, 6 s QSB null at 40 % | one image, 496 rows | **two images**, 200 + 274 rows |
//!
//! Three line periods is 0.45 s on Robot 36 and 0.9 s on Robot 72, so on
//! those modes a *sub-second* dropout was enough. The trigger was
//! reverted on 2026-08-06 and this file is what stops it, or anything
//! shaped like it, coming back. A split picture is worse than a late one.
//!
//! **What is asserted.** For a COMPLETE, VIS-anchored transmission with
//! the signal interrupted mid-picture, the decoder must emit exactly
//! **one** `ImageComplete`, `partial: false`, carrying the whole frame —
//! the rows over the interruption are demodulated from whatever was
//! there, which is the honest outcome and the one every previous release
//! produced. The interruptions cover what a receiver actually delivers:
//! band noise (a fade), digital silence (a hard mute or a squelched
//! receiver), a multi-second QSB null, and another station's SSB/CW
//! landing on the frequency.
//!
//! The last test states the other half — a transmission that genuinely
//! stops early still decodes, as a whole frame with a noise tail — so
//! that "we lose nothing by having no end-of-transmission trigger" is a
//! fact in the suite rather than a claim in a commit message.

#![cfg(feature = "test-support")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use tempo_sstv::{SstvDecoder, SstvEvent, SstvImage, SstvMode, WORKING_SAMPLE_RATE_HZ};

/// Audio arrives the way live capture delivers it, not as one buffer.
const CHUNK: usize = 1024;

fn work_rate() -> f64 {
    f64::from(WORKING_SAMPLE_RATE_HZ)
}

fn stream(audio: &[f32]) -> Vec<SstvEvent> {
    let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
    let mut events = Vec::new();
    for chunk in audio.chunks(CHUNK) {
        events.extend(d.process(chunk));
    }
    events
}

/// Deterministic LCG, same constants as `tests/no_vis.rs`.
struct Lcg(u32);
impl Lcg {
    fn next_unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 as f32 / u32::MAX as f32) - 0.5
    }
}

/// SSB-passband-shaped noise at a stated RMS — what a receiver hands you
/// when the signal goes away. A radio never delivers bit-exact zeroes.
fn band_noise(n: usize, rms: f32, seed: u32) -> Vec<f32> {
    let mut rng = Lcg(seed);
    let sr = work_rate();
    let lowpass = (-2.0 * std::f64::consts::PI * 2700.0 / sr).exp() as f32;
    let highpass = (-2.0 * std::f64::consts::PI * 300.0 / sr).exp() as f32;
    let (mut lp, mut hp) = (0.0_f32, 0.0_f32);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let w = rng.next_unit();
        lp = lowpass * lp + (1.0 - lowpass) * w;
        hp = highpass * hp + (1.0 - highpass) * lp;
        out.push(lp - hp);
    }
    let cur = (out
        .iter()
        .map(|s| f64::from(*s) * f64::from(*s))
        .sum::<f64>()
        / out.len() as f64)
        .sqrt() as f32;
    let g = if cur > 0.0 { rms / cur } else { 0.0 };
    out.iter().map(|s| s * g).collect()
}

/// Another station arriving on the frequency: a keyed 800 Hz CW note over
/// speech-band noise. Nothing here is a sync pulse or a picture.
fn ssb_cw(n: usize) -> Vec<f32> {
    let sr = work_rate();
    let mut rng = Lcg(0x51D3);
    (0..n)
        .map(|i| {
            let t = (i as f64) / sr;
            let keyed = (t * 8.0).floor() as i64 % 2 == 0;
            let cw = if keyed {
                0.35 * (2.0 * std::f64::consts::PI * 800.0 * t).sin()
            } else {
                0.0
            };
            (cw + 0.15 * f64::from(rng.next_unit())) as f32
        })
        .collect()
}

fn rgb_source(mode: SstvMode) -> Vec<[u8; 3]> {
    let spec = tempo_sstv::for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let mut rgb = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = ((f64::from(x)) / (f64::from(w)) * 255.0) as u8;
            let g = if y % 8 < 4 { 200 } else { 56 };
            let b = if (y + 2) % 8 < 4 { 200 } else { 56 };
            rgb.push([r, g, b]);
        }
    }
    rgb
}

fn ycrcb_source(mode: SstvMode) -> Vec<[u8; 3]> {
    let spec = tempo_sstv::for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let mut v = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let lum = ((f64::from(x)) / (f64::from(w)) * 255.0) as u8;
            let cr = if y % 4 < 2 { 200 } else { 56 };
            let cb = if (y / 2) % 2 == 0 { 200 } else { 56 };
            v.push([lum, cr, cb]);
        }
    }
    v
}

/// The mode's scanline audio, from the crate's own synthetic modulators —
/// the same ones `tests/roundtrip.rs` validates the decoder against.
fn image_audio(mode: SstvMode) -> Vec<f32> {
    match tempo_sstv::for_mode(mode).channel_layout {
        tempo_sstv::ChannelLayout::PdYcbcr => {
            tempo_sstv::__test_support::mode_pd::encode_pd(mode, &ycrcb_source(mode))
        }
        tempo_sstv::ChannelLayout::RobotYuv => {
            tempo_sstv::__test_support::mode_robot::encode_robot(mode, &ycrcb_source(mode))
        }
        _ => tempo_sstv::__test_support::mode_scottie::encode_scottie(mode, &rgb_source(mode)),
    }
}

fn vis_code(mode: SstvMode) -> u8 {
    (0_u8..=0x7F)
        .find(|c| tempo_sstv::lookup_vis(*c).map(|s| s.mode) == Some(mode))
        .unwrap_or_else(|| panic!("no VIS code for {mode:?}"))
}

fn painted_rows(img: &SstvImage) -> usize {
    let w = img.width as usize;
    (0..img.height)
        .filter(|&y| {
            img.pixels[(y as usize) * w..(y as usize + 1) * w]
                .iter()
                .any(|p| *p != [0, 0, 0])
        })
        .count()
}

/// The one image the decoder must have emitted, asserted whole.
fn the_only_whole_image<'a>(label: &str, events: &'a [SstvEvent]) -> &'a SstvImage {
    let images: Vec<&SstvEvent> = events
        .iter()
        .filter(|e| matches!(e, SstvEvent::ImageComplete { .. }))
        .collect();
    assert_eq!(
        images.len(),
        1,
        "{label}: the picture must arrive as ONE image, got {} — \
         a mid-picture interruption was mistaken for the end of the transmission",
        images.len()
    );
    match images[0] {
        SstvEvent::ImageComplete {
            image,
            partial: false,
        } => image,
        SstvEvent::ImageComplete { partial: true, .. } => {
            panic!("{label}: a complete transmission was reported partial")
        }
        _ => unreachable!(),
    }
}

/// A COMPLETE transmission — VIS header, every scan line — with `secs` of
/// the picture overwritten by `fill` starting `frac` of the way in.
fn transmission_with_interruption(
    mode: SstvMode,
    frac: f64,
    secs: f64,
    fill: &dyn Fn(usize) -> Vec<f32>,
) -> Vec<f32> {
    let mut audio = tempo_sstv::__test_support::vis::synth_vis(vis_code(mode), 0.0);
    let mut img = image_audio(mode);
    let n = (secs * work_rate()) as usize;
    let at = (frac * img.len() as f64) as usize;
    let end = (at + n).min(img.len());
    assert!(end > at, "interruption falls outside the transmission");
    let hole = fill(end - at);
    img[at..end].copy_from_slice(&hole);
    audio.extend_from_slice(&img);
    audio.extend(std::iter::repeat_n(0.0_f32, 16384));
    audio
}

/// A sub-second fade. Robot 36's line period is 150 ms, so 0.6 s is four
/// line periods — the exact input that cost half the frame.
#[test]
fn a_sub_second_fade_does_not_end_the_picture() {
    for (mode, secs) in [
        (SstvMode::Robot36, 0.6_f64),
        (SstvMode::Robot72, 0.95),
        (SstvMode::Martin2, 0.8),
    ] {
        let label = format!("{mode:?} {secs} s fade");
        let audio =
            transmission_with_interruption(mode, 0.5, secs, &|n| band_noise(n, 1e-2, 0xACE1));
        let events = stream(&audio);
        let img = the_only_whole_image(&label, &events);
        assert_eq!(
            painted_rows(img),
            tempo_sstv::for_mode(mode).image_lines as usize,
            "{label}: the whole frame must be present",
        );
    }
}

/// A hard mute — the squelch closing, or a codec dropping to zeroes. The
/// end-of-transmission trigger that was reverted fired on exactly this.
#[test]
fn a_sub_second_silence_does_not_end_the_picture() {
    for (mode, secs) in [(SstvMode::Robot36, 0.6_f64), (SstvMode::Robot72, 0.95)] {
        let label = format!("{mode:?} {secs} s silence");
        let audio = transmission_with_interruption(mode, 0.5, secs, &|n| vec![0.0_f32; n]);
        let events = stream(&audio);
        let img = the_only_whole_image(&label, &events);
        assert_eq!(
            painted_rows(img),
            tempo_sstv::for_mode(mode).image_lines as usize,
            "{label}: the whole frame must be present",
        );
    }
}

/// A deep QSB null lasting several seconds — many line periods on any
/// mode, and the case no gap tolerance can be made safe against, since
/// nothing bounds how long a null lasts.
#[test]
fn a_multi_second_qsb_null_does_not_split_the_picture() {
    for (mode, secs) in [(SstvMode::Scottie1, 4.0_f64), (SstvMode::Pd120, 6.0)] {
        let label = format!("{mode:?} {secs} s QSB null");
        let audio =
            transmission_with_interruption(mode, 0.4, secs, &|n| band_noise(n, 1e-4, 0x1234));
        let events = stream(&audio);
        let img = the_only_whole_image(&label, &events);
        assert_eq!(
            painted_rows(img),
            tempo_sstv::for_mode(mode).image_lines as usize,
            "{label}: the whole frame must be present",
        );
    }
}

/// Another station's SSB/CW over the top of the picture, and the same
/// after it ends. Neither carries sync pulses, so both look exactly like
/// "the transmission stopped" to any detector that listens for them.
#[test]
fn an_ssb_cw_burst_over_the_picture_does_not_end_it() {
    let mode = SstvMode::Robot36;
    let lines = tempo_sstv::for_mode(mode).image_lines as usize;

    let events = stream(&transmission_with_interruption(mode, 0.5, 0.8, &ssb_cw));
    let img = the_only_whole_image("Robot36 mid-picture SSB/CW", &events);
    assert_eq!(painted_rows(img), lines);

    // And a tail after the picture must not produce a second image.
    let mut audio = tempo_sstv::__test_support::vis::synth_vis(vis_code(mode), 0.0);
    audio.extend(image_audio(mode));
    audio.extend(ssb_cw((20.0 * work_rate()) as usize));
    let events = stream(&audio);
    let img = the_only_whole_image("Robot36 trailing SSB/CW", &events);
    assert_eq!(painted_rows(img), lines);
}

/// The other half, stated so it is not merely asserted in prose: a sender
/// who stops early is not left with nothing. The decoder keeps filling
/// its buffer from the band, so the picture arrives as a whole frame with
/// the missing part demodulated from the noise — degraded, contiguous,
/// and correctly placed from row 0. That is what every release before
/// 2026-08-05 did, and it is why removing the end-of-transmission trigger
/// costs the operator no picture.
#[test]
fn a_transmission_cut_short_still_decodes_what_arrived() {
    let mode = SstvMode::Scottie1;
    let spec = tempo_sstv::for_mode(mode);
    let src = rgb_source(mode);

    let mut audio = tempo_sstv::__test_support::vis::synth_vis(vis_code(mode), 0.0);
    let img_audio = image_audio(mode);
    let keep = (0.6 * img_audio.len() as f64) as usize;
    audio.extend_from_slice(&img_audio[..keep]);
    // The sender stops; the band is still there. 70 s covers the rest of
    // the mode's running time with room to spare.
    audio.extend(band_noise((70.0 * work_rate()) as usize, 1e-2, 0xACE1));

    let events = stream(&audio);
    let img = the_only_whole_image("Scottie 1 cut short at 60 %", &events);
    assert_eq!(painted_rows(img), spec.image_lines as usize);

    // The 60 % that was actually sent is intact and starts at row 0.
    let received_rows = (0.55 * f64::from(spec.image_lines)) as usize;
    let to = received_rows * (spec.line_pixels as usize);
    let corr = correlation(&src[..to], &img.pixels[..to]);
    assert!(
        corr > 0.9,
        "the rows that did arrive must be the picture: correlation {corr:.4}"
    );
}

/// Pearson correlation across flattened RGB values.
fn correlation(a: &[[u8; 3]], b: &[[u8; 3]]) -> f64 {
    assert_eq!(a.len(), b.len());
    let n = (a.len() * 3) as f64;
    let (mut sa, mut sb) = (0.0, 0.0);
    for (pa, pb) in a.iter().zip(b) {
        for ch in 0..3 {
            sa += f64::from(pa[ch]);
            sb += f64::from(pb[ch]);
        }
    }
    let (ma, mb) = (sa / n, sb / n);
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (pa, pb) in a.iter().zip(b) {
        for ch in 0..3 {
            let da = f64::from(pa[ch]) - ma;
            let db = f64::from(pb[ch]) - mb;
            cov += da * db;
            va += da * da;
            vb += db * db;
        }
    }
    cov / (va.sqrt() * vb.sqrt())
}
