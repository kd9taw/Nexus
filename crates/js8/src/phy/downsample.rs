//! Band-shift downsample — JS8Call `computeBasebandFFT` + `js8_downsample`
//! (JS8.cpp:1566-1647 @ a7ff1be0) read as the SPEC with runtime geometry;
//! lineage WSJT-X `ft8_downsample.f90`.
//!
//! CONTRACT: `spectrum` is the ONE long FFT of the whole period (NDFFT1 =
//! NSPS·NDD points, zero-padded past the period — JS8.cpp:1577-1580). The
//! caller owns the `Spectrum` value instead of a module SAVE, so the
//! per-radio-context bug that forced the Fortran hoist at
//! ft8_downsample.f90:1-9 cannot exist. `downsample` slices the 10-baud band
//! around `f0` (−1.5 … +8.5 baud), tapers NDD+1 bins at each edge with
//! JS8Call's head/tail tapers, rotates the bin of `f0` to index 0 and
//! inverse-FFTs to NDFFT2 complex samples at FS2 = NDOWNSPS × baud, scaled
//! by 1/sqrt(NDFFT1·NDFFT2) — so tone k of a signal at `f0` is a complex
//! exponential at +k·baud and an NDOWNSPS-point DFT per symbol puts it in
//! bin k, at the amplitude JS8Call's SNR estimate expects.
//!
//! Geometry per speed (params.rs): NDFFT1 = NSPS·NDD (360 960 / 192 000 /
//! 120 000 / 72 000 — each ≥ its period), NDOWN = NSPS/NDOWNSPS, NDFFT2 =
//! NDD·NDOWNSPS (3008 / 3200 / 2000 / 1440). The band is 10·baud/DF = 10·NDD
//! bins wide at every speed, so a taper of NDD+1 bins per edge is the same
//! fraction everywhere (JS8.cpp:1098-1110 builds both tapers per Mode).

use rustfft::{num_complex::Complex, FftPlanner};

use super::params::geom;
use super::speed::Speed;

/// The period's spectrum: bins 0..=NDFFT1/2 (JS8.cpp `ds_cx`), df = 12000/NDFFT1.
pub(crate) struct Spectrum {
    pub bins: Vec<Complex<f32>>,
    pub ndfft1: usize,
    pub df: f32,
}

/// One long FFT of the whole period (JS8.cpp:1566-1581). `dd` may be shorter
/// than NDFFT1 (zero-padded); the decoder never hands more than one period.
pub(crate) fn spectrum(dd: &[f32], speed: Speed, planner: &mut FftPlanner<f32>) -> Spectrum {
    let g = geom(speed);
    let mut buf = vec![Complex::new(0.0f32, 0.0); g.ndfft1];
    for (b, &x) in buf.iter_mut().zip(dd.iter().take(g.ndfft1)) {
        b.re = x;
    }
    planner.plan_fft_forward(g.ndfft1).process(&mut buf);
    buf.truncate(g.ndfft1 / 2 + 1);
    Spectrum { bins: buf, ndfft1: g.ndfft1, df: 12_000.0 / g.ndfft1 as f32 }
}

