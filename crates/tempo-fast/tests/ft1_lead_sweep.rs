//! FT1 lead-in sweep — how much early-timing tolerance does each lead buy, and what does it cost?
//!
//! ## Why this exists
//!
//! FT1's transmit buffer is a FIXED 48000-sample (4.0000 s) frame holding 3.5357 s of tones plus
//! 0.4643 s of trailing zeros, and `Ft1Mode::gen_wave` shifts the lead-in WITHIN that buffer
//! rather than prepending — so the buffer always fills FT1's whole 4 s T/R period. Two things
//! come straight out of that, and they trade against each other one-for-one:
//!
//! * **EARLY-TIMING TOLERANCE = THE LEAD.** `tempofast_decode.f90`'s coarse sweep is
//!   `do istart=0,200,4`, hard-clamped at zero (the fine pass is `max(0,ibest_all-5)`), so the
//!   decoder cannot look for a signal that starts before sample 0. Placing the tones `L` into the
//!   buffer is the only early-side margin there is. `NDOWN = 54`, so one `istart` step is
//!   54/12000 = 4.5 ms and the sweep spans 0–900 ms in 18 ms steps.
//! * **POST-TONE TAIL = 464 ms − LEAD.** PTT is held to the slot boundary (the clamp in
//!   `tempo-audio/src/slot.rs`), and the tones end at `lead + 3.5357 s`. That tail is what
//!   protects the end of the signal from soundcard output latency: the device ring lags the
//!   buffer by L_out, so any output latency exceeding the tail cuts FT1's RF mid-tone.
//!
//! At the shipped 0.400 s lead the tail is **64 ms**, and 20–100 ms cpal buffers are ordinary.
//! At 0.214 s the tail is the full 250 ms. The question this file answers is what that costs.
//!
//! ## What it measures
//!
//! For each lead, the decode rate against a peer whose timing is off by `dt`, at a few SNRs.
//! Reported as a tolerance CURVE, not a single number, because the operator is choosing where to
//! spend a fixed 464 ms budget.
//!
//! Run it: `cargo test -p tempo-fast --test ft1_lead_sweep -- --nocapture --ignored`
//! It is `#[ignore]` because it is a measurement instrument, not a gate — it takes minutes and
//! its output is a table for a human, not an assertion. The two guards at the bottom ARE gates.

use tempo_fast::{decode_frame, encode, gen_wave, NMAX, SAMPLE_RATE};

/// ⚠️ A STANDARD message, not free text. FT8-compatible free text is 13 characters; the 17-char
/// "KD9TAW N9UM HELLO" this started with does not pack, so `encode` returned symbols that could
/// never decode and the whole harness scored 0/8 while looking like a timing result. Matches
/// `crates/tempo-core/tests/loopback_acquire.rs`, which is the known-good acquisition path.
const MSG: &str = "CQ W9XYZ EN37";
const F0: f32 = 1500.0;
/// Tones occupy this much of the buffer (99 symbols x 3000/7 samples at 12 kHz).
const TONES_S: f64 = 99.0 * (3000.0 / 7.0) / 12_000.0;
/// The fixed buffer `tempo_fast::gen_wave` returns.
const BUFFER_S: f64 = NMAX as f64 / 12_000.0;

/// Deterministic white noise — a xorshift so a sweep is reproducible run to run. Seeding per
/// (lead, dt, snr, trial) means every lead sees the SAME noise at the same trial index, so a
/// difference between two rows is the lead and not the draw.
struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // Box-Muller is overkill here; a sum of uniforms is close enough to Gaussian for an
        // AWGN sensitivity sweep and much cheaper.
        let mut s = 0.0f32;
        for _ in 0..4 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            s += (self.0 >> 40) as f32 / (1u64 << 24) as f32 - 0.5;
        }
        s * 0.9
    }
}

/// Build one 4 s receive buffer: tones placed `lead_s` into the frame, then the whole thing
/// shifted by `dt_s` (positive = the peer transmitted LATE), plus AWGN at `snr_db`.
///
/// `snr_db` is referenced to 2500 Hz the way WSJT-X reports it.
fn scene(lead_s: f64, dt_s: f64, snr_db: f32, seed: u64) -> Vec<f32> {
    let tones = gen_wave(&encode(MSG), SAMPLE_RATE, F0);
    let mut buf = vec![0f32; NMAX];
    // Where the first tone sample lands in the receiver's frame.
    let start = ((lead_s + dt_s) * 12_000.0).round() as isize;
    for (i, &s) in tones.iter().enumerate() {
        // gen_wave already returns a full NMAX buffer with the tones at sample 0, so only the
        // leading TONES_S of it carries signal; copying all of it is harmless (the rest is zero)
        // but we must respect the destination bounds.
        let d = start + i as isize;
        if d >= 0 && (d as usize) < buf.len() {
            buf[d as usize] += s;
        }
    }
    // Signal power over the tone span, then noise scaled to the requested SNR in 2500 Hz.
    let n_tone = (TONES_S * 12_000.0) as usize;
    let sig_p: f64 = tones[..n_tone.min(tones.len())]
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum::<f64>()
        / n_tone as f64;
    // Noise power in the full 6 kHz Nyquist span that gives `snr_db` in 2500 Hz.
    let noise_p = sig_p / 10f64.powf(f64::from(snr_db) / 10.0) * (6000.0 / 2500.0);
    let sigma = noise_p.sqrt() as f32;
    let mut rng = Rng(seed | 1);
    for v in buf.iter_mut() {
        *v += rng.next_f32() * sigma;
    }
    buf
}

