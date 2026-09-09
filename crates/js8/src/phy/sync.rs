//! Candidate search + noise baseline — JS8Call `syncjs8` and `baselinejs8`
//! (JS8.cpp:1708-1897 and :1449-1560 @ a7ff1be0) read as the SPEC and
//! re-expressed with runtime geometry, table-driven by the speed's Costas
//! kernel: one code path, `speed.costas()` selects the kernel, nothing
//! branches on speed inside the DSP. Lineage: WSJT-X `sync8.f90`
//! (libtempo/vendor/wsjtx/lib/ft8/sync8.f90) — but JS8Call's version differs
//! in every place that matters and THIS file follows JS8Call:
//!   * symbol spectra over a 2·NSPS NUTTALL window at quarter-symbol steps
//!     (JS8.cpp:1712-1739; sync8 uses NSPS samples zero-padded);
//!   * sync = max(abc, ab, bc) — three block pairings (JS8.cpp:1819-1823;
//!     sync8 has abc/bc), every block bounds-guarded;
//!   * ONE peak per bin over the full ±JZ lag range, dt = TSTEP·(j + 0.5)
//!     (JS8.cpp:1770-1832; sync8 keeps an inner-window second peak);
//!   * normalisation by the exact 40th-percentile rank (JS8.cpp:1852-1868);
//!   * candidates taken sync-descending, each erasing everything within ±AZ
//!     in frequency (JS8.cpp:1872-1895) — no MAXPRECAND, no time window;
//!   * the noise baseline is a degree-5 polynomial through six Chebyshev
//!     nodes of the 10th-percentile dB level over 500–2500 Hz (JS8.cpp:
//!     1449-1560), which the SNR estimate in demod.rs reads back.
//!
//! CONTRACT: `find_candidates` returns at most NMAXCAND `(freq, dt, sync)`
//! triples, sync-descending, every one finite and ≥ ASYNCMIN, pairwise more
//! than AZ apart in frequency, plus the baseline array demod needs. Pure —
//! no statics, so four speeds decode concurrently under `std::thread::scope`
//! (B5). Every peak accumulator is initialised before its loop and a
//! non-finite sync is "no candidate": the front-zero-padded RxRing frame
//! (mostly silence) is a normal input, not the Windows 0xC0000005 class it
//! was in Fortran (reference-fortran-uninitialized-hazard).
//!
//! Deliberate divergences (each at its site): (1) rows whose window would
//! run past the period are ZERO here — JS8.cpp:1720 `break`s and leaves the
//! previous decode's rows in its member array `s` (a pure function has no
//! previous decode); (2) an infinite sync (t0 == tx exactly) is dropped —
//! JS8.cpp would carry it into the candidate list; (3) an all-zero band
//! gives no candidates instead of NaN arithmetic.

use rustfft::{num_complex::Complex, FftPlanner};

use super::params::{geom, Geom, ASYNCMIN, NFOS, NMAXCAND, NSSY};
use super::speed::Speed;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Candidate {
    /// Coarse audio offset of tone 0, a multiple of DF = 12000/(2·NSPS) Hz.
    pub freq_hz: f32,
    /// Coarse start time relative to the speed's ASTART, TSTEP·(j + 0.5).
    pub dt_s: f32,
    /// Normalised Costas sync (1.0 = the band's 40th-percentile value).
    pub sync: f32,
}

pub(crate) struct SyncResult {
    pub candidates: Vec<Candidate>,
    /// `nh1` entries: fitted noise baseline (dB + 0.65) over the search bins, 0 elsewhere.
    pub baseline_db: Vec<f32>,
}