/// Mix `f0` to baseband and decimate (JS8.cpp:1589-1647).
pub(crate) fn downsample(
    sp: &Spectrum,
    speed: Speed,
    f0_hz: f32,
    planner: &mut FftPlanner<f32>,
) -> Vec<Complex<f32>> {
    let g = geom(speed);
    let baud = 12_000.0 / g.nsps as f32;
    let top = (g.ndfft1 / 2) as f32;
    let i0 = (f0_hz / sp.df).round() as isize; // :1602
    let it = ((f0_hz + 8.5 * baud) / sp.df).round().min(top) as usize; // :1603
    let ib = ((f0_hz - 1.5 * baud) / sp.df).round().max(0.0) as usize; // :1604
    let mut cd0 = vec![Complex::new(0.0f32, 0.0); g.ndfft2];
    if it < ib || it - ib + 1 > g.ndfft2 || i0 < ib as isize {
        return cd0; // f0 outside the spectrum: an all-zero baseband, never a panic
    }
    let range = it - ib + 1;
    cd0[..range].copy_from_slice(&sp.bins[ib..=it]); // :1611-1613
    // Head taper (reversed) over cd0[0..=NDD], tail taper over the last NDD+1
    // of the range (:1100-1110, :1622-1623): taper(k) = 0.5·(1 + cos(kπ/NDD)).
    let ndd = g.ndd;
    if range > ndd {
        for k in 0..=ndd {
            let t = 0.5 * (1.0 + (k as f32 * std::f32::consts::PI / ndd as f32).cos());
            cd0[ndd - k] *= t;
            cd0[range - 1 - ndd + k] *= t;
        }
    }
    // std::rotate(begin, begin + (i0 − ib), begin + NDFFT2) — a left rotation
    // that puts the bin of f0 at index 0 (:1629).
    cd0.rotate_left(((i0 - ib as isize) as usize) % g.ndfft2);
    planner.plan_fft_inverse(g.ndfft2).process(&mut cd0); // :1635 (unnormalised, as FFTW)
    let fac = 1.0 / ((g.ndfft1 as f32) * (g.ndfft2 as f32)).sqrt(); // :1641
    for c in &mut cd0 {
        *c *= fac;
    }
    cd0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::params::geom;
    use rustfft::FftPlanner;

    /// A real tone at f0 + 3·baud, band-shifted to f0, is a complex exponential
    /// at +3·baud: an NDOWNSPS-point DFT of one symbol peaks in bin 3. Checked
    /// at every speed (each has its own NDFFT1/NDOWN/NDFFT2 geometry).
    #[test]
    fn tone_at_three_baud_lands_in_bin_three() {
        for speed in Speed::ALL {
            let g = geom(speed);
            let baud = 12_000.0 / g.nsps as f32;
            let f0 = 1500.0f32;
            let f = f0 + 3.0 * baud;
            let dd: Vec<f32> = (0..g.nmax)
                .map(|i| 1000.0 * (std::f32::consts::TAU * f * i as f32 / 12_000.0).cos())
                .collect();
            let mut planner = FftPlanner::<f32>::new();
            let sp = spectrum(&dd, speed, &mut planner);
            assert_eq!(sp.ndfft1, g.ndfft1);
            assert_eq!(sp.bins.len(), g.ndfft1 / 2 + 1);
            let cd = downsample(&sp, speed, f0, &mut planner);
            assert_eq!(cd.len(), g.ndfft2);
            let nds = g.ndownsps;
            // Symbol 10 (well inside the window): DFT, argmax over the 8 tone bins.
            let mut sym: Vec<Complex<f32>> = cd[10 * nds..11 * nds].to_vec();
            planner.plan_fft_forward(nds).process(&mut sym);
            let (peak, _) = sym[..8]
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.norm()))
                .fold((0, -1.0f32), |a, b| if b.1 > a.1 { b } else { a });
            assert_eq!(peak, 3, "{speed:?}: peak bin {peak}");
            let e: f32 = cd.iter().map(|c| c.norm_sqr()).sum();
            assert!(e.is_finite() && e > 0.0, "{speed:?}: energy {e}");
        }
    }

    /// JS8.cpp's scaling: an input tone of amplitude A at 12 kHz comes out of
    /// the downsampler with |cd| ≈ A/2·sqrt(NDFFT1/NDFFT2)·(1/sqrt(NDFFT1·NDFFT2))·NDFFT1…
    /// — rather than derive it, pin the ratio between two speeds so a change
    /// in the 1/sqrt(NDFFT1·NDFFT2) factor (JS8.cpp:1641) is caught.
    #[test]
    fn scaling_follows_the_reference_factor() {
        let mut planner = FftPlanner::<f32>::new();
        let mut mean_mag = |speed: Speed| -> f32 {
            let g = geom(speed);
            let dd: Vec<f32> = (0..g.nmax)
                .map(|i| 1000.0 * (std::f32::consts::TAU * 1500.0 * i as f32 / 12_000.0).cos())
                .collect();
            let sp = spectrum(&dd, speed, &mut planner);
            let cd = downsample(&sp, speed, 1500.0, &mut planner);
            let lo = g.ndfft2 / 4;
            let hi = g.ndfft2 / 2;
            cd[lo..hi].iter().map(|c| c.norm()).sum::<f32>() / (hi - lo) as f32
        };
        // A DC-shifted tone of amplitude A yields |cd| = A/2 · NDFFT1 / sqrt(NDFFT1·NDFFT2)
        // = A/2 · sqrt(NDFFT1/NDFFT2) = A/2 · sqrt(NDOWN).
        for speed in Speed::ALL {
            let g = geom(speed);
            let expect = 500.0 * (g.ndown as f32).sqrt();
            let got = mean_mag(speed);
            // 5 %: the period is shorter than NDFFT1 at Slow/Normal, so the tone is
            // rectangular-windowed and the band-limited reconstruction ripples slightly.
            assert!((got - expect).abs() / expect < 0.05, "{speed:?}: |cd| {got} vs {expect}");
        }
    }
}
