//! The published timings of every mode, written out longhand — the golden
//! record for #264 and for every mode that was already here.
//!
//! **Why this exists, when `roundtrip.rs` and `tx_loopback.rs` already pass.**
//! Both of those encode with our own encoder and decode with our own decoder,
//! and both read the same [`tempo_sstv::for_mode`] table. A line time that is
//! wrong by a millisecond is wrong on BOTH sides, so the picture round-trips
//! perfectly and the test stays green — while a real station's picture arrives
//! sheared. A self-consistent loop cannot check its own constants; only a
//! written-down number can, and that is what is below.
//!
//! So this file is deliberately dumb: one literal per field per mode, with the
//! source it came from. It goes red when somebody edits `modespec.rs`, which is
//! the point — the edit then has to be defended against the citation rather
//! than against a round trip that would have agreed with anything.
//!
//! **Sources.** Every row is slowrx's `modespec.c`
//! (<https://github.com/windytan/slowrx>, ISC — see the crate `NOTICE.md`),
//! whose own entries carry `// N7CXI, 2000` (JL Barber N7CXI, "Proposal for
//! SSTV Mode Specifications", Dayton 2000) or `// KB4YZ, 1999`. VIS codes are
//! that file's `VISmap`.
//!
//! **The one departure, and it is Wraase's pixel time.** slowrx lists W2180
//! with `PixelTime = 0.734532e-3` and `LineTime = 711.0225e-3`, and those two
//! contradict each other — SC-2 has no channel separator, so a line is exactly
//! `Sync + Porch + 3 × 320 × Pixel`, which comes to 711.1732 ms with slowrx's
//! pixel time: 0.15 ms LONGER than the line time on the next line of the same
//! struct. Solving the identity for the pixel time gives 235.0 ms per channel,
//! i.e. 0.734375 ms, which is what Nexus uses. The difference is a fifth of a
//! pixel across a channel and would be invisible on its own — but emitting the
//! self-contradicting value overruns the line boundary 256 times in a row,
//! which is 38 ms of accumulated skew by the bottom of the picture. The
//! `line_time_identity_holds_for_sequential_modes` unit test in `modespec.rs`
//! is the standing guard; this note is the record of the decision.

#![allow(
    clippy::float_cmp,
    clippy::expect_used,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

use tempo_sstv::{for_mode, ChannelLayout, SstvMode, SyncPosition};

/// One row of the published specification, in milliseconds (the unit the
/// sources are written in) so the literals below can be read straight off
/// `modespec.c` without a unit conversion in the reader's head.
struct Published {
    mode: SstvMode,
    slug: &'static str,
    vis: u8,
    width: u32,
    lines: u32,
    line_ms: f64,
    sync_ms: f64,
    porch_ms: f64,
    pixel_ms: f64,
    septr_ms: f64,
}

/// #264's three modes. Kept separate from the eleven that shipped earlier so
/// the new work is legible as new work; `every_published_row_matches_the_table`
/// below walks both lists.
const NEW_IN_1_13_0: &[Published] = &[
    Published {
        mode: SstvMode::WraaseSc2180,
        slug: "w2180",
        vis: 0x37,
        width: 320,
        lines: 256,
        line_ms: 711.0225,
        sync_ms: 5.5225,
        porch_ms: 0.5,
        // See the module header: derived from the line-time identity, NOT
        // slowrx's self-contradicting 0.734532.
        pixel_ms: 0.734_375,
        septr_ms: 0.0,
    },
    Published {
        mode: SstvMode::PasokonP5,
        slug: "p5",
        vis: 0x72,
        width: 640,
        lines: 496,
        line_ms: 614.065,
        sync_ms: 7.813,
        porch_ms: 1.563,
        pixel_ms: 0.3125,
        septr_ms: 1.563,
    },
    Published {
        mode: SstvMode::PasokonP7,
        slug: "p7",
        vis: 0x73,
        width: 640,
        lines: 496,
        line_ms: 818.747,
        sync_ms: 10.417,
        porch_ms: 2.083,
        pixel_ms: 0.4167,
        septr_ms: 2.083,
    },
];

