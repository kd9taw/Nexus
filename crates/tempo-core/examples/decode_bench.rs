//! **THROWAWAY DEV HARNESS — decode-cost measurement for the multi-radio ruling.**
//!
//! Not shipped (an `example`, never linked into the app). Created 2026-07-22 to put a
//! NUMBER on "what does one FT8/FT4 decode actually cost", so the in-process vs
//! process-per-radio decision has arithmetic instead of adjectives. Delete freely.
//!
//! It runs the EXACT decode path `tempo-app/src/engine.rs::run_decode_job` uses on the
//! native branch, at the shipped defaults (`ndepth = 3`, `nfa = 200`, `nfb = 2900`,
//! `nfqso = 0`, no AP context):
//!
//!   synthesized busy slot (f32) --capture_to_i16--> i16 frame
//!     -> modes::DecodeRequest { .. }
//!     -> NativeSource(FT8|FT4).decode_a7(&req, a7_final)
//!
//! Two passes are measured, mirroring `service.rs`:
//! * `boundary` — full slot audio, `a7_final = true` (FT8 a7 save + replay).
//! * `early` — the WSJT-X-style partial: the first 11.8 s (FT8) / 5.5 s (FT4) of the
//!   slot at the FRONT of a zero-tail-padded full-length frame, exactly
//!   `RxRing::frame_latest_padded`, `a7_final = false`.
//!
//! `--ctx` additionally brackets each decode in `DecoderCtx::scoped` (the per-chain
//! modem-state save/restore the multi-radio design would use), so the context overhead
//! is measured rather than quoted.
//!
//! The band is synthesized, not recorded: N independent FT8/FT4 signals (distinct
//! callsign pairs, so packjt77 hashing and the AP machinery see real variety) at
//! random audio frequencies over 300..2700 Hz, random DT in −0.3..+0.5 s, random SNR
//! in −21..+3 dB, summed into unit-variance AWGN and level-normalized to a realistic
//! soundcard RMS before `capture_to_i16`. Signal amplitude uses the repo's own
//! `channel::snr_to_scale` (the WSJT-X 2500 Hz-bandwidth convention).
//!
//! Usage:
//!   cargo run --release -p tempo-core --example decode_bench -- \
//!       [--mode ft8|ft4] [--signals N] [--slots M] [--ndepth D] [--ctx] [--ctx-only]
//!
//! Output: one summary line per (mode, pass) with n / decodes / p50 / p90 / p95 / max / min.

use modes::{DecodeRequest, ModeKind, NativeSource, SignalSource};
use std::time::Instant;
use tempo_core::channel::{capture_to_i16, snr_to_scale};

const FS: f32 = 12_000.0;

