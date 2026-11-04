//! A picture stays straight when its audio is not one clean, steady stretch
//! (#422).
//!
//! **Origin.** A field report on Wraase SC-2 180: pictures that came out in
//! horizontal bands, each band either straight or sheared sideways with its
//! colour channels split down the middle, while QSSTV decoded the same signal
//! cleanly beside it. "The slower the mode, the worse it is."
//!
//! **What it was.** The decoder (slowrx's, `sync::find_sync`) fits ONE
//! straight line through every sync pulse of the picture — one rate, one
//! start — and reads every line off that line. Two ordinary things break it:
//!
//! - audio MISSING from the middle of the picture (a receive path that drops
//!   a block under load). There is no straight line through a gap: every line
//!   on the far side of the fit is read from the wrong place, and the slowrx
//!   slip-wrap can then move the whole picture half a line, which is the
//!   split-channel band in the report. A slower mode is more audio, so more
//!   chances to lose some.
//! - a sound-card clock a few hundred ppm off. The fit's half-degree
//!   slant steps leave a residual tilt: on a clean Scottie 1 at 450 ppm a
//!   fifth of the rows tore, and under band noise far more, on more modes.
//!
//! **What is asserted.** Rows are compared against the transmitted picture,
//! which varies only across a row (never down it), so a row read from the
//! wrong line still scores as long as it is read from the right place IN the
//! line: that isolates the sideways shear from lines simply lost in a gap,
//! which no decoder can bring back.

#![cfg(feature = "test-support")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]

use tempo_sstv::{encode_image, SourceImage, SstvDecoder, SstvEvent, SstvImage, SstvMode};

/// The working rate. Encoding at it and handing the decoder a slightly
/// different rate is a sound-card clock that far off.
const BASE_HZ: u32 = 11_025;

/// Deterministic LCG, same constants as `tests/dropout.rs`.
struct Lcg(u32);
impl Lcg {
    fn next_unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 as f32 / u32::MAX as f32) - 0.5
    }
}

/// Vertical bars in every channel, at different widths, identical on every
/// row — see the module header for why it must not vary down the picture.
fn bars(mode: SstvMode) -> SourceImage {
    let spec = tempo_sstv::for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let mut rgb = Vec::with_capacity((w * h) as usize);
    for _ in 0..h {
        for x in 0..w {
            let r = if (x / 20) % 2 == 0 { 230 } else { 30 };
            let g = ((x * 255) / w) as u8;
            let b = if (x / 40) % 2 == 0 { 220 } else { 40 };
            rgb.push([r, g, b]);
        }
    }
    SourceImage {
        width: w,
        height: h,
        rgb,
    }
}

/// White noise at `rms` under the signal at half level.
fn with_noise(audio: &[f32], rms: f32) -> Vec<f32> {
    let mut rng = Lcg(7);
    audio
        .iter()
        .map(|s| s * 0.5 + rms * 3.46 * rng.next_unit())
        .collect()
}

/// Take `secs` of audio out at `frac` of the way through.
fn lose(audio: &mut Vec<f32>, frac: f64, secs: f64) {
    let at = (audio.len() as f64 * frac) as usize;
    let n = (secs * f64::from(BASE_HZ)) as usize;
    audio.drain(at..at + n);
}

/// Stream `audio` to a decoder told it runs at `rx_hz`, then the band after
/// it, until the picture completes.
fn decode(audio: &[f32], rx_hz: u32) -> SstvImage {
    let mut d = SstvDecoder::new(rx_hz).expect("decoder");
    let tail = vec![0.0_f32; rx_hz as usize * 30];
    for chunk in audio.chunks(1024).chain(tail.chunks(1024)) {
        for e in d.process(chunk) {
            if let SstvEvent::ImageComplete { image, .. } = e {
                return image;
            }
        }
    }
    panic!("no picture decoded");
}

/// Mean absolute error of each row against the source's (identical) rows.
fn row_errors(src: &SourceImage, got: &SstvImage) -> Vec<f64> {
    let w = src.width as usize;
    (0..src.height as usize)
        .map(|y| {
            let mut sum = 0.0;
            for x in 0..w {
                for c in 0..3 {
                    sum += (f64::from(src.rgb[y * w + x][c]) - f64::from(got.pixels[y * w + x][c]))
                        .abs();
                }
            }
            sum / (3.0 * w as f64)
        })
        .collect()
}

/// Band noise for the gap tests: enough that pulses are found among noise,
/// low enough that Robot 72's colour conversion does not tear rows on its own
/// (the gap tests are about where a line is read, not how noisy it is).
const GAP_NOISE_RMS: f32 = 0.08;

