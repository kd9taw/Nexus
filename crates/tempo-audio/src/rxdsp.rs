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

/// Rolling-window state for the spectrum producer. Split out from the thread so tests can drive
/// it deterministically, with no sleeps and no threads.
pub struct RxDsp {
    rs: CaptureResampler,
    window: Vec<f32>,
    epoch: u64,
    rate: u32,
    /// The ballistics-shaped RX level (0..1 RMS) published to the meter feed each tick.
    meter: f32,
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
        }
    }

    /// Drain whatever the capture callback has teed, publish a row, and publish the RX level.
    ///
    /// Returns true when a row was published. Takes only the tap and the two publish seams —
    /// see the module header; this signature IS the safety argument.
    pub fn tick(&mut self, tap: &RxTap, feed: &SpectrumFeed, meters: &MeterFeed) -> bool {
        let Some(src) = tap.current() else {
            // No audio open. Publish an EMPTY row rather than nothing: an empty row makes the
            // UI stop cleanly, whereas publishing nothing sends the reader down the engine-lock
            // fallback, which returns a stale NON-empty row and reproduces the streak. The
            // meter goes to zero for the same honesty: no capture, no level.
            feed.publish_audio(empty_row());
            self.meter = 0.0;
            meters.set_rx_level(0.0);
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
        // The rig scope's own window, when one is on screen asking for it. One extra 2048-point
        // FFT (~30 us of a 20 ms budget) and no extra bytes — see `compute_row_over`. The
        // request expires on its own, so this costs nothing when no scope is up.
        if let Some((lo, hi, win)) = feed.scope_request() {
            feed.publish_scope(compute_row_over(&self.window, lo, hi, win));
        }
        true
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
