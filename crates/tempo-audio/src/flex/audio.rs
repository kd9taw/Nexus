//! DAX audio for Nexus's own Flex client, on the client's one SmartSDR session: receive per slice
//! in use, and the over's transmit audio in the tested format. Opt-in (`flex_native_audio`, Beta),
//! like the client itself; the radio loop decides when it is on ([`super::FlexDaemon::set_native_audio`]).
//!
//! ## Receive
//! Every slice of ours in use holds the DAX channel the radio reports for it, through the
//! refcounted broker ([`DaxBroker`]): the channel's first holder creates its `dax_rx` stream, the
//! last release removes it after the grace window, a stream the radio drops while it is held
//! comes back, and no channel without a slice gets a stream (the "DAX starvation" the older worker
//! documents: idle streams make the radio share audio among them). A slice of ours with no channel
//! is given the lowest one no other slice and no other client's stream uses. A stream is ours by
//! our create's reply or by a status the radio stamps with our handle; it stops being ours when
//! the radio reports it removed. Audio arrives on the session's UDP socket and is accepted only for
//! our streams; a lost packet becomes its own duration of silence (the 4-bit packet count), a late
//! one is dropped, and the rest is resampled 24 → 12 kHz and kept per channel. The radio loop
//! reads the channel of the slice the shim serves ([`Audio::take_audio`]).
//!
//! ## Transmit
//! [`DaxTx`] is the radio loop's alternate transmit route ([`crate::backend::TxTee`]): the over's
//! 12 kHz audio is queued as it is (the loop hands over a whole over in one call, and must not
//! wait on it), then resampled to 24 kHz a packet at a time on the pacer's thread and paced out in
//! real time, 128 stereo frames a packet, in the tested format ([`streams::dax_tx_packet`], float32
//! stereo) to the radio's VITA-49 port (4991). The operator's TX level is applied as each packet leaves, as the sound card applies it,
//! so the Pwr slider means the same on both routes and DAX never carries full-scale audio by
//! default. A packet leaves only while an over of ours (`xmit 1`) is keyed on the session, never
//! during a CWX word, a tune or an ATU cycle, on our own transmit stream, and while no other
//! program feeds DAX: anything queued otherwise is dropped, never held for a later key. The pacer wakes the moment an over is queued, so its first packet leaves
//! without waiting out an idle tick.
//!
//! Nexus's own design, not a port: the broker and the formats it uses are ported in
//! `tempo_net::flex::streams`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_net::flex::admission::another_dax_feeder;
use tempo_net::flex::encode::Command;
use tempo_net::flex::model::{owner_of, Owner, StatusModel};
use tempo_net::flex::session::{Connection, Phase};
use tempo_net::flex::streams::{
    self, Action, DaxBroker, Holder, AUDIO_CLASS, AUDIO_REDUCED_CLASS, CHANNELS, DAX_RATE_HZ,
    TX_FRAMES_PER_PACKET,
};
use tempo_net::flexvita::{parse_vita, VitaGap, VitaSequence};

use crate::capture_resample::CaptureResampler;

use super::ClientState;

/// The decoders' rate.
const MODEM_RATE: u32 = 12_000;
/// Per channel, about ten seconds at 12 kHz: a stalled reader loses the oldest audio, never
/// memory.
const RING_CAP: usize = 120_000;
/// How often the control thread looks at the radio's streams and slices.
const CONTROL_TICK: Duration = Duration::from_millis(50);
/// How long the receive thread waits for a datagram before it looks at the stop flag again.
const RX_POLL: Duration = Duration::from_millis(200);
/// How long a stream or slice command waits for the radio's reply.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1_500);
/// How long before a slice that still has no DAX channel is asked again.
const SLICE_DAX_RETRY: Duration = Duration::from_secs(2);
/// Wall-clock airtime of one DAX TX packet: 128 frames at 24 kHz, 5.333 ms.
const TX_PACKET_PERIOD: Duration =
    Duration::from_nanos(TX_FRAMES_PER_PACKET as u64 * 1_000_000_000 / DAX_RATE_HZ as u64);
