//! Nexus-as-configured FT8 decoder CLI — the "parity lab" reference for
//! measuring Nexus decoder changes against stock WSJT-X (`jt9 -8 -d 3`) on
//! identical audio. DEV-ONLY: this is an `example`, never shipped.
//!
//! It runs the EXACT decode path `tempo-app/src/engine.rs` uses for the "Deep"
//! setting (`decode_depth = 3`) on the authoritative slot-boundary pass:
//!
//!   captured f32 --capture_to_i16--> i16 frame
//!     -> modes::DecodeRequest { ndepth: 3, nfa: 200, nfb: 2900, nfqso: 0, no AP }
//!     -> NativeSource(FT8).decode_a7(&req, /*a7_final=*/ true)
//!
//! That is byte-for-byte the `Engine::decode_frame(.., a7_final = true)` boundary
//! ingest (see engine.rs ~6143): same scaling, same passband, same depth, same
//! a7 cross-cycle entry point. A-priori context is left empty (blank mycall/
//! hiscall, nqso_progress 0) so the comparison is a blind sensitivity test —
//! matching how jt9 is run here with no `-c/-x`.
//!
//! Input: 16-bit PCM mono WAV at 12 kHz, or 48 kHz. The 48k->12k front-end is
//! selectable with `--frontend`:
//!   * `old` (default) — the verbatim `resample_linear` the live capture path
//!     USED to run (stateless linear interp; at exact 4:1 it degenerates to
//!     take-every-4th with NO anti-alias, so all 6-24 kHz energy folds into the
//!     0-6 kHz passband at 0 dB).
//!   * `new` — `tempo_audio::capture_resample::CaptureResampler` (64-tap Hann
//!     polyphase sinc, fc 4500 Hz, ~58 dB alias rejection), the resampler the
//!     live capture path runs TODAY. Vendored verbatim below (see module note).
//!
//! Audio is sliced into 15 s slots (180000 samples); a7 cross-cycle state is
//! reset per file and threaded across slots within a file (exactly as the live
//! engine does across a continuous recording).
//!
//! Output (stdout), one line per decode:
//!     <slot_utc_offset_s> <freq_hz> <snr_db> <dt_s> <message>
//!
//! Usage:  cargo run -q -p tempo-core --example decode_wav_ft8 -- [--frontend old|new] FILE.wav [...]

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use tempo_core::channel::capture_to_i16;
use tempo_core::wavfile::read_wav_i16;

/// FT8 full-slot frame length: 15 s * 12000 Hz. Equals `ft8::NMAX`
/// (`ft1_sys::FT8_NMAX = 180_000`); hardcoded because tempo-core does not depend
/// on the `ft8` crate directly.
const FT8_NMAX: usize = 180_000;
const MODEM_RATE: u32 = 12_000;

/// Verbatim copy of `tempo_audio::resample::resample_linear` — the resampler the
/// live sound-card capture path runs (see `tempo-audio/src/device.rs`, which
/// calls `resample_linear(&dev, in_rate, MODEM_RATE)`). Vendored here so this
/// dev-only example needs no dependency on `tempo-audio` (which pulls the whole
/// app in). Bit-identical, so the 48k->12k path matches Nexus exactly.
fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if input.is_empty() || in_rate == 0 || out_rate == 0 {
        return Vec::new();
    }
    if in_rate == out_rate {
        return input.to_vec();
    }
    let ratio = out_rate as f64 / in_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let i0 = src.floor() as usize;
        let frac = (src - i0 as f64) as f32;
        let a = input.get(i0).copied().unwrap_or(0.0);
        let b = input.get(i0 + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    out
}

/// Verbatim copy of `tempo_audio::capture_resample` (the production module) minus
/// its `#[cfg(test)]` block — the anti-aliased polyphase resampler the live
/// capture path runs TODAY. Vendored here (like `resample_linear` above) because
/// tempo-audio depends on tempo-core, so tempo-core cannot depend back on it. The
/// DSP is pure (no `device` feature) and copied byte-for-byte, so `new` matches
/// Nexus's live 48k->12k front-end exactly. Source of truth:
/// `crates/tempo-audio/src/capture_resample.rs`.
mod capture_resample {
    const FIR_TAPS: usize = 64;
    const NUM_PHASES: usize = 256;
    const CUTOFF_FACTOR: f64 = 0.45;
    const CUTOFF_CAP_HZ: f64 = 4500.0;

    fn cutoff_hz(in_rate: u32, out_rate: u32) -> f64 {
        (f64::from(in_rate.min(out_rate)) * CUTOFF_FACTOR).min(CUTOFF_CAP_HZ)
    }

