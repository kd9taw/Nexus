//! Power-spectrum estimator for the waterfall display.
//!
//! [`power_spectrum`] runs a Hann-windowed real FFT over the captured audio window and resamples
//! the magnitude spectrum onto the requested display bins (peak-hold). This is a real FFT (via
//! `microfft`), so the resolution is set by the FFT size, not a handful of Goertzel taps — finer
//! bins over a wider band than the old 120-tap bank. The single-tone Goertzel ([`tone_power`]) is
//! retained for the CW decoder's envelope detector, which needs power at exactly one pitch, not a
//! whole spectrum.
//!
//! # The intensity axis is dB, and its reference is absolute (2026-08-04)
//!
//! The row is still 0..1 — that contract is unchanged and every consumer still reads it — but a
//! row value is now LINEAR IN dB across [`DB_SPAN`] dB below full scale, not linear in amplitude,
//! and the reference is a full-scale sine rather than the row's own loudest bin.
//!
//! Both halves of that were visible, and together they are the operator's "the waterfall looks so
//! 8 bit" (2026-08-03):
//!
//! - **Amplitude-linear spends the palette where there is nothing to show.** Palette index was
//!   proportional to the amplitude ratio, so on a measured synthetic FT8 row the noise floor —
//!   95% of the picture — occupied LUT indices 0-15, about 4 bits of tone at ~1 step per dB,
//!   while the loudest 15 dB got the whole colourful middle. Every ordinary FT8 signal rendered
//!   as the same dark blue smudge and strong ones leapt blue→green→red with nothing between.
//!   WSJT-X converts to dB (`flat4.f90:18-20`) and indexes its palette linearly in dB at a fixed
//!   8.16 steps/dB (`plotter.cpp:136,194-197`).
//! - **A per-frame reference makes the whole display breathe.** Dividing each row by its own
//!   maximum moved the reference 50 times a second. FT8 stations key up and drop together, so at
//!   every 15 s slot edge the row max stepped 20-30 dB and the entire background stepped with it;
//!   the UI's ~1.4 s AGC ema lagged that step rather than absorbing it, painting a band across
//!   the full width every cycle. WSJT-X has no per-frame normalization anywhere — it fits and
//!   subtracts a baseline and leaves the scale absolute.
//!
//! Temporal averaging is NOT done here — it is done where the frames are, in
//! [`tempo_app::engine::SpectrumFeed`], which returns the mean of every frame published since
//! the last read (~6 at the FT waterfall's cadence, against WSJT-X's `m_waterfallAvg` default of
//! 5, `widegraph.cpp:160-172`). This module's job is one frame's axis.
//!
//! That split is deliberate, and it is what lets the display be smooth AND live. WSJT-X buys its
//! calm floor with window LENGTH — 1.365 s frames stepped 288 ms, ~2.5 s integrated per drawn
//! row. Buying it that way here is exactly what the operator rejected on 2026-08-01 ("smoothed
//! out to remove response"), when [`FFT_N`] was HALVED because a Hann-weighted window is the
//! display's smear: a signal edge cannot appear until a whole window has passed. Averaging
//! already-published 171 ms frames adds no such lag — the newest frame is in every mean — so the
//! edge still snaps while the floor settles. The frames overlap 88%, so six of them buy roughly
//! ENL 1.7 rather than 6; that is a real reduction in the boil, not a claim of WSJT-X parity.
//!
//! On the dB axis the remaining grain reads as film grain rather than the hard-quantized dither
//! it was before, because there are now ~90 palette levels at the noise floor instead of ~15 to
//! render it in.

/// Goertzel power estimate at frequency `f` (Hz) over `samples` at `sr` (Hz).
fn goertzel(samples: &[f32], sr: f32, f: f32) -> f32 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let w = 2.0 * std::f32::consts::PI * f / sr;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in samples {
        let s0 = x + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0)
}

/// Raw (uncompressed) Goertzel power at a single frequency `f` (Hz) — the CW decoder's
/// envelope detector taps this at the operator's pitch.
pub fn tone_power(samples: &[f32], sr: f32, f: f32) -> f32 {
    goertzel(samples, sr, f)
}

