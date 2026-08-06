//! Slant correction + line-zero phase alignment.
//!
//! Translated from slowrx's `sync.c` (Oona Räisänen, ISC License).
//! See `NOTICE.md`. Two responsibilities:
//!
//! 1. [`SyncTracker`] — per-sample boolean "is the 1200 Hz sync pulse
//!    dominant here?" Equivalent of slowrx's `Praw`/`Psync` ratio in
//!    `video.c` lines 271-297.
//! 2. [`find_sync`] — Hough-transform a captured `has_sync` track to
//!    detect slant, adjust the rate to cancel it, then locate line 0's
//!    `Skip` via 8-tap convolution on the column-summed sync image.
//!    Equivalent of slowrx's `sync.c::FindSync` (lines 18-133).
//! 3. [`SyncPulseGate`] — "was there a *real* sync pulse here?", which is
//!    a different and much harder question than (1), and the one an
//!    end-of-transmission trigger needs. Nexus addition, no slowrx
//!    counterpart: slowrx is offline-batch and never has to decide that a
//!    transmission stopped.
//!
//! Slowrx is offline-batch (read-all → first `GetVideo` populates
//! `HasSync[]` → `FindSync` adjusts → second `GetVideo` rereads cached
//! `StoredLum` at corrected pixel times). Our decoder accumulates one
//! image's worth of audio in the `Decoding` state, probes [`SyncTracker`]
//! at every [`SYNC_PROBE_STRIDE`] samples, then runs [`find_sync`] once.
//! The corrected `(rate, skip)` drives a single per-pixel decode pass.
//! `LineDecoded` events fire in fast succession at end-of-buffer rather
//! than incrementally; callers still see every event.

use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Arc;

use crate::modespec::ModeSpec;
use crate::resample::WORKING_SAMPLE_RATE_HZ;

/// Stride between sync-band probes (working-rate samples).
///
/// slowrx uses 13 samples@44.1 kHz (`video.c:295`) ≈ 3.25 samples@11.025 kHz.
/// The fractional equivalence means no integer stride gives exact slowrx parity;
/// we choose 4 (round-up / ceil) rather than 3 (round-down / floor).
///
/// **Probe-count comparison:**
/// - slowrx probes/image ≈ `image_samples / 13` at 44.1 kHz.
/// - Rust probes/image ≈ `image_samples_11025 / 4` at 11.025 kHz.
/// - `image_samples_11025 / 4 ≈ (image_samples_44100 / 4) / 4 ≈ image_samples_44100 / 16`,
///   which is slightly fewer probes than slowrx's `/ 13`.
///
/// With `SYNC_PROBE_STRIDE = 4` Rust's per-image probe count is ≈ 19% fewer
/// than slowrx's. With stride=3 it was ≈ 25% more. Stride=4 is closer in
/// ratio (1.56 vs slowrx) and preserves the ~0.36 ms/probe cadence.  The
/// Hough transform's line-finding is robust to moderate density differences
/// (round-2 audit Finding 8).
pub(crate) const SYNC_PROBE_STRIDE: usize = 4;

/// Hann-windowed audio length per sync probe (samples). 1/4 of slowrx's
/// 64@44.1kHz keeps the time span (~1.5 ms) constant (`video.c:278`).
pub(crate) const SYNC_FFT_WINDOW_SAMPLES: usize = 16;

/// Zero-padded FFT length per sync probe. 256@11025 = 43 Hz/bin matches
/// slowrx's 1024@44100 (`video.c:280`).
pub(crate) const SYNC_FFT_LEN: usize = 256;

// Hough-transform slant search (slowrx `common.h:4-5` MINSLANT/MAXSLANT
// + sync.c step `q++` in 0.5° units via `q/2.0`); slant lock window
// matches sync.c:83 `slantAngle > 89 && slantAngle < 91`.
const MIN_SLANT_DEG: f64 = 30.0;
const MAX_SLANT_DEG: f64 = 150.0;
const SLANT_STEP_DEG: f64 = 0.5;
const SLANT_OK_LO_DEG: f64 = 89.0;
const SLANT_OK_HI_DEG: f64 = 91.0;
const MAX_SLANT_RETRIES: usize = 3;
// `xAcc[700]` (sync.c:23), `SyncImg[700][630]` (sync.c:26),
// `lines[600][...]` (sync.c:24).
const X_ACC_BINS: usize = 700;
const SYNC_IMG_Y_BINS: usize = 630;
const LINES_D_BINS: usize = 600;

/// Right-edge slip threshold for the falling-edge `xmax`: if `xmax`
/// exceeds half the column-accumulator span, the detected pulse
/// belongs to the next line's leading sync — wrap left by this
/// amount. Matches slowrx `sync.c:117` (`if (xmax > 350) xmax -= 350;`).
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
const X_ACC_SLIP_THRESHOLD: i32 = (X_ACC_BINS / 2) as i32; // 350

/// 8-tap falling-edge detection kernel: leading 4 ones, trailing 4
/// negative ones. Convolved with the column-accumulator `x_acc`;
/// the position of the maximum response is the falling edge of the
/// dominant sync pulse. Matches slowrx `sync.c:108` (the inline
/// literal `{1,1,1,1,-1,-1,-1,-1}`).
const SYNC_EDGE_KERNEL: [i32; 8] = [1, 1, 1, 1, -1, -1, -1, -1];
const SYNC_EDGE_KERNEL_LEN: usize = SYNC_EDGE_KERNEL.len();

/// Convert degrees to radians. Matches slowrx `common.c::deg2rad`.
fn deg2rad(deg: f64) -> f64 {
    deg * std::f64::consts::PI / 180.0
}

/// Per-sample sync-band probe context (FFT plan + buffers reused across
/// probes). `sync_target_bin` / `video_{lo,hi}_bin` are pre-computed bin
/// offsets corresponding to `1200 Hz` and `1500..=2300 Hz` shifted by
/// `hedr_shift_hz`.
pub(crate) struct SyncTracker {
    fft: Arc<dyn rustfft::Fft<f32>>,
    hann: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    sync_target_bin: usize,
    video_lo_bin: usize,
    video_hi_bin: usize,
}

