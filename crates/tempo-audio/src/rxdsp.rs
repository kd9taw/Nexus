//! The waterfall row producer, on its own thread.
//!
//! This is the consumer half of [`crate::rxtap`] — read that module's header for why the row
//! moved off the radio loop. The one-line version: the loop is the only thread that issues
//! blocking CAT (up to 2500 ms on slow serial), and it was also the only producer of spectrum
//! rows, so every CAT stall froze the waterfall.
//!
//! THE SAFETY ARGUMENT IS THE CAPTURE LIST. [`RxDsp::tick`] takes an `&RxTap` and a
//! `&SpectrumFeed` and nothing else, and the thread in [`spawn`] closes over exactly those two.
//! No `Rig`, no `CatDaemon`, no `CpalBackend`, no `Engine`. A CAT call cannot be issued from
//! here because nothing here can name one. That is a compile-time property, not a policy.
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
use tempo_app::engine::SpectrumFeed;

use crate::capture_resample::CaptureResampler;
use crate::rxtap::RxTap;

/// Modem/analysis rate the rolling window is kept at (matches the decode path).
const ANALYSIS_RATE: f32 = tempo_core::tempo_fast::SAMPLE_RATE;
/// Same rate as an integer, for the resampler and the test tones.
const ANALYSIS_RATE_HZ: u32 = ANALYSIS_RATE as u32;
/// Rolling window the FFT runs over. Same 4096 the radio loop used, so the row is identical.
const WINDOW: usize = 4096;
/// Consumer cadence. Matches the radio loop's own 20 ms tick, so row rate is unchanged.
pub const TICK_MS: u64 = 20;

/// Rolling-window state for the spectrum producer. Split out from the thread so tests can drive
/// it deterministically, with no sleeps and no threads.
pub struct RxDsp {
    rs: CaptureResampler,
    window: Vec<f32>,
    epoch: u64,
    rate: u32,
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
            window: Vec::with_capacity(WINDOW * 2),
            epoch: 0,
            rate: ANALYSIS_RATE_HZ,
        }
    }

    /// Drain whatever the capture callback has teed, and publish a row.
    ///
    /// Returns true when a row was published. Takes only the tap and the feed — see the module
    /// header; this signature IS the safety argument.
    pub fn tick(&mut self, tap: &RxTap, feed: &SpectrumFeed) -> bool {
        let Some(src) = tap.current() else {
            // No audio open. Publish an EMPTY row rather than nothing: an empty row makes the
            // UI stop cleanly, whereas publishing nothing sends the reader down the engine-lock
            // fallback, which returns a stale NON-empty row and reproduces the streak.
            feed.publish_audio(empty_row());
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
            return false; // nothing new this tick; the last row stands (it is <2 s fresh)
        }
        let pcm = self.rs.process(&dev);
        if pcm.is_empty() {
            return false;
        }
        self.window.extend_from_slice(&pcm);
        if self.window.len() > WINDOW {
            let drop = self.window.len() - WINDOW;
            self.window.drain(0..drop);
        }
        feed.publish_audio(compute_row(&self.window));
        true
    }
}

/// The audio-FFT row, byte-for-byte the computation the radio loop used to do.
fn compute_row(audio: &[f32]) -> Spectrum {
    // Widened from 200-2900: shows the full SSB/DATA passband and the filter-slope shelf. The
    // 6 kHz Nyquist (12 kHz capture) caps the top; 4000 covers every voice/data passband in use.
    const LO_HZ: f32 = 0.0;
    const HI_HZ: f32 = 4000.0;
    const BINS: usize = 512;
    let row = if audio.is_empty() {
        Vec::new()
    } else {
        tempo_core::spectrum::power_spectrum(audio, ANALYSIS_RATE, LO_HZ, HI_HZ, BINS)
    };
    Spectrum {
        row,
        lo_hz: f64::from(LO_HZ),
        hi_hz: f64::from(HI_HZ),
        source: "audio".into(),
    }
}

fn empty_row() -> Spectrum {
    compute_row(&[])
}

