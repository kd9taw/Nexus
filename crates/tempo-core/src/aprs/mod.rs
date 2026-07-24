//! APRS over AFSK-1200 (Bell 202) + AX.25 — the packet-radio foundation.
//!
//! Built bottom-up, mirroring the RTTY modem's structure, all at the app's 12 kHz modem rate
//! (12000 / 1200 baud = exactly 10 samples/bit):
//!   * [`frame`] — AX.25 UI frames + the CRC-16/X.25 FCS.
//!   * [`hdlc`]  — flag framing, bit-stuffing, NRZI.
//!   * modem     — AFSK-1200 mark/space modulate + demodulate, in `tempo-audio` (next).
//!
//! No external crate: the CRC and the bit-level address codec are implemented here with inline
//! round-trip + known-vector tests, matching the house convention (see `rtty/`, `tempo-sstv`).
//!
//! References: AX.25 v2.2 §3 (frame format) and the APRS 1.0.1 spec (UI frames, PID 0xF0).

pub mod frame;
pub mod hdlc;

pub use frame::{fcs, Address, Frame, CONTROL_UI, PID_NO_L3};
pub use hdlc::{deframe, encode_frame, nrzi_decode, nrzi_encode, FLAG};
