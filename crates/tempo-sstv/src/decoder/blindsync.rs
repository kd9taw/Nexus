//! Blind (no-VIS) sync acquisition — mid-picture tune-in.
//!
//! The VIS header is what normally starts a decode: it names the mode,
//! fixes the timing origin (image row 0), and supplies `hedr_shift_hz`.
//! An operator who tunes across 14.230 with a picture already in flight
//! never hears it, and [`crate::decoder::SstvDecoder`] would sit in
//! `AwaitingVis` forever. Every other SSTV program shows that operator
//! the part of the picture that is still on the air; this module is how
//! Nexus does.
//!
//! **What replaces VIS.** Sync pulses. Every mode emits a 1200 Hz pulse
//! once per radio line, and the `(line period, sync length)` pair is a
//! near-unique fingerprint in the mode table (see `separability` in the
//! tests below, which asserts it against [`ALL_SPECS`] rather than
//! trusting a comment). We reuse [`SyncTracker`] — the same 1200 Hz
//! probe the VIS-anchored decode already runs — extract pulses from the
//! boolean track, fit a grid, and match the fitted period + pulse width
//! against the table. `hedr_shift_hz` is then recovered from the pulses
//! themselves, which are a known 1200 Hz reference.
//!
//! **Prior art.** MMSSTV measures "the interval time of the
//! synchronization pulses" to auto-start without VIS (its `VIS only`
//! option exists to turn that *off*); QSSTV's receive tab likewise
//! auto-detects the mode from line timing when "use VIS" is unchecked.
//! Both also ship a manual mode pick, which is the operator override
//! this module reports the need for but does not build.
//!
//! **The thing this module is mostly made of is refusal.**
//! `tests/no_vis.rs` exists because an ISS Zarya recording carrying no
//! SSTV at all had to be proven not to produce images, and relaxing the
//! VIS requirement is exactly how a decoder starts hallucinating
//! pictures out of noise. An operator who gets garbage from an empty
//! band trusts the feature less than one who occasionally misses a
//! picture, so every gate below is a veto and they are `AND`ed:
//!
//! 1. **Run length** — a candidate pulse is ≥ [`MIN_RUN_PROBES`]
//!    consecutive `true` probes. White noise makes the probe true ~22 %
//!    of the time; this drops the speckle.
//! 2. **Complete grid** — [`LOCK_PULSES`] pulses must fill *every* slot
//!    of `anchor + k·period`, no gaps. Noise has plenty of pulses and
//!    never six on a grid; this is the gate that does most of the work.
//! 3. **Fitted period in a mode bin** — least-squares slope within
//!    [`PERIOD_TOL_FRAC`] of a table entry. Never snap to nearest: a
//!    period matching nothing is a reject.
//! 4. **Pulse width matches that mode's sync length** — an independent
//!    axis (4.862 / 9 / 20 ms), which is what separates the tightest
//!    pair in the table (Scottie 1 vs Martin 1, 4.2 % apart in period
//!    but 9 vs 4.862 ms in sync).
//! 5. **No shorter fundamental** — if the half- or third-period slots
//!    are also populated, the candidate is a harmonic alias and the
//!    true period is shorter. This gate is not optional and it is not
//!    what a "are all predicted pulses present?" duty check does: a
//!    Robot 36 train contains *every* Robot 72 pulse (2 × 150 ms =
//!    300 ms exactly), and PD-120 doubles to 1.017 s against PD-240's
//!    1.000 s — inside the period tolerance. Both would lock as the
//!    wrong mode without this.
//! 6. **Video-band occupancy between pulses** — an SSTV line is a
//!    narrowband FM sweep in 1500-2300 Hz. This is what rejects "there
//!    is a pulse train but no picture" when the "no picture" is
//!    *broadband*: SSB speech, a CW pileup keying near 1200 Hz, or noise
//!    that happened to look periodic.
//! 7. **Video-band modulation between pulses** — occupancy asks whether
//!    the band is *busy*, and an unmodulated carrier sitting in it
//!    answers yes. That is not a hypothetical: pulses at a mode's exact
//!    line period with a steady 1900 Hz carrier in the gaps produced a
//!    lock and a picture on 13 of the 13 blind-eligible modes before this
//!    gate existed. A picture *modulates* the video band and a carrier
//!    does not, so this gate measures variation, not presence. See
//!    [`BlindSync::gaps_are_modulated`].
//!
//! **Robot 24 and Robot 36 are deliberately excluded** from inference —
//! see [`blind_eligible`]. Their `ModeSpec`s are byte-identical in every
//! timing field, so they cannot be told apart by timing even in
//! principle; that part is harmless (the two decode identically). What
//! is not harmless is that `mode_robot::decode_r36_or_r24_line` derives
//! Cr-vs-Cb from `line_index % 2`, and a mid-picture start does not know
//! the absolute row parity. Getting it wrong swaps chroma for the whole
//! picture, which looks like a fault in the operator's radio. Real
//! Robot 36 marks the parity with a 1500/2300 Hz separator tone, but our
//! own encoder emits 1500 Hz for both parities (`tone.rs` has a single
//! `SEPTR_HZ`), so that fix cannot be written or tested here without an
//! off-air recording. Until then a mid-start Robot 36 produces no image
//! rather than a wrong-coloured one.

