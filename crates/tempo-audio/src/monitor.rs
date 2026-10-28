//! Headphone monitor: a live pass-through of the RX audio the decoder hears to a
//! chosen output device, so the operator can HEAR the band and diagnose levels /
//! RFI. Ships DARK — off by default; a later attended session at the rig verifies
//! latency and levels.
//!
//! The load-bearing invariant is that **the decode path must never degrade**. The
//! capture callback (which feeds the decoder) and the monitor output callback each
//! run on their own real-time audio thread. A mutex shared between them could block
//! the capture thread. So monitoring uses a wait-free single-producer /
//! single-consumer ring ([`SpscRing`]) built from atomics only: the capture thread
//! `push`es and NEVER blocks or allocates, dropping samples on overflow (a monitor
//! glitch) — the decoder is never stalled by monitoring.
//!
//! The pure pieces (the ring, the TX-device guard, the resampler) compile without
//! the `device` feature so they are unit-testable in the headless workspace build;
//! the cpal output stream lives behind `#[cfg(feature = "device")]`.

// The ring is tempo-codec's, below the engine, where the headset's rings share it; named here as
// it always was.
pub use tempo_codec::ring::SpscRing;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The TX-device guard, as a pure predicate: `true` when opening the monitor on
/// `monitor_device` would feed the received band into the rig's TX audio device
/// (`audio_out`) and thus transmit it back out. When it returns `true` the monitor
/// must NOT open.
///
/// Two devices collide when their names match case-insensitively, or when BOTH are
/// empty (each meaning "system default output", i.e. the same device). An empty
/// against a named device is treated as distinct — we cannot prove by name that a
/// named device *is* the current system default, so the guard doesn't block it.
/// SmartSDR's DAX transmit device is refused whatever `audio_out` is
/// ([`is_flex_dax_tx_device`]).
/// Resolve an empty device name ("system default output") to the ACTUAL default
/// output device's name, so the TX guard can compare real devices instead of
/// treating "" as unknowable. Returns the input unchanged when it's non-empty or
/// the host can't name a default (guard then falls back to the pure rule).
#[cfg(feature = "device")]
pub fn resolve_output_name(name: &str) -> String {
    use cpal::traits::HostTrait;
    if !name.trim().is_empty() {
        return name.to_string();
    }
    let _guard = crate::device::AUDIO_HOST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Through the crate's one fallible name reader — `to_string()` would panic where this
    // skips. See `device::cpal_device_name`.
    cpal::default_host()
        .default_output_device()
        .and_then(|d| crate::device::cpal_device_name(&d))
        .unwrap_or_default()
}

pub fn monitor_would_transmit(monitor_device: &str, audio_out: &str) -> bool {
    let m = monitor_device.trim();
    let o = audio_out.trim();
    if is_flex_dax_tx_device(m) {
        return true;
    }
    if m.is_empty() && o.is_empty() {
        return true; // both "system default output" → the same device
    }
    if m.is_empty() || o.is_empty() {
        return false;
    }
    m.eq_ignore_ascii_case(o)
}

/// Whether `name` is SmartSDR's DAX transmit device on Windows: "DAX Audio TX (FlexRadio Systems
/// DAX TX)" as SmartSDR's DAX documents it, or "DAX TX (FlexRadio DAX)" as newer DAX drivers name
/// the live endpoint. What plays on it goes to a Flex's transmitter, whatever Nexus's own TX audio
/// device is (with native Flex audio it is not that device at all), so [`monitor_would_transmit`]
/// refuses it as it refuses the TX device. Matched on the name's start, as a whole word, case-blind:
/// the DAX receive and IQ devices, and a device that only mentions DAX, are not it.
pub(crate) fn is_flex_dax_tx_device(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    ["dax audio tx", "dax tx"].iter().any(|p| {
        n.strip_prefix(p)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric()))
    })
}

/// Nearest-neighbour (sample-and-hold) mono resampler driven by an [`SpscRing`],
/// converting the ring's `in_rate` samples to the output device's `out_rate`. Used
/// only when the monitor output device cannot open at the capture rate; at equal
/// rates it pops exactly one sample per output frame (a straight pass-through).
///
/// Pure and allocation-free — it holds only a phase accumulator and the last sample,
/// so it can run in the real-time output callback. Underruns emit silence.
pub struct MonoResampler {
    acc: f32,
    last: f32,
    in_rate: f32,
    out_rate: f32,
}

impl MonoResampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            acc: 0.0,
            last: 0.0,
            in_rate: in_rate.max(1) as f32,
            out_rate: out_rate.max(1) as f32,
        }
    }

    /// The next output-rate mono sample, consuming from `ring` as needed. On
    /// underrun (a needed sample is missing) it yields `0.0` (silence).
    pub fn next(&mut self, ring: &SpscRing) -> f32 {
        self.next_from(|| ring.pop())
    }

    /// [`Self::next`], consuming from `pop` (`None` = nothing to play: silence).
    fn next_from(&mut self, mut pop: impl FnMut() -> Option<f32>) -> f32 {
        self.acc += self.in_rate;
        while self.acc >= self.out_rate {
            self.acc -= self.out_rate;
            self.last = pop().unwrap_or(0.0);
        }
        self.last
    }
}

