//! Real sound-card audio via `cpal` (feature `device`).
//!
//! Opens the default input and output devices, downmixes input to mono, fans
//! mono output to all channels, and resamples between the device's native rate
//! and the modem's 12 kHz. The RX/decode path uses a stateful, anti-aliased
//! decimator ([`crate::capture_resample::CaptureResampler`]); TX playback keeps
//! the plain linear resample (upsampling has no aliasing hazard). The cpal
//! callbacks (which run on an audio thread)
//! exchange device-rate samples with this struct through WAIT-FREE rings
//! ([`SpscRing`]);
//! [`AudioBackend::capture`]/[`AudioBackend::play`] do the resampling on the
//! caller's thread.
//!
//! ⚠️ **NO CALLBACK MAY EVER BLOCK ON THE RADIO LOOP** (#172, KR8MER). Both decode-path rings
//! used to be `Mutex<VecDeque<f32>>`, and the producer held the TX one across
//! `ring.extend(dev)` — a whole upsampled FT over, ~630k f32, with the `VecDeque` reallocation
//! inside the critical section — while the realtime output callback locked the same mutex every
//! buffer. That is textbook priority inversion: when Windows parks cores on battery the copy
//! outruns the callback deadline and the callback emits silence, which is the gated TX the
//! reporter heard (and why it was dramatically worse on battery, why raising process priority
//! helped, and why WSJT-X on identical hardware is clean). Both rings are now the same
//! atomics-only [`SpscRing`] the headphone monitor has always used, the RX meter is a plain
//! atomic, and what the rings drop is COUNTED ([`audio_health`]) rather than asserted away.
//!
//! Device/rate selection here is the conservative default; on a real station you
//! may want to pick a specific CODEC device and a 48 kHz config explicitly.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};

use crate::audiodev::{split_device_ordinal, AudioDevice};
use crate::backend::AudioBackend;
use crate::capture_resample::CaptureResampler;
use crate::monitor::{Monitor, SpscRing};
use crate::resample::resample_linear;

const MODEM_RATE: u32 = 12_000;

/// How much device-rate TX audio the sound-card ring holds, in seconds.
///
/// The FT slot path hands over an entire 13.14 s over in ONE `play` call, so anything less than
/// that would truncate a transmission. 20 s leaves margin for the longest over plus whatever a
/// slow tick has not drained yet; rounded up to a power of two by [`SpscRing::new`] that is 4 MB
/// at 48 kHz. Overflow is impossible in normal use and COUNTED when it happens
/// ([`AudioHealth::tx_dropped`]) rather than silently truncating the air.
const TX_RING_SECONDS: usize = 20;

/// How much device-rate capture audio the RX ring holds, in seconds. The radio loop drains it
/// every tick (~20 ms), so this is pure headroom against a stalled loop; a stall long enough to
/// overflow it has already cost the decode.
const RX_RING_SECONDS: usize = 4;

/// Realtime health counters for the sound-card path (#172) — what the rings dropped and where the
/// callbacks ran dry.
///
/// KR8MER asked for these with the dropout fix, and he is right to: "the inversion is gone" is an
/// assertion, whereas a TX-underrun count that reads zero across an AC-vs-battery A/B is evidence.
/// They are ATOMIC COUNTERS ONLY — a realtime callback may not log, allocate, or lock, so the
/// callbacks count and the radio loop reports (see [`CpalBackend::capture`]).
///
/// Process-global rather than per-backend: there is exactly one sound card open at a time, the
/// backend is rebuilt on every settings change, and a counter that reset under the operator mid-run
/// would be useless for the very comparison it exists to serve.
pub struct AudioHealth {
    /// Output callbacks that had audio at entry and ran DRY before filling the block — a real gap
    /// in a transmission. Not counted while idle: an empty ring between overs is silence by
    /// design, not an underrun.
    tx_underruns: AtomicU32,
    /// TX samples the producer could not queue because the ring was full.
    tx_dropped: AtomicU64,
    /// Input callbacks that had to drop at least one captured sample (ring full — the radio loop
    /// is not draining).
    rx_overruns: AtomicU32,
    /// Captured samples dropped for that reason.
    rx_dropped: AtomicU64,
}

impl AudioHealth {
    const fn new() -> Self {
        Self {
            tx_underruns: AtomicU32::new(0),
            tx_dropped: AtomicU64::new(0),
            rx_overruns: AtomicU32::new(0),
            rx_dropped: AtomicU64::new(0),
        }
    }
}

static AUDIO_HEALTH: AudioHealth = AudioHealth::new();

/// A reading of [`AUDIO_HEALTH`]: plain numbers, safe to render or log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioHealthSnapshot {
    pub tx_underruns: u32,
    pub tx_dropped: u64,
    pub rx_overruns: u32,
    pub rx_dropped: u64,
}

impl AudioHealthSnapshot {
    /// Nothing has gone wrong yet — the reading a healthy station gives.
    pub fn is_clean(&self) -> bool {
        *self == Self::default()
    }
}

/// Read the sound-card health counters (#172). Cumulative for the process, so an operator can
/// compare a run on mains against a run on battery by taking a reading at each end — see
/// [`AudioHealth`].
pub fn audio_health() -> AudioHealthSnapshot {
    AudioHealthSnapshot {
        tx_underruns: AUDIO_HEALTH.tx_underruns.load(Ordering::Relaxed),
        tx_dropped: AUDIO_HEALTH.tx_dropped.load(Ordering::Relaxed),
        rx_overruns: AUDIO_HEALTH.rx_overruns.load(Ordering::Relaxed),
        rx_dropped: AUDIO_HEALTH.rx_dropped.load(Ordering::Relaxed),
    }
}

/// One output callback's pass over the TX ring.
///
/// Exists so the two things the callback must get right are testable without a sound card: the
/// level is read PER SAMPLE, and a block that runs dry MID-OVER is counted as an underrun while an
/// idle block is not.
///
/// ⚠️ THE LEVEL ATOMIC IS READ PER SAMPLE, DELIBERATELY. Issue #14 was that the level was baked
/// into each sample in [`AudioBackend::play`] and then queued, so the ring held committed
/// amplitudes and a slider move could not reach anything already in flight. The ring is deep — the
/// FT slot path enqueues a whole 13.14 s over in one call — so "already in flight" meant seconds,
/// and the operator overshot chasing a control that had not caught up. Reading here is what makes
/// the change audible on the next buffer instead. A relaxed load per sample is a plain move on
/// every target we ship; hoisting it per block would save nothing measurable and would put the
/// liveness back out of reach of a test.
pub(crate) struct TxBlock<'a> {
    ring: &'a SpscRing,
    level: &'a AtomicU32,
    had_audio: bool,
    ran_dry: bool,
}

impl<'a> TxBlock<'a> {
    /// Open a block: honour any pending hard stop, then note whether there was audio to play, so
    /// [`Self::finish`] can tell a mid-over gap from ordinary idle silence.
    #[inline]
    pub(crate) fn begin(ring: &'a SpscRing, level: &'a AtomicU32) -> Self {
        ring.apply_flush();
        Self {
            ring,
            level,
            had_audio: !ring.is_empty(),
            ran_dry: false,
        }
    }

    /// The next mono TX sample at the level **as it stands right now**. An empty ring is silence,
    /// never a held sample and never a click.
    #[inline]
    pub(crate) fn next(&mut self) -> f32 {
        match self.ring.pop() {
            Some(s) => s * f32::from_bits(self.level.load(Ordering::Relaxed)),
            None => {
                self.ran_dry = true;
                0.0
            }
        }
    }

    /// Close the block, counting one underrun if it started with audio and ran out. Returns
    /// whether it did, so a test can assert on THIS block rather than on a process-global counter
    /// that every other test is also moving.
    #[inline]
    pub(crate) fn finish(self) -> bool {
        let underran = self.had_audio && self.ran_dry;
        if underran {
            AUDIO_HEALTH.tx_underruns.fetch_add(1, Ordering::Relaxed);
        }
        underran
    }
}

/// A sample format the audio path can carry, and the only two conversions it needs.
///
/// ⭐ WHY THIS EXISTS: every stream builder in this crate used to spell its own `match` over
/// `SampleFormat` with a hand-written conversion in each arm — four sites (the decode input, the TX
/// output, the voice mic, the monitor output) × four formats, sixteen near-identical blocks, and
/// SIX of the ten formats cpal 0.15 can report handled nowhere at all. A card whose default config
/// is `U16` or `F64` got "unsupported input sample format" and NO AUDIO. That is a live outage, and
/// the cpal 0.18 upgrade would widen it: 0.18 adds `I24`/`U24` and re-ranks `default_*_config()` so
/// `F64`, `U32`, `I24` and `U24` all outrank the `I16` we do handle (`cmp_default_heuristics`,
/// lib.rs:790-810). One conversion per format, one generic builder per site: the coverage gap is
/// closed for every format that exists today, and a new variant is one impl and one dispatch line.
///
/// ⚠️ THE FOUR SHIPPED CONVERSIONS ARE BIT-EXACT, deliberately. `to_unit`/`from_unit` for `F32`,
/// `I16`, `U8` and `I32` are the arithmetic that was in those arms, character for character —
/// including `f32` output being UNCLAMPED (nothing can wrap, and the clamp on the integer arms is
/// there only to stop `as i16` wrapping full scale to negative full scale). cpal's own
/// `dasp_sample` conversions were the obvious alternative and were NOT used: they would have
/// changed the RX level and the TX drive by up to an LSB on the four formats operators are already
/// on the air with, which is not a change to make as a side effect of adding six others.
pub(crate) trait DeviceSample: cpal::SizedSample {
    /// Device sample → the modem's `-1.0..=1.0`.
    fn to_unit(self) -> f32;
    /// The modem's `-1.0..=1.0` → a device sample. Integer formats clamp here, because the cast
    /// wraps at full scale and a wrapped peak is an inverted spike on the air.
    fn from_unit(x: f32) -> Self;
}

impl DeviceSample for f32 {
    fn to_unit(self) -> f32 {
        self
    }
    fn from_unit(x: f32) -> Self {
        // UNCLAMPED, as shipped: no cast, so nothing can wrap.
        x
    }
}
impl DeviceSample for f64 {
    fn to_unit(self) -> f32 {
        self as f32
    }
    fn from_unit(x: f32) -> Self {
        x as f64
    }
}
impl DeviceSample for i8 {
    fn to_unit(self) -> f32 {
        self as f32 / 128.0
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) * 127.0) as i8
    }
}
impl DeviceSample for i16 {
    fn to_unit(self) -> f32 {
        self as f32 / 32768.0
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) * 32767.0) as i16
    }
}
impl DeviceSample for i32 {
    fn to_unit(self) -> f32 {
        self as f32 / 2_147_483_648.0
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) * 2_147_483_647.0) as i32
    }
}
impl DeviceSample for i64 {
    fn to_unit(self) -> f32 {
        // f32 carries 24 bits of mantissa, so the bottom bits of a 64-bit sample are lost here.
        // Nothing is lost that the 12 kHz f32 modem path was not going to lose anyway, and no
        // sound card has ever offered this — it is covered so the class is closed, not because
        // anyone is expected to hit it.
        (self as f64 / 9_223_372_036_854_775_808.0) as f32
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) as f64 * 9_223_372_036_854_775_807.0) as i64
    }
}
/// ⭐ THE FORMAT THE UPGRADE WAS ABOUT (#137's blast radius). `I24` did not exist as a
/// `SampleFormat` on cpal 0.15 — it was commented out of the enum — so a 24-bit interface could
/// never be offered one. 0.18 adds it AND ranks it above the `I16` we handle, so without this impl
/// the macOS enumeration fix would have been a Windows and Linux outage for every 24-bit rig
/// interface: "unsupported sample format" and no audio at all.
///
/// The container is an `i32` carrying 24 significant bits (`dasp_sample`: equilibrium 0, range
/// -8_388_608..=8_388_607). Same shape as every other integer format — scale by the POSITIVE rail
/// so full scale cannot wrap.
impl DeviceSample for cpal::I24 {
    fn to_unit(self) -> f32 {
        self.inner() as f32 / 8_388_608.0
    }
    fn from_unit(x: f32) -> Self {
        let v = (x.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
        // `new_unchecked` skips a range check the clamp above has already made unnecessary; the
        // second clamp makes that precondition evident at the call site rather than argued in a
        // comment. `new(..).unwrap()` would be a panic in a realtime callback.
        cpal::I24::new_unchecked(v.clamp(-8_388_608, 8_388_607))
    }
}

/// `U24` is offset-binary in an `i32` container (equilibrium 8_388_608, range 0..=16_777_215) —
/// the same relationship to `I24` that `U8` has to `I8`. Ranked just below `I24` in 0.18 and still
/// above `I16`, so it carries the same outage risk and gets the same treatment.
impl DeviceSample for cpal::U24 {
    fn to_unit(self) -> f32 {
        (self.inner() - 8_388_608) as f32 / 8_388_608.0
    }
    fn from_unit(x: f32) -> Self {
        let v = (x.clamp(-1.0, 1.0) * 8_388_607.0) as i32 + 8_388_608;
        cpal::U24::new_unchecked(v.clamp(0, 16_777_215))
    }
}

impl DeviceSample for u8 {
    fn to_unit(self) -> f32 {
        // Offset-binary around 128.
        (self as f32 - 128.0) / 128.0
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) * 127.0 + 128.0) as u8
    }
}
impl DeviceSample for u16 {
    fn to_unit(self) -> f32 {
        (self as f32 - 32768.0) / 32768.0
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) * 32767.0 + 32768.0) as u16
    }
}
impl DeviceSample for u32 {
    fn to_unit(self) -> f32 {
        (self as f64 - 2_147_483_648.0) as f32 / 2_147_483_648.0
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) as f64 * 2_147_483_647.0 + 2_147_483_648.0) as u32
    }
}
impl DeviceSample for u64 {
    fn to_unit(self) -> f32 {
        ((self as f64 - 9_223_372_036_854_775_808.0) / 9_223_372_036_854_775_808.0) as f32
    }
    fn from_unit(x: f32) -> Self {
        (x.clamp(-1.0, 1.0) as f64 * 9_223_372_036_854_775_807.0 + 9_223_372_036_854_775_808.0)
            as u64
    }
}

/// Every format [`DeviceSample`] is implemented for — i.e. every PCM format any stream in this
/// crate can be built in. The three DSD variants cpal 0.18 adds are deliberately ABSENT: see the
/// `other` arm of [`dispatch_format!`].
///
/// ⚠️ THIS LIST AND THE FOUR DISPATCH `match`es ARE ONE CONTRACT. A format here with no dispatch
/// arm is refused at build time; a dispatch arm not listed here is invisible to the monitor's
/// picker. It used to be TWO hand-maintained lists (this one lived in `monitor.rs` as
/// `buildable_format`, out of step with `device.rs`, which handled the same four but had no list at
/// all); it is one now, and `every_dispatchable_format_is_listed` holds the dispatches to it. It is
/// still a convention rather than a mechanism — the compiler cannot tie a `match` arm to a slice.
pub(crate) const DEVICE_FORMATS: &[SampleFormat] = &[
    SampleFormat::F32,
    SampleFormat::F64,
    SampleFormat::I8,
    SampleFormat::I16,
    SampleFormat::I24,
    SampleFormat::I32,
    SampleFormat::I64,
    SampleFormat::U8,
    SampleFormat::U16,
    SampleFormat::U24,
    SampleFormat::U32,
    SampleFormat::U64,
];

/// Can a stream be built in this format? The monitor's config picker asks before choosing one, so
/// it never selects a format the builder would then refuse (#8: ALSA offers `I8` first, the picker
/// took it, and the monitor died on a card the decode path opens every session).
pub(crate) fn supported_device_format(f: SampleFormat) -> bool {
    DEVICE_FORMATS.contains(&f)
}

/// How GOOD a format is for us, higher is better — used where we get to CHOOSE one.
///
/// ⭐ THE SECOND HALF OF #8, and it only became visible once every format was carried. That fix
/// made the monitor's picker skip formats it could not build, and on ALSA the first format a rig
/// codec offers is `I8`; with `I8` unbuildable, "first buildable" happened to mean "first decent
/// one". Now that `I8` builds, "first" would hand the operator an 8-bit monitor on a card that
/// also offered `I16` — a fixed bug turning into an audible regression. Where we choose, choose
/// the best.
///
/// Floats first (no quantisation and no clamping arithmetic), then integers by WIDTH, and signed
/// above unsigned at the same width — so the arms read in descending order and stay that way. The
/// 64-bit integers sit LOW despite their width: no sound card offers them as a stream format, they
/// lose precision through our `f32` path anyway, and ranking them top would hand a hypothetical
/// card the worst of both.
///
/// This is OUR ranking, not cpal's: `cmp_default_heuristics` changed between 0.15 and 0.18 (0.15
/// ranked only F32 > I16 > U16 and tied the rest; 0.18 ranks all fifteen), and a preference that
/// lives here does not move when the dependency does.
pub(crate) fn format_quality_rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::F32 => 100,
        SampleFormat::F64 => 90,
        SampleFormat::I32 => 80,
        SampleFormat::U32 => 78,
        SampleFormat::I24 => 76,
        SampleFormat::U24 => 74,
        SampleFormat::I16 => 70,
        SampleFormat::U16 => 65,
        SampleFormat::I64 => 40,
        SampleFormat::U64 => 35,
        SampleFormat::I8 => 20,
        SampleFormat::U8 => 15,
        // Anything we cannot carry must never be chosen, whatever else is on offer.
        _ => 0,
    }
}