impl SyncTracker {
    /// Construct a tracker with the radio mistuning offset extracted at
    /// VIS time.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn new(hedr_shift_hz: f64) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(SYNC_FFT_LEN);
        let scratch_len = fft.get_inplace_scratch_len();

        // Use slowrx-equivalent truncation via `crate::dsp::get_bin` (not `.round()`).
        // See `crate::dsp::get_bin` for rationale.  sync_target_bin for 1200 Hz
        // is 27 (slowrx-correct) not 28 (what `.round()` would give).
        let bin_for =
            |hz: f64| -> usize { crate::dsp::get_bin(hz, SYNC_FFT_LEN, WORKING_SAMPLE_RATE_HZ) };

        Self {
            fft,
            hann: build_sync_hann(),
            fft_buf: vec![Complex { re: 0.0, im: 0.0 }; SYNC_FFT_LEN],
            scratch: vec![Complex { re: 0.0, im: 0.0 }; scratch_len.max(SYNC_FFT_LEN)],
            sync_target_bin: bin_for(1200.0 + hedr_shift_hz),
            video_lo_bin: bin_for(1500.0 + hedr_shift_hz),
            video_hi_bin: bin_for(2300.0 + hedr_shift_hz),
        }
    }

    /// Probe a single window centered at `center_sample` of `audio`.
    /// Returns `true` when the 1200 Hz sync band has more power per Hz
    /// than the 1500-2300 Hz video band by at least 2×.
    ///
    /// Translated from slowrx `video.c` lines 271-297.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    pub fn has_sync_at(&mut self, audio: &[f32], center_sample: usize) -> bool {
        let half = (SYNC_FFT_WINDOW_SAMPLES as i64) / 2;
        self.fft_buf.fill(Complex { re: 0.0, im: 0.0 });
        for i in 0..SYNC_FFT_WINDOW_SAMPLES {
            let idx = (center_sample as i64) - half + (i as i64);
            let s = if idx >= 0 && (idx as usize) < audio.len() {
                audio[idx as usize]
            } else {
                0.0
            };
            self.fft_buf[i].re = s * self.hann[i];
        }
        self.fft
            .process_with_scratch(&mut self.fft_buf, &mut self.scratch[..]);

        // Praw = average power per bin across video band (video.c:282-288).
        let mut p_raw = 0.0_f64;
        let lo = self.video_lo_bin.max(1);
        let hi = self.video_hi_bin.min(SYNC_FFT_LEN / 2 - 1);
        if hi >= lo {
            for k in lo..=hi {
                p_raw += crate::dsp::power(self.fft_buf[k]);
            }
            p_raw /= (hi - lo).max(1) as f64;
        }

        // Psync = triangle-weighted sum across [bin-1, bin, bin+1] / 2
        // (video.c:285-289).
        let mut p_sync = 0.0_f64;
        let bin = self.sync_target_bin.clamp(1, SYNC_FFT_LEN / 2 - 1);
        for offset in -1_i32..=1 {
            let k = (bin as i32 + offset) as usize;
            let weight = 1.0 - 0.5 * f64::from(offset.abs());
            p_sync += crate::dsp::power(self.fft_buf[k]) * weight;
        }
        p_sync /= 2.0;

        // slowrx video.c:293: HasSync = (Psync > 2*Praw)
        p_sync > 2.0 * p_raw
    }
}

/// Build the Hann window used per sync probe.
fn build_sync_hann() -> Vec<f32> {
    crate::dsp::build_hann(SYNC_FFT_WINDOW_SAMPLES)
}

/// Share of a window's mean-square power that a single-frequency
/// correlation at the sync tone must account for before that window
/// counts as a sync pulse.
///
/// Measured 2026-08-06 over 60 s each of SSB-passband-shaped noise at
/// −120, −20 and −6 dBFS (two seeds) plus white noise at 0.3 RMS, sliding
/// the window every [`SYNC_PROBE_STRIDE`] samples: across ~165 000 window
/// positions per case the **highest** value reached was 0.451 with the
/// 4.862 ms Martin window, 0.295 at 9 ms and 0.134 at 20 ms — and the
/// figures were bit-identical at every amplitude, which is the point (see
/// [`SyncPulseGate`]). Over the same measurement a real transmission
/// reached ≥ 0.6 on **every** scan line of Martin 1, Scottie 1, PD-120 and
/// PD-290, clean and with AWGN down to 6 dB SNR. 0.6 has margin on both
/// sides; a threshold anywhere in 0.5-0.75 gives the same verdicts.
const SYNC_TONE_PURITY_MIN: f64 = 0.60;

/// Samples between renormalisations of the phase rotator. Bounds the
/// multiplicative drift of the `e^{jω}` recurrence without a `sin`/`cos`
/// call per sample.
const ROTATOR_RENORM_SAMPLES: usize = 4096;

