//! Decoded-signal subtraction — JS8Call `genjs8refsig` + `subtractjs8` and
//! the filter its constructor builds (JS8.cpp:1977-2063 and :2132-2196 @
//! a7ff1be0) read as the SPEC with runtime NSPS; lineage WSJT-X
//! `subtractft8.f90` (libtempo/vendor/wsjtx/lib/ft8/subtractft8.f90:3-8 for
//! the identity below).
//!
//! CONTRACT: measured dd(t) = a(t)cos(2πf0t+θ(t)); reference cref(t) =
//! exp(j(2πf0t+φ(t))); complex amplitude cfilt = LPF[dd·conj(cref)];
//! dd ← dd − 2·Re{cref·cfilt}. The LPF is a cos² window of NFILT+1 taps
//! applied by an NMAX-point circular convolution. The reference is the
//! complex twin of B1's `modulate` — rectangular phase-continuous CPFSK
//! (JS8Call Modulator.cpp, NOT gen_ft8wave's GFSK, which subtractft8.f90:45
//! uses for FT8 and would leave a shaped residual here). A constant
//! phase/amplitude offset between `dd` and the reference is absorbed by
//! cfilt, so exact phase alignment with `modulate` is not required — only
//! the per-sample frequency law must match.
//!
//! Two JS8Call facts reproduced deliberately, because the stock-decode
//! reproduction test depends on pass-2 seeing the same residual JS8Call's
//! pass 2 sees: (1) the filter's negative-lag half is NOT wrapped to the end
//! of the NMAX buffer — `std::rotate` at JS8.cpp:2164-2166 runs over the
//! first NFILT+1 elements only, so taps j = −700..−1 sit at indices
//! 701..1400 (a 1401-sample delay of half the window; subtractft8.f90:35
//! cshifts the whole buffer instead); (2) there is no end correction
//! (subtractft8.f90:39-41, :81-82 has one). A future change to either is an
//! A/B on the lab corpus (Task B3.11), not a drive-by fix.
//!
//! Runs in the decoder's outer passes 1–2 (JS8Call: 3 outer passes,
//! subtract on 1–2) so later passes find weaker stations under strong ones.

use rustfft::{num_complex::Complex, FftPlanner};

use super::params::NFILT;
use super::speed::Speed;

const TAU: f32 = std::f32::consts::TAU;

/// 79·NSPS-sample complex reference (JS8.cpp:1977-2007): exp(jφ) with φ
/// advancing by 2π(f0/12000 + tone/NSPS) per sample, fmod TAU, phase-
/// continuous across symbols; f32 throughout like the reference.
pub(crate) fn reference_wave(tones: &[u8; 79], speed: Speed, f0_hz: f32) -> Vec<Complex<f32>> {
    let nsps = speed.nsps();
    let bfpi = TAU * f0_hz / 12_000.0;
    let mut phi = 0f32;
    let mut cref = Vec::with_capacity(79 * nsps);
    for &t in tones {
        let dphi = bfpi + TAU * t as f32 / nsps as f32;
        for _ in 0..nsps {
            cref.push(Complex::from_polar(1.0, phi));
            phi = (phi + dphi) % TAU;
        }
    }
    cref
}

/// The frequency-domain LPF (JS8.cpp:2132-2196): cos²(πj/NFILT) for
/// j = −NFILT/2..=NFILT/2 into indices 0..=NFILT, normalised by its sum,
/// rotated left by NFILT/2 WITHIN those NFILT+1 elements, zero elsewhere,
/// NMAX-point FFT, × 1/NMAX.
pub(crate) fn subtract_filter(nmax: usize, planner: &mut FftPlanner<f32>) -> Vec<Complex<f32>> {
    let half = NFILT / 2;
    let mut w = vec![0f32; NFILT + 1];
    let mut sum = 0f32;
    for (idx, v) in w.iter_mut().enumerate() {
        let j = idx as f32 - half as f32;
        *v = (std::f32::consts::PI * j / NFILT as f32).cos().powi(2);
        sum += *v;
    }
    let mut filter = vec![Complex::new(0.0f32, 0.0); nmax];
    for (idx, &v) in w.iter().enumerate() {
        filter[idx] = Complex::new(v / sum, 0.0);
    }
    filter[..NFILT + 1].rotate_left(half); // :2164-2166 — NOT a whole-buffer cshift
    planner.plan_fft_forward(nmax).process(&mut filter);
    let fac = 1.0 / nmax as f32;
    for c in &mut filter {
        *c *= fac;
    }
    filter
}

