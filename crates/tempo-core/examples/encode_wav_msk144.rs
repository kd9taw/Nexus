//! Nexus MSK144 TRANSMIT CLI — the encode arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! > **Encode here → decode with stock WSJT-X `jt9 -k`.**
//!
//! ⭐ MSK144 IS NOT THE OTHER MODES WITH A SHORTER PERIOD. It keys for essentially
//! the whole period (`trPeriod − 0.25`) sending the SAME 72 ms frame over and over —
//! ~204 copies in a 15 s interval. Meteor scatter works by transmitting continuously
//! and hoping one copy finds a reflection lasting a tenth of a second. A file
//! containing one frame would decode fine here and be useless on the air.
//!
//! ⭐ THE AUDIO FREQUENCY IS FIXED at a 1500 Hz centre. The signal is 1000 Hz wide
//! (two tones at ±500), so it fills a normal SSB passband and there is nowhere to
//! tune it. Upstream hardcodes the same thing.
//!
//! Usage:
//!   cargo run -q -p tempo-core --example encode_wav_msk144 -- OUT.wav "K1ABC W9XYZ EN37" \
//!       [--period 15]
//!
//! Then, from the parity lab:
//!   stock/b/jt9 -k -p 15 OUT.wav

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
        eprintln!("usage: encode_wav_msk144 OUT.wav \"MESSAGE\" [--period 15]");
        std::process::exit(2);
    }
    let (out_path, msg) = (pos[0], pos[1].as_str());
    let period_s: u16 = arg(&args, "--period", 15);

    let mode = make_mode(ModeKind::Msk144 { period_s });
    let itone = mode.encode(msg);
    if itone.is_empty() {
        eprintln!("encode failed: {msg:?} does not pack as an MSK144 message");
        std::process::exit(1);
    }
    let wave = mode.gen_wave(&itone, SAMPLE_RATE, 0.0);
    if wave.is_empty() {
        eprintln!("gen_wave refused MSK144-{period_s}");
        std::process::exit(1);
    }

    let slot_len = usize::from(period_s) * SAMPLE_RATE as usize;
    let mut buf = vec![0f32; slot_len];
    let n = wave.len().min(slot_len);
    buf[..n].copy_from_slice(&wave[..n]);

    let peak = buf.iter().fold(0f32, |m, &x| m.max(x.abs())).max(1e-9);
    let pcm: Vec<i16> = buf
        .iter()
        .map(|&x| (x * (8000.0 / peak)).clamp(-32768.0, 32767.0) as i16)
        .collect();
    write_wav_i16(out_path, &pcm, SAMPLE_RATE as u32).expect("write wav");

    let frames = wave.len() as f32 / (itone.len() * msk144::NSPS) as f32;
    eprintln!(
        "# MSK144-{period_s} {msg:?}  {} symbols/frame  {:.0} frames  {:.2}s of {period_s}s slot",
        itone.len(),
        frames,
        wave.len() as f32 / SAMPLE_RATE,
    );
    eprintln!("# wrote {out_path} ({} samples)", pcm.len());
}