/// "Was there a real sync pulse here?" — the end-of-transmission detector.
///
/// [`SyncTracker::has_sync_at`] cannot answer this, and it is worth being
/// precise about why, because the shipped end-of-transmission trigger was
/// built on it and consequently could not fire on a radio. That probe is
/// slowrx's `Psync > 2·Praw`, a **ratio with no floor**: both sides scale
/// with the input, so its verdict does not depend on the signal being
/// there at all. Band noise satisfies it 24.8 % of the time — measured,
/// and *identically* at −120 dBFS and at −6 dBFS. For [`find_sync`] that
/// is harmless (the Hough transform dilutes scattered false probes), and
/// for "has the carrier stopped?" it is fatal: the trigger asked for three
/// line periods without a single true probe, and only digital silence —
/// which no receiver produces — ever delivered that.
///
/// This gate asks a question that an amplitude cannot answer either way:
/// **is the audio here a coherent tone at the sync frequency, for as long
/// as this mode's own sync pulse lasts?** A sync pulse is exactly that by
/// construction. Noise is not, at any level — spreading its power across
/// the passband is what makes it noise. The statistic is the share of the
/// window's mean-square power accounted for by a single-frequency
/// correlation at `1200 Hz + hedr_shift` (1.0 for a pure tone, and
/// scale-free, so no gain setting moves it); the threshold and its
/// measured separation are [`SYNC_TONE_PURITY_MIN`].
///
/// Fed the decoder's growing audio buffer, it slides that window one
/// sample at a time on three running sums, evaluating every
/// [`SYNC_PROBE_STRIDE`] samples, and remembers where the last qualifying
/// window began.
pub(crate) struct SyncPulseGate {
    /// Analysis window — this mode's sync duration, in samples.
    win: usize,
    /// `cos ω`, `sin ω` for the per-sample phase rotator.
    step: (f64, f64),
    /// Current rotator value `e^{jωi}` for the next sample `i`.
    rot: (f64, f64),
    /// Ring of `[s·cos ωi, s·sin ωi, s²]` for the last `win` samples.
    ring: Vec<[f64; 3]>,
    /// Running sums over `ring`.
    sums: [f64; 3],
    /// Next sample index of the caller's buffer to consume.
    next: usize,
    /// Start sample of the most recent window that qualified as a pulse.
    last_pulse: Option<usize>,
}

impl SyncPulseGate {
    /// Build a gate for `spec`, listening at `1200 Hz + hedr_shift_hz` —
    /// the same mistuning offset the rest of the decode uses, so a radio
    /// off frequency does not silently lose its sync pulses.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn new(spec: ModeSpec, hedr_shift_hz: f64) -> Self {
        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        // The shortest sync in the table is Martin's 4.862 ms = 54 samples;
        // the floor only guards a hypothetical future table entry short
        // enough to make the correlation meaningless.
        let win = ((spec.sync_seconds * work_rate).round() as usize).max(16);
        let omega = 2.0 * std::f64::consts::PI * (1200.0 + hedr_shift_hz) / work_rate;
        Self {
            win,
            step: (omega.cos(), omega.sin()),
            rot: (1.0, 0.0),
            ring: vec![[0.0; 3]; win],
            sums: [0.0; 3],
            next: 0,
            last_pulse: None,
        }
    }

    /// Consume everything in `audio` past what has already been analysed.
    /// `audio` must be the same append-only buffer across calls.
    #[allow(clippy::cast_precision_loss)]
    pub fn advance(&mut self, audio: &[f32]) {
        while self.next < audio.len() {
            let i = self.next;
            let s = f64::from(audio[i]);
            let add = [s * self.rot.0, s * self.rot.1, s * s];
            let slot = i % self.win;
            let old = self.ring[slot];
            for (sum, (a, o)) in self
                .sums
                .iter_mut()
                .zip(add.iter().copied().zip(old.iter().copied()))
            {
                *sum += a - o;
            }
            self.ring[slot] = add;

            // Advance the rotator, renormalising periodically so the
            // recurrence cannot drift.
            let (c, sn) = self.step;
            self.rot = (
                self.rot.0.mul_add(c, -(self.rot.1 * sn)),
                self.rot.0.mul_add(sn, self.rot.1 * c),
            );
            if i.is_multiple_of(ROTATOR_RENORM_SAMPLES) {
                let m = self.rot.0.hypot(self.rot.1);
                if m > 0.0 {
                    self.rot = (self.rot.0 / m, self.rot.1 / m);
                }
            }

            self.next = i + 1;
            // The window moves SYNC_PROBE_STRIDE samples (0.36 ms) between
            // evaluations — far finer than the shortest sync pulse, so a
            // pulse cannot be stepped over.
            if self.next >= self.win
                && self.next.is_multiple_of(SYNC_PROBE_STRIDE)
                && self.window_is_tone()
            {
                self.last_pulse = Some(self.next - self.win);
            }
        }
    }

    /// Does the window currently in the ring hold a coherent sync tone?
    #[allow(clippy::cast_precision_loss)]
    fn window_is_tone(&self) -> bool {
        let [i_sum, q_sum, sq_sum] = self.sums;
        if sq_sum <= 0.0 {
            return false;
        }
        let n = self.win as f64;
        // `2·|Σ s·e^{-jωt}|² / N²` is the mean-square a sinusoid of the
        // best-fitting amplitude at ω contributes; `Σs²/N` is the window's
        // own mean square. The ratio is 1 for a pure tone.
        let tone = 2.0 * i_sum.mul_add(i_sum, q_sum * q_sum) / (n * n);
        tone / (sq_sum / n) >= SYNC_TONE_PURITY_MIN
    }

    /// Start sample of the most recent qualifying pulse, if any.
    pub fn last_pulse_sample(&self) -> Option<usize> {
        self.last_pulse
    }

    /// How much of the caller's buffer has been analysed, in samples.
    pub fn analysed_samples(&self) -> usize {
        self.next
    }
}

/// Result of [`find_sync`]: slant-corrected rate + line-zero `Skip`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SyncResult {
    /// Adjusted working-rate sample rate (Hz).
    pub adjusted_rate_hz: f64,
    /// Sample offset from the start of the sync track where line 0's
    /// video data begins. May be slightly negative; the decoder zero-pads
    /// out-of-range reads when computing per-channel slices.
    pub skip_samples: i64,
    /// Detected slant angle (degrees), or `None` when the Hough transform
    /// found no sync pulses at all (degenerate/empty input).
    ///
    /// Diagnostic — read by tests; the decoder consumes only
    /// `adjusted_rate_hz` + `skip_samples`. Using `Option<f64>` avoids the
    /// round-2 audit Finding 10 ambiguity where `90.0` would be returned for
    /// both "perfectly aligned input" and "nothing-detected-at-all input".
    #[allow(dead_code)]
    pub slant_deg: Option<f64>,
}

