//! Mid-picture tune-in and truncated transmissions must produce an image.
//!
//! **Origin.** Operator report, 2026-08-05: *"when someone transfers sstv,
//! and I don't hear the start of the transmission, it won't decode. It
//! should decode partial images as well."* That is the ordinary way SSTV
//! is received — you tune across 14.230, a picture is already halfway
//! through, and every other SSTV program shows you the bottom half.
//!
//! The report has two halves and they share one missing mechanism
//! ("emit what you have"):
//!
//! 1. **No VIS at all.** The decoder started in `AwaitingVis` and the
//!    only way out was a VIS header, so a mid-picture start decoded
//!    nothing, forever.
//! 2. **VIS heard, transmission cut short.** `Decoding` had exactly one
//!    exit — the audio buffer reaching `target_audio_samples` — so a
//!    transmission that stopped one percent short emitted *nothing*, not
//!    even the lines already received, while its buffer grew unbounded.
//!
//! Every waveform here is built from the crate's own synthetic
//! modulators (`__test_support`), the same ones `tests/roundtrip.rs`
//! validates against — real FM audio through the real resampler and
//! demodulator, not a mocked event stream.
//!
//! **The other half of this file is the part that matters most.**
//! `tests/no_vis.rs` exists because an ISS Zarya recording carrying no
//! SSTV at all had to be proven not to produce images, and relaxing the
//! VIS requirement is exactly how a decoder starts hallucinating
//! pictures out of an empty band. Every adversarial input the design
//! named — white noise, silence, SSB speech, a CW pileup keyed *on the
//! sync frequency*, and a bare periodic 1200 Hz pulse train with no
//! picture between the pulses — is asserted to produce no image and no
//! lock here.

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

/// Streaming chunk size — a mid-picture start must work when audio
/// arrives the way live capture delivers it, not as one buffer.
const CHUNK: usize = 1024;

fn work_rate() -> f64 {
    f64::from(WORKING_SAMPLE_RATE_HZ)
}

/// Feed `audio` to a fresh working-rate decoder in small chunks.
fn stream(audio: &[f32]) -> Vec<SstvEvent> {
    let mut d = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder");
    let mut events = Vec::new();
    for chunk in audio.chunks(CHUNK) {
        events.extend(d.process(chunk));
    }
    events
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

/// Index of the first image row carrying any non-black pixel.
fn first_painted_row(img: &SstvImage) -> Option<u32> {
    let w = img.width as usize;
    (0..img.height).find(|&y| {
        img.pixels[(y as usize) * w..(y as usize + 1) * w]
            .iter()
            .any(|p| *p != [0, 0, 0])
    })
}

fn the_partial(events: &[SstvEvent]) -> &SstvImage {
    let mut found = None;
    for e in events {
        if let SstvEvent::ImageComplete {
            image,
            partial: true,
        } = e
        {
            assert!(found.is_none(), "more than one partial image emitted");
            found = Some(image);
        }
    }
    found.unwrap_or_else(|| {
        panic!(
            "no ImageComplete{{partial:true}}; got {} events",
            events.len()
        )
    })
}

fn count_images(events: &[SstvEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, SstvEvent::ImageComplete { .. }))
        .count()
}

// ---------------------------------------------------------------------
// Source images (same shapes tests/roundtrip.rs uses, so the modulators
// are exercised on content they can actually reproduce).
// ---------------------------------------------------------------------

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

/// Build a full on-air transmission, then throw away everything before
/// `cut_lines` (plus a fraction of a line, because an operator does not
/// tune in on a line boundary) — including the VIS header. Returns the
/// truncated audio with trailing silence, as a real receiver would hear
/// it after the sender stops.
fn tune_in_late(mode: SstvMode, image_audio: &[f32], cut_lines: f64) -> Vec<f32> {
    let spec = tempo_sstv::for_mode(mode);
    let cut = (cut_lines * spec.line_seconds * work_rate()) as usize;
    assert!(
        cut < image_audio.len(),
        "cut past the end of the transmission"
    );
    let mut audio = image_audio[cut..].to_vec();
    // The sender stops. Silence long enough for the end-of-transmission
    // trigger (3 line periods) plus resampler group-delay headroom.
    audio.extend(std::iter::repeat_n(
        0.0_f32,
        (5.0 * spec.line_seconds * work_rate()) as usize + 8192,
    ));
    audio
}

// ---------------------------------------------------------------------
// THE HEADLINE: a picture already in flight must decode.
// ---------------------------------------------------------------------