/// A representative slice of what shipped before #264 — one mode per family,
/// so this file is a check on the whole table rather than only on the new rows,
/// and so a sweeping edit to `modespec.rs` cannot land unnoticed just because
/// it missed the three modes somebody was watching.
const ALREADY_SHIPPED: &[Published] = &[
    Published {
        mode: SstvMode::Pd120,
        slug: "pd120",
        vis: 0x5F,
        width: 640,
        lines: 496,
        line_ms: 508.48,
        sync_ms: 20.0,
        porch_ms: 2.08,
        pixel_ms: 0.19,
        septr_ms: 0.0,
    },
    Published {
        mode: SstvMode::Robot36,
        slug: "robot36",
        vis: 0x08,
        width: 320,
        lines: 240,
        line_ms: 150.0,
        sync_ms: 9.0,
        porch_ms: 3.0,
        pixel_ms: 0.1375,
        septr_ms: 6.0,
    },
    Published {
        mode: SstvMode::Scottie1,
        slug: "scottie1",
        vis: 0x3C,
        width: 320,
        lines: 256,
        line_ms: 428.38,
        sync_ms: 9.0,
        porch_ms: 1.5,
        pixel_ms: 0.432,
        septr_ms: 1.5,
    },
    Published {
        mode: SstvMode::Martin1,
        slug: "martin1",
        vis: 0x2C,
        width: 320,
        lines: 256,
        line_ms: 446.446,
        sync_ms: 4.862,
        porch_ms: 0.572,
        pixel_ms: 0.4576,
        septr_ms: 0.572,
    },
];

/// Millisecond literals hold at most six significant figures here, so compare
/// at a nanosecond — far finer than any timing that matters, coarse enough that
/// the decimal-to-binary round trip of a literal like `818.747` cannot trip it.
const TOL_SEC: f64 = 1e-12;

fn check(p: &Published) {
    let s = for_mode(p.mode);
    assert_eq!(s.short_name, p.slug, "{:?}: short_name", p.mode);
    assert_eq!(s.vis_code, p.vis, "{:?}: VIS code", p.mode);
    assert_eq!(s.line_pixels, p.width, "{:?}: line_pixels", p.mode);
    assert_eq!(s.image_lines, p.lines, "{:?}: image_lines", p.mode);
    for (name, got, want_ms) in [
        ("line_seconds", s.line_seconds, p.line_ms),
        ("sync_seconds", s.sync_seconds, p.sync_ms),
        ("porch_seconds", s.porch_seconds, p.porch_ms),
        ("pixel_seconds", s.pixel_seconds, p.pixel_ms),
        ("septr_seconds", s.septr_seconds, p.septr_ms),
    ] {
        let want = want_ms / 1000.0;
        assert!(
            (got - want).abs() < TOL_SEC,
            "{:?}: {name} is {:.9} s, the published figure is {:.9} s ({want_ms} ms). \
             If the table is right, the published figure in this file is what needs \
             the citation — do not just move the number.",
            p.mode,
            got,
            want,
        );
    }
}

#[test]
fn every_published_row_matches_the_table() {
    for p in NEW_IN_1_13_0.iter().chain(ALREADY_SHIPPED) {
        check(p);
    }
}

