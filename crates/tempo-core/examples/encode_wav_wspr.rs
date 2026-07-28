//! Nexus WSPR/FST4W BEACON TRANSMIT CLI — the encode arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! > **Encode here → decode with stock WSJT-X `wsprd` (or `jt9 -W` for FST4W).**
//!
//! Writes one beacon interval as a WAV, produced by the same path the radio uses:
//! `Mode::encode` → `Mode::gen_wave`, lead-in included.
//!
//! ⭐ These are BEACONS, not QSO modes. The payload is exactly callsign + grid +
//! power in dBm — no exchange, no addressing, no free text. The power figure is
//! not decoration: WSPR reports feed a public propagation database, so a wrong
//! number corrupts other operators' conclusions as well as your own.
//!
//! Usage:
//!   cargo run -q -p tempo-core --example encode_wav_wspr -- OUT.wav KD9TAW EN52 30 \
//!       [--mode wspr|fst4w] [--period 120] [--f0 1500]
//!
//! Then, from the parity lab:
//!   stock/b/wsprd -f 10.1387 OUT.wav

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
    if pos.len() < 4 {
        eprintln!("usage: encode_wav_wspr OUT.wav CALL GRID DBM [--mode wspr|fst4w] [--period 120] [--f0 1500]");
        std::process::exit(2);
    }
    let (out_path, call, grid) = (pos[0], pos[1].as_str(), pos[2].as_str());
    let dbm: i32 = pos[3].parse().expect("power in dBm");
    let which: String = arg(&args, "--mode", "wspr".to_string());
    let f0: f32 = arg(&args, "--f0", 1500.0);

    // The beacon message is the same shape for both modes.
    let msg = wspr::message(call, grid, dbm);

    let (kind, period_s) = if which.eq_ignore_ascii_case("fst4w") {
        let p: u16 = arg(&args, "--period", 120);
        (
            ModeKind::Fst4 {
                period_s: p,
                wspr: true,
            },
            p,
        )
    } else {
        (ModeKind::Wspr, 120u16)
    };
    let mode = make_mode(kind);

    assert!(
        mode.capabilities().beacon_only,
        "this CLI is for beacons; {} is a QSO mode",
        kind.as_str()
    );

    let itone = mode.encode(&msg);
    if itone.is_empty() {
        eprintln!("encode failed: {msg:?} is not a valid beacon message");
        std::process::exit(1);
    }
    let wave = mode.gen_wave(&itone, SAMPLE_RATE, f0);
    if wave.is_empty() {
        eprintln!("gen_wave refused {} at f0 {f0}", kind.as_str());
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

    eprintln!(
        "# {} beacon {msg:?} f0={f0} Hz  {} symbols  {:.1}s of {period_s}s window",
        kind.as_str(),
        itone.len(),
        wave.len() as f32 / SAMPLE_RATE,
    );
    eprintln!("# wrote {out_path} ({} samples)", pcm.len());
}
