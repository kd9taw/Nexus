//! Tempo real-radio transport.
//!
//! Bridges the transport-agnostic [`tempo_app::engine::Engine`] to a real
//! station: sound-card audio (via `cpal`, behind the `device` feature) and
//! PTT/CAT via Hamlib's `rigctld` daemon over TCP (no `libhamlib` build
//! dependency — just run `rigctld`). The slot-clock loop ([`runtime::Transceiver`])
//! transmits the engine's `poll_tx` waveforms on TX slots (keying PTT) and feeds
//! captured 4-second frames to `ingest` on RX slots.
//!
//! ## Layers
//! - [`rig`] — PTT/CAT via rigctld TCP, serial RTS/DTR, or VOX no-op. Pure std; unit-tested.
//! - [`rigctld_proc`] — builds the `rigctld` command line and launches the daemon. Pure args; unit-tested.
//! - [`rigmodels`] — curated Hamlib rig-model table + name lookup for the UI. Pure; tested.
//! - [`ports`] — serial-port enumeration for the UI (feature `serial`; empty Vec otherwise).
//! - [`frames::RxRing`] — rolling buffer of the latest 4 s of audio. Pure; tested.
//! - [`backend::AudioBackend`] — capture/play seam; [`backend::MockBackend`] for tests.
//! - [`runtime::Transceiver`] — the slot loop tying it all together. Tested with a mock backend.
//! - `device::CpalBackend` (feature `device`) — real sound-card I/O via cpal.
//!
//! The pure layers compile and test with no audio libraries. Build the device
//! backend on the station PC with `--features device` (needs ALSA/CoreAudio/WASAPI
//! at build time and a sound card at runtime).

pub mod amplifier;
/// APRS (AFSK-1200 / AX.25) RX decode thread — same armed-decoder pattern, RX ONLY.
#[cfg(feature = "device")]
pub mod aprsrx;
/// The audio-device LIST the operator picks from, and the rules that build it
/// (ALSA hint → `{name, label}` naming + pruning). Pure string policy — no cpal, no
/// alsa-lib — so it unit-tests in the headless workspace build and on every CI
/// platform; `device::available_devices` supplies the live enumeration.
pub mod audiodev;
pub mod backend;
/// Test-CAT baud-ladder diagnosis for Icom CI-V rigs — probes the SAME serial port at
/// the other common CI-V rates when the configured probe got zero bytes, and composes
/// the exact "fix the app / fix the rig menu" verdict. Pure logic unit-tested; the real
/// probe rides the `serial` feature and runs only in the `test_cat` command context.
pub mod baud_ladder;
/// Stateful, anti-aliased capture-path resampler (device rate → 12 kHz). Pure
/// DSP — no audio device — so it builds and unit-tests without the `device`
/// feature; `device::CpalBackend` owns one per capture stream.
pub mod capture_resample;
pub mod civ;
/// Opening a serial port used ONLY for DTR/RTS control lines (CW keyline, FSK
/// keyline, serial PTT): the baud ladder that heals a rig refusing a given rate.
/// Pure fallback logic, unit-tested; the real open rides the `serial` feature.
pub mod control_line;
/// FlexRadio native DAX RX audio orchestrator (Phase 2) — same VITA-49 path as flexspectrum.
#[cfg(feature = "device")]
pub mod flexdax;
/// FlexRadio native panadapter orchestrator — needs tempo-net (SmartSDR/VITA parsers), so it
/// rides the `device` feature like the rest of the station-side transport code.
#[cfg(feature = "device")]
pub mod flexspectrum;
pub mod frames;
pub mod monitor;
/// OmniRig (VE3NEA's Windows COM rig-control server) as a CAT backend — a local
/// rigctld-protocol shim over COM, so `Rig` and every CAT verb are unchanged.
pub mod omnirig;
pub mod port_prober;
pub mod ports;
/// PSK31 RX decode thread (armed-decoder-on-the-RX-path, the `rttyrx` pattern).
/// RX side of PSK31 (TX runs in the radio loop — `service.rs`).
#[cfg(feature = "device")]
pub mod pskrx;
pub mod resample;
pub mod rig;
pub mod rigctld_proc;
pub mod rigctld_server;
pub mod rigmodels;
pub mod rotator;
pub mod rtty_afsk;
pub mod rtty_fsk;
/// RTTY RX decode thread (armed-decoder-on-the-RX-path, the `aicw` pattern).
/// RX ONLY — no TX path.
#[cfg(feature = "device")]
pub mod rttyrx;
pub mod runtime;
pub mod rxdsp;
pub mod rxtap;
pub mod serial_keyer;
pub mod slot;
/// SSTV image/gallery persistence helpers (BMP writer, preview downscale,
/// gallery.json load/save). Pure — no audio device, unit-tested.
pub mod sstv_store;
/// SSTV RX decode thread (VIS detect → progressive image → gallery save).
/// RX ONLY — no TX path.
#[cfg(feature = "device")]
pub mod sstvrx;
pub mod usbrig;
pub mod voice;
pub mod winkeyer;

#[cfg(all(feature = "device", feature = "ai-cw"))]
pub mod aicw;
#[cfg(feature = "device")]
pub mod device;
#[cfg(feature = "device")]
pub mod service;