    fn fir_tap(tap_index: usize, frac: f64, fc: f64) -> f32 {
        let m = FIR_TAPS as f64;
        let n = (tap_index as f64) - (m - 1.0) / 2.0 - frac;
        let sinc = if n.abs() < 1e-12 {
            2.0 * fc
        } else {
            (2.0 * std::f64::consts::PI * fc * n).sin() / (std::f64::consts::PI * n)
        };
        let w = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * (tap_index as f64) / (m - 1.0)).cos());
        (sinc * w) as f32
    }

    pub struct CaptureResampler {
        passthrough: bool,
        in_rate: u32,
        out_rate: u32,
        frac_num: u64,
        tail: Vec<f32>,
        taps: Box<[[f32; FIR_TAPS]; NUM_PHASES]>,
    }

    impl CaptureResampler {
        #[must_use]
        pub fn new(in_rate: u32, out_rate: u32) -> Self {
            let passthrough = in_rate == 0 || out_rate == 0 || in_rate == out_rate;
            let mut taps: Box<[[f32; FIR_TAPS]; NUM_PHASES]> =
                Box::new([[0.0_f32; FIR_TAPS]; NUM_PHASES]);
            if !passthrough {
                let cutoff_norm = cutoff_hz(in_rate, out_rate) / f64::from(in_rate);
                for phase_idx in 0..NUM_PHASES {
                    let frac = (phase_idx as f64) / (NUM_PHASES as f64);
                    for k in 0..FIR_TAPS {
                        taps[phase_idx][k] = fir_tap(k, frac, cutoff_norm);
                    }
                }
            }
            Self {
                passthrough,
                in_rate,
                out_rate,
                frac_num: 0,
                tail: Vec::new(),
                taps,
            }
        }

        #[must_use]
        pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
            if self.passthrough {
                return input.to_vec();
            }
            if input.is_empty() && self.tail.is_empty() {
                return Vec::new();
            }

            let mut buf = std::mem::take(&mut self.tail);
            buf.extend_from_slice(input);

            let den = u64::from(self.out_rate);
            let step = u64::from(self.in_rate);
            let mut out = Vec::new();
            loop {
                let i0 = (self.frac_num / den) as usize;
                if i0 + FIR_TAPS > buf.len() {
                    break;
                }
                let frac = (self.frac_num % den) as f64 / den as f64;
                let phase_idx = ((frac * NUM_PHASES as f64).round() as usize).min(NUM_PHASES - 1);
                let taps = &self.taps[phase_idx];
                let mut acc = 0.0_f32;
                for k in 0..FIR_TAPS {
                    acc += taps[k] * buf[i0 + k];
                }
                out.push(acc);
                self.frac_num += step;
            }

            let drop = ((self.frac_num / den) as usize).min(buf.len());
            self.tail = buf[drop..].to_vec();
            self.frac_num -= drop as u64 * den;
            out
        }
    }
}

/// Which 48k->12k front-end the A/B measurement runs.
#[derive(Clone, Copy, PartialEq)]
enum Frontend {
    /// `resample_linear` — the old, no-anti-alias linear/decimate path.
    Old,
    /// `CaptureResampler` — the new anti-aliased polyphase path.
    New,
}

fn decode_file(path: &str, frontend: Frontend) -> std::io::Result<usize> {
    let (samples, sr) = read_wav_i16(path)?;
    // Soundcard capture delivers normalized f32 (i16 / 32768); the engine then
    // restores full-scale i16 via capture_to_i16. Mirror that exactly so the
    // decoder sees the level WSJT-X/jt9 see from the same file.
    let f: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let f12 = if sr == MODEM_RATE {
        f
    } else {
        match frontend {
            Frontend::Old => resample_linear(&f, sr, MODEM_RATE),
            Frontend::New => capture_resample::CaptureResampler::new(sr, MODEM_RATE).process(&f),
        }
    };
    let iwave_all = capture_to_i16(&f12);

    let n_slots = (iwave_all.len() / FT8_NMAX).max(1);
    let mut src = NativeSource::from_kind(ModeKind::Ft8);
    modes::reset_ft8_a7(); // fresh a7 cross-cycle table per file

    let mut total = 0usize;
    for slot in 0..n_slots {
        let start = slot * FT8_NMAX;
        let mut frame = vec![0i16; FT8_NMAX];
        if start < iwave_all.len() {
            let end = (start + FT8_NMAX).min(iwave_all.len());
            frame[..end - start].copy_from_slice(&iwave_all[start..end]);
        }
        let req = DecodeRequest {
            iwave: &frame,
            nfa: 200,   // engine decode_flow_hz default
            nfb: 2900,  // engine decode_fhigh_hz default
            ndepth: 3,  // "Deep" (decode_depth default)
            mycall: "", // blind decode (no a-priori) — like jt9 w/o -c
            hiscall: "",
            nqso_progress: 0,
            nfqso: 0, // band center (no worked-station bias)
            // Slot key: slot * 15000 ms, exactly engine's frame_time_ms for FT8.
            frame_time_ms: (slot as i64) * 15_000,
        };
        let mut decs = src.decode_a7(&req, true); // authoritative boundary pass
        decs.sort_by(|a, b| {
            a.freq
                .partial_cmp(&b.freq)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for d in &decs {
            // slot_utc_offset(s)  freq(Hz)  snr(dB)  dt(s)  message
            println!(
                "{} {:.1} {} {:+.2} {}",
                slot * 15,
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
    // `--frontend old|new` selects the 48k->12k resampler (default old = the
    // historical live-capture path). Everything else is treated as a wav path.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut frontend = Frontend::Old;
    let mut files: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--frontend" => match it.next().map(String::as_str) {
                Some("old") => frontend = Frontend::Old,
                Some("new") => frontend = Frontend::New,
                other => {
                    eprintln!("--frontend expects old|new, got {other:?}");
                    std::process::exit(2);
                }
            },
            other if other.starts_with("--") => {} // ignore unknown flags, as before
            _ => files.push(a.clone()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: decode_wav_ft8 [--frontend old|new] FILE.wav [FILE2.wav ...]");
        std::process::exit(2);
    }
    eprintln!(
        "# frontend: {}",
        if frontend == Frontend::New {
            "new"
        } else {
            "old"
        }
    );
    for path in &files {
        eprintln!("# file: {path}");
        match decode_file(path, frontend) {
            Ok(n) => eprintln!("# {path}: {n} decode(s)"),
            Err(e) => eprintln!("# {path}: ERROR {e}"),
        }
    }
}
