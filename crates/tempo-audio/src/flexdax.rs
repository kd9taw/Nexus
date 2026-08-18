//! FlexRadio native DAX audio orchestrator (Phase 2) — behind the `device` feature.
//!
//! Native DAX audio: the rig's audio travels over the SAME VITA-49 UDP path as the panadapter, so
//! FT8/APRS/RTTY decode straight off the network instead of through the WDM-KS "DAX Audio RX"
//! soundcard device — which is invisible under Remote Desktop (the documented DAX-under-RDP
//! problem). Mirrors [`crate::flexspectrum`], with three threads:
//! - a **TCP control** thread ([`FlexCat`]) registers Nexus as a client, creates ONE `dax_rx`
//!   stream on channel 1 + binds the operating slice's audio to it, learns the stream's VITA id
//!   from its own create REPLY (never from another client's status — see [`dax_stream_is_ours`]),
//!   creates the `dax_tx` stream and routes the rig's transmit audio to it, keeps the session
//!   alive, and puts every one of those back on teardown;
//! - a **UDP audio** thread receives VITA-49 datagrams, filters to that stream id (mandatory — the
//!   `0x03E3` class is shared with plain remote audio), decodes ([`parse_dax_audio`]) to mono
//!   24 kHz, resamples to the 12 kHz modem rate, and appends to a ring the engine drains as its RX
//!   audio source; and
//! - a **TX pacer** thread that streams the modem's transmit audio to the radio in REAL TIME
//!   ([`DaxTxSender`]).
//!
//! ⚠️ BOTH DIRECTIONS — NOT "RX ONLY". This header claimed "RX ONLY — nothing here touches TX"
//! for as long as the opposite was shipped, and that stale sentence is what sent the 2026-08-17
//! Flex audit hunting a transmit bug that is in fact a deliberate decision. What actually happens
//! while `flex_native_audio` is on: RX audio arrives over DAX, AND the rig's transmit audio is
//! re-routed to our `dax_tx` stream (`transmit set dax=1`) — so the operator's microphone is
//! disconnected for as long as the feature is on, and the sound card carries no TX audio. That is
//! the OPERATOR RULING of 2026-07-26 (reaffirmed 2026-08-17), restated in full at the
//! `transmit set dax=1` call site: one toggle means native audio both ways. Teardown puts the mic
//! and the slice's own DAX routing back.
//!
//! Opt-in via `flex_native_audio`, OFF by default, and never yet exercised against a real Flex.
//! Only channel 1 is ever created (the "DAX starvation" gotcha: unused streams make the radio
//! round-robin audio across all of them, starving the active one).

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::engine::Engine;
use tempo_net::flexcat::{
    parse_create_stream_id, parse_dax_stream_status, parse_slice_status, FlexCat, FlexMsg, FlexRecv,
    SliceStatus,
};
use tempo_net::flexvita::{
    build_dax_tx_packet, parse_dax_audio, parse_vita, VitaGap, VitaSequence, DAX_AUDIO_CLASS,
    DAX_AUDIO_REDUCED_CLASS, DAX_SAMPLE_RATE,
};

use crate::capture_resample::CaptureResampler;

/// The one DAX channel we use (never open all four — see the starvation note above).
const DAX_CHANNEL: u8 = 1;
/// The 12 kHz modem rate the decoders consume (mirrors `capture_resample::MODEM_RATE`).
const MODEM_RATE: u32 = 12_000;
/// Cap the audio ring so a stalled engine drops oldest audio instead of growing without bound
/// (~10 s at 12 kHz).
const RING_CAP: usize = 120_000;
/// Keep the SmartSDR client session alive with periodic traffic.
const KEEPALIVE: Duration = Duration::from_secs(5);
/// How often the RX thread may name accumulated packet loss in the log. A line per lost datagram
/// would itself be the problem on a lossy link.
const LOSS_REPORT_EVERY: Duration = Duration::from_secs(30);
/// The radio's VITA-49 UDP port (where we SEND DAX TX packets). Standard SmartSDR VITA port.
const FLEX_VITA_PORT: u16 = 4993;
/// Audio frames per DAX TX packet (AetherSDR `TX_SAMPLES_PER_PACKET`).
const TX_SAMPLES_PER_PACKET: usize = 128;
/// Wall-clock airtime of one DAX TX packet: 128 samples at 24 kHz = 5.333 ms. The pacer's tick.
const TX_PACKET_PERIOD: Duration =
    Duration::from_nanos((TX_SAMPLES_PER_PACKET as u64 * 1_000_000_000) / DAX_SAMPLE_RATE as u64);
/// Pacer sleep while a packet's slot is still in the future (a granularity floor, not a deadline —
/// the pacer catches up from the wall clock, so a long OS sleep quantum costs jitter, not rate).
const TX_PACER_TICK: Duration = Duration::from_millis(1);
/// Pacer sleep while nothing is queued (between overs). Nothing is on the air, so precision here
/// buys nothing and the wakeups are pure cost.
const TX_IDLE_TICK: Duration = Duration::from_millis(20);
/// How far behind schedule the pacer may fall before it resyncs to now instead of catching up. A
/// descheduled thread must not be allowed to turn its backlog into a burst at the radio.
const TX_MAX_CATCHUP: Duration = Duration::from_millis(100);
/// Cap the paced-out TX queue at ~20 s of 24 kHz audio — one FT8 over is 12.6 s. A queue with no
/// ceiling would turn a wedged pacer into unbounded memory growth.
const TX_QUEUE_CAP: usize = DAX_SAMPLE_RATE as usize * 20;

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

// ---- DAX TX control (protocol built; the TX-audio routing tee is a hardware-pass follow-up) ----

/// Create a DAX **TX** audio stream (the outbound half; the encoder is
/// [`tempo_net::flexvita::build_dax_tx_packet`]).
pub fn dax_tx_create_command() -> String {
    "stream create type=dax_tx".to_string()
}

/// Route the rig's transmit audio to the DAX TX stream (`on` → modulator reads our VITA packets) or
/// back to the physical mic path (`!on` → the radio discards DAX TX packets). `dax=1` vs `dax=0`.
pub fn transmit_set_dax_command(on: bool) -> String {
    format!("transmit set dax={}", u8::from(on))
}

