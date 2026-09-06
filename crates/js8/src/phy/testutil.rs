//! Test-only synthesis helpers shared by the phy unit tests: a deterministic
//! LCG + Box–Muller AWGN source (no `rand` dep — the ft8/ft4/ft2 parity tests
//! use the same generator) and `synth_window`, one period of int16 audio in
//! WSJT-X's 2500 Hz SNR convention. WHY here and not in tests/: the colocated
//! module tests (sync, downsample, demod, subtract, decoder) all need the same
//! window and cannot see an integration-test crate; tests/common/mod.rs (B2)
//! carries the integration-test twin (`common::Rng`, `common::place_in_period`).

use super::frame::{Payload72, Word87, I3};
use super::speed::Speed;
use super::{encode_word, modulate};

/// Numerical Recipes LCG + Box–Muller (identical to crates/ft8/tests/decode_parity.rs:30-46).
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    pub(crate) fn gauss(&mut self) -> f32 {
        let u1 = (self.next_f64() + 1e-12).min(1.0);
        let u2 = self.next_f64();
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
}

/// One full period (`period_s * 12000` samples) of int16 audio: every
/// `(word, f0_hz, snr_db)` modulated slot-positioned (the wave already carries
/// the start-delay silence, so the frame starts at JS8Call's ASTART and its
/// dt is 0) and scaled to `snr_db` in the 2500 Hz convention
/// (`sig = sqrt(2 * 2500/6000) * 10^(snr/20)` against unit-variance noise,
/// exactly `tests/ft8_acquire.c` / crates/ft8/tests/decode_parity.rs:48-66),
/// then AWGN, ×100, clamped to i16.
pub(crate) fn synth_window(signals: &[(Word87, f32, f32)], speed: Speed, seed: u64) -> Vec<i16> {
    let n = speed.period_s() as usize * 12_000;
    let bw_ratio = 2500.0f32 / 6000.0;
    let mut dd = vec![0f32; n];
    for (word, f0, snr_db) in signals {
        let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db);
        let wave = modulate(&encode_word(word, speed), speed, *f0, 12_000.0);
        for (i, &w) in wave.iter().enumerate() {
            if i < n {
                dd[i] += sig * w;
            }
        }
    }
    let mut rng = Rng::new(seed);
    dd.iter()
        .map(|&s| (((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
        .collect()
}

/// A Word87 from twelve six-bit chars (First+Last set, Data clear) — the
/// shape of a one-frame heartbeat-class test word.
pub(crate) fn word(chars12: &str) -> Word87 {
    let c = crate::proto::alphabet::sixbit_from_str(chars12).expect("12 sixbit chars");
    Word87::new(
        Payload72::from_chars12(c),
        I3 { first: true, last: true, data: false },
    )
}