/// Scottie 1 — mid-line sync, the family whose line start is *not* the
/// pulse, so this also proves the line-boundary trim.
#[test]
fn mid_picture_scottie1_decodes_the_bottom_of_the_picture() {
    let mode = SstvMode::Scottie1;
    let spec = tempo_sstv::for_mode(mode);
    let src = rgb_source(mode);
    let audio = tune_in_late(
        mode,
        &tempo_sstv::__test_support::mode_scottie::encode_scottie(mode, &src),
        102.37,
    );

    let events = stream(&audio);

    // The mode was inferred from sync timing alone — no VIS in this audio.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SstvEvent::SyncLocked {
                mode: SstvMode::Scottie1,
                ..
            }
        )),
        "no SyncLocked(Scottie1); got {:?}",
        events
            .iter()
            .map(std::mem::discriminant)
            .collect::<Vec<_>>()
            .len()
    );
    // A blind image is never claimed to be complete.
    assert_eq!(count_images(&events), 1, "expected exactly one image");
    let img = the_partial(&events);
    assert_eq!(img.mode, mode);
    assert_eq!(
        (img.width, img.height),
        (spec.line_pixels, spec.image_lines)
    );

    // Bottom-anchored: the picture starts near where we tuned in, and the
    // rows above it are black rather than garbage.
    let first = first_painted_row(img).expect("some rows painted");
    assert!(
        (100..=110).contains(&first),
        "expected the picture to start near line 103 (we tuned in at 102.37), got {first}"
    );
    let blank = &img.pixels[..(first as usize) * (spec.line_pixels as usize)];
    assert!(
        blank.iter().all(|p| *p == [0, 0, 0]),
        "rows above the tune-in point must stay black, not carry garbage"
    );

    // And the painted rows are the RIGHT rows — bottom-anchoring recovers
    // the true row indices when the sender ran to the end of the picture.
    let from = (first as usize) * (spec.line_pixels as usize);
    let corr = correlation(&src[from..], &img.pixels[from..]);
    assert!(
        corr > 0.9,
        "decoded bottom-of-picture correlation {corr:.4} <= 0.9 (rows {first}..{})",
        spec.image_lines
    );
}

/// Martin 1 — line-start sync and the 4.862 ms sync class, i.e. the mode
/// that sits closest to Scottie 1 in line period (4.2 %). Locking the
/// right one of that pair is the tightest call inference has to make.
#[test]
fn mid_picture_martin1_decodes_and_is_not_confused_with_scottie1() {
    let mode = SstvMode::Martin1;
    let spec = tempo_sstv::for_mode(mode);
    let src = rgb_source(mode);
    let audio = tune_in_late(
        mode,
        &tempo_sstv::__test_support::mode_scottie::encode_scottie(mode, &src),
        80.4,
    );

    let events = stream(&audio);
    let locked: Vec<SstvMode> = events
        .iter()
        .filter_map(|e| match e {
            SstvEvent::SyncLocked { mode, .. } => Some(*mode),
            _ => None,
        })
        .collect();
    assert_eq!(
        locked,
        vec![SstvMode::Martin1],
        "Martin 1 must lock as Martin 1 (its nearest neighbour in period is Scottie 1)"
    );

    let img = the_partial(&events);
    assert_eq!(img.mode, mode);
    let first = first_painted_row(img).expect("some rows painted");
    assert!(
        (79..=89).contains(&first),
        "expected the picture to start near line 81, got {first}"
    );
    let from = (first as usize) * (spec.line_pixels as usize);
    let corr = correlation(&src[from..], &img.pixels[from..]);
    assert!(corr > 0.9, "Martin 1 partial correlation {corr:.4} <= 0.9");
}

/// PD-120 — the ISS mode, and the only family that packs two image rows
/// into each radio frame, so row placement goes through a different arm.
#[test]
fn mid_picture_pd120_decodes_the_bottom_of_the_picture() {
    let mode = SstvMode::Pd120;
    let spec = tempo_sstv::for_mode(mode);
    let ycrcb = ycrcb_source(mode);
    let reference: Vec<[u8; 3]> = ycrcb
        .iter()
        .map(|p| tempo_sstv::__test_support::mode_pd::ycbcr_to_rgb(p[0], p[1], p[2]))
        .collect();
    // 248 radio frames carry 496 image rows; tune in around frame 90.
    let audio = tune_in_late(
        mode,
        &tempo_sstv::__test_support::mode_pd::encode_pd(mode, &ycrcb),
        90.6,
    );

    let events = stream(&audio);
    assert!(
        events.iter().any(|e| matches!(
            e,
            SstvEvent::SyncLocked {
                mode: SstvMode::Pd120,
                ..
            }
        )),
        "no SyncLocked(Pd120)"
    );
    let img = the_partial(&events);
    let first = first_painted_row(img).expect("some rows painted");
    // Frame 91 → image row 182.
    assert!(
        (180..=196).contains(&first),
        "expected the picture to start near row 182, got {first}"
    );
    let from = (first as usize) * (spec.line_pixels as usize);
    let corr = correlation(&reference[from..], &img.pixels[from..]);
    assert!(corr > 0.9, "PD120 partial correlation {corr:.4} <= 0.9");
}

