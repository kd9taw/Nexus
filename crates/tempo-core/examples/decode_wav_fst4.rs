//! Nexus-as-configured FST4 decoder CLI — the FST4 arm of the parity lab.
//! DEV-ONLY: an `example`, never shipped.
//!
//! Why this exists at all: Nexus ships FST4 RECEIVE-ONLY, so there is no
//! `fst4_encode` in libtempo and no way to synthesise an FST4 signal in-tree.
//! Every FST4 test before this one proved the decoder RUNS and stays silent on
//! noise — not that it decodes anything. Measuring sensitivity needs stock
//! `fst4sim` for signals and stock `jt9 -7` as the reference decoder, with this
//! CLI as the Nexus side of the comparison.
//!
//! Mirrors decode_wav_ft8.rs / decode_wav_ft4.rs, with the FST4 specifics:
//!
//!   * `decode()`, not `decode_a7()`. FST4 has no cross-cycle replay table.
//!   * ONE T/R PERIOD. The C ABI pins 15 s / 180000 samples, because
//!     `fst4_decode` sizes its frame from ntrperiod. Feeding it a file cut for a
//!     different period would read the wrong span, so files are sliced at
//!     exactly `ModeKind::Fst4.frame_samples()`.
//!   * `frame_time_ms` steps by 15000, matching the slot.
//!
//! Runs BLIND (empty mycall/hiscall, nqso_progress 0) so the stock comparison is
//! fair — jt9 must be run `-Q 0 -c b -x b` to match, or AP is armed on one side
//! only and the numbers are meaningless. That mistake cost a whole calibration
//! round on the FT8 side; see reference-decode-parity-lab.
//!
//! Output (stdout), one line per decode, same shape as the FT8/FT4 CLIs so one
//! parser reads all three:
//!     <slot_offset_s> <freq_hz> <snr_db> <dt_s> <message>
//!
//! Usage:  cargo run -q -p tempo-core --example decode_wav_fst4 -- FILE.wav [...]

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12000;

/// Stateless linear resample, matching the other CLIs' `old` front end. Only used
/// if the input is not already 12 kHz; fst4sim emits 12 kHz.
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

    // FST4's slot IS its decode frame at the pinned 15 s period (unlike FT4, where
    // the 7.5 s slot is longer than the 6.048 s frame).
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
            nftx: 0,
            frame_time_ms: (slot as i64) * (period_s as i64) * 1000,
            ap: true, // stock (FT8/FT4 AP controls; inert for FST4)
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
    let mut wspr = false;
    let mut files: Vec<String> = Vec::new();

    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--period" => match it.next().and_then(|v| v.parse::<u16>().ok()) {
                Some(v) => period_s = v,
                None => {
                    eprintln!("--period needs one of {:?}", ModeKind::FST4_PERIODS);
                    std::process::exit(2);
                }
            },
            // FST4W: the WSPR-like beacon mode. 50-bit messages, no AP.
            "--wspr" => wspr = true,
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        eprintln!(
            "usage: decode_wav_fst4 [--period 15|30|60|120|300|900|1800] [--wspr] FILE.wav [...]"
        );
        std::process::exit(2);
    }
    if !ModeKind::FST4_PERIODS.contains(&period_s) {
        eprintln!("unsupported FST4 period {period_s}");
        std::process::exit(2);
    }

    let kind = ModeKind::Fst4 { period_s, wspr };
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