/// Subtract one decoded frame in place (JS8.cpp:2022-2063). `start_s` may be
/// negative; the overlap of the reference with `dd` is what gets corrected.
pub(crate) fn subtract(
    dd: &mut [f32],
    cref: &[Complex<f32>],
    start_s: f32,
    filter: &[Complex<f32>],
    planner: &mut FftPlanner<f32>,
) {
    let nmax = dd.len();
    debug_assert_eq!(filter.len(), nmax);
    let nstart = (start_s * 12_000.0) as isize; // static_cast<int>: truncation toward zero
    let cref_start = if nstart < 0 { (-nstart) as usize } else { 0 };
    let dd_start = if nstart > 0 { nstart as usize } else { 0 };
    if cref_start >= cref.len() || dd_start >= nmax {
        return;
    }
    let size = (cref.len() - cref_start).min(nmax - dd_start);
    let mut cfilt = vec![Complex::new(0.0f32, 0.0); nmax];
    for i in 0..size {
        cfilt[i] = dd[dd_start + i] * cref[cref_start + i].conj(); // :2032-2035
    }
    let fwd = planner.plan_fft_forward(nmax);
    let inv = planner.plan_fft_inverse(nmax);
    fwd.process(&mut cfilt);
    for (c, f) in cfilt.iter_mut().zip(filter.iter()) {
        *c *= *f; // :2048-2052
    }
    inv.process(&mut cfilt); // unnormalised; the filter carries 1/NMAX
    for i in 0..size {
        dd[dd_start + i] -= 2.0 * (cfilt[i] * cref[cref_start + i]).re; // :2059-2062
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::params::geom;
    use crate::phy::testutil::word;
    use crate::phy::{encode_word, modulate};
    use rustfft::FftPlanner;

    /// Subtracting a clean frame with its own tones/f0/start removes it:
    /// residual energy ≤ 5 % (−13 dB) of the original at every speed. Not
    /// zero, by construction: JS8Call's filter (see the header) leaves the
    /// first NFILT+1 samples of the frame half-corrected, and that residual
    /// is what pass-2 decodes in decode_parity honestly have to live with.
    /// The measured ratio is printed so a regression in the filter build
    /// shows as a number, not just a red test.
    #[test]
    fn exact_subtraction_removes_the_frame() {
        for speed in Speed::ALL {
            let g = geom(speed);
            let w = word("N0CALLAA00zz");
            let tones = encode_word(&w, speed);
            let wave = modulate(&tones, speed, 1234.5, 12_000.0);
            let mut dd = vec![0f32; g.nmax];
            for (i, &x) in wave.iter().enumerate() {
                if i < g.nmax {
                    dd[i] = 1000.0 * x;
                }
            }
            let before: f32 = dd.iter().map(|x| x * x).sum();
            let mut planner = FftPlanner::<f32>::new();
            let filter = subtract_filter(g.nmax, &mut planner);
            let cref = reference_wave(&tones, speed, 1234.5);
            assert_eq!(cref.len(), 79 * g.nsps);
            subtract(&mut dd, &cref, g.astart, &filter, &mut planner);
            let after: f32 = dd.iter().map(|x| x * x).sum();
            eprintln!("{speed:?}: residual {:.4}", after / before);
            assert!(after / before < 0.05, "{speed:?}: residual {:.4}", after / before);
        }
    }

    /// Subtracting the WRONG tones leaves most of the energy (positive control
    /// for the test above: the subtraction is signal-specific, not a notch).
    #[test]
    fn wrong_tones_do_not_remove_the_frame() {
        let speed = Speed::Normal;
        let g = geom(speed);
        let w = word("N0CALLAA00zz");
        let other = word("W9XYZEN37abc");
        let wave = modulate(&encode_word(&w, speed), speed, 1234.5, 12_000.0);
        let mut dd = vec![0f32; g.nmax];
        for (i, &x) in wave.iter().enumerate() {
            dd[i] = 1000.0 * x;
        }
        let before: f32 = dd.iter().map(|x| x * x).sum();
        let mut planner = FftPlanner::<f32>::new();
        let filter = subtract_filter(g.nmax, &mut planner);
        let cref = reference_wave(&encode_word(&other, speed), speed, 1234.5);
        subtract(&mut dd, &cref, g.astart, &filter, &mut planner);
        let after: f32 = dd.iter().map(|x| x * x).sum();
        assert!(after / before > 0.3, "residual {:.3}", after / before);
    }

    /// A negative start (the frame began before the window) neither panics
    /// nor touches samples outside the overlap.
    #[test]
    fn negative_start_is_clipped_not_panicked() {
        let speed = Speed::Turbo;
        let g = geom(speed);
        let tones = encode_word(&word("N0CALLAA00zz"), speed);
        let mut dd = vec![0f32; g.nmax];
        let mut planner = FftPlanner::<f32>::new();
        let filter = subtract_filter(g.nmax, &mut planner);
        let cref = reference_wave(&tones, speed, 800.0);
        subtract(&mut dd, &cref, -1.0, &filter, &mut planner);
        assert!(dd.iter().all(|x| x.is_finite()));
    }
}
