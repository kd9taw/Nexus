//! Real sound-card audio via `cpal` (feature `device`).
//!
//! Opens the default input and output devices, downmixes input to mono, fans
//! mono output to all channels, and resamples between the device's native rate
//! and the modem's 12 kHz. The RX/decode path uses a stateful, anti-aliased
//! decimator ([`crate::capture_resample::CaptureResampler`]); TX playback keeps
//! the plain linear resample (upsampling has no aliasing hazard). The cpal
//! callbacks (which run on an audio thread)
//! exchange device-rate samples with this struct through lock-guarded rings;
//! [`AudioBackend::capture`]/[`AudioBackend::play`] do the resampling on the
//! caller's thread.
//!
//! Device/rate selection here is the conservative default; on a real station you
//! may want to pick a specific CODEC device and a 48 kHz config explicitly.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};

use crate::audiodev::{split_device_ordinal, AudioDevice};
use crate::backend::AudioBackend;
use crate::capture_resample::CaptureResampler;
use crate::monitor::{Monitor, SpscRing};
use crate::resample::resample_linear;

const MODEM_RATE: u32 = 12_000;

/// Pop the next mono TX sample and apply the level **as it stands right now**.
///
/// ⚠️ THE ATOMIC IS READ PER SAMPLE, DELIBERATELY. Issue #14 was that the level was baked into
/// each sample in [`AudioBackend::play`] and then queued, so `out_ring` held committed amplitudes
/// and a slider move could not reach anything already in flight. The ring is deep — the FT slot
/// path enqueues a whole 13.14 s over in one call — so "already in flight" meant seconds, and the
/// operator overshot chasing a control that had not caught up. Reading here is what makes the
/// change audible on the next buffer instead. A relaxed load per sample is a plain move on every
/// target we ship; hoisting it per block would save nothing measurable and would put the liveness
/// back out of reach of a test.
#[inline]
fn next_tx_sample(ring: &mut VecDeque<f32>, level: &AtomicU32) -> f32 {
    ring.pop_front().unwrap_or(0.0) * f32::from_bits(level.load(Ordering::Relaxed))
}

fn err_fn(e: cpal::StreamError) {
    eprintln!("tempo-audio: cpal stream error: {e}");
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
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    (
        devices_from_cpal_names(inputs),
        devices_from_cpal_names(outputs),
    )
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
        self.name().ok()
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
pub(crate) fn resolve_configured<D: NamedDevice>(
    devices: Option<impl Iterator<Item = D>>,
    name: Option<&str>,
    default: Option<D>,
    what: &str,
) -> Result<D, String> {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        None => default.ok_or_else(|| format!("this system has no default {what} device")),
        Some(wanted) => {
            // Collected first so a FAILURE can name what the open path could actually see.
            // The old message guessed at a cause ("missing, or in use by another application")
            // and named neither, which is how #2 (akhepcat) stayed unexplained across two
            // releases: the operator picks from OUR ALSA-hint enumeration, which opens nothing,
            // while resolution happens against CPAL's, which probe-opens every hint and drops
            // whatever will not open. A name that is legitimately on the menu can therefore be
            // absent HERE — and be reported as in use when nothing of the sort was established.
            let all: Vec<D> = devices.map(|it| it.collect()).unwrap_or_default();
            let seen: Vec<String> = all.iter().filter_map(|d| d.device_name()).collect();
            pick_device(Some(all.into_iter()), Some(wanted), None).ok_or_else(|| {
                format!(
                    "audio {what} device {wanted:?} is not available. The audio backend offered \
                     {n} {what} device(s) when opening{list} If it is on the menu but not in \
                     that list, the backend dropped it while probing — commonly because it is \
                     already open, which selecting one card for BOTH input and output can do.",
                    n = seen.len(),
                    list = if seen.is_empty() {
                        ".".to_string()
                    } else {
                        format!(": {}.", seen.join(", "))
                    },
                )
            })
        }
    }
}

