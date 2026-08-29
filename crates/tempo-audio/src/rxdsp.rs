//! The waterfall row producer, on its own thread.
//!
//! This is the consumer half of [`crate::rxtap`] — read that module's header for why the row
//! moved off the radio loop. The one-line version: the loop is the only thread that issues
//! blocking CAT (up to 2500 ms on slow serial), and it was also the only producer of spectrum
//! rows, so every CAT stall froze the waterfall.
//!
//! THE SAFETY ARGUMENT IS THE CAPTURE LIST. [`RxDsp::tick`] takes an `&RxTap`, a
//! `&SpectrumFeed` and a `&MeterFeed` and nothing else, and the thread in [`spawn`] closes over
//! exactly those three. No `Rig`, no `CatDaemon`, no `CpalBackend`, no `Engine`. A CAT call
//! cannot be issued from here because nothing here can name one. That is a compile-time
//! property, not a policy.
//!
//! THE RX LEVEL METER LIVES HERE TOO (2026-08-01). The tick drains the exact post-gain mono
//! samples the capture callback teed, so the level is measured from the same audio the decode
//! path hears — and publishing it from THIS thread (onto the wait-free [`MeterFeed`]) means a
//! CAT stall can no longer freeze the needle, which it did when the radio loop carried the copy.
//! Ballistics are a real instrument's: fast attack, standard decay — shaping RESPONSE, never
//! inventing motion.
//!
//! DELIBERATELY NOT MOVED: the mode taps (`feed_rx_audio` — CW/RTTY/APRS/SSTV/QSO rings) stay
//! on the radio loop. Moving them here was designed and rejected: this thread would have to
//! reach the engine mutex, and during a 700-2500 ms CAT hold a bounded backlog would start
//! DROPPING audio — losing CW/RTTY copy in exactly the stall this change exists to fix. Today
//! nothing is dropped, and keeping them on the loop costs nothing (the loop already holds that
//! lock). It also leaves this thread with zero engine handles, which is what makes the capture
//! list above the whole argument.

use std::sync::Arc;
use std::time::Duration;

use tempo_app::dto::Spectrum;
use tempo_app::engine::{MeterFeed, SpectrumFeed};
use tempo_core::spectrum::WindowN;

use crate::capture_resample::CaptureResampler;
use crate::rxtap::RxTap;

/// Modem/analysis rate the rolling window is kept at (matches the decode path).
const ANALYSIS_RATE: f32 = tempo_core::tempo_fast::SAMPLE_RATE;
/// Same rate as an integer, for the resampler and the test tones.
const ANALYSIS_RATE_HZ: u32 = ANALYSIS_RATE as u32;
/// Longest window the RIG SCOPE may ask for (`WindowN::Sharp`). The rolling buffer is kept this
/// long so a Sharp row has 4096 samples to look at — the wide row is unaffected, because
/// `power_spectrum` reads the LAST `FFT_N` samples of whatever it is handed, so a longer buffer
/// changes nothing for it. That is what keeps the byte-stable golden row green.
const MAX_WINDOW: usize = tempo_core::spectrum::MAX_FFT_N;
/// Consumer cadence. Matches the radio loop's own 20 ms tick, so row rate is unchanged.
pub const TICK_MS: u64 = 20;
/// RX meter ATTACK per 20 ms tick: rising audio closes 60% of the remaining distance each tick,
/// so a full-scale step reaches 90% within three ticks (60 ms) — near-instant, like a hardware
/// S-meter (IC-9700 class). The old SYMMETRIC smoothing (0.85 both ways) took ~150–300 ms to
/// register a key-down: the operator heard the signal before the needle moved ("accurate, but
/// slow"). Attack fast / decay slow is standard instrument ballistics, not invented motion.
const RX_METER_ATTACK: f32 = 0.6;
/// RX meter DECAY per 20 ms tick: level falls to 85% each tick when the audio drops, ~285 ms to
/// fall 90% — the standard smooth needle-fall of a hardware S-meter, and the same feel the old
/// per-callback meter had (0.85 per ~10–20 ms callback). Decay is smoothing the eye WANTS.
const RX_METER_DECAY: f32 = 0.85;

