//! The audio device seam: capture/play 12 kHz mono samples. Real hardware is
//! [`crate::device::CpalBackend`] (feature `device`); tests use [`MockBackend`].

use std::collections::VecDeque;

/// An ALTERNATE route for transmit audio — Flex native DAX, which carries the over to the radio
/// over the network instead of through a sound card.
///
/// ⚠️ EXCLUSIVE, NOT PARALLEL. While a tee is installed, [`AudioBackend::play`] hands the over to
/// the tee and NOT to the output device. It was parallel, and that was a shipped defect: the Flex
/// setup Nexus itself creates with one click points the output device at the "DAX TX" endpoint, so
/// the same over reached the radio TWICE by two routes with different rates, resampler states and
/// latencies — and when the operator's output is speakers instead, every native over played out
/// loud in the room (2026-08-17 Flex audit, finding #1051).
///
/// Being the only route brings the other two obligations with it: the tee applies the operator's
/// TX level ([`TxTee::set_level`]), because nothing downstream will, and it can be emptied
/// ([`TxTee::flush`]), because a hard Stop TX must cut whichever route is carrying the over.
pub trait TxTee: Send + Sync {
    /// Take one 12 kHz mono TX buffer. Called on the radio loop, with a whole over in one call —
    /// implementations must not block and must not do their own I/O inline.
    fn feed(&self, samples: &[f32]);
    /// Adopt the backend's TX audio level (0.0–1.0). Pushed on install and on every change, so the
    /// tee can apply it AS THE AUDIO LEAVES (the `next_tx_sample` rule — see `device.rs`).
    fn set_level(&self, level: f32);
    /// Discard anything queued on this route (hard Stop TX). Returns the count discarded.
    fn flush(&self) -> usize;
}

/// A shared handle to the installed [`TxTee`].
pub type TxTeeHandle = std::sync::Arc<dyn TxTee>;