use crate::modespec::{ModeSpec, SstvMode, ALL_SPECS};
use crate::resample::WORKING_SAMPLE_RATE_HZ;
use crate::sync::{SyncTracker, SYNC_PROBE_STRIDE};

/// Minimum consecutive `true` probes for a run to count as a candidate
/// sync pulse. One probe is `SYNC_PROBE_STRIDE` (4) working-rate samples
/// ≈ 0.363 ms, so 3 probes ≈ 1.09 ms — well under Martin's 4.862 ms sync
/// (the shortest in the table) and well over the run length white noise
/// produces by chance.
const MIN_RUN_PROBES: usize = 3;

/// How many pulses must fill a candidate grid before a lock is declared.
/// Six is 6× the longest run any (anchor, mode) hypothesis achieves on
/// white noise, while still locking within 1-3 % of a transmission's
/// airtime (Robot 72 ≈ 1.5 s, Scottie 1 ≈ 2.6 s, Scottie DX ≈ 6.3 s).
const LOCK_PULSES: usize = 6;

/// Phase tolerance for a pulse to count as filling a grid slot, as a
/// fraction of the candidate mode's line period.
const PHASE_TOL_FRAC: f64 = 0.05;

/// Tolerance on the least-squares-fitted line period against the mode
/// table. Under half the tightest gap between two eligible modes
/// (Scottie 1 → Martin 1, 4.2 %) and far above any real transmitter
/// clock error (< 0.1 %).
const PERIOD_TOL_FRAC: f64 = 0.02;

/// Tolerance on the median pulse width against the mode's
/// `sync_seconds`. Generous because the 16-sample probe window smears
/// the measured run length by ~1.5 ms either way; the three sync classes
/// (4.862 / 9 / 20 ms) stay separable regardless.
const WIDTH_TOL_FRAC: f64 = 0.40;

/// Minimum share of 300-3300 Hz energy that must sit in the 1450-2350 Hz
/// video band, in every gap between locked pulses.
const OCCUPANCY_MIN: f64 = 0.70;

/// Points at which the video band is demodulated across each gap between
/// locked pulses, to measure how much it moves.
const LUMA_SAMPLES_PER_GAP: usize = 16;

/// Working-rate samples kept clear of each sync pulse before the video
/// band is sampled. The demodulator's own window must never straddle a
/// 1200 Hz pulse: the peak search is confined to the video band, so a
/// straddling window clips to the band edge and reads as a huge luminance
/// excursion — which is variation the *pulse* produced, not the picture.
const PULSE_CLEARANCE_SAMPLES: usize = 64;

/// Hann-window index handed to [`crate::demod::ChannelDemod::pixel_freq`]
/// — `HANN_LENS[4]` = 64 samples ≈ 5.8 ms. Short enough to follow a scan
/// line, long enough to place a tone inside the 800 Hz video band.
const LUMA_WINDOW_IDX: usize = 4;

/// Minimum median per-gap standard deviation of the demodulated video-band
/// frequency, in Hz, before the gaps count as carrying a picture.
///
/// 30 Hz is 9.6 grey levels of the 255-level luminance ramp
/// (`(2300-1500)/255 = 3.137 Hz` per level), i.e. **a picture whose
/// luminance varies by under about ten levels RMS along a line is refused
/// as unmodulated.** Measured 2026-08-06 with
/// [`crate::demod::ChannelDemod::pixel_freq`] sampling
/// [`LUMA_SAMPLES_PER_GAP`] points per gap:
///
/// | input | median per-gap σ |
/// |---|---|
/// | steady 1900 Hz carrier between pulses, clean | 0.01 Hz |
/// | …with AWGN at 20 / 10 / 6 / 0 dB SNR | 1.8 / 5.7 / 9.0 / 17.9 Hz |
/// | Scottie 1 luminance ramp spanning 24 levels | 31 Hz |
/// | the test corpus' pictures (Scottie/Martin/Robot/PD) | 147-271 Hz |
///
/// **What it rejects, stated plainly:** any signal whose video band does
/// not move — a tuning carrier, a stuck carrier with periodic QRM, a data
/// mode whose framing lands on a line period. **What it also rejects, and
/// this is the cost:** a genuine picture that is very nearly a flat field.
/// A dead-uniform PD frame measures 0.01 Hz, which is to say it is
/// *identical* to a carrier in this band — there is no signal in the audio
/// that separates them, and refusing it forfeits an image that carries no
/// information anyway. Modes with channel separators (Scottie, Martin,
/// Robot) clear the threshold even dead-flat, because the separators
/// themselves move the band.
const VIDEO_MODULATION_MIN_HZ: f64 = 30.0;

/// Sub-multiples of a candidate period probed for a shorter fundamental.
const SUBHARMONIC_DIVISORS: [usize; 2] = [2, 3];

/// Reject a candidate when at least this share of a sub-multiple's extra
/// slots also carry pulses (i.e. the real period is shorter).
const SUBHARMONIC_REJECT_FRAC: f64 = 0.5;

/// Samples analysed per inter-pulse occupancy window (~93 ms at the
/// working rate). Shorter than the smallest inter-pulse gap in the
/// table (Robot 24/36: 150 − 9 = 141 ms).
const OCCUPANCY_WINDOW_SAMPLES: usize = 1024;

