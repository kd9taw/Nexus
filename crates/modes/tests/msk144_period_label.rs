//! MSK144 decodes in a period labelled by the wall clock (#579).
//!
//! The radio loop numbers its periods from the Unix epoch, so the `frame_time_ms` it hands an
//! MSK144 decode is today's date in milliseconds. `mskrtd` prints the period label into its
//! output line as `i6.6` (`mskrtd.f90`, format 1021), WSJT-X's six-digit UTC `hhmmss` — a
//! ten-digit label printed there is six asterisks, the line no longer reads back, and the
//! decode is lost.

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};

const MSG: &str = "CQ K1ABC FN42";

/// A 15 s period holding one meteor ping: three 72 ms frames of `MSG` at 1500 Hz, 4 s into the
/// period, over light noise.
fn one_ping() -> Vec<i16> {
    let kind = ModeKind::MSK144_15;
    let mode = modes::make_mode(kind);
    let over = mode.gen_wave(&mode.encode(MSG), 12_000.0, 1_500.0);
    let ping = &over[..3 * 864];
    let mut x = vec![0f32; kind.frame_samples()];
    let mut s = 0x5EED_0579_u64;
    for v in x.iter_mut() {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *v = ((s >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.5;
    }
    for (v, p) in x[48_000..].iter_mut().zip(ping) {
        *v += p;
    }
    x.iter().map(|v| (v * 1000.0).round() as i16).collect()
}

fn decoded(frame_time_ms: i64) -> Vec<String> {
    let iwave = one_ping();
    let mut src = NativeSource::from_kind(ModeKind::MSK144_15);
    let req = DecodeRequest {
        frame_time_ms,
        ..DecodeRequest::full_band(&iwave)
    };
    src.decode(&req)
        .into_iter()
        .map(|d| d.message.trim().to_string())
        .collect()
}

/// ⭐ The radio loop's label: 2025-11-27 12:00:15 UTC as Unix milliseconds, the boundary that
/// ends the period. The ping decodes.
#[test]
fn a_ping_decodes_in_a_period_labelled_by_the_wall_clock() {
    let heard = decoded(1_764_244_815_000);
    assert!(heard.iter().any(|m| m == MSG), "heard {heard:?}");
}

/// Control: the same ping under a small label, as every test before this one used.
#[test]
fn a_ping_decodes_in_a_period_with_a_small_label() {
    let heard = decoded(15_000);
    assert!(heard.iter().any(|m| m == MSG), "heard {heard:?}");
}
