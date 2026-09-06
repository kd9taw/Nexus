//! Per-speed decoder geometry — JS8Call's four `Mode{A,B,C,E}` constant
//! blocks (JS8.cpp:211-338 @ a7ff1be0) as ONE runtime table, so every DSP
//! module below is written once and takes `geom(speed)` instead of being
//! instantiated per speed the way JS8.cpp's `DecodeMode<Mode>` template is.
//!
//! CONTRACT: every field is either a transcribed constant (NDD, BASESUB, the
//! AZ factor, JSTRT) or the same derivation JS8.cpp performs from NSPS
//! (NFFT1 = NSPS·NFOS, NSTEP = NSPS/NSSY, NHSYM = NMAX/NSTEP − 3, NDOWN =
//! NSPS/NDOWNSPS, NDFFT1 = NSPS·NDD, NDFFT2 = NDFFT1/NDOWN, NP2 = NN·NDOWNSPS,
//! FS2 = 12000/NDOWN, DF = 12000/NFFT1, TSTEP = NSTEP/12000). JSTRT is a
//! TABLE, not a division: JS8.cpp computes `static_cast<int>(ASTART/TSTEP)`
//! in float and the truncation of 6.25 / 12.5 / 8.0 / 8.0 is a fact we pin
//! rather than re-derive in a different arithmetic.
//!
//! WHY NDD/BASESUB/AZ live here and not in `Speed` (interfaces §1.1): they
//! are decoder-internal tuning constants no other crate needs; `Speed` stays
//! the on-air contract (NSPS, period, delay, Costas, JZ, NDOWNSPS).
//!
//! Namespace-level constants (JS8.cpp:173-190): ASYNCMIN 1.5, NFSRCH 5
//! (±2.5 Hz in 0.5 Hz steps), NMAXCAND 300, NFILT 1400, NROWS 8, NFOS 2,
//! NSSY 4, and the FT8-era `NP2 = 2812` that JS8.cpp:1216 and :1239 use
//! UNQUALIFIED (the symbol-extraction bound) while :1950 uses `Mode::NP2` —
//! two different bounds, both reproduced (`NP2_SYMBOL_BOUND` vs `Geom::np2`).

use super::speed::Speed;

/// Candidates below this normalised sync are dropped (JS8.cpp:180).
pub(crate) const ASYNCMIN: f32 = 1.5;
/// Fine frequency search half-range in 0.5 Hz steps (JS8.cpp:181).
pub(crate) const NFSRCH: i32 = 5;
/// Candidate list cap (JS8.cpp:182).
pub(crate) const NMAXCAND: usize = 300;
/// Subtraction LPF length (JS8.cpp:183).
pub(crate) const NFILT: usize = 1400;
/// Tone rows kept per symbol spectrum (JS8.cpp:184).
pub(crate) const NROWS: usize = 8;
/// Frequency-bin oversampling of the sync spectra (JS8.cpp:185).
pub(crate) const NFOS: usize = 2;
/// Quarter-symbol steps per symbol (JS8.cpp:186).
pub(crate) const NSSY: usize = 4;
/// The FT8-era NP2 used as the per-symbol extraction bound (JS8.cpp:188, :1216, :1239).
pub(crate) const NP2_SYMBOL_BOUND: usize = 2812;
/// Channel symbols per frame (JS8.cpp:179).
pub(crate) const NN: usize = 79;
/// Data symbols per frame (JS8.cpp:177).
pub(crate) const ND: usize = 58;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Geom {
    pub nsps: usize,
    /// Period in samples (NTXDUR × 12000).
    pub nmax: usize,
    /// Sync-spectrum FFT length (2·NSPS) and its bin count NH1.
    pub nfft1: usize,
    pub nh1: usize,
    /// Quarter-symbol step in samples, and the number of spectra rows.
    pub nstep: usize,
    pub nhsym: usize,
    pub ndownsps: usize,
    pub ndown: usize,
    /// Downsample geometry: NDD, NDFFT1 = NSPS·NDD, NDFFT2 = NDFFT1/NDOWN.
    pub ndd: usize,
    pub ndfft1: usize,
    pub ndfft2: usize,
    /// Frame length in downsampled samples (NN·NDOWNSPS) — the fine-sync bound.
    pub np2: usize,
    pub tstep: f32,
    pub df: f32,
    /// Near-duplicate candidate window in Hz.
    pub az: f32,
    /// Downsampled rate and its sample period.
    pub fs2: f32,
    pub dt2: f32,
    /// Start delay in seconds (JS8Call ASTART = Speed::delay_ms/1000).
    pub astart: f32,
    /// Baseline offset for the SNR estimate.
    pub basesub: f32,
    pub jstrt: isize,
    pub jz: isize,
    pub nqsymbol: isize,
}