/// FFT size for the waterfall spectrum — matches the display path's rolling audio window
/// (`tempo-audio::rxdsp::WINDOW`; the engine's Companion fallback tail follows it too).
///
/// 2048 @ 12 kHz = 171 ms. Halved from 4096 (2026-08-01) for display liveliness: the window is
/// the display's only temporal smoothing, and 341 ms of Hann-weighted history made every signal
/// edge fade in instead of appear. Raw bins are 5.86 Hz — still finer than the 512-bin display
/// (7.81 Hz over 0–4000 Hz), so nothing visible is lost; the FFT is ~half the work.
const FFT_N: usize = 2048;

/// Display span of the intensity axis, in dB. A row value is LINEAR IN dB across
/// `[-DB_SPAN, 0]` dBFS: `0.0` = the axis floor, `1.0` = full scale.
///
/// ⚠️ The UI mirrors this as `WF_DB_SPAN` in `ui/src/waterfall.ts` — it is the only way a
/// consumer can turn a display value back into dB (the legend readouts, PhoneScope's
/// minimum-dynamic-range clamp). Change one and you must change the other.
///
/// 120 dB is chosen to clear any real capture without clipping: a full-scale sine sits at
/// the top and a quiet rig's per-bin noise floor lands around -95 dBFS, so nothing real is
/// ever crushed against either end. The span is not a contrast control — the UI's visual-AGC
/// re-fits the occupied part of it to the palette every row.
pub const DB_SPAN: f32 = 120.0;

/// Raw-FFT power that a full-scale (amplitude 1.0) sine deposits in its own bin under the Hann
/// window: the bin magnitude is `A·N·CG/2` with Hann's coherent gain `CG = 0.5`, so `p = (N/4)²`.
/// This is the axis's ABSOLUTE reference — it does not move with the signal.
const FULL_SCALE_POWER: f32 = ((FFT_N / 4) * (FFT_N / 4)) as f32;

/// Linear power, RELATIVE TO FULL SCALE, → its 0..1 display value on the dB axis
/// (see [`DB_SPAN`]). Silence (p = 0) floors at 0.0 rather than producing -inf.
///
/// This and [`display_to_power`] are the axis, and they are exact inverses. Everything that
/// converts between the two domains goes through this pair so the definition cannot drift.
pub fn power_to_display(power_ratio: f32) -> f32 {
    let db = 10.0 * power_ratio.max(1e-30).log10();
    ((db + DB_SPAN) / DB_SPAN).clamp(0.0, 1.0)
}

/// A 0..1 display value → the linear power ratio it encodes. The inverse of
/// [`power_to_display`].
///
/// ⚠️ ARITHMETIC ON A ROW MUST COME THROUGH HERE. A row value is linear in dB, i.e. a LOGARITHM,
/// so adding or averaging the values themselves is a geometric mean of the powers — a different
/// and wrong operation, and one that reads plausibly instead of failing. Averaging a waterfall
/// row (`tempo_app::engine::SpectrumFeed`) converts to power, averages, and converts back;
/// WSJT-X does the same, summing its linear-power `s[i]` and taking dB only at the draw
/// (`widegraph.cpp:160-172` then `flat4.f90:18-20`).
pub fn display_to_power(display: f32) -> f32 {
    10f32.powf((display - 1.0) * DB_SPAN / 10.0)
}

/// One raw bin's power → its 0..1 display value on the dB axis (see [`DB_SPAN`]).
fn db_display(power: f32) -> f32 {
    power_to_display(power / FULL_SCALE_POWER)
}

/// Hann window coefficient for sample `i` of an `FFT_N`-length frame (reduces spectral leakage so a
/// carrier reads as a clean peak, not a smear across neighbouring bins).
fn hann(i: usize) -> f32 {
    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_N as f32 - 1.0)).cos()
}

std::thread_local! {
    /// Reused FFT input buffer (8 KB) so the per-tick spectrum computes with no allocation, and
    /// works lock-free from both the radio-loop thread and the IPC fallback thread.
    static FFT_SCRATCH: std::cell::RefCell<[f32; FFT_N]> = const { std::cell::RefCell::new([0.0; FFT_N]) };
}