/// Dispatch on a device sample format, calling `$build::<T>(..)` for the matching Rust type.
///
/// ⭐ THE ONE PLACE THE FORMAT LIST LIVES. All four stream builders — the decode input, the TX
/// output, the voice mic and the headphone monitor — go through this, so a format is carried
/// everywhere or nowhere, and adding one at the cpal 0.18 upgrade is ONE line here plus one
/// [`DeviceSample`] impl. It used to be four independent `match`es that had already drifted (the
/// monitor kept its own list of what it could build; `device.rs` kept none).
///
/// The `other` arm is not decoration: `cpal::SampleFormat` is `#[non_exhaustive]`, and 0.18 adds
/// `DsdU8`/`DsdU16`/`DsdU32`, which are 1-bit DSD streams that CANNOT be converted by scaling and
/// must stay a refusal rather than become noise.
macro_rules! dispatch_format {
    ($fmt:expr, $build:ident, $which:expr $(, $arg:expr)* $(,)?) => {{
        match $fmt {
            SampleFormat::F32 => $build::<f32>($($arg),*),
            SampleFormat::F64 => $build::<f64>($($arg),*),
            SampleFormat::I8 => $build::<i8>($($arg),*),
            SampleFormat::I16 => $build::<i16>($($arg),*),
            SampleFormat::I24 => $build::<cpal::I24>($($arg),*),
            SampleFormat::I32 => $build::<i32>($($arg),*),
            SampleFormat::I64 => $build::<i64>($($arg),*),
            SampleFormat::U8 => $build::<u8>($($arg),*),
            SampleFormat::U16 => $build::<u16>($($arg),*),
            SampleFormat::U24 => $build::<cpal::U24>($($arg),*),
            SampleFormat::U32 => $build::<u32>($($arg),*),
            SampleFormat::U64 => $build::<u64>($($arg),*),
            other => Err(crate::device::unsupported_format($which, other)),
        }
    }};
}
pub(crate) use dispatch_format;

/// The one wording for a format nothing can carry. Names the format, because "unsupported format"
/// alone sends the operator to a forum rather than to us — and this string reaches him as a
/// persistent banner through `Engine::set_audio_error`, never as silence.
pub(crate) fn unsupported_format(which: &str, f: SampleFormat) -> String {
    format!(
        "this device's {which} audio format ({f:?}) is not one Nexus can carry. \
         Supported: {supported}. Pick a different device, or set the card to one of these formats \
         in the operating system's sound settings.",
        supported = DEVICE_FORMATS
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Clamp an RX Gain setting to the sane 1.0–8.0 (+18 dB) range, so a stray value cannot blow up
/// the decode/monitor path.
///
/// ⚠️ ONE RULE FOR EVERY RX ROUTE. The slider multiplies captured audio in the cpal input
/// callbacks; the native Flex DAX route bypasses those callbacks entirely (its audio arrives over
/// UDP), so the control was simply INERT there — a quiet Flex slice could not be boosted, and the
/// operator's first troubleshooting step did nothing (Flex audit 2026-08-17, #1049; the RX sibling
/// of the TX-drive bypass, #1048). `service.rs` applies the same multiply to DAX audio through
/// this same clamp, so the slider means one thing regardless of where the audio came from.
pub(crate) fn clamp_rx_gain(gain: f32) -> f32 {
    gain.clamp(1.0, 8.0)
}

/// Should occurrence `n` (1-based) of a repeating stream error be PRINTED?
///
/// ⚠️ #139 (M0LHJ): a 294 MB, 2.76-million-line log. cpal 0.15's ALSA worker does not recover from
/// a POLLERR — it retries in a tight loop, calling the error callback every pass — and every one of
/// those printed a line. The first few are the diagnosis; the next 2.76 million are the same fact
/// at 106 bytes each, and they cost the operator his disk before they cost him anything else.
///
/// The first three, then every power of two, so a genuinely persistent fault still shows its scale
/// in the log (the count is printed with each line) while a spin can never fill a disk: M0LHJ's
/// 2.76 million occurrences become 22 lines.
pub(crate) fn should_report_stream_error(n: u32) -> bool {
    n <= 3 || n.is_power_of_two()
}

fn err_fn(e: cpal::Error) {
    use std::sync::atomic::AtomicU32;
    static SEEN: AtomicU32 = AtomicU32::new(0);
    let n = SEEN.fetch_add(1, Ordering::Relaxed) + 1;
    if should_report_stream_error(n) {
        eprintln!("tempo-audio: cpal stream error (occurrence {n}): {e}");
    }
}

/// The slot a dead decode-path stream reports itself into, drained by the radio loop.
///
/// `Mutex<Option<String>>` rather than a bare flag because the loop shows the operator WHAT the
/// OS said, and the FIRST report is the informative one — a device that disappears kills the
/// capture and playback streams within milliseconds of each other, and "capture stream: ..." is
/// the useful half.
pub(crate) type StreamErrSlot = std::sync::Arc<Mutex<Option<String>>>;

/// Build a cpal error callback that RECORDS the failure instead of only printing it.
///
/// The decode path's streams used to share [`err_fn`], which `eprintln!`s and returns. That is
/// invisible twice over: a Finder-launched `.app` has nowhere for stderr to go, and nothing in
/// the loop ever learned the stream was dead. `RadioLoop::step` only reopens the sound card when
/// the SETTINGS change, so a stream killed by the OS — device unplugged, a CoreAudio topology
/// change when a Bluetooth headset connects, or a codec that drops off the bus by itself (an
/// USB codec was measured doing exactly this, 2.0–2.4 s after every open) — left Nexus
/// running with a blank waterfall, no decodes, no banner and no diagnosis. Recording the text
/// here lets the loop raise the same banner a failed OPEN raises and arm the same retry, so the
/// stream comes back by itself once the device does.
///
/// Keeps the `eprintln!` for the terminal-launched/CI case, and keeps only the FIRST error: the
/// callback can fire repeatedly while the loop is between ticks, and later repeats say nothing new.
fn err_recorder(
    slot: &StreamErrSlot,
    which: &'static str,
) -> impl FnMut(cpal::Error) + Clone + Send + 'static {
    let slot = slot.clone();
    // Per-callback, so a playback spin cannot spend the capture stream's budget.
    let seen = Arc::new(AtomicU32::new(0));
    move |e| {
        // RATE-LIMITED — see should_report_stream_error for the 294 MB log this prevents (#139).
        let n = seen.fetch_add(1, Ordering::Relaxed) + 1;
        if should_report_stream_error(n) {
            eprintln!("tempo-audio: cpal {which} stream error (occurrence {n}): {e}");
        }
        let mut held = slot.lock().unwrap_or_else(|p| p.into_inner());
        if held.is_none() {
            // Only the FIRST one reaches the diagnostic log too — the callback can fire
            // repeatedly between loop ticks, and a device that vanishes must not be able to
            // spend the file's size bound on repeats of one fact.
            tempo_core::applog::error("audio", &format!("{which} stream died: {e}"));
            *held = Some(format!("{which} stream: {e}"));
        }
    }
}

/// Decay applied to the RX peak meter each input callback (per callback, not per
/// sample): the meter falls smoothly when the signal goes quiet.
const RX_METER_DECAY: f32 = 0.85;

/// Serializes ALL cpal host/device/stream access in this process.
///
/// cpal's host init and stream construction/teardown are NOT safe to drive from
/// two threads at once: on ALSA (`snd_config`/`snd_pcm`) and on WASAPI/COM the
/// native device-graph activation has shared, non-reentrant global state. The
/// crash this guards against: opening Settings right after launch fires
/// `available_devices()` (enumeration) on a Tauri command thread *while* the radio
/// loop is still inside [`CpalBackend::open`] building the streams — two concurrent
/// `cpal::default_host()` callers fault natively and hard-kill the process (the
/// default `unwind` strategy can't catch a native SIGSEGV/abort).
///
/// Every entry point that touches the cpal host/devices/streams must hold this for
/// the full duration of that work, so enumeration can never overlap a stream open.
pub(crate) static AUDIO_HOST_LOCK: Mutex<()> = Mutex::new(());

/// Enumerate the host's input and output devices for the Settings pickers. Errors (and
/// devices whose name can't be read) are ignored, yielding empty/partial lists rather
/// than failing — this feeds a UI dropdown.
///
/// Each entry carries the string that ADDRESSES the device and the string the operator
/// READS; see [`AudioDevice`] for why those differ on Linux and why only the former is
/// ever persisted.
pub fn available_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    // cpal's host/device enumeration can PANIC deep in the platform backend (Windows WASAPI has
    // been seen to panic on a broken/virtual device — some Flex DAX, RDP-remote-audio, or bad-driver
    // setups). This runs when the Settings tab opens, so an un-isolated panic there crashes the whole
    // app before the operator can even finish Rig setup. Isolate it: a panic yields empty lists (the
    // operator can still TYPE a device name) instead of taking down the process. (A genuine native
    // access-violation in a driver DLL can't be caught here — that needs the faulting module named in
    // Windows Event Viewer — but a Rust-level panic in cpal is caught and survived.)
    std::panic::catch_unwind(|| {
        // Serialize against CpalBackend::open() (see AUDIO_HOST_LOCK) — concurrent cpal
        // host/device access during stream construction crashes natively.
        //
        // ⚠️ Still required on the Linux path even though it no longer calls cpal at all:
        // `snd_device_name_hint` walks alsa-lib's refcounted GLOBAL config tree, which
        // `CpalBackend::open` may concurrently be inside `snd_pcm_open` on. Do not delete
        // this as dead weight when reading the Linux branch alone.
        let _host_guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        enumerate_devices()
    })
    .unwrap_or_else(|_| {
        // Surface caught enumeration panics (rate-limited) — silent catches hid a
        // per-poll panic storm on one tester's laptop (unwind cost = sluggishness,
        // and the panic machinery's backtrace cache = a phantom 68 MB "leak").
        use std::sync::atomic::{AtomicU32, Ordering};
        static CAUGHT: AtomicU32 = AtomicU32::new(0);
        let n = CAUGHT.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n.is_multiple_of(100) {
            eprintln!(
                "nexus: audio-device enumeration panicked (caught; occurrence {n}) — \
                 a broken/virtual audio device on this system; device lists returned empty"
            );
        }
        (Vec::new(), Vec::new())
    })
}

/// **Linux**: name devices from ALSA's PCM hints directly, never from cpal.
///
/// cpal's ALSA host walks the same `HintIter` but keeps only `hint.name`, discarding the
/// human `desc` and the `direction`, and it probe-OPENS every hint in both directions,
/// silently dropping whatever will not open. On a PipeWire desktop that reduced a
/// reported 8-card machine to ONE card under raw `hw:`/`dmix:` names while flooding the
/// log with ALSA open errors. Reading the hints ourselves opens NO PCM, so nothing is
/// dropped for being busy and the error flood disappears by construction. See
/// [`crate::audiodev`] for the full account and the pruning policy.
#[cfg(target_os = "linux")]
fn enumerate_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    match alsa_hints() {
        Ok(hints) => crate::audiodev::prune_alsa_hints(&hints),
        Err(e) => {
            // No cpal fallback on purpose: cpal's ALSA host reads the SAME hint list, so
            // if this failed cpal's enumeration has nothing left to offer either.
            eprintln!("nexus: ALSA device enumeration failed ({e}); device lists are empty");
            tempo_core::applog::warn(
                "audio",
                &format!("ALSA device enumeration failed ({e}); device lists are empty"),
            );
            (Vec::new(), Vec::new())
        }
    }
}

/// Read every PCM hint alsa-lib knows about.
///
/// Opens no PCM stream — only each card's non-exclusive CONTROL device, which is exactly
/// why `aplay -L` still lists a card PipeWire is holding. So a busy rig codec is LISTED
/// (it is the operator's device and may well be free by the time he transmits) instead of
/// vanishing, and an unopenable one fails loudly at open time via [`resolve_configured`].
#[cfg(target_os = "linux")]
fn alsa_hints() -> Result<Vec<crate::audiodev::PcmHint>, String> {
    use crate::audiodev::{HintDir, PcmHint};
    let iter = alsa::device_name::HintIter::new_str(None, "pcm").map_err(|e| e.to_string())?;
    Ok(iter
        .filter_map(|h| {
            Some(PcmHint {
                name: h.name?,
                desc: h.desc,
                // ALSA's IOID has no "both" value — a bidirectional PCM omits it, which
                // the alsa crate reports as None. Pass that through unchanged.
                direction: match h.direction {
                    Some(alsa::Direction::Capture) => Some(HintDir::Capture),
                    Some(alsa::Direction::Playback) => Some(HintDir::Playback),
                    None => None,
                },
            })
        })
        .collect())
}

/// **Windows / macOS**: cpal's own device names, exactly as before the Linux naming fix.
///
/// WASAPI reports `DEVPKEY_Device_FriendlyName` and CoreAudio
/// `kAudioDevicePropertyDeviceNameCFString` — both already the friendly string the OS
/// shows the operator — so `label == name` and every rendered `<option>` is byte-identical
/// to what shipped. That is asserted, not assumed: see
/// `audiodev::tests::cpal_names_are_their_own_labels` (the pure string path, run on every
/// platform) and `tests::non_linux_devices_are_their_own_labels` below (the real path, run
/// on the Windows/macOS runners).
#[cfg(not(target_os = "linux"))]
fn enumerate_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    use crate::audiodev::devices_from_cpal_names;
    let host = cpal::default_host();
    let inputs: Vec<String> = host
        .input_devices()
        .map(|it| it.filter_map(|d| cpal_device_name(&d)).collect())
        .unwrap_or_default();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| cpal_device_name(&d)).collect())
        .unwrap_or_default();
    (
        devices_from_cpal_names(inputs),
        devices_from_cpal_names(outputs),
    )
}

/// Read a cpal device's ADDRESSING name, FALLIBLY — the one place that conversion happens.
///
/// ⚠️ NEVER `to_string()`. cpal 0.18 removed `DeviceTrait::name()` in favour of `description()`,
/// and the new `Display for Device` forwards to it and returns `fmt::Error` when it fails — which
/// makes `to_string()` PANIC on exactly the device that used to be skipped quietly. A device whose
/// description cannot be read is a device that dropped off the bus mid-enumeration, which is a
/// NORMAL event on a USB rig interface, not an exceptional one. #132 is the precedent: a panicking
/// serial driver took down port enumeration the same way, and the fix there was the same shape.
/// The `.ok()` here is what keeps `enumerate_devices` returning a partial list instead of dying.
///
/// ⚠️⚠️ AND ON LINUX IT IS `driver()`, NOT `name()` — THE ADDRESS, NOT THE LABEL. This is the trap
/// the cpal 0.18 migration sets, and taking `name()` everywhere would have broken every Linux
/// station at once. 0.15's ALSA `name()` returned the raw PCM name (`plughw:CARD=CODEC,DEV=0`),
/// which is what the picker PERSISTS and what [`resolve_configured`] matches on. 0.18's ALSA
/// `description().name()` is the HUMAN description ("USB AUDIO CODEC") and the PCM id moved to
/// `description().driver()`. Swapping one for the other makes every saved device unresolvable:
/// strict resolution errors, the banner says "not available", and the loop falls back to the
/// system default — the laptop microphone, with TX audio going to the speakers while PTT still
/// keys the rig. That is the dead carrier `resolve_configured` exists to prevent.
///
/// Confined to Linux deliberately. WASAPI's `description().name()` is `DEVPKEY_Device_FriendlyName`
/// and CoreAudio's is the device name — both exactly what 0.15's `name()` returned, and neither
/// sets `driver`, so those hosts keep the string the operator's saved config already holds.
/// See `device_naming_tests` for the guard, which runs against the REAL host because the
/// `NamedDevice` stand-ins cannot see this.
pub(crate) fn cpal_device_name(d: &cpal::Device) -> Option<String> {
    let desc = d.description().ok()?;
    #[cfg(target_os = "linux")]
    if let Some(pcm_id) = desc.driver() {
        return Some(pcm_id.to_string());
    }
    Some(desc.name().to_string())
}

/// Anything that can report its device name.
///
/// Implemented for `cpal::Device` in the app and for a plain `String` in the tests — cpal
/// has no public `Device` constructor, so without this seam the selection POLICY
/// ([`pick_device`]'s ordinal handling, [`resolve_configured`]'s strict/fallback split)
/// could not be tested at all on a machine without a sound card.
pub(crate) trait NamedDevice {
    fn device_name(&self) -> Option<String>;
}

impl NamedDevice for cpal::Device {
    fn device_name(&self) -> Option<String> {
        cpal_device_name(self)
    }
}