/// JS8.cpp:211-338, one row per speed.
pub(crate) fn geom(speed: Speed) -> Geom {
    // (NDD, BASESUB, AZ factor, JSTRT) — transcribed per Mode block.
    let (ndd, basesub, az_fac, jstrt) = match speed {
        Speed::Slow => (94usize, 42.0f32, 0.64f32, 6isize), // ModeE, JS8.cpp:309-338
        Speed::Normal => (100, 40.0, 0.64, 12),             // ModeA, :211-240
        Speed::Fast => (100, 39.0, 0.8, 8),                 // ModeB, :242-271
        Speed::Turbo => (120, 38.0, 0.6, 8),                // ModeC, :273-302
    };
    let nsps = speed.nsps();
    let nmax = speed.period_s() as usize * 12_000;
    let nfft1 = nsps * NFOS;
    let nstep = nsps / NSSY;
    let ndownsps = speed.ndownsps();
    let ndown = nsps / ndownsps;
    let ndfft1 = nsps * ndd;
    Geom {
        nsps,
        nmax,
        nfft1,
        nh1: nfft1 / 2,
        nstep,
        nhsym: nmax / nstep - 3,
        ndownsps,
        ndown,
        ndd,
        ndfft1,
        ndfft2: ndfft1 / ndown,
        np2: NN * ndownsps,
        tstep: nstep as f32 / 12_000.0,
        df: 12_000.0 / nfft1 as f32,
        az: (12_000.0 / nsps as f32) * az_fac,
        fs2: 12_000.0 / ndown as f32,
        dt2: ndown as f32 / 12_000.0,
        astart: speed.delay_ms() as f32 / 1000.0,
        basesub,
        jstrt,
        jz: speed.jz() as isize,
        nqsymbol: (ndownsps / 4) as isize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every derived constant against JS8Call's Mode{E,A,B,C} blocks
    /// (JS8.cpp:211-338) — the values a human can read off the C++ without
    /// running it. JSTRT is `static_cast<int>(ASTART / TSTEP)` evaluated in
    /// float: 0.5/0.08 → 6.25 → 6, 0.5/0.04 → 12.5 → 12, 0.2/0.025 → 8,
    /// 0.1/0.0125 → 8 (JS8.cpp:236, :267, :299, :334).
    #[test]
    fn geometry_matches_js8call_mode_blocks() {
        let rows = [
            // speed,      nmax,   nfft1, nstep, nhsym, ndown, ndfft1, ndfft2, np2,  fs2,   df,     tstep,  jstrt, nq, az,   basesub
            (Speed::Slow,   360_000, 7680, 960,  372,   120,   360_960, 3008,  2528, 100.0, 1.5625, 0.08,   6,     8,  2.0,  42.0),
            (Speed::Normal, 180_000, 3840, 480,  372,   60,    192_000, 3200,  2528, 200.0, 3.125,  0.04,   12,    8,  4.0,  40.0),
            (Speed::Fast,   120_000, 2400, 300,  397,   60,    120_000, 2000,  1580, 200.0, 5.0,    0.025,  8,     5,  8.0,  39.0),
            (Speed::Turbo,  72_000,  1200, 150,  477,   50,    72_000,  1440,  948,  240.0, 10.0,   0.0125, 8,     3,  12.0, 38.0),
        ];
        for (speed, nmax, nfft1, nstep, nhsym, ndown, ndfft1, ndfft2, np2, fs2, df, tstep, jstrt, nq, az, basesub) in rows {
            let g = geom(speed);
            assert_eq!(g.nmax, nmax, "{speed:?} nmax");
            assert_eq!(g.nfft1, nfft1, "{speed:?} nfft1");
            assert_eq!(g.nh1, nfft1 / 2, "{speed:?} nh1");
            assert_eq!(g.nstep, nstep, "{speed:?} nstep");
            assert_eq!(g.nhsym, nhsym, "{speed:?} nhsym");
            assert_eq!(g.ndown, ndown, "{speed:?} ndown");
            assert_eq!(g.ndfft1, ndfft1, "{speed:?} ndfft1");
            assert_eq!(g.ndfft2, ndfft2, "{speed:?} ndfft2");
            assert_eq!(g.np2, np2, "{speed:?} np2");
            assert!((g.fs2 - fs2).abs() < 1e-3, "{speed:?} fs2 {}", g.fs2);
            assert!((g.df - df).abs() < 1e-5, "{speed:?} df {}", g.df);
            assert!((g.tstep - tstep).abs() < 1e-6, "{speed:?} tstep {}", g.tstep);
            assert_eq!(g.jstrt, jstrt, "{speed:?} jstrt");
            assert_eq!(g.jz, speed.jz() as isize, "{speed:?} jz");
            assert_eq!(g.nqsymbol, nq, "{speed:?} nqsymbol");
            assert!((g.az - az).abs() < 1e-4, "{speed:?} az {}", g.az);
            assert!((g.basesub - basesub).abs() < 1e-6, "{speed:?} basesub");
            assert!((g.astart - speed.delay_ms() as f32 / 1000.0).abs() < 1e-6, "{speed:?} astart");
            // The downsample window must cover the whole period (JS8.cpp:1578 zero-pads
            // "any remainder"; NDFFT1 < NMAX would truncate audio).
            assert!(g.ndfft1 >= g.nmax, "{speed:?}: NDFFT1 {} < NMAX {}", g.ndfft1, g.nmax);
            // Every symbol of a dt-0 frame lies inside the downsampled buffer.
            assert!(((g.astart * g.fs2) as usize) + g.np2 <= g.ndfft2, "{speed:?}: frame outruns NDFFT2");
        }
    }

    /// The synth helper's contract: a period-long buffer whose first
    /// `delay_ms` is pure noise (the frame is slot-positioned), non-silent.
    #[test]
    fn synth_window_is_period_long_and_slot_positioned() {
        use crate::phy::testutil::{synth_window, word};
        for speed in Speed::ALL {
            let iw = synth_window(&[(word("KD9TAWEN52ab"), 1500.0, 20.0)], speed, 1);
            assert_eq!(iw.len(), speed.period_s() as usize * 12_000);
            let delay = speed.delay_ms() as usize * 12;
            let head: f64 = iw[..delay].iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / delay as f64;
            let body: f64 = iw[delay..delay + 12_000].iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / 12_000.0;
            assert!(body > 20.0 * head, "{speed:?}: body/head power {}", body / head);
        }
    }
}