/// A 12 kHz mono audio source/sink.
pub trait AudioBackend {
    /// 12 kHz mono samples captured since the last call (possibly empty).
    fn capture(&mut self) -> Vec<f32>;
    /// Queue 12 kHz mono samples for transmission.
    fn play(&mut self, samples: &[f32]);
    /// Smoothed RX input RMS (0.0–1.0), measured on the audio callback. Since 2026-08-01 the
    /// APP UI meters via the rx-dsp thread's `MeterFeed` (fast attack / standard decay,
    /// CAT-stall-immune) — this remains for standalone diagnostics (`examples/audio_probe`).
    /// Default 0 for non-hardware backends (the real sound card overrides it).
    fn rx_level(&self) -> f32 {
        0.0
    }
    /// The wait-free tee of the capture stream feeding the waterfall producer, with the device
    /// rate its samples are at. `None` for backends with no real capture stream (mocks, DAX-only
    /// paths) — the producer then simply has nothing to drain. See `rxtap.rs` for why the row is
    /// produced off the radio loop at all.
    fn spectrum_tap(&self) -> Option<(std::sync::Arc<crate::monitor::SpscRing>, u32)> {
        None
    }
    /// Set the TX audio level (0.0–1.0) applied to played samples. No-op default
    /// for non-hardware backends (the real sound card overrides it).
    fn set_tx_level(&mut self, _level: f32) {}
    /// Install (or clear with `None`) a TX-audio tee: while set, every [`AudioBackend::play`] hands
    /// the 12 kHz samples to the tee INSTEAD of the output device, so exactly one route carries an
    /// over. Used to send TX audio over Flex native DAX, WITHOUT changing the TX schedule. Default
    /// no-op; the real sound card overrides it. See [`TxTee`].
    fn set_tx_tee(&mut self, _tee: Option<TxTeeHandle>) {}
    /// Take the report of a decode-path stream the OS has KILLED since the last call, if any.
    ///
    /// A stream can die without any setting changing — the device unplugged, a CoreAudio
    /// reconfiguration when a Bluetooth headset connects, a codec dropping off the USB bus. The
    /// loop reopens the sound card only on a settings change, so without this a dead stream is
    /// permanent silence with no banner: blank waterfall, no decodes, nothing on screen saying
    /// why. Draining (rather than peeking) makes this edge-triggered — the caller reacts once per
    /// death and re-arms for the next.
    ///
    /// `None` for backends with no real streams (mocks, DAX-only paths); the real sound card
    /// overrides it.
    fn take_stream_error(&mut self) -> Option<String> {
        None
    }
    /// Set the RX capture gain (a multiplier ≥ 1.0 applied to the captured samples
    /// before decode). Headroom for a low-output interface — e.g. a rig codec whose
    /// line-out reads quiet in Nexus. No-op default; the real sound card overrides it.
    fn set_rx_gain(&mut self, _gain: f32) {}
    /// Discard any queued-but-not-yet-played TX audio immediately (a hard Stop TX
    /// mid-transmission). Default no-op; the real sound card clears its output
    /// ring. Returns the count discarded (for tests).
    fn flush_output(&mut self) -> usize {
        0
    }
    /// Start / stop / retune the dark headphone monitor in place, WITHOUT rebuilding
    /// the capture/TX streams (the decode path must never restart). `enabled` is the
    /// already-guard-resolved decision (the caller has refused any TX-device
    /// collision); `device` is the output device name ("" = system default); `level`
    /// is 0.0–1.0. `Err` = the monitor output device failed to open. Default no-op
    /// (non-hardware backends have no monitor); the real sound card overrides it.
    fn set_monitor(&mut self, _enabled: bool, _device: &str, _level: f32) -> Result<(), String> {
        Ok(())
    }
    /// Open (`Some(name)`) or close (`None`) a transient SECOND input stream capturing
    /// the operator's voice from a dedicated mic, used only while a recording is in
    /// progress — so "record a voice message" captures the mic, not the shared rig-codec
    /// input the decoder hears. Opening never touches the main capture/TX streams (the
    /// decode path never restarts). `Err` = the named device failed to open (the caller
    /// falls back to the shared capture tap). Default no-op; the real sound card overrides.
    fn set_voice_mic(&mut self, _device: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    /// 12 kHz mono samples captured from the voice-mic stream since the last call (empty
    /// when no mic stream is open). Default empty; the real sound card overrides it.
    fn voice_capture(&mut self) -> Vec<f32> {
        Vec::new()
    }
    /// Release the sound card NOW, in place, without dropping the backend value.
    ///
    /// ⚠️ Exists because **ALSA opens a card once**. The radio loop rebuilds audio by
    /// opening the replacement backend and only then dropping the old one at the swap —
    /// which means the probe of the new device runs while our OWN previous streams still
    /// hold the card. Changing to any device on the card you were already using therefore
    /// could not succeed: `audio input device "plughw:CARD=CODEC,DEV=0" is not available`
    /// (akhepcat, #2/#8; reproduced on 1.2.10, whose `tempo-audio` is byte-identical to
    /// 1.3.0's). Picking input and output in ONE save worked only because that opened both
    /// from a single fresh backend.
    ///
    /// After this returns the backend is INERT — `capture` yields nothing and `play` goes
    /// nowhere. The caller must replace it. It is not "pause": pausing a cpal stream keeps
    /// the ALSA handle, which is the very thing that has to go.
    ///
    /// Default no-op, so backends with no real device (mocks, DAX-only paths) are unaffected.
    fn release_device(&mut self) {}
}

/// Whether a recording should capture from the dedicated voice-mic device instead of
/// the shared input tap: only when a recording is actually in progress AND the operator
/// configured a (non-empty) voice-mic device. The pure decision behind opening the
/// transient second input stream (see the radio loop) — the source ACTUALLY fed to the
/// recorder also depends on that stream opening, since a failed open falls back to the
/// shared tap. Empty device = today's zero-surprise behavior (record the shared input).
pub fn want_voice_mic(recording_active: bool, voice_mic_device: &str) -> bool {
    recording_active && !voice_mic_device.trim().is_empty()
}

/// Does a LIVE MIC want the second input stream this tick?
///
/// `may_stream` is [`tempo_app::Engine::live_mic_may_stream`] — already the whole safety
/// question (PTT held, TX enabled, in privilege, under the hard ceiling), recomputed every
/// tick. This adds only the configuration half: a live mic must actually be chosen. The rig's
/// own mic is the default and streams nothing from this computer.
///
/// Split out as a pure function for the same reason as [`want_voice_mic`]: the decision is
/// then testable without a device, a rig, or a running loop.
pub fn want_live_mic(may_stream: bool, live_mic_device: &str) -> bool {
    may_stream && !live_mic_device.trim().is_empty()
}

/// WHO OWNS THE SINGLE SECOND-INPUT-STREAM SLOT this tick.
///
/// There is one transient capture stream ([`AudioBackend::set_voice_mic`]) and two claimants:
/// a voice-message RECORDING, and a LIVE MIC that is on the air. They cannot both have it.
///
/// **The live mic wins, and the asymmetry is deliberate.** If a recording took the slot in the
/// middle of an over, the operator's voice would stop reaching the air while PTT stayed keyed —
/// a dead carrier, which is the one failure the operator cannot hear happening. A spoiled
/// recording is noticed immediately and can simply be made again. Losing the recording is
/// recoverable; transmitting silence without knowing is not.
///
/// In practice the two do not overlap: recording a voice message is offline preparation and
/// holding PTT is not. This function exists so that the case is DECIDED rather than left to
/// whichever block happens to run first in the loop.
pub fn second_mic_owner(
    recording_active: bool,
    voice_mic_device: &str,
    live_may_stream: bool,
    live_mic_device: &str,
) -> SecondMic {
    if want_live_mic(live_may_stream, live_mic_device) {
        SecondMic::Live
    } else if want_voice_mic(recording_active, voice_mic_device) {
        SecondMic::Recording
    } else {
        SecondMic::None
    }
}

/// The claimant on the one transient capture stream. See [`second_mic_owner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondMic {
    /// Nobody: the slot stays closed and the shared tap is the only input.
    None,
    /// A voice-message recording is capturing the operator, off the air.
    Recording,
    /// A live mic is on the air right now.
    Live,
}

