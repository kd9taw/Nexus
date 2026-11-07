//! How often does [`tempo_fast::decode_frame`] copy a clean FT1 frame? Measured over many noise
//! seeds, per message and per time offset, because a single seed says almost nothing.
//!
//! **Why this file exists.** `every_callsign_class_pair_runs_to_73_and_logs` once drove the
//! modem at one fixed seed and passed 20 of 60 seeds: a real miss rate looked like a flaky test
//! until someone counted. The rate depends on three things a fixed-seed test cannot see:
//!
//! * **The message.** The same receiver could copy one over every time and lose another one
//!   frame in four at the same SNR, because the channel symbols differ.
//! * **The time offset.** 2996 samples is the harness's `ON_TIME_OFFSET`; 3600 is the real
//!   0.300 s lead-in; the others sit at different fractions of a downsampled sample.
//! * **The toolchain.** Each compiler reshuffles which seeds lose, without moving the rate. A
//!   threshold over many seeds is stable where one seed's verdict is not.
//!
//! The misses being guarded are not sensitivity: they stay flat or rise with SNR, so the
//! gate runs where a frame has no excuse (+15 dB). A wrong decode is counted separately, and
//! any at all fails: a sequencer can survive a miss but it will act on a false copy.
//!
//! The gate runs in about a minute. The full table takes tens of minutes and is a measuring
//! instrument: `cargo test -p tempo-core --test ft1_decode_rate measure -- --ignored --nocapture`

use std::collections::BTreeMap;
use tempo_core::channel::{to_i16, VirtualAir};
use tempo_core::{tempo_fast, tx};

type Case = (&'static str, &'static str, &'static str);

/// (message, receiver's call, sender's call): every message shape a QSO puts on the air, with
/// the calls `run_loopback_qso` would pass for a-priori decoding.
const MESSAGES: &[Case] = &[
    ("CQ W9XYZ JN18", "KD9TAW", "W9XYZ"),
    ("CQ K2DEF FN31", "KD9TAW", "K2DEF"),
    ("W9XYZ KD9TAW -12", "W9XYZ", "KD9TAW"),
    ("KD9TAW W9XYZ R-05", "KD9TAW", "W9XYZ"),
    ("KD9TAW W9XYZ RRR", "KD9TAW", "W9XYZ"),
    ("W9XYZ KD9TAW 73", "W9XYZ", "KD9TAW"),
    ("CQ PJ4/K1ABC", "YW18FIFA", "PJ4/K1ABC"),
    ("CQ YW18FIFA", "KD9TAW", "YW18FIFA"),
    ("CQ 3D2/W1AW", "KD9TAW", "3D2/W1AW"),
    ("<PJ4/K1ABC> YW18FIFA", "PJ4/K1ABC", "YW18FIFA"),
    ("<YW18FIFA> PJ4/K1ABC", "YW18FIFA", "PJ4/K1ABC"),
    ("<W9XYZ> YW18FIFA", "W9XYZ", "YW18FIFA"),
    ("<PJ4/K1ABC> YW18FIFA 73", "PJ4/K1ABC", "YW18FIFA"),
    ("<YW18FIFA> PJ4/K1ABC 73", "YW18FIFA", "PJ4/K1ABC"),
    ("<W9XYZ> YW18FIFA 73", "W9XYZ", "YW18FIFA"),
    ("<YW18FIFA> PJ4/K1ABC RRR", "YW18FIFA", "PJ4/K1ABC"),
    ("<PJ4/K1ABC> YW18FIFA RRR", "PJ4/K1ABC", "YW18FIFA"),
    ("<W9XYZ> YW18FIFA RRR", "W9XYZ", "YW18FIFA"),
];

/// Time offsets in 12 kHz samples for the gate: the harness's `ON_TIME_OFFSET`, the worst
/// fraction measured before the fix, and the real 0.300 s lead-in.
const GATE_OFFSETS: [usize; 3] = [2996, 3010, 3600];
/// The instrument adds two more positions on the old 54-sample grid.
const ALL_OFFSETS: [usize; 5] = [2970, 2996, 3010, 3600, 3627];

/// Seeds per (message, offset) at +15 dB: 540 frames. With [`LOW_SEEDS`] the gate runs about a
/// minute (the modem takes a global lock, so frames run one at a time).
const GATE_SEEDS: u64 = 10;
/// Seeds per (message, offset) at -10 dB: 216 frames.
const LOW_SEEDS: u64 = 4;

// Limits are set from `measure_decode_rate` on 100 seeds per (message, offset), well above the
// fixed decoder and below the broken one, so a compiler's reshuffle of which seeds lose cannot
// flip the verdict.
//
//                         broken decoder      fixed decoder
//   +15 dB, per offset    6.7 / 15.6 / 10.9%  0.5 - 0.9%
//   +15 dB, per message   worst 23%           worst 3.3%
//   -10 dB, per offset    41 - 45%            8.0 - 8.8%
const MAX_MISS_PCT_15DB_PER_OFFSET: f64 = 4.0;
const MAX_MISS_PCT_15DB_PER_MESSAGE: f64 = 14.0;
const MAX_MISS_PCT_MINUS_10DB_PER_OFFSET: f64 = 25.0;
/// Wrong decodes tolerated across the -10 dB frames: about 3 in 10800 were measured, near
/// where CRC-valid noise lives, so one is not an alarm. At +15 dB there is no such excuse.
const MAX_WRONG_MINUS_10DB: u32 = 1;

/// splitmix64, so consecutive seeds are decorrelated: the LCG in `Awgn` starts close to where it
/// started for a neighboring small seed.
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Copied / missed / wrong text, for whatever slice of the sweep a key selects.
#[derive(Default, Clone, Copy)]
struct Tally {
    frames: u32,
    missed: u32,
    wrong: u32,
}

impl Tally {
    fn miss_pct(&self) -> f64 {
        100.0 * f64::from(self.missed) / f64::from(self.frames.max(1))
    }
}

/// One frame through the same path as `run_loopback_qso`, with the HARQ buffers cleared so no
/// frame can be rescued by the one before it. Returns (copied, wrong decodes).
fn one_frame(wave: &[f32], case: Case, offset: usize, snr: f32, seed: u64) -> (bool, u32) {
    let (msg, rx, tx) = case;
    let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, mix(seed));
    let iwave = to_i16(&air.receive(wave, offset, snr));
    tempo_fast::harq_reset();
    let decodes = tempo_fast::decode_frame(&iwave, 200, 2900, 3, rx, tx, 0, 0);
    let copied = decodes.iter().any(|d| d.message == msg);
    let wrong = decodes.iter().filter(|d| d.message != msg).count() as u32;
    (copied, wrong)
}