/// Linear Hough transform + 8-tap convolution edge-find.
///
/// `has_sync` is the per-stride boolean track produced by [`SyncTracker`].
/// `initial_rate_hz` is normally [`WORKING_SAMPLE_RATE_HZ`] but the
/// function may adjust it. Translated from slowrx `sync.c::FindSync`
/// (lines 18-133).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub(crate) fn find_sync(has_sync: &[bool], initial_rate_hz: f64, spec: ModeSpec) -> SyncResult {
    let line_width: usize = ((spec.line_seconds / spec.sync_seconds) * 4.0) as usize;
    let num_lines = spec.image_lines as usize;
    let mut rate = initial_rate_hz;
    let mut slant_deg_detected: Option<f64> = None;

    for retry in 0..=MAX_SLANT_RETRIES {
        let Some((slant, adjusted)) = hough_detect_slant(has_sync, rate, spec, line_width) else {
            // No sync pulses → no Hough peak → no rate correction.
            break;
        };
        slant_deg_detected = Some(slant);

        // Apply a deadband at 90° so an exact-rate input is not
        // perturbed by half-degree Hough quantization noise (see
        // docs/intentional-deviations.md "FindSync 90° slant deadband").
        if (slant - 90.0).abs() > SLANT_STEP_DEG {
            rate = adjusted;
        }

        // sync.c:86-90 resets to 44100 on retry exhaustion; we keep
        // our last estimate (see docs/intentional-deviations.md
        // "FindSync retry-exhaustion"). Open interval (89, 91) matches
        // slowrx sync.c:83 exactly — half-open `89.0..91.0` would
        // widen the lock by one 0.5°-Hough bin (round-2 audit
        // Finding 7).
        if (slant > SLANT_OK_LO_DEG && slant < SLANT_OK_HI_DEG) || retry == MAX_SLANT_RETRIES {
            break;
        }
    }

    let xmax = find_falling_edge(has_sync, rate, spec, num_lines);
    let s_secs = skip_seconds_for(xmax, spec);
    let skip_samples = (s_secs * rate).round() as i64;

    SyncResult {
        adjusted_rate_hz: rate,
        skip_samples,
        slant_deg: slant_deg_detected,
    }
}

/// Build the 2D sync image at `rate_hz`, then linear-Hough-transform
/// it to find the dominant slant angle. Returns `None` when no sync
/// pulses register at all (degenerate input). The returned
/// `adjusted_rate` already has the standard Hough-derived correction
/// applied (`rate × tan(90° − slant) / line_width × rate`); the
/// caller applies the 90° deadband before adopting it.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn hough_detect_slant(
    has_sync: &[bool],
    rate_hz: f64,
    spec: ModeSpec,
    line_width: usize,
) -> Option<(f64 /* slant_deg */, f64 /* adjusted_rate */)> {
    let n_slant_bins = ((MAX_SLANT_DEG - MIN_SLANT_DEG) / SLANT_STEP_DEG).round() as usize;
    let num_lines = spec.image_lines as usize;

    // Column-major: x is the outer dim because the Hough vote loop
    // iterates `for cy { for cx { … } }` and we want sequential x to
    // share a cache line. Matches slowrx C's `SyncImg[700][630]`
    // shape (C10 audit).
    let sync_img_idx = |x: usize, y: usize| x * SYNC_IMG_Y_BINS + y;

    // Row-major: d is the outer dim (the slowrx C `Lines[600][240]`
    // shape). Vote increments scan q-inner.
    let lines_idx = |d: usize, q: usize| d * n_slant_bins + q;

    let probe_index = |t: f64| -> usize {
        let raw = t * rate_hz / (SYNC_PROBE_STRIDE as f64);
        if raw < 0.0 {
            0
        } else {
            raw as usize
        }
    };

    // Draw the 2D sync signal at current rate.
    let mut sync_img = vec![false; X_ACC_BINS * SYNC_IMG_Y_BINS];
    for y in 0..num_lines.min(SYNC_IMG_Y_BINS) {
        for x in 0..line_width.min(X_ACC_BINS) {
            let t = ((y as f64) + (x as f64) / (line_width as f64)) * spec.line_seconds;
            let idx = probe_index(t);
            if idx < has_sync.len() {
                sync_img[sync_img_idx(x, y)] = has_sync[idx];
            }
        }
    }

    // Linear Hough transform.
    let mut lines = vec![0u16; LINES_D_BINS * n_slant_bins];
    let mut q_most = 0_usize;
    let mut max_count = 0_u16;
    for cy in 0..num_lines.min(SYNC_IMG_Y_BINS) {
        for cx in 0..line_width.min(X_ACC_BINS) {
            if !sync_img[sync_img_idx(cx, cy)] {
                continue;
            }
            for q in 0..n_slant_bins {
                let theta = deg2rad(MIN_SLANT_DEG + (q as f64) * SLANT_STEP_DEG);
                let d_signed = (line_width as f64)
                    + (-(cx as f64) * theta.sin() + (cy as f64) * theta.cos()).round();
                if d_signed > 0.0 && d_signed < (line_width as f64) {
                    let d = d_signed as usize;
                    if d < LINES_D_BINS {
                        let cell = &mut lines[lines_idx(d, q)];
                        *cell = cell.saturating_add(1);
                        if *cell > max_count {
                            max_count = *cell;
                            q_most = q;
                        }
                    }
                }
            }
        }
    }

    if max_count == 0 {
        return None;
    }

    let slant_angle = MIN_SLANT_DEG + (q_most as f64) * SLANT_STEP_DEG;
    let adjusted_rate =
        rate_hz + (deg2rad(90.0 - slant_angle).tan() / (line_width as f64)) * rate_hz;
    Some((slant_angle, adjusted_rate))
}