/// Nuttall window of length `n`, normalised so Σw = n/300 (JS8.cpp:2077-2107:
/// coefficients a0..a3, `value / sum * size / 300`). Summed in f64 — the C++
/// uses Kahan summation to match gfortran.
fn nuttall(n: usize) -> Vec<f32> {
    const A: [f64; 4] = [0.3635819, -0.4891775, 0.1365995, -0.0106411];
    let pi = std::f64::consts::PI;
    let mut w = Vec::with_capacity(n);
    let mut sum = 0f64;
    for i in 0..n {
        let x = i as f64 / n as f64;
        let v = A[0]
            + A[1] * (2.0 * pi * x).cos()
            + A[2] * (4.0 * pi * x).cos()
            + A[3] * (6.0 * pi * x).cos();
        w.push(v);
        sum += v;
    }
    let fac = n as f64 / (sum * 300.0);
    w.into_iter().map(|v| (v * fac) as f32).collect()
}

/// Solve the 6×6 Vandermonde system V·c = y (V[i][k] = x_i^k) by Gaussian
/// elimination with partial pivoting — the exact interpolant JS8.cpp:1530
/// gets from Eigen's colPivHouseholderQr on the same square matrix.
fn solve_vandermonde(x: &[f64; 6], y: &[f64; 6]) -> [f64; 6] {
    let mut a = [[0f64; 7]; 6];
    for i in 0..6 {
        let mut p = 1f64;
        for cell in a[i].iter_mut().take(6) {
            *cell = p;
            p *= x[i];
        }
        a[i][6] = y[i];
    }
    for col in 0..6 {
        let piv = (col..6)
            .max_by(|&r, &s| a[r][col].abs().total_cmp(&a[s][col].abs()))
            .unwrap();
        a.swap(col, piv);
        let d = a[col][col];
        if d == 0.0 {
            continue; // degenerate nodes cannot happen for distinct Chebyshev abscissae
        }
        let pivot = a[col]; // Copy of the pivot row so the r-loop needn't alias `a`.
        for (r, row) in a.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / d;
                for (dst, &pv) in row.iter_mut().zip(pivot.iter()).skip(col) {
                    *dst -= f * pv;
                }
            }
        }
    }
    let mut c = [0f64; 6];
    for i in 0..6 {
        c[i] = if a[i][i] != 0.0 {
            a[i][6] / a[i][i]
        } else {
            0.0
        };
    }
    c
}

/// baselinejs8 (JS8.cpp:1471-1560): replace `savg` (power) with the fitted
/// noise baseline in dB (+0.65) over bins [ia, ib], 0 elsewhere.
fn baseline(savg: &mut [f32], g: &Geom, ia: usize, ib: usize) {
    const DEGREE: usize = 5; // BASELINE_DEGREE, JS8.cpp:374
    const SAMPLE: usize = 10; // BASELINE_SAMPLE (percentile), :375
    const FMIN: f32 = 500.0; // BASELINE_MIN, :380
    const FMAX: f32 = 2500.0; // BASELINE_MAX, :381
    let bmin = (FMIN / g.df).round() as usize;
    let bmax = (FMAX / g.df).round() as usize;
    let size = bmax - bmin + 1;
    let arm = size / (2 * (DEGREE + 1));
    // Power → dB (:1490-1497). Divergence: a zero bin is floored at 1e-30
    // (−300 dB) instead of −inf so silence stays finite.
    let data: Vec<f32> = savg[bmin..=bmax]
        .iter()
        .map(|&v| 10.0 * v.max(1e-30).log10())
        .collect();
    // Six Chebyshev nodes on [0, size): 0.5·(1 − cos((2i+1)π/12)) (:401-416), each
    // sampled at the 10th percentile of its ±arm span (:1501-1512).
    let mut xs = [0f64; 6];
    let mut ys = [0f64; 6];
    for (i, (xi, yi)) in xs.iter_mut().zip(ys.iter_mut()).enumerate() {
        let node =
            size as f64 * 0.5 * (1.0 - (std::f64::consts::PI / 12.0 * (2 * i + 1) as f64).cos());
        let base = node.round() as isize;
        let lo = (base - arm as isize).clamp(0, size as isize) as usize;
        let hi = (base + arm as isize).clamp(0, size as isize) as usize;
        let mut span: Vec<f32> = data[lo..hi].to_vec();
        if span.is_empty() {
            span.push(data[base.clamp(0, size as isize - 1) as usize]);
        }
        span.sort_by(f32::total_cmp);
        let n = span.len() * SAMPLE / 100;
        *xi = node;
        *yi = span[n] as f64;
    }
    let c = solve_vandermonde(&xs, &ys);
    // Evaluate over [ia, ib] with JS8Call's index mapping (:1539-1556): the
    // SEARCH range is stretched onto the polynomial's [0, size−1] domain.
    for v in savg.iter_mut() {
        *v = 0.0;
    }
    let last = (size - 1) as f32;
    let span = (ib - ia).max(1) as f32;
    for (offset, v) in savg[ia..=ib].iter_mut().enumerate() {
        let x = (offset as f32 * last / span) as f64;
        let mut acc = 0f64;
        let mut pw = 1f64;
        for &ck in &c {
            acc += ck * pw;
            pw *= x;
        }
        *v = acc as f32 + 0.65;
    }
}

