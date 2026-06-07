//! The per-slot transmit/receive decision — the heart of the radio loop, split
//! out of `service.rs::run_radio` so it is unit-testable with a `MockBackend`
//! (and a VOX/mock rig) and needs no sound card. This is a behavior-preserving
//! extraction of the slot core; the device/network/tune machinery stays in
//! `run_radio`.

use tempo_app::engine::Engine;
use tempo_core::ft1;

use crate::backend::AudioBackend;
use crate::frames::RxRing;
use crate::rig::Rig;

/// PTT-hold tail after the transmitted audio plays out (ms) — covers ring
/// drain + relay release so the start of RX isn't clipped by our own carrier.
const TX_TAIL_MS: f64 = 250.0;

/// What a slot did, for the caller to thread back into loop state + reporting.
pub struct SlotAction {
    /// Set when we transmitted: hold PTT until this Unix-ms deadline.
    pub tx_until_ms: Option<f64>,
    /// True when we decoded a receive frame into the engine this slot.
    pub did_rx: bool,
}

/// Run one slot boundary. If the engine has audio queued for this `slot`, key
/// PTT, play it, and return the PTT-hold deadline (audio duration + tail).
/// Otherwise — and only when not mid-transmit — decode the captured RX frame
/// into the engine. Mirrors the original loop body exactly (so behavior is
/// preserved); `currently_tx` is the caller's `tx_until_ms.is_some()`.
pub fn run_slot(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    rx: &mut RxRing,
    slot: u64,
    now_ms: f64,
    currently_tx: bool,
) -> SlotAction {
    let waves = eng.poll_tx(slot);
    if !waves.is_empty() {
        let _ = rig.ptt(true);
        let mut secs = 0.0f32;
        for w in &waves {
            secs += w.len() as f32 / ft1::SAMPLE_RATE;
            backend.play(w);
        }
        rx.clear(); // don't decode our own transmission
        SlotAction {
            tx_until_ms: Some(now_ms + secs as f64 * 1000.0 + TX_TAIL_MS),
            did_rx: false,
        }
    } else if !currently_tx {
        // Receive slot (and not mid-transmit): decode the captured frame.
        eng.ingest(&rx.frame(), slot);
        SlotAction {
            tx_until_ms: None,
            did_rx: true,
        }
    } else {
        // Mid-transmit with nothing new to send: do nothing this iteration.
        SlotAction {
            tx_until_ms: None,
            did_rx: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;

    #[test]
    fn tx_slot_keys_ptt_plays_audio_and_sets_hold() {
        // Engine with tx_parity 0 transmits on EVEN slots; queue a broadcast.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.broadcast("CQ TEST W9XYZ EN37");
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let act = run_slot(&mut eng, &mut rig, &mut backend, &mut rx, 0, 1000.0, false);

        assert!(rig.keyed, "PTT keyed for the TX over");
        assert!(
            !backend.played.is_empty(),
            "transmit audio played to the backend"
        );
        assert!(
            act.tx_until_ms.unwrap() > 1000.0 + 250.0,
            "PTT held for audio duration + tail"
        );
        assert!(!act.did_rx);
    }

    #[test]
    fn rx_slot_decodes_without_keying() {
        // Idle engine → nothing to send even on its TX slot → receive path.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::Ft1); // FT1-modem slot test (default is FT8)
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let act = run_slot(&mut eng, &mut rig, &mut backend, &mut rx, 0, 1000.0, false);

        assert!(!rig.keyed, "no PTT on a receive slot");
        assert!(backend.played.is_empty(), "no audio played on RX");
        assert!(act.did_rx, "decoded the RX frame");
        assert!(act.tx_until_ms.is_none());
    }

    #[test]
    fn mid_transmit_does_not_double_decode() {
        // While the PTT tail is still held (currently_tx), an idle slot is a no-op:
        // we must NOT decode (we'd be decoding our own tail) and not re-key.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let act = run_slot(&mut eng, &mut rig, &mut backend, &mut rx, 0, 1000.0, true);

        assert!(!act.did_rx, "no RX decode mid-transmit");
        assert!(act.tx_until_ms.is_none());
        assert!(!rig.keyed);
    }
}