/// Spawn the producer thread. Panic-wrapped: a silent death here would kill the waterfall with
/// no other symptom, which is exactly the silent-fallback failure mode the TX-safety notes warn
/// about, so the panic is surfaced rather than swallowed.
pub fn spawn(tap: Arc<RxTap>, feed: SpectrumFeed) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("nexus-rx-dsp".into())
        .spawn(move || {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut dsp = RxDsp::new();
                loop {
                    dsp.tick(&tap, &feed);
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
        let mut dsp = RxDsp::new();

        let mut published = 0;
        for _ in 0..120 {
            ring.push_slice(&tone(12_000, 1500.0, 256));
            if dsp.tick(&tap, &feed) {
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
    }

    #[test]
    fn a_tone_lands_in_the_right_bin_at_the_native_rate() {
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(48_000));
        tap.publish_card(ring.clone(), 12_000);
        let feed = SpectrumFeed::default();
        let mut dsp = RxDsp::new();
        ring.push_slice(&tone(12_000, 1500.0, WINDOW * 2));
        for _ in 0..4 {
            dsp.tick(&tap, &feed);
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
        let mut dsp = RxDsp::new();
        ring.push_slice(&tone(44_100, 1500.0, 44_100 / 2));
        for _ in 0..8 {
            dsp.tick(&tap, &feed);
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
        let mut dsp = RxDsp::new();

        let a = Arc::new(SpscRing::new(200_000));
        tap.publish_card(a.clone(), 44_100);
        a.push_slice(&tone(44_100, 800.0, 44_100 / 2));
        for _ in 0..8 {
            dsp.tick(&tap, &feed);
        }

        let b = Arc::new(SpscRing::new(48_000));
        tap.publish_card(b.clone(), 12_000);
        b.push_slice(&tone(12_000, 2500.0, WINDOW * 2));
        for _ in 0..4 {
            dsp.tick(&tap, &feed);
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
        let mut dsp = RxDsp::new();
        assert!(!dsp.tick(&tap, &feed), "nothing to publish");
        let row = feed.row().expect("an EMPTY row is still published");
        assert!(
            row.row.is_empty(),
            "and it is empty, so the UI stops cleanly"
        );
    }

    /// VALUE PIN, relocated from the `watch_identity` golden.
    ///
    /// That golden used to assert the waterfall row byte-for-byte for a known input. The row is
    /// no longer produced by the engine, so the assertion moved here with it — losing it would
    /// have quietly dropped the only check that the DSP output itself does not drift. Same
    /// arithmetic input (a deterministic sawtooth at the modem rate), digest over the whole row.
    ///
    /// A DIFF HERE IS A DSP BEHAVIOUR CHANGE. Justify it, then update the digest — do not
    /// loosen the assertion.
    #[test]
    fn the_row_for_a_known_input_is_byte_stable() {
        // The exact generator the golden used: a sawtooth over the analysis window.
        let saw: Vec<f32> = (0..WINDOW).map(|i| (i % 64) as f32 / 64.0 - 0.5).collect();
        let tap = RxTap::new();
        let ring = Arc::new(SpscRing::new(WINDOW * 2));
        tap.publish_card(ring.clone(), ANALYSIS_RATE_HZ); // native rate → passthrough resampler
        let feed = SpectrumFeed::default();
        let mut dsp = RxDsp::new();
        ring.push_slice(&saw);
        assert!(dsp.tick(&tap, &feed), "a row is produced");
        let row = feed.row().expect("published").row;

        assert_eq!(row.len(), 512, "bin count is part of the contract");
        // FNV-1a over the row's bit patterns — a compact byte-identity check.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in &row {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        assert_eq!(
            h, 8_281_068_622_764_297_801,
            "waterfall row digest drifted for a fixed input"
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
        let mut dsp = RxDsp::new();
        // Push far more than the ring holds — push_slice must not block or panic.
        let accepted = ring.push_slice(&tone(12_000, 1500.0, 100_000));
        assert!(
            accepted < 100_000,
            "the ring is bounded and drops the excess"
        );
        assert!(dsp.tick(&tap, &feed), "and a row is still produced");
    }
}