/// ⚠️ `decode_frame`, NOT `decode_rt`. `decode_rt` is the real-time entry and its own doc says
/// the waveform must start at sample 0 — it does no acquisition, so it cannot see a lead at all
/// and scores 0/8 even on a clean on-time signal. `decode_frame` is the full RX path that runs
/// `tempofast_decode.f90`'s `istart` sweep, which is the thing under test here.
fn decodes(lead_s: f64, dt_s: f64, snr_db: f32, trials: u32) -> u32 {
    (0..trials)
        .filter(|t| {
            let w = scene(lead_s, dt_s, snr_db, 0x9E37_79B9 ^ u64::from(*t) * 0x1_0001);
            // The ABI takes i16 at 12 kHz. gen_wave is ~unit-scale, so 8000 keeps headroom for
            // signal + noise without clipping the loud cells of the sweep.
            let iwave: Vec<i16> = w.iter().map(|&v| (v * 8000.0) as i16).collect();
            // `frame_time_ms` labels the period and MUST DIFFER between calls — it is half of the
            // decoder's duplicate suppressor, so a fixed value silently drops every repeat.
            let stamp = 1_000_000i64 + i64::from(*t) * 4_000;
            decode_frame(&iwave, 200, 2900, 3, "N9UM", "KD9TAW", 0, stamp)
                .iter()
                .any(|d| d.message.trim() == MSG)
        })
        .count() as u32
}

/// The tail an operator gets for a given lead — the whole reason a shorter lead is wanted.
fn tail_ms(lead_s: f64) -> f64 {
    (BUFFER_S - (lead_s + TONES_S)) * 1000.0
}

#[test]
#[ignore = "measurement instrument: minutes to run, prints a table for a human"]
fn sweep_lead_against_timing_error() {
    const TRIALS: u32 = 12;
    let leads = [0.150, 0.214, 0.250, 0.300, 0.400];
    let dts = [
        -0.45, -0.40, -0.35, -0.30, -0.25, -0.20, -0.15, -0.10, -0.05, 0.0, 0.10, 0.25, 0.40,
    ];
    for &snr in &[-4.0f32, -8.0, -12.0] {
        println!("\n=== SNR {snr} dB (2500 Hz), {TRIALS} trials per cell ===");
        print!("{:>10} {:>9} |", "lead (ms)", "tail (ms)");
        for d in dts {
            print!("{:>6}", (d * 1000.0) as i32);
        }
        println!("   <- peer timing error (ms), + = LATE");
        for &lead in &leads {
            print!("{:>10.0} {:>9.0} |", lead * 1000.0, tail_ms(lead));
            for &dt in &dts {
                let n = decodes(lead, dt, snr, TRIALS);
                print!("{n:>6}");
            }
            println!();
        }
    }
    println!(
        "\nEarly tolerance should track the lead (istart clamps at 0); late tolerance should run \
         to ~900 ms - lead (the coarse sweep's top). Tail is 464 ms - lead, and is what protects \
         the signal from soundcard output latency."
    );
}

// ── The two GATES. These assert the invariants the sweep is exploring, at the SHIPPED lead. ──

#[test]
fn the_shipped_lead_decodes_an_on_time_peer() {
    // The floor: whatever we choose, an on-time peer at a workable SNR must decode. If this ever
    // fails the sweep above is measuring noise, not timing.
    let n = decodes(0.400, 0.0, -4.0, 8);
    assert!(n >= 7, "an on-time peer must decode at -4 dB: {n}/8");
}

#[test]
fn the_early_cliff_is_the_lead_and_not_something_else() {
    // ⭐ THE MECHANISM, pinned. The decoder cannot search before sample 0
    // (`tempofast_decode.f90` `do istart=0,200,4`), so a peer earlier than the lead falls off a
    // cliff rather than degrading. This is the whole reason FT1 carries a lead at all, and it is
    // what makes lead-vs-tail a real trade instead of a free choice.
    //
    // Measured just INSIDE the shipped lead (300 ms early of a 400 ms lead) it decodes; just
    // OUTSIDE it (500 ms early) it cannot, at an SNR where timing is the only variable.
    let inside = decodes(0.400, -0.30, -4.0, 8);
    let outside = decodes(0.400, -0.50, -4.0, 8);
    assert!(
        inside >= 6,
        "300 ms early is INSIDE a 400 ms lead and must decode: {inside}/8"
    );
    assert!(
        outside <= 1,
        "500 ms early is OUTSIDE a 400 ms lead — the istart clamp makes this a cliff, so a \
         decode here means the scene is not testing what it claims: {outside}/8"
    );
}
