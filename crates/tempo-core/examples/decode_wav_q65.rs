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
//!   * ONE PERIOD AND ONE SUBMODE. The C ABI pins Q65-30A (30 s / 360000
//!     samples), because `q65_decode` sizes its frame from ntrperiod. Files are
//!     sliced at exactly `ModeKind::Q65.frame_samples()`.
//!   * `frame_time_ms` steps by 30000, matching the slot.
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
const SLOT_SECS: usize = 30;

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

fn decode_file(path: &str) -> Result<usize, String> {
    let (samples, sr) = read_wav_i16(path).map_err(|e| e.to_string())?;
    let f: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let f12 = if sr == MODEM_RATE {
        f
    } else {
        resample_linear(&f, sr, MODEM_RATE)
    };
    let iwave_all = capture_to_i16(&f12);

    // Q65's slot IS its decode frame at the pinned 30 s period.
    let frame_len = ModeKind::Q65.frame_samples();
    let n_slots = (iwave_all.len() / frame_len).max(1);

    let mut src = NativeSource::from_kind(ModeKind::Q65);
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
            frame_time_ms: (slot as i64) * (SLOT_SECS as i64) * 1000,
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
                slot * SLOT_SECS,
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: decode_wav_q65 FILE.wav [FILE2.wav ...]");
        std::process::exit(2);
    }
    eprintln!(
        "# mode: Q65-30A (blind, ndepth 3, 200-2900 Hz, {} s period, {} samples/frame)",
        SLOT_SECS,
        ModeKind::Q65.frame_samples()
    );
    for path in &args {
        eprintln!("# file: {path}");
        match decode_file(path) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}
