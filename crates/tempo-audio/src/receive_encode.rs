//! Opus encoding of the station's receive audio, for a remote listener.
//!
//! One reader on [`ReceiveAudioFeed`] → resample to an Opus-legal rate → 20 ms
//! CELT-only packets, each carrying a sequence number and the capture mark of its
//! first sample. **This module knows nothing about a transport.** It hands a caller
//! frames; turning them into a wire message is a separate layer, so swapping the
//! relay for a direct connection later changes nothing here.
//!
//! Nothing in the tree constructs a [`ReceiveEncoder`] yet — no browser, no relay,
//! no operator control reaches this code — so an idle station encodes nothing at
//! all. That is not an accident of wiring: the feed itself copies no samples until
//! a reader exists (`receive_audio.rs`), so the cost of the whole path while nobody
//! is listening is zero.
//!
//! ## What this must never do to the RX DSP thread
//!
//! The feed's contract is "a slow or contended consumer loses media instead of
//! delaying the spectrum or decoder", held up by a `try_lock` in the producer. This
//! module keeps its half: every sample it encodes is an owned copy taken out of the
//! feed by [`ReceiveAudioReader::read`], and no lock is held across a codec call.
//! An encoder that falls behind therefore loses audio — the designed outcome — and
//! never stalls the thread that also drives the waterfall and the decoders.
//!
//! ## The codec configuration, and why each line is what it is
//!
//! * **[`opus::Application::LowDelay`] — CELT is forced, not hoped for.**
//!   `Application::Audio` does *not* guarantee CELT: it sits a few kbit/s from a
//!   runtime mode threshold with ±4000 of hysteresis and can flip mid-stream, and
//!   `OPUS_SET_SIGNAL(VOICE)` overrides the application outright and forces SILK.
//!   Only `RESTRICTED_LOWDELAY` hard-forces CELT-only.
//!   **Why that matters here and nowhere else:** SILK reproduces a steady CW tone
//!   essentially perfectly, so tone damage is not the concern. SILK *flatters the
//!   noise floor*, measured at a consistent ~3 dB at every SNR while CELT tracks the
//!   uncoded signal within 0.1 dB. A remote operator's whole task is judging a signal
//!   and tuning by ear; a codec that quietly improves the noise floor by 3 dB is
//!   falsifying the thing they are listening for. **Never call `set_signal`, and
//!   never "improve" this to `Application::Audio` or `Application::Voip`.**
//! * **20 ms frames.** At ~24 kbit/s, 10 ms → 20 ms measured 6× less amplitude
//!   ripple, 7× less frequency wander and 17 dB less harmonic noise on a CW tone.
//!   It is the single largest CW-quality lever in this configuration, and it is also
//!   the RX DSP tick (`rxdsp.rs:49`), so one publication is one frame at 48 kHz.
//! * **24 kbit/s.** There is a sharp knee between 20 and 21 kbit/s; 24 sits above it
//!   with margin. [`MIN_BITRATE_BPS`] is a floor, not a preference.
//! * **DTX off.** Discontinuous transmission stops sending during pauses, which a
//!   later transport gap rule would read as a dead link. It is off by default; this
//!   module states it anyway so the intent survives an edit.
//! * **In-band FEC is deliberately NOT set.** It is a SILK-layer feature and is inert
//!   in a CELT-only stream. Setting it would read as loss handling that is not there;
//!   loss concealment belongs to the transport layer, not to a codec flag.
//! * **[`ENCODE_RATE_HZ`] = 24 kHz.** Opus accepts 8/12/16/24/48 kHz only, so the
//!   device rate never goes in raw. 24 kHz is an exact 2:1 decimation of the common
//!   48 kHz card, and it is the lowest legal rate at which the wideband cap below is
//!   even reachable (12 kHz input clamps Opus to mediumband). The resampler is the
//!   existing anti-aliased [`CaptureResampler`], whose cutoff caps at 4.5 kHz — above
//!   any rig's audio passband and well inside 24 kHz Nyquist — and it runs on the
//!   reader's own thread, never on the RX DSP thread.
//!
//! Neural extensions (deep PLC, DRED, OSCE) are off: they are non-default
//! `opusic-sys` features and must stay that way — see the crate's Cargo.toml.

