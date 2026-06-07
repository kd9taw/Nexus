//! Behavioural proof that a-priori (AP) decoding actually fires and recovers
//! frames the no-context path misses — exercising the SAME golden WSJT-X decoder
//! the engine now feeds real MyCall/DxCall/nQSOProgress into.

use ft8::{decode_frame, encode, gen_wave, NMAX, SAMPLE_RATE};

/// Unit-variance Gaussian (LCG + Box-Muller) — matches tempo-core's `Awgn` so the
/// SNR convention is identical, without a cross-crate dep.
struct Awgn {
    state: u64,
}
impl Awgn {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
    fn u01(&mut self) -> f64 {
        (self.next_u32() as f64 + 1.0) / (u32::MAX as f64 + 2.0)
    }
    fn sample(&mut self) -> f32 {
        let u1 = self.u01();
        let u2 = self.u01();
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

fn snr_to_scale(snr_db: f32, fs: f32) -> f32 {
    let bw_ratio = 2500.0 / (fs / 2.0);
    (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db)
}

/// Build a noisy 15 s int16 frame containing `msg` at carrier 1500 Hz, SNR
/// `snr_db`, AWGN seed `seed`.
fn noisy_frame(msg: &str, snr_db: f32, seed: u64) -> Vec<i16> {
    let tones = encode(msg);
    let wave = gen_wave(&tones, SAMPLE_RATE, 1500.0);
    let sig = snr_to_scale(snr_db, SAMPLE_RATE);
    let mut noise = Awgn::new(seed);
    let noff = 6_000usize; // FT8 TX starts 0.5 s into the slot
    let mut buf = vec![0f32; NMAX];
    for (i, &s) in wave.iter().enumerate() {
        if noff + i < NMAX {
            buf[noff + i] = sig * s;
        }
    }
    for s in buf.iter_mut() {
        *s += noise.sample();
    }
    buf.iter()
        .map(|&x| (x * 100.0).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16)
        .collect()
}

/// AP recovers frames the no-context decoder cannot, and the recovery is
/// attributed to an actual AP pass (`nap > 0`), not a coincidental standalone
/// decode. This drives the SAME golden WSJT-X `ft8b` the engine feeds real
/// MyCall/DxCall/nQSOProgress into, so it is the behavioural proof of the wiring.
///
/// Operating point −22 dB (see `explore_ap_margin`): on this RR73-addressed-to-me
/// message the deepest AP case (iaptype 6, all 77 bits known) recovers ~every
/// seed while the no-context path recovers none — a several-dB gain.
#[test]
fn ap_recovers_frames_the_no_context_path_cannot() {
    let msg = "KD9TAW W1AW RR73"; // RR73 to me → iaptype 6 (all 77 ap bits)
    let seeds = 12u64;
    let (mut ap, mut ap_via_ap_pass, mut noap) = (0u32, 0u32, 0u32);
    for seed in 0..seeds {
        let iwave = noisy_frame(msg, -22.0, seed);
        // AP context: responder awaiting RR73 → nQSOProgress = 3 (matches
        // State::AwaitRoger/AwaitRr73 territory the engine now supplies).
        let decs = decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3);
        if let Some(d) = decs.iter().find(|d| d.message == msg) {
            ap += 1;
            if d.nap > 0 {
                ap_via_ap_pass += 1; // recovery explicitly credited to an AP pass
            }
        }
        if decode_frame(&iwave, 200, 2900, 3, "", "", 0)
            .iter()
            .any(|d| d.message == msg)
        {
            noap += 1;
        }
    }
    assert_eq!(
        noap, 0,
        "the no-context decoder must NOT recover this frame at -22 dB, got {noap}/{seeds}"
    );
    assert!(
        ap >= 9,
        "AP must recover the frame in most seeds, got {ap}/{seeds}"
    );
    assert!(
        ap_via_ap_pass >= 9,
        "recoveries must be credited to an AP pass (nap>0), got {ap_via_ap_pass}/{ap}"
    );
}

/// EXPLORATION ONLY (ignored): print AP-vs-no-AP recovery across an SNR band so we
/// can pick a marginal operating point. Run with:
///   cargo test -p ft8 --test ap_decode explore -- --ignored --nocapture
#[test]
#[ignore]
fn explore_ap_margin() {
    let msg = "KD9TAW W1AW RR73"; // RR73 to me → deepest AP (iaptype 6, all 77 bits)
    for &snr in &[-18.0f32, -20.0, -21.0, -22.0, -23.0, -24.0, -25.0, -26.0] {
        let (mut ap, mut noap) = (0u32, 0u32);
        let seeds = 12u64;
        for seed in 0..seeds {
            let iwave = noisy_frame(msg, snr, seed);
            // With AP context (responder awaiting RR73 → nQSOProgress = 3).
            if decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "W1AW", 3)
                .iter()
                .any(|d| d.message == msg)
            {
                ap += 1;
            }
            // Without AP context.
            if decode_frame(&iwave, 200, 2900, 3, "", "", 0)
                .iter()
                .any(|d| d.message == msg)
            {
                noap += 1;
            }
        }
        println!("SNR {snr:>6} dB:  AP {ap:2}/{seeds}   no-AP {noap:2}/{seeds}");
    }
}