/// Estimate a `bins`-point power spectrum over `[f_lo, f_hi]` Hz, normalized to 0..1
/// (sqrt-compressed). Bin `i` spans `[f_lo + i·w, f_lo + (i+1)·w)` where `w = (f_hi-f_lo)/bins`, and
/// takes the PEAK raw-FFT power in that range (so a narrow carrier can't fall between display bins).
/// A Hann-windowed real FFT over the last `FFT_N` samples (front-zero-padded while warming up); the
/// DC bin is excluded. Empty input → a zeroed row of length `bins`.
pub fn power_spectrum(samples: &[f32], sr: f32, f_lo: f32, f_hi: f32, bins: usize) -> Vec<f32> {
    if bins == 0 {
        return Vec::new();
    }
    let mut out = FFT_SCRATCH.with(|sc| {
        let mut buf = sc.borrow_mut();
        // Load the last FFT_N samples (front-zero-padded if we have fewer), applying the Hann window.
        let n = samples.len().min(FFT_N);
        let pad = FFT_N - n;
        for v in buf[..pad].iter_mut() {
            *v = 0.0;
        }
        let src = &samples[samples.len() - n..];
        // Remove the DC offset before windowing so a bias in the capture can't leak into the low
        // bins (the bin-0 skip below only drops the exact-DC/Nyquist bin, not the leakage skirt).
        let mean = if n > 0 {
            src.iter().sum::<f32>() / n as f32
        } else {
            0.0
        };
        for i in 0..n {
            buf[pad + i] = (src[i] - mean) * hann(pad + i);
        }
        // In-place real FFT → FFT_N/2 complex bins; bin k is centred at k·sr/FFT_N Hz. rfft packs
        // Nyquist into bin 0's imaginary part, so bin 0 (DC + Nyquist) is skipped entirely.
        let spec = microfft::real::rfft_2048(&mut buf);
        let hz_per_bin = sr / FFT_N as f32;
        let k_max = (FFT_N / 2 - 1) as isize;
        let span = f_hi - f_lo;
        // ⭐ SCATTER, NOT GATHER. Each RAW bin is assigned to the ONE display bin containing its
        // centre frequency, and display bins peak-hold whatever lands in them.
        //
        // The old form gathered per display bin over `floor(flo)..=ceil(fhi)`, which made
        // CONSECUTIVE display bins share raw bins whenever the display grid was coarser than the
        // raw grid — the shipped case, 7.8125 Hz display bins over a 5.859 Hz raw grid. Display
        // bin 0 covered raw 1-2, bin 1 raw 1-3, bin 2 raw 2-4, so one carrier was peak-held into
        // two or three display bins AT IDENTICAL AMPLITUDE: a flat-topped 15.6-23.4 Hz plateau
        // before the Hann lobe was even considered. See
        // `a_carrier_is_one_display_bin_not_a_plateau`.
        //
        // The old comment defended `ceil` as stopping a narrow carrier falling BETWEEN display
        // bins. Scatter is strictly safer on that axis — every raw bin lands in exactly one
        // display bin, so no carrier can be lost — and the nearest-bin fill below covers the
        // opposite regime (a display grid FINER than the raw grid, which is what a zoomed scope
        // span asks for). It is also O(k_max) rather than O(bins x cover), i.e. cheaper.
        let mut acc = vec![0.0f32; bins];
        let mut hit = vec![false; bins];
        for k in 1..=k_max {
            let f = k as f32 * hz_per_bin;
            if f < f_lo || f >= f_hi {
                continue;
            }
            let i = (((f - f_lo) / span) * bins as f32) as usize;
            if i >= bins {
                continue;
            }
            let c = spec[k as usize];
            let p = c.re * c.re + c.im * c.im;
            if p > acc[i] {
                acc[i] = p;
            }
            hit[i] = true;
        }
        // A display bin narrower than a raw bin receives nothing. Fill it from the raw bin
        // NEAREST its centre so a zoomed span reads as a smooth lobe rather than a comb of
        // zeros — the display is finer than the data there, and saying so honestly means
        // repeating the nearest measurement, never inventing one.
        for (i, h) in hit.iter().enumerate() {
            if *h {
                continue;
            }
            let f = f_lo + span * (i as f32 + 0.5) / bins as f32;
            let k = (f / hz_per_bin).round() as isize;
            if (1..=k_max).contains(&k) {
                let c = spec[k as usize];
                acc[i] = c.re * c.re + c.im * c.im;
            }
        }
        acc
    });
    // Raw power → the dB display axis. ABSOLUTE (against full scale), so the reference never
    // moves with the signal — see `db_display` and the two axis tests below.
    for v in out.iter_mut() {
        *v = db_display(*v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect()
    }

    #[test]
    fn tone_peaks_in_the_right_bin() {
        let sr = 12_000.0;
        let s = tone(1500.0, sr, 4096);
        let bins = 120;
        let (f_lo, f_hi) = (200.0, 2900.0);
        let row = power_spectrum(&s, sr, f_lo, f_hi, bins);
        assert_eq!(row.len(), bins);

        // The strongest bin should be the one whose center is nearest 1500 Hz.
        let peak = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        let peak_f = f_lo + (f_hi - f_lo) * (peak as f32 + 0.5) / bins as f32;
        assert!(
            (peak_f - 1500.0).abs() < (f_hi - f_lo) / bins as f32 * 2.0,
            "peak at {peak_f} Hz"
        );
        // A full-scale sine sits at the TOP of the dB axis. This used to read exactly 1.0 by
        // construction — every row was divided by its own loudest bin, so the peak was 1.0
        // whatever the signal level was. Now it is 1.0 because the tone really is 0 dBFS, and
        // the tolerance is the Hann coherent gain's departure from exactly 0.5 (the window uses
        // an N-1 denominator), not slack.
        assert!(
            (row[peak] - 1.0).abs() < 0.002,
            "a full-scale tone reads the top of the axis (got {})",
            row[peak]
        );
    }

    /// ⭐ A carrier must be ONE display bin with real shoulders, not a flat-topped plateau.
    ///
    /// The old display-bin assignment gathered raw bins with `floor(flo)`..`ceil(fhi)`, so
    /// CONSECUTIVE display bins shared raw bins: at 512 bins over 0-4000 Hz (7.8125 Hz display
    /// bins over a 5.859 Hz raw grid) display bin 0 covered raw 1-2, bin 1 covered raw 1-3, bin 2
    /// covered raw 2-4. A carrier landing on one raw bin was therefore peak-held into two or
    /// three display bins AT IDENTICAL AMPLITUDE — a 15.6-23.4 Hz flat top before the Hann lobe
    /// is even considered. On the CW cockpit's 800 Hz view that is a ~75 px blob interpolated
    /// from three real numbers, which is the operator's "not crisp" (2026-08-15).
    ///
    /// The scatter form assigns each raw bin to the ONE display bin containing its centre, so a
    /// carrier reads as a peak with true -6 dB Hann shoulders.
    #[test]
    fn a_carrier_is_one_display_bin_not_a_plateau() {
        let sr = 12_000.0;
        // Sit the tone exactly on a raw-bin centre (k * sr/FFT_N) so this measures the display-bin
        // assignment and not scalloping loss: k = 256 -> 1500.0 Hz.
        let f = 256.0 * sr / FFT_N as f32;
        let s = tone(f, sr, FFT_N);
        // The shipped scope geometry: 512 display bins over 0-4000 Hz.
        let row = power_spectrum(&s, sr, 0.0, 4000.0, 512);
        let peak = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;

        // `db_display` is linear-in-dB over DB_SPAN, so a display delta converts straight to dB.
        let db_per_unit = DB_SPAN;
        let shoulder_db = |i: usize| (row[peak] - row[i]) * db_per_unit;

        assert!(
            peak > 0 && peak + 1 < row.len(),
            "peak {peak} needs both neighbours"
        );
        assert!(
            shoulder_db(peak - 1) >= 5.0,
            "the bin BELOW the carrier must be a real shoulder, not a copy of the peak \
             (got {:.1} dB down; a plateau reads ~0)",
            shoulder_db(peak - 1)
        );
        assert!(
            shoulder_db(peak + 1) >= 5.0,
            "the bin ABOVE the carrier must be a real shoulder, not a copy of the peak \
             (got {:.1} dB down; a plateau reads ~0)",
            shoulder_db(peak + 1)
        );
    }

    #[test]
    fn empty_input_is_zeros() {
        let row = power_spectrum(&[], 12_000.0, 200.0, 2900.0, 64);
        assert_eq!(row.len(), 64);
        assert!(row.iter().all(|&v| v == 0.0));
    }

    fn two_tones(f1: f32, f2: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                (2.0 * std::f32::consts::PI * f1 * t).sin()
                    + (2.0 * std::f32::consts::PI * f2 * t).sin()
            })
            .collect()
    }

    // The fidelity proof: two tones only 40 Hz apart resolve as two peaks with a dip between them —
    // impossible with the old 120-bin/22.5 Hz Goertzel bank, easy at ~7.8 Hz FFT display bins.
    #[test]
    fn resolves_two_close_tones() {
        let sr = 12_000.0;
        let (lo, hi, bins) = (0.0f32, 4000.0f32, 512usize);
        let row = power_spectrum(&two_tones(1500.0, 1540.0, sr, 4096), sr, lo, hi, bins);
        let bin_of = |f: f32| ((f - lo) / (hi - lo) * bins as f32) as usize;
        let near = |b: usize| row[b - 1].max(row[b]).max(row[b + 1]); // allow ±1 bin for the peak
        let peak1 = near(bin_of(1500.0));
        let peak2 = near(bin_of(1540.0));
        let dip = row[bin_of(1520.0)];
        // Both full-scale tones reach the top of the axis...
        assert!(
            peak1 > 0.99 && peak2 > 0.99,
            "both tones present (p1={peak1}, p2={peak2})"
        );
        // ...and the valley between them is a real null. Stated in dB, because the axis is dB:
        // the old `dip < 0.5` was an amplitude-ratio threshold, and a log axis necessarily reads
        // a deep null as a HIGH number (a 40 dB null is 0.67 of a 120 dB span, not 0.01). That
        // made the old constant look violated by a result that is in fact ~40 dB of separation —
        // far better than the ~6 dB the amplitude threshold actually demanded.
        let null_db = (peak1.min(peak2) - dip) * DB_SPAN;
        assert!(
            null_db > 20.0,
            "resolved with a deep null between them (only {null_db} dB down)"
        );
    }

    /// A sum of `(freq, amplitude)` tones, for level-accuracy tests.
    fn tones(spec: &[(f32, f32)], sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                spec.iter()
                    .map(|(f, a)| a * (2.0 * std::f32::consts::PI * f * t).sin())
                    .sum()
            })
            .collect()
    }

    /// Peak display value in the neighbourhood of `f` Hz of a 512-bin 0–4000 Hz row.
    fn peak_near(row: &[f32], f: f32) -> f32 {
        let b = (f / 4000.0 * 512.0) as usize;
        row[b.saturating_sub(2)..(b + 3).min(row.len())]
            .iter()
            .copied()
            .fold(0.0f32, f32::max)
    }

    // 750 / 1500 / 2250 Hz are exact raw-bin centers at 12 kHz / 2048 (5.859375 Hz per bin),
    // so these tests measure levels, not scalloping loss.
    const F_A: f32 = 750.0;
    const F_B: f32 = 1500.0;
    const F_C: f32 = 2250.0;

    // ⭐ THE AXIS CONTRACT. The display value must be LINEAR IN dB, so equal dB steps are equal
    // palette steps everywhere — WSJT-X indexes its palette at a fixed 8.16 steps/dB
    // (plotter.cpp:136,194-197) after converting to dB in flat4.f90:18-20.
    //
    // The old axis was amplitude-linear (`(p/max).sqrt()`), which spends the palette where
    // there is no texture to show: measured on a synthetic FT8 row, the noise floor occupied
    // LUT indices 0-15 — 15 of 256 levels, ~1 step per dB — while the top 15 dB got the entire
    // colourful middle. That is the operator's "looks so 8 bit" (2026-08-03), literally: about
    // 4 bits of tone across 95% of the picture.
    #[test]
    fn equal_db_steps_are_equal_display_steps() {
        let sr = 12_000.0;
        // Three tones 20 dB apart: 0, -20, -40 dBFS.
        let s = tones(&[(F_A, 1.0), (F_B, 0.1), (F_C, 0.01)], sr, 4096);
        let row = power_spectrum(&s, sr, 0.0, 4000.0, 512);
        let (a, b, c) = (
            peak_near(&row, F_A),
            peak_near(&row, F_B),
            peak_near(&row, F_C),
        );
        assert!(a > b && b > c, "monotonic in level (a={a}, b={b}, c={c})");
        // 20 dB is 20/DB_SPAN of the axis, wherever it sits.
        let step = 20.0 / DB_SPAN;
        assert!(
            (a - b - step).abs() < 0.005,
            "0→-20 dB is one 20 dB step (got {})",
            a - b
        );
        assert!(
            (b - c - step).abs() < 0.005,
            "-20→-40 dB is the SAME step (got {}) — the axis is dB-linear, not amplitude-linear",
            b - c
        );
    }

    // ⭐ THE REFERENCE CONTRACT. The axis is ABSOLUTE (dB relative to a full-scale sine), never
    // per-frame. The old code divided every row by its own loudest bin, so the reference moved
    // 50 times a second: FT8 stations all key up and drop together, so the row max stepped
    // 20-30 dB at every 15 s slot edge and the whole background brightness stepped with it —
    // a band across the full width every cycle, which the UI's 1.4 s AGC ema lagged rather
    // than absorbed. WSJT-X has no per-frame normalization anywhere.
    #[test]
    fn a_loud_signal_does_not_move_the_rest_of_the_row() {
        let sr = 12_000.0;
        let quiet = power_spectrum(&tones(&[(F_C, 0.01)], sr, 4096), sr, 0.0, 4000.0, 512);
        // The SAME weak tone, now with a full-scale station keyed up elsewhere in the band.
        let loud = power_spectrum(
            &tones(&[(F_C, 0.01), (F_A, 1.0)], sr, 4096),
            sr,
            0.0,
            4000.0,
            512,
        );
        let (before, after) = (peak_near(&quiet, F_C), peak_near(&loud, F_C));
        assert!(
            (before - after).abs() < 0.002,
            "the weak tone reads the same with and without a loud neighbour \
             ({before} → {after}); a moving reference IS the slot-edge pumping"
        );
    }

    // The reference itself: full scale is the top of the axis, and halving the amplitude costs
    // exactly 6.02 dB of it. Pins `FULL_SCALE_POWER` against the Hann coherent gain, so a change
    // to the window can't silently slide the whole display up or down.
    #[test]
    fn full_scale_sits_at_the_top_of_the_axis() {
        let sr = 12_000.0;
        let full = peak_near(
            &power_spectrum(&tones(&[(F_B, 1.0)], sr, 4096), sr, 0.0, 4000.0, 512),
            F_B,
        );
        let half = peak_near(
            &power_spectrum(&tones(&[(F_B, 0.5)], sr, 4096), sr, 0.0, 4000.0, 512),
            F_B,
        );
        assert!(
            (full - 1.0).abs() < 0.002,
            "a full-scale sine reads ~1.0 (got {full})"
        );
        assert!(
            (full - half - 6.0206 / DB_SPAN).abs() < 0.002,
            "half amplitude is 6.02 dB down (got {} dB)",
            (full - half) * DB_SPAN
        );
    }

    // A large DC bias in the capture must not dominate the low bins (mean-removed + bin-0 skipped).
    #[test]
    fn dc_offset_is_excluded() {
        let sr = 12_000.0;
        let s: Vec<f32> = (0..4096)
            .map(|i| 5.0 + (2.0 * std::f32::consts::PI * 1500.0 * i as f32 / sr).sin())
            .collect();
        let row = power_spectrum(&s, sr, 0.0, 4000.0, 512);
        let peak = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        let peak_f = peak as f32 / 512.0 * 4000.0;
        assert!(
            (peak_f - 1500.0).abs() < 20.0,
            "peak is the tone, not DC (got {peak_f} Hz)"
        );
    }

    // Warm-up: fewer than FFT_N samples are front-zero-padded and still peak in the right place.
    #[test]
    fn short_input_still_peaks() {
        let sr = 12_000.0;
        let row = power_spectrum(&tone(1500.0, sr, 1000), sr, 0.0, 4000.0, 512);
        let peak = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        let peak_f = peak as f32 / 512.0 * 4000.0;
        assert!(
            (peak_f - 1500.0).abs() < 40.0,
            "short-input peak near 1500 Hz (got {peak_f} Hz)"
        );
    }
}
