//! PSK63 at the layer that actually keys: the radio loop's one-shot and latched PSK paths must
//! render PSK63 at its own 62.5 Bd — 192 samples a symbol at 12 kHz — with the idle preamble and
//! postamble kept at their air time (twice the bits). The modem itself is proven in
//! `tempo_core::psk`; these pin the WIRING, which picks the rate from the engine's sub-mode on
//! every tick (`PskTxConfig::for_mode`, `psk_over_bits_for`). Each test compares what the mock
//! card was handed, sample for sample, against the reference render of the same bits.

use super::*;
use tempo_core::psk::{bpsk_samples, psk_over_bits_for, PskModeKind, PskStream, PskTxConfig};

/// The Keyboard section on PSK63 with TX armed (as a manual mode does), set up further by `setup`.
fn psk63_engine(setup: impl FnOnce(&mut Engine)) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_operating_mode("keyboard", false);
        e.set_psk_mode(PskModeKind::Bpsk63, false).unwrap();
        setup(&mut e);
    }
    engine
}

/// Step the loop `ticks` × 20 ms from 100 ms on a VOX rig; the card and the rig after.
fn run(engine: &Arc<Mutex<Engine>>, ticks: usize) -> (MockBackend, Rig) {
    let (mut backend, mut rig, mut state) = (MockBackend::new(), Rig::vox(), loop_state());
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let mut t = 100.0;
    for _ in 0..ticks {
        state
            .step(
                engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        t += 20.0;
    }
    (backend, rig)
}

#[test]
fn a_psk63_message_keys_at_62_5_baud_with_its_preamble_at_full_air_time() {
    let engine = psk63_engine(|e| e.psk_send_text("CQ TEST").unwrap());
    let center = engine.lock().unwrap().psk_center_hz();
    let (backend, rig) = run(&engine, 300); // 6 s: the over is ~3.5 s
    let bits = psk_over_bits_for("CQ TEST", PskModeKind::Bpsk63);
    let want = bpsk_samples(&bits, &PskTxConfig::for_mode(center, PskModeKind::Bpsk63));
    assert_eq!(
        want.len(),
        (bits.len() + 1) * 192,
        "premise: 192 samples a symbol"
    );
    // CONTROL: the same bits at PSK31's rate are a different (twice as long) waveform, so the
    // comparison below can tell the rates apart.
    let at_31 = bpsk_samples(&bits, &PskTxConfig::for_mode(center, PskModeKind::Bpsk31));
    assert_eq!(at_31.len(), 2 * want.len());
    assert!(
        !backend.played.is_empty(),
        "premise: the message was keyed (rig keyed: {})",
        rig.keyed
    );
    assert_eq!(
        backend.played.len(),
        want.len(),
        "the one-shot over is not 62.5 Bd with the doubled idle"
    );
    assert!(
        backend.played == want,
        "the one-shot over is not the PSK63 render of its bits"
    );
}

#[test]
fn a_latched_psk63_over_idles_at_62_5_baud() {
    let engine = psk63_engine(|e| e.set_psk_latched(true).unwrap());
    let center = engine.lock().unwrap().psk_center_hz();
    let (backend, rig) = run(&engine, 50); // 1 s latched, nothing typed: idle reversals
    assert!(rig.keyed, "premise: the latch keyed the rig");
    assert!(!backend.played.is_empty(), "premise: the idle was fed");
    // Each idle tick renders one character time (10 bits) of reversals; un-tailed, n bits
    // render exactly n symbols, so what was fed is the reference stream over that many bits.
    let n = backend.played.len() / 192;
    assert_eq!(backend.played.len(), n * 192, "not whole 62.5 Bd symbols");
    let want = PskStream::new(PskTxConfig::for_mode(center, PskModeKind::Bpsk63))
        .chunk(&vec![false; n], false);
    assert!(
        backend.played == want,
        "the latched stream is not PSK63 idle at 62.5 Bd"
    );
}
