//! Per-candidate demodulation — JS8Call `js8dec` + `syncjs8d` (JS8.cpp:
//! 1148-1440 and :1906-1970 @ a7ff1be0) read as the SPEC; lineage WSJT-X
//! `ft8b.f90`/`sync8d.f90`, with the JS8 differences applied at the source:
//! NO Gray map — the bit metrics are max-of-four over the plain binary
//! groupings {4..7}, {2,3,6,7}, {1,3,5,7} (JS8.cpp:1313-1333), llr0 on the
//! linear magnitudes and llr1 on their logs (:1324-1332); the speed's own
//! Costas rows for the hard-sync count; LDPC(174,87) BP with CRC-12 as the
//! acceptance test; JS8Call's four inner passes (:1364-1390): llr0, llr1, llr0 with bits 0..24 erased, llr0 with bits
//! 0..48 erased — the erasure is CUMULATIVE because pass 3's `std::fill` on
//! `llr0` persists into pass 4 (:1373-1374). JS8 has no a-priori (callsign)
//! AP and no OSD.
//!
//! CONTRACT: given a coarse `Candidate`, refine time (±NQSYMBOL downsampled
//! samples = ¼ symbol, :1171-1188) then frequency (±NFSRCH·0.5 Hz, :1194-
//! 1210), rotate the baseband by the frequency tweak IN PLACE rather than
//! re-downsampling (:1212-1222), take the 79 per-symbol NDOWNSPS-point DFTs
//! (:1231-1253), bail unless at least 7 of the 21 Costas symbols hard-sync
//! (:1257-1288), build the two LLR sets, normalise (:1336-1358), run the four
//! BP passes and return the FIRST CRC-verified word with its refined
//! `freq_hz` (tone 0), `dt_s` (= `start_s − ASTART`, what JS8Call reports:
//! :2366), `snr_db` (:1427-1431 — tone power over the fitted noise baseline,
//! floor −60) and the 79 tones for subtraction. Pure.
//!
//! Acceptance per pass (:1387-1390): nharderrors in 0..60; not (sync < 2 and
//! nh > 35) where `sync` is the RAW fine-sync power after the tweak (:1227);
//! not (pass > 2 and > 39); not (pass 4 and > 30); the all-zero codeword is
//! rejected first (:1382-1385); CRC-12 must verify (:1392).
//!
//! Two bounds, both JS8Call's: the fine sync only counts Costas symbols
//! that end inside `Geom::np2` = NN·NDOWNSPS (:1950 `Mode::NP2`) — for a
//! frame starting at ASTART the last symbols of block c are OUTSIDE it and
//! do not contribute; the symbol DFTs use the unqualified FT8-era `NP2 =
//! 2812` (:1239) — with cd0 zero past NDFFT2. Reproduced, not "fixed": a
//! different bound changes which weak frames sync and would break the
//! stock-decode reproduction test.
//!
//! dt convention: JS8Call's `xdt − ASTART` (0.5/0.5/0.2/0.1 s), equal to the
//! WSJT-X "t − 0.5" convention at Normal/Slow. Parity test: stock `js8`
//! output within its print resolution (tests/decode_parity.rs).

use rustfft::{num_complex::Complex, FftPlanner};

use super::downsample::{downsample, Spectrum};
use super::frame::Word87;
use super::ldpc::decode174;
use super::params::{geom, Geom, ND, NFSRCH, NN, NP2_SYMBOL_BOUND, NROWS};
use super::speed::Speed;
use super::sync::Candidate;

/// BP iteration cap (JS8.cpp:659 BP_MAX_ITERATIONS).
const MAX_ITER: u32 = 30;
/// LLR scale after normalisation (JS8.cpp:1352).
const SCALEFAC: f32 = 2.83;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Demod {
    pub freq_hz: f32,
    /// Reported dt: absolute start − ASTART.
    pub dt_s: f32,
    /// Absolute start of symbol 0 in the window (xdt2) — what subtraction uses.
    pub start_s: f32,
    pub word: Word87,
    pub nharderrors: u8,
    pub snr_db: i32,
    pub tones: [u8; 79],
}