/// In-memory backend for tests: serves scripted capture chunks and records every
/// sample handed to `play`.
#[derive(Default)]
pub struct MockBackend {
    to_capture: VecDeque<Vec<f32>>,
    pub played: Vec<f32>,
    /// How many times `flush_output` was called (for hard-Stop-TX tests).
    pub flush_calls: usize,
    /// Scripted chunks the next `voice_capture()` calls return (voice-mic tests).
    to_voice_capture: VecDeque<Vec<f32>>,
    /// Every `set_voice_mic` argument, in order (for asserting open/close behavior).
    pub voice_mic_calls: Vec<Option<String>>,
    /// Whether a mock voice-mic stream is currently "open".
    pub voice_mic_open: bool,
    /// When true, `set_voice_mic(Some(_))` returns `Err` (simulates an open failure).
    pub voice_mic_fail: bool,
    /// Models an ALSA card that can only be open once: while this is held, a mock
    /// `reopen_audio` refuses to open the same card. `release_device()` clears it, which
    /// is exactly what the real backend does by dropping its streams.
    card: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Scripted stream death: the next `take_stream_error()` returns this and clears it,
    /// standing in for the OS killing a capture/playback stream out from under the loop.
    pub stream_error: Option<String>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self::default()
    }
    /// Queue a chunk that the next `capture()` will return.
    pub fn queue_capture(&mut self, samples: Vec<f32>) {
        self.to_capture.push_back(samples);
    }
    /// Queue a chunk that the next `voice_capture()` will return (voice-mic tests).
    pub fn queue_voice_capture(&mut self, samples: Vec<f32>) {
        self.to_voice_capture.push_back(samples);
    }
    /// Make this backend HOLD a (mock) exclusive sound card. The flag stays set until
    /// `release_device()`, so a test's `reopen_audio` can refuse the way ALSA does.
    pub fn holding(mut self, card: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        card.store(true, std::sync::atomic::Ordering::SeqCst);
        self.card = Some(card);
        self
    }
}

