//! The JTTY RX decode thread — the armed-decoder-on-the-RX-path pattern (`pskrx.rs`,
//! `rttyrx.rs`). While the operator has JTTY armed, the engine's radio loop taps 12 kHz RX audio
//! onto a reception's own sample timeline; this thread runs the engine's
//! [`JttyDriver`] every ~100 ms, which takes that audio under a brief lock, decodes it OFF the lock
//! (WSJT-X's live receive pump, one quarter-frame step per call) and hands the messages back to the
//! engine for the `get_jtty_state` poll, ALL.TXT and, once a call is decoded twice, PSK Reporter.
//!
//! RX ONLY: nothing here keys PTT or emits TX audio. Disarmed, a tick is one brief lock and a flag
//! read, so everyone else pays nothing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::engine::jtty_rx::JttyDriver;
use tempo_app::engine::Engine;

use crate::service::SHUTDOWN;

/// Drain cadence: the PSK/RTTY threads' 100 ms. A JTTY search step is a quarter frame (0.472 s),
/// so most ticks find no step to run and cost a lock and a take.
const POLL: Duration = Duration::from_millis(100);

/// Spawn the JTTY RX decode thread (call once at startup, beside `spawn_psk_rx`).
pub fn spawn_jtty_rx(engine: Arc<Mutex<Engine>>) {
    std::thread::Builder::new()
        .name("jtty-rx".into())
        .spawn(move || run(engine))
        .expect("spawn jtty-rx");
}

fn run(engine: Arc<Mutex<Engine>>) {
    // The receive context lives here, not in the engine: its reception is decode-thread state,
    // and the decode must run with the engine lock released.
    let mut driver = JttyDriver::default();
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        driver.tick(&engine);
    }
}
