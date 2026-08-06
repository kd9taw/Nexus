//! Negative real-radio regression — [`tempo_sstv::SstvDecoder`] must not
//! false-positive on non-SSTV audio.
//!
//! **Origin.** An ISS Zarya capture on 2026-05-04 22:38:27 UTC turned
//! out to carry no SSTV (Zarya transmits SSTV only during ARISS-
//! scheduled events). 0 VIS codes detected on the 25-minute recording;
//! a separate spectrum check confirmed the audio sat in the 247–563 Hz
//! band with no energy in SSTV's 1500–2300 Hz pixel band. We could not
//! commit the recording itself (`/tests/fixtures` is gitignored per the
//! project's "no third-party-licensed fixtures in the repo" convention,
//! and the redistribution-license story for community-shared captures
//! is unclear), so the regression coverage is reproduced with two
//! synthetic non-SSTV audio buffers: white noise at the Zarya
//! recording's measured RMS level (~0.3), and pure silence.
//!
//! Both must produce zero `SstvEvent::ImageComplete` events of ANY kind
//! without panicking. This is the no-signal counterpart to the synthetic
//! round-trip suite in `tests/roundtrip.rs` — both must hold for any
//! release.
//!
//! **Re-baselined 2026-08-05 (blind mid-picture decode).** The filter
//! used to be `ImageComplete { partial: false, .. }`, with `partial:
//! false` as a *literal sub-pattern* — so an `ImageComplete { partial:
//! true }` did not match, was not counted, and left both assertions
//! green. That was a hole precisely where this file's whole purpose
//! lies: the no-VIS decode path emits partials, and a decoder that
//! hallucinated pictures out of the Zarya noise and stamped them
//! `partial: true` would have passed the test that exists *because of
//! that incident*. The guard now counts every `ImageComplete`. It passed
//! before the blind path was written and passes after — it costs
//! nothing and closes the hole rather than leaving the feature to fall
//! into it. Adversarial coverage beyond noise and silence (SSB speech, a
//! CW pileup keyed on 1200 Hz, a bare periodic sync train) lives in
//! `tests/partial_decode.rs`.

#![allow(clippy::expect_used, clippy::cast_precision_loss)]

use tempo_sstv::{SstvDecoder, SstvEvent};

/// Sample rate matching what a typical SDR / file capture produces;
/// also exercises the resampler since `WORKING_SAMPLE_RATE_HZ` = `11_025`.
const SAMPLE_RATE_HZ: u32 = 48_000;
const DURATION_SEC: u32 = 10;

/// Deterministic linear-congruential generator. Numerical Recipes'
/// "Quick and Dirty" constants — sufficient for white-noise regression
/// purposes (no cryptographic strength needed). A fixed seed makes the
/// test reproducible across runs / hosts.
struct Lcg(u32);

impl Lcg {
    fn new(seed: u32) -> Self {
        Self(seed)
    }

    fn next_unit(&mut self) -> f32 {
        // state ← state · 1664525 + 1013904223 (mod 2³²)
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // Map u32 → [-0.5, 0.5).
        (self.0 as f32 / u32::MAX as f32) - 0.5
    }
}

/// Count every emitted image, partial or not. Do NOT narrow this to
/// `partial: false` — see the re-baselining note in the module header.
fn count_complete_images(events: &[SstvEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, SstvEvent::ImageComplete { .. }))
        .count()
}

#[test]
fn decoder_no_vis_on_white_noise() {
    let n = (SAMPLE_RATE_HZ * DURATION_SEC) as usize;
    let mut rng = Lcg::new(0x5EED_5EED);
    // ~0.3 RMS — matches the measured level of the Zarya 2026-05-04
    // recording, so the decoder operates on realistic signal-strength
    // input rather than near-zero amplitudes.
    //
    // `next_unit()` is uniform on `[-0.5, 0.5]`, whose RMS is
    // `1/sqrt(12) ≈ 0.2887`. Scaling by `target_rms * sqrt(12)` makes
    // the resulting noise hit the target RMS exactly (modulo
    // sample-count statistical jitter).
    let noise_scale = 0.3_f32 * 12.0_f32.sqrt();
    let audio: Vec<f32> = (0..n).map(|_| rng.next_unit() * noise_scale).collect();

    let mut decoder = SstvDecoder::new(SAMPLE_RATE_HZ).expect("decoder construct");
    let events = decoder.process(&audio);

    let n_complete = count_complete_images(&events);
    assert_eq!(
        n_complete, 0,
        "decoder false-positive: emitted {n_complete} ImageComplete \
         event(s) on 10 s of white noise",
    );
}

#[test]
fn decoder_no_vis_on_silence() {
    let n = (SAMPLE_RATE_HZ * DURATION_SEC) as usize;
    let audio = vec![0.0_f32; n];

    let mut decoder = SstvDecoder::new(SAMPLE_RATE_HZ).expect("decoder construct");
    let events = decoder.process(&audio);

    let n_complete = count_complete_images(&events);
    assert_eq!(
        n_complete, 0,
        "decoder false-positive: emitted {n_complete} ImageComplete \
         event(s) on 10 s of pure silence",
    );
}