/// syncjs8 re-expressed (JS8.cpp:1708-1897). `dd` is the full period as f32
/// (raw i16 values; the window carries the 1/300 scaling).
pub(crate) fn find_candidates(
    dd: &[f32],
    speed: Speed,
    nfa: f32,
    nfb: f32,
    planner: &mut FftPlanner<f32>,
) -> SyncResult {
    let g = geom(speed);
    debug_assert_eq!(
        dd.len(),
        g.nmax,
        "decoder must hand sync exactly one period"
    );
    let empty = |baseline_db: Vec<f32>| SyncResult {
        candidates: Vec::new(),
        baseline_db,
    };

    // 1. Symbol spectra (JS8.cpp:1712-1739): row j = |FFT(dd[j·NSTEP ..+NFFT1] · nuttall)|²,
    //    bins 0..NH1; savg accumulates. Rows whose window passes the period end stay zero.
    let win = nuttall(g.nfft1);
    let fft = planner.plan_fft_forward(g.nfft1);
    let mut buf = vec![Complex::new(0.0f32, 0.0); g.nfft1];
    let mut s = vec![0f32; g.nhsym * g.nh1]; // [row][bin]
    let mut savg = vec![0f32; g.nh1];
    for j in 0..g.nhsym {
        let ia = j * g.nstep;
        let ib = ia + g.nfft1;
        if ib > g.nmax {
            break;
        }
        for (b, (&x, &w)) in buf.iter_mut().zip(dd[ia..ib].iter().zip(win.iter())) {
            *b = Complex::new(x * w, 0.0);
        }
        fft.process(&mut buf);
        let row = &mut s[j * g.nh1..(j + 1) * g.nh1];
        for (i, (r, b)) in row.iter_mut().zip(buf.iter()).enumerate() {
            let p = b.norm_sqr();
            *r = p;
            savg[i] += p;
        }
    }

    // 2. Band clamp (JS8.cpp:1742-1757) and bin range.
    let (mut nfa, mut nfb) = (nfa.round() as i32, nfb.round() as i32);
    let nwin = nfb - nfa;
    if nfa < 100 {
        nfa = 100;
        if nwin < 100 {
            nfb = nfa + nwin;
        }
    }
    if nfb > 4910 {
        nfb = 4910;
        if nwin < 100 {
            nfa = nfb - nwin;
        }
    }
    let ia = ((nfa as f32 / g.df).round() as isize).max(0) as usize;
    let ib = (nfb as f32 / g.df).round().max(0.0) as usize;
    if ib <= ia || ib + NFOS * 6 >= g.nh1 || (2500.0 / g.df).round() as usize >= g.nh1 {
        return empty(vec![0.0; g.nh1]);
    }

    // 3. Noise baseline replaces savg (JS8.cpp:1762).
    baseline(&mut savg, &g, ia, ib);

    // 4. Sync metric per bin: max over lags of max(abc, ab, bc) (JS8.cpp:1770-1832).
    let costas = speed.costas();
    let at = |bin: usize, row: usize| -> f32 { s[row * g.nh1 + bin] };
    let mut entries: Vec<Candidate> = Vec::with_capacity(ib - ia + 1);
    for i in ia..=ib {
        let mut max_value = f32::NEG_INFINITY;
        let mut max_index = -g.jz;
        for j in -g.jz..=g.jz {
            let mut tx = [0f32; 3];
            let mut t0 = [0f32; 3];
            for p in 0..3usize {
                for (n, &tone) in costas[p].iter().enumerate() {
                    let offset = j + g.jstrt + (NSSY * n) as isize + (p * 36 * NSSY) as isize;
                    if offset < 0 || offset as usize >= g.nhsym {
                        continue;
                    }
                    let row = offset as usize;
                    tx[p] += at(i + NFOS * tone as usize, row);
                    for f in 0..7usize {
                        t0[p] += at(i + NFOS * f, row);
                    }
                }
            }
            let sync_of = |a: usize, b: usize| -> f32 {
                let (mut x, mut y) = (0f32, 0f32);
                for k in a..=b {
                    x += tx[k];
                    y += t0[k];
                }
                x / ((y - x) / 6.0)
            };
            let v = sync_of(0, 2).max(sync_of(0, 1)).max(sync_of(1, 2));
            if v > max_value {
                max_value = v;
                max_index = j;
            }
        }
        entries.push(Candidate {
            freq_hz: g.df * i as f32,
            dt_s: g.tstep * (max_index as f32 + 0.5),
            sync: max_value,
        });
    }

    // 5. Normalise to the 40th-percentile rank (JS8.cpp:1852-1868): the element
    //    of ascending rank floor(n·4/10). Silence has no floor → no candidates.
    let mut ranked: Vec<f32> = entries
        .iter()
        .map(|e| {
            if e.sync.is_finite() {
                e.sync
            } else {
                f32::NEG_INFINITY
            }
        })
        .collect();
    ranked.sort_by(f32::total_cmp);
    let base = ranked[ranked.len() * 4 / 10];
    if !base.is_finite() || base <= 0.0 {
        return empty(savg);
    }
    for e in &mut entries {
        e.sync /= base;
    }

    // 6. Extract (JS8.cpp:1872-1895): strongest first; each taken candidate
    //    erases every entry within ±AZ of its frequency (itself included).
    let mut pool: Vec<Candidate> = entries
        .into_iter()
        .filter(|e| e.sync.is_finite() && e.sync >= ASYNCMIN)
        .collect();
    pool.sort_by(|a, b| b.sync.total_cmp(&a.sync));
    let mut candidates = Vec::new();
    while let Some(best) = pool.first().copied() {
        if candidates.len() >= NMAXCAND {
            break;
        }
        candidates.push(best);
        pool.retain(|c| (c.freq_hz - best.freq_hz).abs() > g.az);
    }
    SyncResult {
        candidates,
        baseline_db: savg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phy::params::geom;
    use crate::phy::testutil::{synth_window, word};
    use rustfft::FftPlanner;

    /// A clean −5 dB frame at 1500 Hz, dt 0, at every speed: the top candidate
    /// sits on the right bin (1500 is an exact multiple of every DF), within
    /// three coarse steps of dt 0 (JS8Call's 2·NSPS Nuttall window centres
    /// one to two steps EARLY for a perfectly aligned frame — the fine search
    /// in demod recovers it), and clears ASYNCMIN by a wide margin.
    #[test]
    fn clean_frame_is_the_top_candidate_at_every_speed() {
        for speed in Speed::ALL {
            let g = geom(speed);
            let w = word("KD9TAWEN52ab");
            let iw = synth_window(&[(w, 1500.0, -5.0)], speed, 7);
            let dd: Vec<f32> = iw.iter().map(|&s| s as f32).collect();
            let mut planner = FftPlanner::<f32>::new();
            let r = find_candidates(&dd, speed, 100.0, 4000.0, &mut planner);
            assert!(!r.candidates.is_empty(), "{speed:?}: no candidates");
            let top = r.candidates[0];
            assert!(
                (top.freq_hz - 1500.0).abs() < 1.0,
                "{speed:?}: top freq {}",
                top.freq_hz
            );
            assert!(
                top.dt_s.abs() <= 3.0 * g.tstep + 1e-6,
                "{speed:?}: top dt {}",
                top.dt_s
            );
            assert!(top.sync > 5.0, "{speed:?}: top sync {}", top.sync);
            assert!(r.candidates.len() <= NMAXCAND);
            assert_eq!(r.baseline_db.len(), g.nh1);
            // The baseline is a finite dB figure across the search band.
            let ia = (100.0 / g.df).round() as usize;
            let ib = (4000.0 / g.df).round() as usize;
            assert!(
                r.baseline_db[ia..=ib].iter().all(|v| v.is_finite()),
                "{speed:?}: non-finite baseline"
            );
        }
    }

    /// The front-zero-padded RxRing frame (all silence) is a NORMAL input:
    /// no panic, no NaN, no candidates.
    #[test]
    fn silence_yields_no_candidates_and_no_panic() {
        for speed in Speed::ALL {
            let dd = vec![0f32; geom(speed).nmax];
            let mut planner = FftPlanner::<f32>::new();
            let r = find_candidates(&dd, speed, 100.0, 4000.0, &mut planner);
            assert!(
                r.candidates.is_empty(),
                "{speed:?}: {} candidates from silence",
                r.candidates.len()
            );
        }
    }

    /// Every returned candidate is finite, ≥ ASYNCMIN, inside the band, the
    /// list is sync-descending, and no two are within AZ of each other (the
    /// near-duplicate erase, JS8.cpp:1890-1893).
    #[test]
    fn candidates_are_finite_sorted_in_band_and_az_separated() {
        let speed = Speed::Normal;
        let g = geom(speed);
        let w = word("W1AWFN31abcd");
        let iw = synth_window(&[(w, 2200.0, -18.0)], speed, 11);
        let dd: Vec<f32> = iw.iter().map(|&s| s as f32).collect();
        let mut planner = FftPlanner::<f32>::new();
        let c = find_candidates(&dd, speed, 100.0, 4000.0, &mut planner).candidates;
        for pair in c.windows(2) {
            assert!(pair[0].sync >= pair[1].sync);
        }
        for (i, x) in c.iter().enumerate() {
            assert!(x.sync.is_finite() && x.sync >= ASYNCMIN);
            assert!(x.freq_hz >= 100.0 && x.freq_hz <= 4000.0);
            for y in &c[i + 1..] {
                assert!(
                    (x.freq_hz - y.freq_hz).abs() > g.az,
                    "two candidates within AZ: {x:?} {y:?}"
                );
            }
        }
    }

    /// JS8Call's band clamp (JS8.cpp:1742-1757): nfa < 100 → 100, nfb > 4910
    /// → 4910, and a narrow window is slid rather than widened.
    #[test]
    fn band_edges_are_clamped_like_js8call() {
        let dd = vec![0f32; geom(Speed::Turbo).nmax];
        let mut planner = FftPlanner::<f32>::new();
        // Purely structural: no panic on absurd edges, and no candidates from silence.
        for (nfa, nfb) in [
            (0.0, 20.0),
            (-50.0, 6000.0),
            (4900.0, 4950.0),
            (100.0, 4000.0),
        ] {
            let r = find_candidates(&dd, Speed::Turbo, nfa, nfb, &mut planner);
            assert!(r.candidates.is_empty());
        }
    }
}