/// ---- THE CW ZERO-BEAT MEASUREMENT (2026-08-28) ----
///
/// The operator asked for "a light that comes on when you are zero beat to the CW signal".
/// The app already draws the TARGET (the pitch marker on the CW scope) and had no way to
/// measure what is actually coming in, so an operator had to eyeball the waterfall against
/// the hairline. This tick closes that: [`tempo_core::spectrum::dominant_tone`] over the
/// rolling window the row is already built from.
///
/// IT RIDES THIS THREAD ON PURPOSE, and the capture list in the module header is unchanged —
/// the pitch comes IN and the reading goes OUT through the wait-free [`MeterFeed`], which this
/// thread already holds. No `Engine`, no `Rig`, so a 2500 ms CAT stall cannot freeze the light
/// any more than it can freeze the waterfall.
///
/// ⛔ IT DISPLAYS AND NEVER ACTS. Nothing downstream steers the radio from this reading: there
/// is no path from here to a CAT command, and there must never be one. Auto-tuning the
/// operator's dial to zero-beat would be a defect (notify-never-act), not a feature.
///
/// COST: one extra 2048-point rfft (~30 us) per 20 ms tick, and ONLY while the CW section has
/// armed a target — every other section leaves it disarmed and this costs one atomic load.
/// The row's own FFT cannot be reused: `power_spectrum_n` destroys its raw bins into 512
/// peak-held, dB-mapped display bins, and interpolating a peak across THOSE is a different
/// and wrong operation, not merely a coarser one.
///
/// How long a reading survives with no fresh measurement, in 20 ms ticks (2 s). CW IS KEYED:
/// the 171 ms analysis window spends most of a QSO looking at a gap — at 10 wpm an inter-word
/// gap alone is 840 ms — so a reading that expired at the first key-up would strobe the
/// indicator on every letter. Two seconds outlasts any gap a human sends, and still blanks
/// within a breath of the station actually stopping. A tuning aid may lag; it may not lie.
pub const ZERO_BEAT_HOLD_TICKS: u32 = 2_000 / TICK_MS as u32;
/// How far either side of the operator's pitch the tone search runs. ±400 Hz: the operator's
/// own framing is that being 80 Hz off must not look like being 400 Hz off, so the guidance
/// has to reach 400; and past it a signal is outside any CW filter he would be using, i.e.
/// not the signal he is tuning. Reporting one would swing the needle at a station he cannot
/// even hear.
///
/// ⚠️ MIRRORED IN TYPESCRIPT as `ZERO_BEAT_RANGE_HZ` (`ui/src/waterfall.ts`), which is the
/// needle's full-scale deflection — the same quantity from the display's side. **This note is
/// not what keeps them equal**: `ui/src/wire-consistency.test.ts` reads BOTH constants out of
/// BOTH sources and fails naming both values, because the `RadioProfilePatch` seam proved five
/// times that a comment naming the other side is documentation, not a mechanism. Renaming this
/// constant fails that guard too, rather than quietly making it stop looking.
/// How close two consecutive tone estimates must sit to count as the SAME tone, in Hz.
///
/// ⭐ THIS IS WHAT STOPS THE NEEDLE FLYING AROUND ON A DEAD BAND, and it is a different problem
/// from the SNR floor. [`MIN_TONE_SNR_DB`] is a false-alarm budget against PURE noise, and it is
/// a good one — about 1e-7 per measurement. But the ±400 Hz search window on a real band is not
/// pure noise: it holds other CW signals, birdies, carriers and atmospheric peaks, so the
/// detector honestly reports the loudest thing in the window and that thing is simply not the
/// signal being tuned. Operator report (2026-08-29): "the little line flies around a lot in dead
/// air and looks too active, but the on-pitch detection when CW is being sent seems accurate" —
/// both halves of which are exactly this.
///
/// A CW note being tuned holds its frequency; a noise peak hops hundreds of Hz between ticks.
/// 12 Hz is about two raw bins at 5.86 Hz, so real drift and estimator wobble stay inside it
/// while anything that moved to a different peak falls outside.
///
/// ⚠️ RAISING THE SNR FLOOR WOULD HAVE BEEN THE WRONG FIX. It is what costs weak-signal
/// accuracy, and the operator's report is that the accuracy is already right.
const ZERO_BEAT_STABLE_HZ: f32 = 12.0;

/// How many agreeing measurements a tone needs before it is shown at all.
///
/// ⚠️ THIS IS NOT "A FEW TICKS", AND THE REASON IS THE WINDOW OVERLAP. Consecutive measurements
/// are NOT independent evidence: the analysis window is 171 ms and it advances 20 ms per tick,
/// so successive windows share ~88% of their audio and a peak that dominates one dominates the
/// next eight for no better reason than still being inside the window. A three-tick rule looks
/// like a stability test and is really a restatement of the window length.
///
/// Nine ticks is 180 ms — just past one whole window — so the first and last measurement of a
/// run come from essentially non-overlapping audio. That is the shortest run that asks the
/// signal to still be there after the evidence has completely turned over.
const ZERO_BEAT_STABLE_TICKS: u8 = 9;

/// How long a candidate survives ticks with NO detection, before the run is abandoned.
///
/// ⭐ WITHOUT THIS, FAST CW COULD NEVER ACQUIRE. A run has to span a whole analysis window, but
/// at 25 wpm a dah is only 144 ms — shorter than that. Successive elements are the SAME tone
/// though, so the run is allowed to continue across the gaps between them, and a real note
/// accumulates its evidence over several elements. A hopping peak gains nothing from this: each
/// new peak DISAGREES with the candidate and resets the run to one, whether or not a gap
/// intervened. 400 ms outlasts an inter-letter gap without keeping a dead candidate alive.
const ZERO_BEAT_GRACE_TICKS: u8 = 20;

const ZERO_BEAT_SEARCH_HZ: f32 = 400.0;
/// The search never runs into DC/rumble, whatever the pitch.
const ZERO_BEAT_MIN_HZ: f32 = 50.0;

/// Rolling-window state for the spectrum producer. Split out from the thread so tests can drive
/// it deterministically, with no sleeps and no threads.
pub struct RxDsp {
    rs: CaptureResampler,
    window: Vec<f32>,
    epoch: u64,
    rate: u32,
    /// The ballistics-shaped RX level (0..1 RMS) published to the meter feed each tick.
    meter: f32,
    /// The last measured received CW tone (Hz), or `None` for "nothing to tune to".
    cw_tone: Option<f32>,
    /// The last RAW estimate, whether or not it was published — the candidate a new one is
    /// compared against. Distinct from `cw_tone`, which is what the operator can see.
    cw_candidate: Option<f32>,
    /// How many agreeing estimates `cw_candidate` has accumulated.
    cw_agree: u8,
    /// Ticks with no detection since the last one, while a candidate still stands.
    cw_quiet: u8,
    /// Ticks that reading may still stand for — see [`ZERO_BEAT_HOLD_TICKS`].
    cw_hold: u32,
}

impl Default for RxDsp {
    fn default() -> Self {
        Self::new()
    }
}

impl RxDsp {
    pub fn new() -> Self {
        Self {
            rs: CaptureResampler::new(ANALYSIS_RATE_HZ, ANALYSIS_RATE_HZ),
            window: Vec::with_capacity(MAX_WINDOW * 2),
            epoch: 0,
            rate: ANALYSIS_RATE_HZ,
            meter: 0.0,
            cw_tone: None,
            cw_candidate: None,
            cw_agree: 0,
            cw_quiet: 0,
            cw_hold: 0,
        }
    }

