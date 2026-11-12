//! The FT8 and FT4 waveforms: `gen_ft8wave.f90` and `gen_ft4wave.f90` (`icmplx = 0`, the real
//! audio), in single precision with the Fortran's own order of operations.
//!
//! The pulse is `gfsk_pulse.f90`'s, through the platform's `erff`, the same function gfortran's
//! `ERF` calls there; the phase is wrapped with an exact remainder, as gfortran's `MOD` is (`fprem`
//! on x86-64). The one place gfortran may round differently is the cosine of the two amplitude
//! ramps, which it vectorises through glibc's vector maths library: a few units in the last place
//! over the first and last ramp of samples, nowhere else.
//!
//! ORIGIN AND LICENCE. Translated into Rust by KD9TAW, 2026-10-14, from `lib/ft8/gen_ft8wave.f90`,
//! `lib/ft4/gen_ft4wave.f90` and `lib/ft2/gfsk_pulse.f90` of WSJT-X, © Joe Taylor (K1JT) and the
//! WSJT Development Group, GNU General Public License, version 3 (the copies vendored at
//! `libtempo/vendor/wsjtx/`). Distributed under the same licence, as part of Nexus (GPL-3.0-only).
//! See NOTICE, "WSJT-X".

use std::f32::consts::{LN_2, PI, TAU};

extern "C" {
    /// C99 `erff`, from the platform maths library every Nexus build already links (the vendored
    /// `gfsk_pulse.f90` calls it for gfortran's `ERF`).
    fn erff(x: f32) -> f32;
}

fn erf(x: f32) -> f32 {
    // SAFETY: a pure function of one float, with no state and no pointer.
    unsafe { erff(x) }
}

/// `gfsk_pulse(b, t)`: the Gaussian frequency pulse.
fn gfsk_pulse(b: f32, t: f32) -> f32 {
    // `pi*sqrt(2.0/log(2.0))`, folded by gfortran in default real: 0x40AAC42B.
    let c = PI * (2.0 / LN_2).sqrt();
    let cb = c * b;
    0.5 * (erf((t + 0.5) * cb) - erf((t - 0.5) * cb))
}

/// The pulse table both routines build: `pulse(i)=gfsk_pulse(bt,(i-1.5*nsps)/real(nsps))`.
fn pulse(bt: f32, nsps: usize) -> Vec<f32> {
    (1..=3 * nsps)
        .map(|i| gfsk_pulse(bt, (i as f32 - 1.5 * nsps as f32) / nsps as f32))
        .collect()
}

/// The amplitude ramp's factor at `i` of `n` samples: `(1 -/+ cos(twopi*i/(2n)))`, before the
/// division by 2 the callers make after multiplying.
fn ramp(i: usize, n: usize, rising: bool) -> f32 {
    let c = (TAU * i as f32 / (2.0 * n as f32)).cos();
    if rising {
        1.0 - c
    } else {
        1.0 + c
    }
}

/// `ft8_gen_wave`: the bare FT8 audio for `itone` (`nsym * 1920` samples; 1920 samples a symbol
/// whatever `fsample` is, as `ft8_cabi.f90` passes it), Gaussian BT = 2.0.
pub fn ft8_wave(itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
    const NSPS: usize = 1920;
    let nsym = itone.len();
    if nsym == 0 {
        return Vec::new();
    }
    let nwave = nsym * NSPS;
    let pulse = pulse(2.0, NSPS);
    let dt = 1.0 / fsample;
    let dphi_peak = TAU * 1.0 / NSPS as f32;
    // The smoothed frequency, one dummy symbol each side.
    let mut dphi = vec![0f32; (nsym + 2) * NSPS];
    for (j, &tone) in itone.iter().enumerate() {
        let ib = j * NSPS;
        for (d, &p) in dphi[ib..ib + 3 * NSPS].iter_mut().zip(&pulse) {
            *d += dphi_peak * p * tone as f32;
        }
    }
    let first = dphi_peak * itone[0] as f32;
    for (d, &p) in dphi[..2 * NSPS].iter_mut().zip(&pulse[NSPS..]) {
        *d += first * p;
    }
    let last = dphi_peak * itone[nsym - 1] as f32;
    for (d, &p) in dphi[nsym * NSPS..].iter_mut().zip(&pulse[..2 * NSPS]) {
        *d += last * p;
    }
    let shift = TAU * f0 * dt;
    for d in &mut dphi {
        *d += shift;
    }
    let mut wave = vec![0f32; nwave];
    let mut phi = 0f32;
    for (k, s) in wave.iter_mut().enumerate() {
        *s = phi.sin();
        phi = (phi + dphi[NSPS + k]) % TAU;
    }
    let nramp = (NSPS as f32 / 8.0).round() as usize;
    for (i, s) in wave[..nramp].iter_mut().enumerate() {
        *s = *s * ramp(i, nramp, true) / 2.0;
    }
    let k1 = nsym * NSPS - nramp;
    for (i, s) in wave[k1..].iter_mut().enumerate() {
        *s = *s * ramp(i, nramp, false) / 2.0;
    }
    wave
}

/// FT4's samples a symbol (at any `fsample`, as `ft4_gen_wave` takes it).
pub(crate) const FT4_NSPS: usize = 576;

/// `ft4_gen_wave`: the full FT4 frame for `itone` (`72576` samples, the shaped signal from the
/// first, silence after it; 576 samples a symbol), Gaussian BT = 1.0.
pub fn ft4_wave(itone: &[i32], fsample: f32, f0: f32) -> Vec<f32> {
    const NSPS: usize = FT4_NSPS;
    const NMAX: usize = 21 * 3456;
    let nsym = itone.len();
    let pulse = pulse(1.0, NSPS);
    let dt = 1.0 / fsample;
    let dphi_peak = TAU * 1.0 / NSPS as f32;
    let nz = (nsym + 2) * NSPS;
    let mut dphi = vec![0f32; nz];
    for (j, &tone) in itone.iter().enumerate() {
        let ib = j * NSPS;
        for (d, &p) in dphi[ib..ib + 3 * NSPS].iter_mut().zip(&pulse) {
            *d += dphi_peak * p * tone as f32;
        }
    }
    let shift = TAU * f0 * dt;
    for d in &mut dphi {
        *d += shift;
    }
    let mut wave = vec![0f32; NMAX];
    let mut phi = 0f32;
    for (s, &d) in wave.iter_mut().zip(&dphi) {
        *s = phi.sin();
        phi = (phi + d) % TAU;
    }
    for (i, s) in wave[..NSPS.min(nz)].iter_mut().enumerate() {
        *s = *s * ramp(i, NSPS, true) / 2.0;
    }
    let k1 = (nsym + 1) * NSPS;
    for (i, s) in wave.iter_mut().skip(k1).take(NSPS).enumerate() {
        *s = *s * ramp(i, NSPS, false) / 2.0;
    }
    wave
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pulse constant is the one gfortran folds into `gfsk_pulse.f90.o` (`.rodata.cst4`).
    #[test]
    fn the_pulse_constant_is_gfortrans() {
        assert_eq!((PI * (2.0 / LN_2).sqrt()).to_bits(), 0x40AA_C42B);
        // `twopi=8.0*atan(1.0)` in default real is TAU.
        assert_eq!(8.0 * 1.0f32.atan(), TAU);
    }
}