/// A test stand-in: the device's name plus an id, so a test can tell WHICH of two
/// identically-named codecs the picker returned (the ordinal suffix exists for exactly
/// that case, and a stand-in that could not distinguish them could not prove it).
#[cfg(test)]
impl NamedDevice for (String, u32) {
    fn device_name(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

/// Pick a device by name from an iterator of devices, falling back to `default`
/// when `name` is empty/None or no device matches. Understands the " #N" ordinal suffix
/// `audiodev::disambiguate_names` appends to identically-named devices, so two rigs sharing
/// the generic "USB Audio CODEC" name resolve to DIFFERENT codecs (else `find()` always
/// returns the first).
///
/// ⚠️ The silent `default` fallback is right for exactly one caller — the voice mic, which
/// passes `default = None` and so gets no fallback at all. Anything resolving a device the
/// operator explicitly CHOSE must use [`resolve_configured`] instead.
pub(crate) fn pick_device<D: NamedDevice>(
    devices: Option<impl Iterator<Item = D>>,
    name: Option<&str>,
    default: Option<D>,
) -> Option<D> {
    let wanted = name.map(str::trim).filter(|n| !n.is_empty());
    if let (Some(wanted), Some(devs)) = (wanted, devices) {
        let (base, ordinal) = split_device_ordinal(wanted);
        let mut seen = 0usize;
        for d in devs {
            if d.device_name().as_deref() == Some(base) {
                seen += 1;
                if seen == ordinal {
                    return Some(d);
                }
            }
        }
    }
    default
}

/// The ALSA `CARD=<id>` token of a device name, e.g. `plughw:CARD=Device,DEV=0` →
/// `CARD=Device`. Two names sharing it are the SAME physical card reached by DIFFERENT
/// access paths (`hw:` / `plughw:` / `dsnoop:` / `default:` …). `None` when the name carries
/// no such token (Windows / Pulse / JACK names), which is what confines the card-identity
/// fallback in [`resolve_configured`] to ALSA, where the menu-vs-cpal naming split lives.
fn alsa_card_token(name: &str) -> Option<&str> {
    let rest = &name[name.find("CARD=")?..];
    Some(&rest[..rest.find(',').unwrap_or(rest.len())])
}

/// Resolve a CONFIGURED device name, strictly.
///
/// The difference from [`pick_device`] is the whole of fix C: an EMPTY selection still
/// means "system default" (a real choice the picker offers, listed as "System default"),
/// but a NON-EMPTY one that resolves to nothing is an `Err` the operator SEES.
///
/// It used to fall back to the system default silently. So an operator who had explicitly
/// chosen his rig's codec, on a day it would not open, was captured from the LAPTOP
/// MICROPHONE with TX audio going to the PC speakers — while PTT still keyed the rig over
/// CAT. That is a dead, unmodulated carrier on the air, and it looked like everything was
/// working. `Engine::set_audio_error` renders as a persistent banner, so now it says so.
///
/// On Linux the usual cause is BUSY, not absent: resolution runs through cpal's
/// `input_devices()`, which probe-opens (see [`enumerate_devices`]), so a card PipeWire or
/// another application is holding is listed by us and unresolvable here. The message names
/// both possibilities rather than guessing.
///
/// **`mk_devices` is a factory, not an iterator, because resolution takes TWO independent
/// lazy passes** (exact name, then the card-identity fallback below) and each MUST start from
/// a fresh enumeration — reusing pass 1's iterator, or retaining its devices to re-scan, would
/// hold the ALSA handles that the lazy drop exists to release (see pass 1). Pass 2 runs only
/// on the error path, so the second enumeration is paid only when the exact name already
/// missed.
/// Whether this host's `Device` addresses the whole CARD (both directions — ALSA,
/// CoreAudio: one AudioDeviceID carries capture and render) or a single-direction
/// ENDPOINT (WASAPI: `input_devices()` yields only eCapture endpoints, and asking one
/// for an output config is a refusal by design).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DeviceGrain {
    Card,
    Endpoint,
}

/// The running host's grain. Windows is the one endpoint-grained host cpal gives us;
/// macOS must stay `Card` — a CoreAudio duplex USB codec is one device with both
/// directions under one name, and the sharing shortcut is CORRECT there.
pub(crate) const HOST_GRAIN: DeviceGrain = if cfg!(target_os = "windows") {
    DeviceGrain::Endpoint
} else {
    DeviceGrain::Card
};

/// The one-card-for-both-directions decision (#2, #8), made platform-honest: on an
/// endpoint-grained host two directions NEVER share a device, however alike their
/// names — a shared Windows friendly name is the NORMAL presentation of a USB rig
/// interface, not evidence of a shared handle (#99, #104).
pub(crate) fn shares_one_device(grain: DeviceGrain, out_name: Option<&str>, in_name: &str) -> bool {
    if grain == DeviceGrain::Endpoint {
        return false;
    }
    out_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .is_some_and(|want| {
            let want_base = split_device_ordinal(want).0;
            want_base == in_name
                || matches!(
                    (alsa_card_token(want_base), alsa_card_token(in_name)),
                    (Some(w), Some(h)) if w == h
                )
        })
}

pub(crate) fn resolve_configured<D, I, F>(
    mk_devices: F,
    name: Option<&str>,
    default: Option<D>,
    what: &str,
) -> Result<D, String>
where
    D: NamedDevice,
    I: Iterator<Item = D>,
    F: Fn() -> Option<I>,
{
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        None => default.ok_or_else(|| format!("this system has no default {what} device")),
        Some(wanted) => {
            // ⚠️ CPAL 0.18 UPDATE: this is now belt-and-braces on Linux rather than load-bearing.
            // 0.18's ALSA enumerator no longer probe-opens (its `Device` is `{pcm_id, desc,
            // direction}` — no handles at all), so a card can no longer be busy against itself
            // here. Kept unchanged: it is still correct, it costs nothing, and the reasoning below
            // is why the shape is what it is.
            //
            // ⭐ LAZY, ONE DEVICE AT A TIME — the #2/#8 fix. This used to `collect()` the
            // whole iterator first (so a failure could name what it saw), and on Linux
            // that collect IS the failure: cpal's ALSA enumerator probe-opens every hint
            // as it yields, and each yielded Device RETAINS its opened handles — so
            // collecting held every card's handles simultaneously, and a card's `plughw:`
            // hint probed while its own `hw:` hint (yielded moments earlier, still held
            // in the Vec) had the card open. BUSY → cpal silently skips the hint → the
            // operator's saved plughw pick was never in the list, the resolve failed,
            // and the loop fell back to the default device forever. Iterating lazily and
            // DROPPING each non-match before the next probe releases the handles between
            // hints, so a card's later alias probes against a free card.
            //
            // The ordinal handling (" #N" for identically-named codecs) mirrors
            // `pick_device`; the dropped devices' names still feed the failure message.
            let (base, ordinal) = split_device_ordinal(wanted);
            let mut seen: Vec<String> = Vec::new();
            let mut matched = 0usize;
            if let Some(devs) = mk_devices() {
                for d in devs {
                    let n = d.device_name();
                    if n.as_deref() == Some(base) {
                        matched += 1;
                        if matched == ordinal {
                            return Ok(d);
                        }
                    }
                    if let Some(n) = n {
                        seen.push(n);
                    }
                    // `d` drops HERE, before the next probe — load-bearing, see above.
                }
            }
            // ⭐ CARD-IDENTITY FALLBACK — #8 (mw0cqu, FT-847 on a CM108/CODEC card). The
            // menu stores the card's `plughw:CARD=<id>` hint (audiodev prefers plughw for
            // its rate/format conversion), but cpal's own enumerator can surface the SAME
            // physical card only as `hw:CARD=<id>` — a different ACCESS PATH to one device,
            // never yielded under the saved name — so the exact pass above never matched and
            // his real card read as "not available" while its FFT was plainly bouncing. When
            // the saved name carries an ALSA `CARD=<id>` token, take the first enumerated
            // device on the SAME card (`CARD=<id>` is the ALSA card id — unique per card, so
            // this cannot cross to a different rig). A FRESH lazy pass, so the drop-between-
            // probes guarantee from pass 1 still holds. Confined to ALSA by construction: a
            // Windows/Pulse/JACK name has no CARD= token, so this never fires there.
            if let Some(card) = alsa_card_token(base) {
                if let Some(devs) = mk_devices() {
                    for d in devs {
                        let is_mate =
                            d.device_name().as_deref().and_then(alsa_card_token) == Some(card);
                        if is_mate {
                            return Ok(d);
                        }
                        // `d` drops HERE before the next probe — same reason as pass 1.
                    }
                }
            }
            Err(format!(
                "audio {what} device {wanted:?} is not available. The audio backend offered \
                 {n} {what} device(s) when opening{list} If it is on the menu but not in \
                 that list, the backend dropped it while probing — commonly because it is \
                 already open in another application.",
                n = seen.len(),
                list = if seen.is_empty() {
                    ".".to_string()
                } else {
                    format!(": {}.", seen.join(", "))
                },
            ))
        }
    }
}

/// Swap a system-default device handle for its enumerated twin (macOS input; see the call
/// site in [`CpalBackend::open`]).
///
/// cpal's CoreAudio backend registers its `kAudioDevicePropertyDeviceIsAlive` disconnect
/// listener only when `!is_default`, and the handle from `default_input_device()` carries
/// `is_default: true` — while its input AudioUnit is pinned to the concrete AudioDeviceID
/// regardless, so a default-input stream neither follows a later default switch NOR reports
/// its own death. Net effect: with the out-of-box "system default" ("") selection, a rig
/// codec unplug or a sleep/wake renumbering stops callbacks with no StreamError — nothing
/// arms `audio_suspect`, no banner, no self-heal; the [`err_recorder`] contract held on
/// macOS only for explicitly NAMED devices (mac QA audit merged[48]). Re-resolving the
/// default to the same device through the enumerator hands the stream a listener-carrying
/// handle: death now reports, probation confirms it, and the rebuild re-resolves "" against
/// whatever the default is by then.
///
/// First name match wins — the same heuristic [`pick_device`] uses for duplicate names. No
/// match (a default the enumerator cannot see) or a failed enumeration keeps the original
/// handle: exactly today's behavior, nothing new can fail. Lazy iteration with per-device
/// drop, same as [`resolve_configured`] (load-bearing on ALSA; harmless elsewhere).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // called from the mac-gated open path + tests
pub(crate) fn enumerated_default<D, I, F>(mk_devices: F, default: D) -> D
where
    D: NamedDevice,
    I: Iterator<Item = D>,
    F: Fn() -> Option<I>,
{
    let Some(want) = default.device_name() else {
        return default;
    };
    if let Some(devs) = mk_devices() {
        for d in devs {
            if d.device_name().as_deref() == Some(want.as_str()) {
                return d;
            }
            // `d` drops HERE, before the next probe — see resolve_configured.
        }
    }
    default
}

/// Real sound-card backend. Keep it alive for the duration of operation — the
/// cpal streams stop when this is dropped.
pub struct CpalBackend {
    /// `Option` so [`AudioBackend::release_device`] can drop it IN PLACE. Dropping a
    /// cpal `Stream` is what actually frees the ALSA handle; pausing does not.
    _in_stream: Option<Stream>,
    /// The transmit route through the sound card — its ring, and the stream that drains it, which
    /// is not opened until the operator first transmits. See [`TxOutput`] (#139).
    tx_out: TxOutput,
    /// Set by the capture/playback error callbacks when the OS kills a stream, drained each tick
    /// by the radio loop via [`AudioBackend::take_stream_error`]. See [`err_recorder`] for why a
    /// dead stream has to reach the loop at all.
    stream_err: StreamErrSlot,
    /// Captured device-rate mono, filled by the input callback and drained by
    /// [`AudioBackend::capture`] on the radio loop. Wait-free, because the callback is realtime
    /// (#172 — it used to be a `Mutex<VecDeque>` the loop could hold against it).
    in_ring: Arc<SpscRing>,
    /// Anti-aliased device-rate → 12 kHz decimator for the RX/decode path,
    /// carrying filter history + phase across [`capture`](AudioBackend::capture)
    /// calls (vs the old stateless per-block linear resample that folded
    /// 6–24 kHz energy into the decoder passband). Owned here so its state is
    /// per-capture-stream and never reset mid-stream.
    capture_rs: CaptureResampler,
    /// Anti-aliased 12 kHz → device-rate UPsampler for the TX/playback path, the
    /// mirror of `capture_rs`. The old stateless `resample_linear` drew straight
    /// chords between the modem's 12 kHz samples (~8 per cycle at 1.5 kHz); at a
    /// non-integer device ratio the chord's amplitude droop cycles, printing a
    /// periodic envelope RIPPLE onto what should be a flat constant-envelope FT8/FT4
    /// signal (the beaded-waveform bug — Nexus vs WSJT-X, 2026-07-21). The polyphase
    /// windowed-sinc reconstructs the sinusoid faithfully, so the envelope stays flat
    /// like WSJT-X's. Stateful: carries filter history across `play` calls, so the
    /// continuous phone/monitor streams get no per-chunk seam either.
    tx_rs: CaptureResampler,
    /// Smoothed RX input RMS (0.0–1.0, as `f32` bits), updated on the audio thread. Rendered as
    /// a WSJT-X-style dB level in the UI.
    ///
    /// An ATOMIC, not a `Mutex<f32>` — the input callback writes it every block, and #172's
    /// inversion is the same defect however small the critical section: the callback is realtime
    /// and the reader is not.
    rx_level: Arc<AtomicU32>,
    /// RX capture gain (f32 bits): a multiplier (≥1.0) applied to captured samples on the audio
    /// thread. Live-updatable from Settings for a quiet interface; 1.0 = unchanged. Atomic because
    /// the realtime input callback reads it every block.
    rx_gain: Arc<AtomicU32>,
    /// Tx audio level (f32 bits, 0.0–1.0), applied by the realtime OUTPUT callback as each sample
    /// leaves the ring — the same shape as [`Self::rx_gain`] and for the same reason.
    ///
    /// ⚠️ IT USED TO BE A PLAIN `f32` BAKED IN AT GENERATION TIME, and that was issue #14. `play`
    /// multiplied by it and pushed the already-scaled samples into `out_ring`, so the ring held
    /// committed amplitudes rather than modem audio, and moving the slider could not affect
    /// anything already queued. The queue is deep: the FT slot path enqueues an entire over —
    /// 13.14 s of FT8 — in one call, so the operator moved Pwr and watched the ALC sit at the old
    /// drive for seconds, then overshot chasing a control that had not caught up. Applying it on
    /// the way OUT means the level is whatever it is at the instant a sample reaches the card, so
    /// there is nothing queued to be stale.
    tx_level: Arc<AtomicU32>,
    /// Wait-free tee of the capture stream feeding the waterfall producer (see rxtap.rs). The
    /// caller publishes this to the RxTap after a successful open.
    spectrum_tap: Arc<SpscRing>,
    /// Device capture rate, so the consumer can build its own resampler.
    in_rate: u32,
    /// Dark headphone monitor: an in-place, off-by-default pass-through of the RX
    /// audio to a chosen output device. Reconfigured via [`AudioBackend::set_monitor`]
    /// WITHOUT touching the capture/TX streams (the decode path never restarts).
    monitor: Monitor,
    /// A transient SECOND input stream capturing the operator's voice from a dedicated
    /// mic, opened via [`AudioBackend::set_voice_mic`] only while a recording is in
    /// progress. `None` = no mic stream (recordings read the shared input). Opening /
    /// closing it never touches the main capture/TX streams.
    voice_mic: Option<CaptureStream>,
    /// Optional TX-audio tee (Flex native DAX): when set it REPLACES the sound card as the route
    /// for transmit audio — [`Self::play`] hands the 12 kHz samples to the tee and queues nothing.
    /// See [`crate::backend::TxTee`] for why it is exclusive rather than parallel.
    tx_tee: Option<crate::backend::TxTeeHandle>,
    /// The last [`audio_health`] reading the loop reported, so [`AudioBackend::capture`] logs a
    /// counter only when it MOVES. Reporting lives on the loop because a realtime callback may not
    /// log — it counts, and this reads.
    health_seen: AudioHealthSnapshot,
}

/// The sound card's transmit route: the ring the output callback drains, and — until the first
/// over — the device and config to build that callback's stream from.
///
/// ⚠️ #139 (M0LHJ, Fedora + FT-710): no FT8 decodes and a 294 MB log, on a machine where
/// `pw-record` and `pw-play` each worked alone and failed together. [`CpalBackend::open`] built
/// AND started BOTH streams unconditionally, with the output behind no transmit gate at all — so a
/// codec that cannot do simultaneous capture and playback got a playback stream it could not
/// serve, cpal's ALSA worker retried POLLERR forever, and the device-death check never fired
/// because capture genuinely kept delivering samples. Spectrum, no decodes. A station that only
/// listens now opens no playback stream at all.
///
/// ⭐ AND ON cpal 0.18 IT HOLDS NO PCM EITHER. The open question this fix shipped with was whether
/// keeping the resolved `cpal::Device` until the first transmit still held an idle ALSA playback
/// handle — 0.15's ALSA enumerator probe-opened BOTH directions and its `Device` retained them.
/// 0.18's ALSA `Device` is `{pcm_id, desc, direction}` with no handles, and its enumerator opens
/// nothing, so a receive-only station now holds no playback resource of any kind.
struct TxOutput {
    /// Wait-free (#172), and it exists from `open` whether or not the stream does — audio queued
    /// before the first open is played by it, not lost.
    ring: Arc<SpscRing>,
    /// The resolved device + config, held from `open` until the stream is built. The config is
    /// probed at OPEN time deliberately: an unusable output device must still be an error the
    /// operator sees when he saves the setting, not a surprise at his first transmission.
    deferred: Option<Box<DeferredOut>>,
    /// The live output stream, once something has actually been transmitted.
    stream: Option<Stream>,
    /// A deferred open that failed. The error is already on its way to the operator through
    /// `stream_err`; this stops the next buffer trying again.
    failed: bool,
}