    /// Drain whatever the capture callback has teed, publish a row, and publish the RX level.
    ///
    /// Returns true when a row was published. Takes only the tap and the two publish seams —
    /// see the module header; this signature IS the safety argument.
    pub fn tick(&mut self, tap: &RxTap, feed: &SpectrumFeed, meters: &MeterFeed) -> bool {
        // Age the held zero-beat reading FIRST, on every tick — including the ones that
        // publish no row. A capture that goes quiet must expire the indicator, never freeze it.
        self.age_zero_beat(meters);
        let Some(src) = tap.current() else {
            // No audio open. Publish an EMPTY row rather than nothing: an empty row makes the
            // UI stop cleanly, whereas publishing nothing sends the reader down the engine-lock
            // fallback, which returns a stale NON-empty row and reproduces the streak. The
            // meter goes to zero for the same honesty: no capture, no level.
            feed.publish_audio(empty_row());
            self.meter = 0.0;
            meters.set_rx_level(0.0);
            // No capture, no reading — not even a held one, for the same reason the level
            // goes to zero: there is no audio to have measured.
            self.publish_zero_beat(meters, None);
            return false;
        };
        // A new source (device swap, rate change) must never smear two rates into one window.
        if src.epoch != self.epoch {
            self.rs = CaptureResampler::new(src.rate, ANALYSIS_RATE_HZ);
            self.window.clear();
            self.epoch = src.epoch;
            self.rate = src.rate;
        }
        let mut dev: Vec<f32> = Vec::new();
        while let Some(s) = src.ring.pop() {
            dev.push(s);
        }
        if dev.is_empty() {
            return false; // nothing new this tick; the last row AND level stand (no new info)
        }
        // ---- RX level meter: RMS of this tick's post-gain samples, instrument ballistics ----
        // Measured on the DEVICE-rate samples (the exact `m` values the callback teed), before
        // the display resample, so the reading matches what the sound card actually delivered.
        // RMS (not peak) keeps the number comparable to WSJT-X's meter (the UI renders
        // 20·log10(rms)+90.3, see ui LevelMeter).
        let sum_sq: f32 = dev.iter().map(|s| s * s).sum();
        let rms = (sum_sq / dev.len() as f32).sqrt().clamp(0.0, 1.0);
        self.meter = if rms > self.meter {
            // ATTACK: fast — the needle must move with the audio, not after it.
            self.meter + (rms - self.meter) * RX_METER_ATTACK
        } else {
            // DECAY: the standard smooth fall (85%/tick).
            self.meter * RX_METER_DECAY + rms * (1.0 - RX_METER_DECAY)
        };
        meters.set_rx_level(self.meter);
        // The RAW tick RMS, retained for the MSK144 Fast Graph — a meteor ping is exactly the
        // fast attack the meter ballistics above exist to smooth away, so the graph gets the
        // unshaped sample. Same thread, same capture list; the ring lives inside MeterFeed so
        // the safety argument (no new handle here) holds unchanged.
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        meters.push_fast_power(unix_ms, rms);
        let pcm = self.rs.process(&dev);
        if pcm.is_empty() {
            return false;
        }
        self.window.extend_from_slice(&pcm);
        if self.window.len() > MAX_WINDOW {
            let drop = self.window.len() - MAX_WINDOW;
            self.window.drain(0..drop);
        }
        feed.publish_audio(compute_row(&self.window));
        // The CW zero-beat reading, off the SAME rolling window the row was just built from —
        // so the number and the picture can never describe different audio.
        self.measure_zero_beat(meters);
        // The rig scope's own window, when one is on screen asking for it. One extra 2048-point
        // FFT (~30 us of a 20 ms budget) and no extra bytes — see `compute_row_over`. The
        // request expires on its own, so this costs nothing when no scope is up.
        if let Some((lo, hi, win)) = feed.scope_request() {
            feed.publish_scope(compute_row_over(&self.window, lo, hi, win));
        }
        true
    }

    /// Store a zero-beat reading (or its absence) and restart its hold.
    fn publish_zero_beat(&mut self, meters: &MeterFeed, tone: Option<f32>) {
        self.cw_tone = tone;
        self.cw_hold = if tone.is_some() {
            ZERO_BEAT_HOLD_TICKS
        } else {
            0
        };
        meters.set_cw_tone_hz(tone);
    }

    /// Count down the hold on the standing reading, and blank it when it runs out.
    fn age_zero_beat(&mut self, meters: &MeterFeed) {
        if self.cw_tone.is_none() {
            return;
        }
        if self.cw_hold > 0 {
            self.cw_hold -= 1;
            return;
        }
        self.cw_tone = None;
        meters.set_cw_tone_hz(None);
    }

    /// Measure the dominant tone near the operator's CW pitch, when one is armed.
    ///
    /// A window with NO tone in it is not "no signal" — CW is keyed, and most windows land in
    /// a gap — so a miss leaves the standing reading to [`Self::age_zero_beat`] rather than
    /// blanking on the spot.
    fn measure_zero_beat(&mut self, meters: &MeterFeed) {
        let Some(target) = meters.cw_target_hz() else {
            // Disarmed (any section but CW). Drop a reading left over from the last one.
            if self.cw_tone.is_some() {
                self.publish_zero_beat(meters, None);
            }
            return;
        };
        let lo = (target - ZERO_BEAT_SEARCH_HZ).max(ZERO_BEAT_MIN_HZ);
        let hi = target + ZERO_BEAT_SEARCH_HZ;
        let Some(est) = tempo_core::spectrum::dominant_tone(
            &self.window,
            ANALYSIS_RATE,
            lo,
            hi,
            target,
            tempo_core::spectrum::MIN_TONE_SNR_DB,
        ) else {
            // Nothing above the floor. A PUBLISHED reading is left alone — `age_zero_beat`
            // owns its expiry, and blanking here would strobe the indicator on every
            // inter-letter gap, which is what the hold exists to prevent.
            //
            // The CANDIDATE is kept for a grace period rather than dropped, so a run can span
            // the gaps between CW elements; see ZERO_BEAT_GRACE_TICKS.
            self.cw_quiet = self.cw_quiet.saturating_add(1);
            if self.cw_quiet > ZERO_BEAT_GRACE_TICKS {
                self.cw_candidate = None;
                self.cw_agree = 0;
            }
            return;
        };

        // ⭐ STABILITY, NOT SENSITIVITY. A tone must be found in the SAME place on several
        // consecutive ticks before it is shown. A CW note being tuned holds still; a noise peak
        // or a distant signal hops, so it never accumulates a run and never reaches the needle.
        // See ZERO_BEAT_STABLE_HZ for why this is the right knob and the SNR floor is not.
        let agrees = self
            .cw_candidate
            .is_some_and(|prev| (prev - est.hz).abs() <= ZERO_BEAT_STABLE_HZ);
        self.cw_agree = if agrees {
            self.cw_agree.saturating_add(1)
        } else {
            1
        };
        self.cw_candidate = Some(est.hz);
        self.cw_quiet = 0;

        // An ESTABLISHED reading keeps updating on every agreeing tick, so once the operator is
        // on a signal the needle tracks it live — the run requirement is paid once, at the start.
        if self.cw_agree >= ZERO_BEAT_STABLE_TICKS || self.cw_tone.is_some() && agrees {
            self.publish_zero_beat(meters, Some(est.hz));
        }
    }
}