/// The headphone monitor's way in for a receive source that is not the sound card's capture
/// callback: Nexus's TCI client, whose receive audio arrives over the network on its session's
/// thread and is what the decoders hear while Audio over TCI is on, or native Flex audio, which the
/// radio loop takes from Nexus's Flex client (#372: the monitor kept playing the idle sound card,
/// so "Play receive audio here" was silent).
///
/// It is a second [`SpscRing`] of the monitor's, at the monitor's own rate, with the source of the
/// moment (the TCI session's thread, or the radio loop for the Flex) as its one producer and the
/// monitor's output callback as its one consumer, so neither ring ever has two producers: a source
/// lets go of the feed before another takes it, even a TCI session a switch keeps alive. The output plays this ring while the feed is active and the
/// card's otherwise ([`Playout`]). It shares the card path's two gates: nothing is written unless
/// the operator's monitor is on (the TX-device guard has already refused a collision before the
/// monitor turns on) and the monitor is not muted for a transmission.
#[derive(Clone)]
pub struct MonitorFeed {
    ring: Arc<SpscRing>,
    rate: u32,
    enabled: Arc<AtomicBool>,
    tx_mute: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
}

impl MonitorFeed {
    /// A feed at `rate` sharing the monitor's two gates, inactive.
    pub fn new(rate: u32, enabled: Arc<AtomicBool>, tx_mute: Arc<AtomicBool>) -> MonitorFeed {
        MonitorFeed {
            // ~0.5 s, as the card's monitor ring.
            ring: Arc::new(SpscRing::new((rate as usize / 2).max(4096))),
            rate,
            enabled,
            tx_mute,
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The rate the monitor plays this feed at: what a source resamples to before [`Self::push`].
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Whether `other` is this same feed (the same monitor), not another backend's.
    pub fn is(&self, other: &MonitorFeed) -> bool {
        Arc::ptr_eq(&self.ring, &other.ring)
    }

    /// Play this feed in the sound card's place, or the sound card again. The radio loop's.
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Whether audio pushed now would be played: the feed is the monitor's source, the operator's
    /// monitor is on, and it is not muted for a transmission.
    pub fn open(&self) -> bool {
        self.is_active()
            && self.enabled.load(Ordering::Acquire)
            && !self.tx_mute.load(Ordering::Acquire)
    }

    /// Producer: queue `samples` (at [`Self::rate`]) for the monitor. Never blocks; a full ring
    /// drops, as the card's does. The caller checks [`Self::open`] first.
    pub fn push(&self, samples: &[f32]) {
        self.ring.push_slice(samples);
    }

    /// Queued samples, for tests and diagnostics.
    pub fn queued(&self) -> usize {
        self.ring.len()
    }

    /// Consumer: the oldest queued sample, as the monitor's output takes it. For tests.
    #[cfg(all(test, feature = "device"))]
    pub(crate) fn pop(&self) -> Option<f32> {
        self.ring.pop()
    }

    /// Discard what is queued, with no output reading it (the monitor's stream gone).
    #[cfg(feature = "device")]
    fn clear(&self) {
        self.ring.clear();
    }

    /// Ask the output to discard what is queued now, at the top of its next callback
    /// ([`Playout::fill`]), on a keying edge (`Monitor::set_tx_mute`). The ring's read side is the
    /// output callback's alone: a clear from the loop thread racing its read can lose, and then
    /// the whole jitter buffer of audio from before the key plays into the over. The request
    /// records how far the queue reached, so audio queued after it is kept. The loop is not this
    /// ring's producer for TCI audio: an older reading of how far it reached only discards less,
    /// never audio that is not queued.
    #[cfg(feature = "device")]
    fn request_flush(&self) {
        self.ring.request_flush();
    }
}

/// How much of a [`MonitorFeed`]'s audio the monitor holds before it plays it, and again after it
/// ran dry. The feed's audio arrives over the network in bursts (a TCI block is about 43 ms, and
/// LAN or Wi-Fi timing wanders by tens of milliseconds more) while the output takes it at the
/// device's steady pace, so playing from an empty start turns every late burst into a gap of hard
/// zeros: clicks and dropouts. The sound card's own capture needs none of this: its producer and
/// the output are both device-clocked.
pub const FEED_PREROLL_MS: usize = 100;
/// The most of a feed's audio the monitor lets queue. Past it (a burst after a stall, or a source
/// whose clock runs a little fast) the oldest goes, down to the pre-roll, so the headphones never
/// fall further and further behind the band.
pub const FEED_MAX_MS: usize = 300;

/// The monitor output's consumer side: which ring it plays each callback, and the feed's jitter
/// buffer. The output callback owns it and is both rings' one consumer.
///
/// While the feed is not active it plays the sound card's ring exactly as it always did (the feed's
/// is emptied). While the feed is active it empties the card's, holds silence until
/// [`FEED_PREROLL_MS`] of the feed's audio is queued, plays it, and when the feed runs dry goes
/// back to holding silence until the pre-roll is queued again, rather than stuttering through every
/// burst that arrives late. Nothing a source queued before a switch plays after it.
pub struct Playout {
    rs: MonoResampler,
    /// Whether the feed's pre-roll is queued and its audio is playing.
    primed: bool,
    preroll: usize,
    max: usize,
}

impl Playout {
    /// The consumer for rings at `in_rate`, played at `out_rate`.
    pub fn new(in_rate: u32, out_rate: u32) -> Playout {
        let ms = |n: usize| (in_rate as usize * n / 1_000).max(1);
        Playout {
            rs: MonoResampler::new(in_rate, out_rate),
            primed: false,
            preroll: ms(FEED_PREROLL_MS),
            max: ms(FEED_MAX_MS),
        }
    }

    /// One output callback: `frames` mono samples at the output rate, handed to `out` in order.
    /// Wait-free and allocation-free: the rings' atomics and the resampler's phase only.
    pub fn fill(
        &mut self,
        card: &SpscRing,
        feed: &MonitorFeed,
        frames: usize,
        mut out: impl FnMut(f32),
    ) {
        // A keying edge's discard, asked for by the loop (`MonitorFeed::request_flush`).
        feed.ring.apply_flush();
        if !feed.is_active() {
            feed.ring.clear();
            self.primed = false;
            for _ in 0..frames {
                out(self.rs.next(card));
            }
            return;
        }
        card.clear();
        let ring = &*feed.ring;
        let queued = ring.len();
        if queued > self.max {
            for _ in 0..queued - self.preroll {
                ring.pop();
            }
        }
        if !self.primed && ring.len() >= self.preroll {
            self.primed = true;
        }
        let Playout { rs, primed, .. } = self;
        for _ in 0..frames {
            out(rs.next_from(|| {
                if !*primed {
                    return None;
                }
                let s = ring.pop();
                // Dry: silence until the pre-roll is queued again.
                *primed = s.is_some();
                s
            }));
        }
    }
}

#[cfg(feature = "device")]
pub use device_monitor::Monitor;

#[cfg(feature = "device")]
mod device_monitor {
    use super::{MonitorFeed, Playout, SpscRing};
    use crate::device::{dispatch_format, AUDIO_HOST_LOCK};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;

    fn err_fn(e: cpal::Error) {
        eprintln!("tempo-audio: monitor stream error: {e}");
    }

    /// Owns the dark headphone-monitor output stream and the state it shares with
    /// the capture callback. Lives inside [`crate::device::CpalBackend`] on the
    /// radio-loop thread (a cpal `Stream` is `!Send`). Reconfigured in place by
    /// [`Monitor::apply`] — it NEVER touches the capture/TX streams, so toggling the
    /// monitor can't restart or degrade the decode path.
    pub struct Monitor {
        /// The RX samples the capture callback pushes (at `in_rate`, mono).
        ring: Arc<SpscRing>,
        /// Gates the capture-callback push: cleared → the callback skips the ring.
        enabled: Arc<AtomicBool>,
        /// Set while the rig is KEYED. The capture callback treats it as a second gate, so the
        /// monitor goes silent for the duration of a transmission and the operator does not hear
        /// the rig's own delayed MONI over their own voice. Deliberately NOT `enabled`: that is
        /// the operator's setting and must come back unchanged when the over ends.
        tx_mute: Arc<AtomicBool>,
        /// Playback level as `f32` bits, read live by the output callback.
        level_bits: Arc<AtomicU32>,
        /// The capture rate the ring is filled at (the monitor output opens here
        /// when the device supports it; otherwise it resamples).
        in_rate: u32,
        /// The live monitor output stream (`None` = monitor off).
        out_stream: Option<Stream>,
        /// The device name `out_stream` targets ("" = system default), so a device
        /// change rebuilds only the output stream.
        active_device: String,
        /// The way in for a receive source other than the capture callback (Nexus's TCI client),
        /// at `in_rate`, on the same two gates; the output plays it while it is active.
        feed: MonitorFeed,
    }

    impl Monitor {
        pub fn new(
            ring: Arc<SpscRing>,
            enabled: Arc<AtomicBool>,
            tx_mute: Arc<AtomicBool>,
            level_bits: Arc<AtomicU32>,
            in_rate: u32,
        ) -> Self {
            let feed = MonitorFeed::new(in_rate, enabled.clone(), tx_mute.clone());
            Self {
                ring,
                enabled,
                tx_mute,
                level_bits,
                in_rate,
                out_stream: None,
                active_device: String::new(),
                feed,
            }
        }

        /// The monitor's way in for a receive source other than the sound card's capture.
        pub fn feed(&self) -> MonitorFeed {
            self.feed.clone()
        }

        /// Reconfigure the monitor in place. `enabled` is the guard-resolved decision
        /// (the caller has already refused a TX-device collision). Starts, stops, or
        /// retunes the output stream without disturbing capture. `Err` = the output
        /// device failed to open.
        /// Drop the monitor's output stream, freeing its device. **The caller MUST already
        /// hold [`AUDIO_HOST_LOCK`]** — this deliberately does not take it, because
        /// `std::sync::Mutex` is not reentrant and the whole point is to release every
        /// stream the backend owns inside ONE critical section
        /// (`AudioBackend::release_device`). Use [`Self::apply`] with `enabled: false`
        /// anywhere else; that one locks for you.
        /// Mute or unmute for a transmission. Called from the radio loop on every keying change.
        ///
        /// On the RISING edge the ring is cleared as well as gated: whatever the capture pushed in
        /// the moments before the key went down is band audio from before the over, and playing it
        /// out when the over ends would be a small burst of the past. Cheap — `clear` is two atomic
        /// stores — and it runs on the loop thread, never in an audio callback. Another source's
        /// queue (TCI, native Flex audio) holds up to its jitter buffer, so its discard is left to
        /// the output callback (`MonitorFeed::request_flush`), which cannot lose a race to itself.
        pub fn set_tx_mute(&mut self, muted: bool) {
            let was = self.tx_mute.swap(muted, Ordering::Release);
            if muted && !was {
                self.ring.clear();
                self.feed.request_flush();
            }
        }

        pub fn release_locked(&mut self) {
            self.enabled.store(false, Ordering::Release);
            self.out_stream = None;
            self.active_device.clear();
            self.ring.clear();
            self.feed.clear();
        }
        pub fn apply(&mut self, enabled: bool, device: &str, level: f32) -> Result<(), String> {
            self.level_bits
                .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            if !enabled {
                self.enabled.store(false, Ordering::Release);
                if self.out_stream.is_some() {
                    // Tear the stream down under the host lock (native device-graph
                    // teardown shares the same non-reentrant state as construction).
                    let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                    self.out_stream = None;
                }
                self.active_device.clear();
                self.ring.clear();
                self.feed.clear();
                return Ok(());
            }
            if self.out_stream.is_none() || self.active_device != device {
                let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                self.out_stream = None; // drop the old stream (frees the device) first
                self.ring.clear();
                self.feed.clear();
                let stream = build_output(
                    device,
                    self.in_rate,
                    &self.ring,
                    &self.feed,
                    &self.level_bits,
                )?;
                self.out_stream = Some(stream);
                self.active_device = device.to_string();
            }
            self.enabled.store(true, Ordering::Release);
            Ok(())
        }
    }

    /// Can [`build_output`] actually build a stream in this sample format?
    ///
    /// ⭐ ONE LIST NOW, not two. This used to be a hand-maintained `matches!` here whose comment
    /// said it "IS `build_output`'s match arms and has to stay in step with them" — and it had
    /// already drifted from `device.rs`, which carried the same four formats with no list at all
    /// and no way for the monitor's picker to know. Both now ask
    /// [`crate::device::supported_device_format`], and both build through the one
    /// `dispatch_format!`, so a format is carried on every path or refused on every path.
    fn buildable_format(f: SampleFormat) -> bool {
        crate::device::supported_device_format(f)
    }

    /// Pick an output config at `want_rate` from what the device offers (so the ring's
    /// samples play straight through with no resampling), else `None`.
    ///
    /// ⚠️ THE FORMAT CHOICE IS THE WHOLE POINT (#8). This took the FIRST range whose rate span
    /// covered `want_rate` with no regard for its sample format, and on ALSA the first format a
    /// rig codec offers is `I8` — which [`build_output`] could not build, so the monitor died with
    /// "unsupported monitor output format: I8" on a card whose audio the DECODE path was opening
    /// happily the whole time. The decode path never hit this because it takes
    /// `default_output_config()`, the card's own PREFERRED format, and never enumerates. Picking
    /// nothing is safe: `build_output` then falls back to that same default config.
    ///
    /// ⭐ AND IT PICKS THE BEST, NOT THE FIRST. Every format is buildable now
    /// ([`crate::device::DeviceSample`]), so a "skip what I cannot build" filter would no longer
    /// skip `I8` at all — it would OPEN in `I8` on a card that also offered `I16`, turning a fixed
    /// bug into an 8-bit monitor. Where there is a choice, [`crate::device::format_quality_rank`]
    /// makes it, and it is our ranking rather than cpal's so it cannot move under a dependency bump.
    fn output_config_at_rate(
        configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
        want_rate: u32,
    ) -> Option<cpal::SupportedStreamConfig> {
        configs
            .filter(|range| {
                buildable_format(range.sample_format())
                    && range.min_sample_rate() <= want_rate
                    && want_rate <= range.max_sample_rate()
            })
            .max_by_key(|range| crate::device::format_quality_rank(range.sample_format()))
            // FALLIBLE: cpal 0.18's `with_sample_rate` PANICS out of range. The filter above
            // already guarantees the range covers `want_rate`, so this cannot return None — using
            // the checked form anyway costs nothing and removes a panic path from the open path.
            .and_then(|range| range.try_with_sample_rate(want_rate))
    }

    /// The monitor's output callback, once, for every sample format.
    ///
    /// Wait-free by construction: it pops the capture callback's [`SpscRing`] through a
    /// [`MonoResampler`] that holds only a phase accumulator, so neither audio thread can block the
    /// other. An underrun is silence, never a stall.
    #[allow(clippy::too_many_arguments)]
    fn build_monitor_stream<T: crate::device::DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: &cpal::StreamConfig,
        out_ch: usize,
        in_rate: u32,
        out_rate: u32,
        ring: &Arc<SpscRing>,
        feed: &MonitorFeed,
        level_bits: &Arc<AtomicU32>,
    ) -> Result<Stream, String> {
        let ring_cb = ring.clone();
        let feed_cb = feed.clone();
        let level_cb = level_bits.clone();
        let mut playout = Playout::new(in_rate, out_rate);
        dev.build_output_stream(
            // `StreamConfig` is `Copy` in 0.18 and taken BY VALUE.
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let level = f32::from_bits(level_cb.load(Ordering::Relaxed));
                let n = data.len().div_ceil(out_ch);
                let mut frames = data.chunks_mut(out_ch);
                // The capture's ring, or another source's while it is the station's (TCI audio,
                // native Flex audio), through its jitter buffer.
                playout.fill(&ring_cb, &feed_cb, n, |s| {
                    // Clamped HERE, on every format including f32: unlike the TX path this is a
                    // product of two operator-set gains and can genuinely exceed unity.
                    let v = T::from_unit((s * level).clamp(-1.0, 1.0));
                    if let Some(frame) = frames.next() {
                        for x in frame.iter_mut() {
                            *x = v;
                        }
                    }
                });
            },
            err_fn,
            None,
        )
        .map_err(|e| e.to_string())
    }