/// The second half of the operator's report: the VIS *was* heard, but the
/// sender stopped mid-picture. Before this work that emitted nothing at
/// all — not a partial, not even the lines already decoded. Here the
/// origin is known, so the image is TOP-anchored: decoded rows at 0..n
/// and the missing tail black.
#[test]
fn vis_heard_then_transmission_cut_short_emits_a_top_anchored_partial() {
    let mode = SstvMode::Scottie1;
    let spec = tempo_sstv::for_mode(mode);
    let src = rgb_source(mode);

    let mut audio = tempo_sstv::__test_support::vis::synth_vis(0x3C, 0.0);
    let image_audio = tempo_sstv::__test_support::mode_scottie::encode_scottie(mode, &src);
    // The sender drops the carrier 60 % of the way through the picture.
    let keep = (0.6 * image_audio.len() as f64) as usize;
    audio.extend_from_slice(&image_audio[..keep]);
    audio.extend(std::iter::repeat_n(
        0.0_f32,
        (5.0 * spec.line_seconds * work_rate()) as usize + 8192,
    ));

    let events = stream(&audio);
    assert!(
        events.iter().any(|e| matches!(
            e,
            SstvEvent::VisDetected {
                mode: SstvMode::Scottie1,
                ..
            }
        )),
        "the VIS header is present and must still be detected"
    );
    // A VIS-anchored decode never needs the blind path.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SstvEvent::SyncLocked { .. })),
        "a VIS-started transmission must not also blind-lock"
    );

    let img = the_partial(&events);
    // Top-anchored: row 0 is row 0 because the VIS said so.
    assert_eq!(
        first_painted_row(img),
        Some(0),
        "a VIS-anchored partial keeps its known origin at row 0"
    );
    // Roughly 60 % of the picture arrived; the tail stays black.
    let painted = (0..spec.image_lines)
        .filter(|&y| {
            let w = spec.line_pixels as usize;
            img.pixels[(y as usize) * w..(y as usize + 1) * w]
                .iter()
                .any(|p| *p != [0, 0, 0])
        })
        .count();
    assert!(
        (140..=160).contains(&painted),
        "expected ~154 of 256 rows painted, got {painted}"
    );
    let to = painted * (spec.line_pixels as usize);
    let corr = correlation(&src[..to], &img.pixels[..to]);
    assert!(corr > 0.9, "truncated-image correlation {corr:.4} <= 0.9");
}

/// A complete transmission must still decode as a COMPLETE image. The
/// end-of-transmission trigger must never pre-empt the full-buffer path
/// and downgrade a whole picture to a partial.
#[test]
fn a_complete_transmission_is_still_not_partial() {
    let mode = SstvMode::Scottie1;
    let src = rgb_source(mode);
    let mut audio = tempo_sstv::__test_support::vis::synth_vis(0x3C, 0.0);
    audio.extend(tempo_sstv::__test_support::mode_scottie::encode_scottie(
        mode, &src,
    ));
    audio.extend(std::iter::repeat_n(0.0_f32, 8192));

    let events = stream(&audio);
    assert_eq!(count_images(&events), 1);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SstvEvent::ImageComplete { partial: false, .. })),
        "a whole transmission must still report partial:false"
    );
}

// ---------------------------------------------------------------------
// THE GUARD: everything that is not a picture must produce nothing.
//
// `assert!(events.is_empty())` throughout — stricter than "no image".
// A blind decoder that emitted a "searching" heartbeat would be a false
// positive by another name, so the lock state is deliberately not an
// event.
// ---------------------------------------------------------------------

/// Deterministic LCG, same constants as `tests/no_vis.rs`.
struct Lcg(u32);
impl Lcg {
    fn next_unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 as f32 / u32::MAX as f32) - 0.5
    }
}

fn assert_silent(label: &str, audio: &[f32]) {
    let events = stream(audio);
    assert!(
        events.is_empty(),
        "{label} produced {} event(s): {:?}",
        events.len(),
        &events[..events.len().min(4)]
    );
}

#[test]
fn white_noise_produces_nothing() {
    // 30 s — far longer than the ~2.6 s a Scottie 1 lock needs, so the
    // gate gets many chances to fail.
    let mut rng = Lcg(0x5EED_5EED);
    let scale = 0.3_f32 * 12.0_f32.sqrt();
    let audio: Vec<f32> = (0..(WORKING_SAMPLE_RATE_HZ as usize * 30))
        .map(|_| rng.next_unit() * scale)
        .collect();
    assert_silent("white noise", &audio);
}

#[test]
fn silence_produces_nothing() {
    assert_silent(
        "silence",
        &vec![0.0_f32; WORKING_SAMPLE_RATE_HZ as usize * 30],
    );
}

