//! Per-speed JS8 decode cost at depth 3 — the number B5's multi-speed
//! scheduler (`js8_multi_decode_due`) and the all-four-speeds-by-default
//! ruling are gated on: four decodes must finish well inside Turbo's 6 s
//! cycle (in parallel under std::thread::scope; here they are timed one at a
//! time so each number is attributable). Realistic load: three signals at
//! −8/−14/−18 dB plus noise, so the candidate list and pass 2/3 are
//! exercised, not a single clean frame. Timed = decode() only.
//!
//! Run (release — debug numbers are meaningless):
//!   cargo run --release -p js8 --example decode_bench
//! Record the four lines in crates/js8/src/phy/decoder.rs's `Bench` header
//! line with the date and box; re-run rather than trust them, they are
//! machine-specific (the ft8 explore_early_pass_cost rule).

use js8::phy::{decode, encode_word, modulate};
use js8::proto::alphabet::sixbit_from_str;
use js8::{DecodeParams, Speed, Word87};
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn gauss(&mut self) -> f32 {
        let u1 = (self.next_f64() + 1e-12).min(1.0);
        let u2 = self.next_f64();
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
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

fn window(speed: Speed) -> Vec<i16> {
    let signals = [
        (word("KD9TAWEN52ab"), 900.0f32, -8.0f32),
        (word("W1AWFN31cdef"), 1500.0, -14.0),
        (word("VK3XYZQF22gh"), 2100.0, -18.0),
    ];
    let n = speed.period_s() as usize * 12_000;
    let bw_ratio = 2500.0f32 / 6000.0;
    let mut dd = vec![0f32; n];
    for (w, f0, snr) in &signals {
        let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr);
        let wave = modulate(&encode_word(w, speed), speed, *f0, 12_000.0);
        for (i, &x) in wave.iter().enumerate() {
            if i < n {
                dd[i] += sig * x;
            }
        }
    }
    let mut rng = Rng(0xBE4C);
    dd.iter()
        .map(|&s| (((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
        .collect()
}

fn main() {
    let params = DecodeParams::stock();
    let mut total_ms = 0f64;
    for speed in Speed::ALL {
        let iw = window(speed);
        let _ = decode(&iw, speed, &params); // warm the allocator/planner path
        let mut best = f64::MAX;
        let mut ndec = 0;
        for _ in 0..3 {
            let t = Instant::now();
            let d = decode(&iw, speed, &params);
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
            ndec = d.len();
        }
        total_ms += best;
        println!("{speed:?}: {ndec} of 3 decoded, {best:7.1} ms (best of 3, depth 3)");
    }
    println!("all four serial: {total_ms:7.1} ms  (Turbo cycle budget 6000 ms)");
}
