//! Manual receive start — decoding a picture whose VIS header you never heard
//! (#202), and the measurement that says nothing else can start one.
//!
//! **Read the revert before this file.** A no-VIS decode was built and reverted
//! on 2026-08-06 (`65682855`) after three reviews, for two reasons:
//!
//! 1. It *invented pictures*. It tried to RECOGNISE one in the audio — seven
//!    heuristic gates plus mode inference — and the gates measured whether the
//!    video band moved, not whether the movement looked like a scan line.
//!    Reviewers built passing signals straight off the module's own list of
//!    what it claimed to reject.
//! 2. It *regressed ordinary VIS-anchored decoding*, by giving `Decoding` a
//!    second, audio-derived exit. On HF a carrier that stopped and a carrier
//!    momentarily lost are the same observation, so a 0.6 s fade cost half a
//!    Robot 36 and a 4 s QSB null split a Scottie 1 in two.
//!
//! [`SstvDecoder::start_manual`] is reachable by neither route, and that is the
//! whole design rather than a mitigation of it. There is no recogniser: the
//! decode starts when the operator says so and at no other time, and the mode
//! is the operator's own answer rather than something inferred. There is no new
//! exit from `Decoding`: the buffer fills and the picture is emitted, exactly
//! as for a header-anchored one, which is why `tests/dropout.rs` is untouched.
//!
//! So "zero false starts on noise" is a property of the SHAPE — nothing in the
//! audio is consulted about whether to begin. It is measured below anyway,
//! across many seeds and with a control that must trip, because a property
//! nobody measured is a claim.
//!
//! What this file checks, in order:
//!
//! - a picture with its header cut off decodes when the operator names the mode
//! - the same picture joined MID-LINE still comes out straight (the slant is
//!   recovered from the sync pulses, which is what `find_sync` always did — it
//!   never needed the header)
//! - naming the WRONG mode does not produce a right-looking picture
//! - 40 different noises, 10 s each, start nothing
//! - the control: the same audio DOES decode once the operator starts it, so
//!   the silence above is about the trigger and not about a dead pipe

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use tempo_sstv::{encode_image, for_mode, SourceImage, SstvDecoder, SstvEvent, SstvMode};

/// Nexus TX/RX audio rate, the rate `tx_loopback.rs` uses.
const RATE: u32 = 12_000;

/// Pre-silence (0.100 s) + the calibration/VIS header (0.910 s) — everything
/// `encode_image` emits before line 0. Cutting exactly this much is "the
/// operator tuned in the instant the picture started and missed the header".
const HEADER_SECS: f64 = 0.100 + 0.910;

/// A picture whose rows are all IDENTICAL: a horizontal ramp in red, a
/// complementary ramp in green, constant blue. Two properties, both wanted.
///
/// Because no row differs from any other, a decode that starts a line or two
/// late looks exactly like one that started on time — which is the honest bar
/// for joining a transmission already in progress, where the top of the picture
/// is genuinely gone.
///
/// Because every row has strong horizontal structure, a SHEARED decode does not
/// look like anything: each row would be displaced a little further than the
/// one above it, and the error against the source grows down the picture. That
/// is the failure this file exists to catch, and a flat or grey test card would
/// hide it completely.
fn row_invariant_image(mode: SstvMode) -> SourceImage {
    let spec = for_mode(mode);
    let (w, h) = (spec.line_pixels, spec.image_lines);
    let mut rgb = Vec::with_capacity((w * h) as usize);
    for _y in 0..h {
        for x in 0..w {
            let r = (x * 255 / (w - 1)) as u8;
            rgb.push([r, 255 - r, 96]);
        }
    }
    SourceImage {
        width: w,
        height: h,
        rgb,
    }
}

/// `mode`'s full transmission with the first `cut_secs` of it thrown away —
/// header included — plus a second of runway so the decoder's buffer fills.
fn headerless_audio(mode: SstvMode, img: &SourceImage, cut_secs: f64) -> Vec<f32> {
    let mut audio = encode_image(mode, img, RATE).expect("encode");
    let cut = (cut_secs * f64::from(RATE)).round() as usize;
    assert!(cut < audio.len(), "cut past the end of the transmission");
    audio.drain(..cut);
    audio.extend(std::iter::repeat_n(0.0_f32, RATE as usize));
    audio
}

/// Mean per-channel difference between `img` and the decoded pixels.
fn mean_diff(src: &[[u8; 3]], dec: &[[u8; 3]]) -> f64 {
    mean_diff_rows(src, dec, src.len(), 0)
}

