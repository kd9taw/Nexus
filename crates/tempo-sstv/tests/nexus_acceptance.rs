//! Nexus acceptance gate (WS3 — SSTV receiver core).
//!
//! Independent cross-check of the vendored decoder: a test-local SSTV
//! modulator written directly from the published mode timings (NOT the
//! crate's `__test_support` encoders — separate code, separate constants)
//! synthesizes a full transmission at 12 kHz, the Nexus audio-tap rate.
//! The audio is fed in small streaming chunks the way live capture will
//! deliver it, and the decoded image must correlate > 0.9 with the source.
//!
//! Covers: VIS detect at 12 kHz with the real on-air preamble
//! (leader/break/leader/start — the vendored `synth_vis` omits the first
//! leader+break), streaming chunked `process()`, progressive `LineDecoded`
//! ordering, `ImageComplete` geometry, pixel fidelity (Scottie 1 and the
//! ISS mode PD120), and VIS rejection (unknown code, plain noise).

#![cfg(feature = "test-support")]
#![allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use tempo_sstv::{SstvDecoder, SstvEvent, SstvMode};

/// Nexus shared audio tap rate (Hz).
const NEXUS_RATE: f64 = 12_000.0;
/// Streaming chunk size — small enough that every image spans thousands of
/// `process()` calls, proving the decoder needs no single-buffer feed.
const CHUNK: usize = 1024;

/// Continuous-phase FM tone generator at an arbitrary sample rate.
/// Cumulative-time targets so per-tone rounding never drifts — the same
/// discipline as the crate's `ToneWriter`, re-implemented independently.
struct Fm {
    out: Vec<f32>,
    phase: f64,
    t: f64,
    rate: f64,
}

impl Fm {
    fn new(rate: f64) -> Self {
        Self {
            out: Vec::new(),
            phase: 0.0,
            t: 0.0,
            rate,
        }
    }

    fn tone(&mut self, freq_hz: f64, secs: f64) {
        self.t += secs;
        let target = (self.t * self.rate).round() as usize;
        let dphi = 2.0 * std::f64::consts::PI * freq_hz / self.rate;
        while self.out.len() < target {
            self.out.push(self.phase.sin() as f32);
            self.phase = (self.phase + dphi) % (2.0 * std::f64::consts::PI);
        }
    }

    fn lum(v: u8) -> f64 {
        1500.0 + 800.0 * f64::from(v) / 255.0
    }
}

/// Real on-air VIS header: 1900 Hz leader, 10 ms 1200 Hz break, second
/// leader, 30 ms 1200 Hz start bit, 7 data bits LSB-first (1 = 1100 Hz,
/// 0 = 1300 Hz), even parity bit, 30 ms 1200 Hz stop bit.
fn vis_header(fm: &mut Fm, code: u8) {
    assert!(code < 0x80);
    fm.tone(1900.0, 0.300);
    fm.tone(1200.0, 0.010);
    fm.tone(1900.0, 0.300);
    fm.tone(1200.0, 0.030);
    let mut parity = 0u8;
    for b in 0..7 {
        let bit = (code >> b) & 1;
        parity ^= bit;
        fm.tone(if bit == 1 { 1100.0 } else { 1300.0 }, 0.030);
    }
    fm.tone(if parity == 1 { 1100.0 } else { 1300.0 }, 0.030);
    fm.tone(1200.0, 0.030);
}

