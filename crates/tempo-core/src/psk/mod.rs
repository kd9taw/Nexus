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
//! so the transcript/print stage never learns a modulation. TX is Phase 2 of
//! the Keyboard Modes campaign and deliberately does not exist here; the only
//! modulator in this crate is test-only.

pub mod demod;
pub mod varicode;

pub use demod::{PskConfig, PskDemod, PskDemodulator, AFC_CLAMP_HZ, BAUD, SAMPLE_RATE};
pub use varicode::{VaricodeDecoder, VARICODE};
