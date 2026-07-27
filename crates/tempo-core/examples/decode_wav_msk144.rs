//! Nexus-as-configured MSK144 decoder CLI — the MSK144 arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Why this exists: Nexus ships MSK144 RECEIVE-ONLY, so there is no encode path in
//! libtempo and no way to synthesise a meteor-scatter signal in-tree. Measuring
//! anything needs stock `msk144sim` for signals and stock `jt9 -9` as the reference
//! decoder, with this CLI as the Nexus side.
//!
//! MSK144 specifics that differ from the other CLIs:
//!
//!   * `--period` selects 5/10/15/30 s; the frame is period*12000 samples.
//!     Defaults to 15, the 6 m workhorse.
//!   * ⭐ A DISTINCT nutc PER FILE. mskrtd suppresses duplicate decodes and resets
//!     that suppressor only when nutc changes, so decoding a batch of files with a
//!     constant nutc would silently drop every repeat after the first. The Mode
//!     layer derives nutc from `frame_time_ms`, so this steps it per file — get
//!     that wrong and a ladder reads as a decoder that stops working.
//!   * The decode type is printed (`&` single-ping, `^` long average, plain
//!     frame-averaged), because on meteor scatter WHICH kind of decode you got is
//!     operationally meaningful.
//!
//! Runs BLIND (empty mycall/hiscall) so a stock comparison is fair.
//!
//! Output (stdout), one line per decode:
//!     <slot_offset_s> <freq_hz> <snr_db> <dt_s> <message> [type=<n>]

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12000;

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

fn decode_file(
    path: &str,
    kind: ModeKind,
    period_s: usize,
    file_index: usize,
) -> Result<usize, String> {
    let (samples, sr) = read_wav_i16(path).map_err(|e| e.to_string())?;
    let f: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let f12 = if sr == MODEM_RATE {
        f
    } else {
        resample_linear(&f, sr, MODEM_RATE)
    };
    let iwave_all = capture_to_i16(&f12);

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
        // Distinct per (file, slot) — see the banner. The Mode layer turns
        // frame_time_ms into mskrtd's nutc, and a repeated value suppresses decodes.
        let frame_time_ms = ((file_index * n_slots + slot) as i64) * (period_s as i64) * 1000;
        let req = DecodeRequest {
            iwave: &frame,
            nfa: 200,
            nfb: 2900,
            ndepth: 3,
            mycall: "",
            hiscall: "",
            nqso_progress: 0,
            nfqso: 1500,
            frame_time_ms,
        };
        let mut decs = src.decode(&req);
        decs.sort_by(|a, b| {
            a.freq
                .partial_cmp(&b.freq)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for d in &decs {
            println!(
                "{} {:.1} {} {:+.2} {}",
                slot * period_s,
                d.freq,
                d.snr,
                d.dt,
                d.message.trim()
            );
            total += 1;
        }
    }
    Ok(total)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut period_s: u16 = 15;
    let mut files: Vec<String> = Vec::new();

    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--period" => match it.next().and_then(|v| v.parse::<u16>().ok()) {
                Some(v) => period_s = v,
                None => {
                    eprintln!("--period needs one of {:?}", ModeKind::MSK144_PERIODS);
                    std::process::exit(2);
                }
            },
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        eprintln!("usage: decode_wav_msk144 [--period 5|10|15|30] FILE.wav [...]");
        std::process::exit(2);
    }
    if !ModeKind::MSK144_PERIODS.contains(&period_s) {
        eprintln!("unsupported MSK144 period {period_s}");
        std::process::exit(2);
    }

    let kind = ModeKind::Msk144 { period_s };
    eprintln!(
        "# mode: {} (blind, ndepth 3, 200-2900 Hz, {} s period, {} samples/frame)",
        kind.as_str(),
        period_s,
        kind.frame_samples()
    );
    for (i, path) in files.iter().enumerate() {
        eprintln!("# file: {path}");
        match decode_file(path, kind, period_s as usize, i) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}
