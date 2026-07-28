//! Nexus FST4 TRANSMIT CLI — the encode arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Writes a slot-length WAV containing one FST4 transmission, produced by exactly
//! the path the radio uses: `Mode::encode` → `Mode::gen_wave`, lead-in included.
//!
//! # The claim this proves
//! > **Encode here → decode with stock WSJT-X `jt9 -7`.**
//!
//! If upstream decodes our transmission we are on-air compatible. Our own decoder
//! is not involved, so a shared bug in our encode/decode pair — both sides come
//! from the same vendored `genfst4` — cannot hide.
//!
//! ⭐ FST4's waveform is NOT Q65's. `gen_fst4wave` applies a GFSK
//! frequency-deviation pulse (BT=2.0) spanning three symbols plus raised-cosine
//! ramps, and internally shifts DOWN by 1.5 tone spacings while every upstream
//! caller shifts UP by the same amount (`mainwindow.cpp:12703`). The two cancel,
//! leaving the lowest tone at the reported frequency. Dropping either half moves
//! the signal by 1.5 spacings — at FST4-1800 that is 0.13 Hz and would still
//! decode, which is exactly why it needs a test rather than an eyeball.
//!
//! Usage:
//!   cargo run -q -p tempo-core --example encode_wav_fst4 -- OUT.wav "K1ABC W9XYZ EN37" \
//!       [--period 60] [--f0 1500] [--snr-db N] [--seed N]
//!
//! Then, from the parity lab:
//!   stock/b/jt9 -7 -p 60 -F 100 -d 3 OUT.wav

use modes::{make_mode, ModeKind};
use tempo_core::wavfile::write_wav_i16;

const SAMPLE_RATE: f32 = 12_000.0;

fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")))
        .map(|(_, a)| a)
        .collect();
    if positional.len() < 2 {
        eprintln!("usage: encode_wav_fst4 OUT.wav \"MESSAGE\" [--period 60] [--f0 1500] [--snr-db N] [--seed N]");
        std::process::exit(2);
    }
    let (out_path, msg) = (positional[0], positional[1].as_str());
    let period_s: u16 = arg(&args, "--period", 60);
    let f0: f32 = arg(&args, "--f0", 1500.0);
    let snr_db: Option<f32> = args
        .iter()
        .position(|a| a == "--snr-db")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let mode = make_mode(ModeKind::Fst4 {
        period_s,
        wspr: false,
    });
    let itone = mode.encode(msg);
    if itone.is_empty() {
        eprintln!("encode failed: {msg:?} does not pack as an FST4 message");
        std::process::exit(1);
    }
    let wave = mode.gen_wave(&itone, SAMPLE_RATE, f0);
    if wave.is_empty() {
        eprintln!("gen_wave refused FST4-{period_s} at f0 {f0}");
        std::process::exit(1);
    }

    let slot_len = usize::from(period_s) * SAMPLE_RATE as usize;
    let mut buf = vec![0f32; slot_len];
    let n = wave.len().min(slot_len);
    buf[..n].copy_from_slice(&wave[..n]);

    if let Some(snr) = snr_db {
        add_awgn(&mut buf, snr, arg(&args, "--seed", 0xC0FFEEu32));
    }

    let peak = buf.iter().fold(0f32, |m, &x| m.max(x.abs())).max(1e-9);
    let scale = if snr_db.is_some() {
        8000.0
    } else {
        8000.0 / peak
    };
    let pcm: Vec<i16> = buf
        .iter()
        .map(|&x| (x * scale).clamp(-32768.0, 32767.0) as i16)
        .collect();
    write_wav_i16(out_path, &pcm, SAMPLE_RATE as u32).expect("write wav");

    eprintln!(
        "# FST4-{period_s} f0={f0} Hz  spacing={:.4} Hz  bw={:.3} Hz  over={:.1}s of {period_s}s slot",
        fst4::tone_spacing_hz(period_s, 1).unwrap_or(0.0),
        fst4::bandwidth_hz(period_s, 1).unwrap_or(0.0),
        fst4::tx_duration_secs(period_s).unwrap_or(0.0),
    );
    eprintln!("# wrote {out_path} ({} samples)", pcm.len());
}

/// AWGN for a target SNR in the 2500 Hz reference bandwidth — the convention every
/// WSJT-X mode reports SNR in. Deterministic off `seed0` so a ladder is repeatable
/// while still varying its realisations.
fn add_awgn(buf: &mut [f32], snr_db: f32, seed0: u32) {
    let sig_pow: f64 = buf
        .iter()
        .map(|&x| f64::from(x) * f64::from(x))
        .sum::<f64>()
        / buf.len() as f64;
    if sig_pow <= 0.0 {
        return;
    }
    let snr_lin = 10f64.powf(f64::from(snr_db) / 10.0);
    let sigma = (sig_pow / snr_lin * (6000.0 / 2500.0)).sqrt();
    let mut seed: u32 = seed0;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(seed >> 8) / f64::from(1u32 << 24)
    };
    for x in buf.iter_mut() {
        let (u1, u2) = (next().max(1e-12), next());
        let g = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
        *x += (sigma * g) as f32;
    }
}
