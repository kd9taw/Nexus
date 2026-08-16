//! FT2 decode verification — the harness MSK144 shipped without and this mode
//! must not.
//!
//! ⚠️ SYNTHETIC-ONLY, AND HONEST ABOUT IT. Unlike the FT8/FT4 twins of this
//! file, there is no reference WAV here: Decodium publishes no sample audio
//! (its repo carries only UI alert sounds — checked at vendor time), and no
//! second implementation of this FT2 exists to cross-decode against. What this
//! harness proves is INTERNAL consistency at the published operating point:
//! the vendored encoder and decoder agree through AWGN at and below the
//! −10.8 dB the mode claims, with a negative control that shows the sweep can
//! fail. What it cannot prove is on-air interop with a real Decodium station —
//! that is a NEEDS-BENCH item, and the first captured Decodium WAV belongs in
//! fixtures/ beside a decode assertion the moment one exists.
//!
//! SNR convention: WSJT-X's 2500 Hz reference bandwidth, same arithmetic as
//! the ft4 harness this file is adapted from.

use ft2::{decode_frame, encode, gen_wave, NMAX, SAMPLE_RATE};

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

/// One encoded FT2 message at `f0`, scaled to `snr_db` (2500 Hz convention),
/// placed at the nominal window start (the decoder's dt≈0 point is 0.5 s in),
/// plus AWGN across the whole 3.75 s window.
fn window_with(signals: &[(&str, f32)], snr_db: f32, seed: u64) -> Vec<i16> {
    let bw_ratio = 2500.0 / (SAMPLE_RATE / 2.0);
    let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db);
    let start = (0.5 * SAMPLE_RATE) as usize;
    let mut dd = vec![0f32; NMAX];
    for (msg, f0) in signals {
        let itone = encode(msg).expect("message packs");
        let wave = gen_wave(&itone, *f0).expect("wave generates");
        for (i, &w) in wave.iter().enumerate() {
            if start + i < NMAX {
                dd[start + i] += sig * w;
            }
        }
    }
    let mut rng = Rng(seed);
    dd.iter()
        .map(|&s| (((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0)) as i16)
        .collect()
}

fn decode(iwave: &[i16]) -> Vec<ft2::Decode> {
    decode_frame(iwave, 200, 2900, 3, "KD9TAW", "", 1500)
}

/// The published operating point: Decodium claims −10.8 dB (−12 with AP).
/// Without AP help, the decoder must hold −10.8 across several noise seeds —
/// majority, not unanimity: right at threshold, individual seeds may lose.
#[test]
fn holds_the_published_sensitivity_at_minus_10_8_db() {
    let mut hits = 0;
    for seed in 1..=5u64 {
        let iwave = window_with(&[("CQ KD9TAW EN52", 1500.0)], -10.8, seed);
        if decode(&iwave).iter().any(|d| d.message == "CQ KD9TAW EN52") {
            hits += 1;
        }
    }
    assert!(
        hits >= 3,
        "decoded {hits}/5 at -10.8 dB; the mode claims this SNR"
    );
}

/// Comfortably above threshold every seed must decode — a flaky pass at −5 dB
/// would mean the chain is broken in a way the threshold test might mask.
#[test]
fn always_decodes_well_above_threshold() {
    for seed in 1..=3u64 {
        let iwave = window_with(&[("K1ABC KD9TAW R-07", 1200.0)], -5.0, seed);
        assert!(
            decode(&iwave)
                .iter()
                .any(|d| d.message == "K1ABC KD9TAW R-07"),
            "seed {seed} failed at -5 dB"
        );
    }
}

/// NEGATIVE CONTROL for the sweep: far below threshold the decoder must find
/// nothing. Proves the sensitivity test can fail — without this, a decoder
/// that returned the encoded message regardless of the audio (through some
/// AP/state leak) would sail through everything above.
#[test]
fn fails_well_below_threshold() {
    let mut hits = 0;
    for seed in 1..=3u64 {
        let iwave = window_with(&[("CQ KD9TAW EN52", 1500.0)], -20.0, seed);
        hits += usize::from(decode(&iwave).iter().any(|d| d.message == "CQ KD9TAW EN52"));
    }
    assert!(
        hits == 0,
        "decoded {hits}/3 at -20 dB — that is not credible for this code"
    );
}

/// Two overlapping signals at different frequencies both decode — the
/// candidate scan is a real scan, not a single-signal tracker.
#[test]
fn recovers_two_overlapping_signals() {
    let iwave = window_with(
        &[("CQ KD9TAW EN52", 800.0), ("CQ ON8ST JO10", 2200.0)],
        -3.0,
        7,
    );
    let decodes = decode(&iwave);
    assert!(
        decodes.iter().any(|d| d.message == "CQ KD9TAW EN52"),
        "{decodes:?}"
    );
    assert!(
        decodes.iter().any(|d| d.message == "CQ ON8ST JO10"),
        "{decodes:?}"
    );
}

/// The reported frequency lands near where the signal was placed. GFSK tone 0
/// sits at f0 and the four tones span ~125 Hz above it, so the reported centre
/// is allowed a generous band around f0 — this pins gross scaling errors
/// (wrong NDOWN, wrong baud), not calibration.
#[test]
fn reported_frequency_is_sane() {
    let iwave = window_with(&[("CQ KD9TAW EN52", 1000.0)], -2.0, 11);
    let decodes = decode(&iwave);
    let d = decodes
        .iter()
        .find(|d| d.message == "CQ KD9TAW EN52")
        .expect("decodes at -2 dB");
    assert!(
        (d.freq - 1000.0).abs() < 150.0,
        "freq {} for a signal placed at 1000 Hz",
        d.freq
    );
    // Placed at the nominal start ⇒ dt near zero.
    assert!(d.dt.abs() < 0.2, "dt {} for an on-time signal", d.dt);
}