/// Pure 8-tap falling-edge convolution + slip-wrap. Returns `xmax`
/// already adjusted for the `X_ACC_SLIP_THRESHOLD` right-edge slip.
///
/// **A6 fix (#88):** The loop iterates exactly
/// `X_ACC_BINS - SYNC_EDGE_KERNEL_LEN` times — matching slowrx C's
/// `for (n=0; n<X_ACC_BINS-8; n++)` (692 iterations). Rust's native
/// `Iterator::windows(8)` yields 693 windows over a 700-element
/// slice (indices 0..=692); the `.take(X_ACC_BINS - SYNC_EDGE_KERNEL_LEN)`
/// caps it at 692 (indices 0..=691), so `xAcc[699]` is never read.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn falling_edge_from_x_acc(x_acc: &[u32]) -> i32 {
    debug_assert_eq!(x_acc.len(), X_ACC_BINS, "x_acc must be X_ACC_BINS long");
    let mut xmax: i32 = 0;
    let mut max_convd: i32 = 0;
    for (x, window) in x_acc
        .windows(SYNC_EDGE_KERNEL_LEN)
        .take(X_ACC_BINS - SYNC_EDGE_KERNEL_LEN)
        .enumerate()
    {
        let convd: i32 = window
            .iter()
            .zip(SYNC_EDGE_KERNEL.iter())
            .map(|(&v, &k)| (v as i32) * k)
            .sum();
        if convd > max_convd {
            max_convd = convd;
            xmax = (x as i32) + (SYNC_EDGE_KERNEL_LEN as i32) / 2;
        }
    }

    // sync.c:117 — pulse near the right edge slipped from previous left.
    if xmax > X_ACC_SLIP_THRESHOLD {
        xmax -= X_ACC_SLIP_THRESHOLD;
    }

    xmax
}

/// Column-accumulate `has_sync` into `X_ACC_BINS` bins at `rate_hz`,
/// then convolve with `SYNC_EDGE_KERNEL` to find the steepest
/// falling edge. Returns the `xmax` integer with the
/// `X_ACC_SLIP_THRESHOLD` right-edge slip-wrap already applied.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn find_falling_edge(has_sync: &[bool], rate_hz: f64, spec: ModeSpec, num_lines: usize) -> i32 {
    let probe_index = |t: f64| -> usize {
        let raw = t * rate_hz / (SYNC_PROBE_STRIDE as f64);
        if raw < 0.0 {
            0
        } else {
            raw as usize
        }
    };

    let mut x_acc = vec![0u32; X_ACC_BINS];
    for y in 0..num_lines {
        for (x, slot) in x_acc.iter_mut().enumerate() {
            let t = (y as f64) * spec.line_seconds
                + ((x as f64) / (X_ACC_BINS as f64)) * spec.line_seconds;
            let idx = probe_index(t);
            if idx < has_sync.len() && has_sync[idx] {
                *slot = slot.saturating_add(1);
            }
        }
    }

    falling_edge_from_x_acc(&x_acc)
}

