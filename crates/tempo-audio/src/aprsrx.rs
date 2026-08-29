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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempo_app::engine::{engine_lock, AprsHeard, AprsSource, Engine};
use tempo_core::aprs::{AprsPacket, Deframer, Demod};

use crate::service::SHUTDOWN;

/// Drain cadence: short enough that packets surface promptly, long enough that the disarmed idle
/// cost is negligible (one lock + bool read).
const POLL: Duration = Duration::from_millis(100);

/// A sample this close to full scale is clipped for reporting purposes (-0.09 dBFS).
const CLIP_PEAK: f32 = 0.99;

/// How often an armed decoder writes its health line to the diagnostic log. Long enough that a
/// session's log stays readable, short enough that a fault the operator noticed is bracketed.
const LOG_EVERY: Duration = Duration::from_secs(30);

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

/// What one drain of audio produced. `frames_seen` counts HDLC segments that look like an AX.25
/// UI frame ([`looks_like_ax25`]) but have NOT yet had their FCS checked, so
/// `frames_seen > packets.len()` means the demodulator is finding packets it cannot verify — a
/// mistuned or over-driven channel, which is a completely different fault from hearing nothing at
/// all.
pub(crate) struct DecodeStep {
    /// Each decoded packet paired with the TNC2 monitor line it came from. The line is kept
    /// because the RX iGate contributes packets VERBATIM — re-encoding a parsed packet would
    /// normalise away exactly the evidence (the digipeater path, an information field this parser
    /// does not decode) that APRS-IS wants. Bytes, not `String`: a Mic-E info field is not UTF-8.
    pub packets: Vec<(AprsPacket, Vec<u8>)>,
    pub frames_seen: usize,
    /// Peak |sample| of the audio consumed — 0.0 means the tap is being fed silence.
    pub audio_peak: f32,
    /// Samples at/near full scale. A burst hitting the rails in the rig→app path shows up here;
    /// Bell-202 survives hard limiting (measured: decodes 30 dB into clipping), but it costs the
    /// operator headroom and is worth telling them about.
    pub clipped_samples: usize,
}

/// Shortest possible AX.25 UI frame: 14 bytes of address + control + PID + the 2-byte FCS.
const MIN_UI_FRAME: usize = 18;

/// Could this HDLC segment be an AX.25 UI frame at all?
///
/// The deframer does not check the FCS — it hands back every byte-aligned flag-to-flag segment it
/// finds, and on an open squelch plain receiver hiss produces one roughly every four seconds
/// (measured: 149 of them in 600 s of Gaussian noise with nothing transmitted). Counting those as
/// `frames_seen` is what made the cockpit tell the operator "{n} failed CRC" on a channel that was
/// simply quiet — and the health card's own advice, open the squelch, manufactured the evidence.
///
/// So do what a TNC does and require the segment to be *shaped* like a frame before it counts:
/// long enough, and an address field whose callsign octets are the shifted 7-bit ASCII AX.25
/// specifies. This is a plausibility gate, NOT a second FCS check — a frame that gets in here and
/// then fails its CRC is still counted, which is the reading ("mistuned or over-driven") the
/// counter exists for.
fn looks_like_ax25(bytes: &[u8]) -> bool {
    if bytes.len() < MIN_UI_FRAME {
        return false;
    }
    // Destination at 0, source at 7 — six callsign octets each, every one an uppercase letter,
    // digit or space shifted left one bit. Deliberately NOT the SSID octets at 6 and 13: those
    // carry the C/H bit and the extension bit, are not ASCII, and demanding it there would reject
    // every real frame whose destination has the command bit set.
    [0usize, 7].iter().all(|&base| {
        bytes[base..base + 6].iter().all(|&b| {
            let c = b >> 1;
            c.is_ascii_uppercase() || c.is_ascii_digit() || c == b' '
        })
    })
}