/// Working-rate audio retained while searching, in seconds. Must hold
/// `LOCK_PULSES` line periods of the slowest eligible mode (Scottie DX,
/// 1.0503 s → 5 periods ≈ 5.3 s) plus slack, and bounds the memory a
/// permanently-armed receiver can hold at ~350 kB.
const RETAIN_SECONDS: f64 = 8.0;

/// New working-rate audio required between lock attempts (~0.25 s). The
/// grid search is O(pulses × modes × slots); throttling keeps a
/// noise-fed decoder cheap without materially delaying a lock.
const ATTEMPT_STRIDE_SAMPLES: usize = 2756;

/// Frequency step of the Goertzel bank used for occupancy and for the
/// `hedr_shift_hz` refinement, in Hz.
const BANK_STEP_HZ: f64 = 25.0;

/// Half-width of the `hedr_shift_hz` search around 1200 Hz, in Hz.
const HEDR_SEARCH_HZ: f64 = 250.0;

/// Step of the `hedr_shift_hz` refinement scan, in Hz.
const HEDR_STEP_HZ: f64 = 2.0;

/// Samples trimmed from each end of a pulse before its interior is used
/// for the `hedr_shift_hz` estimate, so the probe window's smear and the
/// transmitter's tone transition do not bias the frequency.
const PULSE_EDGE_TRIM_SAMPLES: usize = 8;

/// Is this mode a candidate for blind inference?
///
/// Robot 24 and Robot 36 are excluded: their timing is byte-identical
/// (so inference cannot name which of the two it is) and, decisively,
/// their Cr/Cb assignment comes from absolute row parity
/// (`mode_robot.rs`, `line_index % 2`), which a mid-picture start cannot
/// know. See the module header.
#[must_use]
pub(crate) fn blind_eligible(spec: ModeSpec) -> bool {
    !matches!(spec.mode, SstvMode::Robot24 | SstvMode::Robot36)
}

/// A run of consecutive `true` probes — one candidate sync pulse.
#[derive(Clone, Copy, Debug)]
struct Pulse {
    /// Index of the first `true` probe.
    start_probe: usize,
    /// Number of consecutive `true` probes.
    len_probes: usize,
}

/// A successful blind lock: everything the decoder needs to start
/// pixel decode without a VIS header.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlindLock {
    /// The inferred mode's spec.
    pub spec: ModeSpec,
    /// Index into the retained audio where the inferred radio line 0 of
    /// *this capture* begins. Chosen so the decoder's audio buffer
    /// starts on a line boundary, which is the same contract the
    /// VIS path gives it (`find_sync` and every per-line decoder assume
    /// line-start-relative timing).
    pub line_start_sample: usize,
    /// Radio mistuning offset in Hz, measured from the locked pulses
    /// against their nominal 1200 Hz.
    pub hedr_shift_hz: f64,
    /// Least-squares-fitted line period, seconds. Diagnostic — the
    /// decode uses the table value plus `find_sync`'s slant correction.
    pub period_seconds: f64,
}

/// Rolling search for a sync-pulse grid in audio that carried no VIS.
///
/// Fed the same working-rate audio the VIS detector sees while the
/// decoder is in `AwaitingVis`. Holds a bounded ring so a lock can
/// reach *back* to the first pulse of the grid it locked onto — those
/// lines belong in the picture, not spent on acquisition.
pub(crate) struct BlindSync {
    tracker: SyncTracker,
    /// Retained working-rate audio, trimmed to [`RETAIN_SECONDS`].
    audio: Vec<f32>,
    /// One entry per [`SYNC_PROBE_STRIDE`] samples of `audio`.
    has_sync: Vec<bool>,
    /// Next sample index in `audio` to probe (always a stride multiple).
    next_probe_sample: usize,
    /// New samples accumulated since the last lock attempt.
    samples_since_attempt: usize,
}

impl BlindSync {
    /// Construct a searcher probing at zero mistuning.
    ///
    /// The probe is deliberately built at `hedr_shift_hz = 0`: the
    /// 16-sample sync window is spectrally very broad, so it tolerates
    /// a few hundred Hz of mistuning, and the true offset is measured
    /// from the pulses once they are found ([`Self::refine_hedr_shift`]).
    /// Running a bank of pre-shifted probes would widen the search but
    /// also widen the false-lock surface, so it is not done here — see
    /// the limitation noted in the crate CHANGELOG.
    pub fn new() -> Self {
        Self {
            tracker: SyncTracker::new(0.0),
            audio: Vec::new(),
            has_sync: Vec::new(),
            next_probe_sample: 0,
            samples_since_attempt: 0,
        }
    }

    /// Drop all retained audio and probes. Called whenever the decoder
    /// leaves `AwaitingVis`, so the tail of a just-decoded image can
    /// never be re-locked as a fresh transmission.
    pub fn reset(&mut self) {
        self.audio.clear();
        self.has_sync.clear();
        self.next_probe_sample = 0;
        self.samples_since_attempt = 0;
    }

    /// Absorb newly resampled working-rate audio and probe the sync band
    /// over whatever became fully available.
    pub fn push(&mut self, audio: &[f32]) {
        if audio.is_empty() {
            return;
        }
        self.audio.extend_from_slice(audio);
        self.samples_since_attempt = self.samples_since_attempt.saturating_add(audio.len());

        // Same conservative availability rule as the decoding path: wait
        // until the buffer extends a full stride beyond the probe centre.
        while self.next_probe_sample + SYNC_PROBE_STRIDE * 2 <= self.audio.len() {
            let center = self.next_probe_sample + SYNC_PROBE_STRIDE / 2;
            let has = self.tracker.has_sync_at(&self.audio, center);
            self.has_sync.push(has);
            self.next_probe_sample += SYNC_PROBE_STRIDE;
        }

        self.trim();
    }