impl AudioBackend for MockBackend {
    fn release_device(&mut self) {
        // Mirrors the real backend: the card is let go, and this instance is inert after.
        if let Some(card) = self.card.take() {
            card.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        self.to_capture.clear();
    }
    fn capture(&mut self) -> Vec<f32> {
        self.to_capture.pop_front().unwrap_or_default()
    }
    fn take_stream_error(&mut self) -> Option<String> {
        self.stream_error.take()
    }
    fn play(&mut self, samples: &[f32]) {
        self.played.extend_from_slice(samples);
    }
    fn flush_output(&mut self) -> usize {
        self.flush_calls += 1;
        0
    }
    fn set_voice_mic(&mut self, device: Option<&str>) -> Result<(), String> {
        self.voice_mic_calls.push(device.map(str::to_string));
        match device {
            Some(_) if self.voice_mic_fail => Err("mock voice mic failed to open".to_string()),
            Some(_) => {
                self.voice_mic_open = true;
                Ok(())
            }
            None => {
                self.voice_mic_open = false;
                Ok(())
            }
        }
    }
    fn voice_capture(&mut self) -> Vec<f32> {
        self.to_voice_capture.pop_front().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn want_voice_mic_only_when_recording_and_a_device_is_set() {
        assert!(
            want_voice_mic(true, "USB Mic"),
            "recording + device → use the mic"
        );
        assert!(
            !want_voice_mic(false, "USB Mic"),
            "not recording → shared input (no idle second stream)"
        );
        assert!(
            !want_voice_mic(true, ""),
            "no device → shared input (today's zero-surprise default)"
        );
        assert!(
            !want_voice_mic(true, "   "),
            "whitespace-only device → shared input"
        );
        assert!(!want_voice_mic(false, ""));
    }
}

#[cfg(test)]
mod second_mic_tests {
    use super::{second_mic_owner, want_live_mic, SecondMic};

    // THE CONTROL FOR EVERY CASE BELOW. `want_live_mic` is an AND of two independent
    // conditions, so a test that only ever passes both cannot tell a working gate from
    // `fn want_live_mic(..) -> bool { true }`. Each half is shown to refuse on its own.
    #[test]
    fn a_live_mic_needs_both_the_safety_gate_and_a_chosen_device() {
        assert!(want_live_mic(true, "USB Mic"), "both satisfied — must stream");
        assert!(
            !want_live_mic(false, "USB Mic"),
            "the engine gate said no (PTT up / TX off / out of privilege / over the ceiling); \
             a chosen device must not override it"
        );
        assert!(
            !want_live_mic(true, ""),
            "no live mic chosen — the rig's own mic is the default and streams nothing"
        );
        assert!(
            !want_live_mic(true, "   "),
            "whitespace is not a device name; it must read as 'not chosen', not as a device"
        );
    }

    #[test]
    fn nobody_wants_the_slot_when_neither_is_configured() {
        assert_eq!(
            second_mic_owner(false, "", false, ""),
            SecondMic::None,
            "idle: the slot stays closed and the shared tap is the only input"
        );
    }

    #[test]
    fn a_recording_takes_the_slot_when_no_live_mic_is_on_the_air() {
        assert_eq!(
            second_mic_owner(true, "USB Mic", false, "USB Mic"),
            SecondMic::Recording,
            "PTT is up, so the live mic is not streaming and the recording may have the slot"
        );
    }

    #[test]
    fn the_live_mic_takes_the_slot_from_a_recording_while_it_is_on_the_air() {
        // The asymmetry this whole enum exists for. Losing the recording is recoverable;
        // a dead carrier — PTT keyed with the operator's voice going nowhere — is the one
        // failure the operator cannot hear, so the on-air claimant wins.
        assert_eq!(
            second_mic_owner(true, "Rec Mic", true, "Live Mic"),
            SecondMic::Live,
            "both claim it while transmitting: the on-air path must win"
        );
    }

    #[test]
    fn a_recording_does_not_lose_the_slot_to_a_live_mic_that_is_only_configured() {
        // Configured but not keying is the ordinary state of a station with a live mic set
        // up. It must not starve recordings for the rest of time.
        assert_eq!(
            second_mic_owner(true, "Rec Mic", false, "Live Mic"),
            SecondMic::Recording,
            "a live mic that is not on the air has no claim at all"
        );
    }
}