/// Convert a falling-edge `xmax` (post-slip-wrap) to skip seconds,
/// applying the mode's sync-position offset. Pure arithmetic — no
/// global state. The raw `s_secs` is computed assuming the falling
/// edge lands at `(xmax / X_ACC_BINS) × line_seconds` and the sync
/// pulse runs `sync_seconds` long; `ModeSpec::skip_correction_seconds()`
/// then hoists the result for mid-line-sync modes (Scottie).
#[allow(clippy::cast_precision_loss)]
fn skip_seconds_for(xmax: i32, spec: ModeSpec) -> f64 {
    let raw = (f64::from(xmax) / (X_ACC_BINS as f64)) * spec.line_seconds - spec.sync_seconds;
    raw + spec.skip_correction_seconds()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;
    use crate::modespec;
    use crate::resample::WORKING_SAMPLE_RATE_HZ;
    use std::f64::consts::PI;

    fn synth_tone(freq_hz: f64, secs: f64) -> Vec<f32> {
        let n = (secs * f64::from(WORKING_SAMPLE_RATE_HZ)).round() as usize;
        (0..n)
            .map(|i| {
                let t = (i as f64) / f64::from(WORKING_SAMPLE_RATE_HZ);
                (2.0 * PI * freq_hz * t).sin() as f32
            })
            .collect()
    }

    #[test]
    fn has_sync_at_detects_1200_hz_burst() {
        let mut tracker = SyncTracker::new(0.0);
        let audio = synth_tone(1200.0, 0.050);
        assert!(tracker.has_sync_at(&audio, audio.len() / 2));
    }

    #[test]
    fn has_sync_at_rejects_1900_hz_tone() {
        let mut tracker = SyncTracker::new(0.0);
        let audio = synth_tone(1900.0, 0.050);
        assert!(!tracker.has_sync_at(&audio, audio.len() / 2));
    }

    #[test]
    fn has_sync_at_rejects_silence() {
        let mut tracker = SyncTracker::new(0.0);
        assert!(!tracker.has_sync_at(&vec![0.0_f32; 1024], 512));
    }

    /// The defect [`SyncPulseGate`] exists because of, pinned so it cannot
    /// come back as a "harmless" simplification: `has_sync_at` is a ratio
    /// and its verdict does not depend on there being a signal. Band noise
    /// sets it true about a quarter of the time, and *identically* at
    /// −120 dBFS and at −6 dBFS — which is why an end-of-transmission
    /// trigger built on it could only ever fire against digital silence.
    #[test]
    fn has_sync_at_answers_the_same_on_noise_at_every_level() {
        let n = 200_000_usize;
        let mut rates = Vec::new();
        for &rms in &[1e-6_f32, 1e-1, 0.5] {
            let audio = band_noise(n, rms, 0xACE1);
            let mut tracker = SyncTracker::new(0.0);
            let mut hits = 0_usize;
            let mut probes = 0_usize;
            let mut i = 0_usize;
            while i + SYNC_PROBE_STRIDE * 2 <= audio.len() {
                if tracker.has_sync_at(&audio, i + SYNC_PROBE_STRIDE / 2) {
                    hits += 1;
                }
                probes += 1;
                i += SYNC_PROBE_STRIDE;
            }
            rates.push(hits as f64 / probes as f64);
        }
        assert!(
            rates[0] > 0.15,
            "band noise should satisfy the ratio often, got {:.3}",
            rates[0]
        );
        for r in &rates[1..] {
            assert!(
                (r - rates[0]).abs() < 1e-9,
                "the ratio is scale-free: {rates:?}"
            );
        }
    }

    /// SSB-passband-shaped noise at a stated RMS.
    fn band_noise(n: usize, rms: f32, seed: u32) -> Vec<f32> {
        let mut state = seed;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state as f32 / u32::MAX as f32) - 0.5
        };
        let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
        let lowpass = (-2.0 * PI * 2700.0 / sr).exp() as f32;
        let highpass = (-2.0 * PI * 300.0 / sr).exp() as f32;
        let (mut lp, mut hp) = (0.0_f32, 0.0_f32);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let w = next();
            lp = lowpass * lp + (1.0 - lowpass) * w;
            hp = highpass * hp + (1.0 - highpass) * lp;
            out.push(lp - hp);
        }
        let cur = (out
            .iter()
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum::<f64>()
            / out.len() as f64)
            .sqrt() as f32;
        let g = if cur > 0.0 { rms / cur } else { 0.0 };
        out.iter().map(|s| s * g).collect()
    }

    /// A 1200 Hz burst of `secs` planted `at` samples into silence.
    fn planted_pulse(total: usize, at: usize, secs: f64, hz: f64) -> Vec<f32> {
        let sr = f64::from(WORKING_SAMPLE_RATE_HZ);
        let len = (secs * sr) as usize;
        let mut audio = vec![0.0_f32; total];
        for i in 0..len {
            audio[at + i] = (0.5 * (2.0 * PI * hz * ((at + i) as f64) / sr).sin()) as f32;
        }
        audio
    }

    #[test]
    fn sync_pulse_gate_finds_a_real_pulse_where_it_is() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Scottie1);
        let audio = planted_pulse(4096, 1500, spec.sync_seconds, 1200.0);
        let mut gate = SyncPulseGate::new(spec, 0.0);
        gate.advance(&audio);
        let found = gate.last_pulse_sample().expect("the pulse must be found");
        // Windows that overlap the pulse by more than `SYNC_TONE_PURITY_MIN`
        // of their length also qualify, so the reported position is the
        // last such window and can trail the true start by up to
        // `(1 - threshold) × sync` — 3.4 ms of a 428 ms Scottie line, which
        // neither the frame count nor the end-of-transmission gap can see.
        let win = (spec.sync_seconds * f64::from(WORKING_SAMPLE_RATE_HZ)) as i64;
        assert!(
            (found as i64 - 1500).abs() <= win / 2,
            "pulse at 1500, gate reported {found} (window {win})"
        );
        assert_eq!(gate.analysed_samples(), audio.len());
    }

    /// The whole point: a level cannot buy a verdict. Band noise from
    /// −120 dBFS to −6 dBFS produces no pulse at any of them.
    #[test]
    fn sync_pulse_gate_refuses_noise_at_every_level() {
        for spec in [
            modespec::for_mode(crate::modespec::SstvMode::Martin1),
            modespec::for_mode(crate::modespec::SstvMode::Scottie1),
            modespec::for_mode(crate::modespec::SstvMode::Pd120),
        ] {
            for &rms in &[1e-6_f32, 1e-3, 1e-2, 1e-1, 0.5] {
                let audio = band_noise(200_000, rms, 0xACE1);
                let mut gate = SyncPulseGate::new(spec, 0.0);
                gate.advance(&audio);
                assert!(
                    gate.last_pulse_sample().is_none(),
                    "{} claimed a sync pulse in band noise at {rms} RMS",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn sync_pulse_gate_refuses_silence_and_a_video_tone() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Scottie1);
        let mut gate = SyncPulseGate::new(spec, 0.0);
        gate.advance(&vec![0.0_f32; 4096]);
        assert!(gate.last_pulse_sample().is_none(), "silence is not a pulse");

        // A 1900 Hz video-band carrier is a perfectly coherent tone — but
        // not at the sync frequency, which is what the gate correlates on.
        let mut gate = SyncPulseGate::new(spec, 0.0);
        gate.advance(&planted_pulse(4096, 0, 0.3, 1900.0));
        assert!(
            gate.last_pulse_sample().is_none(),
            "a 1900 Hz carrier is not a sync pulse"
        );
    }

    /// A mistuned radio moves its sync pulse with everything else, so the
    /// gate must listen where the rest of the decode is listening.
    #[test]
    fn sync_pulse_gate_follows_the_mistuning_offset() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Scottie1);
        let audio = planted_pulse(4096, 1500, spec.sync_seconds, 1350.0);
        let mut tuned = SyncPulseGate::new(spec, 150.0);
        tuned.advance(&audio);
        assert!(
            tuned.last_pulse_sample().is_some(),
            "a +150 Hz-mistuned pulse must be found when the gate is told so"
        );
        let mut untuned = SyncPulseGate::new(spec, 0.0);
        untuned.advance(&audio);
        assert!(
            untuned.last_pulse_sample().is_none(),
            "…and the gate is genuinely frequency-selective, not a level test"
        );
    }

    /// Fed in arbitrary chunks (as live capture delivers), the gate must
    /// reach the same verdict as one buffer — the running sums and the
    /// phase rotator carry across calls.
    #[test]
    fn sync_pulse_gate_is_chunk_size_invariant() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Martin1);
        let audio = planted_pulse(60_000, 41_000, spec.sync_seconds, 1200.0);
        let mut whole = SyncPulseGate::new(spec, 0.0);
        whole.advance(&audio);
        for chunk in [1_usize, 7, 1024] {
            let mut streamed = SyncPulseGate::new(spec, 0.0);
            let mut fed = 0_usize;
            while fed < audio.len() {
                fed = (fed + chunk).min(audio.len());
                streamed.advance(&audio[..fed]);
            }
            assert_eq!(
                streamed.last_pulse_sample(),
                whole.last_pulse_sample(),
                "chunk size {chunk} changed the verdict"
            );
        }
    }

    /// Build a synthetic `has_sync` track with a sync pulse at every line start.
    fn synth_has_sync(spec: ModeSpec, rate_hz: f64) -> Vec<bool> {
        let total = (f64::from(spec.image_lines) * spec.line_seconds * rate_hz
            / (SYNC_PROBE_STRIDE as f64)) as usize
            + 16;
        let mut track = vec![false; total];
        for y in 0..spec.image_lines {
            let i_start =
                (f64::from(y) * spec.line_seconds * rate_hz / (SYNC_PROBE_STRIDE as f64)) as usize;
            let i_end = ((f64::from(y) * spec.line_seconds + spec.sync_seconds) * rate_hz
                / (SYNC_PROBE_STRIDE as f64)) as usize;
            for slot in track.iter_mut().take(i_end.min(total)).skip(i_start) {
                *slot = true;
            }
        }
        track
    }

    /// Build a synthetic `has_sync` track where the signal was *captured*
    /// at `capture_rate_hz` but the true line cadence runs at
    /// `true_rate_hz`. Each captured line is `true_rate / capture_rate`
    /// of a real line, so sync pulses drift through the (probe-stride-
    /// quantized) track — i.e. the slant is non-90°.
    fn synth_has_sync_slanted(
        spec: ModeSpec,
        true_rate_hz: f64,
        // Documents the intended `find_sync(track, capture_rate, spec)`
        // call site; not used in synthesis (pulses are placed at
        // true-rate cadence — find_sync interprets the buffer at
        // capture_rate_hz, so the y-linear position difference between
        // expected and actual indices is the slant). Underscored to
        // suppress the unused-arg warning; keep the parameter as
        // self-documenting API.
        _capture_rate_hz: f64,
    ) -> Vec<bool> {
        let total = (f64::from(spec.image_lines) * spec.line_seconds * true_rate_hz
            / (SYNC_PROBE_STRIDE as f64)) as usize
            + 16;
        let mut track = vec![false; total];
        for y in 0..spec.image_lines {
            let i_start = (f64::from(y) * spec.line_seconds * true_rate_hz
                / (SYNC_PROBE_STRIDE as f64)) as usize;
            let i_end =
                i_start + (spec.sync_seconds * true_rate_hz / (SYNC_PROBE_STRIDE as f64)) as usize;
            for slot in track.iter_mut().take(i_end.min(total)).skip(i_start) {
                *slot = true;
            }
        }
        track
    }

    #[test]
    fn find_sync_locks_clean_track_to_90_degrees() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Pd120);
        let rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let r = find_sync(&synth_has_sync(spec, rate), rate, spec);
        let slant = r.slant_deg.expect("sync detected");
        assert!((slant - 90.0).abs() < 1.0, "{slant:.2}°");
        assert!((r.adjusted_rate_hz - rate).abs() / rate < 0.005);
        assert!(r.skip_samples.abs() < (0.05 * rate) as i64);
    }

    /// With all-zero `has_sync`, the Hough transform finds nothing.
    /// `slant_deg` must be `None` (not `Some(90.0)`) and `skip_samples`
    /// must encode a negative offset (xmax=0, no sync detected).
    /// Verifies round-2 audit Finding 6 (xmax=0 on zero input) and
    /// Finding 10 (`slant_deg` is None, not the misleading 90.0 default).
    #[test]
    fn find_sync_empty_track_has_no_slant_detected() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Pd120);
        let rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let r = find_sync(&vec![false; 16384], rate, spec);
        assert!(
            r.slant_deg.is_none(),
            "empty track should yield slant_deg=None, got {:?}",
            r.slant_deg
        );
        // xmax=0 → s_secs = 0 - sync_seconds → skip is negative.
        assert!(
            r.skip_samples < 0,
            "empty track skip should be negative (xmax=0)"
        );
    }

    #[test]
    fn find_sync_recovers_known_offset() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Pd120);
        let rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        // Right-shift the track by ~10 ms (a real-radio settling gap).
        let mut track = synth_has_sync(spec, rate);
        let shift = ((0.010 * rate) / (SYNC_PROBE_STRIDE as f64)) as usize;
        let mut shifted = vec![false; shift];
        shifted.append(&mut track);
        let r = find_sync(&shifted, rate, spec);
        let expected = (0.010 * rate) as i64;
        // 700-bin row ≈ 0.7 ms / bin at PD120; allow a few bins for wobble.
        assert!(
            (r.skip_samples - expected).abs() < (0.005 * rate) as i64,
            "Skip off (expected ≈ {expected}, got {})",
            r.skip_samples
        );
    }

    #[test]
    fn find_sync_handles_empty_track() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Pd120);
        let rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let r = find_sync(&vec![false; 16384], rate, spec);
        assert!(r.adjusted_rate_hz.is_finite());
        assert!((r.adjusted_rate_hz - rate).abs() < 1.0);
        // Rate must be bit-exact when no sync detected (no rate correction ran).
        assert!(
            (r.adjusted_rate_hz - rate).abs() < f64::EPSILON,
            "rate should be unchanged, got {}",
            r.adjusted_rate_hz
        );
    }

    /// F2 (#88). Hough slant correction path — 0.5% capture-rate drift
    /// produces a slant well outside the (89°, 91°) lock window
    /// (`tan(90° − slant)/line_width ≈ 0.005` implies a Hough peak
    /// near 63° or 117° depending on which symmetry the dominant
    /// line in `sync_img` lands in). The retry loop must shrink the
    /// rate error toward zero; we assert it ends up under half the
    /// initial drift.
    /// (0.3% drift falls *inside* the 0.5°-quantized Hough bins as
    /// near-90°, hits the 0.5° deadband, and never triggers
    /// correction; 0.5% is the minimum drift that reliably runs the
    /// correction path.)
    #[test]
    fn find_sync_corrects_0p5pct_slant_at_pd120() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Pd120);
        let true_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let capture_rate = true_rate * 1.005;
        let track = synth_has_sync_slanted(spec, true_rate, capture_rate);
        let r = find_sync(&track, capture_rate, spec);
        let err_pct = (r.adjusted_rate_hz - true_rate).abs() / true_rate * 100.0;
        let initial_err_pct = (capture_rate - true_rate).abs() / true_rate * 100.0;
        // Verify sync was detected at all.
        assert!(
            r.slant_deg.is_some(),
            "expected sync to be detected (slant_deg should be Some)"
        );
        // Rate should have moved toward true_rate (correction was applied).
        assert!(
            r.adjusted_rate_hz < capture_rate,
            "adjusted rate {:.2} should be less than capture_rate {capture_rate:.2} (slant correction moved it toward true_rate)",
            r.adjusted_rate_hz
        );
        // Final error should be well under the initial drift.
        assert!(
            err_pct < initial_err_pct / 2.0,
            "rate err {err_pct:.3}% should be < half of initial {initial_err_pct:.3}%"
        );
    }

    /// F2 (#88). Larger drift (1% capture-rate offset) produces a
    /// Hough peak far outside the lock window — the retry loop must
    /// do real work to converge. Verify the retry loop actually
    /// progresses: the final rate error must be strictly smaller
    /// than the initial guess.
    #[test]
    fn find_sync_corrects_1pct_slant_via_retries() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Pd120);
        let true_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let capture_rate = true_rate * 1.01;
        let track = synth_has_sync_slanted(spec, true_rate, capture_rate);
        let r = find_sync(&track, capture_rate, spec);
        let initial_err_pct = (capture_rate - true_rate).abs() / true_rate * 100.0;
        let final_err_pct = (r.adjusted_rate_hz - true_rate).abs() / true_rate * 100.0;
        assert!(
            final_err_pct < initial_err_pct,
            "retry should shrink rate error: initial {initial_err_pct:.3}% → final {final_err_pct:.3}%"
        );
        assert!(
            final_err_pct < 0.2,
            "rate err after retries should be ≤ 0.2%, got {final_err_pct:.3}%"
        );
    }

    /// F3 (#88). Scottie modes use mid-line sync; the
    /// `skip_correction_seconds()` path on `ModeSpec` subtracts
    /// `chan_len/2 - 2*porch` from the raw `s_secs`. Feeding
    /// line-start pulses (the existing `synth_has_sync` helper)
    /// with a Scottie spec lands `xmax` near 0 (small), so
    /// `s_secs_raw ≈ 0` and the final skip should equal the
    /// correction itself (negative, ~ -65 ms for Scottie1 at
    /// 11025 Hz).
    #[test]
    fn find_sync_scottie_applies_skip_correction() {
        let spec = modespec::for_mode(crate::modespec::SstvMode::Scottie1);
        let rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let track = synth_has_sync(spec, rate);
        let r = find_sync(&track, rate, spec);

        let chan_len = f64::from(spec.line_pixels) * spec.pixel_seconds;
        let expected_secs = -chan_len / 2.0 + 2.0 * spec.porch_seconds;
        let expected_skip = (expected_secs * rate).round() as i64;
        let tolerance = (0.005 * rate) as i64; // ~55 samples ≈ 5 ms

        assert!(
            (r.skip_samples - expected_skip).abs() < tolerance,
            "Scottie skip {} should be ≈ {expected_skip} (correction = {expected_secs:.4}s, tol = {tolerance})",
            r.skip_samples
        );
        // Sanity: Scottie correction is always negative.
        assert!(
            r.skip_samples < 0,
            "Scottie skip should be negative (mid-line hoist); got {}",
            r.skip_samples
        );
    }

    /// A6 regression guard (#88). slowrx C's loop runs `n ∈ 0..691`
    /// (`for (n=0; n<X_ACC_BINS-8; n++)`), so `xAcc[699]` is never
    /// read. Rust's native `windows(8)` over a 700-bin slice yields
    /// 693 windows (`n ∈ 0..=692`); without `.take(X_ACC_BINS - 8)`
    /// the kernel would read `xAcc[699]` at `n=692`. This test
    /// constructs an `x_acc` whose strongest convd at n=692 differs
    /// from the strongest at n=691: pre-fix lands at n=692 (xmax=696
    /// → slip-wrap → 346); post-fix lands at n=691 (xmax=695 →
    /// slip-wrap → 345). The assertion fails on the pre-fix code and
    /// passes on the post-fix code.
    #[test]
    fn falling_edge_from_x_acc_off_by_one_a6() {
        let mut x_acc = vec![0u32; X_ACC_BINS];
        // x_acc[691..=695] = 100, x_acc[696..=699] = 0.
        for slot in x_acc.iter_mut().take(696).skip(691) {
            *slot = 100;
        }
        // n=691: window = [100,100,100,100,100,0,0,0]
        //   convd = 4*100 - 100 - 0 - 0 - 0 = 300.
        // n=692: window = [100,100,100,100,0,0,0,0]
        //   convd = 4*100 - 0 = 400  (pre-fix only).
        // Pre-fix max at n=692 → xmax = 696 → slip-wrap → 346.
        // Post-fix max at n=691 → xmax = 695 → slip-wrap → 345.
        let xmax = falling_edge_from_x_acc(&x_acc);
        assert_eq!(
            xmax, 345,
            "post-A6-fix should pick n=691 (xmax=695, slip=345); pre-fix would give 346"
        );
    }

    /// A6 baseline: an edge well away from the right edge produces
    /// the same `xmax` pre-fix and post-fix. Sanity check that the
    /// `.take(...)` bound only changes behavior at the very right
    /// edge, not anywhere else.
    #[test]
    fn falling_edge_from_x_acc_detects_mid_array_edge() {
        let mut x_acc = vec![0u32; X_ACC_BINS];
        // Edge at indices 100..=103.
        for slot in x_acc.iter_mut().take(104).skip(100) {
            *slot = 100;
        }
        // n=100: window = [100,100,100,100,0,0,0,0], convd = 400,
        // xmax = 100 + 4 = 104. 104 < X_ACC_SLIP_THRESHOLD (350), no
        // slip-wrap. Pre-fix and post-fix agree.
        let xmax = falling_edge_from_x_acc(&x_acc);
        assert_eq!(xmax, 104, "mid-array edge detection unchanged by A6 fix");
    }
}