    /// Bound the ring at [`RETAIN_SECONDS`], dropping whole probe
    /// windows from the front so `has_sync[i]` keeps describing
    /// `audio[i * SYNC_PROBE_STRIDE ..]`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn trim(&mut self) {
        let cap = (RETAIN_SECONDS * f64::from(WORKING_SAMPLE_RATE_HZ)) as usize;
        if self.audio.len() <= cap {
            return;
        }
        // Drop a whole number of probe strides so the probe track and
        // the audio stay in lockstep.
        let excess = self.audio.len() - cap;
        let drop_samples = (excess / SYNC_PROBE_STRIDE) * SYNC_PROBE_STRIDE;
        if drop_samples == 0 {
            return;
        }
        self.audio.drain(..drop_samples);
        let drop_probes = (drop_samples / SYNC_PROBE_STRIDE).min(self.has_sync.len());
        self.has_sync.drain(..drop_probes);
        self.next_probe_sample = self.next_probe_sample.saturating_sub(drop_samples);
    }

    /// Hand the retained audio from `from` forward to the caller,
    /// leaving the searcher empty. Used on lock to seed the decoding
    /// buffer with the audio the lock was made of.
    pub fn take_audio_from(&mut self, from: usize) -> Vec<f32> {
        let out = if from < self.audio.len() {
            self.audio[from..].to_vec()
        } else {
            Vec::new()
        };
        self.reset();
        out
    }

    /// Attempt a lock. Returns `None` — the overwhelmingly common
    /// answer — unless every gate in the module header passes.
    ///
    /// `demod` is the decoder's own per-pixel demodulator, borrowed for
    /// gate 7 so the lock is judged by the same luminance estimator that
    /// would draw the picture (and so this module does not build a second
    /// 1024-point FFT plan for every armed receiver).
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn try_lock(&mut self, demod: &mut crate::demod::ChannelDemod) -> Option<BlindLock> {
        if self.samples_since_attempt < ATTEMPT_STRIDE_SAMPLES {
            return None;
        }
        self.samples_since_attempt = 0;

        let pulses = extract_pulses(&self.has_sync);
        if pulses.len() < LOCK_PULSES {
            return None;
        }

        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let probe_seconds = SYNC_PROBE_STRIDE as f64 / work_rate;

        // Gates 2-5 are cheap and run on the boolean track alone; the
        // occupancy gate touches audio and runs only on a survivor.
        for spec in ALL_SPECS.iter().copied().filter(|s| blind_eligible(*s)) {
            let period_probes = spec.line_seconds / probe_seconds;
            let phase_tol = period_probes * PHASE_TOL_FRAC;

            for anchor_idx in 0..pulses.len() {
                let Some(matched) =
                    match_grid(&pulses, anchor_idx, period_probes, phase_tol, LOCK_PULSES)
                else {
                    continue;
                };

                // Gate 3 — least-squares period must land in this bin.
                let centres: Vec<f64> = matched
                    .iter()
                    .map(|&i| pulses[i].start_probe as f64)
                    .collect();
                let Some((intercept, slope)) = least_squares_fit(&centres) else {
                    continue;
                };
                let fitted_seconds = slope * probe_seconds;
                if (fitted_seconds - spec.line_seconds).abs() / spec.line_seconds > PERIOD_TOL_FRAC
                {
                    continue;
                }
                // Residuals must be small too — a fit whose slope is
                // right but whose points scatter is not a grid.
                if centres
                    .iter()
                    .enumerate()
                    .any(|(k, &t)| (t - (intercept + slope * k as f64)).abs() > phase_tol)
                {
                    continue;
                }

                // Gate 4 — median pulse width against this mode's sync.
                let mut widths: Vec<f64> = matched
                    .iter()
                    .map(|&i| pulses[i].len_probes as f64 * probe_seconds)
                    .collect();
                widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let median_width = widths[widths.len() / 2];
                if (median_width - spec.sync_seconds).abs() > spec.sync_seconds * WIDTH_TOL_FRAC {
                    continue;
                }

                // Gate 5 — a shorter fundamental disqualifies this period.
                if has_shorter_fundamental(&pulses, intercept, slope, phase_tol, LOCK_PULSES) {
                    continue;
                }

                // Gate 6 — the gaps between pulses must carry energy in
                // the video band.
                if !self.gaps_carry_video(&centres, spec) {
                    continue;
                }

                let hedr =
                    self.refine_hedr_shift(&matched.iter().map(|&i| pulses[i]).collect::<Vec<_>>());

                // Gate 7 — and that energy must be a picture rather than a
                // carrier: the video band has to move.
                if !self.gaps_are_modulated(&centres, spec, hedr, demod) {
                    continue;
                }

                let first_pulse_sample = (centres[0] * SYNC_PROBE_STRIDE as f64) as usize;
                let line_start_sample = line_start_for(
                    spec,
                    first_pulse_sample,
                    period_probes * SYNC_PROBE_STRIDE as f64,
                );

                return Some(BlindLock {
                    spec,
                    line_start_sample,
                    hedr_shift_hz: hedr,
                    period_seconds: fitted_seconds,
                });
            }
        }
        None
    }

    /// Gate 6 — in every gap between locked pulses, the 1450-2350 Hz
    /// video band must dominate 300-3300 Hz. An SSTV scan line is a
    /// narrowband FM sweep inside that band; noise is broadband, speech
    /// sits far lower, and CW is a keyed carrier with silence between
    /// elements.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn gaps_carry_video(&self, centres: &[f64], spec: ModeSpec) -> bool {
        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        for pair in centres.windows(2) {
            // Centre the analysis window in the gap, clear of both pulses.
            let gap_start = (pair[0] * SYNC_PROBE_STRIDE as f64) + spec.sync_seconds * work_rate;
            let gap_end = pair[1] * SYNC_PROBE_STRIDE as f64;
            if gap_end <= gap_start {
                return false;
            }
            let mid = f64::midpoint(gap_start, gap_end);
            let half = (OCCUPANCY_WINDOW_SAMPLES as f64 / 2.0).min((gap_end - gap_start) / 2.0);
            let lo = (mid - half).max(0.0) as usize;
            let hi = ((mid + half) as usize).min(self.audio.len());
            if hi <= lo + 64 {
                return false;
            }
            if video_band_share(&self.audio[lo..hi]) < OCCUPANCY_MIN {
                return false;
            }
        }
        true
    }

    /// Gate 7 — the video band between the pulses must MOVE.
    ///
    /// [`Self::gaps_carry_video`] asks whether the band is *occupied*, and
    /// an unmodulated carrier occupies it perfectly — which is how a
    /// pulse train plus a steady 1900 Hz tone produced pictures on every
    /// blind-eligible mode. The question that actually separates a picture
    /// from a carrier is whether the luminance the decoder would write
    /// varies, so that is what this measures, with the decoder's own
    /// per-pixel demodulator: sample the video-band frequency at
    /// [`LUMA_SAMPLES_PER_GAP`] points across each gap, take the standard
    /// deviation per gap, and require the **median** across gaps to reach
    /// [`VIDEO_MODULATION_MIN_HZ`].
    ///
    /// The median rather than every gap: a real picture can hold a solid
    /// band across a few lines and one flat line should not veto a lock.
    /// A carrier is flat in *every* gap, so the median still catches it —
    /// as does anything unmodulated over half its lines or more.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    fn gaps_are_modulated(
        &self,
        centres: &[f64],
        spec: ModeSpec,
        hedr_shift_hz: f64,
        demod: &mut crate::demod::ChannelDemod,
    ) -> bool {
        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let mut sigmas: Vec<f64> = Vec::with_capacity(centres.len());
        for pair in centres.windows(2) {
            let pulse_end =
                (pair[0] * SYNC_PROBE_STRIDE as f64 + spec.sync_seconds * work_rate) as usize;
            let lo = pulse_end + PULSE_CLEARANCE_SAMPLES;
            let hi = ((pair[1] * SYNC_PROBE_STRIDE as f64) as usize)
                .saturating_sub(PULSE_CLEARANCE_SAMPLES);
            if hi <= lo + PULSE_CLEARANCE_SAMPLES || hi > self.audio.len() {
                return false;
            }
            sigmas.push(gap_luma_sigma(&self.audio, lo, hi, hedr_shift_hz, demod));
        }
        if sigmas.is_empty() {
            return false;
        }
        sigmas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sigmas[sigmas.len() / 2] >= VIDEO_MODULATION_MIN_HZ
    }

    /// Measure the radio mistuning from the locked pulses.
    ///
    /// A sync pulse is a known 1200 Hz reference lasting 4.862-20 ms, so
    /// the offset VIS would have handed us is recoverable — and to
    /// better precision, since we average over many pulses instead of
    /// one leader. Powers are summed across pulses incoherently and the
    /// peak is taken.
    ///
    /// **The scan must evaluate the frequency it asks for.** This used
    /// [`crate::dsp::goertzel_power`], which snaps its target to an
    /// integer bin — over a 36-sample pulse interior that is a 306 Hz
    /// grid, so the 2 Hz scan across ±250 Hz sampled *three* distinct
    /// frequencies and returned the low edge of whichever plateau won.
    /// The answer for a perfectly tuned Martin 1 was −128 Hz, and it
    /// moved with the measured pulse length rather than with the radio:
    /// a mid-picture decode was colour-shifted by 40 grey levels, and
    /// [`crate::sync::SyncPulseGate`] — which correctly listens at
    /// `1200 + hedr` — then heard no sync pulses at all, so the image
    /// was never released. [`crate::dsp::tone_power`] evaluates off the
    /// bin grid and the estimate lands within a few Hz.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn refine_hedr_shift(&self, pulses: &[Pulse]) -> f64 {
        let n_steps = ((2.0 * HEDR_SEARCH_HZ) / HEDR_STEP_HZ) as usize + 1;
        let mut best_hz = 1200.0;
        let mut best_power = f64::NEG_INFINITY;
        let mut interiors: Vec<&[f32]> = Vec::new();
        for p in pulses {
            let start = p.start_probe * SYNC_PROBE_STRIDE + PULSE_EDGE_TRIM_SAMPLES;
            let end = (p.start_probe + p.len_probes) * SYNC_PROBE_STRIDE;
            let end = end
                .saturating_sub(PULSE_EDGE_TRIM_SAMPLES)
                .min(self.audio.len());
            if end > start + 16 {
                interiors.push(&self.audio[start..end]);
            }
        }
        if interiors.is_empty() {
            return 0.0;
        }
        for i in 0..n_steps {
            let hz = 1200.0 - HEDR_SEARCH_HZ + (i as f64) * HEDR_STEP_HZ;
            // Normalise per pulse so a long pulse cannot outvote the rest.
            let p: f64 = interiors
                .iter()
                .map(|w| crate::dsp::tone_power(w, hz) / (w.len() as f64))
                .sum();
            if p > best_power {
                best_power = p;
                best_hz = hz;
            }
        }
        best_hz - 1200.0
    }
}

