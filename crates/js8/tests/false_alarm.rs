//! False-alarm gate: ZERO decodes on pure AWGN at every JS8 speed, stock
//! parameters, depth 3. This is the role of `tests/ft8_false_alarm.c`, but
//! inside `cargo test` so CI actually runs it (none of the C harnesses do —
//! ci.yml never references false_alarm). It rides `cargo test --workspace`.
//!
//! WHY it matters more here than for FT8: JS8's acceptance is CRC-12 (1 in
//! 4096) over four BP passes per candidate, not FT8's CRC-14 + OSD distance —
//! a false decode is a garbled callsign or an unrequested autoreply on the
//! air. Any nonzero count fails the gate, regardless of yield elsewhere
//! (the parity lab's revert rule).
//!
//! Positive control (feedback-negative-results-need-a-positive-control): the
//! same harness with a −10 dB frame injected MUST decode, otherwise "zero
//! decodes" only proves the decoder is dead.

mod common;

use js8::phy::{decode, encode_word, modulate};
use js8::proto::alphabet::sixbit_from_str;
use js8::{DecodeParams, Speed, Word87};

/// One period of int16 audio: each `(word, f0, snr_db)` slot-positioned by
/// `modulate` and scaled in the 2500 Hz convention, plus unit AWGN, ×100
/// (the crates/ft8/tests/decode_parity.rs arithmetic; `common::Rng` is B2's).
fn window(signals: &[(Word87, f32, f32)], speed: Speed, seed: u64) -> Vec<i16> {
    let n = speed.period_s() as usize * 12_000;
    let bw_ratio = 2500.0f32 / 6000.0;
    let mut dd = vec![0f32; n];
    for (w, f0, snr) in signals {
        let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr);
        let wave = modulate(&encode_word(w, speed), speed, *f0, 12_000.0);
        for (i, &x) in wave.iter().enumerate() {
            if i < n {
                dd[i] += sig * x;
            }
        }
    }
    let mut rng = common::Rng(seed);
    dd.iter()
        .map(|&s| (((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
        .collect()
}

fn word(chars12: &str) -> Word87 {
    let c = sixbit_from_str(chars12).expect("12 sixbit chars");
    Word87::new(
        js8::Payload72::from_chars12(c),
        js8::I3 {
            first: true,
            last: true,
            data: false,
        },
    )
}

const SEEDS: u64 = 8;

#[test]
fn pure_noise_never_decodes_at_any_speed() {
    let params = DecodeParams::stock();
    for speed in Speed::ALL {
        for seed in 1..=SEEDS {
            let iw = window(&[], speed, 0xA11CE ^ seed);
            let d = decode(&iw, speed, &params);
            assert!(
                d.is_empty(),
                "{speed:?} seed {seed}: {} false decode(s): {:?}",
                d.len(),
                d.iter()
                    .map(|x| (x.freq_hz, x.dt_s, x.snr_db, x.nharderrors))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn positive_control_a_real_frame_decodes_in_the_same_harness() {
    let params = DecodeParams::stock();
    for speed in Speed::ALL {
        let w = word("KD9TAWEN52ab");
        let iw = window(&[(w, 1500.0, -10.0)], speed, 0xA11CE ^ 1);
        let d = decode(&iw, speed, &params);
        assert!(
            d.iter().any(|x| x.word == w),
            "{speed:?}: control frame not decoded"
        );
    }
}