use std::sync::Arc;
use std::time::Instant;

use crate::capture_resample::CaptureResampler;
use crate::receive_audio::{ReceiveAudioFeed, ReceiveAudioReader, ReceiveError, ReceiveSource};

/// One Opus frame, in milliseconds. Also the RX DSP tick.
pub const FRAME_MS: u64 = 20;
/// The Opus-legal rate the feed is resampled to before encoding.
pub const ENCODE_RATE_HZ: u32 = 24_000;
/// Samples per encoded frame at [`ENCODE_RATE_HZ`].
pub const FRAME_SAMPLES: usize = (ENCODE_RATE_HZ as usize) * (FRAME_MS as usize) / 1000;
/// Target bit rate. The "tuning by ear" quality class.
pub const BITRATE_BPS: i32 = 24_000;
/// Hard floor, not a preference: below ~21 kbit/s CW quality steps off a knee.
/// Nothing here may be budgeted below this.
pub const MIN_BITRATE_BPS: i32 = 22_000;
/// Encoder complexity. This runs once per 20 ms on a station PC, not on a phone.
const COMPLEXITY: i32 = 10;
/// Packet buffer ceiling. A 20 ms frame at 24 kbit/s is ~60 bytes; this is room for
/// any VBR excursion and still far inside every byte bound a transport imposes.
const MAX_PACKET_BYTES: usize = 400;

/// One encoded 20 ms frame, ready for a transport to bundle and send.
///
/// `capture_ms` is milliseconds on the station's **monotonic** clock, measured from
/// this encoder's first publication. It exists for relative spacing inside one
/// stream and for nothing else: it is not a wall clock, it is not comparable across
/// encoders, and a receiver must never use it to judge freshness — freshness is
/// transport-measured residence, because the shack clock may simply be wrong.
/// A hole in the feed shows up here as a jump, which is the point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFrame {
    /// Dense and monotonic within one epoch, starting at 0.
    pub seq: u32,
    /// The capture source's generation. Changes only when the source does — and a
    /// source change ends this encoder, so every frame from one encoder shares it.
    pub epoch: u64,
    /// Monotonic capture mark of this frame's first sample, in milliseconds.
    pub capture_ms: u64,
    /// Stated, never inferred from the payload length.
    pub frame_ms: u16,
    /// One Opus packet (RFC 6716 §3), exactly one frame.
    pub packet: Vec<u8>,
}