/// What [`TxOutput`] needs to build its stream later.
struct DeferredOut {
    dev: cpal::Device,
    cfg: cpal::SupportedStreamConfig,
    channels: usize,
}

/// [`TxOutput`]'s state, as the lazy-open rule sees it — an enum so that rule is testable on a
/// machine with no sound card (a `Stream` cannot be constructed in a test).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TxStream {
    /// Resolved but not opened: the #139 state, and where a receive-only station stays.
    Deferred,
    Live,
    /// The deferred open failed and was reported.
    Failed,
    /// No sound-card output at all — after [`AudioBackend::release_device`], and in tests.
    Absent,
}

/// THE LAZY-OPEN RULE (#139): open the output stream on the first transmit, once.
///
/// Not when a tee is installed — the Flex native DAX route carries the over instead of the sound
/// card ([`CpalBackend::route_tx`]), so opening a playback stream there would re-create exactly the
/// idle playback stream this fix exists to avoid, on a path that will never use it.
pub(crate) fn should_open_tx_stream(state: TxStream, tee_installed: bool) -> bool {
    state == TxStream::Deferred && !tee_installed
}

impl TxOutput {
    fn state(&self) -> TxStream {
        if self.stream.is_some() {
            TxStream::Live
        } else if self.failed {
            TxStream::Failed
        } else if self.deferred.is_some() {
            TxStream::Deferred
        } else {
            TxStream::Absent
        }
    }
}

/// A named mono capture stream + its ring. Downmixes the device to mono at its native
/// rate into a lock-guarded ring; [`CaptureStream::drain`] drains + resamples to 12 kHz.
/// Dropping this stops and frees the device.
///
/// Used for the transient voice mic, and reusable for any second capture device that
/// needs its own stream independent of the main RX tap.
pub(crate) struct CaptureStream {
    _stream: Stream,
    ring: Arc<Mutex<VecDeque<f32>>>,
    rate: u32,
    /// The device name this stream targets, so a re-`set_voice_mic` on the SAME device
    /// is a no-op (only a different device rebuilds the stream).
    device: String,
}

/// Everything the decode-path input callback fans one captured sample out to.
///
/// A struct rather than seven parameters because [`dispatch_format!`] hands the same arguments to
/// every arm, and because these travel together by nature: they are the taps on one capture stream.
struct RxTaps {
    /// The decoder's ring, drained by the radio loop.
    in_ring: Arc<SpscRing>,
    /// The waterfall producer's tee (rxtap.rs) — always live, ungated.
    tap_ring: Arc<SpscRing>,
    /// The headphone monitor's ring — gated by the two flags below.
    mon_ring: Arc<SpscRing>,
    mon_enabled: Arc<AtomicBool>,
    mon_tx_mute: Arc<AtomicBool>,
    rx_gain: Arc<AtomicU32>,
    rx_level: Arc<AtomicU32>,
}

/// The decode path's capture callback, once, for every sample format.
///
/// ⚠️ REALTIME. No lock, no allocation, no logging — see the module header for the #172 inversion
/// this discipline exists to prevent. Everything it touches is an atomic or a wait-free ring.
fn build_rx_stream<T: DeviceSample + Send + 'static>(
    dev: &cpal::Device,
    cfg: &cpal::SupportedStreamConfig,
    ch: usize,
    taps: RxTaps,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, String> {
    dev.build_input_stream(
        cfg.config(),
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut dropped = 0u64;
            // Read the monitor gate ONCE per callback (not per sample). When on, push each mono
            // sample into the wait-free monitor ring — it never blocks or allocates and drops on
            // overflow, so the decode path (this same callback) is never stalled by monitoring.
            let monitoring = taps.mon_enabled.load(Ordering::Relaxed)
                && !taps.mon_tx_mute.load(Ordering::Relaxed);
            let g = f32::from_bits(taps.rx_gain.load(Ordering::Relaxed));
            let mut sum_sq = 0.0f32;
            let mut n = 0usize;
            for frame in data.chunks(ch) {
                // Fold to mono by AVERAGING the channels (× RX gain). Averaging keeps the signal
                // phase-coherent across the whole FT8 window no matter how the rig's codec lays
                // mono onto a stereo stream. Per-block "loudest lane" picking (0.8.9) thrashed L↔R
                // on a hiss channel and shredded decodes on stereo interfaces (Flex DAX, Xiegu
                // DE-19) — a quiet rig is handled by RX Gain, not by discarding a channel.
                let m = frame.iter().map(|&s| s.to_unit()).sum::<f32>() / ch as f32 * g;
                sum_sq += m * m;
                n += 1;
                if !taps.in_ring.push(m) {
                    dropped += 1;
                }
                // Tee to the waterfall producer (see rxtap.rs). Wait-free: atomics only, never
                // blocks, never allocates, drops on overflow — the same discipline the monitor ring
                // uses. UNGATED, unlike the monitor: the waterfall is always live, and an undrained
                // ring simply fills and drops.
                taps.tap_ring.push(m);
                if monitoring {
                    taps.mon_ring.push(m);
                }
            }
            update_rx_meter(&taps.rx_level, sum_sq, n);
            record_rx_drops(dropped);
        },
        err,
        None,
    )
    .map_err(|e| e.to_string())
}

/// The voice mic's mono capture callback, once, for every format.
///
/// Keeps this path's `Mutex<VecDeque>` deliberately: it is a TRANSIENT stream open only while a
/// voice message is being recorded, it does not feed the decoder or the transmitter, and #172's
/// inversion costs a recording glitch here rather than a gated over. Converting it too would be a
/// change to a path with no reported defect; it is noted in the module header instead.
fn build_voice_stream<T: DeviceSample + Send + 'static>(
    dev: &cpal::Device,
    cfg: &cpal::SupportedStreamConfig,
    ch: usize,
    ring: &Arc<Mutex<VecDeque<f32>>>,
) -> Result<Stream, String> {
    let ring_cb = ring.clone();
    dev.build_input_stream(
        cfg.config(),
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut r = ring_cb.lock().unwrap_or_else(|e| e.into_inner());
            for frame in data.chunks(ch) {
                r.push_back(frame.iter().map(|&s| s.to_unit()).sum::<f32>() / ch as f32);
            }
        },
        err_fn,
        None,
    )
    .map_err(|e| e.to_string())
}

impl CaptureStream {
    /// Open a mono capture stream on the named input device. The caller MUST already
    /// hold [`AUDIO_HOST_LOCK`]. Unlike the main input / monitor pickers this does NOT
    /// fall back to the system default: a missing named device returns `Err` so the
    /// caller falls back to the shared capture tap (opening the wrong default device
    /// would reintroduce the very "records the band" surprise this deliberately avoids).
    pub(crate) fn open(name: &str) -> Result<Self, String> {
        let host = cpal::default_host();
        let dev = pick_device(host.input_devices().ok(), Some(name), None)
            .ok_or_else(|| format!("voice-mic input device {name:?} not found"))?;
        let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
        let rate = cfg.sample_rate();
        let ch = cfg.channels() as usize;
        let ring = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        // ONE builder, dispatched per format — see `DeviceSample` for why these are no longer
        // sixteen hand-written conversion arms across four sites.
        let stream = dispatch_format!(
            cfg.sample_format(),
            build_voice_stream,
            "voice-mic input",
            &dev,
            &cfg,
            ch.max(1),
            &ring
        )?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            ring,
            rate,
            device: name.to_string(),
        })
    }

    /// Drain the ring and resample the device's native rate to 12 kHz. Body moved
    /// verbatim from `CpalBackend::voice_capture`, which now delegates here.
    pub(crate) fn drain(&self) -> Vec<f32> {
        let dev: Vec<f32> = {
            let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            ring.drain(..).collect()
        };
        resample_linear(&dev, self.rate, MODEM_RATE)
    }
}

impl CpalBackend {
    /// Open the system default input + output devices and start streaming.
    /// Thin wrapper over [`Self::open`] with no explicit device names.
    pub fn open_default() -> Result<Self, String> {
        Self::open(None, None)
    }

    /// Open the named input + output devices and start streaming.
    ///
    /// Empty/`None` → the system default. A NON-EMPTY name that matches no device is an
    /// `Err` naming the device (it used to fall back to the system default silently — see
    /// [`resolve_configured`] for why that had to stop).
    pub fn open(in_name: Option<&str>, out_name: Option<&str>) -> Result<Self, String> {
        // Hold the host lock across the ENTIRE host/device/stream-construction
        // sequence (through both `.play()` calls below) so a concurrent
        // `available_devices()` — e.g. the Settings panel enumerating at startup —
        // can never drive cpal's native init at the same time. See AUDIO_HOST_LOCK.
        let _host_guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let host = cpal::default_host();
        let in_default = host.default_input_device();
        // macOS: the default-INPUT handle streams pinned to the concrete device but with NO
        // disconnect listener — swap it for its enumerated twin so an unplug/sleep-wake death
        // actually reports (see enumerated_default). Input only, deliberately: the empty
        // OUTPUT gets cpal's DefaultOutput unit, which genuinely follows macOS default
        // switches, and trading that away is not this fix's call to make.
        #[cfg(target_os = "macos")]
        let in_default = in_default.map(|d| enumerated_default(|| host.input_devices().ok(), d));
        let in_dev =
            resolve_configured(|| host.input_devices().ok(), in_name, in_default, "input")?;
        // ONE CARD FOR BOTH DIRECTIONS (#2 "either alone works, both fails"; #8's CM108):
        // the resolved input device holds the card's handle pair, so re-enumerating for
        // the output would probe the SAME card, find it busy, and drop it — the output
        // name could never resolve. cpal's ALSA Device is Clone with Arc-shared handles:
        // the clone hands the output stream the PLAYBACK half of the pair already opened
        // at probe time, no re-probe at all. (Names compare on the base — the " #N"
        // ordinal names the same physical card.) The card-token arm keeps this working when
        // the card-identity fallback resolved the saved `plughw:CARD=X` input to cpal's
        // `hw:CARD=X`: the output's saved `plughw:CARD=X` no longer matches the resolved
        // name by string, but it IS the same card, so it must still clone rather than
        // re-probe a busy device (regressing the very CM108 both-directions case above).
        // ⚠️ BUT ONLY WHERE A DEVICE *IS* A CARD. On WASAPI a cpal Device is a
        // single-direction ENDPOINT (`input_devices()` yields only eCapture), and a
        // USB rig interface gets ONE Windows friendly name for BOTH endpoints — so
        // this shortcut cloned a capture endpoint as the output device and
        // `default_output_config()` answered "The requested stream type is not
        // supported by the device", failing the whole duplex open. Issues #99
        // (Xiegu DE-19) and #104 (QRP Labs QDX), both Windows 11, both regressions
        // from exactly this line's introduction in 1.3.0. `shares_one_device`
        // carries the platform grain; the probe below is the belt-and-braces: even
        // where sharing is believed correct, a clone that cannot produce an output
        // config falls back to real resolution rather than failing the open.
        // Through the fallible reader (see `cpal_device_name`): a name that cannot be read is
        // never "the same card" — fall through to ordinary resolution rather than treating an
        // unreadable input as a false-positive match.
        let same_card = in_dev
            .device_name()
            .is_some_and(|have| shares_one_device(HOST_GRAIN, out_name, &have));
        let out_dev = match same_card.then(|| in_dev.clone()) {
            Some(d) if d.default_output_config().is_ok() => d,
            _ => resolve_configured(
                || host.output_devices().ok(),
                out_name,
                host.default_output_device(),
                "output",
            )?,
        };

        let in_cfg = in_dev.default_input_config().map_err(|e| e.to_string())?;
        let out_cfg = out_dev.default_output_config().map_err(|e| e.to_string())?;
        let in_rate = in_cfg.sample_rate();
        let out_rate = out_cfg.sample_rate();
        let in_ch = in_cfg.channels() as usize;
        let out_ch = out_cfg.channels() as usize;

        // WAIT-FREE, BOUNDED (#172). Both decode-path rings are the atomics-only SpscRing the
        // monitor has always used; what a full ring drops is counted (see `audio_health`) rather
        // than paid for in callback latency. The TX ring must hold a whole over in one push — see
        // TX_RING_SECONDS.
        let in_ring = Arc::new(SpscRing::new(
            (in_rate as usize).saturating_mul(RX_RING_SECONDS).max(4096),
        ));
        let out_ring = Arc::new(SpscRing::new(
            (out_rate as usize)
                .saturating_mul(TX_RING_SECONDS)
                .max(4096),
        ));
        let rx_level = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let rx_gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        // TX level starts at unity and is read by the output callback each block — see the field's
        // doc for why it is applied on the way OUT rather than baked in at generation.
        let tx_level = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        // Shared by BOTH decode-path streams: either one dying means the sound card is gone, and
        // the loop rebuilds the pair together, so one slot is the whole report.
        let stream_err: StreamErrSlot = Arc::new(Mutex::new(None));
        let err_capture = err_recorder(&stream_err, "capture");
        // The playback half is built with its stream, at the first transmit (see `ensure_tx_stream`).

        // ---- headphone monitor shared state (DARK; nothing drains it until the
        // operator enables the monitor, which opens the output stream). Sized ~0.5 s
        // of capture-rate mono so the 20 ms loop bursts never overflow it in normal
        // use. The capture callback only pushes here while `mon_enabled` is set. ----
        let mon_ring = Arc::new(SpscRing::new((in_rate as usize / 2).max(4096)));
        // The waterfall's own tee of the capture stream. Sized ~1 s of device audio: the
        // producer never blocks, so a consumer that stalls just loses the excess. Lives in the
        // STREAM (not in RxTap) so each open owns exactly one producer — see rxtap.rs.
        let tap_ring = Arc::new(SpscRing::new((in_rate as usize).max(12_000)));
        let mon_enabled = Arc::new(AtomicBool::new(false));
        // MUTED WHILE KEYED. The monitor plays what the capture callback hears, and while the rig
        // is transmitting that is not the band — it is whatever the codec emits during TX, which on
        // many radios is their own MONI. Played back through this ring it arrives DELAYED, and
        // delayed sidetone is genuinely hard to talk over. Separate from `mon_enabled` on purpose:
        // that one is the operator's setting and must survive a transmission unchanged.
        let mon_tx_mute = Arc::new(AtomicBool::new(false));
        let mon_level = Arc::new(AtomicU32::new(0.5f32.to_bits()));

        // ---- input: fold to mono f32 (dominant lane × RX gain) → in_ring (+ peak meter) ----
        // ONE callback for every format now (see `DeviceSample` / `dispatch_format!`): the four
        // hand-written arms here differed only in how they turned a device sample into a float,
        // and six of the ten formats cpal can report had no arm at all.
        let taps = RxTaps {
            in_ring: in_ring.clone(),
            tap_ring: tap_ring.clone(),
            mon_ring: mon_ring.clone(),
            mon_enabled: mon_enabled.clone(),
            mon_tx_mute: mon_tx_mute.clone(),
            rx_gain: rx_gain.clone(),
            rx_level: rx_level.clone(),
        };
        let in_stream = dispatch_format!(
            in_cfg.sample_format(),
            build_rx_stream,
            "input",
            &in_dev,
            &in_cfg,
            in_ch.max(1),
            taps,
            err_capture,
        )?;

        // ---- output: NOT OPENED YET (#139) ----
        // The device and its config are resolved and probed here, so an unusable output device is
        // still an error the operator sees when he saves the setting. The PCM itself is opened by
        // `ensure_tx_stream` at the first transmit; see `TxOutput`.
        let tx_out = TxOutput {
            ring: out_ring,
            deferred: Some(Box::new(DeferredOut {
                dev: out_dev,
                cfg: out_cfg,
                channels: out_ch,
            })),
            stream: None,
            failed: false,
        };
        in_stream.play().map_err(|e| e.to_string())?;

        Ok(Self {
            _in_stream: Some(in_stream),
            tx_out,
            stream_err,
            in_ring,
            // The device output rate lives inside tx_rs now — play() resamples
            // through it, so the raw rate no longer needs a field of its own.
            capture_rs: CaptureResampler::new(in_rate, MODEM_RATE),
            tx_rs: CaptureResampler::new(MODEM_RATE, out_rate),
            rx_level,
            rx_gain,
            tx_level,
            monitor: Monitor::new(mon_ring, mon_enabled, mon_tx_mute, mon_level, in_rate),
            spectrum_tap: tap_ring,
            in_rate,
            voice_mic: None,
            tx_tee: None,
            health_seen: audio_health(),
        })
    }

