//! The APRS RX decode thread — the armed-decoder-on-the-RX-path pattern (see `rttyrx.rs`).
//!
//! While the operator has APRS armed (`aprs_arm`), the engine's radio loop accumulates 12 kHz RX
//! audio in a drain buffer; this thread empties it every ~100 ms, runs the streaming AFSK-1200
//! demodulator + AX.25 deframer OFF-lock, decodes each recovered frame into an `AprsPacket`, and
//! pushes a flattened `AprsHeard` back to the engine for the cockpit poll.
//!
//! RX ONLY: nothing here keys PTT or emits TX audio. Disarmed = the buffer stays empty and this
//! loop does nothing but a brief flag check, so everyone else pays nothing.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempo_app::engine::{AprsHeard, Engine};
use tempo_core::aprs::{AprsPacket, Deframer, Demod};

use crate::service::SHUTDOWN;

/// Drain cadence: short enough that packets surface promptly, long enough that the disarmed idle
/// cost is negligible (one lock + bool read).
const POLL: Duration = Duration::from_millis(100);

/// Spawn the APRS RX decode thread (call once at startup, beside `spawn_rtty_rx`).
pub fn spawn_aprs_rx(engine: Arc<Mutex<Engine>>) {
    std::thread::Builder::new()
        .name("aprs-rx".into())
        .spawn(move || run(engine))
        .expect("spawn aprs-rx");
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn run(engine: Arc<Mutex<Engine>>) {
    // Streaming decoder state is thread-private (like RTTY's demod): dropped + rebuilt on disarm so
    // every re-arm is a clean acquire (fresh timing PLL, fresh frame sync).
    let mut decoder: Option<(Demod, Deframer)> = None;
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        let armed = match engine.lock() {
            Ok(e) => e.aprs_armed(),
            Err(_) => continue,
        };
        if !armed {
            decoder = None;
            continue;
        }
        let audio = match engine.lock() {
            Ok(mut e) => e.take_aprs_audio(),
            Err(_) => continue,
        };
        if audio.is_empty() {
            continue;
        }
        let (demod, deframer) = decoder.get_or_insert_with(|| (Demod::new(), Deframer::new()));
        // The heavy part — correlators, timing PLL, HDLC de-stuff, FCS — runs off-lock.
        let frames = deframer.push(&demod.feed(&audio));
        if frames.is_empty() {
            continue;
        }
        let at = now_unix();
        let heard: Vec<AprsHeard> = frames
            .iter()
            .filter_map(|f| AprsPacket::from_bytes(f))
            .map(|pkt| AprsHeard::from_packet(&pkt, at))
            .collect();
        if !heard.is_empty() {
            if let Ok(mut e) = engine.lock() {
                for h in heard {
                    // Auto-ack a message addressed to us that carries a line number. The engine's
                    // gate decides whether it actually keys (our call / TX enabled / privileges).
                    if h.kind == "message" {
                        if let (Some(id), Some(to)) = (&h.msg_id, &h.addressee) {
                            e.aprs_auto_ack(&h.source, to, id);
                        }
                    }
                    e.push_aprs_heard(h);
                }
            }
        }
    }
}