/// The pacer's granularity while a packet's slot is in the future.
const TX_PACER_TICK: Duration = Duration::from_millis(1);
/// How long the idle pacer waits for audio before it looks at the stop flag again. A queued over
/// wakes it at once.
const TX_IDLE_WAIT: Duration = Duration::from_millis(100);
/// How far behind schedule the pacer may fall before it resyncs instead of catching up: a
/// descheduled thread must not turn its backlog into a burst at the radio.
const TX_MAX_CATCHUP: Duration = Duration::from_millis(100);
/// About twenty seconds at the modem rate; an FT8 over is 12.6 s.
const TX_QUEUE_CAP: usize = MODEM_RATE as usize * 20;
/// Modem-rate samples resampled at a time: one packet's worth.
const TX_CHUNK: usize = TX_FRAMES_PER_PACKET * MODEM_RATE as usize / DAX_RATE_HZ as usize;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The DAX audio of one client session: its three threads and what they share.
pub(crate) struct Audio {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

struct Shared {
    conn: Weak<Connection>,
    state: Arc<ClientState>,
    stop: AtomicBool,
    /// Our receive streams, id → channel, as the control thread last saw them.
    rx_streams: Mutex<BTreeMap<u32, u8>>,
    /// 12 kHz mono audio per channel since the last take.
    rings: Mutex<BTreeMap<u8, Vec<f32>>>,
    /// The channel the radio loop reads: the served slice's. 0 while there is none.
    served_channel: AtomicU8,
    /// The radio's DAX source flag as the control thread last read it: 0 unknown, 1 the mic,
    /// 2 DAX.
    radio_dax: AtomicU8,
    /// Another program feeds the radio's DAX transmit audio, as the control thread last saw it.
    other_feeder: AtomicBool,
    tx: Arc<DaxTx>,
}

impl Audio {
    /// Start the threads: receive on `udp`, send DAX TX from it to `vita`.
    pub(crate) fn start(
        conn: Weak<Connection>,
        state: Arc<ClientState>,
        udp: UdpSocket,
        vita: SocketAddr,
    ) -> std::io::Result<Audio> {
        udp.set_read_timeout(Some(RX_POLL))?;
        let tx_sock = udp.try_clone()?;
        let tx = Arc::new(DaxTx::new(conn.clone()));
        let shared = Arc::new(Shared {
            conn,
            state,
            stop: AtomicBool::new(false),
            rx_streams: Mutex::new(BTreeMap::new()),
            rings: Mutex::new(BTreeMap::new()),
            served_channel: AtomicU8::new(0),
            radio_dax: AtomicU8::new(0),
            other_feeder: AtomicBool::new(false),
            tx,
        });
        let mut threads = Vec::new();
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new().name(name.into()).spawn(f)
        };
        {
            let shared = Arc::clone(&shared);
            threads.push(spawn(
                "flex-dax-rx",
                Box::new(move || receive(&shared, &udp)),
            )?);
        }
        {
            let shared = Arc::clone(&shared);
            threads.push(spawn(
                "flex-dax-tx",
                Box::new(move || pace(&shared.tx, &shared.stop, &tx_sock, vita)),
            )?);
        }
        {
            let shared = Arc::clone(&shared);
            threads.push(spawn("flex-dax", Box::new(move || control(&shared)))?);
        }
        Ok(Audio { shared, threads })
    }

