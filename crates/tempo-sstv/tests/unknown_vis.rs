//! `SstvDecoder` must surface an unrecognized-but-well-formed VIS burst as
//! `SstvEvent::UnknownVis` (rather than dropping it silently) and then keep
//! detecting subsequent valid VIS bursts — i.e. the unknown-code path reseeds
//! the VIS detector per the `#40` re-anchor contract (audit #89: A1 + C1).

#![cfg(feature = "test-support")]
#![allow(clippy::expect_used, clippy::cast_possible_truncation, clippy::panic)]

use tempo_sstv::{SstvDecoder, SstvEvent, SstvMode, WORKING_SAMPLE_RATE_HZ};

/// 0x01 is a valid 7-bit VIS code (parity 1) that maps to no SSTV mode.
const UNKNOWN_CODE: u8 = 0x01;
/// 0x5F == PD120.
const PD120_CODE: u8 = 0x5F;

#[test]
fn decoder_emits_unknown_vis_then_recovers() {
    // burst 1: a well-formed VIS for an unknown code.
    // burst 2: a well-formed VIS for PD120, immediately following.
    // trailing zeros so the resampler's FIR group delay still yields a full
    // set of stop-bit windows for burst 2.
    let mut audio = tempo_sstv::__test_support::vis::synth_vis(UNKNOWN_CODE, 0.0);
    audio.extend(tempo_sstv::__test_support::vis::synth_vis(PD120_CODE, 0.0));
    audio.extend(std::iter::repeat_n(0.0_f32, 512));

    let mut decoder = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder construct");
    let events = decoder.process(&audio);

    // Expect: UnknownVis { code: 0x01, .. } then VisDetected { mode: Pd120, .. }.
    let unknown_at = events
        .iter()
        .position(|e| matches!(e, SstvEvent::UnknownVis { code, .. } if *code == UNKNOWN_CODE))
        .unwrap_or_else(|| panic!("no UnknownVis event for 0x{UNKNOWN_CODE:02x}; got {events:?}"));
    let detected_at = events
        .iter()
        .position(|e| {
            matches!(
                e,
                SstvEvent::VisDetected {
                    mode: SstvMode::Pd120,
                    ..
                }
            )
        })
        .unwrap_or_else(|| {
            panic!("no VisDetected for PD120 after the unknown burst; got {events:?}")
        });
    assert!(
        unknown_at < detected_at,
        "UnknownVis should precede the recovered VisDetected; got {events:?}"
    );

    // The recovered detection's sample_offset should be a sane working-rate
    // index (non-zero, and not wildly past the end of the fed audio). Exact
    // semantics post-restart (relative-to-restart vs absolute) are tracked
    // separately; this is only a gross-corruption guard.
    if let SstvEvent::VisDetected { sample_offset, .. } = &events[detected_at] {
        assert!(
            *sample_offset > 0 && (*sample_offset as usize) <= audio.len(),
            "VisDetected.sample_offset = {sample_offset} out of [1, {}]",
            audio.len()
        );
    }
}

/// #264 — Pasokon P3 (VIS 0x71) is the nearest neighbour of the two Pasokon
/// modes this build DOES decode, one code below P5. That makes it the exact
/// shape of mistake worth pinning: a table that resolved a code it has no
/// timings for would decode a P3 picture with P5's line time and paint a
/// shear, and nothing would say so. It must come back as an unknown code, and
/// the decoder must still find the P5 burst that follows.
#[test]
fn pasokon_p3_is_reported_unknown_and_p5_still_decodes() {
    /// Pasokon P3 — a real VIS code in the same block, deliberately not
    /// implemented (slowrx `VISmap` 0x71).
    const P3_CODE: u8 = 0x71;
    /// Pasokon P5 — implemented (slowrx `VISmap` 0x72).
    const P5_CODE: u8 = 0x72;

    let mut audio = tempo_sstv::__test_support::vis::synth_vis(P3_CODE, 0.0);
    audio.extend(tempo_sstv::__test_support::vis::synth_vis(P5_CODE, 0.0));
    audio.extend(std::iter::repeat_n(0.0_f32, 512));

    let mut decoder = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder construct");
    let events = decoder.process(&audio);

    let unknown_at = events
        .iter()
        .position(|e| matches!(e, SstvEvent::UnknownVis { code, .. } if *code == P3_CODE))
        .unwrap_or_else(|| {
            panic!("P3 (0x{P3_CODE:02x}) was not reported as an unknown VIS; got {events:?}")
        });
    let detected_at = events
        .iter()
        .position(|e| {
            matches!(
                e,
                SstvEvent::VisDetected {
                    mode: SstvMode::PasokonP5,
                    ..
                }
            )
        })
        .unwrap_or_else(|| panic!("no VisDetected for Pasokon P5 after P3; got {events:?}"));
    assert!(unknown_at < detected_at, "ordering: {events:?}");
}

/// Wraase SC-2 180's own neighbour check. 0x3F is SC-2 120 in slowrx's map —
/// same family, not implemented here — so it must report unknown rather than
/// resolve to the 180 that sits four codes away.
#[test]
fn wraase_sc2_120_is_reported_unknown_and_sc2_180_still_decodes() {
    const SC2_120_CODE: u8 = 0x3F;
    const SC2_180_CODE: u8 = 0x37;

    let mut audio = tempo_sstv::__test_support::vis::synth_vis(SC2_120_CODE, 0.0);
    audio.extend(tempo_sstv::__test_support::vis::synth_vis(
        SC2_180_CODE,
        0.0,
    ));
    audio.extend(std::iter::repeat_n(0.0_f32, 512));

    let mut decoder = SstvDecoder::new(WORKING_SAMPLE_RATE_HZ).expect("decoder construct");
    let events = decoder.process(&audio);

    assert!(
        events
            .iter()
            .any(|e| matches!(e, SstvEvent::UnknownVis { code, .. } if *code == SC2_120_CODE)),
        "SC-2 120 (0x{SC2_120_CODE:02x}) was not reported as unknown; got {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            SstvEvent::VisDetected {
                mode: SstvMode::WraaseSc2180,
                ..
            }
        )),
        "no VisDetected for Wraase SC-2 180 after the unknown burst; got {events:?}"
    );
}
