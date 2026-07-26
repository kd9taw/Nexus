//! TEMPORARY diagnostic probe (delete after use).
//! Q1: what RV does the sequencer actually put on the air, over by over?
//! Q2: does the FIRST over decode through the shipped path at a workable SNR?
//! Q3: RV-AWARE loopback QSO (engine-shaped: build_rv(tx_rv)) at a workable SNR —
//!     does the far station decode nothing, or does it decode?

use modes::Decode;
use tempo_core::channel::{self, VirtualAir, ON_TIME_OFFSET};
use tempo_core::qso::Station;
use tempo_core::{tempo_fast, tx};

#[test]
fn probe_rv_sequence_no_reply() {
    let mut s = Station::calling_cq("W9XYZ", "EN37");
    let mut seq = Vec::new();
    for _ in 0..7 {
        let (_m, rv) = s.outgoing_rv().expect("pending");
        seq.push(rv);
        s.after_tx();
    }
    println!("RV sequence, partner NEVER replies: {seq:?}");
}

#[test]
fn probe_rv0_first_over_decodes() {
    let msg = "CQ W9XYZ EN37";
    let f0 = 1500.0;
    for snr in [10.0f32, 0.0, -5.0] {
        let mut ok = 0;
        for seed in 0..8 {
            let w = tempo_fast::gen_wave(
                &tempo_fast::encode_rv(msg, 0),
                tempo_fast::SAMPLE_RATE,
                f0,
            );
            let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, seed);
            let rx = channel::to_i16(&air.receive(&w, ON_TIME_OFFSET, snr));
            tempo_fast::harq_reset();
            let d = tempo_fast::decode_frame(&rx, 200, 2900, 3, "", "", 0, 0);
            if d.iter().any(|x| x.message == msg) {
                ok += 1;
            }
        }
        println!("RV0 standalone at {snr:+.0} dB: {ok}/8 decoded");
    }
}

/// Engine-shaped RV-aware loopback: every over is built with `build_rv(tx_rv)`
/// exactly as `engine.rs:6634` does, with `tx_rv` taken from `outgoing_rv()`
/// exactly as `engine.rs:6550` does (harq_enabled = true).
#[test]
fn probe_rv_aware_loopback_qso() {
    for snr in [10.0f32, 0.0] {
        let mut a = Station::calling_cq("W9XYZ", "EN37");
        let mut b = Station::answering("K2DEF", "FN31", "W9XYZ");
        let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, 0xC0FFEE);
        tempo_fast::harq_reset();
        println!("--- RV-aware loopback @ {snr:+.0} dB ---");
        for slot in 0..12u64 {
            let (txs, rxs): (&mut Station, &mut Station) = if slot % 2 == 0 {
                (&mut a, &mut b)
            } else {
                (&mut b, &mut a)
            };
            let Some((msg, rv)) = txs.outgoing_rv() else {
                println!("slot {slot}: (nothing to send)");
                continue;
            };
            let text = msg.to_text();
            let frame = tx::build_rv(&text, tempo_fast::SAMPLE_RATE, 1500.0, rv as i32);
            let rx_f32 = air.receive(&frame.wave, ON_TIME_OFFSET, snr);
            let iwave = channel::to_i16(&rx_f32);
            let decodes: Vec<Decode> = tempo_fast::decode_frame(
                &iwave,
                200,
                2900,
                3,
                rxs.mycall.as_str(),
                txs.mycall.as_str(),
                0,
                (slot as i64).wrapping_mul(4000),
            )
            .into_iter()
            .map(Into::into)
            .collect();
            let heard: Vec<&str> = decodes.iter().map(|d| d.message.as_str()).collect();
            println!(
                "slot {slot}: {} TX rv{rv} \"{text}\"  -> far end decoded {heard:?}",
                txs.mycall
            );
            rxs.observe(&decodes);
            txs.after_tx();
        }
        println!("A transcript: {:?}", a.transcript);
        println!("B transcript: {:?}", b.transcript);
    }
}