/// Standard deviation, in Hz, of the demodulated video-band frequency
/// across [`LUMA_SAMPLES_PER_GAP`] points of `audio[lo..hi]` — the
/// statistic gate 7 is built on.
///
/// The frequency is read with the decoder's own per-pixel demodulator, so
/// this is literally "how much does the luminance this decode would write
/// move?", in the same units the mode table uses (1500-2300 Hz spans the
/// 255 grey levels, 3.137 Hz per level).
#[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
fn gap_luma_sigma(
    audio: &[f32],
    lo: usize,
    hi: usize,
    hedr_shift_hz: f64,
    demod: &mut crate::demod::ChannelDemod,
) -> f64 {
    let span = hi.saturating_sub(lo);
    let mut freqs = [0.0_f64; LUMA_SAMPLES_PER_GAP];
    for (j, f) in freqs.iter_mut().enumerate() {
        let at = lo + span * j / (LUMA_SAMPLES_PER_GAP - 1);
        *f = demod.pixel_freq(audio, at as i64, hedr_shift_hz, LUMA_WINDOW_IDX);
    }
    let n = LUMA_SAMPLES_PER_GAP as f64;
    let mean = freqs.iter().sum::<f64>() / n;
    (freqs.iter().map(|f| (f - mean) * (f - mean)).sum::<f64>() / n).sqrt()
}