/// The stream id a `stream create` reply gives us — or `None`, meaning we have no stream: nothing
/// may be sent on it, the rig's transmit audio must NOT be re-routed to it, and (RX side) no UDP
/// audio may be accepted for it.
///
/// ⚠️ THE REPLY CODE IS PART OF THE ANSWER, and it was being discarded (`if let Ok((_, reply))`).
/// `FlexCat::command` returns `Ok` for ANY matching reply, error frames included — `R7|50000015|bad`
/// parses to a perfectly good `Reply` — so a REFUSED create was indistinguishable from a granted
/// one, on a LAN, at zero latency. Fail closed: `code == 0` (the SmartSDR success convention) AND a
/// parseable id, or nothing (audit #1002 / #1044).
///
/// Used by BOTH directions now: the dax_tx create has always been read this way, and the dax_rx id
/// is no longer taken from an unfiltered broadcast (audit #1013/#1024/#1046 — see
/// [`dax_stream_is_ours`]).
pub fn stream_from_create_reply(code: u32, body: &str) -> Option<u32> {
    if code != 0 {
        return None;
    }
    parse_create_stream_id(body)
}

/// Is this `stream …` status OUR dax_rx stream?
///
/// ⚠️ THE CHANNEL NUMBER IS NOT AN OWNERSHIP TEST (audit #1013/#1024/#1046). The worker adopted
/// any `stream … type=dax_rx dax_channel=1` status that arrived, unconditionally and
/// last-writer-wins, on the reasoning that "we created exactly one stream on DAX_CHANNEL, so a
/// status for that channel is ours". A Flex is multi-client: the DAX driver on the operator's PC,
/// a second Nexus window, another program's DAX session can each hold channel 1, and the id we
/// then adopt is used to FILTER incoming audio (foreign audio into our decoders) and, on teardown,
/// to `stream remove` — deleting somebody else's stream. `client_handle` is on the wire and was
/// parsed and thrown away; its own doc comment in `flexcat` says what it is for.
///
/// Ownership, in order:
/// 1. **The create reply** (`stream_from_create_reply`) — authoritative, since we sent the create.
///    Once known, a status is ours only if it is about that same id, so nothing can repoint us.
/// 2. **The owning client handle** on our channel, for the window before the reply lands or if it
///    is lost. Fail closed with neither: no id, no audio, rather than another client's stream.
pub fn dax_stream_is_ours(
    known: Option<u32>,
    our_handle: Option<u32>,
    st: &tempo_net::flexcat::DaxStreamStatus,
) -> bool {
    match known {
        Some(ours) => st.stream_id == Some(ours),
        None => {
            st.dax_channel == Some(DAX_CHANNEL)
                && our_handle.is_some()
                && st.client_handle == our_handle
        }
    }
}

/// Remembers the slice DAX routing we FOUND, so teardown can put it back.
///
/// ⚠️ WHY A TRACKER RATHER THAN A FIELD READ AT THE BIND. `slice set N dax=1` is a persistent
/// radio-side property; Nexus overwrote it and never restored it, so an operator running slice A on
/// DAX channel 2 for WSJT-X found it moved to channel 1 permanently, with nothing anywhere saying
/// Nexus did it (audit #1027 / #1054). The prior value IS on the wire — `SliceStatus.dax_channel` —
/// but SmartSDR status lines are PARTIAL updates, so the message that triggers a bind usually
/// carries no `dax=` key at all; it has to come from what was seen earlier (the full dump the radio
/// sends on `sub slice all`). And once a slice is ours the radio echoes OUR `dax=1` straight back,
/// so tracking must STOP at the claim, or the "restore" would write back the value we imposed.
#[derive(Default)]
pub struct SliceDaxTracker {
    seen: HashMap<u32, u8>,
    claimed: HashMap<u32, Option<u8>>,
}

impl SliceDaxTracker {
    /// Note a `dax=<n>` carried by a slice status. Ignored for a slice we have already claimed.
    pub fn observe(&mut self, slice: u32, dax_channel: Option<u8>) {
        if let Some(channel) = dax_channel {
            if !self.claimed.contains_key(&slice) {
                self.seen.insert(slice, channel);
            }
        }
    }

    /// Take a slice over, freezing what it was first. Idempotent — re-claiming a slice we already
    /// hold never overwrites the original snapshot.
    pub fn claim(&mut self, slice: u32) {
        let prior = self.seen.get(&slice).copied();
        self.claimed.entry(slice).or_insert(prior);
    }

    /// What to write back for `slice`, or `None` when no `dax=` value for it was ever seen — we
    /// then leave it alone rather than guess, since `dax=0` would be a second unasked-for change
    /// to the operator's radio, not a restore.
    pub fn restore(&self, slice: u32) -> Option<u8> {
        self.claimed.get(&slice).copied().flatten()
    }
}

/// A slice we can PROVE belongs to another client: both handles known and different. Unknown on
/// either side is NOT foreign — the radio may simply not have told us yet, and refusing to bind on
/// silence would make native audio a coin flip on a single-client desk.
fn slice_is_foreign(st: &SliceStatus, our_handle: Option<u32>) -> bool {
    matches!((st.client_handle, our_handle), (Some(theirs), Some(ours)) if theirs != ours)
}

/// The per-slice picture, merged across SmartSDR's PARTIAL status deltas.
///
/// ⚠️ TWO DEFECTS LIVE HERE, and the second is why this type exists rather than a pair of locals.
///
/// **#1025 — a status delta carries only the keys that changed.** The bind test was
/// `in_use == Some(true) && active == Some(true)` evaluated against ONE status line, and
/// `parse_slice_status` leaves every key the line did not carry as `None`. A front-panel or
/// SmartSDR slice change sends `slice 1 active=1` on its own, which can never satisfy that `&&`,
/// so the re-bind the code's own comment promised ("re-bind if the operator switches the active
/// slice") could not fire at all. State has to be remembered across updates for the test to mean
/// anything.
///
/// **#1014/#1026 — `active=1` is not "the slice Nexus is operating".** See
/// [`crate::rigmodels::flex_slice_for_cat_addr`]: the CAT address names the slice the dial, the
/// waterfall and the log all come from, so that is the slice the audio must follow. The active
/// flag is the fallback for a CAT link whose port says nothing (a custom port, a virtual COM
/// pair), and there it is at least ownership-filtered now.
#[derive(Default)]
pub struct SliceView {
    slices: std::collections::BTreeMap<u32, SliceStatus>,
}

impl SliceView {
    /// Fold one status delta in: only the keys this line actually carried are overwritten.
    pub fn merge(&mut self, st: &SliceStatus) {
        let e = self.slices.entry(st.num).or_insert_with(|| SliceStatus {
            num: st.num,
            ..Default::default()
        });
        if st.in_use.is_some() {
            e.in_use = st.in_use;
        }
        if st.active.is_some() {
            e.active = st.active;
        }
        if st.tx_slice.is_some() {
            e.tx_slice = st.tx_slice;
        }
        if st.dax_channel.is_some() {
            e.dax_channel = st.dax_channel;
        }
        if st.client_handle.is_some() {
            e.client_handle = st.client_handle;
        }
    }