#[derive(Debug)]
pub enum EncodeError {
    /// The feed ended this reader: a device change, a closed source, or the seam
    /// taken by someone else. **Terminal** — build a new encoder against the new
    /// source; this one can never resume.
    Feed(ReceiveError),
    /// libopus refused. Terminal for this encoder.
    Codec(opus::Error),
    /// 2^32 frames — about 2.7 years of continuous listening. A wrapped sequence
    /// would read to a receiver as a decrease, which it must treat as a replay, so
    /// this ends the stream honestly instead.
    SequenceExhausted,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Feed(err) => write!(f, "receive audio feed ended: {err:?}"),
            Self::Codec(err) => write!(f, "opus: {err}"),
            Self::SequenceExhausted => f.write_str("frame sequence exhausted"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// A single reader on the receive feed, encoding it to Opus.
///
/// Construction *is* the subscription: there is exactly one reader per feed, so a
/// second `start` on a live feed is refused with [`ReceiveError::InUse`] and the
/// first listener is not disturbed.
pub struct ReceiveEncoder {
    reader: ReceiveAudioReader,
    resampler: CaptureResampler,
    encoder: opus::Encoder,
    source: ReceiveSource,
    /// Resampled samples not yet a whole frame. Bounded by one publication.
    pending: Vec<f32>,
    /// Capture mark of `pending[0]`, milliseconds since `origin`.
    pending_at_ms: u64,
    /// The first publication this encoder saw — the zero of `capture_ms`.
    origin: Option<Instant>,
    seq: u32,
    frames: u32,
    bytes: u64,
    finished: bool,
}

impl ReceiveEncoder {
    /// Subscribe to `feed` and build the encoder. `expected` must be the source the
    /// caller already observed; the feed refuses a stale or foreign one, so no
    /// caller can select another input by handing over a different rate or epoch.
    pub fn start(
        feed: &Arc<ReceiveAudioFeed>,
        expected: ReceiveSource,
    ) -> Result<Self, EncodeError> {
        let reader = feed.subscribe(expected).map_err(EncodeError::Feed)?;
        Ok(Self {
            reader,
            resampler: CaptureResampler::new(expected.rate, ENCODE_RATE_HZ),
            encoder: build_encoder()?,
            source: expected,
            pending: Vec::with_capacity(FRAME_SAMPLES * 2),
            pending_at_ms: 0,
            origin: None,
            seq: 0,
            frames: 0,
            bytes: 0,
            finished: false,
        })
    }

    /// The capture source this encoder is bound to for its whole life.
    pub fn source(&self) -> ReceiveSource {
        self.source
    }

    /// Frames emitted so far. Zero while nothing is being heard.
    pub fn frames_encoded(&self) -> u32 {
        self.frames
    }

    /// Payload bytes emitted so far — the measurement that says whether an idle
    /// station is costing anything.
    pub fn encoded_bytes(&self) -> u64 {
        self.bytes
    }

    /// Drain everything the feed is holding and encode whole frames out of it.
    ///
    /// Returns an empty vector when there is nothing new, which is the normal case
    /// on a station nobody is transmitting to. Never blocks on the producer: `read`
    /// takes the feed lock only long enough to move one owned block out, and the
    /// encode happens afterwards on samples this encoder owns.
    ///
    /// On [`EncodeError::Feed`] the encoder is finished. Whatever was buffered is
    /// dropped rather than emitted: the feed has already retired the reader and
    /// cleared its blocks, and splicing the tail of a dead source onto a new one is
    /// exactly the two-sample-rate smear the epoch exists to prevent.
    pub fn poll(&mut self, now: Instant) -> Result<Vec<EncodedFrame>, EncodeError> {
        if self.finished {
            return Err(EncodeError::Feed(ReceiveError::Ended));
        }
        let mut frames = Vec::new();
        loop {
            let block = match self.reader.read(now) {
                Ok(Some(block)) => block,
                Ok(None) => break,
                Err(err) => {
                    self.finish();
                    return Err(EncodeError::Feed(err));
                }
            };
            self.absorb(&block);
            while self.pending.len() >= FRAME_SAMPLES {
                match self.emit() {
                    Ok(frame) => frames.push(frame),
                    Err(err) => {
                        self.finish();
                        return Err(err);
                    }
                }
            }
        }
        Ok(frames)
    }

    fn finish(&mut self) {
        self.finished = true;
        self.pending.clear();
    }

    /// Resample one publication into `pending`, and re-anchor the capture mark off
    /// it.
    ///
    /// The anchor is recomputed from *this* block every time rather than advanced by
    /// a counter, and that is the whole trick: the samples already pending were
    /// captured exactly their own duration before this block was published, so
    /// `published_at − pending` is the mark of `pending[0]` whether or not the feed
    /// dropped anything in between. When it did drop something — which it is
    /// entitled to do, media being cheaper than a stalled DSP thread — the mark
    /// jumps by the size of the hole instead of quietly claiming the stream ran on.
    fn absorb(&mut self, block: &crate::receive_audio::ReceiveBlock) {
        let origin = *self.origin.get_or_insert(block.published_at);
        let published_ms = block
            .published_at
            .saturating_duration_since(origin)
            .as_millis() as u64;
        let pending_ms = (self.pending.len() as u64) * 1000 / u64::from(ENCODE_RATE_HZ);
        self.pending_at_ms = published_ms.saturating_sub(pending_ms);
        self.pending.extend(self.resampler.process(&block.samples));
    }

    fn emit(&mut self) -> Result<EncodedFrame, EncodeError> {
        let frame: Vec<f32> = self.pending.drain(..FRAME_SAMPLES).collect();
        let packet = self
            .encoder
            .encode_vec_float(&frame, MAX_PACKET_BYTES)
            .map_err(EncodeError::Codec)?;
        let seq = self.seq;
        self.seq = seq.checked_add(1).ok_or(EncodeError::SequenceExhausted)?;
        self.frames = self.frames.saturating_add(1);
        self.bytes = self.bytes.saturating_add(packet.len() as u64);
        let capture_ms = self.pending_at_ms;
        self.pending_at_ms += FRAME_MS;
        Ok(EncodedFrame {
            seq,
            epoch: self.source.epoch,
            capture_ms,
            frame_ms: FRAME_MS as u16,
            packet,
        })
    }

    /// The live encoder, for tests that pin the configuration by reading it back.
    #[cfg(test)]
    fn encoder_mut(&mut self) -> &mut opus::Encoder {
        &mut self.encoder
    }
}

/// Run `samples` (device-rate mono, at `rate`) through the real feed and encoder at
/// the 20 ms RX DSP tick, and return every frame produced.
///
/// **Measurement harness, not a production path** — the station's encoder is fed by
/// the RX DSP thread, and the feed's producer is deliberately crate-private so
/// nothing outside can inject audio into it. This exists so a fidelity or bit-rate
/// measurement runs against the code that actually ships instead of against a
/// re-implementation of it, which is the whole difference between measuring the
/// artifact and measuring a proxy for it. See `examples/remote_audio_frames.rs`.
/// A [`ReceiveAudioFeed`] that is attached to no capture device, with the only way to
/// push samples into it.
///
/// **Not a production path, and it cannot become one.** It builds its OWN feed, so it
/// can never inject audio into the station's real tee — the producer there stays
/// crate-private, which is the whole point of `receive_audio`'s seam. It exists so a
/// consumer in another crate (the Remote audio lane) can be tested against the real
/// feed, the real encoder and real libopus instead of against a re-implementation of
/// all three, which is the difference between measuring the artifact and measuring a
/// proxy for it.
#[doc(hidden)]
pub struct DetachedFeed {
    pub feed: Arc<ReceiveAudioFeed>,
    source: ReceiveSource,
}

impl DetachedFeed {
    pub fn new(rate: u32) -> Self {
        let feed = Arc::new(ReceiveAudioFeed::default());
        let source = ReceiveSource { epoch: 1, rate };
        feed.replace_source(Some(source));
        Self { feed, source }
    }
    pub fn source(&self) -> ReceiveSource {
        self.source
    }
    pub fn publish(&self, at: Instant, samples: &[f32]) {
        self.feed.publish(self.source, at, samples);
    }
    /// Retire the source, exactly as a capture device change does. Every reader ends.
    pub fn end(&self) {
        self.feed.replace_source(None);
    }
    /// Replace the source with a new generation, as reopening a device does.
    pub fn restart(&mut self, rate: u32) {
        self.source = ReceiveSource {
            epoch: self.source.epoch + 1,
            rate,
        };
        self.feed.replace_source(Some(self.source));
    }
}

pub fn encode_offline(rate: u32, samples: &[f32]) -> Result<Vec<EncodedFrame>, EncodeError> {
    let feed = Arc::new(ReceiveAudioFeed::default());
    let source = ReceiveSource { epoch: 1, rate };
    feed.replace_source(Some(source));
    let mut encoder = ReceiveEncoder::start(&feed, source)?;

    let tick_samples = (rate as usize) * (FRAME_MS as usize) / 1000;
    let tick = std::time::Duration::from_millis(FRAME_MS);
    let base = Instant::now();
    let mut frames = Vec::new();
    for (k, chunk) in samples.chunks(tick_samples.max(1)).enumerate() {
        let at = base + tick * (k as u32);
        feed.publish(source, at, chunk);
        frames.extend(encoder.poll(at)?);
    }
    Ok(frames)
}

/// The one place the codec is configured. Read the module header before changing a
/// line of it — every setting here is load-bearing for what the operator hears.
fn build_encoder() -> Result<opus::Encoder, EncodeError> {
    const _: () = assert!(BITRATE_BPS >= MIN_BITRATE_BPS);
    let mut encoder = opus::Encoder::new(
        ENCODE_RATE_HZ,
        opus::Channels::Mono,
        // CELT-only, forced. NOT `Audio` — see the module header.
        opus::Application::LowDelay,
    )
    .map_err(EncodeError::Codec)?;
    encoder
        .set_expert_frame_duration(opus::FrameSize::Ms20)
        .map_err(EncodeError::Codec)?;
    encoder
        .set_bitrate(opus::Bitrate::Bits(BITRATE_BPS))
        .map_err(EncodeError::Codec)?;
    encoder
        .set_complexity(COMPLEXITY)
        .map_err(EncodeError::Codec)?;
    encoder
        .set_max_bandwidth(opus::Bandwidth::Wideband)
        .map_err(EncodeError::Codec)?;
    // Stated rather than left to the default, so a later edit has to argue with it.
    encoder.set_dtx(false).map_err(EncodeError::Codec)?;
    // Deliberately NOT set: `set_signal` (Voice forces SILK) and `set_inband_fec`
    // (a SILK feature, inert in CELT-only — loss is the transport's problem).
    Ok(encoder)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::receive_audio::MAX_MEDIA_AGE;
    use std::sync::mpsc;
    use std::time::Duration;

    const DEVICE_RATE: u32 = 48_000;
    const TICK: Duration = Duration::from_millis(FRAME_MS);
    /// One 20 ms RX DSP tick of device-rate mono samples (`rxdsp.rs:49` TICK_MS).
    const TICK_SAMPLES: usize = (DEVICE_RATE as usize) * (FRAME_MS as usize) / 1000;

    fn fixture() -> (Arc<ReceiveAudioFeed>, ReceiveSource, Instant) {
        let feed = Arc::new(ReceiveAudioFeed::default());
        let source = ReceiveSource {
            epoch: 3,
            rate: DEVICE_RATE,
        };
        feed.replace_source(Some(source));
        (feed, source, Instant::now())
    }

    /// A deterministic 700 Hz CW-style tone on pseudo-noise — broadband enough that
    /// the encoder has to spend real bits, and reproducible so a bit-rate band means
    /// something. `k` is the tick index, so successive calls are phase-continuous.
    fn tick_signal(k: usize) -> Vec<f32> {
        let mut noise: u32 = 0x1234_5678 ^ (k as u32).wrapping_mul(2_654_435_761);
        (0..TICK_SAMPLES)
            .map(|i| {
                let n = k * TICK_SAMPLES + i;
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let hiss = ((noise >> 8) as f32 / 8_388_608.0) - 1.0;
                let keyed = if (n / (DEVICE_RATE as usize / 10)).is_multiple_of(2) {
                    1.0
                } else {
                    0.0
                };
                let phase = 2.0 * std::f32::consts::PI * 700.0 * (n as f32) / (DEVICE_RATE as f32);
                0.20 * keyed * phase.sin() + 0.05 * hiss
            })
            .collect()
    }

    fn silence() -> Vec<f32> {
        vec![0.0; TICK_SAMPLES]
    }

    /// Publish `n` ticks at the 20 ms cadence, polling after each one — the way the
    /// station's own loop would. Returns every frame produced, in order.
    fn run_ticks(
        feed: &Arc<ReceiveAudioFeed>,
        source: ReceiveSource,
        encoder: &mut ReceiveEncoder,
        base: Instant,
        n: usize,
        mut sample: impl FnMut(usize) -> Vec<f32>,
    ) -> Vec<EncodedFrame> {
        let mut frames = Vec::new();
        for k in 0..n {
            let at = base + TICK * (k as u32);
            feed.publish(source, at, &sample(k));
            frames.extend(encoder.poll(at).expect("the feed is alive"));
        }
        frames
    }

    /// Opus TOC byte (RFC 6716 §3.1), unpacked. `config` 16..=31 is CELT-only;
    /// within it `(config - 16) % 4 == 3` is the 20 ms frame and `(config - 16) / 4`
    /// is the bandwidth (0 NB, 1 WB, 2 SWB, 3 FB).
    fn toc(packet: &[u8]) -> (u8, bool, u8) {
        let t = packet[0];
        (t >> 3, (t >> 2) & 1 == 1, t & 3)
    }

    #[test]
    fn the_configuration_pins_celt_only_twenty_millisecond_mono_frames() {
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();

        // The CTLs, read back off the live encoder rather than off our own constants.
        let enc = encoder.encoder_mut();
        assert_eq!(enc.get_application().unwrap(), opus::Application::LowDelay);
        assert_eq!(
            enc.get_expert_frame_duration().unwrap(),
            opus::FrameSize::Ms20
        );
        assert!(!enc.get_dtx().unwrap(), "DTX must stay off");
        assert_eq!(
            enc.get_signal().unwrap(),
            opus::Signal::Auto,
            "Signal::Voice would override the application and force SILK"
        );
        assert_eq!(enc.get_max_bandwidth().unwrap(), opus::Bandwidth::Wideband);
        assert_eq!(
            enc.get_bitrate().unwrap(),
            opus::Bitrate::Bits(BITRATE_BPS),
            "the ~24 kbit/s class, above the 22 kbit/s knee"
        );

        // …and the same facts read off a real packet, which a wrong CTL cannot fake.
        let frames = run_ticks(&feed, source, &mut encoder, base, 12, tick_signal);
        assert!(!frames.is_empty(), "twelve ticks must produce frames");
        for frame in &frames {
            let (config, stereo, code) = toc(&frame.packet);
            assert!(
                config >= 16,
                "config {config} is a SILK or hybrid packet, not CELT-only"
            );
            assert_eq!((config - 16) % 4, 3, "config {config} is not a 20 ms frame");
            assert!(
                (config - 16) / 4 <= 1,
                "config {config} exceeds the wideband cap"
            );
            assert!(!stereo, "the feed is mono and the wire must stay mono");
            assert_eq!(code, 0, "one Opus frame per packet");
            assert_eq!(frame.frame_ms, FRAME_MS as u16);
        }
    }

    #[test]
    fn dtx_off_keeps_sending_through_a_dead_band_and_dtx_on_would_not() {
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        let quiet = run_ticks(&feed, source, &mut encoder, base, 60, |_| silence());
        assert!(quiet.len() >= 50, "produced {} frames", quiet.len());
        assert!(
            quiet.iter().all(|f| f.packet.len() > 2),
            "a packet of 2 bytes or fewer is a DTX no-data frame; \
             a later gap rule would read the silence as a dead link"
        );

        // Positive control on the very same measurement: turn DTX on and it must trip.
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        encoder.encoder_mut().set_dtx(true).unwrap();
        let dtx = run_ticks(&feed, source, &mut encoder, base, 60, |_| silence());
        assert!(
            dtx.iter().any(|f| f.packet.len() <= 2),
            "control did not trip: DTX on produced no no-data frame, \
             so the check above proves nothing"
        );
    }

    #[test]
    fn the_bit_rate_lands_in_the_expected_band_on_a_known_input() {
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        let frames = run_ticks(&feed, source, &mut encoder, base, 150, tick_signal);
        assert!(frames.len() >= 140, "produced {} frames", frames.len());
        let bytes: usize = frames.iter().map(|f| f.packet.len()).sum();
        let bits_per_second = (bytes * 8 * 1000) as f64 / (frames.len() as f64 * FRAME_MS as f64);
        assert!(
            (18_000.0..30_000.0).contains(&bits_per_second),
            "measured {bits_per_second:.0} bit/s, outside the ~24 kbit/s class"
        );
        assert_eq!(encoder.encoded_bytes(), bytes as u64);
        assert_eq!(encoder.frames_encoded() as usize, frames.len());
    }

    #[test]
    fn frames_carry_a_monotonic_sequence_and_a_capture_mark_that_follows_the_feed() {
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        let frames = run_ticks(&feed, source, &mut encoder, base, 20, tick_signal);
        assert!(frames.len() >= 15, "produced {} frames", frames.len());
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(frame.seq, i as u32, "sequence must be dense and monotonic");
            assert_eq!(frame.epoch, source.epoch);
        }
        for pair in frames.windows(2) {
            let step = pair[1].capture_ms - pair[0].capture_ms;
            assert!(
                (19..=21).contains(&step),
                "contiguous audio must advance the capture mark by one frame, saw {step} ms"
            );
        }

        // A publication that does not continue the audio moves the mark by the real
        // gap, so the far end can see the hole instead of being told the stream ran on.
        let resumed = base + TICK * 20 + Duration::from_millis(160);
        feed.publish(source, resumed, &tick_signal(20));
        feed.publish(source, resumed + TICK, &tick_signal(21));
        let after = encoder.poll(resumed + TICK).unwrap();
        assert!(!after.is_empty(), "the stream must resume");
        let jump = after[0].capture_ms - frames.last().unwrap().capture_ms;
        assert!(
            jump >= 100,
            "a 160 ms hole must show in the capture mark, saw {jump} ms"
        );
    }

    #[test]
    fn a_device_change_ends_the_encoder_cleanly() {
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        let before = run_ticks(&feed, source, &mut encoder, base, 10, tick_signal);
        assert!(!before.is_empty());

        feed.replace_source(Some(ReceiveSource {
            epoch: source.epoch + 1,
            rate: 44_100,
        }));
        let at = base + TICK * 10;
        assert!(matches!(
            encoder.poll(at),
            Err(EncodeError::Feed(ReceiveError::Ended))
        ));
        // Terminal, not transient: the caller rebuilds against the new source, and the
        // stale one can never be resubscribed.
        assert!(matches!(
            encoder.poll(at),
            Err(EncodeError::Feed(ReceiveError::Ended))
        ));
        assert!(matches!(
            ReceiveEncoder::start(&feed, source),
            Err(EncodeError::Feed(ReceiveError::SourceChanged))
        ));
        drop(encoder);
        let next = ReceiveSource {
            epoch: source.epoch + 1,
            rate: 44_100,
        };
        assert!(ReceiveEncoder::start(&feed, next).is_ok());
    }

    #[test]
    fn an_idle_station_encodes_nothing() {
        let (feed, source, base) = fixture();
        // Nothing has asked for audio, so the producer's publications are not even copied.
        for k in 0..25 {
            feed.publish(source, base + TICK * k, &tick_signal(k as usize));
        }
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        let at = base + TICK * 25;
        for _ in 0..25 {
            assert!(encoder.poll(at).unwrap().is_empty());
        }
        assert_eq!(encoder.frames_encoded(), 0);
        assert_eq!(
            encoder.encoded_bytes(),
            0,
            "an idle station must spend no codec work and produce no bytes"
        );

        // Positive control: the same accounting must move the moment audio arrives.
        let frames = run_ticks(&feed, source, &mut encoder, at, 10, tick_signal);
        assert!(!frames.is_empty(), "control did not trip");
        assert!(encoder.encoded_bytes() > 0);
    }

    #[test]
    fn polling_at_the_tick_loses_no_audio_while_a_stalled_encoder_does() {
        const TICKS: usize = 40;

        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        let polled = run_ticks(&feed, source, &mut encoder, base, TICKS, tick_signal);
        // One tick is the feed's `primed` discard; one more is the resampler's
        // 64-tap warm-up. Everything else must survive.
        assert!(
            polled.len() >= TICKS - 3,
            "polled at the tick, kept only {} of {TICKS}",
            polled.len()
        );

        // Positive control on the same accounting: an encoder that does not poll must
        // LOSE media — the feed retains 200 ms / 16 blocks and drops the rest, by design.
        // If this came back with the same count, the measurement above would prove nothing.
        let (feed, source, base) = fixture();
        let mut stalled = ReceiveEncoder::start(&feed, source).unwrap();
        for k in 0..TICKS {
            feed.publish(source, base, &tick_signal(k));
        }
        let late = stalled.poll(base).unwrap();
        assert!(
            late.len() < polled.len() / 2,
            "control did not trip: a stalled encoder kept {} of {TICKS}",
            late.len()
        );
    }

    #[test]
    fn the_producer_never_waits_for_the_encoder() {
        // The guarantee itself is `receive_audio.rs`'s `try_lock` (its own
        // `a_contended_consumer_cannot_block_publication_…` test owns that). This one
        // guards the consumer half: encoding happens on owned samples, outside the feed
        // lock, so a future refactor that encodes while holding it goes red here.
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        for k in 0..10 {
            feed.publish(source, base, &tick_signal(k));
        }
        let (done, waiting) = mpsc::channel();
        let producer = feed.clone();
        let publishing = std::thread::spawn(move || {
            for k in 10..60 {
                producer.publish(source, base, &tick_signal(k));
            }
            done.send(()).unwrap();
        });
        let frames = encoder.poll(base).unwrap();
        let returned = waiting.recv_timeout(Duration::from_secs(2));
        publishing.join().unwrap();
        assert!(returned.is_ok(), "publication waited for the encoder");
        assert!(
            !frames.is_empty(),
            "the encode under test must have happened"
        );

        // Positive control on the instrument: the same timeout must report a genuine wait.
        let (blocked, waiting) = mpsc::channel();
        let stalling = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            blocked.send(()).unwrap();
        });
        assert!(
            waiting.recv_timeout(Duration::from_millis(50)).is_err(),
            "control did not trip: recv_timeout cannot see a wait"
        );
        stalling.join().unwrap();
    }

    #[test]
    fn a_second_encoder_is_refused_and_the_first_keeps_streaming() {
        // One listener per station in v1. The refusal must be explicit, and it must
        // not disturb the encoder that already holds the seam.
        let (feed, source, base) = fixture();
        let mut first = ReceiveEncoder::start(&feed, source).unwrap();
        assert!(matches!(
            ReceiveEncoder::start(&feed, source),
            Err(EncodeError::Feed(ReceiveError::InUse))
        ));
        let frames = run_ticks(&feed, source, &mut first, base, 10, tick_signal);
        assert!(!frames.is_empty(), "the first listener must be untouched");
        assert_eq!(first.source(), source);
    }

    #[test]
    fn stale_media_is_never_encoded() {
        let (feed, source, base) = fixture();
        let mut encoder = ReceiveEncoder::start(&feed, source).unwrap();
        feed.publish(source, base, &tick_signal(0)); // primed away
        feed.publish(source, base, &tick_signal(1));
        assert!(
            encoder.poll(base + MAX_MEDIA_AGE).unwrap().is_empty(),
            "audio older than the feed's retention must be dropped, never replayed"
        );
        assert_eq!(encoder.encoded_bytes(), 0);
    }
}