/// Share of 300-3300 Hz energy sitting in the 1450-2350 Hz video band,
/// sampled by a Goertzel bank at [`BANK_STEP_HZ`].
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn video_band_share(window: &[f32]) -> f64 {
    const LO_HZ: f64 = 300.0;
    const HI_HZ: f64 = 3300.0;
    const VIDEO_LO_HZ: f64 = 1450.0;
    const VIDEO_HI_HZ: f64 = 2350.0;
    let n = ((HI_HZ - LO_HZ) / BANK_STEP_HZ) as usize + 1;
    let mut total = 0.0_f64;
    let mut video = 0.0_f64;
    for i in 0..n {
        let hz = LO_HZ + (i as f64) * BANK_STEP_HZ;
        let p = crate::dsp::goertzel_power(window, hz);
        total += p;
        if (VIDEO_LO_HZ..=VIDEO_HI_HZ).contains(&hz) {
            video += p;
        }
    }
    if total <= 0.0 {
        return 0.0;
    }
    video / total
}

/// Where does the radio line containing `pulse_sample` begin?
///
/// PD, Robot and Martin put the sync pulse at line start, so the answer
/// is the pulse itself. Scottie is the exception — its sync sits between
/// the B and R channels, `2·septr + 2·chan_len` into the line — so the
/// line began earlier. When that reaches back past the start of the
/// retained audio, step forward one whole line instead of returning a
/// negative origin. Landing the decode buffer on a line boundary is what
/// lets `find_sync` and every per-line decoder run unchanged.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn line_start_for(spec: ModeSpec, pulse_sample: usize, period_samples: f64) -> usize {
    let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
    let sync_offset_seconds = match spec.sync_position {
        crate::modespec::SyncPosition::LineStart => 0.0,
        crate::modespec::SyncPosition::Scottie => {
            let chan_len = f64::from(spec.line_pixels) * spec.pixel_seconds;
            2.0 * spec.septr_seconds + 2.0 * chan_len
        }
    };
    let mut start = pulse_sample as f64 - sync_offset_seconds * work_rate;
    while start < 0.0 {
        start += period_samples;
    }
    start as usize
}