#[test]
fn ssb_speech_produces_nothing() {
    // Voiced speech: a wandering pitch with harmonics inside the SSB
    // passband, amplitude-gated into syllables.
    let n = WORKING_SAMPLE_RATE_HZ as usize * 30;
    let mut audio = Vec::with_capacity(n);
    let mut phase = [0.0_f64; 4];
    for i in 0..n {
        let t = (i as f64) / work_rate();
        let pitch = 110.0 + 40.0 * (2.0 * std::f64::consts::PI * 0.7 * t).sin();
        // Syllable envelope ~4 Hz, with pauses between words.
        let env = (0.5 + 0.5 * (2.0 * std::f64::consts::PI * 4.0 * t).sin())
            * if (t * 0.8).fract() < 0.7 { 1.0 } else { 0.05 };
        let mut s = 0.0;
        for (h, ph) in phase.iter_mut().enumerate() {
            let f = pitch * ((h + 3) as f64); // formant-ish harmonics, 330 Hz up
            *ph += 2.0 * std::f64::consts::PI * f / work_rate();
            s += ph.sin() / ((h + 1) as f64);
        }
        audio.push((0.35 * env * s) as f32);
    }
    assert_silent("SSB speech", &audio);
}

#[test]
fn cw_pileup_produces_nothing() {
    // Several stations at once, one of them keyed EXACTLY on the 1200 Hz
    // sync frequency — the worst case for a sync-pulse gate — at speeds
    // whose element timings are near-submultiples of real line periods.
    let n = WORKING_SAMPLE_RATE_HZ as usize * 30;
    let tones = [1200.0_f64, 700.0, 1450.0, 1900.0, 2400.0];
    let dits = [0.060_f64, 0.075, 0.113, 0.050, 0.150];
    let mut audio = vec![0.0_f32; n];
    for (k, (&f, &dit)) in tones.iter().zip(dits.iter()).enumerate() {
        let offset = (k as f64) * 0.037;
        for (i, s) in audio.iter_mut().enumerate() {
            let t = (i as f64) / work_rate() + offset;
            // Pseudo-random Morse-ish keying: elements of 1 or 3 dits.
            let elem = (t / dit) as u64;
            let on = (elem.wrapping_mul(2_654_435_761) >> 5) & 3 != 0;
            if on {
                *s += (0.3 * (2.0 * std::f64::consts::PI * f * t).sin()) as f32;
            }
        }
    }
    assert_silent("CW pileup", &audio);
}

/// The direct adversary for a sync-timing gate: a *perfect* 1200 Hz
/// pulse train at exactly Scottie 1's line period, with noise where the
/// picture should be. Periodicity alone is not evidence of a picture —
/// this is what the video-band occupancy gate is for.
#[test]
fn periodic_1200hz_pulses_without_a_picture_produce_nothing() {
    let spec = tempo_sstv::for_mode(SstvMode::Scottie1);
    let n = WORKING_SAMPLE_RATE_HZ as usize * 30;
    let period = (spec.line_seconds * work_rate()) as usize;
    let sync = (spec.sync_seconds * work_rate()) as usize;
    let mut rng = Lcg(0x1234_9876);
    let audio: Vec<f32> = (0..n)
        .map(|i| {
            if i % period < sync {
                let t = (i as f64) / work_rate();
                (0.5 * (2.0 * std::f64::consts::PI * 1200.0 * t).sin()) as f32
            } else {
                rng.next_unit() * 0.5
            }
        })
        .collect();
    assert_silent("a bare 1200 Hz pulse train with no picture", &audio);
}

/// A mid-picture Robot 36 must produce NOTHING rather than a
/// wrong-coloured picture. Robot 24 and Robot 36 are timing-identical, so
/// inference cannot name which it is, and — decisively — their Cr/Cb
/// assignment comes from absolute row parity, which a mid-picture start
/// does not know. Guessing would swap chroma for the whole image, which
/// reads to the operator as a fault in their radio. The subharmonic gate
/// also has to stop this locking as Robot 72 (exactly 2 × 150 ms).
#[test]
fn mid_picture_robot36_is_refused_rather_than_guessed() {
    let mode = SstvMode::Robot36;
    let src = ycrcb_source(mode);
    let audio = tune_in_late(
        mode,
        &tempo_sstv::__test_support::mode_robot::encode_robot(mode, &src),
        60.3,
    );
    let events = stream(&audio);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SstvEvent::SyncLocked { .. })),
        "Robot 36 must not blind-lock (chroma parity is unrecoverable mid-picture); got {events:?}"
    );
    assert_eq!(
        count_images(&events),
        0,
        "no image at all is the correct answer for a mid-picture Robot 36"
    );
}