/// Deterministic LCG — same generator as `tempo_core::channel::Awgn`, reused here so the
/// harness needs no RNG dependency and every run is reproducible.
struct Lcg(u64);
impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 32) as u32
    }
    fn f01(&mut self) -> f64 {
        (self.next_u32() as f64 + 1.0) / (u32::MAX as f64 + 2.0)
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f01()
    }
    /// N(0,1) via Box-Muller — identical construction to `Awgn::sample`.
    fn gauss(&mut self) -> f32 {
        let u1 = self.f01();
        let u2 = self.f01();
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

/// A plausible-looking unique callsign, so no two signals in a slot collide in the
/// packjt77 hash table (which would make the decode unrealistically cheap/odd).
fn callsign(i: usize) -> String {
    const P: [&str; 8] = ["K", "W", "N", "AA", "KD", "WB", "VE", "G"];
    let pfx = P[i % P.len()];
    let digit = (i / P.len()) % 10;
    let a = (b'A' + (i % 26) as u8) as char;
    let b = (b'A' + ((i / 26) % 26) as u8) as char;
    let c = (b'A' + ((i / 7) % 26) as u8) as char;
    format!("{pfx}{digit}{a}{b}{c}")
}

/// One of the four standard FT8/FT4 QSO messages, cycling so the slot looks like a
/// real band (CQ, call, report, RRR) rather than 30 copies of one frame.
fn message(i: usize) -> String {
    let me = callsign(i);
    let dx = callsign(i + 37);
    match i % 4 {
        0 => format!("CQ {me} EM69"),
        1 => format!("{dx} {me} FN42"),
        2 => format!("{dx} {me} -{:02}", 5 + (i % 15)),
        _ => format!("{dx} {me} RR73"),
    }
}

/// Build one busy slot of `n_sig` signals as normalized capture-level f32 audio,
/// `frame_len` samples long (the mode's full decode frame).
fn busy_slot(kind: ModeKind, n_sig: usize, frame_len: usize, rng: &mut Lcg) -> Vec<f32> {
    let mode = modes::make_mode(kind);
    let mut buf = vec![0f32; frame_len];
    let mut placed = 0usize;
    for i in 0..n_sig {
        let msg = message(i.wrapping_add(rng.next_u32() as usize % 1000));
        let tones = mode.encode(&msg);
        if tones.is_empty() {
            continue;
        }
        let f0 = rng.range(300.0, 2700.0) as f32;
        let wave = mode.gen_wave(&tones, FS, f0); // slot-positioned (0.5 s lead-in)
        let snr = rng.range(-21.0, 3.0) as f32;
        let scale = snr_to_scale(snr, FS);
        // DT jitter around nominal; negative shifts the signal earlier in the slot.
        let dt = rng.range(-0.3, 0.5);
        let off = (dt * FS as f64).round() as isize;
        for (j, &s) in wave.iter().enumerate() {
            let k = j as isize + off;
            if k >= 0 && (k as usize) < frame_len {
                buf[k as usize] += scale * s;
            }
        }
        placed += 1;
    }
    debug_assert!(placed > 0);
    // Unit-variance AWGN, then level-normalize the whole slot to a realistic
    // soundcard level (RMS 0.05 of full scale ≈ −26 dBFS) before capture_to_i16,
    // which multiplies by 32767. Scaling the SUM preserves every signal's SNR.
    for s in buf.iter_mut() {
        *s += rng.gauss();
    }
    let rms = (buf.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / buf.len() as f64).sqrt();
    let g = if rms > 0.0 { 0.05 / rms as f32 } else { 1.0 };
    for s in buf.iter_mut() {
        *s *= g;
    }
    buf
}

/// `RxRing::frame_latest_padded(n)` for a synthesized slot: the first `n` samples of the
/// slot at the FRONT, zero-padded out to the full frame length.
fn early_frame(full: &[f32], n: usize) -> Vec<f32> {
    let mut out = vec![0f32; full.len()];
    let take = n.min(full.len());
    out[..take].copy_from_slice(&full[..take]);
    out
}

struct Stats {
    v: Vec<f64>,
}
impl Stats {
    fn new(mut v: Vec<f64>) -> Self {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Self { v }
    }
    fn q(&self, p: f64) -> f64 {
        if self.v.is_empty() {
            return 0.0;
        }
        // Nearest-rank; with n≈40 this is honest about being a small sample.
        let idx = ((p * self.v.len() as f64).ceil() as usize).clamp(1, self.v.len()) - 1;
        self.v[idx]
    }
    fn min(&self) -> f64 {
        *self.v.first().unwrap_or(&0.0)
    }
    fn max(&self) -> f64 {
        *self.v.last().unwrap_or(&0.0)
    }
    fn mean(&self) -> f64 {
        if self.v.is_empty() {
            0.0
        } else {
            self.v.iter().sum::<f64>() / self.v.len() as f64
        }
    }
}

fn report(label: &str, ms: Vec<f64>, decodes: usize) {
    let n = ms.len();
    let s = Stats::new(ms);
    println!(
        "{label:<34} n={n:<3} dec/slot={:<5.1} min={:<8.1} p50={:<8.1} p90={:<8.1} p95={:<8.1} max={:<8.1} mean={:<8.1} (ms)",
        decodes as f64 / n.max(1) as f64,
        s.min(),
        s.q(0.50),
        s.q(0.90),
        s.q(0.95),
        s.max(),
        s.mean(),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mode_s = "ft8".to_string();
    let mut n_sig = 25usize;
    let mut slots = 40usize;
    let mut ndepth = 3i32;
    let mut use_ctx = false;
    let mut ctx_only = false;
    let mut shift_scan = false;
    let mut chains = 1usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                mode_s = args[i].clone();
            }
            "--signals" => {
                i += 1;
                n_sig = args[i].parse().unwrap();
            }
            "--slots" => {
                i += 1;
                slots = args[i].parse().unwrap();
            }
            "--ndepth" => {
                i += 1;
                ndepth = args[i].parse().unwrap();
            }
            "--ctx" => use_ctx = true,
            "--ctx-only" => ctx_only = true,
            "--shift-scan" => shift_scan = true,
            "--chains" => {
                i += 1;
                chains = args[i].parse().unwrap();
            }
            other => panic!("unknown arg {other}"),
        }
        i += 1;
    }

    // --- Pure context save/restore microbench (no decode at all). -------------
    if ctx_only {
        let mut ctx = tempo_fast::DecoderCtx::new();
        println!("tempo_ctx_size() = {} bytes", ctx.len());
        // Warm.
        for _ in 0..50 {
            ctx.scoped(|| std::hint::black_box(0u32));
        }
        let mut v = Vec::new();
        for _ in 0..2000 {
            let t = Instant::now();
            ctx.scoped(|| std::hint::black_box(0u32));
            v.push(t.elapsed().as_secs_f64() * 1e3);
        }
        report("ctx restore+save (empty)", v, 0);
        return;
    }

    let kind = match mode_s.as_str() {
        "ft8" => ModeKind::Ft8,
        "ft4" => ModeKind::Ft4,
        other => panic!("mode must be ft8|ft4, got {other}"),
    };

    // --- N CHAINS, one process, one shared gate: the real multi-radio cost. --
    // Each "chain" is its own thread with its own NativeSource and its own
    // DecoderCtx, all serialized by ONE shared mutex held across
    // restore→decode→save (the guard `run_decode_job` would need across chains —
    // note the shipped guard is the PER-ENGINE `source` mutex, which does not
    // serialize two engines at all; see engine.rs:293-303). Reports the WALL time
    // for all N chains to finish one slot's early passes, and one slot's boundary
    // passes — the numbers the 11.8→15.0 s window has to absorb.
    if chains > 1 {
        use std::sync::{Arc, Mutex};
        let frame_len = kind.frame_samples();
        let cap = kind.capture_samples();
        let early_n = match kind {
            ModeKind::Ft8 => (11.8 * FS as f64) as usize,
            _ => (5.5 * FS as f64) as usize,
        };
        let mut rng = Lcg(0x5EED_1234_ABCD_0001);
        // A DIFFERENT busy band per chain (two radios are on two bands).
        let bands: Vec<Vec<Vec<f32>>> = (0..chains)
            .map(|_| {
                (0..slots)
                    .map(|_| busy_slot(kind, n_sig, cap, &mut rng))
                    .collect()
            })
            .collect();
        let gate = Arc::new(Mutex::new(()));
        let bands = Arc::new(bands);
        for (pass_name, a7f) in [("early", false), ("boundary", true)] {
            let mut wall = Vec::new();
            for s in 0..slots {
                let start = Instant::now();
                std::thread::scope(|sc| {
                    for c in 0..chains {
                        let gate = Arc::clone(&gate);
                        let bands = Arc::clone(&bands);
                        sc.spawn(move || {
                            let mut src = NativeSource::from_kind(kind);
                            let mut ctx = tempo_fast::DecoderCtx::new();
                            let audio = &bands[c][s];
                            let win: Vec<f32> = if a7f {
                                audio[..frame_len.min(audio.len())].to_vec()
                            } else {
                                let mut w = vec![0f32; frame_len];
                                let t = early_n.min(frame_len).min(audio.len());
                                w[..t].copy_from_slice(&audio[..t]);
                                w
                            };
                            let iw = capture_to_i16(&win);
                            let req = req_for(&iw, ndepth, s as i64 * 15_000);
                            let _g = gate.lock().unwrap();
                            let _ = ctx.scoped(|| src.decode_a7(&req, a7f));
                        });
                    }
                });
                wall.push(start.elapsed().as_secs_f64() * 1e3);
            }
            report(
                &format!("{} {chains}x {pass_name} WALL", kind.as_str()),
                wall,
                0,
            );
        }
        return;
    }

    // --- Delayed-boundary decode-YIELD scan. ---------------------------------
    // A boundary decode that has to wait Δ for the other chain's lock does not just
    // land late: `RxRing::frame()` returns the latest `cap` samples, so by the time it
    // runs the ring has rolled forward by Δ. The decoder is handed the just-ended
    // slot starting at Δ, with the first Δ seconds GONE and every signal's dt pulled
    // Δ earlier. This measures what that costs in decodes.
    if shift_scan {
        let frame_len = kind.frame_samples();
        let cap = kind.capture_samples();
        let mut rng = Lcg(0x5EED_1234_ABCD_0001);
        // One extra slot: the roll-in audio that displaces the front of the window.
        let audio: Vec<Vec<f32>> = (0..slots + 1)
            .map(|_| busy_slot(kind, n_sig, cap, &mut rng))
            .collect();
        let mut src = NativeSource::from_kind(kind);
        println!(
            "# delayed-boundary yield scan: {} slots, {n_sig} signals/slot",
            slots
        );
        for &shift_s in &[0.0f64, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0] {
            modes::reset_ft8_a7();
            let sh = (shift_s * FS as f64) as usize;
            let mut total = 0usize;
            for s in 0..slots {
                // The ring at boundary+Δ: slot s from Δ onward, then slot s+1's head.
                let mut win = Vec::with_capacity(cap);
                win.extend_from_slice(&audio[s][sh.min(cap)..]);
                win.extend_from_slice(&audio[s + 1][..sh.min(cap)]);
                win.truncate(cap);
                let iw = capture_to_i16(&win[..frame_len.min(win.len())]);
                let req = req_for(&iw, ndepth, s as i64 * (kind.slot_secs() * 1000.0) as i64);
                total += src.decode_a7(&req, true).len();
            }
            println!(
                "  boundary delayed by {shift_s:>4.2} s -> {:>6.2} decodes/slot ({total} total)",
                total as f64 / slots as f64
            );
        }
        return;
    }
    let frame_len = kind.frame_samples();
    let slot_secs = kind.slot_secs() as f64;
    // The early-pass trigger the shipped loop uses (service.rs ~3607).
    let early_at_s = match kind {
        ModeKind::Ft8 => 11.8,
        ModeKind::Ft4 => 5.5,
        _ => unreachable!(),
    };
    let early_n = (early_at_s * FS as f64) as usize;

    eprintln!(
        "# mode={} frame={} samples ({:.2} s) slot={:.1} s early_at={:.1} s signals/slot={} slots={} ndepth={} ctx={}",
        kind.as_str(), frame_len, frame_len as f64 / FS as f64, slot_secs, early_at_s, n_sig, slots, ndepth, use_ctx
    );

    let mut rng = Lcg(0x5EED_1234_ABCD_0001);
    // Pre-synthesize every slot so waveform generation is NOT inside the timer.
    // (gen_wave takes MODEM_LOCK too, so building inline would pollute the number.)
    eprintln!("# synthesizing {slots} busy slots ...");
    let slots_audio: Vec<Vec<f32>> = (0..slots)
        .map(|_| busy_slot(kind, n_sig, frame_len, &mut rng))
        .collect();

    let mut src = NativeSource::from_kind(kind);
    let mut ctx = tempo_fast::DecoderCtx::new();
    if use_ctx {
        eprintln!("# DecoderCtx = {} bytes", ctx.len());
    }
    // Fresh a7 state for the run (the engine does this on band/QSO change).
    modes::reset_ft8_a7();

    let mut t_boundary = Vec::new();
    let mut t_early = Vec::new();
    let mut n_boundary = 0usize;
    let mut n_early = 0usize;
    // `--ctx`: the restore+save bracket cost measured IN SITU, per call, by timing the
    // inside of the closure and subtracting. This is the honest per-decode context
    // overhead — the isolated `--ctx-only` microbench runs with the 3.3 MB buffer hot
    // in cache, which a real decode's working set evicts.
    let mut t_ctx = Vec::new();

    // Two warm-up slots: first-call FFTW plan creation is a one-time process cost and
    // would otherwise land in p95.
    for warm in 0..2 {
        let iw = capture_to_i16(&slots_audio[warm % slots_audio.len()]);
        let req = req_for(&iw, ndepth, 0);
        let _ = src.decode_a7(&req, true);
    }

    for (s, audio) in slots_audio.iter().enumerate() {
        let slot = s as i64;
        let frame_time_ms = slot * (slot_secs * 1000.0) as i64;

        // --- Early pass: partial audio at the front, a7_final = false. --------
        let ef = early_frame(audio, early_n);
        let iw = capture_to_i16(&ef);
        let req = req_for(&iw, ndepth, frame_time_ms);
        let t = Instant::now();
        let mut inner = 0f64;
        let d = if use_ctx {
            ctx.scoped(|| {
                let ti = Instant::now();
                let d = src.decode_a7(&req, false);
                inner = ti.elapsed().as_secs_f64() * 1e3;
                d
            })
        } else {
            src.decode_a7(&req, false)
        };
        let outer = t.elapsed().as_secs_f64() * 1e3;
        if use_ctx {
            t_ctx.push(outer - inner);
        }
        t_early.push(outer);
        n_early += d.len();

        // --- Boundary pass: full audio, a7_final = true. ----------------------
        let iw = capture_to_i16(audio);
        let req = req_for(&iw, ndepth, frame_time_ms);
        let t = Instant::now();
        let mut inner = 0f64;
        let d = if use_ctx {
            ctx.scoped(|| {
                let ti = Instant::now();
                let d = src.decode_a7(&req, true);
                inner = ti.elapsed().as_secs_f64() * 1e3;
                d
            })
        } else {
            src.decode_a7(&req, true)
        };
        let outer = t.elapsed().as_secs_f64() * 1e3;
        if use_ctx {
            t_ctx.push(outer - inner);
        }
        t_boundary.push(outer);
        n_boundary += d.len();
    }

    let tag = if use_ctx { "+ctx" } else { "     " };
    report(&format!("{} early  {tag}", kind.as_str()), t_early, n_early);
    report(
        &format!("{} bound. {tag}", kind.as_str()),
        t_boundary,
        n_boundary,
    );
    if use_ctx {
        report("  ctx restore+save IN SITU", t_ctx, 0);
    }
}

fn req_for<'a>(iwave: &'a [i16], ndepth: i32, frame_time_ms: i64) -> DecodeRequest<'a> {
    // Shipped engine defaults — engine.rs::build_decode_job (nfa/nfb/ndepth/nfqso).
    DecodeRequest {
        iwave,
        nfa: 200,
        nfb: 2900,
        ndepth,
        mycall: "",
        hiscall: "",
        nqso_progress: 0,
        nfqso: 0,
        frame_time_ms,
    }
}