/// Complex Costas references (JS8.cpp:2110-2130): for block b, position n,
/// sample j: exp(j·phi) with phi advancing by 2π·tone/NDOWNSPS, fmod TAU.
/// Layout: `[(b*7 + n)*nds + j]`.
fn costas_refs(costas: &[[u8; 7]; 3], nds: usize) -> Vec<Complex<f32>> {
    let mut out = Vec::with_capacity(21 * nds);
    for row in costas {
        for &tone in row {
            let dphi = TAU * tone as f32 / nds as f32;
            let mut phi = 0f32;
            for _ in 0..nds {
                out.push(Complex::from_polar(1.0, phi));
                phi = (phi + dphi) % TAU;
            }
        }
    }
    out
}

/// syncjs8d (JS8.cpp:1906-1970): Costas sync power at downsampled start `i0`
/// with an optional frequency tweak `delf` (Hz). Symbols that do not end
/// inside `np2` contribute nothing.
fn syncjs8d(cd0: &[Complex<f32>], g: &Geom, csyncs: &[Complex<f32>], i0: isize, delf: f32) -> f32 {
    let nds = g.ndownsps;
    let mut adj = vec![Complex::new(1.0f32, 0.0); nds];
    if delf != 0.0 {
        let dphi = TAU * delf / g.fs2; // BASE_DPHI · delf, :1907
        let mut phi = 0f32;
        for a in adj.iter_mut() {
            *a = Complex::from_polar(1.0, phi);
            phi = (phi + dphi) % TAU;
            if phi < 0.0 {
                phi += TAU; // fmod of a negative step, kept in [0, TAU) (:1928-1935)
            }
        }
    }
    let mut sync = 0f32;
    for b in 0..3usize {
        for n in 0..7usize {
            let offset = (36 * b * nds) as isize + i0 + (n * nds) as isize;
            if offset < 0 || offset as usize + nds > g.np2 || offset as usize + nds > cd0.len() {
                continue;
            }
            let off = offset as usize;
            let r = &csyncs[(b * 7 + n) * nds..(b * 7 + n + 1) * nds];
            let mut z = Complex::new(0.0f32, 0.0);
            for k in 0..nds {
                z += cd0[off + k] * (adj[k] * r[k]).conj(); // :1960-1962
            }
            sync += z.norm_sqr();
        }
    }
    sync
}

/// normalizeLLR (JS8.cpp:1336-1352): divide by the standard deviation (RMS
/// when the variance is not positive), then × 2.83.
fn normalize_llr(llr: &mut [f32; 174]) {
    let n = llr.len() as f32;
    let av = llr.iter().sum::<f32>() / n;
    let av2 = llr.iter().map(|v| v * v).sum::<f32>() / n;
    let var = av2 - av * av;
    let sig = if var > 0.0 { var.sqrt() } else { av2.sqrt() };
    for v in llr.iter_mut() {
        *v = (*v / sig) * SCALEFAC;
    }
}

/// Max of four rows of a symbol's tone magnitudes.
fn max4(ps: &[f32; NROWS], idx: [usize; 4]) -> f32 {
    idx.iter().map(|&i| ps[i]).fold(f32::MIN, f32::max)
}

