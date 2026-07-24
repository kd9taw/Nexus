//! FlexRadio native DAX RX audio orchestrator (Phase 2) — behind the `device` feature.
//!
//! Native DAX RX audio: the rig's receive audio arrives over the SAME VITA-49 UDP path as the
//! panadapter, so FT8/APRS/RTTY decode straight off the network instead of through the WDM-KS "DAX
//! Audio RX" soundcard device — which is invisible under Remote Desktop (the documented
//! DAX-under-RDP problem). Mirrors [`crate::flexspectrum`]:
//! - a **TCP control** thread ([`FlexCat`]) registers Nexus as a client, creates ONE `dax_rx`
//!   stream on channel 1 + binds the active slice's audio to it, learns the stream's VITA id from
//!   the async status, keeps the session alive, and removes the stream on teardown; and
//! - a **UDP audio** thread receives VITA-49 datagrams, filters to that stream id (mandatory — the
//!   `0x03E3` class is shared with plain remote audio), decodes ([`parse_dax_audio`]) to mono
//!   24 kHz, resamples to the 12 kHz modem rate, and appends to a ring the engine drains as its RX
//!   audio source.
//!
//! RX ONLY — nothing here touches TX (`backend.play` is unchanged); DAX TX is a separate,
//! approval-gated follow-up. Opt-in via `flex_native_audio`; the command syntax is verified on a
//! Flex. Only channel 1 is ever created (the "DAX starvation" gotcha: unused streams make the radio
//! round-robin audio across all of them, starving the active one).

use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::engine::Engine;
use tempo_net::flexcat::{parse_dax_stream_status, FlexCat, FlexMsg};
use tempo_net::flexvita::{
    parse_dax_audio, parse_vita, DAX_AUDIO_CLASS, DAX_AUDIO_REDUCED_CLASS, DAX_SAMPLE_RATE,
};

use crate::capture_resample::CaptureResampler;

/// The one DAX channel we use (never open all four — see the starvation note above).
const DAX_CHANNEL: u8 = 1;
/// The slice whose audio we bind to the DAX channel. Slice A (`0`) is the RX slice on a typical
/// single-slice digital setup.
const DAX_SLICE: u32 = 0;
/// The 12 kHz modem rate the decoders consume (mirrors `capture_resample::MODEM_RATE`).
const MODEM_RATE: u32 = 12_000;
/// Cap the audio ring so a stalled engine drops oldest audio instead of growing without bound
/// (~10 s at 12 kHz).
const RING_CAP: usize = 120_000;
/// Keep the SmartSDR client session alive with periodic traffic.
const KEEPALIVE: Duration = Duration::from_secs(5);

// ---- pure command helpers (unit-tested; exact SmartSDR syntax verified on a Flex) ----

/// Register Nexus as a client and route the DAX UDP stream to our port.
pub fn register_dax_commands(udp_port: u16) -> Vec<String> {
    vec![
        "client program Nexus".to_string(),
        format!("client udpport {udp_port}"),
        "sub slice all".to_string(),
    ]
}

/// Create a `dax_rx` audio stream on a channel (registers us as a DAX client so the radio streams;
/// without a client the radio sends silence).
pub fn dax_rx_create_command(channel: u8) -> String {
    format!("stream create type=dax_rx dax_channel={channel}")
}

/// Bind a slice's receive audio to a DAX channel.
pub fn slice_dax_command(slice: u32, channel: u8) -> String {
    format!("slice set {slice} dax={channel}")
}

/// Remove the DAX stream on teardown.
pub fn dax_remove_command(stream_id: u32) -> String {
    format!("stream remove 0x{stream_id:08X}")
}

// ---- orchestrator ----

/// A running Flex DAX RX audio feed. Keep it while native Flex audio is the RX source; dropping it
/// stops both threads and removes the DAX stream.
pub struct FlexDax {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    /// 12 kHz mono RX audio accumulated since the last [`FlexDax::take_audio`].
    ring: Arc<Mutex<Vec<f32>>>,
}