    /// Open the sound card's output stream — the FIRST-TRANSMIT half of the #139 fix.
    ///
    /// Idempotent and cheap on every call after the first (one enum compare). A failure is
    /// reported through `stream_err`, which is the lane a stream death already travels: the loop
    /// raises the same banner and re-arms, so a rig whose playback endpoint will not open says so
    /// instead of transmitting silence. It is NOT retried per buffer — a card that refuses once
    /// refuses at audio rate.
    ///
    /// ⚠️ NOTHING QUEUED IS LOST. The ring exists from `open` and the stream drains it from its
    /// head, so audio queued before this ran — the tune carrier's 250 ms lead, most obviously —
    /// plays in full, in order, from its first sample.
    ///
    /// ⚠️ WHAT IT DOES COST is the open itself, once per session, on the radio loop. For a PHONE
    /// over `set_monitor_tx_mute` gets there first (see its call site: the loop drives it from
    /// `manual_ptt`), so the cost lands in the PTT settling window. Every other mode — an FT slot,
    /// CW, RTTY — reaches audio without a keying edge the backend can see, so its FIRST over of a
    /// session starts later by however long cpal takes to build and start the stream. Nothing is
    /// lost and nothing after the first over is affected, but it is a real shift on the one over
    /// that pays it, and it has not been measured on hardware.
    fn ensure_tx_stream(&mut self) {
        if !should_open_tx_stream(self.tx_out.state(), self.tx_tee.is_some()) {
            return;
        }
        let Some(d) = self.tx_out.deferred.take() else {
            return;
        };
        // Same non-reentrant native device-graph state as every other cpal entry point.
        let _host_guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match build_tx_stream(
            &d,
            &self.tx_out.ring,
            &self.tx_level,
            err_recorder(&self.stream_err, "playback"),
        ) {
            Ok(stream) => self.tx_out.stream = Some(stream),
            Err(e) => {
                self.tx_out.failed = true;
                tempo_core::applog::error(
                    "audio",
                    &format!("TX output stream failed to open: {e}"),
                );
                let mut held = self.stream_err.lock().unwrap_or_else(|p| p.into_inner());
                if held.is_none() {
                    *held = Some(format!("playback stream: {e}"));
                }
            }
        }
    }

    /// Log the realtime counters when they MOVE, from the radio loop (#172).
    ///
    /// The counters themselves are incremented in the audio callbacks, which may not log — the
    /// whole point of the fix is that nothing in a callback waits on a file, a lock or an
    /// allocator. This runs on the loop, off `capture`, and writes a line only on a change, so a
    /// healthy station never sees one and KR8MER's AC-vs-battery A/B is a `grep` rather than a
    /// stopwatch.
    fn report_health(&mut self) {
        let now = audio_health();
        if now == self.health_seen {
            return;
        }
        self.health_seen = now;
        tempo_core::applog::warn(
            "audio",
            &format!(
                "sound-card health: tx_underruns={} tx_dropped={} rx_overruns={} rx_dropped={}",
                now.tx_underruns, now.tx_dropped, now.rx_overruns, now.rx_dropped
            ),
        );
    }

    /// Route one buffer of 12 kHz TX audio: to the alternate route (Flex native DAX) when one is
    /// installed, otherwise to the sound card. EXACTLY ONE of them, ever.
    ///
    /// ⚠️ THIS USED TO FEED BOTH, and that was a shipped transmit defect (2026-08-17 Flex audit,
    /// finding #1051): the Flex configuration Nexus creates with its own one-click "Pair DAX audio"
    /// button points `audio_out` at the "DAX TX" endpoint, so every native over arrived at the
    /// radio twice — two routes, two resampler states, two latencies — and if the operator's output
    /// device was the PC speakers instead, the modem tones played out loud in the room. The tee
    /// carries the over or the card does.
    ///
    /// Split out of `play` (a one-line delegation now) so the rule is testable at all: a
    /// `CpalBackend` cannot be built without a sound card, which is why the parallel feed shipped
    /// with no test able to see it. Same doctrine as `service::dax_starved`.
    fn route_tx(
        tee: Option<&crate::backend::TxTeeHandle>,
        tx_rs: &mut CaptureResampler,
        out_ring: &SpscRing,
        samples: &[f32],
    ) {
        if let Some(tee) = tee {
            tee.feed(samples);
            return;
        }
        // Anti-aliased, stateful UPsample 12 kHz → device rate (see `tx_rs`). The old
        // `resample_linear` here put a periodic amplitude ripple on the constant-envelope
        // FT8/FT4 waveform; the polyphase reconstruction keeps it flat like WSJT-X.
        let dev = tx_rs.process(samples);
        // UNSCALED on purpose — the level is applied by the output callback as each sample
        // leaves. See `tx_level`: baking it in here is what made the Pwr slider unable to change
        // audio that was already queued. (The DAX route applies it the same way, at the same
        // point in its own path — as the packet leaves.)
        queue_to_card(out_ring, &dev);
    }

    /// Hard Stop TX: empty EVERY route, not merely the active one. Split out of `flush_output` for
    /// the same reason as [`Self::route_tx`] — and it MUST follow it. Making the DAX route
    /// exclusive without this would have left Stop TX clearing an output ring the transmission no
    /// longer travels through, i.e. cutting nothing at all.
    ///
    /// Both are cleared, not just the installed one: the ring can still hold audio queued before a
    /// tee was installed, and a stop that leaves audio anywhere is not a stop.
    /// `draining` is whether an output stream is actually running: only then is there a consumer
    /// to perform the drop. `tail` is consumer-owned (see [`SpscRing::request_flush`]), so with no
    /// stream open — a receive-only session under #139, or after `release_device` — the producer
    /// is the only thread touching the ring and empties it itself. Without that branch a Stop TX
    /// before the first transmit would leave the audio queued, to play at the next one.
    fn flush_tx(
        tee: Option<&crate::backend::TxTeeHandle>,
        out_ring: &SpscRing,
        draining: bool,
    ) -> usize {
        let n = out_ring.request_flush();
        if !draining {
            out_ring.apply_flush();
        }
        n + tee.map_or(0, |t| t.flush())
    }
}

/// Queue device-rate TX audio for the output callback, counting anything the ring cannot take.
///
/// ⭐ #172: this is the producer half of the priority inversion. It used to lock the mutex the
/// realtime output callback locks every buffer, and hold it across an `extend` of a whole
/// upsampled over (~630k f32) INCLUDING the `VecDeque` reallocation. Now it is a bounded, atomics-
/// only push: the callback is never parked on it, and the only thing that can go wrong is the ring
/// filling — which cannot happen at [`TX_RING_SECONDS`] and is counted if it ever does, because a
/// truncated over is an on-air defect and must not be silent.
/// Returns how many samples it had to drop — for the same reason [`TxBlock::finish`] returns its
/// verdict: a test can then assert on this call rather than on a shared counter.
fn queue_to_card(out_ring: &SpscRing, dev: &[f32]) -> usize {
    let dropped = dev.len() - out_ring.push_slice(dev);
    if dropped > 0 {
        AUDIO_HEALTH
            .tx_dropped
            .fetch_add(dropped as u64, Ordering::Relaxed);
    }
    dropped
}

/// Count what an input callback could not fit into the decode ring. One overrun EVENT per
/// callback plus the sample count, so "a few isolated glitches" and "the loop has stopped
/// draining" read differently.
#[inline]
fn record_rx_drops(dropped: u64) {
    if dropped > 0 {
        AUDIO_HEALTH.rx_overruns.fetch_add(1, Ordering::Relaxed);
        AUDIO_HEALTH
            .rx_dropped
            .fetch_add(dropped, Ordering::Relaxed);
    }
}

/// The TX output callback, once, for every sample format. See [`TxBlock`] for the per-sample level
/// rule and the underrun accounting, and [`DeviceSample::from_unit`] for why the integer formats
/// clamp and `f32` does not.
fn build_tx_stream_fmt<T: DeviceSample + Send + 'static>(
    dev: &cpal::Device,
    cfg: &cpal::SupportedStreamConfig,
    out_ch: usize,
    ring: &Arc<SpscRing>,
    level: &Arc<AtomicU32>,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, String> {
    let ring_cb = ring.clone();
    let level_cb = level.clone();
    dev.build_output_stream(
        cfg.config(),
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let mut blk = TxBlock::begin(&ring_cb, &level_cb);
            for frame in data.chunks_mut(out_ch) {
                let v = T::from_unit(blk.next());
                for x in frame.iter_mut() {
                    *x = v;
                }
            }
            blk.finish();
        },
        err,
        None,
    )
    .map_err(|e| e.to_string())
}