/// The identity that decides whether a picture comes out straight, restated
/// here against the LITERALS above rather than against the table — so it holds
/// even if somebody edits `modespec.rs` and the unit test with it.
#[test]
fn published_line_times_account_for_their_own_content() {
    for p in NEW_IN_1_13_0 {
        let chan = f64::from(p.width) * p.pixel_ms;
        let two_septr = p.sync_ms + p.porch_ms + 3.0 * chan + 2.0 * p.septr_ms;
        let three_septr = two_septr + p.septr_ms;
        assert!(
            two_septr <= p.line_ms + 1e-6,
            "{:?}: sync+porch+3 channels+2 separators is {two_septr:.4} ms, longer than \
             the published line time {:.4} ms — the picture would shear",
            p.mode,
            p.line_ms,
        );
        assert!(
            three_septr >= p.line_ms - 1e-6,
            "{:?}: even with a third separator the line only accounts for {three_septr:.4} ms \
             of its published {:.4} ms — a channel looks missing",
            p.mode,
            p.line_ms,
        );
    }
}

/// #264's three modes are sequential, R→G→B, sync at line start. Stated
/// separately from the timings because getting this wrong does not shear a
/// picture — it swaps red and blue, which reads as a colour problem in the
/// radio rather than a bug here.
#[test]
fn the_new_modes_are_rgb_ordered_with_sync_at_line_start() {
    for p in NEW_IN_1_13_0 {
        let s = for_mode(p.mode);
        assert_eq!(
            s.channel_layout,
            ChannelLayout::SequentialRgb,
            "{:?}: slowrx gives these ColorEnc = RGB, not GBR",
            p.mode
        );
        assert_eq!(s.sync_position, SyncPosition::LineStart, "{:?}", p.mode);
    }
}

/// The airtimes, spelled out. A reader can check these against the mode NAMES:
/// SC-2 *180* is about three minutes, P*5* about five, P*7* about seven — the
/// modes are named for their own length, which is the cheapest sanity check
/// there is on a transcribed line time.
#[test]
fn airtimes_match_the_names_the_modes_carry() {
    let secs = |m| for_mode(m).airtime_seconds();
    let w = secs(SstvMode::WraaseSc2180);
    assert!(
        (178.0..186.0).contains(&w),
        "Wraase SC-2 180 should be ≈180 s, got {w:.1}"
    );
    let p5 = secs(SstvMode::PasokonP5);
    assert!(
        (295.0..312.0).contains(&p5),
        "Pasokon P5 should be ≈5 min, got {p5:.1}"
    );
    let p7 = secs(SstvMode::PasokonP7);
    assert!(
        (395.0..415.0).contains(&p7),
        "Pasokon P7 should be ≈7 min, got {p7:.1}"
    );
}

// ---------------------------------------------------------------------------
// CHANNEL ORDER, measured off the transmitted waveform.
//
// This is here because the round trip cannot do it. `roundtrip.rs` and
// `tx_loopback.rs` encode and decode through the SAME `channel_colours`
// mapping, so reversing that mapping reverses both sides and the picture comes
// back perfect — verified by experiment while #264 was being written: flipping
// `SequentialRgb` to `[1, 2, 0]` left `wraase_sc2_180_roundtrip` GREEN. A
// symmetric loop cannot see its own asymmetry.
//
// So: encode a picture that is pure red, and look at the AUDIO. Whichever of
// the three channel slots comes out at the white tone is the one carrying red.
// Frequency is estimated by counting zero crossings, which needs nothing from
// inside the crate — 1500 Hz (black) and 2300 Hz (white) are 800 Hz apart and
// a slot is tens of milliseconds long, so the two are never in doubt.
// ---------------------------------------------------------------------------

use tempo_sstv::{encode_image, SourceImage};

/// Working rate — `encode_image` synthesizes at whatever rate it is given.
const RATE: u32 = 11_025;
/// Pre-silence + the calibration/VIS header, seconds. Both are constants of
/// `encode.rs` (0.100 s and 0.910 s); restated rather than imported because
/// this file's whole job is to be an outside opinion.
const BEFORE_LINE_ZERO_SECS: f64 = 0.100 + 0.910;

/// Dominant frequency of `w` in Hz, from its zero-crossing rate.
#[allow(clippy::cast_precision_loss)]
fn freq_of(w: &[f32], rate: u32) -> f64 {
    let crossings = w
        .windows(2)
        .filter(|p| (p[0] < 0.0) != (p[1] < 0.0))
        .count();
    crossings as f64 * f64::from(rate) / (2.0 * (w.len() as f64 - 1.0))
}