    /// Which slice DAX audio should be bound to right now, or `None` while the radio has not said
    /// enough. `cat_slice` is the slice the CAT link drives (`flex_slice_for_cat_addr`).
    ///
    /// The CAT slice is preferred whenever the radio has mentioned it and has not said it is gone;
    /// a slice proven to belong to another client is never bound, in either path.
    pub fn bind_target(&self, our_handle: Option<u32>, cat_slice: Option<u32>) -> Option<u32> {
        if let Some(n) = cat_slice {
            let st = self.slices.get(&n)?;
            return (st.in_use != Some(false) && !slice_is_foreign(st, our_handle)).then_some(n);
        }
        self.slices
            .values()
            .find(|st| {
                st.in_use == Some(true)
                    && st.active == Some(true)
                    && !slice_is_foreign(st, our_handle)
            })
            .map(|st| st.num)
    }
}

// ---- DAX TX sender ----

/// Encodes the modem's 12 kHz mono TX audio into DAX VITA-49 packets for the radio.
///
/// Installed into `CpalBackend` as its [`TxTee`] while native Flex audio is active, so it carries
/// exactly the audio the soundcard path would and the TX schedule is untouched. `feed` (the radio
/// loop, via `play`) upsamples 12k→24k and QUEUES; the pacer thread converts to int16 and sends,
/// 128 samples per VITA packet, one packet per 5.333 ms of wall clock. Nothing is sent until the
/// dax_tx stream id is learned, so nothing reaches the air prematurely.
///
/// ⚠️ TWO INVARIANTS THIS TYPE EXISTS TO HOLD, both from the 2026-08-17 Flex audit:
/// 1. **The TX level is applied as the audio LEAVES** (`take_packet`), never as it is queued —
///    the same rule, and the same reason, as `device::next_tx_sample`: the queue holds a whole
///    13 s over, so a level baked in at `feed` time could not be changed by the Pwr slider (that
///    was issue #14 on the soundcard path). Before this, the DAX route did not apply the level at
///    ALL and put full-scale audio into the transmitter no matter where Pwr sat (#1048).
/// 2. **It is PACED to real time.** `play` hands over an entire prebuilt over in ONE call; the old
///    `feed` drained it straight to the socket, ~2,370 datagrams (~670 KB) in a few milliseconds
///    for 12.6 s of audio (#1042). The radio expects a stream, not a flood.
pub struct DaxTxSender {
    /// Our dax_tx stream id, learned from the create reply. `None` = nothing may be sent.
    stream_id: Arc<Mutex<Option<u32>>>,
    /// TX drive (f32 bits, 0.0–1.0) — the operator's Pwr slider, pushed down by the backend that
    /// owns it (`CpalBackend::set_tx_level` / `set_tx_tee`), read per packet as it leaves.
    level: AtomicU32,
    state: Mutex<TxSendState>,
}

struct TxSendState {
    resampler: CaptureResampler,
    /// 24 kHz mono audio waiting for its real-time send slot. UNSCALED — see invariant 1.
    queue: VecDeque<f32>,
}

/// The pacer's clock rule: once a packet has been sent at `now`, when is the next one due?
///
/// Exactly one packet's airtime after the LAST due time — not after `now` — so the long-run rate is
/// the radio's own sample rate no matter what the OS sleep granularity does to any single wakeup
/// (Windows' default quantum is ~15 ms against a 5.3 ms packet, so catching up is the normal case,
/// not an error case). The one exception is falling further behind than [`TX_MAX_CATCHUP`]: a
/// thread that was descheduled for a second must resync rather than discharge its whole backlog at
/// the radio, which is the burst this pacer exists to prevent. Pure, so the pacing is testable with
/// no Flex on the bench (audit #1042).
fn advance_pacer(now: Instant, next_at: Instant) -> Instant {
    let next = next_at + TX_PACKET_PERIOD;
    if now.saturating_duration_since(next) > TX_MAX_CATCHUP {
        now
    } else {
        next
    }
}

