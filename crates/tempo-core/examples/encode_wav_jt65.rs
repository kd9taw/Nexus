//! Nexus JT65 TRANSMIT CLI — the encode arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! > **Encode here → decode with stock WSJT-X `jt9 -6`.**
//!
//! ⭐ JT65 IS NATIVELY 11025 Hz, the only mode here that is not 12 kHz. Symbol
//! length is 4096 samples at THAT rate, so at 12 kHz a symbol is 4458.503…
//! samples — fractional. Boundaries are taken as round(i*nsps) from the start so
//! the error stays bounded at half a sample instead of accumulating ~63 samples
//! of drift across 126 symbols.
//!
//! ⭐ Messages are at most 22 CHARACTERS (legacy `packjt`, not `packjt77`).
//!
//! Usage:
//!   cargo run -q -p tempo-core --example encode_wav_jt65 -- OUT.wav "K1ABC W9XYZ EN37" \
//!       [--submode 0] [--f0 1500]
//!
//! Then, from the parity lab:
//!   stock/b/jt9 -6 -p 60 OUT.wav

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
    let pos: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--")))
        .map(|(_, a)| a)
        .collect();
    if pos.len() < 2 {
        eprintln!("usage: encode_wav_jt65 OUT.wav \"MESSAGE\" [--submode 0] [--f0 1500]");
        std::process::exit(2);
    }
    let (out_path, msg) = (pos[0], pos[1].as_str());
    let submode: u8 = arg(&args, "--submode", 0);
    let f0: f32 = arg(&args, "--f0", 1500.0);

    let mode = make_mode(ModeKind::Jt65 { submode });
    let itone = mode.encode(msg);
    if itone.is_empty() {
        eprintln!("encode failed: {msg:?} does not pack (JT65 messages are <= 22 chars)");
        std::process::exit(1);
    }
    let wave = mode.gen_wave(&itone, SAMPLE_RATE, f0);
    if wave.is_empty() {
        eprintln!("gen_wave refused JT65 submode {submode}");
        std::process::exit(1);
    }

    let slot_len = 60 * SAMPLE_RATE as usize;
    let mut buf = vec![0f32; slot_len];
    let n = wave.len().min(slot_len);
    buf[..n].copy_from_slice(&wave[..n]);

    let peak = buf.iter().fold(0f32, |m, &x| m.max(x.abs())).max(1e-9);
    let pcm: Vec<i16> = buf
        .iter()
        .map(|&x| (x * (8000.0 / peak)).clamp(-32768.0, 32767.0) as i16)
        .collect();
    write_wav_i16(out_path, &pcm, SAMPLE_RATE as u32).expect("write wav");

    eprintln!(
        "# JT65{} {msg:?} f0={f0} Hz  spacing={:.4} Hz  bw={:.0} Hz  over={:.1}s of 60s slot",
        (b'A' + submode) as char,
        jt65::tone_spacing_hz(submode).unwrap_or(0.0),
        jt65::bandwidth_hz(submode).unwrap_or(0.0),
        jt65::tx_duration_secs(),
    );
    eprintln!("# wrote {out_path} ({} samples)", pcm.len());
}