/// Real sound-card backend. Keep it alive for the duration of operation — the
/// cpal streams stop when this is dropped.
pub struct CpalBackend {
    _in_stream: Stream,
    _out_stream: Stream,
    in_ring: Arc<Mutex<VecDeque<f32>>>,
    out_ring: Arc<Mutex<VecDeque<f32>>>,
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
    /// Smoothed RX input RMS (0.0–1.0), updated on the audio thread. Rendered as
    /// a WSJT-X-style dB level in the UI.
    rx_level: Arc<Mutex<f32>>,
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
    /// Optional TX-audio tee (Flex native DAX): when set, every [`Self::play`] also hands the 12 kHz
    /// samples to this closure so TX audio reaches the radio over DAX in parallel with the soundcard.
    tx_tee: Option<crate::backend::TxTee>,
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
        let rate = cfg.sample_rate().0;
        let ch = cfg.channels() as usize;
        let ring = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let ring_cb = ring.clone();
        let stream = match cfg.sample_format() {
            SampleFormat::F32 => dev.build_input_stream(
                &cfg.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut r = ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks(ch.max(1)) {
                        r.push_back(frame.iter().copied().sum::<f32>() / ch.max(1) as f32);
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => dev.build_input_stream(
                &cfg.config(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut r = ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks(ch.max(1)) {
                        let m = frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                            / ch.max(1) as f32;
                        r.push_back(m);
                    }
                },
                err_fn,
                None,
            ),
            // Radio USB CODEC mics may advertise U8 or I32 — handle them too.
            SampleFormat::U8 => dev.build_input_stream(
                &cfg.config(),
                move |data: &[u8], _: &cpal::InputCallbackInfo| {
                    let mut r = ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks(ch.max(1)) {
                        let m = frame
                            .iter()
                            .map(|&s| (s as f32 - 128.0) / 128.0)
                            .sum::<f32>()
                            / ch.max(1) as f32;
                        r.push_back(m);
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I32 => dev.build_input_stream(
                &cfg.config(),
                move |data: &[i32], _: &cpal::InputCallbackInfo| {
                    let mut r = ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks(ch.max(1)) {
                        let m = frame
                            .iter()
                            .map(|&s| s as f32 / 2_147_483_648.0)
                            .sum::<f32>()
                            / ch.max(1) as f32;
                        r.push_back(m);
                    }
                },
                err_fn,
                None,
            ),
            other => return Err(format!("unsupported voice-mic input format: {other:?}")),
        }
        .map_err(|e| e.to_string())?;
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
        let in_dev = resolve_configured(
            host.input_devices().ok(),
            in_name,
            host.default_input_device(),
            "input",
        )?;
        let out_dev = resolve_configured(
            host.output_devices().ok(),
            out_name,
            host.default_output_device(),
            "output",
        )?;

        let in_cfg = in_dev.default_input_config().map_err(|e| e.to_string())?;
        let out_cfg = out_dev.default_output_config().map_err(|e| e.to_string())?;
        let in_rate = in_cfg.sample_rate().0;
        let out_rate = out_cfg.sample_rate().0;
        let in_ch = in_cfg.channels() as usize;
        let out_ch = out_cfg.channels() as usize;

        let in_ring = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let out_ring = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let rx_level = Arc::new(Mutex::new(0.0f32));
        let rx_gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        // TX level starts at unity and is read by the output callback each block — see the field's
        // doc for why it is applied on the way OUT rather than baked in at generation.
        let tx_level = Arc::new(AtomicU32::new(1.0f32.to_bits()));

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
        let mon_level = Arc::new(AtomicU32::new(0.5f32.to_bits()));

        // ---- input: fold to mono f32 (dominant lane × RX gain) → in_ring (+ peak meter) ----
        let in_ring_cb = in_ring.clone();
        let rx_meter_cb = rx_level.clone();
        let mon_ring_in = mon_ring.clone();
        let tap_ring_in = tap_ring.clone();
        let mon_enabled_in = mon_enabled.clone();
        let rx_gain_cb = rx_gain.clone();
        let in_stream = match in_cfg.sample_format() {
            SampleFormat::F32 => in_dev.build_input_stream(
                &in_cfg.config(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut ring = in_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    // Read the monitor gate ONCE per callback (not per sample). When on,
                    // push each mono sample into the wait-free monitor ring — it never
                    // blocks or allocates and drops on overflow, so the decode path
                    // (this same callback) is never stalled by monitoring.
                    let monitoring = mon_enabled_in.load(Ordering::Relaxed);
                    let g = f32::from_bits(rx_gain_cb.load(Ordering::Relaxed));
                    let ch = in_ch.max(1);
                    let mut sum_sq = 0.0f32;
                    let mut n = 0usize;
                    for frame in data.chunks(ch) {
                        // Fold to mono by AVERAGING the channels (× RX gain). Averaging keeps the
                        // signal phase-coherent across the whole FT8 window no matter how the rig's
                        // codec lays mono onto a stereo stream. Per-block "loudest lane" picking
                        // (0.8.9) thrashed L↔R on a hiss channel and shredded decodes on stereo
                        // interfaces (Flex DAX, Xiegu DE-19) — a quiet rig is handled by RX Gain,
                        // not by discarding a channel.
                        let m = frame.iter().copied().sum::<f32>() / ch as f32 * g;
                        sum_sq += m * m;
                        n += 1;
                        ring.push_back(m);
                        // Tee to the waterfall producer (see rxtap.rs). Wait-free: atomics
                        // only, never blocks, never allocates, drops on overflow — the same
                        // discipline the monitor ring uses, so the decode path (this callback)
                        // is never stalled by it. UNGATED, unlike the monitor: the waterfall is
                        // always live, and an undrained ring simply fills and drops.
                        tap_ring_in.push(m);
                        if monitoring {
                            mon_ring_in.push(m);
                        }
                    }
                    update_rx_meter(&rx_meter_cb, sum_sq, n);
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => in_dev.build_input_stream(
                &in_cfg.config(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut ring = in_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    let monitoring = mon_enabled_in.load(Ordering::Relaxed);
                    let g = f32::from_bits(rx_gain_cb.load(Ordering::Relaxed));
                    let ch = in_ch.max(1);
                    let mut sum_sq = 0.0f32;
                    let mut n = 0usize;
                    for frame in data.chunks(ch) {
                        let m =
                            frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / ch as f32 * g;
                        sum_sq += m * m;
                        n += 1;
                        ring.push_back(m);
                        // Tee to the waterfall producer (see rxtap.rs). Wait-free: atomics
                        // only, never blocks, never allocates, drops on overflow — the same
                        // discipline the monitor ring uses, so the decode path (this callback)
                        // is never stalled by it. UNGATED, unlike the monitor: the waterfall is
                        // always live, and an undrained ring simply fills and drops.
                        tap_ring_in.push(m);
                        if monitoring {
                            mon_ring_in.push(m);
                        }
                    }
                    update_rx_meter(&rx_meter_cb, sum_sq, n);
                },
                err_fn,
                None,
            ),
            // Radio USB CODECs (e.g. the IC-9700) may advertise U8 or I32 capture
            // rather than F32/I16 — handle them so RX capture opens on any rig.
            SampleFormat::U8 => in_dev.build_input_stream(
                &in_cfg.config(),
                move |data: &[u8], _: &cpal::InputCallbackInfo| {
                    let mut ring = in_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    let monitoring = mon_enabled_in.load(Ordering::Relaxed);
                    let g = f32::from_bits(rx_gain_cb.load(Ordering::Relaxed));
                    let ch = in_ch.max(1);
                    let mut sum_sq = 0.0f32;
                    let mut n = 0usize;
                    for frame in data.chunks(ch) {
                        // U8 is offset-binary around 128.
                        let m = frame
                            .iter()
                            .map(|&s| (s as f32 - 128.0) / 128.0)
                            .sum::<f32>()
                            / ch as f32
                            * g;
                        sum_sq += m * m;
                        n += 1;
                        ring.push_back(m);
                        // Tee to the waterfall producer (see rxtap.rs). Wait-free: atomics
                        // only, never blocks, never allocates, drops on overflow — the same
                        // discipline the monitor ring uses, so the decode path (this callback)
                        // is never stalled by it. UNGATED, unlike the monitor: the waterfall is
                        // always live, and an undrained ring simply fills and drops.
                        tap_ring_in.push(m);
                        if monitoring {
                            mon_ring_in.push(m);
                        }
                    }
                    update_rx_meter(&rx_meter_cb, sum_sq, n);
                },
                err_fn,
                None,
            ),
            SampleFormat::I32 => in_dev.build_input_stream(
                &in_cfg.config(),
                move |data: &[i32], _: &cpal::InputCallbackInfo| {
                    let mut ring = in_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    let monitoring = mon_enabled_in.load(Ordering::Relaxed);
                    let g = f32::from_bits(rx_gain_cb.load(Ordering::Relaxed));
                    let ch = in_ch.max(1);
                    let mut sum_sq = 0.0f32;
                    let mut n = 0usize;
                    for frame in data.chunks(ch) {
                        let m = frame
                            .iter()
                            .map(|&s| s as f32 / 2_147_483_648.0)
                            .sum::<f32>()
                            / ch as f32
                            * g;
                        sum_sq += m * m;
                        n += 1;
                        ring.push_back(m);
                        // Tee to the waterfall producer (see rxtap.rs). Wait-free: atomics
                        // only, never blocks, never allocates, drops on overflow — the same
                        // discipline the monitor ring uses, so the decode path (this callback)
                        // is never stalled by it. UNGATED, unlike the monitor: the waterfall is
                        // always live, and an undrained ring simply fills and drops.
                        tap_ring_in.push(m);
                        if monitoring {
                            mon_ring_in.push(m);
                        }
                    }
                    update_rx_meter(&rx_meter_cb, sum_sq, n);
                },
                err_fn,
                None,
            ),
            other => return Err(format!("unsupported input sample format: {other:?}")),
        }
        .map_err(|e| e.to_string())?;

        // ---- output: mono f32 from out_ring → all channels ----
        let out_ring_cb = out_ring.clone();
        let tx_level_cb = tx_level.clone();
        let out_stream = match out_cfg.sample_format() {
            SampleFormat::F32 => out_dev.build_output_stream(
                &out_cfg.config(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut ring = out_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks_mut(out_ch.max(1)) {
                        let s = next_tx_sample(&mut ring, &tx_level_cb);
                        for x in frame.iter_mut() {
                            *x = s;
                        }
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => out_dev.build_output_stream(
                &out_cfg.config(),
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut ring = out_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks_mut(out_ch.max(1)) {
                        let v = (next_tx_sample(&mut ring, &tx_level_cb).clamp(-1.0, 1.0) * 32767.0)
                            as i16;
                        for x in frame.iter_mut() {
                            *x = v;
                        }
                    }
                },
                err_fn,
                None,
            ),
            // Radio USB CODECs (e.g. the IC-9700) may advertise U8 or I32 playback
            // rather than F32/I16 — handle them so TX/output opens on any rig.
            SampleFormat::U8 => out_dev.build_output_stream(
                &out_cfg.config(),
                move |data: &mut [u8], _: &cpal::OutputCallbackInfo| {
                    let mut ring = out_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks_mut(out_ch.max(1)) {
                        let v = (next_tx_sample(&mut ring, &tx_level_cb).clamp(-1.0, 1.0) * 127.0
                            + 128.0) as u8;
                        for x in frame.iter_mut() {
                            *x = v;
                        }
                    }
                },
                err_fn,
                None,
            ),
            SampleFormat::I32 => out_dev.build_output_stream(
                &out_cfg.config(),
                move |data: &mut [i32], _: &cpal::OutputCallbackInfo| {
                    let mut ring = out_ring_cb.lock().unwrap_or_else(|e| e.into_inner());
                    for frame in data.chunks_mut(out_ch.max(1)) {
                        let v = (next_tx_sample(&mut ring, &tx_level_cb).clamp(-1.0, 1.0)
                            * 2_147_483_647.0) as i32;
                        for x in frame.iter_mut() {
                            *x = v;
                        }
                    }
                },
                err_fn,
                None,
            ),
            other => return Err(format!("unsupported output sample format: {other:?}")),
        }
        .map_err(|e| e.to_string())?;

        in_stream.play().map_err(|e| e.to_string())?;
        out_stream.play().map_err(|e| e.to_string())?;

        Ok(Self {
            _in_stream: in_stream,
            _out_stream: out_stream,
            in_ring,
            out_ring,
            // The device output rate lives inside tx_rs now — play() resamples
            // through it, so the raw rate no longer needs a field of its own.
            capture_rs: CaptureResampler::new(in_rate, MODEM_RATE),
            tx_rs: CaptureResampler::new(MODEM_RATE, out_rate),
            rx_level,
            rx_gain,
            tx_level,
            monitor: Monitor::new(mon_ring, mon_enabled, mon_level, in_rate),
            spectrum_tap: tap_ring,
            in_rate,
            voice_mic: None,
            tx_tee: None,
        })
    }
}

/// Fold a callback's RMS into the smoothed RX meter. The stored value is the
/// normalized RMS (0..1) of the post-gain audio — the frontend renders it as a
/// WSJT-X-style dB level (20·log10(rms)+90.3). RMS (not peak) is what makes the
/// reading comparable to WSJT-X's meter. Exponentially smoothed for stability.
fn update_rx_meter(meter: &Arc<Mutex<f32>>, sum_sq: f32, n: usize) {
    if n == 0 {
        return;
    }
    let rms = (sum_sq / n as f32).sqrt().clamp(0.0, 1.0);
    let mut lvl = meter.lock().unwrap_or_else(|e| e.into_inner());
    *lvl = *lvl * RX_METER_DECAY + rms * (1.0 - RX_METER_DECAY);
}

impl AudioBackend for CpalBackend {
    fn spectrum_tap(&self) -> Option<(Arc<SpscRing>, u32)> {
        Some((self.spectrum_tap.clone(), self.in_rate))
    }

    fn capture(&mut self) -> Vec<f32> {
        let dev: Vec<f32> = {
            let mut ring = self.in_ring.lock().unwrap_or_else(|e| e.into_inner());
            ring.drain(..).collect()
        };
        // Anti-aliased, stateful decimation to 12 kHz (see `capture_rs`). Carries
        // filter history + fractional phase across calls, so no block-boundary
        // discontinuity and no long-run drift. The voice-mic path below keeps the
        // plain linear resample: that audio is a recorded voice message, not
        // decoded, so aliasing is harmless there.
        self.capture_rs.process(&dev)
    }

    fn play(&mut self, samples: &[f32]) {
        // Flex native DAX TX: also hand the modem's 12 kHz TX audio to the DAX encoder (parallel to
        // the soundcard; the TX schedule is untouched — this is the same audio, another route).
        if let Some(tee) = &self.tx_tee {
            tee(samples);
        }
        // Anti-aliased, stateful UPsample 12 kHz → device rate (see `tx_rs`). The old
        // `resample_linear` here put a periodic amplitude ripple on the constant-envelope
        // FT8/FT4 waveform; the polyphase reconstruction keeps it flat like WSJT-X.
        let dev = self.tx_rs.process(samples);
        // UNSCALED on purpose — the level is applied by the output callback as each sample
        // leaves. See `tx_level`: baking it in here is what made the Pwr slider unable to change
        // audio that was already queued.
        let mut ring = self.out_ring.lock().unwrap_or_else(|e| e.into_inner());
        ring.extend(dev.iter().copied());
    }

    fn set_tx_tee(&mut self, tee: Option<crate::backend::TxTee>) {
        self.tx_tee = tee;
    }

    /// Current RX input level (0.0–1.0): a decaying peak meter sampled on the
    /// audio thread. Diagnostics only since 2026-08-01 (`examples/audio_probe`) —
    /// the app UI meters via the rx-dsp thread's `MeterFeed` (see rxdsp.rs).
    fn rx_level(&self) -> f32 {
        *self.rx_level.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Set the Tx audio level (0.0–1.0) applied to outgoing samples in [`play`].
    ///
    /// [`play`]: AudioBackend::play
    fn set_tx_level(&mut self, level: f32) {
        self.tx_level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Set the RX capture gain (a ≥1.0 multiplier applied to captured samples on the audio
    /// thread). Live: the realtime input callback reads the atomic each block. Clamped to a
    /// sane 1.0–8.0 (+18 dB) so a stray value can't blow up the decode/monitor path.
    fn set_rx_gain(&mut self, gain: f32) {
        self.rx_gain
            .store(gain.clamp(1.0, 8.0).to_bits(), Ordering::Relaxed);
    }

    /// Discard queued-but-unplayed TX audio (hard Stop TX): clear the output ring
    /// so the current transmission is cut immediately, not at the slot's end.
    fn flush_output(&mut self) -> usize {
        let mut ring = self.out_ring.lock().unwrap_or_else(|e| e.into_inner());
        let n = ring.len();
        ring.clear();
        n
    }

    /// Reconfigure the dark headphone monitor in place (start/stop/retune its output
    /// stream) — the capture and TX streams are untouched, so the decode path never
    /// restarts.
    fn set_monitor(&mut self, enabled: bool, device: &str, level: f32) -> Result<(), String> {
        self.monitor.apply(enabled, device, level)
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
    use super::next_tx_sample;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn ring(v: &[f32]) -> VecDeque<f32> {
        v.iter().copied().collect()
    }

    /// ISSUE #14, and the whole point of the change: audio that is ALREADY QUEUED must respond to
    /// the Pwr slider.
    ///
    /// The level used to be multiplied into each sample inside `play` and then pushed, so
    /// `out_ring` held committed amplitudes. Nothing already queued could ever change, and the
    /// queue is deep — the FT slot path enqueues an entire 13.14 s over in one call — so the
    /// operator moved the slider, watched the ALC hold the old drive for seconds, and overshot
    /// chasing a control that had not caught up. The reporter measured exactly this against his
    /// rig's ALC meter while holding Tune.
    ///
    /// This is the test that could not exist while the multiply lived in the realtime callback,
    /// which is why the read was factored out rather than hoisted per block.
    #[test]
    fn a_level_change_reaches_audio_that_is_already_queued() {
        let mut r = ring(&[1.0, 1.0, 1.0, 1.0]);
        let level = AtomicU32::new(1.0f32.to_bits());

        assert_eq!(next_tx_sample(&mut r, &level), 1.0, "unity passes through");

        // The operator moves Pwr mid-transmission. Everything still in the ring was generated
        // before this instant — under the old code it would keep going out at full drive.
        level.store(0.25f32.to_bits(), Ordering::Relaxed);
        assert_eq!(next_tx_sample(&mut r, &level), 0.25);
        assert_eq!(next_tx_sample(&mut r, &level), 0.25);

        // ...and back up again, same buffer.
        level.store(0.5f32.to_bits(), Ordering::Relaxed);
        assert_eq!(next_tx_sample(&mut r, &level), 0.5);
    }

    /// The waveform is scaled, not gated: shape is preserved and only amplitude moves, which is
    /// what a real drive control does. A flush-and-refill would have stepped the envelope.
    #[test]
    fn the_queued_waveform_keeps_its_shape() {
        let mut r = ring(&[1.0, -0.5, 0.25, -1.0]);
        let level = AtomicU32::new(0.5f32.to_bits());
        let out: Vec<f32> = (0..4).map(|_| next_tx_sample(&mut r, &level)).collect();
        assert_eq!(out, vec![0.5, -0.25, 0.125, -0.5]);
    }

    /// An empty ring is silence, not the last sample held — and silence stays silence whatever
    /// the level says, so an underrun can never emit a click scaled up by a high drive setting.
    #[test]
    fn an_empty_ring_is_silence_at_every_level() {
        let mut r = ring(&[]);
        for bits in [0.0f32, 0.5, 1.0] {
            let level = AtomicU32::new(bits.to_bits());
            assert_eq!(next_tx_sample(&mut r, &level), 0.0);
        }
    }

    /// Zero really is zero. `set_tx_level` clamps to 0.0–1.0, and an operator who drags Pwr to
    /// the bottom mid-over must get silence out of the samples already queued.
    #[test]
    fn zero_silences_audio_that_was_already_queued() {
        let mut r = ring(&[1.0, -1.0]);
        let level = AtomicU32::new(0.0f32.to_bits());
        assert_eq!(next_tx_sample(&mut r, &level), 0.0);
        assert_eq!(next_tx_sample(&mut r, &level), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{pick_device, resolve_configured};

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
            Some(host().into_iter()),
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
                resolve_configured(Some(host().into_iter()), name, Some(default_dev()), "input"),
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
                Some(host().into_iter()),
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
            Some(alsa.into_iter()),
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
    use super::{resolve_configured, NamedDevice};

    impl NamedDevice for &'static str {
        fn device_name(&self) -> Option<String> {
            Some((*self).to_string())
        }
    }

    /// The success path must be untouched by the diagnostics — collecting the list to report
    /// on failure must not stop a present device resolving. (My first attempt at this returned
    /// Err even on a hit.)
    #[test]
    fn a_present_device_still_resolves() {
        let got = resolve_configured(
            Some(["plughw:CARD=CODEC,DEV=0", "default"].into_iter()),
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
            Some(["default", "plughw:CARD=Generic,DEV=0"].into_iter()),
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
            None::<std::vec::IntoIter<&'static str>>,
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
        let got = resolve_configured(Some(["a"].into_iter()), None, Some("the-default"), "output");
        assert_eq!(got.ok(), Some("the-default"));
    }
}