/// Build and start the sound card's TX output stream. The caller MUST already hold
/// [`AUDIO_HOST_LOCK`]. Lifted verbatim out of [`CpalBackend::open`] so it can run later — see
/// [`CpalBackend::ensure_tx_stream`] and [`TxOutput`] for why it no longer runs there (#139).
fn build_tx_stream(
    d: &DeferredOut,
    ring: &Arc<SpscRing>,
    level: &Arc<AtomicU32>,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<Stream, String> {
    let stream = dispatch_format!(
        d.cfg.sample_format(),
        build_tx_stream_fmt,
        "output",
        &d.dev,
        &d.cfg,
        d.channels.max(1),
        ring,
        level,
        err,
    )?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Fold a callback's RMS into the smoothed RX meter. The stored value is the
/// normalized RMS (0..1) of the post-gain audio — the frontend renders it as a
/// WSJT-X-style dB level (20·log10(rms)+90.3). RMS (not peak) is what makes the
/// reading comparable to WSJT-X's meter. Exponentially smoothed for stability.
///
/// The meter is a plain atomic (#172): a `Mutex<f32>` here put a lock acquisition inside the
/// realtime INPUT callback for the sake of one float. This callback is the only writer, so a
/// load/store pair is all the smoothing needs — no compare-exchange, no contention.
fn update_rx_meter(meter: &AtomicU32, sum_sq: f32, n: usize) {
    if n == 0 {
        return;
    }
    let rms = (sum_sq / n as f32).sqrt().clamp(0.0, 1.0);
    let prev = f32::from_bits(meter.load(Ordering::Relaxed));
    let next = prev * RX_METER_DECAY + rms * (1.0 - RX_METER_DECAY);
    meter.store(next.to_bits(), Ordering::Relaxed);
}

impl AudioBackend for CpalBackend {
    /// Free every device this backend holds, in ONE critical section.
    ///
    /// All four holders have to go, not just the obvious two: a monitor output stream or a
    /// live voice-mic stream keeps its card open just as firmly as the main capture stream,
    /// and either one left behind reproduces the original bug on that card.
    ///
    /// Takes [`AUDIO_HOST_LOCK`] ITSELF and calls `Monitor::release_locked` (which does not
    /// lock) rather than `Monitor::apply`, because `std::sync::Mutex` is not reentrant —
    /// `apply` locks internally, so calling it from under the lock would deadlock the radio
    /// loop. For the same reason the CALLER must not hold the lock around this.
    fn release_device(&mut self) {
        let _host_guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.monitor.release_locked();
        self.voice_mic = None;
        self._in_stream = None;
        // Both halves of the TX output go: the running stream AND the deferred device handle.
        // (On cpal 0.18 the ALSA `Device` holds no PCM at all — see `TxOutput` — but the handle is
        // still a resource this backend must not keep past a release on every other host.)
        self.tx_out.stream = None;
        self.tx_out.deferred = None;
    }
    fn spectrum_tap(&self) -> Option<(Arc<SpscRing>, u32)> {
        Some((self.spectrum_tap.clone(), self.in_rate))
    }

    fn take_stream_error(&mut self) -> Option<String> {
        self.stream_err
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    fn capture(&mut self) -> Vec<f32> {
        // Drained on the RADIO LOOP, which is also where the realtime counters get reported — a
        // callback may count but never log (#172).
        self.report_health();
        let mut dev: Vec<f32> = Vec::with_capacity(self.in_ring.len());
        while let Some(s) = self.in_ring.pop() {
            dev.push(s);
        }
        // Anti-aliased, stateful decimation to 12 kHz (see `capture_rs`). Carries
        // filter history + fractional phase across calls, so no block-boundary
        // discontinuity and no long-run drift. The voice-mic path below keeps the
        // plain linear resample: that audio is a recorded voice message, not
        // decoded, so aliasing is harmless there.
        self.capture_rs.process(&dev)
    }

    fn play(&mut self, samples: &[f32]) {
        // FIRST TRANSMIT OPENS THE CARD (#139). The backstop for `set_monitor_tx_mute`'s earlier
        // open at key-down: a route that reaches audio without a keying edge still gets a stream.
        // Order matters — open BEFORE queueing, so the stream never starts mid-push.
        self.ensure_tx_stream();
        Self::route_tx(
            self.tx_tee.as_ref(),
            &mut self.tx_rs,
            &self.tx_out.ring,
            samples,
        );
    }

    /// Install / clear the alternate TX route (Flex native DAX), handing it the CURRENT TX level
    /// as it goes in — the tee applies the level itself, and installing one without it would put
    /// full-scale audio on the air until the operator next moved the slider.
    fn set_tx_tee(&mut self, tee: Option<crate::backend::TxTeeHandle>) {
        if let Some(t) = &tee {
            t.set_level(f32::from_bits(self.tx_level.load(Ordering::Relaxed)));
        }
        self.tx_tee = tee;
    }

    /// Current RX input level (0.0–1.0): a decaying peak meter sampled on the
    /// audio thread. Diagnostics only since 2026-08-01 (`examples/audio_probe`) —
    /// the app UI meters via the rx-dsp thread's `MeterFeed` (see rxdsp.rs).
    fn rx_level(&self) -> f32 {
        f32::from_bits(self.rx_level.load(Ordering::Relaxed))
    }

    /// Set the Tx audio level (0.0–1.0) applied to outgoing samples in [`play`].
    ///
    /// Pushed to the alternate route too, if one is installed: the Pwr slider must mean the same
    /// thing whichever way the over reaches the radio. It did not — the DAX route ignored the
    /// level entirely and carried full-scale audio no matter where Pwr sat, an IMD/splatter path
    /// with an on-screen control that appeared to work (2026-08-17 Flex audit, finding #1048).
    ///
    /// [`play`]: AudioBackend::play
    fn set_tx_level(&mut self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        self.tx_level.store(level.to_bits(), Ordering::Relaxed);
        if let Some(tee) = &self.tx_tee {
            tee.set_level(level);
        }
    }

    /// Set the RX capture gain (a ≥1.0 multiplier applied to captured samples on the audio
    /// thread). Live: the realtime input callback reads the atomic each block. Clamped by
    /// [`clamp_rx_gain`].
    fn set_rx_gain(&mut self, gain: f32) {
        self.rx_gain
            .store(clamp_rx_gain(gain).to_bits(), Ordering::Relaxed);
    }

    /// Discard queued-but-unplayed TX audio (hard Stop TX): clear the route the over is actually
    /// travelling on, so the transmission is cut immediately, not at the slot's end.
    fn flush_output(&mut self) -> usize {
        Self::flush_tx(
            self.tx_tee.as_ref(),
            &self.tx_out.ring,
            self.tx_out.stream.is_some(),
        )
    }

    /// Reconfigure the dark headphone monitor in place (start/stop/retune its output
    /// stream) — the capture and TX streams are untouched, so the decode path never
    /// restarts.
    fn set_monitor(&mut self, enabled: bool, device: &str, level: f32) -> Result<(), String> {
        self.monitor.apply(enabled, device, level)
    }

    fn set_monitor_tx_mute(&mut self, muted: bool) {
        // PHONE KEY-DOWN OPENS THE CARD EARLY (#139). The loop drives this from `manual_ptt`, so
        // it fires ahead of the first voice audio and the deferred open lands in the PTT settling
        // window rather than inside the over. It covers PHONE only — see `ensure_tx_stream` for
        // what the other modes pay — and `play` remains the backstop for all of them.
        if muted {
            self.ensure_tx_stream();
        }
        self.monitor.set_tx_mute(muted);
    }

    /// Open (`Some(name)`) or close (`None`) the transient voice-mic input stream. Opens
    /// a SECOND cpal input on the named device WITHOUT touching the main capture/TX
    /// streams (the decode path never restarts). All host/device/stream work is under
    /// [`AUDIO_HOST_LOCK`], like every other cpal entry point. `Err` = the named device
    /// failed to open (the caller falls back to the shared capture tap).
    fn set_voice_mic(&mut self, device: Option<&str>) -> Result<(), String> {
        let wanted = device.map(str::trim).filter(|d| !d.is_empty());
        match wanted {
            None => {
                if self.voice_mic.is_some() {
                    // Tear the stream down under the host lock (native device-graph
                    // teardown shares the non-reentrant state construction uses).
                    let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                    self.voice_mic = None;
                }
                Ok(())
            }
            Some(name) => {
                // Already open on this exact device → nothing to rebuild.
                if self.voice_mic.as_ref().map(|v| v.device == name) == Some(true) {
                    return Ok(());
                }
                let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                self.voice_mic = None; // free any prior device first
                self.voice_mic = Some(CaptureStream::open(name)?);
                Ok(())
            }
        }
    }

    /// 12 kHz mono samples captured from the voice-mic stream since the last call (empty
    /// when no mic stream is open), resampled from the mic device's native rate.
    fn voice_capture(&mut self) -> Vec<f32> {
        self.voice_mic
            .as_ref()
            .map(CaptureStream::drain)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tx_level_tests {
    use super::{audio_health, SpscRing, TxBlock};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn ring(v: &[f32]) -> SpscRing {
        let r = SpscRing::new(v.len().max(2));
        assert_eq!(r.push_slice(v), v.len(), "fixture fits");
        r
    }

    /// ISSUE #14, and the whole point of the change: audio that is ALREADY QUEUED must respond to
    /// the Pwr slider.
    ///
    /// The level used to be multiplied into each sample inside `play` and then pushed, so
    /// the TX ring held committed amplitudes. Nothing already queued could ever change, and the
    /// queue is deep — the FT slot path enqueues an entire 13.14 s over in one call — so the
    /// operator moved the slider, watched the ALC hold the old drive for seconds, and overshot
    /// chasing a control that had not caught up. The reporter measured exactly this against his
    /// rig's ALC meter while holding Tune.
    ///
    /// This is the test that could not exist while the multiply lived in the realtime callback,
    /// which is why the read was factored out rather than hoisted per block.
    #[test]
    fn a_level_change_reaches_audio_that_is_already_queued() {
        let r = ring(&[1.0, 1.0, 1.0, 1.0]);
        let level = AtomicU32::new(1.0f32.to_bits());
        let mut blk = TxBlock::begin(&r, &level);

        assert_eq!(blk.next(), 1.0, "unity passes through");

        // The operator moves Pwr mid-transmission. Everything still in the ring was generated
        // before this instant — under the old code it would keep going out at full drive.
        level.store(0.25f32.to_bits(), Ordering::Relaxed);
        assert_eq!(blk.next(), 0.25);
        assert_eq!(blk.next(), 0.25);

        // ...and back up again, same buffer.
        level.store(0.5f32.to_bits(), Ordering::Relaxed);
        assert_eq!(blk.next(), 0.5);
        blk.finish();
    }

    /// The waveform is scaled, not gated: shape is preserved and only amplitude moves, which is
    /// what a real drive control does. A flush-and-refill would have stepped the envelope.
    #[test]
    fn the_queued_waveform_keeps_its_shape() {
        let r = ring(&[1.0, -0.5, 0.25, -1.0]);
        let level = AtomicU32::new(0.5f32.to_bits());
        let mut blk = TxBlock::begin(&r, &level);
        let out: Vec<f32> = (0..4).map(|_| blk.next()).collect();
        blk.finish();
        assert_eq!(out, vec![0.5, -0.25, 0.125, -0.5]);
    }

    /// An empty ring is silence, not the last sample held — and silence stays silence whatever
    /// the level says, so an underrun can never emit a click scaled up by a high drive setting.
    #[test]
    fn an_empty_ring_is_silence_at_every_level() {
        let r = SpscRing::new(4);
        for bits in [0.0f32, 0.5, 1.0] {
            let level = AtomicU32::new(bits.to_bits());
            let mut blk = TxBlock::begin(&r, &level);
            assert_eq!(blk.next(), 0.0);
            blk.finish();
        }
    }

    /// Zero really is zero. `set_tx_level` clamps to 0.0–1.0, and an operator who drags Pwr to
    /// the bottom mid-over must get silence out of the samples already queued.
    #[test]
    fn zero_silences_audio_that_was_already_queued() {
        let r = ring(&[1.0, -1.0]);
        let level = AtomicU32::new(0.0f32.to_bits());
        let mut blk = TxBlock::begin(&r, &level);
        assert_eq!(blk.next(), 0.0);
        assert_eq!(blk.next(), 0.0);
        blk.finish();
    }

    /// ⭐ #172, THE MEASURABLE HALF. KR8MER asked for underrun counters with the dropout fix, and
    /// he is right: "the inversion is gone" is an assertion, a count is evidence. A block that had
    /// audio and ran dry is a real gap in a transmission and is counted; an idle block — the
    /// station between overs, which is most of them — is silence by design and must NOT be, or the
    /// counter would read in the millions on a receive-only evening and mean nothing.
    #[test]
    fn a_block_that_runs_dry_mid_over_is_an_underrun_and_an_idle_block_is_not() {
        let level = AtomicU32::new(1.0f32.to_bits());

        // IDLE: nothing queued, the whole block is silence. The positive control for the
        // assertion below — without it, a counter that fired on every empty pop would pass.
        let idle = SpscRing::new(8);
        let mut blk = TxBlock::begin(&idle, &level);
        for _ in 0..8 {
            assert_eq!(blk.next(), 0.0);
        }
        assert!(!blk.finish(), "an idle block is not an underrun");

        // MID-OVER: two samples queued, eight frames to fill — the gap the operator hears.
        let short = ring(&[0.5, 0.5]);
        let mut blk = TxBlock::begin(&short, &level);
        for _ in 0..8 {
            blk.next();
        }
        assert!(
            blk.finish(),
            "a block that had audio and ran out is an underrun"
        );

        // FULLY SERVED: the ordinary case, and the second half of the control — a block that
        // never ran short must not be counted however much audio it consumed.
        let plenty = ring(&[0.5; 16]);
        let mut blk = TxBlock::begin(&plenty, &level);
        for _ in 0..8 {
            blk.next();
        }
        assert!(!blk.finish(), "a block that was served in full is clean");
    }

    /// The counters are wired to the reading the operator gets. A counted underrun must actually
    /// move `audio_health()` — otherwise the whole diagnostic is a local variable.
    #[test]
    fn a_counted_underrun_reaches_the_health_reading() {
        let level = AtomicU32::new(1.0f32.to_bits());
        let short = ring(&[0.5]);
        let before = audio_health().tx_underruns;
        let mut blk = TxBlock::begin(&short, &level);
        for _ in 0..4 {
            blk.next();
        }
        assert!(blk.finish());
        assert!(
            audio_health().tx_underruns > before,
            "the reading must move when a block underruns"
        );
    }

    /// A hard Stop TX asked for by the producer is performed HERE, at the top of the next block —
    /// `tail` is the callback's to move (see `SpscRing::request_flush`). What follows is silence,
    /// not the rest of the over.
    #[test]
    fn a_block_honours_a_stop_requested_between_callbacks() {
        let r = ring(&[1.0, 1.0, 1.0, 1.0]);
        let level = AtomicU32::new(1.0f32.to_bits());

        let mut blk = TxBlock::begin(&r, &level);
        assert_eq!(blk.next(), 1.0, "the over is playing");
        blk.finish();

        r.request_flush(); // the operator hits Stop TX, on the radio loop
        let mut blk = TxBlock::begin(&r, &level);
        assert_eq!(blk.next(), 0.0, "the next block is silence, not the rest");
        blk.finish();
    }
}

/// The TX ROUTING rule: exactly one route carries an over, and a hard stop empties every route.
#[cfg(test)]
mod tx_route_tests {
    use super::{
        queue_to_card, should_open_tx_stream, CaptureResampler, CpalBackend, SpscRing, TxStream,
        MODEM_RATE,
    };
    use crate::backend::{TxTee, TxTeeHandle};
    use std::sync::{Arc, Mutex};

    /// A stand-in for the Flex DAX route (the real one needs a Flex, a socket and three threads):
    /// records what the backend hands it.
    #[derive(Default)]
    struct RecordingTee {
        fed: Mutex<Vec<f32>>,
        flushes: Mutex<usize>,
    }

    impl TxTee for RecordingTee {
        fn feed(&self, samples: &[f32]) {
            self.fed.lock().unwrap().extend_from_slice(samples);
        }
        fn set_level(&self, _level: f32) {}
        /// A stand-in count, distinguishable from any ring length in these tests.
        fn flush(&self) -> usize {
            *self.flushes.lock().unwrap() += 1;
            7
        }
    }

    fn tx_ring() -> SpscRing {
        SpscRing::new(48_000 * super::TX_RING_SECONDS)
    }

    /// EXCLUSIVE, NOT PARALLEL (audit #1051). `play` fed BOTH routes, and Nexus's own one-click
    /// Flex pairing points the output device at the radio's "DAX TX" endpoint — so the same over
    /// arrived at the radio twice, by two paths with different rates and latencies. And with the
    /// output device left on speakers, every native over played out loud in the room.
    #[test]
    fn an_installed_tee_carries_the_over_and_the_sound_card_gets_nothing() {
        let tee = Arc::new(RecordingTee::default());
        let handle: TxTeeHandle = tee.clone();
        let mut tx_rs = CaptureResampler::new(MODEM_RATE, 48_000);
        let ring = tx_ring();
        let over = vec![0.25f32; 600];

        // No tee → the sound card carries it, exactly as it always has (the positive control:
        // without this, the assertion below would pass on a route that never works at all).
        CpalBackend::route_tx(None, &mut tx_rs, &ring, &over);
        let queued_by_the_card = ring.len();
        assert!(
            queued_by_the_card > 0,
            "the sound-card route must still queue when no tee is installed"
        );
        assert!(tee.fed.lock().unwrap().is_empty(), "no tee → nothing teed");

        // Tee installed → the tee carries it, and the sound card queue does not grow by one sample.
        CpalBackend::route_tx(Some(&handle), &mut tx_rs, &ring, &over);
        assert_eq!(
            &*tee.fed.lock().unwrap(),
            &over,
            "the tee gets the modem's 12 kHz samples, unresampled and unscaled"
        );
        assert_eq!(
            ring.len(),
            queued_by_the_card,
            "the over must NOT also be queued to the sound card"
        );
    }

    /// Stop TX has to cut the route the over is actually on — and any other route that still holds
    /// audio. Clearing only the sound-card ring while the DAX tee carries the transmission would be
    /// a stop control that stops nothing.
    ///
    /// `draining: true` here — a live output stream — so the drop is the callback's to perform;
    /// the no-stream case has its own test below, because it is the one that breaks silently.
    #[test]
    fn a_hard_stop_empties_every_route() {
        let ring = SpscRing::new(8);
        assert_eq!(ring.push_slice(&[0.1f32; 5]), 5);
        assert_eq!(
            CpalBackend::flush_tx(None, &ring, true),
            5,
            "with no tee, the count is the sound-card ring's"
        );
        ring.apply_flush(); // the callback's next block
        assert!(ring.is_empty());

        let tee = Arc::new(RecordingTee::default());
        let handle: TxTeeHandle = tee.clone();
        let ring = SpscRing::new(8);
        assert_eq!(ring.push_slice(&[0.1f32; 3]), 3);
        assert_eq!(
            CpalBackend::flush_tx(Some(&handle), &ring, true),
            3 + 7,
            "both routes are emptied, and both counts are reported"
        );
        assert_eq!(*tee.flushes.lock().unwrap(), 1, "the tee was told to flush");
        ring.apply_flush();
        assert!(
            ring.is_empty(),
            "audio queued before the tee was installed is discarded too"
        );
    }

    /// ⭐ #139's sharp edge: with no output stream open there is NO CONSUMER, so a flush that only
    /// asks would never be performed and the audio would sit in the ring until the next
    /// transmission played it. Stop TX before the card has ever been opened must still be a stop.
    #[test]
    fn a_hard_stop_with_no_output_stream_open_empties_the_ring_itself() {
        let ring = SpscRing::new(8);
        assert_eq!(ring.push_slice(&[0.1f32; 4]), 4);
        assert_eq!(CpalBackend::flush_tx(None, &ring, false), 4);
        assert!(
            ring.is_empty(),
            "nothing else will ever drop this — the producer must do it itself"
        );
        // The positive control: the same call with a stream running leaves the drop to the
        // callback, so a flush_tx that ignored `draining` would fail here.
        let ring = SpscRing::new(8);
        assert_eq!(ring.push_slice(&[0.1f32; 4]), 4);
        assert_eq!(CpalBackend::flush_tx(None, &ring, true), 4);
        assert_eq!(ring.len(), 4, "the consumer has not run yet");
    }

    /// ⭐ #139 (M0LHJ): the output stream is opened by the FIRST TRANSMIT, once, and never for a
    /// station that only listens — which is the whole fix, since his codec could not carry a
    /// playback stream at the same time as capture and cpal's ALSA worker then spun on POLLERR
    /// forever (a 294 MB log, spectrum but no decodes).
    #[test]
    fn the_output_stream_opens_on_the_first_transmit_and_only_then() {
        // The receive-only station: resolved, never opened.
        assert!(
            should_open_tx_stream(TxStream::Deferred, false),
            "the first over opens the card"
        );
        // Once open, or once failed, it is not opened again — a card that refuses once refuses at
        // audio rate, and re-opening a live stream would restart the transmission.
        assert!(!should_open_tx_stream(TxStream::Live, false));
        assert!(!should_open_tx_stream(TxStream::Failed, false));
        assert!(!should_open_tx_stream(TxStream::Absent, false));
        // And never behind an installed tee: the Flex native DAX route carries the over instead of
        // the sound card, so opening a playback stream there re-creates the idle stream this fix
        // exists to avoid, on a path that will never use it.
        assert!(!should_open_tx_stream(TxStream::Deferred, true));
    }

    /// ⚠️ THE LAZY OPEN MUST NOT LOSE AUDIO. The tune carrier pre-queues a ~250 ms lead before the
    /// stream can possibly exist, and the FT slot path hands over a whole over in one call. The
    /// ring lives from `open`, so everything queued ahead of the stream is still there, in order,
    /// for the first block to play.
    #[test]
    fn audio_queued_before_the_stream_exists_is_all_there_when_it_opens() {
        let ring = tx_ring();
        let mut tx_rs = CaptureResampler::new(MODEM_RATE, 48_000);
        // 250 ms of tune lead at the modem rate, queued with no output stream open.
        let lead: Vec<f32> = (0..3_000).map(|i| (i as f32 / 3_000.0) - 0.5).collect();
        CpalBackend::route_tx(None, &mut tx_rs, &ring, &lead);
        let queued = ring.len();
        assert!(queued > 0, "the lead is queued with no stream open");

        // The stream opens now and starts draining from the ring's HEAD.
        let played: Vec<f32> = std::iter::from_fn(|| ring.pop()).collect();
        assert_eq!(played.len(), queued, "not one sample of the lead was lost");
    }

    /// A truncated over is an on-air defect, so an overflow is COUNTED, never silent. It cannot
    /// happen at TX_RING_SECONDS — this pins the behaviour, with a ring small enough to force it.
    #[test]
    fn tx_audio_that_does_not_fit_is_counted_rather_than_silently_lost() {
        let small = SpscRing::new(4); // capacity 4
        assert_eq!(
            queue_to_card(&small, &[0.1; 4]),
            0,
            "what fits is queued and nothing is reported"
        );
        assert_eq!(
            queue_to_card(&small, &[0.2; 3]),
            3,
            "and what does not fit is counted, every sample of it"
        );
    }
}

/// ⭐ THE cpal 0.18 MIGRATION'S OWN TRAP: what a device is CALLED versus how it is ADDRESSED.
#[cfg(test)]
mod device_naming_tests {
    use super::{cpal_device_name, resolve_configured, NamedDevice};
    use cpal::traits::{DeviceTrait, HostTrait};

    /// ⚠️ THE REGRESSION THIS MIGRATION NEARLY SHIPPED TO EVERY LINUX STATION.
    ///
    /// cpal 0.15's ALSA `Device::name()` returned the raw PCM name — `plughw:CARD=CODEC,DEV=0` —
    /// which is the string the picker PERSISTS and `resolve_configured` matches on. 0.18 removed
    /// `name()` in favour of `description()`, and its ALSA `description().name()` is the HUMAN
    /// DESCRIPTION (the first line of the hint's DESC, e.g. "USB AUDIO CODEC"), with the PCM id
    /// moved to `description().driver()`.
    ///
    /// So the obvious migration — `description().ok().map(|d| d.name().to_string())` — silently
    /// swaps an ADDRESS for a LABEL. Every Linux operator's saved device would have stopped
    /// resolving at once: strict resolution returns Err, the banner says the device "is not
    /// available", and the loop falls back to the system default — which is the laptop microphone
    /// with TX audio on the speakers while PTT still keys the rig. That is the dead-carrier
    /// scenario `resolve_configured` was written to end (fix C), reintroduced by a dependency bump.
    ///
    /// ⭐ WHY NOTHING ELSE CAUGHT IT: every resolution test in this file drives the `NamedDevice`
    /// stand-ins (`&'static str`, `(String, u32)`, `BusyDev`), because cpal has no public `Device`
    /// constructor. That seam is what makes the POLICY testable on a machine with no sound card —
    /// and it is exactly what makes those tests blind to what the REAL `cpal::Device` returns. 689
    /// green tests said nothing about it. This one runs against the real host.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_linux_device_is_named_by_its_pcm_address_not_its_human_description() {
        let host = cpal::default_host();
        let mut seen = 0usize;
        for d in host
            .input_devices()
            .into_iter()
            .flatten()
            .chain(host.output_devices().into_iter().flatten())
        {
            let Ok(desc) = d.description() else { continue };
            let Some(pcm) = desc.driver() else { continue };
            seen += 1;
            assert_eq!(
                cpal_device_name(&d).as_deref(),
                Some(pcm),
                "a device must be named by the PCM id we persist and resolve on, not by its \
                 description ({:?}) — see this test's comment",
                desc.name()
            );
        }
        // ⚠️ A VACUOUS PASS IS NOT A PASS. A machine with no ALSA devices at all proves nothing
        // here, and must say so rather than going green in silence.
        assert!(
            seen > 0,
            "no ALSA device was enumerated, so this test asserted NOTHING — it is not evidence \
             that device naming survived the cpal 0.18 migration"
        );
    }

    /// And the property that actually matters, end to end: the name we hand the operator's config
    /// must resolve BACK to the same device through the real enumerator. Naming and resolution are
    /// two halves of one contract, and testing them apart is how they drifted.
    #[test]
    fn every_enumerated_device_resolves_by_the_name_we_give_it() {
        let host = cpal::default_host();
        let names: Vec<String> = host
            .input_devices()
            .into_iter()
            .flatten()
            .filter_map(|d| cpal_device_name(&d))
            .collect();
        for want in &names {
            let got = resolve_configured(
                || host.input_devices().ok(),
                Some(want.as_str()),
                None,
                "input",
            )
            .unwrap_or_else(|e| panic!("we named a device {want:?} that cannot be resolved: {e}"));
            assert_eq!(got.device_name().as_deref(), Some(want.as_str()));
        }
        assert!(
            !names.is_empty(),
            "no input device was enumerated, so this test asserted NOTHING"
        );
    }
}

/// SAMPLE-FORMAT COVERAGE: every format a host can hand us is carried, or refused by name.
#[cfg(test)]
mod format_tests {
    use super::{supported_device_format, unsupported_format, DeviceSample, DEVICE_FORMATS};
    use cpal::SampleFormat;

    /// A stand-in builder: it builds nothing, it just proves the dispatch has an arm for `T`.
    fn probe_format<T: DeviceSample>() -> Result<&'static str, String> {
        Ok(std::any::type_name::<T>())
    }

    /// ⭐ THE TWO LISTS ARE TIED, MECHANICALLY. `DEVICE_FORMATS` is what the monitor's config
    /// picker chooses from and `dispatch_format!` is what actually builds the stream; a format in
    /// one and not the other is either a format the picker chooses and the builder then refuses
    /// (that was #8 — "unsupported monitor output format: I8" on a card the decode path opens every
    /// session) or a format we can carry and never offer. This walks the list THROUGH the dispatch,
    /// so the two cannot drift without going red.
    #[test]
    fn every_listed_format_has_a_dispatch_arm() {
        for &f in DEVICE_FORMATS {
            assert!(
                dispatch_format!(f, probe_format, "test").is_ok(),
                "{f:?} is offered by supported_device_format but has no dispatch arm"
            );
            assert!(supported_device_format(f));
        }
        assert_eq!(
            DEVICE_FORMATS.len(),
            12,
            "every PCM format cpal 0.18 can report — the ten from 0.15 plus I24 and U24"
        );
    }

    /// ⭐ THE UPGRADE'S OWN OUTAGE RISK, CLOSED. `I24` did not exist as a `SampleFormat` on cpal
    /// 0.15 (commented out of the enum), so this test could not have been written before the bump.
    /// 0.18 adds it AND ranks it ABOVE the `I16` we already handled, so a 24-bit rig interface
    /// would have been handed a format with no arm — "unsupported sample format", no audio, on
    /// Windows and Linux, as the price of a macOS enumeration fix.
    #[test]
    fn the_24_bit_formats_the_upgrade_introduces_are_carried() {
        for f in [SampleFormat::I24, SampleFormat::U24] {
            assert!(supported_device_format(f), "{f:?} must be carried");
            assert!(dispatch_format!(f, probe_format, "test").is_ok());
        }
        // They outrank I16 in cpal's default heuristics, which is WHY they matter — and they
        // outrank it in ours too, so the monitor picker prefers 24-bit where a card offers both.
        assert!(
            super::format_quality_rank(SampleFormat::I24)
                > super::format_quality_rank(SampleFormat::I16)
        );
        assert!(
            super::format_quality_rank(SampleFormat::I32)
                > super::format_quality_rank(SampleFormat::I24)
        );

        // The whole ranking reads in one direction: wider beats narrower, signed beats unsigned at
        // the same width, floats beat everything. Pinned as a chain so inserting a format in the
        // wrong slot — which is exactly what adding I24/U24 did on the first attempt, leaving U24
        // above U32 — goes red instead of shipping a slightly worse monitor.
        let order = [
            SampleFormat::F32,
            SampleFormat::F64,
            SampleFormat::I32,
            SampleFormat::U32,
            SampleFormat::I24,
            SampleFormat::U24,
            SampleFormat::I16,
            SampleFormat::U16,
            SampleFormat::I64,
            SampleFormat::U64,
            SampleFormat::I8,
            SampleFormat::U8,
        ];
        for pair in order.windows(2) {
            assert!(
                super::format_quality_rank(pair[0]) > super::format_quality_rank(pair[1]),
                "{:?} must outrank {:?}",
                pair[0],
                pair[1]
            );
        }

        // The conversions: 24 significant bits in a 32-bit container, offset-binary for U24.
        assert!((cpal::I24::new_unchecked(4_194_304).to_unit() - 0.5).abs() < 1e-6);
        assert!((cpal::I24::new_unchecked(0).to_unit()).abs() < 1e-6);
        assert!(
            (cpal::U24::new_unchecked(8_388_608).to_unit()).abs() < 1e-6,
            "U24 silence is its offset midpoint"
        );
        for x in [-0.75f32, -0.25, 0.0, 0.25, 0.75] {
            assert!(
                (cpal::I24::from_unit(x).to_unit() - x).abs() < 1e-3,
                "I24 {x}"
            );
            assert!(
                (cpal::U24::from_unit(x).to_unit() - x).abs() < 1e-3,
                "U24 {x}"
            );
        }
        // Full scale stays inside the 24-bit range and keeps its sign — the same rail rule the
        // other integer formats get, on a type whose container is wider than its values.
        assert_eq!(cpal::I24::from_unit(1.0).inner(), 8_388_607);
        assert_eq!(cpal::I24::from_unit(-1.0).inner(), -8_388_607);
        assert_eq!(
            cpal::I24::from_unit(9.0).inner(),
            8_388_607,
            "over the rail is held"
        );
        assert_eq!(cpal::U24::from_unit(1.0).inner(), 16_777_215);
        assert_eq!(cpal::U24::from_unit(-1.0).inner(), 1);
        assert_eq!(cpal::U24::from_unit(9.0).inner(), 16_777_215);
    }

    /// ⚠️ DSD IS REFUSED, DELIBERATELY, AND THAT IS NOT AN OMISSION. cpal 0.18 adds three DSD
    /// variants: 1-bit sigma-delta streams where a byte is EIGHT TIME-SAMPLES, not one amplitude.
    /// Scaling one as though it were a PCM sample produces noise, and shipping noise to a
    /// transmitter is worse than refusing to open. They must reach the operator as a named error.
    ///
    /// This is the test that stops someone "completing" the dispatch later.
    #[test]
    fn dsd_is_refused_by_name_rather_than_carried_as_noise() {
        for f in [
            SampleFormat::DsdU8,
            SampleFormat::DsdU16,
            SampleFormat::DsdU32,
        ] {
            assert!(!supported_device_format(f), "{f:?} must not be carried");
            assert_eq!(
                super::format_quality_rank(f),
                0,
                "{f:?} must never be chosen where we pick a format"
            );
            let err = dispatch_format!(f, probe_format, "output")
                .expect_err("DSD must be refused, not scaled");
            assert!(err.contains(&format!("{f:?}")), "names the format: {err}");
        }
    }

    /// ⭐ THE FOUR FORMATS OPERATORS ARE ALREADY ON THE AIR WITH ARE BIT-EXACT.
    ///
    /// The conversions moved out of sixteen hand-written match arms into `DeviceSample`, and the
    /// one thing that must NOT have changed is the arithmetic for the four that shipped: a
    /// different RX scale moves every reported SNR, and a different TX scale moves the drive into
    /// the rig. These are the literal expressions the old arms contained. cpal's own `dasp_sample`
    /// conversions were the obvious shortcut and would have failed this test by an LSB.
    #[test]
    fn the_shipped_conversions_are_unchanged() {
        // to_unit — the old input arms.
        assert_eq!(0.25f32.to_unit(), 0.25, "f32: straight through");
        assert_eq!(1234i16.to_unit(), 1234f32 / 32768.0);
        assert_eq!(200u8.to_unit(), (200f32 - 128.0) / 128.0);
        assert_eq!(1_234_567i32.to_unit(), 1_234_567f32 / 2_147_483_648.0);
        // from_unit — the old output arms, including f32 staying UNCLAMPED.
        assert_eq!(f32::from_unit(0.25), 0.25);
        assert_eq!(f32::from_unit(2.5), 2.5, "f32 output was never clamped");
        assert_eq!(i16::from_unit(0.5), (0.5f32 * 32767.0) as i16);
        assert_eq!(u8::from_unit(0.5), (0.5f32 * 127.0 + 128.0) as u8);
        assert_eq!(i32::from_unit(0.5), (0.5f32 * 2_147_483_647.0) as i32);
    }

    /// FULL SCALE MUST NOT WRAP. The clamp on the integer formats is the whole reason they have
    /// one: `(1.0 * 32768.0) as i16` is `-32768`, an inverted spike at the loudest instant of an
    /// over. Both rails, every integer format.
    #[test]
    fn full_scale_does_not_wrap_on_any_integer_format() {
        // The invariant is the SIGN, at both rails and beyond them. `(1.0 * 32768.0) as i16` is
        // -32768 — an inverted spike at the loudest instant of an over — and that is what the
        // clamp and the 32767 scale exist to prevent.
        fn rails<T: DeviceSample + Into<i128> + Copy>(name: &str) {
            let hi: i128 = T::from_unit(1.0).into();
            let lo: i128 = T::from_unit(-1.0).into();
            assert!(hi > 0, "{name}: +full scale must stay positive, got {hi}");
            assert!(lo < 0, "{name}: -full scale must stay negative, got {lo}");
            // ...and OVER the rail is held there, not wrapped: the positive control, since a
            // conversion with no clamp passes the two assertions above and fails these.
            assert_eq!(T::from_unit(9.0).into(), hi, "{name}: over the top rail");
            assert_eq!(
                T::from_unit(-9.0).into(),
                lo,
                "{name}: under the bottom rail"
            );
        }
        rails::<i8>("i8");
        rails::<i16>("i16");
        rails::<i32>("i32");
        rails::<i64>("i64");

        // Unsigned formats are offset-binary: the rails are the ends of the range, and the
        // midpoint is silence. A wrap here shows up as full scale landing next to silence.
        assert_eq!(u8::from_unit(1.0), 255);
        assert_eq!(u8::from_unit(-1.0), 1);
        assert_eq!(u8::from_unit(0.0), 128, "silence is the offset midpoint");
        assert_eq!(u16::from_unit(1.0), 65535);
        assert_eq!(u16::from_unit(-1.0), 1);
        assert_eq!(u16::from_unit(0.0), 32768);
        assert_eq!(
            u8::from_unit(9.0),
            255,
            "over the rail is held, not wrapped"
        );
        assert_eq!(u8::from_unit(-9.0), 1);

        // The exact shipped i16 rails, spelled out — this is the format most rig codecs use.
        assert_eq!(i16::from_unit(1.0), 32767);
        assert_eq!(i16::from_unit(-1.0), -32767);
    }

    /// Every format carries a signal at roughly the right amplitude — including the six that were
    /// refused outright until now. Round-trip through the device representation and back.
    #[test]
    fn every_format_round_trips_a_signal() {
        fn round_trip<T: DeviceSample>(x: f32) -> f32 {
            T::from_unit(x).to_unit()
        }
        for x in [-0.75f32, -0.25, 0.0, 0.25, 0.75] {
            // The coarsest format here is u8/i8 at ~1/128 per step, so one LSB is the bar.
            let tol = 1.0 / 100.0;
            assert!((round_trip::<f32>(x) - x).abs() < 1e-6, "f32 {x}");
            assert!((round_trip::<f64>(x) - x).abs() < 1e-6, "f64 {x}");
            assert!((round_trip::<i8>(x) - x).abs() < tol, "i8 {x}");
            assert!((round_trip::<i16>(x) - x).abs() < tol, "i16 {x}");
            assert!((round_trip::<i32>(x) - x).abs() < tol, "i32 {x}");
            assert!((round_trip::<i64>(x) - x).abs() < tol, "i64 {x}");
            assert!((round_trip::<u8>(x) - x).abs() < tol, "u8 {x}");
            assert!((round_trip::<u16>(x) - x).abs() < tol, "u16 {x}");
            assert!((round_trip::<u32>(x) - x).abs() < tol, "u32 {x}");
            assert!((round_trip::<u64>(x) - x).abs() < tol, "u64 {x}");
        }
    }

    /// ⚠️ A FORMAT WE CANNOT CARRY IS AN ERROR THAT NAMES IT, NEVER SILENCE. This string reaches
    /// the operator as a persistent banner; "unsupported format" alone sends him to a forum, so it
    /// names what the device offered AND what would work. `SampleFormat` is `#[non_exhaustive]`, so
    /// this arm is reachable on any cpal version — and after the 0.18 upgrade it is where the 1-bit
    /// DSD formats land, which cannot be carried by scaling and must stay refused.
    #[test]
    fn a_format_we_cannot_carry_is_named_in_the_error() {
        let msg = unsupported_format("input", SampleFormat::I64);
        // (I64 IS carried — this only exercises the wording, and the assertion below keeps the
        // two facts from being confused.)
        assert!(supported_device_format(SampleFormat::I64));
        assert!(msg.contains("I64"), "names what the device offered: {msg}");
        assert!(msg.contains("input"), "names which direction: {msg}");
        assert!(msg.contains("F32"), "names what would work instead: {msg}");
        assert!(
            msg.contains("sound settings"),
            "tells him what to do about it: {msg}"
        );
    }
}

/// ⭐ #172 (KR8MER): the realtime discipline of the TX/RX path — an audio callback must never be
/// stoppable by the thread that feeds it.
#[cfg(test)]
mod tx_realtime_tests {
    use super::{queue_to_card, should_report_stream_error, update_rx_meter, SpscRing, TxBlock};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// ⭐ THE PRIORITY INVERSION, #172. Severe TX dropouts on Windows, dramatically worse on
    /// battery — the reporter found the symptom, this is the mechanism.
    ///
    /// The cpal OUTPUT callback runs on a realtime audio thread and used to LOCK a
    /// `std::sync::Mutex` every buffer, while the producer (the radio loop, an ordinary thread)
    /// held that same lock across `ring.extend(dev)` — a whole upsampled FT over, ~630k f32 at
    /// 48 kHz, with the `VecDeque` reallocation INSIDE the critical section. A realtime callback
    /// blocking on a non-realtime thread's lock is the classic inversion: when Windows parks cores
    /// on battery the critical section outruns the callback deadline, the callback emits SILENCE,
    /// and the operator hears gated TX. It explains AC-vs-battery, why raising process priority
    /// helped, and why WSJT-X on identical hardware is clean.
    ///
    /// The test is the property, not a stopwatch (a latency threshold is unusable here — an
    /// ordinary CI preemption is milliseconds): while a whole over is being queued, the
    /// callback-side drain must keep making progress. Under the mutex it made NONE, because it was
    /// parked on the producer's lock for the entire copy. Written against the mutex first, it
    /// reported 11 passes against a bar of 10,000 — zero versus tens of thousands, so nothing here
    /// rests on how fast the machine is.
    #[test]
    fn queueing_a_whole_over_never_stops_the_output_callback() {
        // 60 s of device-rate audio: an over is 13.14 s, but a bigger block makes the producer's
        // critical section unmistakable on fast hardware without changing the mechanism.
        let over = vec![0.25f32; 48_000 * 60];
        let ring = Arc::new(SpscRing::new(over.len() * 2));
        let level = Arc::new(AtomicU32::new(1.0f32.to_bits()));

        let stop = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicUsize::new(0));
        let consumer = {
            let (ring, level, stop, progress) =
                (ring.clone(), level.clone(), stop.clone(), progress.clone());
            std::thread::spawn(move || {
                // The real output callback: a `TxBlock` over the same ring, one frame per pass.
                // An empty ring is silence, so this keeps running whether or not audio is queued.
                while !stop.load(Ordering::Relaxed) {
                    let mut blk = TxBlock::begin(&ring, &level);
                    blk.next();
                    blk.finish();
                    progress.fetch_add(1, Ordering::Relaxed);
                }
            })
        };

        // Wait until the callback stand-in is demonstrably running — otherwise "no progress"
        // would prove only that the thread had not started (the negative-result control).
        while progress.load(Ordering::Relaxed) < 10_000 {
            std::hint::spin_loop();
        }

        for round in 0..3 {
            let before = progress.load(Ordering::Relaxed);
            queue_to_card(&ring, &over);
            let during = progress.load(Ordering::Relaxed) - before;
            assert!(
                during > 10_000,
                "round {round}: the output callback made only {during} passes while a whole over \
                 was queued — it was parked on the producer, which is the #172 dropout"
            );
            ring.request_flush();
            ring.apply_flush();
        }

        stop.store(true, Ordering::Relaxed);
        consumer.join().unwrap();
    }

    /// The RX meter is the same defect one size down: it lived behind a `Mutex<f32>` that the
    /// realtime INPUT callback took every block. As an atomic it still smooths the same way —
    /// the reading has to remain a WSJT-X-comparable RMS, not just a lock-free number.
    #[test]
    fn the_rx_meter_smooths_without_a_lock() {
        let meter = AtomicU32::new(0.0f32.to_bits());
        let read = || f32::from_bits(meter.load(Ordering::Relaxed));

        // A block of constant 0.5 amplitude: RMS 0.5, folded in at (1 - decay).
        update_rx_meter(&meter, 0.25 * 8.0, 8);
        let first = read();
        assert!(
            first > 0.0 && first < 0.5,
            "one block moves part-way: {first}"
        );

        // Repeated blocks converge upward on the true RMS...
        for _ in 0..200 {
            update_rx_meter(&meter, 0.25 * 8.0, 8);
        }
        assert!(
            (read() - 0.5).abs() < 1e-3,
            "converges on the RMS: {}",
            read()
        );

        // ...and silence decays it back down, which is what makes the meter readable.
        for _ in 0..200 {
            update_rx_meter(&meter, 0.0, 8);
        }
        assert!(read() < 1e-3, "decays to silence: {}", read());

        // An empty block is not a data point (n == 0), and must not decay the meter — the
        // positive control for the guard at the top of the function.
        update_rx_meter(&meter, 0.0, 8);
        let held = read();
        update_rx_meter(&meter, 999.0, 0);
        assert_eq!(read(), held, "a zero-length block leaves the meter alone");
    }

    /// ⭐ #139's other half: a 294 MB, 2.76-million-line log. cpal 0.15's ALSA worker retries a
    /// POLLERR in a tight loop and calls the error callback every pass; every one of those printed
    /// a line. The first few are the diagnosis and the rest are a disk-filling machine.
    #[test]
    fn a_stream_error_spin_cannot_fill_the_log() {
        // The diagnosis still gets through — the control, since a rate limiter that reported
        // NOTHING would pass any test that only counts lines.
        for n in 1..=3 {
            assert!(
                should_report_stream_error(n),
                "occurrence {n} is the report"
            );
        }
        // M0LHJ's actual flood: 2.76 M occurrences must not be 2.76 M lines.
        let printed = (1..=2_760_000u32)
            .filter(|&n| should_report_stream_error(n))
            .count();
        assert!(
            printed < 30,
            "2.76 M occurrences printed {printed} lines — that is the 294 MB log again"
        );
        // ...and it still reports at increasing scale, so a persistent fault is visible as one.
        assert!(should_report_stream_error(1024));
        assert!(should_report_stream_error(1_048_576));
        assert!(!should_report_stream_error(1_000_000));
    }
}

#[cfg(test)]
mod tests {

    /// #99 (Xiegu DE-19) + #104 (QRP Labs QDX), both Windows 11: WASAPI gives BOTH
    /// endpoints of one USB-audio rig the SAME friendly name, and `input_devices()`
    /// yields only capture endpoints — so cloning the input as the output handed the
    /// output stream a capture endpoint and cpal answered "The requested stream type
    /// is not supported by the device", failing the whole duplex open. Two directions
    /// never share a device on an endpoint-grained host, however alike their names.
    #[test]
    fn a_windows_endpoint_pair_sharing_one_friendly_name_is_not_one_device() {
        use super::{shares_one_device, DeviceGrain};
        let qdx = "Digital Audio Interface (2- QDX Transceiver)";
        assert!(!shares_one_device(DeviceGrain::Endpoint, Some(qdx), qdx));
        // ...and the ALSA cases the shortcut exists for (#2, #8) still share one card.
        assert!(shares_one_device(
            DeviceGrain::Card,
            Some("plughw:CARD=CODEC,DEV=0"),
            "plughw:CARD=CODEC,DEV=0"
        ));
        assert!(shares_one_device(
            DeviceGrain::Card,
            Some("plughw:CARD=CODEC,DEV=0"),
            "hw:CARD=CODEC,DEV=0"
        ));
        // Blank/absent output name never shares, either grain.
        assert!(!shares_one_device(
            DeviceGrain::Card,
            None,
            "hw:CARD=CODEC,DEV=0"
        ));
        assert!(!shares_one_device(
            DeviceGrain::Card,
            Some("  "),
            "hw:CARD=CODEC,DEV=0"
        ));
    }
    use super::{pick_device, resolve_configured, NamedDevice};
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;

    /// A stand-in with REAL ALSA busy semantics: each device holds its CARD while alive
    /// (registered in `held`, released on Drop), and the iterator refuses to yield a hint
    /// whose card is currently held — exactly cpal's probe-open behavior, where a busy
    /// hint is silently skipped. This is the mechanism of #2: `hw:CARD=X` is yielded
    /// (card held), then `plughw:CARD=X` probes BUSY unless the hw device was dropped
    /// first. A resolver that collects the iterator can never see the plughw hint; a
    /// lazy resolver that drops between probes can.
    struct BusyDev {
        name: String,
        card: String,
        held: Rc<RefCell<HashSet<String>>>,
    }
    impl NamedDevice for BusyDev {
        fn device_name(&self) -> Option<String> {
            Some(self.name.clone())
        }
    }
    impl Drop for BusyDev {
        fn drop(&mut self) {
            self.held.borrow_mut().remove(&self.card);
        }
    }

    fn busy_host(
        hints: &[(&str, &str)],
        held: &Rc<RefCell<HashSet<String>>>,
    ) -> impl Iterator<Item = BusyDev> {
        let held = held.clone();
        hints
            .iter()
            .map(|(n, c)| (n.to_string(), c.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(move |(name, card)| {
                if held.borrow().contains(&card) {
                    return None; // BUSY — cpal silently skips the hint
                }
                held.borrow_mut().insert(card.clone());
                Some(BusyDev {
                    name,
                    card,
                    held: held.clone(),
                })
            })
    }

    /// THE #2 MECHANISM, pinned. The menu's saved pick is the card's `plughw:` hint; the
    /// same card's `hw:` hint enumerates FIRST. Resolution must drop each probed device
    /// before the next hint probes, or the card is busy against itself and the operator's
    /// explicit pick is unresolvable forever (the fallback-to-default loop akhepcat
    /// captured). The `held` registry is the positive control: a collecting resolver
    /// leaves `hw` held when `plughw` probes and this test goes red.
    #[test]
    fn a_cards_later_alias_resolves_because_the_earlier_probe_was_dropped() {
        let held = Rc::new(RefCell::new(HashSet::new()));
        let dev = resolve_configured(
            || {
                Some(busy_host(
                    &[
                        ("hw:CARD=CODEC,DEV=0", "CODEC"),
                        ("plughw:CARD=CODEC,DEV=0", "CODEC"),
                        ("plughw:CARD=Realtek,DEV=0", "Realtek"),
                    ],
                    &held,
                ))
            },
            Some("plughw:CARD=CODEC,DEV=0"),
            None,
            "input",
        )
        .expect("the saved plughw pick must resolve once probes are dropped between hints");
        assert_eq!(dev.name, "plughw:CARD=CODEC,DEV=0");
    }

    /// A stand-in host — cpal exposes no `Device` constructor, so this is the only way to
    /// exercise the policy on a machine with no sound card.
    ///
    /// It reports RAW names, the way a real host does: the " #N" ordinal is a thing OUR
    /// picker adds, so a two-rig station really does present two devices with the SAME
    /// name. (Writing the fixture the other way was the first thing these tests caught.)
    fn host() -> Vec<(String, u32)> {
        vec![
            ("Microphone (USB Audio CODEC)".to_string(), 1),
            ("Line In (Realtek)".to_string(), 2),
            ("Microphone (USB Audio CODEC)".to_string(), 3),
        ]
    }
    fn default_dev() -> (String, u32) {
        ("Line In (Realtek)".to_string(), 2)
    }

    /// FIX C, the behaviour change. An EMPTY selection is "System default" and still
    /// falls back; a name the operator explicitly chose that resolves to nothing is an
    /// error he can see, NOT a silent switch to whatever the OS calls default (which on a
    /// laptop is the built-in microphone, with TX audio going to the speakers while PTT
    /// still keys the rig — a dead carrier on the air that looked like it was working).
    #[test]
    fn an_unresolvable_explicit_choice_is_an_error_not_the_default() {
        let err = resolve_configured(
            || Some(host().into_iter()),
            Some("plughw:CARD=CODEC,DEV=0"),
            Some(default_dev()),
            "input",
        )
        .unwrap_err();
        assert!(err.contains("plughw:CARD=CODEC,DEV=0"), "{err}");
        // The load-bearing half is above and in `unwrap_err()`: an explicit choice that cannot
        // be resolved is an ERROR, never a silent fall back to the default — that is what put a
        // dead carrier on the air. The message itself changed in #2: it used to assert "in use
        // by another application" without establishing it, which sent akhepcat chasing a busy
        // device across two releases when the real asymmetry is that the menu and the open path
        // enumerate differently. It now reports what the backend actually offered.
        assert!(
            err.contains("input device(s)"),
            "says what it could see: {err}"
        );
        assert!(
            !err.contains("in use by another application"),
            "must not assert a cause it has not established: {err}"
        );
    }

    #[test]
    fn an_empty_selection_still_means_the_system_default() {
        for name in [None, Some(""), Some("   ")] {
            assert_eq!(
                resolve_configured(
                    || Some(host().into_iter()),
                    name,
                    Some(default_dev()),
                    "input"
                ),
                Ok(default_dev()),
                "empty selection {name:?} must keep falling back"
            );
        }
    }

    #[test]
    fn a_resolvable_choice_resolves_including_the_ordinal_suffix() {
        // The " #N" suffix is the ADDRESS of the SECOND identically-named codec; losing it
        // would send a two-rig station to the wrong radio. Ids 1 and 3 share a name, so the
        // id is the only proof the right one came back.
        assert_eq!(
            resolve_configured(
                || Some(host().into_iter()),
                Some("Microphone (USB Audio CODEC) #2"),
                Some(default_dev()),
                "input",
            ),
            Ok(("Microphone (USB Audio CODEC)".to_string(), 3))
        );
        // ...and the bare name is still the FIRST, so existing single-rig configs resolve
        // exactly as they always did.
        assert_eq!(
            pick_device(
                Some(host().into_iter()),
                Some("Microphone (USB Audio CODEC)"),
                None
            ),
            Some(("Microphone (USB Audio CODEC)".to_string(), 1))
        );
    }

    /// The voice mic's precedent, unchanged: no default means no fallback, ever.
    #[test]
    fn pick_device_without_a_default_still_returns_none() {
        assert_eq!(
            pick_device(Some(host().into_iter()), Some("no such device"), None),
            None
        );
    }

    /// The label is NOT an address. A Linux operator's stored value is the ALSA PCM name,
    /// and resolving what he READS ("USB AUDIO CODEC") must not accidentally work — if it
    /// ever did, something would be persisting labels.
    #[test]
    fn a_label_never_resolves_as_a_device_name() {
        let alsa = vec![("plughw:CARD=CODEC,DEV=0".to_string(), 7)];
        assert!(resolve_configured(
            || Some(alsa.clone().into_iter()),
            Some("USB AUDIO CODEC"),
            Some(default_dev()),
            "input"
        )
        .is_err());
    }

    /// WINDOWS / macOS REGRESSION GUARD, on the REAL path (this test does not exist on
    /// Linux). Whatever the runner's sound hardware is, every entry the picker offers must
    /// render its own cpal name — i.e. the DTO widening changed no visible string there.
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_devices_are_their_own_labels() {
        let (input, output) = super::available_devices();
        for d in input.iter().chain(output.iter()) {
            assert_eq!(
                d.name, d.label,
                "cpal's name must be what the operator reads"
            );
        }
    }
}

#[cfg(test)]
mod resolve_diagnostics {
    use super::{enumerated_default, resolve_configured, NamedDevice};

    impl NamedDevice for &'static str {
        fn device_name(&self) -> Option<String> {
            Some((*self).to_string())
        }
    }

    /// The macOS default-input disconnect fix (mac QA audit merged[48]): "" must resolve to
    /// the ENUMERATED handle for the same device — the one cpal attaches its disconnect
    /// listener to — not the `default_input_device()` handle. The (name, id) stand-in tells
    /// the two apart the same way the pick_device ordinal tests do.
    #[test]
    fn enumerated_default_swaps_in_the_listener_carrying_twin() {
        let got = enumerated_default(
            || {
                Some(
                    vec![
                        ("Built-in Microphone".to_string(), 1),
                        ("Rig Codec".to_string(), 2),
                    ]
                    .into_iter(),
                )
            },
            ("Rig Codec".to_string(), 0),
        );
        assert_eq!(got, ("Rig Codec".to_string(), 2));
    }

    /// No twin on the list, or no list at all → keep the original default handle: today's
    /// exact behavior, so the swap can never make an open fail that used to succeed.
    #[test]
    fn enumerated_default_keeps_the_handle_when_no_twin_exists() {
        let untouched = enumerated_default(
            || Some(vec![("Other Mic".to_string(), 1)].into_iter()),
            ("Rig Codec".to_string(), 0),
        );
        assert_eq!(untouched, ("Rig Codec".to_string(), 0));
        let unenumerable = enumerated_default(
            || None::<std::vec::IntoIter<(String, u32)>>,
            ("Rig Codec".to_string(), 0),
        );
        assert_eq!(unenumerable, ("Rig Codec".to_string(), 0));
    }

    /// The success path must be untouched by the diagnostics — collecting the list to report
    /// on failure must not stop a present device resolving. (My first attempt at this returned
    /// Err even on a hit.)
    #[test]
    fn a_present_device_still_resolves() {
        let got = resolve_configured(
            || Some(["plughw:CARD=CODEC,DEV=0", "default"].into_iter()),
            Some("plughw:CARD=CODEC,DEV=0"),
            None,
            "output",
        );
        assert_eq!(got.ok(), Some("plughw:CARD=CODEC,DEV=0"));
    }

    /// #2 (akhepcat): the message used to assert "missing, or in use by another application"
    /// while establishing neither. It now reports what the backend actually offered, which is
    /// the difference between a report we can act on and two releases of guessing.
    #[test]
    fn a_missing_device_names_what_the_backend_did_offer() {
        let err = resolve_configured(
            || Some(["default", "plughw:CARD=Generic,DEV=0"].into_iter()),
            Some("plughw:CARD=CODEC,DEV=0"),
            None,
            "output",
        )
        .unwrap_err();
        assert!(
            err.contains("plughw:CARD=CODEC,DEV=0"),
            "names what was asked for: {err}"
        );
        assert!(
            err.contains("2 output device(s)"),
            "counts what was seen: {err}"
        );
        assert!(
            err.contains("plughw:CARD=Generic,DEV=0"),
            "lists them: {err}"
        );
        // The old wording claimed a cause it had not established.
        assert!(
            !err.contains("missing, or in use by another application"),
            "must not assert a cause it did not establish: {err}"
        );
    }

    /// An empty list is its own diagnosis — and must not read as a list of nothing.
    #[test]
    fn an_empty_backend_list_says_so() {
        let err = resolve_configured(
            || None::<std::vec::IntoIter<&'static str>>,
            Some("plughw:CARD=CODEC,DEV=0"),
            None,
            "input",
        )
        .unwrap_err();
        assert!(err.contains("0 input device(s)"), "{err}");
    }

    /// No name configured still falls back to the system default, unchanged.
    #[test]
    fn no_configured_name_uses_the_default() {
        let got = resolve_configured(
            || Some(["a"].into_iter()),
            None,
            Some("the-default"),
            "output",
        );
        assert_eq!(got.ok(), Some("the-default"));
    }

    /// ⭐ #8 (mw0cqu), the exact capture. On 1.2.0 his saved input was
    /// `plughw:CARD=Device,DEV=0`, but cpal's enumerator offered his USB card ONLY as
    /// `hw:CARD=Device,DEV=0` (his four-device list, verbatim) — the `plughw:` name is never
    /// yielded for that card, so the exact-name pass can never match it however lazily it
    /// iterates, and his rig read "not available" while its FFT bounced. The card-identity
    /// fallback resolves the saved `plughw:CARD=Device` to cpal's `hw:CARD=Device` — same
    /// physical card, the only access path cpal offered. This is the fix the earlier lazy
    /// #2/#8 work did NOT cover: that one assumed the plughw hint was yielded but busy.
    #[test]
    fn a_saved_plughw_resolves_to_the_cards_hw_alias_when_thats_all_cpal_offers() {
        let got = resolve_configured(
            || {
                Some(
                    [
                        "default:CARD=PCH",
                        "sysdefault:CARD=PCH",
                        "dsnoop:CARD=PCH,DEV=0",
                        "hw:CARD=Device,DEV=0",
                    ]
                    .into_iter(),
                )
            },
            Some("plughw:CARD=Device,DEV=0"),
            None,
            "input",
        );
        assert_eq!(
            got.ok(),
            Some("hw:CARD=Device,DEV=0"),
            "the saved plughw pick must fall back to the same card's hw alias"
        );
    }

    /// The fallback matches the CARD, never a different rig. With his exact list but the saved
    /// card absent entirely, there is no mate and it stays an error — a two-radio operator's
    /// pick must not silently jump to the other rig.
    #[test]
    fn the_card_fallback_does_not_cross_to_a_different_card() {
        let err = resolve_configured(
            || Some(["hw:CARD=PCH,DEV=0", "dsnoop:CARD=PCH,DEV=0"].into_iter()),
            Some("plughw:CARD=Device,DEV=0"),
            None,
            "input",
        )
        .unwrap_err();
        assert!(err.contains("plughw:CARD=Device,DEV=0"), "{err}");
    }

    /// REGRESSION GUARD for the two-pass ordering: when the saved `plughw:` name IS enumerated,
    /// the exact pass must win and return it — NOT the same card's `hw:` alias picked up by the
    /// fallback. plughw does the rate/format conversion the app relies on; silently downgrading
    /// a working plughw pick to raw hw would be a regression the fallback must not cause.
    #[test]
    fn an_exact_plughw_still_beats_the_hw_card_mate() {
        let got = resolve_configured(
            || Some(["hw:CARD=Device,DEV=0", "plughw:CARD=Device,DEV=0"].into_iter()),
            Some("plughw:CARD=Device,DEV=0"),
            None,
            "input",
        );
        assert_eq!(got.ok(), Some("plughw:CARD=Device,DEV=0"));
    }
}