/// The three channel-slot frequencies of line 0, for an image painted entirely
/// in `colour`. Each slot is sampled across its middle 60 % so neither the
/// separator either side nor the FM transition at the boundary is included.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn slot_frequencies(mode: SstvMode, colour: [u8; 3]) -> [f64; 3] {
    let spec = for_mode(mode);
    let img = SourceImage {
        width: spec.line_pixels,
        height: spec.image_lines,
        rgb: vec![colour; (spec.line_pixels * spec.image_lines) as usize],
    };
    let audio = encode_image(mode, &img, RATE).expect("encode");

    let chan = f64::from(spec.line_pixels) * spec.pixel_seconds;
    let first = BEFORE_LINE_ZERO_SECS + spec.sync_seconds + spec.porch_seconds;
    let at = |secs: f64| (secs * f64::from(RATE)).round() as usize;
    let mut out = [0.0_f64; 3];
    for (slot, f) in out.iter_mut().enumerate() {
        let start = first + slot as f64 * (chan + spec.septr_seconds);
        let lo = at(start + 0.2 * chan);
        let hi = at(start + 0.8 * chan);
        *f = freq_of(&audio[lo..hi], RATE);
    }
    out
}

/// Is `hz` the white tone (2300 Hz) rather than the black one (1500 Hz)?
fn is_white(hz: f64) -> bool {
    (hz - 2300.0).abs() < 100.0
}
/// Is `hz` the black tone (1500 Hz)?
fn is_black(hz: f64) -> bool {
    (hz - 1500.0).abs() < 100.0
}

/// #264 — Wraase SC-2 and Pasokon send RED first (slowrx `ColorEnc = RGB`).
#[test]
fn wraase_and_pasokon_transmit_red_green_blue_in_that_order() {
    for mode in [
        SstvMode::WraaseSc2180,
        SstvMode::PasokonP5,
        SstvMode::PasokonP7,
    ] {
        for (slot, colour, what) in [
            (0_usize, [255_u8, 0, 0], "red"),
            (1, [0, 255, 0], "green"),
            (2, [0, 0, 255], "blue"),
        ] {
            let f = slot_frequencies(mode, colour);
            assert!(
                is_white(f[slot]),
                "{mode:?}: an all-{what} picture should put the white tone in channel slot \
                 {slot}; slot tones were {f:.0?} Hz",
            );
            for (other, hz) in f.iter().enumerate() {
                if other != slot {
                    assert!(
                        is_black(*hz),
                        "{mode:?}: an all-{what} picture left {hz:.0} Hz in slot {other}, \
                         which should be black — the channel order is wrong",
                    );
                }
            }
        }
    }
}

/// The counterpart that proves the check above discriminates: Martin sends
/// GREEN first (slowrx `ColorEnc = GBR`), so the same measurement on the same
/// geometry must come out differently. Without this, a measurement that always
/// answered "slot 0" would pass the test above and mean nothing.
#[test]
fn martin_transmits_green_blue_red_in_that_order() {
    let f_red = slot_frequencies(SstvMode::Martin1, [255, 0, 0]);
    let f_green = slot_frequencies(SstvMode::Martin1, [0, 255, 0]);
    let f_blue = slot_frequencies(SstvMode::Martin1, [0, 0, 255]);
    assert!(
        is_white(f_green[0]) && is_black(f_green[1]) && is_black(f_green[2]),
        "Martin 1 sends green first; slot tones for an all-green picture were {f_green:.0?}",
    );
    assert!(
        is_white(f_blue[1]),
        "Martin 1 sends blue second; {f_blue:.0?}"
    );
    assert!(is_white(f_red[2]), "Martin 1 sends red last; {f_red:.0?}");
}