/// As [`mean_diff`], but over the first `len - skip_tail` pixels only.
///
/// Cutting `n` lines off the FRONT of a transmission means the last `n` lines
/// of the picture were never in the buffer — the decoder demodulates them out
/// of the runway silence and they come back black. Those rows are not evidence
/// about slant or line phase, they are evidence that the audio ended, so a
/// mid-line-join comparison excludes them rather than paying for them with a
/// looser bar that would also hide a real shear.
fn mean_diff_rows(src: &[[u8; 3]], dec: &[[u8; 3]], len: usize, skip_tail: usize) -> f64 {
    assert_eq!(src.len(), dec.len());
    let n = len.saturating_sub(skip_tail);
    let mut sum: u64 = 0;
    for (a, b) in src.iter().zip(dec).take(n) {
        for ch in 0..3 {
            sum += u64::from((i32::from(a[ch]) - i32::from(b[ch])).unsigned_abs() as u8);
        }
    }
    sum as f64 / (n * 3) as f64
}

/// Push `audio` through a decoder the operator has manually started on `mode`,
/// and return the completed picture's pixels.
fn decode_manually(mode: SstvMode, audio: &[f32]) -> Vec<[u8; 3]> {
    let mut dec = SstvDecoder::new(RATE).expect("decoder");
    dec.start_manual(mode);
    assert!(
        dec.is_decoding(),
        "start_manual should put the decoder into a decode straight away"
    );
    let events = dec.process(audio);
    events
        .iter()
        .find_map(|e| match e {
            SstvEvent::ImageComplete { image, .. } => Some(image.pixels.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{mode:?}: manual start produced no picture"))
}

/// The bar the rest of the suite uses for a synthetic round trip.
const MEAN_BAR: f64 = 5.0;

/// The header is gone and the operator names the mode: the picture arrives.
/// Two families, so this is not a property of one mode's geometry — Martin 2 is
/// sequential RGB with sync at line start, Robot 36 is luma plus alternating
/// chroma.
#[test]
fn a_headerless_picture_decodes_when_the_operator_names_the_mode() {
    for mode in [SstvMode::Martin2, SstvMode::Robot36] {
        let img = row_invariant_image(mode);
        let audio = headerless_audio(mode, &img, HEADER_SECS);
        let pixels = decode_manually(mode, &audio);
        let mean = mean_diff(&img.rgb, &pixels);
        assert!(
            mean < MEAN_BAR,
            "{mode:?}: headerless decode mean per-channel diff {mean:.2} >= {MEAN_BAR}",
        );
    }
}

/// The real case: the operator tunes in partway through a line, not neatly at
/// the top of one. Nothing tells the decoder where the line boundary is except
/// the sync pulses themselves — which is exactly what `find_sync` fits, header
/// or no header. A picture that came out sheared here would be the #202 bug.
#[test]
fn joining_mid_line_still_comes_out_straight() {
    for mode in [SstvMode::Martin2, SstvMode::Robot36] {
        let spec = for_mode(mode);
        let img = row_invariant_image(mode);
        // 0.37 of a line in — deliberately not a half or a quarter, so a bug
        // that happens to be symmetric about the line centre has nowhere to
        // hide. Plus three whole lines, because a real tune-in is seconds late,
        // not milliseconds.
        let cut_lines = 3.37_f64;
        let cut = HEADER_SECS + cut_lines * spec.line_seconds;
        let audio = headerless_audio(mode, &img, cut);
        let pixels = decode_manually(mode, &audio);
        // The last four rows were never transmitted into the buffer (3.37 lines
        // came off the front, plus one for the partial); they decode from the
        // runway silence. Everything above them is the real evidence.
        let tail = (cut_lines.ceil() as usize + 1) * spec.line_pixels as usize;
        let mean = mean_diff_rows(&img.rgb, &pixels, img.rgb.len(), tail);
        assert!(
            mean < MEAN_BAR,
            "{mode:?}: mid-line join mean per-channel diff {mean:.2} >= {MEAN_BAR} — \
             the picture is sheared, or the line phase was never found",
        );
    }
}

/// Naming the wrong mode gives a wrong picture, and that is the honest
/// behaviour: the operator's answer is the only thing the decoder has, so it is
/// believed. Stated as a test because it is the cost of the design — the
/// alternative is inferring the mode, which is what was reverted — and because
/// a decoder that silently "recovered" here would be inferring after all.
#[test]
fn naming_the_wrong_mode_does_not_quietly_produce_the_right_picture() {
    // Robot 72 on the air, the operator answers Robot 36. Same raster, same
    // family, half the line time — the closest wrong answer there is, and one
    // whose buffer still fills from the audio available (a wrong answer with a
    // LONGER line time, Martin 2 heard as Martin 1, simply never completes: the
    // decoder waits for a picture's worth of audio that is not coming. That is
    // also correct, and it is why this test names a shorter mode instead).
    let sent = SstvMode::Robot72;
    let img = row_invariant_image(sent);
    let audio = headerless_audio(sent, &img, HEADER_SECS);
    let pixels = decode_manually(SstvMode::Robot36, &audio);
    let mean = mean_diff(&img.rgb, &pixels);
    assert!(
        mean > MEAN_BAR,
        "decoding Robot 72 audio as Robot 36 came out to {mean:.2} mean diff, \
         under the {MEAN_BAR} bar — that would mean the mode the operator names \
         does not actually decide the decode",
    );
}

// ---------------------------------------------------------------------------
// FALSE STARTS
// ---------------------------------------------------------------------------

/// Deterministic xorshift — a fixed seed per trial so a failure is reproducible.
struct Rng(u64);

impl Rng {
    fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 / (1_u64 << 53) as f64) as f32 - 0.5
    }
}

/// Band noise at the ~0.3 RMS the Zarya recording measured (see
/// `tests/no_vis.rs`), `secs` long at [`RATE`].
fn noise(seed: u64, secs: f64) -> Vec<f32> {
    let mut rng = Rng(seed);
    let n = (secs * f64::from(RATE)) as usize;
    let scale = 0.3_f32 * 12.0_f32.sqrt();
    (0..n).map(|_| rng.next_unit() * scale).collect()
}

/// How many independent noises to try. Each is a different band, a different
/// moment, a different set of accidental tone runs.
const FALSE_START_TRIALS: usize = 40;
/// How long each noise runs. Ten seconds is longer than any mode's header
/// window and long enough to contain many accidental 1900/1200 Hz runs.
const FALSE_START_SECS: f64 = 10.0;

/// ⭐ THE MEASUREMENT. Forty noises, ten seconds each, and the decoder emits
/// nothing at all — no lock, no partial, no picture. The manual start exists
/// and is never called, which is the point: it is not a threshold that noise
/// might cross, it is a function call.
#[test]
fn nothing_in_the_audio_starts_a_decode() {
    let mut events_seen = 0_usize;
    let mut worst: Option<(u64, String)> = None;
    for trial in 0..FALSE_START_TRIALS {
        let seed = 0x5EED_0000_u64 + trial as u64 * 0x9E37_79B9;
        let audio = noise(seed, FALSE_START_SECS);
        let mut dec = SstvDecoder::new(RATE).expect("decoder");
        let events = dec.process(&audio);
        assert!(
            !dec.is_decoding(),
            "seed {seed:#x}: noise put the decoder into a decode",
        );
        if !events.is_empty() {
            events_seen += events.len();
            if worst.is_none() {
                worst = Some((seed, format!("{:?}", &events[..events.len().min(3)])));
            }
        }
    }
    assert_eq!(
        events_seen, 0,
        "{FALSE_START_TRIALS} × {FALSE_START_SECS} s of noise produced {events_seen} event(s); \
         first at {worst:?}",
    );
}

/// ⭐ THE CONTROL, and without it the test above proves nothing — a decoder
/// wired to a dead audio path would also emit nothing forty times over. The
/// SAME noise generator, the SAME rate, the SAME `process` call: the only
/// difference is that the operator started it. A picture comes out (of noise,
/// which is what noise looks like), so the silence above is a fact about the
/// trigger and not about the plumbing.
#[test]
fn the_control_the_same_noise_decodes_once_the_operator_starts_it() {
    let mode = SstvMode::Robot36;
    let spec = for_mode(mode);
    // One picture's worth plus runway, from the same generator as above.
    let audio = noise(0x5EED_0000, spec.airtime_seconds() + 2.0);

    let mut untouched = SstvDecoder::new(RATE).expect("decoder");
    assert!(
        untouched.process(&audio).is_empty(),
        "this very audio must start nothing on its own",
    );

    let mut started = SstvDecoder::new(RATE).expect("decoder");
    started.start_manual(mode);
    let events = started.process(&audio);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SstvEvent::ImageComplete { .. })),
        "the manually started decoder produced no picture from the same audio — \
         the false-start measurement above would be vacuous",
    );
}

/// A manual decode ENDS the way any other does: back to waiting for a header,
/// so the next station that sends one is picked up normally and the operator
/// does not have to disarm and re-arm.
#[test]
fn after_a_manual_decode_the_receiver_waits_for_a_header_again() {
    let mode = SstvMode::Robot36;
    let img = row_invariant_image(mode);
    let audio = headerless_audio(mode, &img, HEADER_SECS);
    let mut dec = SstvDecoder::new(RATE).expect("decoder");
    dec.start_manual(mode);
    let events = dec.process(&audio);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SstvEvent::ImageComplete { .. })),
        "expected the manual decode to finish",
    );
    assert!(
        !dec.is_decoding(),
        "the decoder is still mid-picture after ImageComplete",
    );
}