impl DaxTxSender {
    /// The next packet's worth of samples, scaled by the level as it stands RIGHT NOW, or `None`
    /// when less than a whole packet is queued.
    fn take_packet(&self) -> Option<Vec<i16>> {
        let level = f32::from_bits(self.level.load(Ordering::Relaxed));
        let mut st = self.state.lock().unwrap();
        if st.queue.len() < TX_SAMPLES_PER_PACKET {
            return None;
        }
        Some(
            st.queue
                .drain(..TX_SAMPLES_PER_PACKET)
                // Identical to the soundcard route: level first (`next_tx_sample`), then the
                // clamp + full-scale conversion the device format applies (`device.rs`).
                .map(|s| ((s * level).clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect(),
        )
    }
}

impl crate::backend::TxTee for DaxTxSender {
    /// Queue an over. Called from the RADIO LOOP with the whole waveform in one buffer, so it must
    /// never block and never send — the sending is the pacer's job.
    fn feed(&self, mono12: &[f32]) {
        if self.stream_id.lock().unwrap().is_none() {
            return; // stream not up yet → nothing on the air
        }
        let mut st = self.state.lock().unwrap();
        let up = st.resampler.process(mono12); // 12k → 24k
        st.queue.extend(up);
        if st.queue.len() > TX_QUEUE_CAP {
            let drop = st.queue.len() - TX_QUEUE_CAP;
            st.queue.drain(..drop);
        }
    }

    /// Adopt the backend's TX level (0.0–1.0).
    fn set_level(&self, level: f32) {
        self.level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Hard Stop TX for this route: discard everything queued but not yet paced out, so
    /// `flush_output` cuts a native over exactly as it cuts a soundcard one. Returns the count
    /// discarded (24 kHz samples). Without this, making the DAX route exclusive would have left
    /// Stop TX with nothing to cut but a ring the over no longer travels through.
    fn flush(&self) -> usize {
        let mut st = self.state.lock().unwrap();
        let n = st.queue.len();
        st.queue.clear();
        n
    }
}

/// Put a slice's DAX routing back to what the radio reported before we claimed it.
///
/// `prior` is `None` when no `dax=` value for that slice was ever seen — we then leave the slice
/// alone rather than guess. Writing `dax=0` there would be a second unasked-for change to the
/// operator's radio, not a restore; SmartSDR dumps every slice's full state on `sub slice all`, so
/// the value is normally in hand long before a bind fires.
fn restore_slice_dax(flex: &mut FlexCat, slice: u32, prior: Option<u8>) {
    if let Some(channel) = prior {
        let _ = flex.send(&slice_dax_command(slice, channel));
    }
}

// ---- orchestrator ----

/// A running Flex DAX audio feed (RX + TX). Keep it while native Flex audio is active; dropping it
/// stops the threads, routes TX audio back to the mic, and removes the DAX streams.
pub struct FlexDax {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    /// 12 kHz mono RX audio accumulated since the last [`FlexDax::take_audio`].
    ring: Arc<Mutex<Vec<f32>>>,
    /// The TX-audio encoder/sender, teed into `CpalBackend::play`.
    tx: Arc<DaxTxSender>,
}

impl FlexDax {
    /// Connect to the Flex at `ip`, create a DAX RX stream, and stream its audio into an internal
    /// ring. Returns once the UDP socket is bound; the threads run until the value is dropped.
    pub fn start(engine: Arc<Mutex<Engine>>, ip: String) -> std::io::Result<FlexDax> {
        // WHICH SLICE ARE WE OPERATING? The CAT address answers it (audit #1014/#1026 — the
        // `engine` argument used to be discarded right here with a "reserved for future per-slice
        // selection" note, while the audio silently followed the radio's `active` flag instead).
        // Read once, like the Flex IP: a later CAT-address edit takes effect on the next radio
        // re-select, which is when the worker restarts anyway.
        let cat_slice = engine
            .lock()
            .ok()
            .map(|e| e.settings().rig_addr.clone())
            .and_then(|addr| crate::rigmodels::flex_slice_for_cat_addr(&addr));
        let udp = UdpSocket::bind("0.0.0.0:0")?;
        udp.set_read_timeout(Some(Duration::from_millis(400)))?;
        let udp_port = udp.local_addr()?.port();

        let stop = Arc::new(AtomicBool::new(false));
        let stream_id = Arc::new(Mutex::new(None::<u32>));
        let ring = Arc::new(Mutex::new(Vec::<f32>::new()));
        // TX: a send-side clone of the VITA socket + the learned dax_tx stream id + the encoder.
        let tx_sock = udp.try_clone()?;
        let radio: SocketAddr = format!("{ip}:{FLEX_VITA_PORT}").parse().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid Flex IP")
        })?;
        let tx_stream_id = Arc::new(Mutex::new(None::<u32>));
        let tx = Arc::new(DaxTxSender {
            stream_id: tx_stream_id.clone(),
            // Unity until the backend pushes the operator's Pwr down (it does so on install, so
            // this default is never what an over actually goes out at).
            level: AtomicU32::new(1.0f32.to_bits()),
            state: Mutex::new(TxSendState {
                resampler: CaptureResampler::new(MODEM_RATE, DAX_SAMPLE_RATE), // 12k → 24k
                queue: VecDeque::new(),
            }),
        });
        let mut handles = Vec::new();

        // --- TCP control thread ---
        {
            let stop = stop.clone();
            let stream_id = stream_id.clone();
            let tx_stream_id = tx_stream_id.clone();
            handles.push(std::thread::spawn(move || {
                let Ok(mut flex) = FlexCat::connect(&ip) else {
                    return;
                };
                // Dropped while we were connecting (bounded, but not instant): the radio has heard
                // nothing from us, so leave it exactly as we found it. Without this, a worker that
                // finally connects after its `FlexDax` is gone would create streams and re-route
                // transmit audio behind the back of the worker that replaced it.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                for cmd in register_dax_commands(udp_port) {
                    let _ = flex.send(&cmd);
                }
                // Keep the create's sequence number: its reply is the authoritative source of OUR
                // dax_rx stream id (audit #1013 — it used to be learned from whatever `stream …
                // dax_channel=1` status happened to arrive, which is another client's stream as
                // easily as ours). `send` rather than `command` so the status stream still reaches
                // the loop below.
                let rx_seq = flex.send(&dax_rx_create_command(DAX_CHANNEL)).ok();
                // DAX TX: create the outbound stream (its create reply carries the stream id) and
                // route the rig's transmit audio to it. Torn down on stop (TX back to the mic).
                //
                // ⚠️ OPERATOR RULING 2026-07-26, REAFFIRMED 2026-08-17 — THIS IS DELIBERATE, DO
                // NOT "FIX" IT. `transmit_set_dax_command(true)` below has NO OPERATOR-FACING GATE
                // of its own: turning on native Flex audio routes BOTH directions over DAX, RX and
                // TX, from the one `flex_native_audio` toggle. It reads like an oversight (a
                // TX-path command with no TX-specific gate) and has now been queried twice, hence
                // this note.
                //
                // The alternative considered and REJECTED was splitting TX behind its own opt-in.
                // Rejected because one toggle is what an operator actually wants — native audio
                // means native audio — and a half-native path (DAX in, sound card out) is a
                // configuration that mostly exists to be got wrong.
                //
                // The backlog's older "dax_tx stays gated" line means gated behind THIS toggle,
                // not behind a second one. Teardown restores the mic on stop, so the rig is never
                // left expecting DAX audio that stopped arriving.
                //
                // The ONE condition it does carry is not a second opt-in and leaves the ruling
                // intact: we refuse to point the transmitter at a `dax_tx` stream that was never
                // created. "Both directions from one toggle" is untouched; "route TX to a stream
                // that does not exist" was never part of it — that was a bug that left the mic
                // disconnected and nothing on the air (audit #1002 / #1044).
                let mut tx_created: Option<u32> = None;
                if let Ok((code, reply)) =
                    flex.command(&dax_tx_create_command(), Duration::from_millis(600))
                {
                    if let Some(sid) = stream_from_create_reply(code, &reply) {
                        *tx_stream_id.lock().unwrap() = Some(sid);
                        tx_created = Some(sid);
                    }
                }
                // ⚠️ CONDITIONAL ON THE CREATE — and that is NOT a re-gating of the ruling above.
                // The ruling is about which TOGGLE owns the transmit path, and it stands: one
                // toggle, both directions, no second opt-in. This guard is about a stream that
                // does not exist. `transmit set dax=1` was sent unconditionally, so a create that
                // timed out (600 ms is one round trip — a WAN link misses it) or was refused left
                // the radio taking transmit audio from a DAX stream nothing would ever send on:
                // the mic disconnected, Nexus's audio not reaching the air either, and — because
                // teardown was gated on that same failed create — the mic still dead after Nexus
                // exited (audit #1002 / #1044). Fail CLOSED: no stream, no re-route, sound card.
                if tx_created.is_some() {
                    let _ = flex.send(&transmit_set_dax_command(true));
                }
                let mut created: Option<u32> = None;
                let mut bound_slice: Option<u32> = None;
                // What the operator's slice DAX routing was before we touched it (see
                // [`SliceDaxTracker`]) …
                let mut slices = SliceDaxTracker::default();
                // … and the merged per-slice picture the bind decision is made from (see
                // [`SliceView`] — SmartSDR sends partial deltas, so a single line is never the
                // whole state).
                let mut view = SliceView::default();
                let mut last_ka = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    match flex.recv(Duration::from_millis(300)) {
                        // OUR dax_rx create's reply: the authoritative stream id.
                        FlexRecv::Msg(FlexMsg::Reply { seq, code, msg }) if Some(seq) == rx_seq => {
                            if let Some(sid) = stream_from_create_reply(code, &msg) {
                                *stream_id.lock().unwrap() = Some(sid);
                                created = Some(sid);
                            }
                        }
                        FlexRecv::Msg(FlexMsg::Status { body, .. }) => {
                            // The async status confirms the id the reply granted — or, if that
                            // reply was lost, supplies it, but ONLY for a stream the radio says is
                            // ours (audit #1013/#1024/#1046; the rule is on `dax_stream_is_ours`).
                            if let Some(st) = parse_dax_stream_status(&body) {
                                if dax_stream_is_ours(created, flex.handle(), &st) {
                                    if let Some(sid) = st.stream_id {
                                        *stream_id.lock().unwrap() = Some(sid);
                                        created = Some(sid);
                                    }
                                }
                            }
                            // Bind the slice NEXUS IS OPERATING — the one its CAT link drives —
                            // to our DAX channel, and re-bind when that changes (audit
                            // #1014/#1025/#1026; the whole rule is on [`SliceView`]).
                            if let Some(sl) = parse_slice_status(&body) {
                                slices.observe(sl.num, sl.dax_channel);
                                view.merge(&sl);
                                let want = view.bind_target(flex.handle(), cat_slice);
                                if let Some(n) = want {
                                    if bound_slice != Some(n) {
                                        // Let the slice we were on go FIRST: two slices both
                                        // claiming channel 1 is the "DAX starvation" gotcha in
                                        // this module's header, and the abandoned claims used to
                                        // accumulate.
                                        if let Some(old) = bound_slice {
                                            restore_slice_dax(&mut flex, old, slices.restore(old));
                                        }
                                        slices.claim(n);
                                        let _ = flex.send(&slice_dax_command(n, DAX_CHANNEL));
                                        bound_slice = Some(n);
                                    }
                                }
                            }
                        }
                        FlexRecv::Msg(_) | FlexRecv::Idle => {}
                        // ⚠️ THE CONNECTION IS GONE — EXIT, do not loop (audit #1000). A
                        // disconnected channel returns instantly, and this timeout is the only
                        // thing pacing this loop, so continuing free-runs a core and fires a
                        // keepalive `ping` into a dead socket forever. Teardown below still runs:
                        // the sends fail harmlessly, and the radio drops a departed client's DAX
                        // routing on its own.
                        FlexRecv::Closed => break,
                    }
                    if last_ka.elapsed() >= KEEPALIVE {
                        let _ = flex.send("ping");
                        last_ka = Instant::now();
                    }
                }
                // Teardown, in the order the radio needs: transmit audio back to the MIC first,
                // then the slice routing we changed, then the streams we created.
                //
                // ⚠️ THE MIC RESTORE IS UNCONDITIONAL, deliberately. It used to live inside
                // `if let Some(sid) = tx_created`, so the one failure that most needed it — a
                // create whose reply missed its window, after `dax=1` had already gone out — was
                // exactly the case that skipped it, and the operator's microphone stayed dead
                // after Nexus exited with nothing on screen or in SmartSDR explaining why
                // (audit #1002). One line on the wire; `dax=0` is the state a radio with no DAX
                // client belongs in, and it costs nothing when we never enabled it.
                let _ = flex.send(&transmit_set_dax_command(false));
                if let Some(slice) = bound_slice {
                    restore_slice_dax(&mut flex, slice, slices.restore(slice));
                }
                // Remove only the streams we created: both ids now come from our own create
                // replies (or, for RX, a status the radio stamped with our client handle), so
                // `stream remove` can no longer delete another DAX client's stream (audit #1013).
                if let Some(sid) = tx_created {
                    let _ = flex.send(&dax_remove_command(sid));
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
                // Packet continuity for OUR stream (audit #1008/#1052 — see `VitaSequence`), plus
                // a rate-limited count for the log: one line per gap would itself be the problem
                // on a lossy link.
                let mut seq = VitaSequence::default();
                let mut lost: u64 = 0;
                let mut lost_ms = 0.0f64;
                let mut last_report = Instant::now();
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
                    // Read the id into a local FIRST: a guard built in a match scrutinee lives
                    // through every arm, and this thread locks `ring` further down.
                    let want_id = *stream_id.lock().unwrap();
                    match (want_id, pkt.stream_id) {
                        (Some(want), Some(got)) if want == got => {}
                        _ => continue,
                    }
                    // ⚠️ A LOST DATAGRAM MUST COST ITS OWN AIRTIME AND NOTHING ELSE. The ring is a
                    // pure sample store where position implies time, so splicing the next packet
                    // straight on after a loss slides every later sample earlier by the missing
                    // duration — the decode window's t=0 moves and every dt inside it is wrong,
                    // silently (audit #1008/#1052).
                    let gap = seq.observe(pkt.packet_count);
                    if gap == VitaGap::Stale {
                        continue; // duplicate / straggler: never write it mid-stream
                    }
                    let Some(mono24) = parse_dax_audio(class, pkt.payload, pkt.has_trailer) else {
                        continue;
                    };
                    // Fill the hole with its own duration, through the SAME resampler so the
                    // 24k→12k phase stays continuous across it.
                    let mut mono12 = if let VitaGap::Lost(n) = gap {
                        lost += u64::from(n);
                        lost_ms +=
                            n as f64 * mono24.len() as f64 * 1000.0 / DAX_SAMPLE_RATE as f64;
                        resampler.process(&vec![0.0f32; n as usize * mono24.len()])
                    } else {
                        Vec::new()
                    };
                    mono12.extend_from_slice(&resampler.process(&mono24));
                    // Say so — silence that is honest about its length is still silence, and an
                    // operator chasing bad decodes deserves to see the loss in the log.
                    if lost > 0 && last_report.elapsed() >= LOSS_REPORT_EVERY {
                        eprintln!(
                            "tempo-audio: Flex DAX RX has lost {lost} packet(s) this session \
                             (~{lost_ms:.0} ms of audio, filled with silence)"
                        );
                        last_report = Instant::now();
                    }
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

        // --- DAX TX pacer thread ---
        //
        // ⚠️ THE RADIO EXPECTS A STREAM, NOT A FLOOD. `play` hands the tee an entire prebuilt over
        // in ONE call (slot.rs), and the encoder used to drain it straight to the socket: ~2,370
        // datagrams, ~670 KB, of 12.6 s of audio, inside a few milliseconds — on the radio-loop
        // thread, with the engine mutex held, right after PTT-up (audit #1042). This thread is the
        // clock the DAX route never had. It owns the socket and the packet counter; the tee only
        // queues.
        {
            let stop = stop.clone();
            let tx = tx.clone();
            handles.push(std::thread::spawn(move || {
                let mut counter: u8 = 0;
                let mut next_at = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    let now = Instant::now();
                    if now < next_at {
                        std::thread::sleep(TX_PACER_TICK.min(next_at - now));
                        continue;
                    }
                    // No stream id yet → nothing may go on the air, and nothing is queued either
                    // (`feed` refuses too). Checked BEFORE taking a packet, so a packet is never
                    // drained and dropped on the floor.
                    let Some(sid) = *tx.stream_id.lock().unwrap() else {
                        next_at = now;
                        std::thread::sleep(TX_IDLE_TICK);
                        continue;
                    };
                    match tx.take_packet() {
                        Some(chunk) => {
                            let pkt = build_dax_tx_packet(sid, counter, &chunk);
                            counter = (counter + 1) & 0x0F;
                            let _ = tx_sock.send_to(&pkt, radio);
                            next_at = advance_pacer(now, next_at);
                        }
                        // Nothing queued (between overs). Hold the clock at now, so an idle gap
                        // cannot bank credit that would burst the START of the next over out.
                        None => {
                            next_at = now;
                            std::thread::sleep(TX_IDLE_TICK);
                        }
                    }
                }
            }));
        }

        Ok(FlexDax {
            stop,
            handles,
            ring,
            tx,
        })
    }

    /// Drain the 12 kHz mono RX audio accumulated since the last call (the engine's RX-source read,
    /// in place of `backend.capture()` while native DAX is active). Empty until the stream locks.
    pub fn take_audio(&self) -> Vec<f32> {
        self.ring
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }

    /// The TX-audio route for this feed. Install it into `CpalBackend` via
    /// [`crate::backend::AudioBackend::set_tx_tee`] while native Flex audio is active; every
    /// `backend.play()` then reaches the radio over DAX **instead of** the sound card (the TX
    /// schedule is unchanged — same audio, one route).
    pub fn tx_tee(&self) -> crate::backend::TxTeeHandle {
        self.tx.clone()
    }
}

impl Drop for FlexDax {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Bounded, never open-ended: this runs ON THE RADIO LOOP (see `flexspectrum::reap_workers`
        // for why a plain join there is a network peer holding the transmitter).
        crate::flexspectrum::reap_workers(self.handles.drain(..).collect(), || {});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dax_command_strings() {
        assert_eq!(
            dax_rx_create_command(1),
            "stream create type=dax_rx dax_channel=1"
        );
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

    #[test]
    fn dax_tx_command_strings() {
        assert_eq!(dax_tx_create_command(), "stream create type=dax_tx");
        assert_eq!(transmit_set_dax_command(true), "transmit set dax=1");
        assert_eq!(transmit_set_dax_command(false), "transmit set dax=0");
    }

    use crate::backend::TxTee as _;

    /// A sender with a known stream id, ready to be fed.
    fn sender(level: f32) -> DaxTxSender {
        let tx = DaxTxSender {
            stream_id: Arc::new(Mutex::new(Some(0x8400_0000))),
            level: AtomicU32::new(1.0f32.to_bits()),
            state: Mutex::new(TxSendState {
                resampler: CaptureResampler::new(MODEM_RATE, DAX_SAMPLE_RATE),
                queue: VecDeque::new(),
            }),
        };
        tx.set_level(level);
        tx
    }

    /// The sound-card rule, from `device::next_tx_sample` + the device-format conversion:
    /// multiply by the level as the sample LEAVES, then clamp and scale to full range.
    fn card_rule(sample: f32, level: f32) -> i16 {
        ((sample * level).clamp(-1.0, 1.0) * 32767.0) as i16
    }

    /// Put samples straight into the paced queue at the 24 kHz side, so an assertion is about the
    /// LEVEL rule alone and not about the 12k→24k resampler (which has its own tests, and whose
    /// windowed-sinc ripple would otherwise blur an exact comparison).
    fn queue_24k(tx: &DaxTxSender, samples: &[f32]) {
        tx.state
            .lock()
            .unwrap()
            .queue
            .extend(samples.iter().copied());
    }

    /// THE DRIVE CONTROL MUST MEAN THE SAME THING ON BOTH ROUTES (audit #1048).
    ///
    /// The DAX route ignored `tx_level` entirely — it converted at a hard-coded full scale, so the
    /// radio got 0 dBFS audio wherever the operator put Pwr, an IMD/splatter path behind an
    /// on-screen control that appeared to work. Asserted on actual emitted sample amplitudes,
    /// against the sound card's own rule.
    #[test]
    fn the_tx_level_scales_dax_samples_exactly_as_the_sound_card_does() {
        // A packet's worth of a waveform with both polarities and an over-range excursion, so the
        // clamp is exercised too.
        let input: Vec<f32> = (0..TX_SAMPLES_PER_PACKET)
            .map(|i| {
                let phase = i as f32 / TX_SAMPLES_PER_PACKET as f32;
                1.4 * (phase * std::f32::consts::TAU).sin()
            })
            .collect();

        for level in [1.0f32, 0.5, 0.1, 0.0] {
            let tx = sender(level);
            queue_24k(&tx, &input);
            let packet = tx.take_packet().expect("a packet's worth is queued");
            assert_eq!(packet.len(), TX_SAMPLES_PER_PACKET);
            for (i, (&got, &s)) in packet.iter().zip(input.iter()).enumerate() {
                assert_eq!(
                    got,
                    card_rule(s, level),
                    "DAX sample {i} at Pwr {level} must carry the same amplitude the sound card \
                     would send"
                );
            }
            if level == 0.0 {
                // Zero really is zero — the same promise `device::tx_level_tests` makes for the
                // card, and the one the old DAX route broke most loudly (full scale at Pwr 0).
                assert!(
                    packet.iter().all(|&s| s == 0),
                    "Pwr at 0 must put nothing on the air over DAX either"
                );
            }
        }
    }

    /// The level is read AS THE AUDIO LEAVES, not as it is queued — issue #14's rule, which the
    /// DAX route needs even more than the card does: it queues a whole 13 s over at once.
    #[test]
    fn a_level_change_reaches_audio_that_is_already_queued() {
        let tx = sender(1.0);
        queue_24k(&tx, &vec![1.0f32; 2 * TX_SAMPLES_PER_PACKET]);
        assert_eq!(tx.take_packet().expect("queued")[0], card_rule(1.0, 1.0));
        tx.set_level(0.25);
        assert_eq!(
            tx.take_packet().expect("still queued")[0],
            card_rule(1.0, 0.25),
            "moving Pwr must change audio that was queued before the move"
        );
    }

    /// AN OVER IS A STREAM, NOT A FLOOD (audit #1042).
    ///
    /// `play` hands the tee an entire prebuilt over in ONE call, and the encoder used to drain it
    /// straight to the socket — ~2,370 datagrams of 12.6 s of audio inside a few milliseconds.
    /// Driving the shipped pacer rule with a virtual clock: one second of wall time may carry at
    /// most one second of audio (plus the packet in flight), no matter how much is queued.
    #[test]
    fn a_queued_over_leaves_at_real_time_rather_than_all_at_once() {
        let tx = sender(1.0);
        // 12.64 s of FT8 at the modem rate — one full over, exactly as `play` delivers it.
        tx.feed(&vec![0.5f32; (12.64 * MODEM_RATE as f32) as usize]);

        let t0 = Instant::now();
        let mut next_at = t0;
        let mut sent = 0usize;
        // One simulated second, stepped at the pacer's own tick.
        for ms in 1..=1000u64 {
            let now = t0 + Duration::from_millis(ms);
            while now >= next_at {
                if tx.take_packet().is_none() {
                    break;
                }
                sent += 1;
                next_at = advance_pacer(now, next_at);
            }
        }
        let per_second = 1000.0 / (TX_PACKET_PERIOD.as_micros() as f64 / 1000.0);
        assert!(
            (sent as f64) <= per_second + 2.0,
            "{sent} packets in one simulated second is a burst — real time is ~{per_second:.0}"
        );
        assert!(
            (sent as f64) >= per_second - 2.0,
            "{sent} packets in one simulated second starves the radio — real time is ~{per_second:.0}"
        );
        // …and the rest of the over is still queued, not discarded.
        assert!(tx.take_packet().is_some(), "the over continues");
    }

    /// The pacer keeps the LONG-RUN rate exact across a coarse OS sleep quantum (Windows' default
    /// is ~15 ms against a 5.3 ms packet), but resyncs rather than discharging a big backlog.
    #[test]
    fn the_pacer_catches_up_from_jitter_but_never_from_a_stall() {
        let t0 = Instant::now();
        assert_eq!(
            advance_pacer(t0, t0),
            t0 + TX_PACKET_PERIOD,
            "on schedule → the next packet is due one airtime later"
        );
        // 20 ms late: still catching up (the deadline stays in the past, so the backlog drains).
        let late = t0 + Duration::from_millis(20);
        assert_eq!(advance_pacer(late, t0), t0 + TX_PACKET_PERIOD);
        // A full second behind: resync to now instead of firing ~190 packets back to back.
        let stalled = t0 + Duration::from_secs(1);
        assert_eq!(advance_pacer(stalled, t0), stalled);
    }

    /// Stop TX must cut the route the over is actually on. With the DAX route exclusive,
    /// `flush_output` clearing the soundcard ring would otherwise cut nothing at all.
    #[test]
    fn a_hard_stop_empties_the_dax_queue() {
        let tx = sender(1.0);
        tx.feed(&vec![0.5f32; 24_000]);
        assert!(tx.flush() > 0, "there was queued audio to discard");
        assert!(
            tx.take_packet().is_none(),
            "Stop TX leaves nothing queued for the pacer to send"
        );
    }

    /// Nothing reaches the air before the radio has told us our stream id — the same guard that
    /// makes a failed `stream create` safe.
    #[test]
    fn nothing_is_queued_until_the_stream_id_is_known() {
        let tx = DaxTxSender {
            stream_id: Arc::new(Mutex::new(None)),
            level: AtomicU32::new(1.0f32.to_bits()),
            state: Mutex::new(TxSendState {
                resampler: CaptureResampler::new(MODEM_RATE, DAX_SAMPLE_RATE),
                queue: VecDeque::new(),
            }),
        };
        tx.feed(&vec![1.0f32; 4096]);
        assert!(tx.take_packet().is_none(), "no stream id → nothing to send");
    }

    /// A REFUSED create must not look like a granted one (audit #1002/#1044): the reply code is
    /// the difference, and it used to be thrown away — after which `transmit set dax=1` went out
    /// anyway and pointed the transmitter at a stream nothing would ever send on.
    #[test]
    fn a_stream_is_adopted_only_from_a_successful_create() {
        assert_eq!(
            stream_from_create_reply(0, "0x84000000"),
            Some(0x8400_0000),
            "code 0 + an id = ours"
        );
        assert_eq!(
            stream_from_create_reply(0x5000_0015, "0x84000000"),
            None,
            "an ERROR code with an id-shaped body is still a refusal"
        );
        assert_eq!(stream_from_create_reply(0x5000_0015, "bad"), None);
        assert_eq!(stream_from_create_reply(0, ""), None, "no id = no re-route");
    }

    /// DAX AUDIO IS TAKEN FROM OUR OWN STREAM, NOT FROM WHOEVER HOLDS CHANNEL 1
    /// (audit #1013/#1024/#1046).
    ///
    /// The only filter was `dax_channel == 1`, so the DAX driver's own stream — or a second Nexus
    /// window's — was adopted last-writer-wins, and that borrowed id then decided which UDP audio
    /// reached our decoders and which stream `stream remove` deleted on teardown.
    #[test]
    fn a_dax_stream_belonging_to_another_client_is_never_adopted() {
        const OURS: u32 = 0x2ABC;
        const THEIRS: u32 = 0x1234;
        let status = |sid: u32, owner: u32| {
            parse_dax_stream_status(&format!(
                "stream 0x{sid:08X} type=dax_rx dax_channel=1 client_handle=0x{owner:08X} in_use=1"
            ))
            .expect("a dax_rx stream status")
        };

        // Before our create reply lands, the owning handle is the only proof.
        assert!(
            !dax_stream_is_ours(None, Some(OURS), &status(0x0400_0009, THEIRS)),
            "another client's channel-1 stream must never be adopted"
        );
        assert!(dax_stream_is_ours(
            None,
            Some(OURS),
            &status(0x0400_0000, OURS)
        ));
        // Fail closed while our own handle is still unknown.
        assert!(!dax_stream_is_ours(
            None,
            None,
            &status(0x0400_0000, OURS)
        ));
        // A channel we did not create is not ours whoever owns it.
        let other_channel =
            parse_dax_stream_status("stream 0x04000001 type=dax_rx dax_channel=2 client_handle=0x2ABC")
                .expect("status");
        assert!(!dax_stream_is_ours(None, Some(OURS), &other_channel));

        // Once the create reply has named our stream, only that id is ours.
        let ours = stream_from_create_reply(0, "0x04000000").expect("the create reply grants it");
        assert!(dax_stream_is_ours(
            Some(ours),
            Some(OURS),
            &status(ours, OURS)
        ));
        assert!(
            !dax_stream_is_ours(Some(ours), Some(OURS), &status(0x0400_0009, THEIRS)),
            "a foreign status must not repoint the audio filter"
        );
    }

    /// One slice status line off the wire.
    fn slice(body: &str) -> SliceStatus {
        parse_slice_status(body).expect("a slice status body")
    }

    /// DAX FOLLOWS THE SLICE NEXUS IS OPERATING, NOT WHOEVER HAS FOCUS (audit #1014/#1026).
    ///
    /// CAT is pinned to the slice its address names (5002 = A), while the audio bound itself to
    /// whatever the radio broadcast as `active=1` — so with slice B active when native audio
    /// started, the decoders listened to B while the dial, the waterfall and the log all said A,
    /// and nothing anywhere compared them.
    #[test]
    fn dax_binds_the_slice_the_cat_link_drives() {
        const OURS: u32 = 0x2ABC;
        let mut v = SliceView::default();
        // The `sub slice all` dump: A and B both in use, B has the operator's focus.
        v.merge(&slice("slice 0 in_use=1 RF_frequency=14.074 active=0 client_handle=0x2ABC"));
        v.merge(&slice("slice 1 in_use=1 RF_frequency=7.074 active=1 client_handle=0x2ABC"));

        assert_eq!(
            v.bind_target(Some(OURS), Some(0)),
            Some(0),
            "CAT on port 5002 drives slice A — the audio must come from A"
        );
        assert_eq!(
            v.bind_target(Some(OURS), Some(1)),
            Some(1),
            "…and from B when the CAT address is B's port"
        );
        assert_eq!(
            v.bind_target(Some(OURS), None),
            Some(1),
            "with no slice-bearing CAT port, the active slice is the honest fallback"
        );
    }

    /// A PARTIAL DELTA MUST STILL MOVE THE BIND (audit #1025).
    ///
    /// SmartSDR status lines carry only the keys that changed, and the bind test demanded
    /// `in_use=1` AND `active=1` in ONE line — which a focus change (`slice 1 active=1`) can never
    /// satisfy, so the re-bind the code promised could not fire at all.
    #[test]
    fn a_focus_change_that_carries_one_key_still_re_binds() {
        const OURS: u32 = 0x2ABC;
        let mut v = SliceView::default();
        v.merge(&slice("slice 0 in_use=1 active=1 client_handle=0x2ABC"));
        v.merge(&slice("slice 1 in_use=1 active=0 client_handle=0x2ABC"));
        assert_eq!(v.bind_target(Some(OURS), None), Some(0), "precondition");

        // The operator moves focus at the front panel. One key, no `in_use`.
        v.merge(&slice("slice 0 active=0"));
        v.merge(&slice("slice 1 active=1"));
        assert_eq!(
            v.bind_target(Some(OURS), None),
            Some(1),
            "the merged view remembers in_use from the earlier dump"
        );

        // …and a slice the radio says has gone away is not a bind target any more.
        v.merge(&slice("slice 1 in_use=0"));
        assert_eq!(v.bind_target(Some(OURS), None), None);
        assert_eq!(v.bind_target(Some(OURS), Some(1)), None);
    }

    /// ANOTHER CLIENT'S SLICE IS NEVER BOUND (audit #1014). `slice set N dax=1` is a write to the
    /// operator's radio; on a multi-client Flex the active slice can be a Maestro's or a second
    /// Nexus window's.
    #[test]
    fn a_slice_proven_to_belong_to_another_client_is_never_bound() {
        const OURS: u32 = 0x2ABC;
        let mut v = SliceView::default();
        v.merge(&slice("slice 2 in_use=1 active=1 client_handle=0x9999"));
        assert_eq!(v.bind_target(Some(OURS), None), None);
        assert_eq!(v.bind_target(Some(OURS), Some(2)), None);
        // But silence is not proof of foreignness: a radio that never sends client_handle, or a
        // session whose greeting has not arrived yet, must still get audio on a single-client desk.
        let mut v = SliceView::default();
        v.merge(&slice("slice 0 in_use=1 active=1"));
        assert_eq!(v.bind_target(Some(OURS), None), Some(0));
        assert_eq!(v.bind_target(None, Some(0)), Some(0));
    }

    /// The operator's slice routing is borrowed, not taken (audit #1027/#1054).
    #[test]
    fn the_slice_dax_routing_we_found_is_what_gets_restored() {
        let mut t = SliceDaxTracker::default();
        // The `sub slice all` dump: slice 2 was on channel 3 for the operator's other software.
        t.observe(2, Some(3));
        // The status that triggers the bind carries no `dax=` at all — a partial update.
        t.observe(2, None);
        t.claim(2);
        assert_eq!(t.restore(2), Some(3), "put back what the radio reported");

        // The radio now echoes OUR write back. That must not become the "prior" value.
        t.observe(2, Some(DAX_CHANNEL));
        t.claim(2);
        assert_eq!(
            t.restore(2),
            Some(3),
            "our own dax=1 must never be mistaken for what we found"
        );

        // A slice whose dax value was never seen is left alone rather than guessed at.
        t.claim(5);
        assert_eq!(t.restore(5), None);
        // …and a slice we never claimed has nothing to restore.
        assert_eq!(t.restore(9), None);
    }
}
