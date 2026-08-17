//! The PSK31 RX decode thread — the armed-decoder-on-the-RX-path pattern (see
//! `rttyrx.rs` / `aicw.rs`). While the operator has the PSK cockpit armed
//! (`psk_arm` or the view-entry auto-arm), the engine's radio loop accumulates
//! 12 kHz RX audio in a drain buffer; this thread empties it every ~100 ms,
//! runs the `tempo_core::psk` demodulator OFF-lock, and pushes decoded
//! characters (+ per-char phase-margin confidence), the AFC correction and the
//! signal-presence flag back into the engine for the `get_psk_state` poll.
//!
//! RX ONLY: nothing here keys PTT or emits TX audio — no PSK transmit path
//! exists this phase, anywhere. Disarmed = the buffer stays empty and this
//! loop does nothing but a brief flag check, so everyone else pays nothing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::engine::{engine_lock, Engine};
use tempo_core::psk::{PskConfig, PskDemod, PskDemodulator};

use crate::service::SHUTDOWN;

/// Drain cadence: short enough that decoded text flows char-by-char, long
/// enough that the disarmed idle cost is negligible (one lock + bool read).
const POLL: Duration = Duration::from_millis(100);

/// Spawn the PSK RX decode thread (call once at startup, beside `spawn_rtty_rx`).
pub fn spawn_psk_rx(engine: Arc<Mutex<Engine>>) {
    std::thread::Builder::new()
        .name("psk-rx".into())
        .spawn(move || run(engine))
        .expect("spawn psk-rx");
}

fn run(engine: Arc<Mutex<Engine>>) {
    // The demodulator lives here, not in the engine: its FIR/sync state is
    // decode-thread-private, and dropping it on disarm makes every re-arm a
    // clean acquire (fresh AFC pull, fresh symbol clock). The tuned center
    // follows the engine's netted value (a waterfall click); a net or the
    // cockpit's Re-acquire drops + rebuilds it the same clean-acquire way.
    let mut demod: Option<PskDemodulator> = None;
    // The center the demod was built with.
    let mut applied = 0.0f32;
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        let (armed, center, reset) = {
            let mut e = engine_lock(&engine);
            (e.psk_armed(), e.psk_center_hz(), e.take_psk_afc_reset())
        };
        if !armed {
            demod = None;
            continue;
        }
        if reset || center != applied {
            demod = None; // rebuild below: fresh AFC pull on the new center
            applied = center;
        }
        let audio = engine_lock(&engine).take_psk_audio();
        if audio.is_empty() {
            continue;
        }
        let d = demod.get_or_insert_with(|| {
            PskDemodulator::new(PskConfig {
                center_hz: applied,
                ..PskConfig::default()
            })
        });
        // The heavy part — NCO, decimating FIR, matched filter, sync, slicer,
        // varicode — off-lock.
        let chars = d.feed(&audio);
        let (afc_hz, signal) = (d.afc_offset_hz(), d.signal_present());
        engine_lock(&engine).push_psk_decode(&chars, afc_hz, signal);
    }
}
