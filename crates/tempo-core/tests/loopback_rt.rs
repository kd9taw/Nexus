//! TX → virtual-air → RX through the real FT1 modem, using tempo-core's
//! transmit path and channel model with the known-timing `decode_rt` (offset 0).
//!
//! The full-acquisition variant (nonzero time/frequency offset via
//! `ft1_decode_frame`) is added once libtempo exposes the acquisition decoder.

use tempo_core::{channel::VirtualAir, tempo_fast, tx};

#[test]
fn tx_through_channel_decodes() {
    let msg = "CQ W9XYZ EN37";
    let frame = tx::build(msg, tempo_fast::SAMPLE_RATE, 1500.0);

    let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, 2024);
    let rx = air.receive(&frame.wave, 0, 10.0); // dt0 = 0, +10 dB SNR

    let decoded = tempo_fast::decode_rt(&rx, 1500.0, 10.0);
    assert!(
        decoded.ok(),
        "decode failed: ntype={} nharderror={}",
        decoded.ntype,
        decoded.nharderror
    );
    assert_eq!(decoded.message.as_deref(), Some(msg));
}