/// Scottie 1, hardcoded from the published timing (Dayton/KB4YZ tables, as
/// in slowrx `modespec.c`): per line [sep 1.5 ms 1500 Hz][G][sep][B]
/// [sync 9 ms 1200 Hz][porch 1.5 ms 1500 Hz][R], 320 px × 0.432 ms,
/// line total 428.38 ms, 256 lines. No leading starter sync — matches the
/// alignment the vendored decoder is validated against.
fn encode_scottie1(fm: &mut Fm, rgb: &[[u8; 3]]) {
    const SEP: f64 = 1.5e-3;
    const PIX: f64 = 0.432e-3;
    const SYNC: f64 = 9e-3;
    const PORCH: f64 = 1.5e-3;
    const LINE: f64 = 428.38e-3;
    const W: usize = 320;
    const H: usize = 256;
    assert_eq!(rgb.len(), W * H);
    let t0 = fm.t;
    for y in 0..H {
        fm.tone(1500.0, SEP);
        for x in 0..W {
            fm.tone(Fm::lum(rgb[y * W + x][1]), PIX);
        }
        fm.tone(1500.0, SEP);
        for x in 0..W {
            fm.tone(Fm::lum(rgb[y * W + x][2]), PIX);
        }
        fm.tone(1200.0, SYNC);
        fm.tone(1500.0, PORCH);
        for x in 0..W {
            fm.tone(Fm::lum(rgb[y * W + x][0]), PIX);
        }
        // Pad to the exact line boundary.
        let pad = t0 + (y + 1) as f64 * LINE - fm.t;
        if pad > 0.0 {
            fm.tone(1500.0, pad);
        }
    }
}

/// PD120, hardcoded from the published timing: per line pair
/// [sync 20 ms 1200 Hz][porch 2.08 ms 1500 Hz][Y odd][Cr avg][Cb avg]
/// [Y even], 640 px × 0.19 ms per channel, line total 508.48 ms,
/// 496 image lines (248 radio frames). `ycrcb` is row-major [Y, Cr, Cb].
fn encode_pd120(fm: &mut Fm, ycrcb: &[[u8; 3]]) {
    const PIX: f64 = 0.19e-3;
    const SYNC: f64 = 20e-3;
    const PORCH: f64 = 2.08e-3;
    const LINE: f64 = 508.48e-3;
    const W: usize = 640;
    const H: usize = 496;
    assert_eq!(ycrcb.len(), W * H);
    let t0 = fm.t;
    for pair in 0..H / 2 {
        let (r0, r1) = (pair * 2, pair * 2 + 1);
        fm.tone(1200.0, SYNC);
        fm.tone(1500.0, PORCH);
        for x in 0..W {
            fm.tone(Fm::lum(ycrcb[r0 * W + x][0]), PIX);
        }
        for x in 0..W {
            fm.tone(
                Fm::lum(u8::midpoint(ycrcb[r0 * W + x][1], ycrcb[r1 * W + x][1])),
                PIX,
            );
        }
        for x in 0..W {
            fm.tone(
                Fm::lum(u8::midpoint(ycrcb[r0 * W + x][2], ycrcb[r1 * W + x][2])),
                PIX,
            );
        }
        for x in 0..W {
            fm.tone(Fm::lum(ycrcb[r1 * W + x][0]), PIX);
        }
        let pad = t0 + (pair + 1) as f64 * LINE - fm.t;
        if pad > 0.0 {
            fm.tone(1500.0, pad);
        }
    }
}

/// Feed audio to a fresh 12 kHz decoder in `CHUNK`-sample pieces.
fn stream_decode(audio: &[f32]) -> Vec<SstvEvent> {
    let mut d = SstvDecoder::new(NEXUS_RATE as u32).expect("12 kHz decoder");
    let mut events = Vec::new();
    for chunk in audio.chunks(CHUNK) {
        events.extend(d.process(chunk));
    }
    events
}

/// Pearson correlation across all flattened RGB channel values.
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