/// The audio-FFT row, byte-for-byte the computation the radio loop used to do.
///
/// ITS WINDOW IS [`WindowN::Balanced`] — 2048 @ 12 kHz = 171 ms of audio — and that is the
/// window every consumer except the rig scope reads, the FT waterfall included.
///
/// Halved from 4096 (341 ms) for display LIVELINESS (2026-08-01, operator report "smoothed out
/// to remove response"): the Hann taper weights the newest samples near zero, so a signal edge
/// only reaches full brightness a full window later — the window IS the display's time smear.
/// 2048 keeps raw bins at 5.86 Hz, still finer than the 512-bin display's 7.81 Hz over
/// 0-4000 Hz, so visible resolution was unchanged while the edge-to-full-brightness lag halved
/// (and the FFT got cheaper). Pinned by `the_analysis_window_fits_the_display_liveliness_budget`.
///
/// ⚠️ NO LONGER THE ONLY WINDOW (2026-08-15). The rig scope may run ITS row at
/// [`WindowN::Fast`] or [`WindowN::Sharp`] — see `compute_row_over`. This one does not move,
/// because changing it would move the FT waterfall's picture too.
fn compute_row(audio: &[f32]) -> Spectrum {
    // Widened from 200-2900: shows the full SSB/DATA passband and the filter-slope shelf. The
    // 6 kHz Nyquist (12 kHz capture) caps the top; 4000 covers every voice/data passband in use.
    const LO_HZ: f32 = 0.0;
    const HI_HZ: f32 = 4000.0;
    // Always Balanced: every other consumer reads this row, and the window control is the
    // SCOPE's alone. Changing it here would move the FT waterfall's picture too.
    compute_row_over(audio, LO_HZ, HI_HZ, WindowN::Balanced)
}

/// The same row over an arbitrary span.
///
/// THE BIN COUNT IS FIXED AT 512 WHATEVER THE SPAN, which is the whole point: the payload is
/// the same 512 numbers either way, and narrowing the span is therefore free resolution rather
/// than a bigger message. Over the full 0-4000 that is 7.81 Hz per bin; over the CW cockpit's
/// 300-1100 window it is 1.5625 Hz — FIVE TIMES FINER for the same bytes on the wire.
///
/// ⚠️ BE HONEST ABOUT WHAT THIS DOES NOT DO. It does not make a carrier thinner. Apparent
/// width is set by the Hann main lobe (23.4 Hz at N=2048), not by the display grid, and only
/// the window length moves that. What it buys is SHAPE: that lobe is sampled by 3 numbers over
/// the full span and by 15 over the CW window, so the scope draws a lobe instead of
/// interpolating a spike across 75 px.
fn compute_row_over(audio: &[f32], lo_hz: f32, hi_hz: f32, win: WindowN) -> Spectrum {
    const BINS: usize = 512;
    let row = if audio.is_empty() {
        Vec::new()
    } else {
        tempo_core::spectrum::power_spectrum_n(audio, ANALYSIS_RATE, lo_hz, hi_hz, BINS, win)
    };
    Spectrum {
        row,
        lo_hz: f64::from(lo_hz),
        hi_hz: f64::from(hi_hz),
        source: "audio".into(),
    }
}

fn empty_row() -> Spectrum {
    compute_row(&[])
}