/// Extract runs of consecutive `true` probes of at least
/// [`MIN_RUN_PROBES`] length.
fn extract_pulses(has_sync: &[bool]) -> Vec<Pulse> {
    let mut out = Vec::new();
    let mut run_start: Option<usize> = None;
    for (i, &v) in has_sync.iter().enumerate() {
        match (v, run_start) {
            (true, None) => run_start = Some(i),
            (false, Some(s)) => {
                if i - s >= MIN_RUN_PROBES {
                    out.push(Pulse {
                        start_probe: s,
                        len_probes: i - s,
                    });
                }
                run_start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = run_start {
        let len = has_sync.len() - s;
        if len >= MIN_RUN_PROBES {
            out.push(Pulse {
                start_probe: s,
                len_probes: len,
            });
        }
    }
    out
}

/// Find `need` pulses filling consecutive slots of
/// `pulses[anchor].start_probe + k · period`, every slot occupied.
/// Returns the matched pulse indices, or `None` if any slot is empty.
#[allow(clippy::cast_precision_loss)]
fn match_grid(
    pulses: &[Pulse],
    anchor: usize,
    period: f64,
    tol: f64,
    need: usize,
) -> Option<Vec<usize>> {
    let base = pulses[anchor].start_probe as f64;
    let mut matched = Vec::with_capacity(need);
    matched.push(anchor);
    for k in 1..need {
        let want = base + period * (k as f64);
        let found = nearest_pulse(pulses, want, tol)?;
        matched.push(found);
    }
    Some(matched)
}

/// Index of the pulse whose start is nearest `want`, if within `tol`.
#[allow(clippy::cast_precision_loss)]
fn nearest_pulse(pulses: &[Pulse], want: f64, tol: f64) -> Option<usize> {
    // `pulses` is ascending by construction; a binary search finds the
    // insertion point and only the two neighbours can be within `tol`.
    let idx = pulses.partition_point(|p| (p.start_probe as f64) < want);
    let mut best: Option<(usize, f64)> = None;
    for cand in [idx.checked_sub(1), Some(idx)].into_iter().flatten() {
        if let Some(p) = pulses.get(cand) {
            let d = ((p.start_probe as f64) - want).abs();
            if d <= tol && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((cand, d));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Least-squares fit of `t_k = intercept + slope · k`.
/// Returns `None` for a degenerate (< 2 point) input.
#[allow(clippy::cast_precision_loss)]
fn least_squares_fit(t: &[f64]) -> Option<(f64, f64)> {
    let n = t.len();
    if n < 2 {
        return None;
    }
    let nf = n as f64;
    let k_mean = (nf - 1.0) / 2.0;
    let t_mean = t.iter().sum::<f64>() / nf;
    let mut num = 0.0;
    let mut den = 0.0;
    for (k, &tk) in t.iter().enumerate() {
        let dk = (k as f64) - k_mean;
        num += dk * (tk - t_mean);
        den += dk * dk;
    }
    if den.abs() < f64::EPSILON {
        return None;
    }
    let slope = num / den;
    Some((t_mean - slope * k_mean, slope))
}

/// Is the fitted grid actually a harmonic of a shorter one?
///
/// Probes the slots a candidate period would *skip* at each sub-multiple
/// in [`SUBHARMONIC_DIVISORS`]. If those slots are populated too, the
/// true line period is shorter and this candidate names the wrong mode.
/// Robot 36 → Robot 72 (exactly 2×) and PD-120 → PD-240 (1.7 %, inside
/// the period tolerance) are both caught here and nowhere else.
#[allow(clippy::cast_precision_loss)]
fn has_shorter_fundamental(
    pulses: &[Pulse],
    intercept: f64,
    slope: f64,
    tol: f64,
    span: usize,
) -> bool {
    for &div in &SUBHARMONIC_DIVISORS {
        let sub = slope / (div as f64);
        let mut probed = 0_usize;
        let mut hit = 0_usize;
        for k in 0..(span - 1) * div {
            // Skip the slots the candidate grid already claims.
            if k % div == 0 {
                continue;
            }
            let want = intercept + sub * (k as f64);
            probed += 1;
            if nearest_pulse(pulses, want, tol).is_some() {
                hit += 1;
            }
        }
        if probed > 0 && (hit as f64) / (probed as f64) >= SUBHARMONIC_REJECT_FRAC {
            return true;
        }
    }
    false
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

    /// The claim the whole design rests on, asserted against the table
    /// rather than a comment: among blind-eligible modes, every pair is
    /// separated either by more than twice the period tolerance or by
    /// their sync-length class. If a future mode-table addition breaks
    /// that, this fails rather than the decoder silently guessing.
    #[test]
    fn eligible_modes_are_separable_by_period_or_sync_class() {
        let eligible: Vec<ModeSpec> = ALL_SPECS
            .iter()
            .copied()
            .filter(|s| blind_eligible(*s))
            .collect();
        for (i, a) in eligible.iter().enumerate() {
            for b in eligible.iter().skip(i + 1) {
                let rel =
                    (a.line_seconds - b.line_seconds).abs() / a.line_seconds.min(b.line_seconds);
                let sync_sep = (a.sync_seconds - b.sync_seconds).abs()
                    > a.sync_seconds.min(b.sync_seconds) * WIDTH_TOL_FRAC;
                assert!(
                    rel > 2.0 * PERIOD_TOL_FRAC || sync_sep,
                    "{} and {} are not separable: periods {:.6}/{:.6} ({:.2}%), syncs {:.6}/{:.6}",
                    a.name,
                    b.name,
                    a.line_seconds,
                    b.line_seconds,
                    rel * 100.0,
                    a.sync_seconds,
                    b.sync_seconds
                );
            }
        }
    }

    /// Robot 24 and Robot 36 must stay off the inference path — they are
    /// timing-identical and their chroma parity is row-absolute.
    #[test]
    fn robot24_and_robot36_are_not_blind_eligible() {
        assert!(!blind_eligible(crate::modespec::for_mode(
            SstvMode::Robot24
        )));
        assert!(!blind_eligible(crate::modespec::for_mode(
            SstvMode::Robot36
        )));
        // And the exclusion is exactly those two.
        assert_eq!(
            ALL_SPECS.iter().filter(|s| blind_eligible(**s)).count(),
            ALL_SPECS.len() - 2
        );
    }

    #[test]
    fn extract_pulses_drops_short_runs() {
        // runs of 1, 3 and 5 — the 1 is speckle.
        let mut track = vec![false; 40];
        track[2] = true;
        track[10..13].iter_mut().for_each(|v| *v = true);
        track[20..25].iter_mut().for_each(|v| *v = true);
        let p = extract_pulses(&track);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].start_probe, p[0].len_probes), (10, 3));
        assert_eq!((p[1].start_probe, p[1].len_probes), (20, 5));
    }

    #[test]
    fn least_squares_recovers_exact_grid() {
        let t: Vec<f64> = (0..6).map(|k| 7.0 + 13.5 * f64::from(k)).collect();
        let (a, b) = least_squares_fit(&t).unwrap();
        assert!((a - 7.0).abs() < 1e-9, "intercept {a}");
        assert!((b - 13.5).abs() < 1e-9, "slope {b}");
    }

    #[test]
    fn subharmonic_gate_rejects_a_doubled_period() {
        // Pulses every 10 probes. A candidate that fitted period 20
        // would be a 2× alias; the gate must see the odd slots filled.
        let pulses: Vec<Pulse> = (0..12)
            .map(|k| Pulse {
                start_probe: k * 10,
                len_probes: 3,
            })
            .collect();
        assert!(has_shorter_fundamental(
            &pulses,
            0.0,
            20.0,
            1.0,
            LOCK_PULSES
        ));
        // The true fundamental must not be rejected.
        assert!(!has_shorter_fundamental(
            &pulses,
            0.0,
            10.0,
            1.0,
            LOCK_PULSES
        ));
    }

    #[test]
    fn line_start_is_the_pulse_for_line_start_modes() {
        let spec = crate::modespec::for_mode(SstvMode::Martin1);
        assert_eq!(line_start_for(spec, 5000, 4921.0), 5000);
    }

    #[test]
    fn line_start_reaches_back_for_scottie() {
        let spec = crate::modespec::for_mode(SstvMode::Scottie1);
        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let chan_len = f64::from(spec.line_pixels) * spec.pixel_seconds;
        let offset = ((2.0 * spec.septr_seconds + 2.0 * chan_len) * work_rate) as usize;
        let period = spec.line_seconds * work_rate;
        // Well clear of the buffer start: reach back by the sync offset.
        // ±1 sample — the offset is not a whole number of samples, so the
        // implementation's single truncation and this expectation's can
        // land either side of it.
        let got = line_start_for(spec, 60_000, period) as i64;
        assert!(
            (got - (60_000 - offset as i64)).abs() <= 1,
            "expected ≈{}, got {got}",
            60_000 - offset as i64
        );
        // Too close to the buffer start: step forward a line instead of
        // returning a negative origin.
        let got = line_start_for(spec, 100, period);
        assert!(got > 100, "expected a forward step, got {got}");
    }

    /// The P1 statistic, on the two inputs it exists to tell apart. Both
    /// sit squarely in the video band, so [`video_band_share`] says "yes"
    /// to both — occupancy cannot separate them and only variation can.
    #[test]
    fn gap_luma_sigma_separates_a_carrier_from_a_scan_line() {
        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let n = 4096_usize;
        let mut demod = crate::demod::ChannelDemod::new();

        // An unmodulated 1900 Hz carrier: zero picture information.
        let carrier: Vec<f32> = (0..n)
            .map(|i| {
                (0.5 * (2.0 * std::f64::consts::PI * 1900.0 * f64::from(i as i32) / work_rate)
                    .sin()) as f32
            })
            .collect();
        let flat = gap_luma_sigma(&carrier, 0, n, 0.0, &mut demod);
        assert!(
            flat < VIDEO_MODULATION_MIN_HZ,
            "a steady carrier must read as unmodulated, got sigma {flat:.2} Hz"
        );
        assert!(
            video_band_share(&carrier[..1024]) > OCCUPANCY_MIN,
            "the carrier does occupy the video band — that is why gate 6 passes it"
        );

        // A scan line: the video band swept 1500 -> 2300 Hz, phase-continuous.
        let mut phase = 0.0_f64;
        let sweep: Vec<f32> = (0..n)
            .map(|i| {
                let hz = 1500.0 + 800.0 * (i as f64) / (n as f64);
                phase += 2.0 * std::f64::consts::PI * hz / work_rate;
                (0.5 * phase.sin()) as f32
            })
            .collect();
        let moving = gap_luma_sigma(&sweep, 0, n, 0.0, &mut demod);
        assert!(
            moving > VIDEO_MODULATION_MIN_HZ,
            "a swept scan line must read as modulated, got sigma {moving:.2} Hz"
        );
    }

    #[test]
    fn video_band_share_separates_a_video_tone_from_noise() {
        let work_rate = f64::from(WORKING_SAMPLE_RATE_HZ);
        let tone: Vec<f32> = (0..1024_i32)
            .map(|i| (2.0 * std::f64::consts::PI * 1900.0 * f64::from(i) / work_rate).sin() as f32)
            .collect();
        assert!(
            video_band_share(&tone) > OCCUPANCY_MIN,
            "1900 Hz tone share {}",
            video_band_share(&tone)
        );
        // Broadband noise spreads across the whole 300-3300 Hz span.
        let mut state = 0x1234_5678_u32;
        let noise: Vec<f32> = (0..1024)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state as f32 / u32::MAX as f32) - 0.5
            })
            .collect();
        assert!(
            video_band_share(&noise) < OCCUPANCY_MIN,
            "noise share {}",
            video_band_share(&noise)
        );
    }
}