impl FlexDax {
    /// Connect to the Flex at `ip`, create a DAX RX stream, and stream its audio into an internal
    /// ring. Returns once the UDP socket is bound; the threads run until the value is dropped.
    pub fn start(engine: Arc<Mutex<Engine>>, ip: String) -> std::io::Result<FlexDax> {
        let _ = &engine; // reserved for future per-slice selection; kept for a matching signature
        let udp = UdpSocket::bind("0.0.0.0:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(400)))?;
        let udp_port = udp.local_addr()?.port();

        let stop = Arc::new(AtomicBool::new(false));
        let stream_id = Arc::new(Mutex::new(None::<u32>));
        let ring = Arc::new(Mutex::new(Vec::<f32>::new()));
        let mut handles = Vec::new();

        // --- TCP control thread ---
        {
            let stop = stop.clone();
            let stream_id = stream_id.clone();
            handles.push(std::thread::spawn(move || {
                let Ok(mut flex) = FlexCat::connect(&ip) else {
                    return;
                };
                for cmd in register_dax_commands(udp_port) {
                    let _ = flex.send(&cmd);
                }
                let _ = flex.send(&dax_rx_create_command(DAX_CHANNEL));
                let _ = flex.send(&slice_dax_command(DAX_SLICE, DAX_CHANNEL));
                let mut created: Option<u32> = None;
                let mut last_ka = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    // Learn OUR dax_rx stream id from the async status (the create reply / status
                    // echo carries it). We created exactly one stream on DAX_CHANNEL, so a status
                    // for that channel is ours.
                    if let Some(FlexMsg::Status { body, .. }) = flex.recv(Duration::from_millis(300))
                    {
                        if let Some(st) = parse_dax_stream_status(&body) {
                            if st.dax_channel == Some(DAX_CHANNEL) {
                                if let Some(sid) = st.stream_id {
                                    *stream_id.lock().unwrap() = Some(sid);
                                    created = Some(sid);
                                }
                            }
                        }
                    }
                    if last_ka.elapsed() >= KEEPALIVE {
                        let _ = flex.send("ping");
                        last_ka = Instant::now();
                    }
                }
                if let Some(sid) = created {
                    let _ = flex.send(&dax_remove_command(sid));
                }
            }));
        }

        // --- UDP audio thread ---
        {
            let stop = stop.clone();
            let stream_id = stream_id.clone();
            let ring = ring.clone();
            handles.push(std::thread::spawn(move || {
                let mut resampler = CaptureResampler::new(DAX_SAMPLE_RATE, MODEM_RATE); // 24k → 12k
                let mut dg = vec![0u8; 16 * 1024];
                while !stop.load(Ordering::Relaxed) {
                    let Ok((n, _)) = udp.recv_from(&mut dg) else {
                        continue; // timeout → re-check stop
                    };
                    let Some(pkt) = parse_vita(&dg[..n]) else {
                        continue;
                    };
                    let class = match pkt.packet_class {
                        Some(c) if c == DAX_AUDIO_CLASS || c == DAX_AUDIO_REDUCED_CLASS => c,
                        _ => continue,
                    };
                    // Mandatory stream-id filter — the 0x03E3 class is also plain remote audio.
                    // Accept nothing until our stream id is known (never mis-inject foreign audio).
                    match (*stream_id.lock().unwrap(), pkt.stream_id) {
                        (Some(want), Some(got)) if want == got => {}
                        _ => continue,
                    }
                    let Some(mono24) = parse_dax_audio(class, pkt.payload, pkt.has_trailer) else {
                        continue;
                    };
                    let mono12 = resampler.process(&mono24);
                    if mono12.is_empty() {
                        continue;
                    }
                    let mut r = ring.lock().unwrap();
                    r.extend_from_slice(&mono12);
                    if r.len() > RING_CAP {
                        let drop = r.len() - RING_CAP;
                        r.drain(0..drop);
                    }
                }
            }));
        }

        Ok(FlexDax { stop, handles, ring })
    }

    /// Drain the 12 kHz mono RX audio accumulated since the last call (the engine's RX-source read,
    /// in place of `backend.capture()` while native DAX is active). Empty until the stream locks.
    pub fn take_audio(&self) -> Vec<f32> {
        self.ring
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }
}

impl Drop for FlexDax {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dax_command_strings() {
        assert_eq!(dax_rx_create_command(1), "stream create type=dax_rx dax_channel=1");
        assert_eq!(slice_dax_command(0, 1), "slice set 0 dax=1");
        assert_eq!(dax_remove_command(0x0400_0000), "stream remove 0x04000000");
    }

    #[test]
    fn register_routes_udp_to_us() {
        let cmds = register_dax_commands(52002);
        assert_eq!(cmds[0], "client program Nexus");
        assert_eq!(cmds[1], "client udpport 52002");
        assert_eq!(cmds[2], "sub slice all");
    }
}
