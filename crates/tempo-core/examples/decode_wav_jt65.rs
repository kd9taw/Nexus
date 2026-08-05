//! Nexus-as-configured JT65 decoder CLI — the JT65 arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Nexus ships JT65 RECEIVE-ONLY, so there is no encode path in libtempo and no
//! way to synthesise a JT65 signal in-tree. Measuring anything needs stock
//! `jt65sim` for signals and stock `jt9 -6` as the reference decoder.
//!
//! JT65 specifics that differ from the other CLIs:
//!
//!   * `--submode A|B|C` (or 0|1|2). One FIXED 60 s period — no `--period`.
//!   * ⭐ THE FRAME IS THE FULL 60 s even though only 52 s are decoded. The
//!     underlying Fortran dummy is explicit-shape at 720000, so a short buffer
//!     would read past the end. Files are sliced at `ModeKind::Jt65.frame_samples()`.
//!   * Messages are 22 characters (legacy packjt), not 37.
//!   * The decode type is printed: RS = an independent Reed-Solomon decode,
//!     DS = deep search, which matched against candidates built from the
//!     callsigns in play rather than recovering the message on its own merits.
//!
//! Runs BLIND (empty mycall/hiscall/hisgrid) so the stock comparison is fair —
//! with callsigns supplied, deep search is handed the answer and "yield" measures
//! the candidate list rather than the demodulator, exactly as with Q65's q3.

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12000;
const PERIOD_S: usize = 60;

fn resample_linear(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let n_out = ((x.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(n_out);
    for i in 0..n_out {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let frac = (pos - i0 as f64) as f32;
        let a = x[i0.min(x.len() - 1)];
        let b = x[(i0 + 1).min(x.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

fn decode_file(path: &str, kind: ModeKind) -> Result<usize, String> {
    let (samples, sr) = read_wav_i16(path).map_err(|e| e.to_string())?;
    let f: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let f12 = if sr == MODEM_RATE {
        f
    } else {
        resample_linear(&f, sr, MODEM_RATE)
    };
    let iwave_all = capture_to_i16(&f12);

    // The full 60 s, not the 52 s the decoder reads — see the banner.
    let frame_len = kind.frame_samples();
    let n_slots = (iwave_all.len() / frame_len).max(1);

    let mut src = NativeSource::from_kind(kind);
    let mut total = 0usize;

    for slot in 0..n_slots {
        let start = slot * frame_len;
        let mut frame = vec![0i16; frame_len];
        if start < iwave_all.len() {
            let end = (start + frame_len).min(iwave_all.len());
            frame[..end - start].copy_from_slice(&iwave_all[start..end]);
        }
        let req = DecodeRequest {
            iwave: &frame,
            nfa: 200,
            nfb: 2900,
            ndepth: 3,
            mycall: "",
            hiscall: "",
            nqso_progress: 0,
            nfqso: 1500,
            nftx: 1500,
            frame_time_ms: (slot as i64) * (PERIOD_S as i64) * 1000,
            ap: true, // stock (FT8/FT4 AP controls; inert for JT65)
            ap_cq_only: false,
            partial: false,
        };
        let mut decs = src.decode(&req);
        decs.sort_by(|a, b| {
            a.freq
                .partial_cmp(&b.freq)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for d in &decs {
            println!(
                "{} {:.1} {} {:+.2} {} {}",
                slot * PERIOD_S,
                d.freq,
                d.snr,
                d.dt,
                d.message.trim(),
                if d.nap == 0 { "RS" } else { "DS" }
            );
            total += 1;
        }
    }
    Ok(total)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut submode: u8 = 0;
    let mut files: Vec<String> = Vec::new();

    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--submode" => match it.next().map(|v| parse_submode(v)) {
                Some(Some(v)) => submode = v,
                _ => {
                    eprintln!("--submode needs A..C or 0..2");
                    std::process::exit(2);
                }
            },
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        eprintln!("usage: decode_wav_jt65 [--submode A|B|C] FILE.wav [...]");
        std::process::exit(2);
    }

    let kind = ModeKind::Jt65 { submode };
    eprintln!(
        "# mode: {} (blind, ndepth 3, 200-2900 Hz, 60 s period, {} samples/frame, 52 s decoded)",
        kind.as_str(),
        kind.frame_samples()
    );
    for path in &files {
        eprintln!("# file: {path}");
        match decode_file(path, kind) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}

/// `A`..`C` (either case) or `0`..`2` -> the ABI's 0-based submode index.
fn parse_submode(v: &str) -> Option<u8> {
    if let Ok(n) = v.parse::<u8>() {
        return (n < ModeKind::JT65_SUBMODES).then_some(n);
    }
    let c = v.chars().next()?.to_ascii_uppercase();
    (('A'..='C').contains(&c)).then(|| c as u8 - b'A')
}
