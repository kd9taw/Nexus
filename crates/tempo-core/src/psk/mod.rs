//! PSK31 — varicode character layer + BPSK demodulator (receive side).
//!
//! [`varicode`] is the pure character codec (the G3PLX table: self-framing
//! variable-length codes separated by `00`, idle = continuous zeros). [`demod`]
//! is the receive DSP, written from scratch for Nexus: NCO mix at the tuned
//! audio frequency → ×24 decimation to 500 Hz (16 samples/symbol at 31.25 Bd)
//! → one-symbol raised-cosine matched filter → 16-phase symbol sync →
//! differential slicer → varicode decode, with a slew-limited AFC clamped to
//! ±25 Hz and a signal-quality squelch that gates the printed output only.
//!
//! Every decoded character carries a soft confidence (0..1) from the slicer's
//! phase-error margin, and the demodulator sits behind the mode-neutral
//! [`crate::textmode::TextDemod`] seam — the same ensemble RTTY decodes into,
//! so the transcript/print stage never learns a modulation.
//!
//! [`modulator`] is the transmit half (Keyboard Modes Phase 2): cosine-shaped
//! reversals at the tuned offset, idle = continuous reversals, resumable
//! chunked rendering for continuous TX — proven against [`demod`] by the
//! TX→RX loopback tests there.

pub mod demod;
pub mod modulator;
pub mod varicode;

pub use demod::{PskConfig, PskDemod, PskDemodulator, AFC_CLAMP_HZ, BAUD, SAMPLE_RATE};
pub use modulator::{bpsk_samples, psk_over_bits, PskStream, PskTxConfig, TX_DRIVE};
pub use varicode::{encode_bits, VaricodeDecoder, VARICODE};