pub(crate) fn demod_candidate(
    sp: &Spectrum,
    baseline_db: &[f32],
    speed: Speed,
    cand: &Candidate,
    planner: &mut FftPlanner<f32>,
) -> Option<Demod> {
    let g = geom(speed);
    let nds = g.ndownsps;
    let costas = speed.costas();
    let csyncs = costas_refs(costas, nds);
    let mut f1 = cand.freq_hz;

    // Noise baseline at the candidate's SYNC-spectrum bin (JS8.cpp:1158-1160).
    let index = ((f1 / g.df).round().max(0.0) as usize).min(baseline_db.len() - 1);
    let xbase = 10f32.powf(0.1 * (baseline_db[index] - g.basesub));

    // Downsample around f1 (:1167) and search ±¼ symbol for the start (:1171-1188).
    let mut cd0 = downsample(sp, speed, f1, planner);
    let i0 = ((cand.dt_s + g.astart) * g.fs2).round() as isize;
    let mut smax = 0f32;
    let mut ibest = 0isize;
    for idt in (i0 - g.nqsymbol)..=(i0 + g.nqsymbol) {
        let s = syncjs8d(&cd0, &g, &csyncs, idt, 0.0);
        if s > smax {
            smax = s;
            ibest = idt;
        }
    }
    let xdt2 = ibest as f32 * g.dt2; // :1190
    let i0 = (xdt2 * g.fs2).round() as isize; // :1194 (== ibest)

    // Fine frequency: ±NFSRCH steps of 0.5 Hz (:1197-1208).
    let mut smax = 0f32;
    let mut delfbest = 0f32;
    for ifr in -NFSRCH..=NFSRCH {
        let delf = ifr as f32 * 0.5;
        let s = syncjs8d(&cd0, &g, &csyncs, i0, delf);
        if s > smax {
            smax = s;
            delfbest = delf;
        }
    }
    // Apply the tweak to the baseband in place (:1212-1220): sample i gets
    // phase −(i+1)·dphi — the cumulative multiply happens BEFORE the use.
    let wstep = Complex::from_polar(1.0f32, -delfbest * TAU / g.fs2);
    let mut w = Complex::new(1.0f32, 0.0);
    for c in cd0.iter_mut().take(NP2_SYMBOL_BOUND) {
        w *= wstep;
        *c *= w;
    }
    f1 += delfbest; // :1225
    let sync = syncjs8d(&cd0, &g, &csyncs, i0, 0.0); // :1227 — the RAW power the acceptance rule reads

    // Per-symbol spectra (:1231-1253): s2[row][symbol] = |DFT|/1000 over 8 tone bins.
    let fft = planner.plan_fft_forward(nds);
    let mut s2 = [[0f32; NN]; NROWS];
    let mut csymb = vec![Complex::new(0.0f32, 0.0); nds];
    for k in 0..NN {
        let i1 = ibest + (k * nds) as isize;
        for c in csymb.iter_mut() {
            *c = Complex::new(0.0, 0.0);
        }
        if i1 >= 0 && i1 as usize + nds <= NP2_SYMBOL_BOUND {
            // cd0 is zero past NDFFT2 in the reference (its array runs to NP2 =
            // 3200 and is zero-filled beyond the ndfft2 valid samples). OUR cd0
            // is exactly ndfft2 long, so a symbol that starts at or past its end
            // stays all-zero (JS8Call reads those zeros) and a straddling symbol
            // copies only the samples that exist — a start >= len would make the
            // `cd0[start..start+avail]` slice illegal even when avail == 0.
            let start = i1 as usize;
            if start < cd0.len() {
                let avail = (cd0.len() - start).min(nds);
                csymb[..avail].copy_from_slice(&cd0[start..start + avail]);
            }
        }
        fft.process(&mut csymb);
        for (r, row) in s2.iter_mut().enumerate() {
            row[k] = csymb[r].norm() / 1000.0;
        }
    }

    // Hard sync count against the speed's Costas rows (:1257-1288); first max wins.
    let mut nsync = 0usize;
    for (b, row) in costas.iter().enumerate() {
        for (col, &tone) in row.iter().enumerate() {
            let idx = b * 36 + col;
            let mut max_row = 0usize;
            for r in 1..NROWS {
                if s2[r][idx] > s2[max_row][idx] {
                    max_row = r;
                }
            }
            if tone as usize == max_row {
                nsync += 1;
            }
        }
    }
    if nsync <= 6 {
        return None; // :1288
    }

    // Bit metrics on the 58 data symbols (:1298-1333), straight binary.
    let mut llr0 = [0f32; 174];
    let mut llr1 = [0f32; 174];
    for j in 0..ND {
        let k = if j < 29 { 7 + j } else { 43 + (j - 29) };
        let mut ps = [0f32; NROWS];
        for (r, p) in ps.iter_mut().enumerate() {
            *p = s2[r][k];
        }
        llr0[3 * j] = max4(&ps, [4, 5, 6, 7]) - max4(&ps, [0, 1, 2, 3]);
        llr0[3 * j + 1] = max4(&ps, [2, 3, 6, 7]) - max4(&ps, [0, 1, 4, 5]);
        llr0[3 * j + 2] = max4(&ps, [1, 3, 5, 7]) - max4(&ps, [0, 2, 4, 6]);
        for p in ps.iter_mut() {
            *p = (*p + 1e-32).ln(); // :1328
        }
        llr1[3 * j] = max4(&ps, [4, 5, 6, 7]) - max4(&ps, [0, 1, 2, 3]);
        llr1[3 * j + 1] = max4(&ps, [2, 3, 6, 7]) - max4(&ps, [0, 1, 4, 5]);
        llr1[3 * j + 2] = max4(&ps, [1, 3, 5, 7]) - max4(&ps, [0, 2, 4, 6]);
    }
    normalize_llr(&mut llr0);
    normalize_llr(&mut llr1);
    if !llr0.iter().chain(llr1.iter()).all(|v| v.is_finite()) {
        return None; // all-silent symbols: no metric, no decode (never NaN into BP)
    }

    // The four inner passes (:1364-1390). Pass 3 erases llr0[0..24], pass 4
    // ADDS llr0[24..48] — cumulative, exactly as the C++ mutates llr0.
    for ipass in 1..=4u8 {
        if ipass == 3 {
            for v in &mut llr0[0..24] {
                *v = 0.0;
            }
        } else if ipass == 4 {
            for v in &mut llr0[24..48] {
                *v = 0.0;
            }
        }
        let llr = if ipass == 2 { &llr1 } else { &llr0 };
        let Some(d) = decode174(llr, MAX_ITER) else {
            continue; // bpdecode174 returned −1
        };
        if d.cw.iter().all(|&b| b == 0) {
            continue; // :1382-1385
        }
        let nh = d.nharderrors as i32;
        let accept = nh < 60
            && !(sync < 2.0 && nh > 35)
            && !(ipass > 2 && nh > 39)
            && !(ipass == 4 && nh > 30);
        if !accept {
            continue;
        }
        let word = Word87::from_bits(&d.msg87);
        if !word.verify() {
            continue; // :1392 checkCRC12
        }
        let tones = super::encode_word(&word, speed); // :1402-1408 JS8::encode(i3bit, Costas, message)

        // Signal power from the decoded tones over the fitted baseline (:1418-1431).
        let mut xsig = 0f32;
        for (i, &t) in tones.iter().enumerate() {
            xsig += s2[t as usize][i].powi(2);
        }
        let xsnr = (10.0 * (xsig / xbase - 1.0).max(1.259e-10).log10() - 32.0).max(-60.0);
        return Some(Demod {
            freq_hz: f1,
            dt_s: xdt2 - g.astart,
            start_s: xdt2,
            word,
            nharderrors: d.nharderrors,
            snr_db: xsnr.round() as i32,
            tones,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::downsample::spectrum;
    use crate::phy::params::geom;
    use crate::phy::sync::{find_candidates, Candidate};
    use crate::phy::testutil::{synth_window, word};
    use rustfft::FftPlanner;

    /// Hand the demodulator the coarse candidate a clean −5 dB frame produces
    /// (from the real candidate search, so the coarse dt has JS8Call's shape):
    /// the CRC-verified word comes back, the refined frequency is within
    /// 0.5 Hz and dt within two coarse steps, at every speed. This is the seam
    /// test between B1's codec (tones/LDPC/CRC) and B3's demod.
    #[test]
    fn clean_frame_demodulates_to_its_word_at_every_speed() {
        for speed in Speed::ALL {
            let g = geom(speed);
            let w = word("KD9TAWEN52ab");
            let iw = synth_window(&[(w, 1500.0, -5.0)], speed, 3);
            let dd: Vec<f32> = iw.iter().map(|&s| s as f32).collect();
            let mut planner = FftPlanner::<f32>::new();
            let sr = find_candidates(&dd, speed, 100.0, 4000.0, &mut planner);
            let cand = sr.candidates[0];
            let sp = spectrum(&dd, speed, &mut planner);
            let d = demod_candidate(&sp, &sr.baseline_db, speed, &cand, &mut planner)
                .unwrap_or_else(|| panic!("{speed:?}: no decode from {cand:?}"));
            assert_eq!(d.word, w, "{speed:?}: wrong word");
            assert!(
                (d.freq_hz - 1500.0).abs() <= 0.5,
                "{speed:?}: freq {}",
                d.freq_hz
            );
            assert!(
                d.dt_s.abs() <= 2.0 * g.tstep + 1e-6,
                "{speed:?}: dt {}",
                d.dt_s
            );
            assert!((d.start_s - d.dt_s - g.astart).abs() < 1e-6);
            assert_eq!(d.tones, crate::phy::encode_word(&w, speed));
            // Gross-scale sanity only: a −5 dB (2500 Hz convention) frame must not
            // report +40 (baseline read as 0) or −60 (the floor). JS8Call's absolute
            // SNR scale vs the WSJT-X convention is measured, not assumed — the lab
            // (Task B3.11 ab_js8.py) compares stock vs Nexus SNR per file and fails
            // on a mean offset > 3 dB; `snr_estimate_tracks_level` pins the slope.
            //
            // The reported SNR falls monotonically with speed for the SAME injected
            // level — Slow ≈ −11, Normal ≈ −13, Fast ≈ −16, Turbo ≈ −20 for this −5 dB
            // frame — because JS8Call's per-symbol DFT magnitude scales as
            // NDOWNSPS·√NDOWN (a ~4× spread across the four modes) and BASESUB
            // (42/40/39/38) only partly compensates. This faithfully reproduces
            // JS8Call's formula (BASESUB per Mode, the constant −32 offset, xsig/xbase);
            // whether the ABSOLUTE offset matches stock on the same audio is B3.11's
            // gate, not this one. The bound below only rejects the two pathologies its
            // comment names, across the observed per-speed band.
            assert!((-26..=6).contains(&d.snr_db), "{speed:?}: snr {}", d.snr_db);
        }
    }

    /// A candidate pointing at empty spectrum (no Costas there) bails on the
    /// hard-sync count — never reaches BP, never fabricates a word.
    #[test]
    fn candidate_on_noise_returns_none() {
        let speed = Speed::Normal;
        let iw = synth_window(&[], speed, 5);
        let dd: Vec<f32> = iw.iter().map(|&s| s as f32).collect();
        let mut planner = FftPlanner::<f32>::new();
        let sr = find_candidates(&dd, speed, 100.0, 4000.0, &mut planner);
        let sp = spectrum(&dd, speed, &mut planner);
        let cand = Candidate {
            freq_hz: 1500.0,
            dt_s: 0.0,
            sync: 1.6,
        };
        assert!(demod_candidate(&sp, &sr.baseline_db, speed, &cand, &mut planner).is_none());
    }

    /// The SNR estimate tracks the injected level: two frames 10 dB apart
    /// report SNRs 10 ± 3 dB apart (the baseline is common to both).
    #[test]
    fn snr_estimate_tracks_level() {
        let speed = Speed::Normal;
        let w = word("KD9TAWEN52ab");
        let mut planner = FftPlanner::<f32>::new();
        let mut snr_at = |level: f32| -> i32 {
            let iw = synth_window(&[(w, 1500.0, level)], speed, 21);
            let dd: Vec<f32> = iw.iter().map(|&s| s as f32).collect();
            let sr = find_candidates(&dd, speed, 100.0, 4000.0, &mut planner);
            let sp = spectrum(&dd, speed, &mut planner);
            demod_candidate(&sp, &sr.baseline_db, speed, &sr.candidates[0], &mut planner)
                .expect("decode")
                .snr_db
        };
        let hi = snr_at(0.0);
        let lo = snr_at(-10.0);
        assert!(
            (7..=13).contains(&(hi - lo)),
            "SNR delta {} (hi {hi}, lo {lo})",
            hi - lo
        );
    }
}