/// Run every message at every offset for `seeds` seeds and return per-offset and per-message
/// tallies.
fn sweep(
    snr: f32,
    offsets: &[usize],
    seeds: u64,
) -> (BTreeMap<usize, Tally>, BTreeMap<&'static str, Tally>) {
    let mut by_offset: BTreeMap<usize, Tally> = BTreeMap::new();
    let mut by_message: BTreeMap<&'static str, Tally> = BTreeMap::new();
    for &case in MESSAGES {
        let msg = case.0;
        let frame = tx::build(msg, tempo_fast::SAMPLE_RATE, 1500.0);
        for &offset in offsets {
            for seed in 0..seeds {
                let (copied, wrong) = one_frame(&frame.wave, case, offset, snr, seed);
                for t in [
                    by_offset.entry(offset).or_default(),
                    by_message.entry(msg).or_default(),
                ] {
                    t.frames += 1;
                    t.missed += u32::from(!copied);
                    t.wrong += wrong;
                }
            }
        }
    }
    (by_offset, by_message)
}

fn print_table(snr: f32, by_offset: &BTreeMap<usize, Tally>, by_message: &BTreeMap<&str, Tally>) {
    eprintln!("--- {snr:+} dB ---");
    for (offset, t) in by_offset {
        eprintln!(
            "offset {offset:>5}  miss {:5.1}%  wrong {}  ({} frames)",
            t.miss_pct(),
            t.wrong,
            t.frames
        );
    }
    for (msg, t) in by_message {
        eprintln!(
            "{msg:<26} miss {:5.1}%  wrong {}  ({} frames)",
            t.miss_pct(),
            t.wrong,
            t.frames
        );
    }
}

/// The gate: at +15 dB no offset and no message may lose more than a small share of frames and
/// no frame may produce the wrong text; at -10 dB the decoder must still copy most frames.
#[test]
#[ignore = "fails until the FT1 miss floor is fixed: see the module header for the command"]
fn a_clean_frame_is_copied_at_every_offset_and_for_every_message() {
    let (by_offset, by_message) = sweep(15.0, &GATE_OFFSETS, GATE_SEEDS);
    print_table(15.0, &by_offset, &by_message);
    let (low_by_offset, low_by_message) = sweep(-10.0, &GATE_OFFSETS, LOW_SEEDS);
    print_table(-10.0, &low_by_offset, &low_by_message);

    for (offset, t) in &by_offset {
        assert!(
            t.miss_pct() <= MAX_MISS_PCT_15DB_PER_OFFSET,
            "offset {offset}: missed {:.1}% of {} frames at +15 dB",
            t.miss_pct(),
            t.frames
        );
        assert_eq!(t.wrong, 0, "offset {offset}: wrong text at +15 dB");
    }
    for (msg, t) in &by_message {
        assert!(
            t.miss_pct() <= MAX_MISS_PCT_15DB_PER_MESSAGE,
            "{msg}: missed {:.1}% of {} frames at +15 dB",
            t.miss_pct(),
            t.frames
        );
    }
    for (offset, t) in &low_by_offset {
        assert!(
            t.miss_pct() <= MAX_MISS_PCT_MINUS_10DB_PER_OFFSET,
            "offset {offset}: missed {:.1}% of {} frames at -10 dB",
            t.miss_pct(),
            t.frames
        );
    }
    let wrong: u32 = low_by_offset.values().map(|t| t.wrong).sum();
    assert!(
        wrong <= MAX_WRONG_MINUS_10DB,
        "{wrong} wrong decodes at -10 dB"
    );
}

/// The instrument: the same sweep with enough seeds to read a rate from, across the SNRs where the
/// curve has its features.
#[test]
#[ignore = "a measurement that takes minutes: see the module header for the command"]
fn measure_decode_rate() {
    for snr in [40.0, 15.0, 5.0, -5.0, -10.0, -12.0] {
        let (by_offset, by_message) = sweep(snr, &ALL_OFFSETS, 100);
        print_table(snr, &by_offset, &by_message);
    }
}