/// One decode step: audio in, packets + health out.
///
/// Deliberately free of the engine and its lock — this is the entire RX chain below the UI
/// (AFSK demod → HDLC deframe → AX.25 FCS → APRS parse), so a test can drive it from a buffer of
/// audio and check what comes out the far end. `run` below is then just the plumbing around it.
pub(crate) fn decode_step(demod: &mut Demod, deframer: &mut Deframer, audio: &[f32]) -> DecodeStep {
    let audio_peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let clipped_samples = audio.iter().filter(|s| s.abs() >= CLIP_PEAK).count();
    let frames: Vec<Vec<u8>> = deframer
        .push(&demod.feed(audio))
        .into_iter()
        .filter(|f| looks_like_ax25(f))
        .collect();
    DecodeStep {
        frames_seen: frames.len(),
        clipped_samples,
        packets: frames
            .iter()
            .filter_map(|f| {
                let frame = tempo_core::aprs::Frame::decode(f)?;
                Some((AprsPacket::from_frame(&frame), frame.to_tnc2()))
            })
            .collect(),
        audio_peak,
    }
}

/// What the armed decoder has heard since the last diagnostic-log line.
#[derive(Default)]
struct LogWindow {
    peak: f32,
    seen: usize,
    decoded: usize,
}

fn run(engine: Arc<Mutex<Engine>>) {
    // Streaming decoder state is thread-private (like RTTY's demod): dropped + rebuilt on disarm so
    // every re-arm is a clean acquire (fresh timing PLL, fresh frame sync).
    let mut decoder: Option<(Demod, Deframer)> = None;
    // Diagnostic log (see `tempo_core::applog`). APRS had no voice in the log at all, so a report
    // of an APRS fault arrived with a log that talked only about FT8 — the operator could not tell
    // an unarmed decoder from a deaf one from a quiet band. Arm/disarm plus a periodic armed line
    // is the whole of it; the numbers are the same three the cockpit's health card reads.
    let mut was_armed = false;
    let mut window = LogWindow::default();
    let mut last_log = Instant::now();
    loop {
        if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(POLL);
        let armed = engine_lock(&engine).aprs_armed();
        if armed != was_armed {
            was_armed = armed;
            tempo_core::applog::info(
                "aprs",
                if armed {
                    "armed — RX decoder running on the 12 kHz tap"
                } else {
                    "disarmed — RX decoder stopped"
                },
            );
            window = LogWindow::default();
            last_log = Instant::now();
        }
        if !armed {
            decoder = None;
            continue;
        }
        if last_log.elapsed() >= LOG_EVERY {
            tempo_core::applog::info(
                "aprs",
                &format!(
                    "armed: peak {:.4}, {} frames seen, {} decoded in the last {} s",
                    window.peak,
                    window.seen,
                    window.decoded,
                    LOG_EVERY.as_secs()
                ),
            );
            window = LogWindow::default();
            last_log = Instant::now();
        }
        let audio = engine_lock(&engine).take_aprs_audio();
        if audio.is_empty() {
            // NOT a `continue` before reporting: an armed decoder that is being handed nothing is
            // exactly the "the app is deaf" case the operator needs told about, and it is
            // invisible if we only ever report drains that carried audio. The engine records the
            // empty drain WITHOUT clobbering the last real level — this poll runs faster than the
            // radio loop feeds it, so empty drains are routine, not evidence of a fault.
            engine_lock(&engine).note_aprs_rx(0, 0.0, 0, 0, 0, now_unix());
            continue;
        }
        let (demod, deframer) = decoder.get_or_insert_with(|| (Demod::new(), Deframer::new()));
        // The heavy part — correlators, timing PLL, HDLC de-stuff, FCS — runs off-lock.
        let step = decode_step(demod, deframer, &audio);
        window.peak = window.peak.max(step.audio_peak);
        window.seen += step.frames_seen;
        window.decoded += step.packets.len();
        let at = now_unix();
        let heard: Vec<AprsHeard> = step
            .packets
            .iter()
            .map(|(pkt, tnc2)| {
                AprsHeard::from_packet(
                    pkt,
                    at,
                    AprsSource::Rf,
                    String::from_utf8_lossy(tnc2).into_owned(),
                )
            })
            .collect();
        {
            let mut e = engine_lock(&engine);
            e.note_aprs_rx(
                audio.len(),
                step.audio_peak,
                step.clipped_samples,
                step.frames_seen,
                step.packets.len(),
                at,
            );
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
            heard.extend(step.packets.iter().map(|(p, tnc2)| {
                AprsHeard::from_packet(
                    p,
                    1_700_000_000,
                    AprsSource::Rf,
                    String::from_utf8_lossy(tnc2).into_owned(),
                )
            }));
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
        // The off-air chain also produces the exact TNC2 line the RX iGate would contribute —
        // path and information field verbatim, no re-encoding.
        assert_eq!(h.source_kind, AprsSource::Rf);
        assert_eq!(
            h.raw,
            "KD9TAW-9>APZNEX,WIDE1-1,WIDE2-1:!4903.50N/07201.75W-Nexus"
        );
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

    /// Deterministic Gaussian hiss — an open squelch with NOTHING on the channel. A fixed LCG +
    /// Box–Muller so a failure is reproducible rather than a coin toss; `sigma` is the RMS level.
    fn hiss(samples: usize, sigma: f32, seed: u64) -> Vec<f32> {
        fn next_u(state: &mut u64) -> f32 {
            *state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((*state >> 11) as f64 / (1u64 << 53) as f64) as f32
        }
        let mut state = seed | 1;
        let mut out = Vec::with_capacity(samples + 1);
        while out.len() < samples {
            let (u1, u2) = (next_u(&mut state).max(1e-9), next_u(&mut state));
            let r = sigma * (-2.0 * u1.ln()).sqrt();
            let theta = std::f32::consts::TAU * u2;
            out.push(r * theta.cos());
            out.push(r * theta.sin());
        }
        out.truncate(samples);
        out
    }

    #[test]
    fn open_squelch_hiss_is_never_counted_as_a_packet_that_failed_its_crc() {
        // The reported defect (#182). The deframer emits every byte-aligned flag-to-flag segment
        // it can find and does NOT check the FCS, so plain receiver hiss manufactures "frames" at
        // roughly a quarter of a second apart. `framesSeen > 0 && framesDecoded == 0` is what
        // renders "{n} failed CRC" in the cockpit, so within seconds of opening the squelch the
        // operator is told the channel is full of undecodable packets. Measured before the fix:
        // 82/68/64/89/83 counted frames across these five levels, 0 decoded at every one.
        for sigma in [0.3f32, 0.1, 0.03, 0.01, 0.003] {
            let (heard, seen, peak) = run_chain(&hiss(12_000 * 60, sigma, 0x5EED));
            assert!(
                peak > sigma,
                "the hiss is plainly audible at sigma {sigma}, peak {peak}"
            );
            assert!(heard.is_empty(), "noise decoded a station at sigma {sigma}");
            assert_eq!(
                seen, 0,
                "60 s of hiss at sigma {sigma} was reported as {seen} failed packets"
            );
        }
    }

    #[test]
    fn a_genuine_packet_buried_in_that_same_hiss_still_counts_and_still_decodes() {
        // The other direction, and the reason the gate is a plausibility check and not a
        // squelch: a real packet on a noisy channel must still be counted AND decoded. The audio
        // comes from the real modulator, so the gate cannot drift away from the frames the modem
        // actually produces.
        let packet = audio_for(b"!4903.50N/07201.75W-Nexus", "APZNEX");
        let mut audio = hiss(12_000 * 5, 0.05, 0xC0FFEE);
        for (i, s) in packet.iter().enumerate() {
            audio[12_000 + i] += s * 0.5;
        }
        let (heard, seen, _) = run_chain(&audio);
        assert_eq!(seen, 1, "the one real frame on the channel, and only it");
        assert_eq!(heard.len(), 1, "and it decoded");
        assert_eq!(heard[0].source, "KD9TAW-9");
        assert!(heard[0].lat.is_some(), "and it is mappable");
    }
}