    /// Build and start the monitor output stream on `device_name` ("" = default).
    /// The caller MUST already hold [`AUDIO_HOST_LOCK`]. Opens at `in_rate` when the
    /// device supports it (pure pass-through); otherwise falls back to the device
    /// default rate and nearest-neighbour resamples in the callback.
    ///
    /// A NAMED device that will not resolve is an `Err` (already non-fatal — it gets its
    /// own error lane in `service.rs`), never the system default. This one is a TX-safety
    /// edge, not just honesty: `monitor_would_transmit` guards by comparing NAMES, so a
    /// silent fallback to the system default could land on the device that IS the rig's TX
    /// output and monitor the received band straight back onto the air, with the guard
    /// none the wiser because the name it checked was never the device that opened.
    fn build_output(
        device_name: &str,
        in_rate: u32,
        ring: &Arc<SpscRing>,
        feed: &MonitorFeed,
        level_bits: &Arc<AtomicU32>,
    ) -> Result<Stream, String> {
        let host = cpal::default_host();
        let name = (!device_name.trim().is_empty()).then_some(device_name);
        let dev = crate::device::resolve_configured(
            || host.output_devices().ok(),
            name,
            host.default_output_device(),
            "monitor output",
        )?;
        let supported = dev
            .supported_output_configs()
            .ok()
            .and_then(|c| output_config_at_rate(c, in_rate))
            .or_else(|| dev.default_output_config().ok())
            .ok_or("no monitor output config")?;
        let out_rate = supported.sample_rate();
        let out_ch = supported.channels() as usize;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();

        let stream = dispatch_format!(
            sample_format,
            build_monitor_stream,
            "monitor output",
            &dev,
            &config,
            out_ch.max(1),
            in_rate,
            out_rate,
            ring,
            feed,
            level_bits,
        )?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(stream)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use cpal::{SupportedBufferSize, SupportedStreamConfigRange};

        fn range(fmt: SampleFormat, lo: u32, hi: u32) -> SupportedStreamConfigRange {
            SupportedStreamConfigRange::new(2, lo, hi, SupportedBufferSize::Unknown, fmt)
        }

        /// ⭐ THE FORMAT-COVERAGE OUTAGE. Every audio path in this crate handles exactly four of
        /// the ten sample formats cpal 0.15 can report, and anything else is refused — the monitor
        /// refuses to open, and on the DECODE path the same gap fails `CpalBackend::open` outright,
        /// which is no audio at all. That is already live (a card whose default config is `U16` or
        /// `F64` gets nothing today), and the cpal 0.18 upgrade widens it: 0.18 adds `I24`/`U24`
        /// and re-ranks `default_*_config()` so `F64`, `U32`, `I24` and `U24` all outrank the `I16`
        /// we do handle. A macOS enumeration fix must not become a Windows and Linux outage, so the
        /// coverage is closed FIRST, on 0.15, for every format that exists there.
        #[test]
        fn every_sample_format_the_host_can_report_is_carried() {
            // The six that were missing. `U16` and `F64` are the plausible ones today; the rest
            // close the class rather than leave it open at a different width.
            for fmt in [
                SampleFormat::I8,
                SampleFormat::U16,
                SampleFormat::U32,
                SampleFormat::I64,
                SampleFormat::U64,
                SampleFormat::F64,
            ] {
                assert!(
                    buildable_format(fmt),
                    "{fmt:?} is a format cpal can hand us, and refusing it is an audio outage"
                );
            }
            // ...and the four that always worked still do — the positive control, since a
            // `buildable_format` that simply returned true would pass the loop above.
            for fmt in [
                SampleFormat::F32,
                SampleFormat::I16,
                SampleFormat::U8,
                SampleFormat::I32,
            ] {
                assert!(buildable_format(fmt), "{fmt:?} regressed");
            }
        }

        /// ⭐ #8 ON THE WIRE: *"unsupported format: I8"* on the very card whose audio the decoder
        /// was already reading. ALSA offers `I8` FIRST, the picker took the first range whose
        /// rate span fitted, and `build_output` then had no arm for it — so the monitor refused to
        /// open a device the decode path opens every session.
        ///
        /// The fix has since moved on twice. `I8` is now BUILDABLE, so "skip what I cannot build"
        /// would let the picker take it — and hand the operator an 8-bit monitor on a card that
        /// also offered `I16`. The rule is now "take the BEST buildable one", which lands on the
        /// same answer here for a better reason.
        #[test]
        fn the_picker_takes_the_best_format_not_the_first() {
            let offered = [
                range(SampleFormat::I8, 8_000, 48_000), // what ALSA offers first
                range(SampleFormat::I16, 8_000, 48_000),
            ];
            let picked = output_config_at_rate(offered.into_iter(), 12_000)
                .expect("a buildable format was on offer");
            assert_eq!(
                picked.sample_format(),
                SampleFormat::I16,
                "the picker must pass over I8 for the better format offered alongside it"
            );
            assert_eq!(picked.sample_rate(), 12_000, "still at the ring's rate");

            // ...and it reaches PAST a mediocre one for the best, not merely past the worst.
            let offered = [
                range(SampleFormat::I8, 8_000, 48_000),
                range(SampleFormat::I16, 8_000, 48_000),
                range(SampleFormat::F32, 8_000, 48_000),
            ];
            assert_eq!(
                output_config_at_rate(offered.into_iter(), 12_000)
                    .unwrap()
                    .sample_format(),
                SampleFormat::F32,
            );

            // I8 ALONE is now opened rather than refused — the coverage half of the change. An
            // 8-bit monitor is poor; silence and an error banner are worse.
            assert_eq!(
                output_config_at_rate([range(SampleFormat::I8, 8_000, 48_000)].into_iter(), 12_000)
                    .unwrap()
                    .sample_format(),
                SampleFormat::I8,
            );
        }

        /// The other direction, and the one that says the filter is a FILTER rather than a
        /// blanket refusal: every format `build_output` handles is still pickable, and the
        /// rate span is still what decides between them.
        #[test]
        fn every_buildable_format_is_still_picked_and_the_rate_span_still_decides() {
            for fmt in [
                SampleFormat::F32,
                SampleFormat::I16,
                SampleFormat::U8,
                SampleFormat::I32,
            ] {
                let picked = output_config_at_rate([range(fmt, 8_000, 48_000)].into_iter(), 12_000)
                    .unwrap_or_else(|| panic!("{fmt:?} is buildable and must be pickable"));
                assert_eq!(picked.sample_format(), fmt);
            }
            // A buildable format whose span does NOT cover the rate is still passed over.
            assert!(
                output_config_at_rate(
                    [range(SampleFormat::I16, 44_100, 48_000)].into_iter(),
                    12_000
                )
                .is_none(),
                "a range that cannot reach the ring's rate is not a candidate"
            );
        }

        /// Nothing USABLE on offer is `None`, NOT a config that would fail to build —
        /// `build_output` then falls back to the card's own default config, which is exactly the
        /// route the decode path takes and the reason rig audio opens at all.
        ///
        /// The format half of this is now unreachable BY CONSTRUCTION on cpal 0.15 — every format
        /// it can report is buildable — so what is left to test is the rate half. The arm still
        /// matters: `SampleFormat` is `#[non_exhaustive]`, and cpal 0.18 adds DSD variants that
        /// cannot be carried by scaling and must never be picked.
        #[test]
        fn nothing_usable_on_offer_yields_none_so_the_default_config_is_used() {
            assert!(
                output_config_at_rate(
                    [range(SampleFormat::I16, 44_100, 48_000)].into_iter(),
                    12_000
                )
                .is_none(),
                "no range reaches the ring's rate → fall back to the card's default config"
            );
            assert!(
                output_config_at_rate(std::iter::empty(), 12_000).is_none(),
                "nothing on offer at all"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "device")]
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    #[cfg(feature = "device")]
    use std::sync::Arc;

    /// A transmission must not be played back to the operator, and what was buffered before it
    /// must not surface afterwards.
    ///
    /// Drives the REAL thing — `Monitor::set_tx_mute` and the atomics the capture callback reads.
    /// The first version of this test did not: it built a `SpscRing`, called `clear()` and checked
    /// the ring was empty, which `SpscRing::clear` already has its own test for and which passes
    /// with this whole feature deleted (kd9taw caught it on #158). A test that cannot fail when the
    /// feature is removed is not evidence of the feature.
    ///
    /// `Monitor` needs no audio device to construct: `new` takes the shared `Arc`s and
    /// `out_stream` starts `None`.
    #[cfg(feature = "device")]
    #[test]
    fn muting_for_a_transmission_closes_the_gate_and_discards_the_audio_that_preceded_it() {
        let ring = Arc::new(SpscRing::new(64));
        let enabled = Arc::new(AtomicBool::new(true));
        let tx_mute = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let mut m = Monitor::new(
            ring.clone(),
            enabled.clone(),
            tx_mute.clone(),
            level.clone(),
            48_000,
        );

        for _ in 0..16 {
            assert!(
                ring.push(0.5),
                "fixture: band audio buffered before the over"
            );
        }

        // THE RISING EDGE. The gate the capture callback reads closes, and the audio captured
        // before the key went down goes with it — otherwise it plays out as a burst of the past
        // when the over ends, which is a stranger artefact than the one this fixes.
        m.set_tx_mute(true);
        assert!(tx_mute.load(Ordering::Relaxed), "the gate closed");
        assert_eq!(ring.len(), 0, "and the pre-TX audio went with it");

        // EDGE-TRIGGERED, which is a claim the design makes and therefore has to be held to:
        // muting again mid-over must not wipe anything a second time.
        assert!(ring.push(0.25), "something arrives while still keyed");
        m.set_tx_mute(true);
        assert_eq!(ring.len(), 1, "muting again is not a second clear");

        // AND THE OPERATOR'S OWN SETTING IS UNTOUCHED THROUGHOUT. This is the invariant the whole
        // design rests on — the reason the mute is a second atomic rather than borrowing
        // `enabled` — and nothing held us to it before.
        m.set_tx_mute(false);
        assert!(
            !tx_mute.load(Ordering::Relaxed),
            "the gate reopens when the over ends"
        );
        assert!(
            enabled.load(Ordering::Relaxed),
            "the operator's monitor setting survives a transmission unchanged"
        );
    }

    /// Another source's audio (TCI) queued for the monitor is discarded on the keying edge too, and
    /// its writes see the same mute gate as the capture callback.
    #[cfg(feature = "device")]
    #[test]
    fn muting_for_a_transmission_also_discards_another_sources_audio() {
        let enabled = Arc::new(AtomicBool::new(true));
        let tx_mute = Arc::new(AtomicBool::new(false));
        let mut m = Monitor::new(
            Arc::new(SpscRing::new(64)),
            enabled,
            tx_mute,
            Arc::new(AtomicU32::new(0.5f32.to_bits())),
            48_000,
        );
        let feed = m.feed();
        feed.set_active(true);
        assert!(feed.open(), "control: on, unmuted, the source");
        // More than the pre-roll, so the output would play it at once.
        let preroll = 48_000 * super::FEED_PREROLL_MS / 1_000;
        feed.push(&vec![0.5; preroll + 480]);
        m.set_tx_mute(true);
        assert!(!feed.open(), "the mute closes another source's gate too");
        // The discard is the output callback's to make: the ring's read side is its alone, and a
        // clear from the loop thread racing its read can lose, replaying the whole jitter buffer
        // of pre-key audio into the over.
        assert_eq!(
            feed.queued(),
            preroll + 480,
            "the loop thread moved the output's read side"
        );
        let card = SpscRing::new(16);
        let mut p = Playout::new(48_000, 48_000);
        let mut played = Vec::new();
        p.fill(&card, &feed, 480, |s| played.push(s));
        assert!(
            played.iter().all(|s| *s == 0.0),
            "audio from before the key played in the over"
        );
        assert_eq!(feed.queued(), 0, "and its pre-TX audio went with it");
        // Audio after the over is the band's now: none of it is eaten by the discard.
        m.set_tx_mute(false);
        assert!(feed.open(), "reopened after the over");
        feed.push(&vec![0.25; preroll + 480]);
        let mut played = Vec::new();
        p.fill(&card, &feed, 480, |s| played.push(s));
        assert!(
            played.iter().all(|s| *s == 0.25),
            "the audio after the over did not play"
        );
    }

    /// A feed at 1 kHz (so a millisecond is a sample), on and unmuted, and the output's consumer
    /// at the same rate: a pre-roll of 100 samples, at most 300 queued.
    fn feed_and_playout() -> (MonitorFeed, Playout) {
        let feed = MonitorFeed::new(
            1_000,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        (feed, Playout::new(1_000, 1_000))
    }

    /// One output callback of `frames`: what it played.
    fn callback(p: &mut Playout, card: &SpscRing, feed: &MonitorFeed, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        p.fill(card, feed, frames, |s| out.push(s));
        out
    }

    /// ⭐ THE OUTPUT PLAYS THE SOURCE THE STATION HEARS. With no other source active it plays the
    /// capture's ring as it always did (the card path unchanged: no pre-roll, sample for sample);
    /// while one is active it plays that one's, and each switch empties the ring it leaves, so
    /// nothing from before a switch plays after it.
    #[test]
    fn the_monitor_plays_the_active_source_and_the_card_otherwise() {
        let card = SpscRing::new(16);
        let (feed, mut p) = feed_and_playout();
        card.push_slice(&[0.1, 0.2]);
        feed.push(&[0.9]);
        assert_eq!(
            callback(&mut p, &card, &feed, 1),
            [0.1],
            "inactive: the card's audio at once, in order"
        );
        assert_eq!(feed.queued(), 0, "the inactive source's audio was dropped");
        feed.set_active(true);
        feed.push(&[0.7; 100]);
        assert_eq!(
            callback(&mut p, &card, &feed, 1),
            [0.7],
            "active: the other source's audio"
        );
        assert!(card.is_empty(), "the card's leftover went at the switch");
        feed.set_active(false);
        card.push(0.3);
        assert_eq!(callback(&mut p, &card, &feed, 1), [0.3], "the card again");
        assert_eq!(feed.queued(), 0);
    }

    /// ⭐ NETWORK JITTER IS NOT A GAP IN THE HEADPHONES. A source's audio arrives in 43 ms blocks
    /// whose arrival wanders by up to 70 ms (each one on time on average), while the output takes
    /// 10 ms a callback at the device's steady pace. Once the pre-roll is queued, every sample
    /// played is the band's: no hole of silence anywhere. The control: the same feed played from an
    /// empty start with no pre-roll (what the monitor did before) is full of holes.
    #[test]
    fn a_feed_arriving_irregularly_plays_without_gaps() {
        let run = |preroll: bool| {
            let card = SpscRing::new(16);
            let (feed, mut p) = feed_and_playout();
            if !preroll {
                p.preroll = 1;
            }
            feed.set_active(true);
            // Block k is due at 43k ms and arrives up to 70 ms late, in order.
            let jitter = [0, 70, 10, 65, 0, 40, 70, 5, 55, 20, 70, 0, 30, 60];
            let mut arrive: Vec<usize> = Vec::new();
            for k in 0..140 {
                let at = 43 * k + jitter[k % jitter.len()];
                arrive.push(at.max(arrive.last().copied().unwrap_or(0)));
            }
            let mut played = Vec::new();
            let mut next = 0;
            for now in (0..43 * 140).step_by(10) {
                while next < arrive.len() && arrive[next] <= now {
                    feed.push(&[0.5; 43]);
                    next += 1;
                }
                played.extend(callback(&mut p, &card, &feed, 10));
            }
            let first = played.iter().position(|&s| s != 0.0).expect("audio played");
            played[first..played.len() - 200]
                .iter()
                .filter(|&&s| s == 0.0)
                .count()
        };
        assert!(
            run(false) > 0,
            "control: with no pre-roll the jitter is heard as holes"
        );
        assert_eq!(
            run(true),
            0,
            "holes in the monitor's audio after the pre-roll"
        );
    }

    /// A feed that runs dry (the source stalled) holds silence until the pre-roll is queued again,
    /// rather than playing each scrap the moment it arrives; then it plays on.
    #[test]
    fn a_feed_that_ran_dry_waits_for_the_pre_roll_again() {
        let card = SpscRing::new(16);
        let (feed, mut p) = feed_and_playout();
        feed.set_active(true);
        feed.push(&[0.5; 99]);
        assert!(
            callback(&mut p, &card, &feed, 10).iter().all(|&s| s == 0.0),
            "short of the pre-roll: silence"
        );
        feed.push(&[0.5]);
        assert!(
            callback(&mut p, &card, &feed, 100)
                .iter()
                .all(|&s| s == 0.5),
            "the pre-roll queued: it plays, all of it"
        );
        assert_eq!(callback(&mut p, &card, &feed, 1), [0.0], "dry");
        feed.push(&[0.5; 50]);
        assert!(
            callback(&mut p, &card, &feed, 10).iter().all(|&s| s == 0.0),
            "a scrap after running dry waits for the rest of the pre-roll"
        );
        feed.push(&[0.5; 50]);
        assert_eq!(
            callback(&mut p, &card, &feed, 10),
            [0.5; 10],
            "and plays on"
        );
    }

    /// ⛔ THE HEADPHONES NEVER FALL FURTHER AND FURTHER BEHIND. A source whose clock runs 2% fast
    /// for ten minutes, and a 600 ms burst after a stall, never leave more than the jitter buffer's
    /// ceiling queued after a callback: the oldest goes, down to the pre-roll.
    #[test]
    fn the_jitter_buffer_is_bounded() {
        let card = SpscRing::new(16);
        let (feed, mut p) = feed_and_playout();
        feed.set_active(true);
        let mut most = 0;
        for i in 0..60_000 {
            feed.push(&[0.5; 10]);
            if i % 50 == 0 {
                feed.push(&[0.5]);
            }
            let _ = callback(&mut p, &card, &feed, 10);
            most = most.max(feed.queued());
        }
        assert!(most <= FEED_MAX_MS, "{most} ms queued from a fast clock");
        feed.push(&[0.5; 600]);
        let _ = callback(&mut p, &card, &feed, 10);
        assert!(
            feed.queued() <= FEED_PREROLL_MS,
            "a burst caught up to the pre-roll"
        );
    }
    use super::*;

    #[test]
    fn guard_blocks_same_device_and_both_default() {
        // Both empty = both "system default output" → the same device → blocked.
        assert!(monitor_would_transmit("", ""));
        // Same name (case-insensitive) → blocked.
        assert!(monitor_would_transmit("USB Audio CODEC", "usb audio codec"));
        assert!(monitor_would_transmit(" Speakers ", "Speakers"));
        // Distinct devices → allowed.
        assert!(!monitor_would_transmit("Headphones", "USB Audio CODEC"));
        // One default, one named → cannot prove collision by name → allowed.
        assert!(!monitor_would_transmit("Headphones", ""));
        assert!(!monitor_would_transmit("", "USB Audio CODEC"));
    }

    /// SmartSDR's DAX transmit device carries what is played on it to a Flex's transmitter, whatever
    /// Nexus's own TX audio device is (with native Flex audio, it is not that device at all): the
    /// monitor never opens on it. Its names as SmartSDR's DAX documents it, and as newer DAX
    /// drivers name the live endpoint (see the DAX pairing in Settings).
    #[test]
    fn guard_blocks_the_flex_dax_transmit_device() {
        for dax_tx in [
            "DAX Audio TX (FlexRadio Systems DAX TX)",
            "DAX Audio TX",
            "DAX TX (FlexRadio DAX)",
            " dax tx ",
        ] {
            assert!(
                monitor_would_transmit(dax_tx, "Speakers"),
                "{dax_tx:?} let through"
            );
            assert!(monitor_would_transmit(dax_tx, ""), "{dax_tx:?} let through");
        }
        // The receive devices, and devices that only mention DAX, are headphones like any other.
        for other in [
            "DAX Audio RX 1 (FlexRadio Systems DAX Audio)",
            "DAX RESERVED AUDIO RX 3",
            "DAX IQ RX 1",
            "Headphones (DAX TX helper)",
            "DAX TXT",
        ] {
            assert!(
                !monitor_would_transmit(other, "USB Audio CODEC"),
                "{other:?} refused"
            );
        }
    }

    #[test]
    fn resampler_equal_rate_is_passthrough() {
        let r = SpscRing::new(16);
        r.push_slice(&[0.1, 0.2, 0.3, 0.4]);
        let mut rs = MonoResampler::new(48_000, 48_000);
        // One pop per output sample, in order.
        assert!((rs.next(&r) - 0.1).abs() < 1e-6);
        assert!((rs.next(&r) - 0.2).abs() < 1e-6);
        assert!((rs.next(&r) - 0.3).abs() < 1e-6);
        assert!((rs.next(&r) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn resampler_underrun_is_silence() {
        let r = SpscRing::new(16); // empty
        let mut rs = MonoResampler::new(48_000, 48_000);
        assert_eq!(
            rs.next(&r),
            0.0,
            "no audio available → silence, never a hang"
        );
    }

    #[test]
    fn resampler_downsamples_by_dropping() {
        // 4:1 decimation holds the most-recent of each group of input samples.
        let r = SpscRing::new(16);
        r.push_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        let mut rs = MonoResampler::new(48_000, 12_000);
        assert_eq!(rs.next(&r), 4.0, "consumed 4 inputs, held the last");
        assert_eq!(rs.next(&r), 8.0);
    }
}
