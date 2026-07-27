//! Nexus-as-configured Q65 decoder CLI — the Q65 arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Why this exists: Nexus ships Q65 RECEIVE-ONLY, so there is no `q65_encode` in
//! libtempo and no way to synthesise a Q65 signal in-tree. Every Q65 test before
//! this one proves the decoder RUNS and stays silent on noise — not that it
//! decodes anything. Measuring sensitivity needs stock `q65sim` for signals and
//! stock `jt9 -3` as the reference decoder, with this CLI as the Nexus side.
//!
//! Mirrors decode_wav_fst4.rs, with the Q65 specifics:
//!
//!   * PERIOD AND SUBMODE ARE SELECTABLE (`--period`, `--submode`). The frame
//!     length follows the period, so files are sliced at exactly
//!     `ModeKind::Q65{..}.frame_samples()` = period*12000. Defaults to 60A, the
//!     EME working combination, NOT the 30A the ABI was once pinned to.
//!   * `frame_time_ms` steps by the period, matching the slot.
//!   * Q65 reports `idec` (which decode strategy won) where FT8 reports
//!     `iaptype`; the shared `Decode` carries it in `nap`. It is printed here so
//!     a parity run can tell a q3 list decode from an independent one — see
//!     below, because that distinction decides what a "decode" is worth.
//!
//! Runs BLIND (empty mycall/hiscall, nqso_progress 0) so the stock comparison is
//! fair — jt9 must be run with AP off to match, or AP is armed on one side only
//! and the numbers are meaningless. That mistake cost a whole calibration round
//! on the FT8 side; see reference-decode-parity-lab.
//!
//! ⭐ BLIND MATTERS MORE FOR Q65 THAN FOR FT8. Q65's q3 decode does not recover a
//! message independently: it matches against a candidate list pre-built from
//! mycall/hiscall/hisgrid, so with AP armed the decoder is being handed the
//! answer and "yield" measures the list, not the demodulator. Any ladder that
//! reports a q3-heavy yield with callsigns supplied is measuring nothing.
//!
//! Output (stdout), one line per decode, same shape as the FT8/FT4/FST4 CLIs so
//! one parser reads all of them, plus a trailing idec field:
//!     <slot_offset_s> <freq_hz> <snr_db> <dt_s> <message> [idec=<n>]
//!
//! Usage:  cargo run -q -p tempo-core --example decode_wav_q65 -- FILE.wav [...]

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12000;

/// Stateless linear resample, matching the other CLIs' front end. Only used if the
/// input is not already 12 kHz; q65sim emits 12 kHz.
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

fn decode_file(path: &str, kind: ModeKind, period_s: usize) -> Result<usize, String> {
    let (samples, sr) = read_wav_i16(path).map_err(|e| e.to_string())?;
    let f: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let f12 = if sr == MODEM_RATE {
        f
    } else {
        resample_linear(&f, sr, MODEM_RATE)
    };
    let iwave_all = capture_to_i16(&f12);

    // Q65's slot IS its decode frame, whatever the period.
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
            nfqso: 0,
            frame_time_ms: (slot as i64) * (period_s as i64) * 1000,
        };
        let mut decs = src.decode(&req);
        decs.sort_by(|a, b| {
            a.freq
                .partial_cmp(&b.freq)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for d in &decs {
            println!(
                "{} {:.1} {} {:+.2} {} idec={}",
                slot * period_s,
                d.freq,
                d.snr,
                d.dt,
                d.message.trim(),
                d.nap
            );
            total += 1;
        }
    }
    Ok(total)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // Default 60A, not 30A: EME is Q65's flagship use and works Q65-60A/B/C.
    let mut period_s: u16 = 60;
    let mut submode: u8 = 0;
    let mut files: Vec<String> = Vec::new();

    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--period" => match it.next().and_then(|v| v.parse::<u16>().ok()) {
                Some(v) => period_s = v,
                None => {
                    eprintln!("--period needs one of {:?}", ModeKind::Q65_PERIODS);
                    std::process::exit(2);
                }
            },
            // Accepts a letter (A..E) or a digit (0..4) — the on-air name uses the
            // letter, the ABI uses the index, and typing either should work.
            "--submode" => match it.next().map(|v| parse_submode(v)) {
                Some(Some(v)) => submode = v,
                _ => {
                    eprintln!("--submode needs A..E or 0..4");
                    std::process::exit(2);
                }
            },
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        eprintln!(
            "usage: decode_wav_q65 [--period 15|30|60|120|300] [--submode A..E] FILE.wav [...]"
        );
        std::process::exit(2);
    }
    if !ModeKind::Q65_PERIODS.contains(&period_s) || submode >= ModeKind::Q65_SUBMODES {
        eprintln!("unsupported Q65-{period_s} submode {submode}");
        std::process::exit(2);
    }

    let kind = ModeKind::Q65 { period_s, submode };
    eprintln!(
        "# mode: {} (blind, ndepth 3, 200-2900 Hz, {} s period, {} samples/frame)",
        kind.as_str(),
        period_s,
        kind.frame_samples()
    );
    for path in &files {
        eprintln!("# file: {path}");
        match decode_file(path, kind, period_s as usize) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}

/// `A`..`E` (either case) or `0`..`4` -> the ABI's 0-based submode index.
fn parse_submode(v: &str) -> Option<u8> {
    if let Ok(n) = v.parse::<u8>() {
        return (n < ModeKind::Q65_SUBMODES).then_some(n);
    }
    let c = v.chars().next()?.to_ascii_uppercase();
    (('A'..='E').contains(&c)).then(|| c as u8 - b'A')
}
