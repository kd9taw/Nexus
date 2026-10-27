//! Nexus's audio across a network, at both ends of it: the station's receive audio fed to its
//! encoder and decoded where it is heard, a driving computer's microphone encoded and decoded at
//! the shack, the resamplers between their rates, the rings a headset plays and records through,
//! and the voice keyer's WAV. Nothing here opens a device or reaches the engine, so the client core
//! (`tempo-client`) builds on it alone; tempo-audio, which owns the sound devices and the radio,
//! names each module where it always was.
/// Stateful, anti-aliased capture-path resampler (device rate → 12 kHz). Pure
/// DSP — no audio device — so it builds and unit-tests without the `device`
/// feature; tempo-audio's `device::CpalBackend` owns one per capture stream.
pub mod capture_resample;
/// What any headset plays and records through, for a shack this computer drives over its own
/// network: the shack's audio and a tone out through a fixed cushion, the microphone in, on
/// wait-free rings. The devices behind it are tempo-audio's.
pub mod headset;
pub mod mic_decode;
/// The microphone of a computer driving a shack, encoded for the audio road: Opus voice, 20 ms,
/// 32 kbit/s constant, no DTX, no FEC. Pure; tested.
pub mod mic_encode;
pub mod receive_audio;
/// The shack's receive audio, decoded on a computer driving it: one road packet in, 20 ms at
/// 48 kHz out. Pure; tested.
pub mod receive_decode;
pub mod receive_encode;
pub mod resample;
/// The wait-free ring a realtime callback shares.
pub mod ring;
/// A notification sound's WAV file, read and checked (a regular file, a size cap, a plausible
/// RIFF/WAVE format) before a sample of it is played. Pure; tested.
pub mod sound_file;
pub mod tci_audio;
pub mod voice;