/// A row this far off on average is visibly torn; noise alone in these
/// fixtures stays well under it (measured 8-16 on a straight picture).
const TORN_ROW: f64 = 25.0;

/// At most this share of rows may be torn: the few rows a gap actually cut
/// through, and the median's first frames either side of it.
const MAX_TORN_SHARE: f64 = 0.05;

fn assert_straight(label: &str, src: &SourceImage, got: &SstvImage) {
    let errs = row_errors(src, got);
    let torn: Vec<usize> = errs
        .iter()
        .enumerate()
        .filter(|(_, e)| **e > TORN_ROW)
        .map(|(y, _)| y)
        .collect();
    let allowed = (errs.len() as f64 * MAX_TORN_SHARE) as usize;
    assert!(
        torn.len() <= allowed,
        "{label}: {} of {} rows torn (at most {allowed} allowed), first few {:?}",
        torn.len(),
        errs.len(),
        &torn[..torn.len().min(12)],
    );
}

/// The modes that carry their own sync pulse on every radio frame and whose
/// rows can be compared as RGB: the report's own Wraase SC-2 180, one of each
/// other family, and the slowest mode the crate decodes. Robot 36 is left out
/// of the gap test only: it alternates its chroma line by line, so a gap that
/// loses an ODD number of lines swaps red and blue difference below it — a
/// line-parity question this fix does not answer.
const MODES: [SstvMode; 6] = [
    SstvMode::WraaseSc2180,
    SstvMode::Martin1,
    SstvMode::Scottie1,
    SstvMode::Robot72,
    SstvMode::Pd120,
    SstvMode::PasokonP7,
];

/// A clock several hundred ppm off, on a clean signal. The whole-picture
/// slant fit gets most of the way, but its half-degree steps leave a residual
/// tilt: before the fix, 51 of Scottie 1's 256 rows at +453 ppm were torn.
#[test]
fn a_clean_picture_from_a_clock_hundreds_of_ppm_off_is_straight() {
    for mode in [SstvMode::WraaseSc2180, SstvMode::Scottie1] {
        let src = bars(mode);
        let audio = encode_image(mode, &src, BASE_HZ).expect("encode");
        for rx_hz in [BASE_HZ - 5, BASE_HZ + 5] {
            let label = format!("{mode:?} clean at {rx_hz} Hz");
            assert_straight(&label, &src, &decode(&audio, rx_hz));
        }
    }
}

/// THE REPORT. Audio lost twice in the middle of the picture — a third of a
/// second, then a tenth — with a sound-card clock 181 ppm off and band noise,
/// as a loaded receive path delivers it. Before the fix, more than half the
/// rows of every one of these modes were torn, including rows ABOVE the first
/// gap (the fit is pulled off by what comes after it).
#[test]
fn audio_lost_mid_picture_does_not_shear_the_rest() {
    for mode in MODES {
        let src = bars(mode);
        let mut audio = with_noise(
            &encode_image(mode, &src, BASE_HZ).expect("encode"),
            GAP_NOISE_RMS,
        );
        lose(&mut audio, 0.4, 0.35);
        lose(&mut audio, 0.75, 0.1);
        let label = format!("{mode:?} with two gaps");
        assert_straight(&label, &src, &decode(&audio, BASE_HZ + 2));
    }
}

/// A block lost as small as a sound-card buffer (20 ms) shears every line
/// after it just the same — the size of the gap does not matter, only that
/// there is one.
#[test]
fn a_twenty_millisecond_gap_does_not_shear_the_rest() {
    let mode = SstvMode::WraaseSc2180;
    let src = bars(mode);
    let mut audio = with_noise(
        &encode_image(mode, &src, BASE_HZ).expect("encode"),
        GAP_NOISE_RMS,
    );
    lose(&mut audio, 0.5, 0.02);
    assert_straight("SC-2 180, 20 ms gap", &src, &decode(&audio, BASE_HZ));
}

/// The other way to tear a picture without losing any audio: a clock offset
/// UNDER NOISE leaves the whole-picture fit a residual slant. 181 ppm and a
/// noise level these modes still decode cleanly at once each line is read
/// from its own pulse.
#[test]
fn a_clock_offset_under_noise_does_not_tear_rows() {
    for (mode, rms) in [
        (SstvMode::WraaseSc2180, 0.6_f32),
        (SstvMode::Martin1, 0.3),
        (SstvMode::Scottie1, 0.3),
        (SstvMode::PasokonP7, 0.3),
    ] {
        let src = bars(mode);
        let audio = with_noise(&encode_image(mode, &src, BASE_HZ).expect("encode"), rms);
        let label = format!("{mode:?} at 181 ppm, noise {rms}");
        assert_straight(&label, &src, &decode(&audio, BASE_HZ + 2));
    }
}