    /// The served slice's 12 kHz receive audio since the last call.
    pub(crate) fn take_audio(&self) -> Vec<f32> {
        let channel = self.shared.served_channel.load(Ordering::Acquire);
        if channel == 0 {
            return Vec::new();
        }
        lock(&self.shared.rings)
            .get_mut(&channel)
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// The radio's DAX source flag, as last read while native audio was on.
    pub(crate) fn radio_dax(&self) -> Option<bool> {
        match self.shared.radio_dax.load(Ordering::Acquire) {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }

    /// Whether another program feeds the radio's DAX transmit audio, as last seen while native
    /// audio was on.
    pub(crate) fn other_feeder(&self) -> bool {
        self.shared.other_feeder.load(Ordering::Acquire)
    }

    /// The transmit route.
    pub(crate) fn tx(&self) -> Arc<DaxTx> {
        Arc::clone(&self.shared.tx)
    }

    /// Our receive streams and their channels.
    #[cfg(test)]
    pub(crate) fn rx_streams(&self) -> BTreeMap<u32, u8> {
        lock(&self.shared.rx_streams).clone()
    }

    /// Stop the threads. The session's own teardown removes our streams.
    pub(crate) fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.tx.wake.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── Receive ──────────────────────────────────────────────────────────────────────────────────

fn receive(shared: &Shared, udp: &UdpSocket) {
    let mut sequences: BTreeMap<u32, VitaSequence> = BTreeMap::new();
    let mut resamplers: BTreeMap<u8, CaptureResampler> = BTreeMap::new();
    let mut buf = vec![0u8; 16 * 1024];
    while !shared.stop.load(Ordering::Relaxed) {
        let Ok((n, _)) = udp.recv_from(&mut buf) else {
            continue; // the poll timeout: look at the stop flag again
        };
        let Some(pkt) = parse_vita(&buf[..n]) else {
            continue;
        };
        let Some(class) = pkt
            .packet_class
            .filter(|c| *c == AUDIO_CLASS || *c == AUDIO_REDUCED_CLASS)
        else {
            continue;
        };
        // Our streams only: the audio class is shared with other clients' audio.
        let Some(id) = pkt.stream_id else { continue };
        let Some(channel) = lock(&shared.rx_streams).get(&id).copied() else {
            continue;
        };
        let gap = sequences.entry(id).or_default().observe(pkt.packet_count);
        if gap == VitaGap::Stale {
            continue;
        }
        let Some(stereo) = streams::without_trailer(pkt.payload, pkt.has_trailer)
            .and_then(|p| streams::decode_dax_audio(class, p))
        else {
            continue;
        };
        let mono: Vec<f32> = stereo
            .chunks_exact(2)
            .map(|lr| 0.5 * (lr[0] + lr[1]))
            .collect();
        let rs = resamplers
            .entry(channel)
            .or_insert_with(|| CaptureResampler::new(DAX_RATE_HZ, MODEM_RATE));
        // A lost packet costs its own airtime: position in the ring is time.
        let mut out = match gap {
            VitaGap::Lost(lost) => rs.process(&vec![0.0; lost as usize * mono.len()]),
            _ => Vec::new(),
        };
        out.extend(rs.process(&mono));
        let mut rings = lock(&shared.rings);
        let ring = rings.entry(channel).or_default();
        ring.extend_from_slice(&out);
        if ring.len() > RING_CAP {
            let excess = ring.len() - RING_CAP;
            ring.drain(..excess);
        }
    }
}

// ── The control thread: slices, channels, streams ────────────────────────────────────────────

/// The lowest DAX channel no slice, no other client's receive stream and no offer still waiting
/// for the radio's echo (`offered`) uses.
fn free_channel(model: &StatusModel, ours: Option<u32>, offered: &BTreeSet<u8>) -> Option<u8> {
    let used: BTreeSet<u8> = model
        .slices
        .values()
        .filter_map(|s| s.dax_channel)
        .chain(
            model
                .streams
                .values()
                .filter(|s| {
                    s.kind.as_deref() == Some("dax_rx")
                        && owner_of(s.client_handle, ours) != Owner::Ours
                })
                .filter_map(|s| s.dax_channel),
        )
        .filter_map(|c| u8::try_from(c).ok())
        .chain(offered.iter().copied())
        .collect();
    CHANNELS.clone().find(|c| !used.contains(c))
}

/// A channel the radio reported, if it is a DAX channel.
fn channel(value: Option<i32>) -> Option<u8> {
    value
        .and_then(|c| u8::try_from(c).ok())
        .filter(|c| CHANNELS.contains(c))
}

/// What the control thread keeps between ticks.
#[derive(Default)]
struct Control {
    broker: DaxBroker,
    /// Slice → the channel it holds.
    held: BTreeMap<u8, u8>,
    /// Streams of ours the radio has reported: only those can be reported removed.
    seen: BTreeSet<u32>,
    /// The channel a slice was last asked to take, and when.
    asked: BTreeMap<u8, (u8, Instant)>,
}

fn control(shared: &Shared) {
    let epoch = Instant::now();
    let mut c = Control::default();
    while !shared.stop.load(Ordering::Relaxed) {
        std::thread::sleep(CONTROL_TICK);
        let Some(conn) = shared.conn.upgrade() else {
            break;
        };
        let wanted = shared.state.native_audio.load(Ordering::Relaxed);
        if !wanted && c.held.is_empty() && c.broker.snapshot().is_empty() {
            shared.tx.set_route(None);
            continue; // nothing to do and nothing to undo
        }
        let snap = conn.snapshot();
        if snap.phase == Phase::Closed {
            break;
        }
        if snap.phase != Phase::Ready {
            continue;
        }
        let now = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
        tick(&mut c, shared, &conn, &snap, wanted, now);
    }
    // The session is gone or the client is stopping: forget, send nothing (the radio reaps a
    // departed client's streams, and a stopping client's session removes them in its teardown).
    c.broker.reset_for_disconnect();
    lock(&shared.rx_streams).clear();
    shared.served_channel.store(0, Ordering::Release);
    shared.tx.set_route(None);
}

fn tick(
    c: &mut Control,
    shared: &Shared,
    conn: &Connection,
    snap: &tempo_net::flex::session::Snapshot,
    wanted: bool,
    now: u64,
) {
    let ours = snap.handle;
    let model = &snap.model;
    shared.radio_dax.store(
        match model.transmit.dax {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        },
        Ordering::Release,
    );
    // What the radio says about our streams: registered by a status stamped with our handle,
    // forgotten when one it reported is gone.
    let reported: BTreeMap<u32, u8> = model
        .streams
        .iter()
        .filter(|(_, s)| {
            s.kind.as_deref() == Some("dax_rx") && owner_of(s.client_handle, ours) == Owner::Ours
        })
        .filter_map(|(id, s)| Some((*id, channel(s.dax_channel)?)))
        .collect();
    for (id, ch) in &reported {
        c.seen.insert(*id);
        if c.broker.channel_of(*id) != Some(*ch) {
            c.broker.registered(*id, *ch, now);
        }
    }
    let gone: Vec<u32> = c
        .seen
        .iter()
        .filter(|id| !model.streams.contains_key(id))
        .copied()
        .collect();
    for id in gone {
        c.seen.remove(&id);
        c.broker.unregistered(id, now);
    }
    // What each slice of ours in use wants.
    let mut want: BTreeMap<u8, u8> = BTreeMap::new();
    if wanted {
        for (index, s) in &model.slices {
            if owner_of(s.client_handle, ours) != Owner::Ours || s.in_use == Some(false) {
                continue;
            }
            match channel(s.dax_channel) {
                Some(ch) => {
                    want.insert(*index, ch);
                }
                None => {
                    let due = c
                        .asked
                        .get(index)
                        .is_none_or(|(_, at)| at.elapsed() >= SLICE_DAX_RETRY);
                    let offered: BTreeSet<u8> = c
                        .asked
                        .iter()
                        .filter(|(s, (_, at))| *s != index && at.elapsed() < SLICE_DAX_RETRY)
                        .map(|(_, (ch, _))| *ch)
                        .collect();
                    if let (true, Some(ch)) = (due, free_channel(model, ours, &offered)) {
                        c.asked.insert(*index, (ch, Instant::now()));
                        let _ = conn.request(
                            Command::SliceDax {
                                slice: *index,
                                channel: ch,
                            },
                            REQUEST_TIMEOUT,
                        );
                    }
                }
            }
        }
    }
    // Hold exactly those: a slice that went, or moved channel, lets go first.
    let mut actions = Vec::new();
    let held: Vec<(u8, u8)> = c.held.iter().map(|(s, ch)| (*s, *ch)).collect();
    for (slice, ch) in held {
        if want.get(&slice) != Some(&ch) {
            if let Some(h) = Holder::new(slice) {
                c.broker.release(ch, h, now);
            }
            c.held.remove(&slice);
        }
    }
    for (slice, ch) in &want {
        if c.held.get(slice) == Some(ch) {
            continue;
        }
        let Some(h) = Holder::new(*slice) else {
            continue;
        };
        let (_, create) = c.broker.acquire(*ch, h);
        actions.extend(create);
        c.held.insert(*slice, *ch);
    }
    actions.extend(c.broker.poll(now));
    for action in actions {
        match action {
            Action::Create { channel } => {
                match conn.request(Command::StreamCreateDaxRx { channel }, REQUEST_TIMEOUT) {
                    Ok(r) if r.code == 0 => {
                        // The reply names the stream: ours, whether or not its status names us.
                        if let Some(id) =
                            tempo_net::flex::ownership::parse_create_response_stream_id(&r.message)
                        {
                            c.broker.registered(id, channel, now);
                        }
                    }
                    _ => c.broker.create_failed(channel, now),
                }
            }
            Action::Remove { stream, .. } => {
                let _ = conn.request(Command::StreamRemove { stream }, REQUEST_TIMEOUT);
            }
        }
    }
    *lock(&shared.rx_streams) = c.broker.streams().collect();
    let served = super::shim::served_slice(model, ours)
        .and_then(|s| c.held.get(&s).copied())
        .unwrap_or(0);
    shared.served_channel.store(served, Ordering::Release);
    // Transmit: our own stream, while Nexus is the only program feeding DAX.
    let other = another_dax_feeder(model, ours, snap.dax_tx_stream).is_some();
    shared.other_feeder.store(other, Ordering::Release);
    shared
        .tx
        .set_route(snap.dax_tx_stream.filter(|_| wanted && !other));
}

// ── Transmit ─────────────────────────────────────────────────────────────────────────────────

/// The alternate transmit route: the over's audio as DAX TX packets, paced in real time.
pub struct DaxTx {
    conn: Weak<Connection>,
    /// The stream Nexus may send on; 0 while nothing may be sent.
    stream: AtomicU32,
    /// The operator's TX level (f32 bits, 0–1), applied as each packet leaves.
    level: AtomicU32,
    queue: Mutex<TxQueue>,
    wake: Condvar,
}

struct TxQueue {
    /// The over at the modem rate, as handed over.
    modem: VecDeque<f32>,
    resampler: CaptureResampler,
    /// Resampled to 24 kHz, unscaled (the level is applied as it leaves), at most a packet ahead.
    samples: VecDeque<f32>,
}

impl TxQueue {
    /// Whether a whole packet is queued, at either rate.
    fn has_packet(&self) -> bool {
        self.samples.len() + self.modem.len() * DAX_RATE_HZ as usize / MODEM_RATE as usize
            >= TX_FRAMES_PER_PACKET
    }
}

impl DaxTx {
    fn new(conn: Weak<Connection>) -> DaxTx {
        DaxTx {
            conn,
            stream: AtomicU32::new(0),
            // Unity until the sound card pushes the operator's level on install.
            level: AtomicU32::new(1.0f32.to_bits()),
            queue: Mutex::new(TxQueue {
                modem: VecDeque::new(),
                resampler: CaptureResampler::new(MODEM_RATE, DAX_RATE_HZ),
                samples: VecDeque::new(),
            }),
            wake: Condvar::new(),
        }
    }

    /// The stream Nexus may send on, or none. Losing it drops anything queued.
    fn set_route(&self, stream: Option<u32>) {
        let id = stream.unwrap_or(0);
        if self.stream.swap(id, Ordering::AcqRel) != id && id == 0 {
            crate::backend::TxTee::flush(self);
        }
    }

    /// Whether the route is up: Nexus may send on its own transmit stream.
    pub(crate) fn ready(&self) -> bool {
        self.stream.load(Ordering::Acquire) != 0
    }

    /// The stream to send on now: an over of ours (`xmit 1`) is keyed and the route is up. Not a
    /// CWX word, a tune or an ATU cycle: the radio makes those itself, and no audio rides them.
    fn sendable(&self) -> Option<u32> {
        let stream = self.stream.load(Ordering::Acquire);
        (stream != 0 && self.conn.upgrade().is_some_and(|c| c.over_keyed())).then_some(stream)
    }

    /// The next packet's stereo samples, scaled by the level as it stands now, or `None` when
    /// less than a packet is queued. Resamples only what this packet needs.
    fn take_packet(&self) -> Option<Vec<f32>> {
        let level = f32::from_bits(self.level.load(Ordering::Relaxed));
        let mut q = lock(&self.queue);
        while q.samples.len() < TX_FRAMES_PER_PACKET && !q.modem.is_empty() {
            let n = TX_CHUNK.min(q.modem.len());
            let chunk: Vec<f32> = q.modem.drain(..n).collect();
            let up = q.resampler.process(&chunk);
            q.samples.extend(up);
        }
        if q.samples.len() < TX_FRAMES_PER_PACKET {
            return None;
        }
        Some(
            q.samples
                .drain(..TX_FRAMES_PER_PACKET)
                // The sound card's rule: level first, then the clamp the device format applies.
                .map(|s| (s * level).clamp(-1.0, 1.0))
                .flat_map(|s| [s, s])
                .collect(),
        )
    }

    #[cfg(test)]
    fn queued(&self) -> usize {
        let q = lock(&self.queue);
        q.samples.len() + q.modem.len()
    }
}

impl crate::backend::TxTee for DaxTx {
    /// Queue an over. Called on the radio loop with the whole waveform: a copy, nothing more. The
    /// resampling and the sending are the pacer's.
    fn feed(&self, mono12: &[f32]) {
        if self.stream.load(Ordering::Acquire) == 0 {
            return; // no route: nothing may go on the air this way
        }
        let mut q = lock(&self.queue);
        q.modem.extend(mono12);
        if q.modem.len() > TX_QUEUE_CAP {
            let excess = q.modem.len() - TX_QUEUE_CAP;
            q.modem.drain(..excess);
        }
        drop(q);
        self.wake.notify_all();
    }

    fn set_level(&self, level: f32) {
        self.level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Hard Stop TX: drop everything queued, at either rate. Returns the count dropped.
    fn flush(&self) -> usize {
        let mut q = lock(&self.queue);
        let n = q.samples.len() + q.modem.len();
        q.samples.clear();
        q.modem.clear();
        n
    }
}

/// The pacer's clock: the next packet is due one packet's airtime after the last due time, so
/// the rate is the radio's whatever one sleep does; past [`TX_MAX_CATCHUP`] behind, resync.
fn advance(now: Instant, next_at: Instant) -> Instant {
    let next = next_at + TX_PACKET_PERIOD;
    if now.saturating_duration_since(next) > TX_MAX_CATCHUP {
        now
    } else {
        next
    }
}

fn pace(tx: &DaxTx, stop: &AtomicBool, sock: &UdpSocket, vita: SocketAddr) {
    let mut count = 0u8;
    let mut next_at = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        // Idle: wait for an over. Waking holds the clock at now, so an idle gap banks no credit
        // that would burst the start of the next one.
        {
            let mut q = lock(&tx.queue);
            if !q.has_packet() {
                q = tx
                    .wake
                    .wait_timeout(q, TX_IDLE_WAIT)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                let idle = !q.has_packet();
                drop(q);
                next_at = Instant::now();
                if idle {
                    continue;
                }
            }
        }
        let now = Instant::now();
        if now < next_at {
            std::thread::sleep(TX_PACER_TICK.min(next_at - now));
            continue;
        }
        // Only while an over of ours is keyed, on our own stream: otherwise the audio is dropped,
        // never kept for a later key.
        let Some(stream) = tx.sendable() else {
            crate::backend::TxTee::flush(tx);
            continue;
        };
        let Some(samples) = tx.take_packet() else {
            continue;
        };
        if let Some(packet) = streams::dax_tx_packet(stream, count, &samples) {
            count = (count + 1) & 0x0F;
            let _ = sock.send_to(&packet, vita);
        }
        next_at = advance(now, next_at);
    }
}

#[cfg(test)]
pub(crate) fn queued(tx: &DaxTx) -> usize {
    tx.queued()
}