#[test]
fn scottie1_stream_at_12khz_decodes_with_high_correlation() {
    const W: usize = 320;
    const H: usize = 256;
    // Varied source: red gradient, green stripes, blue inverse gradient.
    let mut rgb = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let r = (x * 255 / (W - 1)) as u8;
            let g = if (y / 8) % 2 == 0 { 220 } else { 40 };
            let b = 255 - r;
            rgb.push([r, g, b]);
        }
    }

    let mut fm = Fm::new(NEXUS_RATE);
    vis_header(&mut fm, 0x3C); // Scottie 1
    encode_scottie1(&mut fm, &rgb);
    fm.tone(1500.0, 2.0); // flush resampler delay + fill the decode buffer
    let events = stream_decode(&fm.out);

    assert!(
        events.iter().any(|e| matches!(
            e,
            SstvEvent::VisDetected {
                mode: SstvMode::Scottie1,
                ..
            }
        )),
        "no VisDetected(Scottie1); events head: {:?}",
        &events[..events.len().min(3)]
    );

    // Progressive render contract: every line, in order, full width.
    let lines: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            SstvEvent::LineDecoded {
                line_index, pixels, ..
            } => {
                assert_eq!(pixels.len(), W);
                Some(*line_index)
            }
            _ => None,
        })
        .collect();
    assert_eq!(lines, (0..H as u32).collect::<Vec<_>>());

    let img = events
        .iter()
        .find_map(|e| match e {
            SstvEvent::ImageComplete {
                image,
                partial: false,
            } => Some(image),
            _ => None,
        })
        .expect("ImageComplete");
    assert_eq!(img.mode, SstvMode::Scottie1);
    assert_eq!((img.width, img.height), (W as u32, H as u32));

    let corr = correlation(&rgb, &img.pixels);
    assert!(corr > 0.9, "pixel correlation {corr:.4} <= 0.9");
}

#[test]
fn pd120_stream_at_12khz_decodes_with_high_correlation() {
    const W: usize = 640;
    const H: usize = 496;
    // Source in YCrCb (the PD wire format); reference RGB via the crate's
    // own conversion so the comparison isolates demodulation fidelity, not
    // color-matrix convention. Chroma varies every 4 rows so pair-averaged
    // chroma is recoverable (PD carries one chroma pair per two rows).
    let mut ycrcb = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let lum = (x * 255 / (W - 1)) as u8;
            let cr = if (y / 4) % 2 == 0 { 200 } else { 56 };
            let cb = if ((y / 4) + 1) % 2 == 0 { 190 } else { 66 };
            ycrcb.push([lum, cr, cb]);
        }
    }
    let reference: Vec<[u8; 3]> = ycrcb
        .iter()
        .map(|p| tempo_sstv::__test_support::mode_pd::ycbcr_to_rgb(p[0], p[1], p[2]))
        .collect();

    let mut fm = Fm::new(NEXUS_RATE);
    vis_header(&mut fm, 0x5F); // PD120 — the ISS mode
    encode_pd120(&mut fm, &ycrcb);
    fm.tone(1500.0, 2.0);
    let events = stream_decode(&fm.out);

    assert!(
        events.iter().any(|e| matches!(
            e,
            SstvEvent::VisDetected {
                mode: SstvMode::Pd120,
                ..
            }
        )),
        "no VisDetected(Pd120); events head: {:?}",
        &events[..events.len().min(3)]
    );

    let img = events
        .iter()
        .find_map(|e| match e {
            SstvEvent::ImageComplete {
                image,
                partial: false,
            } => Some(image),
            _ => None,
        })
        .expect("ImageComplete");
    assert_eq!((img.width, img.height), (W as u32, H as u32));

    let corr = correlation(&reference, &img.pixels);
    assert!(corr > 0.9, "pixel correlation {corr:.4} <= 0.9");
}

#[test]
fn unknown_vis_code_at_12khz_is_surfaced_not_decoded() {
    // 0x42 parses as a well-formed VIS burst but maps to no mode.
    let mut fm = Fm::new(NEXUS_RATE);
    vis_header(&mut fm, 0x42);
    fm.tone(1500.0, 1.0);
    let events = stream_decode(&fm.out);

    assert!(
        events
            .iter()
            .any(|e| matches!(e, SstvEvent::UnknownVis { code: 0x42, .. })),
        "expected UnknownVis(0x42); got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SstvEvent::VisDetected { .. })),
        "unknown code must not start a decode; got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SstvEvent::ImageComplete { .. })),
        "unknown code must not produce an image; got {events:?}"
    );
}

#[test]
fn noise_at_12khz_produces_no_events() {
    // Deterministic LCG white noise, 5 s at ~0.3 RMS.
    let mut state = 0x2026_0717_u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (f64::from(state >> 8) / f64::from(1u32 << 24) - 0.5) as f32
    };
    let audio: Vec<f32> = (0..(NEXUS_RATE as usize * 5)).map(|_| next()).collect();
    let events = stream_decode(&audio);
    assert!(events.is_empty(), "noise produced events: {events:?}");
}
