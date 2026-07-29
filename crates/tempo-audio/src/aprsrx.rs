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

/// What one drain of audio produced. `frames_seen` counts HDLC frames the deframer recovered
/// BEFORE the FCS check, so `frames_seen > packets.len()` means the demodulator is finding packets
/// it cannot verify — a mistuned or over-driven channel, which is a completely different fault
/// from hearing nothing at all.
pub(crate) struct DecodeStep {
    pub packets: Vec<AprsPacket>,
    pub frames_seen: usize,
    /// Peak |sample| of the audio consumed — 0.0 means the tap is being fed silence.
    pub audio_peak: f32,
}

/// One decode step: audio in, packets + health out.
///
/// Deliberately free of the engine and its lock — this is the entire RX chain below the UI
/// (AFSK demod → HDLC deframe → AX.25 FCS → APRS parse), so a test can drive it from a buffer of
/// audio and check what comes out the far end. `run` below is then just the plumbing around it.
pub(crate) fn decode_step(demod: &mut Demod, deframer: &mut Deframer, audio: &[f32]) -> DecodeStep {
    let audio_peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let frames = deframer.push(&demod.feed(audio));
    DecodeStep {
        frames_seen: frames.len(),
        packets: frames
            .iter()
            .filter_map(|f| AprsPacket::from_bytes(f))
            .collect(),
        audio_peak,
    }
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
            // NOT a `continue` before reporting: an armed decoder that is being handed nothing is
            // exactly the "the app is deaf" case the operator needs told about, and it is
            // invisible if we only ever report drains that carried audio.
            if let Ok(mut e) = engine.lock() {
                e.note_aprs_rx(0.0, 0, 0, now_unix());
            }
            continue;
        }
        let (demod, deframer) = decoder.get_or_insert_with(|| (Demod::new(), Deframer::new()));
        // The heavy part — correlators, timing PLL, HDLC de-stuff, FCS — runs off-lock.
        let step = decode_step(demod, deframer, &audio);
        let at = now_unix();
        let heard: Vec<AprsHeard> = step
            .packets
            .iter()
            .map(|pkt| AprsHeard::from_packet(pkt, at))
            .collect();
        if let Ok(mut e) = engine.lock() {
            e.note_aprs_rx(step.audio_peak, step.frames_seen, step.packets.len(), at);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_core::aprs::{encode_frame, modulate, nrzi_encode, Address, Frame};

    /// The gap this module had: every APRS test in the tree stopped at the byte level (parser,
    /// FCS, Mic-E vectors) or looped the modem against its own modulator. NOTHING ran the chain
    /// the live thread actually runs — audio in, a mappable station out — so a break anywhere in
    /// the seams between those layers would have shipped green.
    fn audio_for(info: &[u8], dest: &str) -> Vec<f32> {
        let f = Frame::ui(
            Address::new(dest, 0),
            Address::new("KD9TAW", 9),
            vec![Address::new("WIDE1", 1), Address::new("WIDE2", 1)],
            info,
        );
        modulate(&nrzi_encode(&encode_frame(&f.encode(), 32, 3)))
    }

    /// Feed in ~100 ms drains, exactly like the live loop.
    fn run_chain(audio: &[f32]) -> (Vec<AprsHeard>, usize, f32) {
        let mut demod = Demod::new();
        let mut deframer = Deframer::new();
        let (mut heard, mut seen, mut peak) = (Vec::new(), 0usize, 0.0f32);
        for chunk in audio.chunks(1200) {
            let step = decode_step(&mut demod, &mut deframer, chunk);
            seen += step.frames_seen;
            peak = peak.max(step.audio_peak);
            heard.extend(
                step.packets
                    .iter()
                    .map(|p| AprsHeard::from_packet(p, 1_700_000_000)),
            );
        }
        (heard, seen, peak)
    }

    #[test]
    fn audio_becomes_a_station_with_a_position_on_the_map() {
        let (heard, seen, peak) = run_chain(&audio_for(b"!4903.50N/07201.75W-Nexus", "APZNEX"));
        assert_eq!(seen, 1, "one HDLC frame recovered");
        assert_eq!(heard.len(), 1, "one station heard");
        let h = &heard[0];
        assert_eq!(h.source, "KD9TAW-9");
        assert_eq!(h.kind, "position");
        // The map plots `lat`/`lon`; a station that decodes but carries no position is invisible
        // there, so this is the assertion that matters for the reported bug.
        assert!((h.lat.expect("mappable latitude") - 49.058_333).abs() < 1e-4);
        assert!((h.lon.expect("mappable longitude") - (-72.029_166)).abs() < 1e-4);
        assert!(peak > 0.5, "the demodulator saw real audio, peak {peak}");
    }

    #[test]
    fn a_mic_e_tracker_also_lands_on_the_map() {
        // Mic-E is what most 2 m trackers actually send — if only uncompressed positions parsed,
        // a busy channel would list stations that never plot. The hand-worked vector from mice.rs.
        let (heard, _, _) = run_chain(&audio_for(
            &[0x60, b'(', b'#', b'H', 0x1e, 0x1e, b'O', b'>', b'/'],
            "SSRUVT",
        ));
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].kind, "mice");
        assert!((heard[0].lat.expect("mappable") - 33.427_333).abs() < 1e-4);
        assert_eq!(heard[0].speed_knots, Some(20));
    }

    #[test]
    fn silence_reports_no_audio_rather_than_looking_like_a_quiet_band() {
        // The "app is deaf" reading: armed, fed silence, so the operator can tell the difference
        // between nothing being received and nothing being heard.
        let step = decode_step(&mut Demod::new(), &mut Deframer::new(), &[0.0f32; 1200]);
        assert_eq!(step.audio_peak, 0.0);
        assert_eq!(step.frames_seen, 0);
        assert!(step.packets.is_empty());
    }

    #[test]
    fn a_corrupted_frame_is_counted_as_seen_but_not_decoded() {
        // The reading that separates "mistuned / over-driven" from "quiet band": the deframer
        // recovers a frame, the FCS rejects it. Before this counter both looked like an empty map.
        let f = Frame::ui(
            Address::new("APZNEX", 0),
            Address::new("KD9TAW", 9),
            vec![],
            b"!4903.50N/07201.75W-Nexus",
        );
        let mut bytes = f.encode();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x20; // flip a bit in the info field — the FCS must reject it
        let audio = modulate(&nrzi_encode(&encode_frame(&bytes, 32, 3)));
        let (heard, seen, peak) = run_chain(&audio);
        assert_eq!(seen, 1, "the deframer still recovers the frame");
        assert!(
            heard.is_empty(),
            "but the FCS rejects it, so nothing is shown"
        );
        assert!(peak > 0.5, "and audio was plainly present, peak {peak}");
    }

    #[test]
    fn a_message_packet_carries_no_position_and_must_not_fake_one() {
        let (heard, _, _) = run_chain(&audio_for(b":KD9TAW   :hello{001", "APZNEX"));
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].kind, "message");
        assert!(heard[0].lat.is_none(), "a message has no position to plot");
        assert_eq!(heard[0].msg_id.as_deref(), Some("001"));
    }
}
