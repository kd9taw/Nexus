//! Nexus-as-configured FT4 decoder CLI — the FT4 half of the parity lab, for
//! measuring Nexus decoder changes on identical audio. DEV-ONLY: this is an
//! `example`, never shipped.
//!
//! Mirrors `decode_wav_ft8.rs` so the two are comparable, with three FT4-specific
//! differences that matter:
//!
//!   * `decode()`, not `decode_a7()`. The a7 cross-cycle replay table is FT8-only;
//!     FT4 has no equivalent, so there is no per-file state to reset and frames
//!     are independent by construction.
//!
//!   * Slot and decode frame are DIFFERENT lengths. FT8's slot is its frame
//!     (180000 both). FT4's slot is 7.5 s (90000 samples) while the decode frame
//!     is NMAX = 72576 (6.048 s), and the decoder reads the HEAD of the slot,
//!     where the leading Costas sync lives. Feeding it the tail amputates sync.
//!     Files shorter than a slot (stock ft4sim / ft4sim_mult emit exactly one
//!     72576-sample frame) are handled as a single frame.
//!
//!   * `frame_time_ms` steps by 7500, matching engine.rs's FT4 slot key.
//!
//! A-priori context is left empty (blank mycall/hiscall, nqso_progress 0) so the
//! comparison is a blind sensitivity test, matching how jt9 is run in this lab
//! with `-Q 0 -c b -x b`. Leaving AP armed on one side only is the single
//! easiest way to produce a confident wrong number here.
//!
//! Output (stdout), one line per decode, identical shape to the FT8 CLI so the
//! same parser reads both:
//!     <slot_offset_s> <freq_hz> <snr_db> <dt_s> <message>
//!
//! Usage:  cargo run -q -p tempo-core --example decode_wav_ft4 -- FILE.wav [...]

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

const MODEM_RATE: u32 = 12000;

/// Stateless linear resample, matching the FT8 CLI's `old` front end. Only used
/// when the input is not already 12 kHz; the lab's generators emit 12 kHz.
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

    let frame_len = ModeKind::Ft4.frame_samples(); // 72576, the decode window
    let slot_len = ModeKind::Ft4.capture_samples(); // 90000, the full T/R period

    // The decoder reads the head of each SLOT. Step by slot, not by frame, so a
    // multi-slot recording lands each frame on its true slot boundary. A file
    // shorter than one slot (the lab's generators emit exactly one frame) is a
    // single frame.
    let n_slots = if iwave_all.len() <= frame_len {
        1
    } else {
        iwave_all.len().div_ceil(slot_len)
    };

    let mut src = NativeSource::from_kind(ModeKind::Ft4);
    let mut total = 0usize;

    for slot in 0..n_slots {
        let start = slot * slot_len;
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
            frame_time_ms: (slot as i64) * 7_500, // FT4 slot key
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
                (slot as f32 * 7.5) as i32,
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: decode_wav_ft4 FILE.wav [FILE2.wav ...]");
        std::process::exit(2);
    }
    eprintln!("# mode: FT4 (blind, ndepth 3, 200-2900 Hz)");
    for path in &args {
        eprintln!("# file: {path}");
        match decode_file(path) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}
