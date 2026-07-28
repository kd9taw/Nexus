//! Nexus Q65 TRANSMIT CLI — the encode arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Writes a slot-length WAV containing one Q65 transmission, produced by exactly
//! the path the radio uses: `Mode::encode` → `Mode::gen_wave`, including the
//! slot-positioning lead-in.
//!
//! # Why this is the test that matters
//! The RX ladders proved our DECODER against stock `jt9`. This inverts it, and is
//! the stronger claim because it needs no synthetic corpus:
//!
//! > **Encode here → decode with stock WSJT-X `jt9 -3`.**
//!
//! If upstream decodes our transmission, we are on-air compatible with every WSJT-X
//! station. Nothing about our own decoder is involved, so a shared bug in our
//! encode/decode pair — the one thing an in-tree round-trip cannot catch, since
//! both sides come from the same vendored `genq65` — cannot hide here.
//!
//! ⭐ SUBMODE IS THE THING TO EXERCISE. Tone spacing is `(12000/nsps) << submode`,
//! and WSJT-X's own 48 kHz preview path (`mainwindow.cpp:8038`) computes it as
//! submode-A regardless of the selection — only the on-air path at
//! `mainwindow.cpp:12721` applies `2**nSubMode`. An encoder written from the wrong
//! one produces audio that our decoder (which would share the mistake) reads back
//! perfectly and stock jt9 cannot touch. Run this at B/C, not just A.
//!
//! Usage:
//!   cargo run -q -p tempo-core --example encode_wav_q65 -- OUT.wav "K1ABC W9XYZ EN37" \
//!       [--period 60] [--submode 0] [--f0 1500] [--snr-db N]
//!
//! Then, from the parity lab:
//!   stock/b/jt9 -3 -p 60 -b A -d 3 -Q 0 -c b -x b OUT.wav

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
        .filter(|(i, a)| {
            !a.starts_with("--") && (*i == 0 || !args[i.saturating_sub(1)].starts_with("--"))
        })
        .map(|(_, a)| a)
        .collect();
    if positional.len() < 2 {
        eprintln!(
            "usage: encode_wav_q65 OUT.wav \"MESSAGE\" [--period 60] [--submode 0] \
             [--f0 1500] [--snr-db N]"
        );
        std::process::exit(2);
    }
    let out_path = positional[0];
    let msg = positional[1].as_str();

    let period_s: u16 = arg(&args, "--period", 60);
    let submode: u8 = arg(&args, "--submode", 0);
    let f0: f32 = arg(&args, "--f0", 1500.0);
    // Optional AWGN, so this doubles as a TX-side sensitivity ladder against stock
    // jt9. SNR is in the conventional 2500 Hz reference bandwidth, like q65sim's.
    let snr_db: Option<f32> = args
        .iter()
        .position(|a| a == "--snr-db")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let kind = ModeKind::Q65 { period_s, submode };
    let mode = make_mode(kind);

    let itone = mode.encode(msg);
    if itone.is_empty() {
        eprintln!("encode failed: {msg:?} does not pack as a Q65 message");
        std::process::exit(1);
    }

    // gen_wave returns the SLOT-POSITIONED waveform (lead-in included), which is
    // what the radio loop plays at the boundary.
    let wave = mode.gen_wave(&itone, SAMPLE_RATE, f0);
    if wave.is_empty() {
        eprintln!(
            "gen_wave refused Q65-{period_s}{}: bandwidth {:?} Hz at f0 {f0} does not \
             fit below Nyquist",
            (b'A' + submode) as char,
            q65::bandwidth_hz(period_s, submode)
        );
        std::process::exit(1);
    }

    // Pad to the full slot so the file is exactly what a receiver captures.
    let slot_len = usize::from(period_s) * SAMPLE_RATE as usize;
    let mut buf = vec![0f32; slot_len];
    let n = wave.len().min(slot_len);
    buf[..n].copy_from_slice(&wave[..n]);

    if let Some(snr) = snr_db {
        // Seed varies the noise realisation so a ladder can gather N samples at one
        // SNR. Fixed default: a parity run must be repeatable.
        let seed: u32 = arg(&args, "--seed", 0xC0FFEEu32);
        add_awgn(&mut buf, snr, seed);
    }

    // ~±8000 of full scale: well clear of clipping, and the same order the decode
    // path's `capture_to_i16` produces from a real capture.
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
        "# Q65-{}{} f0={f0} Hz  spacing={:.2} Hz  bw={:.0} Hz  over={:.1}s of {period_s}s slot",
        period_s,
        (b'A' + submode) as char,
        q65::tone_spacing_hz(period_s, submode).unwrap_or(0.0),
        q65::bandwidth_hz(period_s, submode).unwrap_or(0.0),
        q65::tx_duration_secs(period_s).unwrap_or(0.0),
    );
    eprintln!("# wrote {out_path} ({} samples)", pcm.len());
}

/// Add white Gaussian noise for a target SNR in a 2500 Hz reference bandwidth —
/// the convention every WSJT-X mode reports SNR in, and the one `q65sim` uses, so
/// a ladder built here is directly comparable to the RX ladders.
fn add_awgn(buf: &mut [f32], snr_db: f32, seed0: u32) {
    let sig_pow: f64 = buf
        .iter()
        .map(|&x| f64::from(x) * f64::from(x))
        .sum::<f64>()
        / buf.len() as f64;
    if sig_pow <= 0.0 {
        return;
    }
    // SNR is referenced to 2500 Hz; the noise here occupies the full 6 kHz Nyquist
    // span, so scale the noise power by (6000/2500).
    let snr_lin = 10f64.powf(f64::from(snr_db) / 10.0);
    let noise_pow = sig_pow / snr_lin * (6000.0 / 2500.0);
    let sigma = noise_pow.sqrt();

    // Deterministic Box-Muller off the caller's seed: repeatable, but varied across
    // a ladder's N samples at one SNR.
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