/// Spawn the producer thread. Panic-wrapped: a silent death here would kill the waterfall with
/// no other symptom, which is exactly the silent-fallback failure mode the TX-safety notes warn
/// about, so the panic is surfaced rather than swallowed.
pub fn spawn(
    tap: Arc<RxTap>,
    feed: SpectrumFeed,
    meters: MeterFeed,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("nexus-rx-dsp".into())
        .spawn(move || {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut dsp = RxDsp::new();
                loop {
                    dsp.tick(&tap, &feed, &meters);
                    std::thread::sleep(Duration::from_millis(TICK_MS));
                }
            }));
            if run.is_err() {
                // The waterfall is now dead for the session. Say so loudly in the log; the row
                // going empty is the user-visible half.
                eprintln!("nexus: rx-dsp thread panicked — the waterfall has stopped");
            }
        })
        .expect("spawn rx-dsp thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::SpscRing;

    fn tone(rate: u32, hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    /// Which 512-bin index covers `hz` over the 0-4000 Hz span.
    fn bin_of(hz: f32) -> usize {
        ((hz / 4000.0) * 512.0) as usize
    }

    fn peak_bin(row: &[f32]) -> usize {
        row.iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// THE REGRESSION THIS WHOLE CHANGE EXISTS FOR.
    ///
    /// Hold the engine mutex for the entire run — the exact condition a blocking CAT call
    /// creates on the radio loop — and assert rows keep being produced anyway. On the old
    /// architecture the equivalent (drive the loop while the engine is held) produces ZERO
    /// rows, which is the vertical streaking the operator photographed.
    #[test]
    fn rows_keep_publishing_while_the_engine_mutex_is_held() {
        use std::sync::Mutex;
        let engine = Arc::new(Mutex::new(()));
        let held = engine.lock().unwrap(); // stand in for a 2.5 s CAT stall

        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();

        let mut published = 0;
        for _ in 0..120 {
            ring.push_slice(&tone(12_000, 1500.0, 256));
            if dsp.tick(&tap, &feed, &meters) {
                published += 1;
            }
        }
        drop(held);

        assert_eq!(
            published, 120,
            "every tick must publish a row while the engine mutex is held — the producer is \
             not allowed to depend on it"
        );
        let row = feed.row().expect("a row is published");
        assert!(!row.row.is_empty(), "and the row carries real spectrum");
        // The RX level meter rides the same tick, so a CAT stall can't freeze the needle either
        // (it used to: the radio loop carried the level copy, and the loop is the CAT thread).
        assert!(
            meters.rx_level() > 0.0,
            "the meter publishes on this thread too — no CAT stall may freeze it"
        );
    }

    /// Display bins within 6 dB of the peak — how finely the Hann main lobe is SAMPLED.
    fn lobe_bins(row: &[f32]) -> usize {
        let peak = row.iter().copied().fold(f32::MIN, f32::max);
        let thr = peak - 6.0 / tempo_core::spectrum::DB_SPAN;
        row.iter().filter(|v| **v >= thr).count()
    }

    /// TIER 3 — the scope's row follows the scope's WINDOW, and that is where fidelity comes from.
    #[test]
    fn a_narrow_scope_window_samples_the_lobe_far_more_finely() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();

        for _ in 0..40 {
            ring.push_slice(&tone(12_000, 700.0, 256));
            dsp.tick(&tap, &feed, &meters);
        }

        let wide = feed.row().expect("a wide row is published");
        assert_eq!((wide.lo_hz, wide.hi_hz), (0.0, 4000.0));
        let wide_lobe = lobe_bins(&wide.row);

        // THE FIRST CALL ONLY REGISTERS THE REQUEST and is answered from the wide row. That
        // fallback is the design, not a wart: it is why the UI needs no state machine and can
        // never draw one window's frequencies under another window's labels.
        let first = feed
            .scope_row(300.0, 1100.0, WindowN::Balanced)
            .expect("a row either way");
        assert_eq!(
            first.hi_hz, 4000.0,
            "until the producer has answered, the request falls back to the full row"
        );

        ring.push_slice(&tone(12_000, 700.0, 256));
        dsp.tick(&tap, &feed, &meters);
        let narrow = feed
            .scope_row(300.0, 1100.0, WindowN::Balanced)
            .expect("the narrow row");
        assert_eq!((narrow.lo_hz, narrow.hi_hz), (300.0, 1100.0));
        assert_eq!(
            narrow.row.len(),
            wide.row.len(),
            "SAME PAYLOAD — the win is resolution per byte, not more bytes"
        );

        let narrow_lobe = lobe_bins(&narrow.row);
        // MEASURED when this landed: wide=1, narrow=4. One bin is the whole point — over the
        // full span the scope holds a SINGLE number describing the carrier's peak region and
        // interpolates it across ~75 px of the CW view, which is what "not crisp" looks like.
        assert!(
            wide_lobe <= 2,
            "the full-span row samples the lobe with almost nothing (got {wide_lobe} bins) — if \
             this ever rises, the premise of this whole tier changed"
        );
        assert!(
            narrow_lobe >= wide_lobe * 3,
            "the 800 Hz window must sample the lobe far more finely than the 4000 Hz one \
             (wide={wide_lobe} bins, narrow={narrow_lobe})"
        );
    }

    /// METER BALLISTICS PIN — fast attack.
    ///
    /// A real S-meter (IC-9700 class) snaps UP nearly instantly and falls smoothly; symmetric
    /// smoothing (the old per-callback EMA) made a key-down take ~150–300 ms to register, which
    /// the operator read as "accurate, but slow". Pin the attack: a full-scale step must reach
    /// 90% of the stored level within THREE 20 ms ticks (60 ms) of the audio that carries it.
    #[test]
    fn a_full_scale_step_reaches_90pct_within_three_ticks() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        // Silence first, so the step starts from a settled 0 level.
        ring.push_slice(&vec![0.0; 256]);
        dsp.tick(&tap, &feed, &meters);
        assert_eq!(meters.rx_level(), 0.0, "settled at zero before the step");
        // Full-scale step: three ticks' worth of ±full-scale samples (RMS = 1.0).
        for _ in 0..3 {
            ring.push_slice(&vec![1.0; 256]);
            dsp.tick(&tap, &feed, &meters);
        }
        assert!(
            meters.rx_level() >= 0.9,
            "a full-scale step must reach 90% within 3 ticks (60 ms); got {} — the attack is \
             too slow, the operator sees the audio before the needle",
            meters.rx_level()
        );
    }

    /// METER BALLISTICS PIN — standard decay.
    ///
    /// The fast attack must NOT come with an instant drop: the needle falls smoothly (85% per
    /// tick, ~285 ms to fall 90%) like a hardware meter, so syllables and CW elements read as
    /// a live level, not a strobe.
    #[test]
    fn the_meter_falls_smoothly_not_instantly() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        for _ in 0..10 {
            ring.push_slice(&vec![1.0; 256]);
            dsp.tick(&tap, &feed, &meters);
        }
        let peak = meters.rx_level();
        assert!(peak > 0.95, "settled near full scale (got {peak})");
        // One silent tick: the level must FALL, but only to ~85% — never snap to zero.
        ring.push_slice(&vec![0.0; 256]);
        dsp.tick(&tap, &feed, &meters);
        let after_one = meters.rx_level();
        assert!(
            after_one < peak && after_one > 0.5 * peak,
            "one silent tick decays smoothly (got {after_one} from {peak})"
        );
        // ~14 silent ticks (≈285 ms): 90% of the level is gone — standard needle fall.
        for _ in 0..13 {
            ring.push_slice(&vec![0.0; 256]);
            dsp.tick(&tap, &feed, &meters);
        }
        assert!(
            meters.rx_level() < 0.15,
            "the needle has fallen ~90% after ~285 ms of silence (got {})",
            meters.rx_level()
        );
    }

    #[test]
    fn a_tone_lands_in_the_right_bin_at_the_native_rate() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        ring.push_slice(&tone(12_000, 1500.0, WindowN::Balanced.n() * 2));
        for _ in 0..4 {
            dsp.tick(&tap, &feed, &meters);
        }
        let row = feed.row().expect("published").row;
        let (want, got) = (bin_of(1500.0), peak_bin(&row));
        assert!(
            got.abs_diff(want) <= 3,
            "1500 Hz should peak near bin {want}, got {got}"
        );
    }

    /// The non-integer ratio is the one that exercises the resampler's fractional phase.
    #[test]
    fn a_tone_lands_in_the_right_bin_at_44_1k() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(200_000));
        tap.publish_card(ring.clone(), 44_100);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        ring.push_slice(&tone(44_100, 1500.0, 44_100 / 2));
        for _ in 0..8 {
            dsp.tick(&tap, &feed, &meters);
        }
        let row = feed.row().expect("published").row;
        let (want, got) = (bin_of(1500.0), peak_bin(&row));
        assert!(
            got.abs_diff(want) <= 4,
            "1500 Hz at 44.1 kHz should peak near bin {want}, got {got}"
        );
    }

    /// A device swap must not blend the old rate into the new window.
    #[test]
    fn an_epoch_change_rebuilds_the_resampler_and_clears_the_window() {
        let tap = RxTap::new();
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();

        let a = Arc::new(SpscRing::new(200_000));
        tap.publish_card(a.clone(), 44_100);
        a.push_slice(&tone(44_100, 800.0, 44_100 / 2));
        for _ in 0..8 {
            dsp.tick(&tap, &feed, &meters);
        }

        let b = Arc::new(SpscRing::new(48_000));
        tap.publish_card(b.clone(), 12_000);
        b.push_slice(&tone(12_000, 2500.0, WindowN::Balanced.n() * 2));
        for _ in 0..4 {
            dsp.tick(&tap, &feed, &meters);
        }

        let row = feed.row().expect("published").row;
        let (want, got) = (bin_of(2500.0), peak_bin(&row));
        assert!(
            got.abs_diff(want) <= 4,
            "after the swap the NEW 2500 Hz tone must dominate (bin {want}), got {got} — a \
             stale 800 Hz peak would mean the old rate smeared through"
        );
    }

    /// With no audio open the row must be EMPTY, not absent: absent sends the reader down the
    /// engine-lock fallback, which hands back a stale non-empty row and streaks.
    #[test]
    fn no_source_publishes_an_empty_row_not_nothing() {
        let tap = RxTap::new();
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        assert!(!dsp.tick(&tap, &feed, &meters), "nothing to publish");
        let row = feed.row().expect("an EMPTY row is still published");
        assert!(
            row.row.is_empty(),
            "and it is empty, so the UI stops cleanly"
        );
        assert_eq!(
            meters.rx_level(),
            0.0,
            "no capture → the meter reads zero, never a stale level"
        );
    }

    /// THE LIVELINESS BUDGET (operator report 2026-07-30: "the waterfall speed seems to be
    /// 'smoothed out' to remove response").
    ///
    /// The only temporal smoothing on the spectrum is the analysis window itself: the row is a
    /// Hann-windowed FFT over the last `WindowN::Balanced` samples, so a signal edge takes a full window
    /// to reach full brightness (and ~half a window to reach half). At 4096 samples / 12 kHz
    /// that was 341 ms — the literal "smoothed out". Pin the window's time span at ≤ 200 ms so
    /// a future "more resolution" change can't quietly re-smear the display. (Frequency cost of
    /// shrinking it: raw bins must stay finer than the 512-bin display over 0–4000 Hz, i.e.
    /// ≤ 7.8 Hz — asserted alongside, so this pin can't be satisfied by gutting resolution.)
    ///
    /// ⚠️ RESTATED, NOT RELAXED (2026-08-15, the rig scope gained a window-length control).
    /// `WindowN::Sharp` runs the SCOPE row at 4096 — 341 ms, outside this budget on purpose,
    /// because the operator asked for frequency resolution and is trading liveliness for it
    /// knowingly. That is an opt-out, so the pin now covers the two windows nobody opted into:
    /// the DEFAULT row (`WindowN::Balanced`, which every other consumer including the FT waterfall reads)
    /// and the scope's DEFAULT (`WindowN::Balanced`). Deleting this pin to make the control fit
    /// would have thrown away the guarantee for everyone who never touches the control.
    #[test]
    fn the_analysis_window_fits_the_display_liveliness_budget() {
        let window_s = WindowN::Balanced.n() as f32 / ANALYSIS_RATE;
        assert!(
            window_s <= 0.200,
            "analysis window is {:.0} ms of audio — a signal edge needs that long to reach \
             full brightness, which reads as lag on CW/voice (budget: 200 ms)",
            window_s * 1000.0
        );
        let raw_bin_hz = ANALYSIS_RATE / WindowN::Balanced.n() as f32;
        assert!(
            raw_bin_hz <= 4000.0 / 512.0,
            "raw FFT bins ({raw_bin_hz:.2} Hz) must stay finer than the 512-bin display \
             (7.81 Hz) — liveliness may not be bought with visible resolution"
        );
        // The scope's DEFAULT must sit inside the same budget: an operator who never opens the
        // control must never be given a slower display than the one this pin was written for.
        let scope_default_s = WindowN::default().seconds(ANALYSIS_RATE);
        assert!(
            scope_default_s <= 0.200,
            "the rig scope's DEFAULT window is {:.0} ms — Sharp is an opt-out, the default is not",
            scope_default_s * 1000.0
        );
        // And the default must still be the shipped length, so an upgrade moves nobody.
        assert_eq!(WindowN::default(), WindowN::Balanced);
    }

    /// VALUE PIN, relocated from the `watch_identity` golden.
    ///
    /// That golden used to assert the waterfall row byte-for-byte for a known input. The row is
    /// no longer produced by the engine, so the assertion moved here with it — losing it would
    /// have quietly dropped the only check that the DSP output itself does not drift. Same
    /// arithmetic input (a deterministic sawtooth at the modem rate), digest over the whole row.
    ///
    /// How far a bin may move before it counts as drift. A one-ULP `log10f` difference reaches
    /// roughly 1e-7 in these 0..=1 display values; a real DSP change moves every bin by O(0.1)
    /// (see the history below). 1e-3 sits four orders of magnitude clear of the noise and three
    /// clear of any change worth catching, so it discriminates rather than splitting the
    /// difference.
    const ROW_TOLERANCE: f32 = 1e-3;

    /// The reference row, generated on a known-good tree by the same code path this test drives.
    fn reference_row() -> Vec<f32> {
        include_str!("../tests/fixtures/rxdsp_reference_row.txt")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                l.trim()
                    .parse::<f32>()
                    .expect("reference row is decimal floats")
            })
            .collect()
    }

    /// A DIFF HERE IS A DSP BEHAVIOUR CHANGE. Justify it, then regenerate the reference — do not
    /// widen the tolerance.
    ///
    /// ⚠️ IT WAS AN EXACT DIGEST OVER THE ROW'S BIT PATTERNS UNTIL 2026-08-08, AND THAT PINNED THE
    /// HOST'S libm AS MUCH AS OUR DSP. Every bin goes through `f32::log10` (and `cosf` in the
    /// window), which resolve to the platform's `log10f`/`cosf`. glibc's are not correctly-rounded
    /// and are free to move between releases — and did: `log10f(3.3e-05)` differs by ONE MANTISSA
    /// BIT between glibc 2.39 and 2.43. With 512 bins hashed by bit pattern, one bin landing on
    /// such an input changes the digest, which is exactly what a bit-pattern hash is designed to
    /// notice. Reported on (K)ubuntu 26.04 (#18) with nothing in Nexus changed between the pass and
    /// the fail, and it would have gone red in our own CI the day the runners rolled forward.
    ///
    /// So the row is compared against a REFERENCE ROW within [`ROW_TOLERANCE`] instead. That keeps
    /// the drift-detection this test exists for — the two real changes in the history below moved
    /// every value by O(0.1), four orders of magnitude clear of the bound — while tolerating a
    /// libm that rounds a hair differently. Values are display intensity in 0..=1.
    ///
    /// Digest history:
    /// - 2026-08-01: the analysis window went 4096 → 2048 for display liveliness (see `compute_row`). The row
    ///   is now an FFT over half the history — deliberately different bytes. Resolution proof
    ///   unchanged: `resolves_two_close_tones` (tempo-core) still splits tones 40 Hz apart.
    /// - 2026-08-04: the intensity axis became dB with an absolute full-scale reference
    ///   (`tempo_core::spectrum` module header). Every value in the row changes, by design: it
    ///   was `sqrt(p / row_max)` — amplitude-linear against a reference that moved every frame —
    ///   and is now linear in dB across `DB_SPAN` below full scale. This is the operator's
    ///   "the waterfall looks so 8 bit" fix, and it matches WSJT-X, which converts to dB
    ///   unconditionally (`flat4.f90:18-20`, "If nflatten=0, convert to dB but do not flatten")
    ///   and indexes its palette linearly in dB (`plotter.cpp:194`). Axis behaviour is proved by
    ///   the three tests in `tempo_core::spectrum`; this digest only pins that it does not drift.

    #[test]
    fn the_row_for_a_known_input_is_byte_stable() {
        // The exact generator the golden used: a sawtooth over the analysis window.
        let saw: Vec<f32> = (0..WindowN::Balanced.n())
            .map(|i| (i % 64) as f32 / 64.0 - 0.5)
            .collect();
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(WindowN::Balanced.n() * 2));
        tap.publish_card(ring.clone(), ANALYSIS_RATE_HZ); // native rate → passthrough resampler
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        ring.push_slice(&saw);
        assert!(dsp.tick(&tap, &feed, &meters), "a row is produced");
        let row = feed.row().expect("published").row;

        assert_eq!(row.len(), 512, "bin count is part of the contract");
        let reference = reference_row();
        assert_eq!(reference.len(), 512, "the reference fixture is 512 bins");
        let (worst_bin, worst) = row
            .iter()
            .zip(&reference)
            .map(|(a, b)| (a - b).abs())
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("512 bins");
        assert!(
            worst <= ROW_TOLERANCE,
            "waterfall row drifted for a fixed input: bin {worst_bin} is off by {worst}, \
             tolerance {ROW_TOLERANCE}. A libm rounding difference is ~1e-7 here; this is not \
             that. Justify the DSP change, then regenerate \
             tests/fixtures/rxdsp_reference_row.txt on a known-good tree."
        );
    }

    // ---- THE CW ZERO-BEAT READING ----
    //
    // The measurement rides THIS tick — same thread, same capture list, same rolling window,
    // no new handle — so a CAT stall cannot freeze the zero-beat light any more than it can
    // freeze the waterfall. These drive the tick directly, with no sleeps and no threads.

    /// A window's worth of solid key-down at `pitch`, from the app's OWN tone generator.
    fn keyed_cw(pitch: f32, n: usize) -> Vec<f32> {
        let s = tempo_core::cw::morse_samples("TTTTTTTT", 5, pitch, 12_000);
        s[s.len() - n..].to_vec()
    }

    /// Feed `audio` through the tick in capture-sized chunks.
    fn feed(
        dsp: &mut RxDsp,
        tap: &RxTap,
        ring: &SpscRing,
        feed: &SpectrumFeed,
        meters: &MeterFeed,
        audio: &[f32],
    ) {
        for chunk in audio.chunks(256) {
            ring.push_slice(chunk);
            dsp.tick(tap, feed, meters);
        }
    }

    fn zero_beat_rig() -> (RxTap, Arc<SpscRing>, SpectrumFeed, MeterFeed, RxDsp) {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        (
            tap,
            ring,
            SpectrumFeed::default(),
            MeterFeed::default(),
            RxDsp::new(),
        )
    }

    #[test]
    fn it_measures_the_received_cw_tone_only_when_a_target_is_armed() {
        let (tap, ring, sp, meters, mut dsp) = zero_beat_rig();
        let audio = keyed_cw(672.0, 4096);

        // NO TARGET ARMED (not in the CW section) — the tick must not measure anything.
        feed(&mut dsp, &tap, &ring, &sp, &meters, &audio);
        assert_eq!(
            meters.cw_tone_hz(),
            None,
            "with no CW target armed there is nothing to zero-beat against"
        );

        // Arm the operator's pitch → the SAME audio now reads out.
        meters.set_cw_target_hz(Some(600.0));
        feed(&mut dsp, &tap, &ring, &sp, &meters, &audio);
        let hz = meters
            .cw_tone_hz()
            .expect("a keyed CW tone in the passband is measured");
        assert!(
            (hz - 672.0).abs() < 3.0,
            "measured {hz} Hz for a 672 Hz signal against a 600 Hz pitch"
        );
    }

    /// ⭐ THE FLYING NEEDLE. Operator report, 2026-08-29: "the little line flies around a lot in
    /// dead air and looks too active, but the on-pitch detection when CW is being sent seems
    /// accurate." Both halves are the same cause — the detector reports the loudest tone in the
    /// ±400 Hz window, and on a real band that window is never empty even when the signal being
    /// tuned is absent. The SNR floor cannot fix it (it is a budget against PURE noise, and it
    /// is already right); what separates a signal from a peak is that a signal STAYS PUT.
    #[test]
    fn a_tone_that_hops_never_reaches_the_needle_while_a_steady_one_does() {
        let (tap, ring, sp, meters, mut dsp) = zero_beat_rig();
        meters.set_cw_target_hz(Some(600.0));

        // Each burst is a loud, clean tone well above the SNR floor — every one of these WOULD
        // have been published before this change — but each sits somewhere else in the window,
        // the way successive noise peaks and distant signals do.
        for hz in [520.0f32, 780.0, 610.0, 900.0, 470.0, 840.0] {
            feed(&mut dsp, &tap, &ring, &sp, &meters, &keyed_cw(hz, 512));
        }
        assert_eq!(
            meters.cw_tone_hz(),
            None,
            "a tone in a different place every tick is not a signal being tuned; showing one is \
             the needle flying around on a dead band"
        );

        // POSITIVE CONTROL, and it is the half that matters most: the operator said the reading
        // is ACCURATE when CW is actually being sent, so the fix must not cost that. The same
        // rig, fed a tone that stays put, reads it.
        // Enough audio for the window to turn over from the hops above and the run to build —
        // about half a second, which is a couple of CW characters.
        for _ in 0..3 {
            feed(&mut dsp, &tap, &ring, &sp, &meters, &keyed_cw(640.0, 2048));
        }
        let hz = meters
            .cw_tone_hz()
            .expect("control: a tone that holds still is still measured");
        assert!(
            (hz - 640.0).abs() < 3.0,
            "and it is measured accurately: {hz} Hz for a 640 Hz signal"
        );
    }

    #[test]
    fn a_dead_band_reads_nothing_rather_than_a_confident_zero() {
        let (tap, ring, sp, meters, mut dsp) = zero_beat_rig();
        meters.set_cw_target_hz(Some(600.0));
        // Silence in the passband — no signal, so no reading, ever.
        feed(&mut dsp, &tap, &ring, &sp, &meters, &vec![0.0f32; 4096]);
        assert_eq!(meters.cw_tone_hz(), None, "silence must read as NO SIGNAL");
        // POSITIVE CONTROL: the identical rig DOES read a tone, so the None above is a
        // verdict about the audio and not a feature that never fires.
        feed(&mut dsp, &tap, &ring, &sp, &meters, &keyed_cw(640.0, 4096));
        assert!(
            meters.cw_tone_hz().is_some(),
            "control: the same tick reads a real tone"
        );
    }

    #[test]
    fn the_reading_is_held_across_keying_gaps_and_then_expires() {
        let (tap, ring, sp, meters, mut dsp) = zero_beat_rig();
        meters.set_cw_target_hz(Some(600.0));
        feed(&mut dsp, &tap, &ring, &sp, &meters, &keyed_cw(640.0, 4096));
        assert!(meters.cw_tone_hz().is_some(), "the signal is being read");

        // KEY-UP. CW is keyed, so most windows contain no tone at all; the reading must
        // survive an inter-word gap or the light would strobe on every letter. A second of
        // silence is longer than any gap at any sending speed a human uses.
        let hold = ZERO_BEAT_HOLD_TICKS as usize;
        let silence = |n: usize| vec![0.0f32; 256 * n];
        feed(&mut dsp, &tap, &ring, &sp, &meters, &silence(hold / 2));
        assert!(
            meters.cw_tone_hz().is_some(),
            "a keying gap must not blank the indicator"
        );

        // …but a signal that has genuinely stopped expires. A stale reading points the
        // operator at a station that is no longer there.
        //
        // The blank comes a little after the hold alone would say: the 171 ms analysis
        // window carries its own tone history, so the countdown does not start until the
        // audio IN it is gone. Asserted with margin rather than to the tick — the contract
        // is "it blanks", not "it blanks on tick 109".
        feed(&mut dsp, &tap, &ring, &sp, &meters, &silence(hold + 20));
        assert_eq!(
            meters.cw_tone_hz(),
            None,
            "past the hold the indicator goes blank, not stale"
        );
    }

    #[test]
    fn closing_the_capture_blanks_the_reading_immediately() {
        let (tap, ring, sp, meters, mut dsp) = zero_beat_rig();
        meters.set_cw_target_hz(Some(600.0));
        feed(&mut dsp, &tap, &ring, &sp, &meters, &keyed_cw(640.0, 4096));
        assert!(meters.cw_tone_hz().is_some(), "reading the signal");
        // No audio open at all — the same honesty the row and the level meter get.
        tap.clear_card();
        dsp.tick(&tap, &sp, &meters);
        assert_eq!(
            meters.cw_tone_hz(),
            None,
            "no capture, no reading — not even a held one"
        );
    }

    /// A full ring drops samples rather than blocking the realtime callback, and the consumer
    /// keeps producing rows regardless.
    #[test]
    fn an_overflowing_tap_never_stops_the_rows() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(1024));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let meters = MeterFeed::default();
        let mut dsp = RxDsp::new();
        // Push far more than the ring holds — push_slice must not block or panic.
        let accepted = ring.push_slice(&tone(12_000, 1500.0, 100_000));
        assert!(
            accepted < 100_000,
            "the ring is bounded and drops the excess"
        );
        assert!(
            dsp.tick(&tap, &feed, &meters),
            "and a row is still produced"
        );
    }
}
