//! The TCI session's audio on Nexus's side, on the session's own thread: receive, the transceiver's
//! audio at the decoders' rate, for the radio loop in place of the sound card, and for the waterfall
//! and Remote Listen; and transmit, Nexus's audio for an over of ours, pulled by the server's
//! TX_CHRONO.
//!
//! ## Receive
//! A TCI server streams receive audio as binary frames, float32 stereo at 48 kHz unless told
//! otherwise (TCI 2.0 §3.4, §4.3), once the session has asked with `audio_start` (the radio's
//! `tci_audio`). Channel 0 is the real audio in both the one- and the two-channel stream (in DIGU and
//! DIGL the second channel is the quadrature one), and it is what WSJT-X takes too
//! (`TCITransceiver.cpp:909-985`). The codec has already made a sample that is not a number silence.
//! Channel 0 is resampled to the modem's 12 kHz with the capture path's own resampler (built again
//! when the stream's rate changes), and the operator's RX gain is applied with the sound card's own
//! clamp, so the slider means one thing on every route. Then it goes two ways:
//! - to the radio loop, which takes what arrived since its last tick ([`Receive::take`]). At most
//!   [`RING_CAP`] samples are kept, the oldest dropped first: a loop that stops reading (a radio the
//!   monitor holds, or one whose receive went back to the sound card) loses audio, never memory;
//! - to the receive tap's ring, while the radio loop has handed one over ([`Receive::set_tap`]). This
//!   thread is that ring's one producer; the waterfall's FFT and Remote Listen drain it on their own
//!   thread (`crate::rxtap`), so no CAT wait in the radio loop (a TCI write waits for the server's
//!   report) can stall them. The ring is held weakly: once the tap has moved on to another source,
//!   nothing more is written.
//!
//! ## Transmit
//! [`TciTx`] is the radio loop's alternate transmit route ([`crate::backend::TxTee`]) while the
//! radio's Audio over TCI is on. It is exclusive, as the Flex client's DAX route is: an over goes to
//! the SDR program or to the sound card, never both. The loop hands it a whole over at its key and
//! must not wait on it, so the over is queued at the modem rate as it is. The server pulls it: each
//! TX_CHRONO asks for a frame's worth, and the session answers it at once ([`Streams::transmit`])
//! with the queue resampled to the server's rate ([`ToServer`], the capture path's resampler), the
//! operator's TX level applied as it leaves, as the sound card applies it, and silence for whatever
//! the queue does not hold (TCI 2.0 §3.4 prefers silence to nothing). So the rest of an over waits
//! in Nexus until it is due, and a mid-over switch replaces it there ([`TciTx`]'s
//! [`crate::backend::TxTee::retune`]): the Flex DAX shape, with the server's clock in place of the
//! pacer's. What the resampler has made ahead (at most one pull) and what the server holds already
//! (its `tx_stream_audio_buffering`) play as they were.
//! - **Only for an over of ours that takes Nexus's audio.** Audio is queued only while one is open
//!   on the session ([`Connection::audio_over_open`]); anything handed over otherwise is dropped with
//!   whatever was queued, never held for a later key. The session asks for audio only for such an
//!   over, answers silence after our stop, and nothing at all for another program's key.
//! - **Stop TX empties it** ([`crate::backend::TxTee::flush`]), the resampler's samples ahead
//!   included, so nothing of a stopped over leaves after the stop, at this key or the next.
//! - **When it is the route** is the radio loop's to say: it installs it once the session's
//!   transmit route is up ([`super::TciDaemon::tx_route_ready`]), and only then lets the client key
//!   Nexus's audio ([`super::TciDaemon::set_tx_route`]).
//!
//! Nexus's own design, not a port.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::Instant;

use tempo_codec::tci_audio::ToServer;
use tempo_net::tci::frame::Block;
use tempo_net::tci::session::{Connection, Streams};

use crate::capture_resample::CaptureResampler;
use crate::monitor::SpscRing;

/// The decoders' rate: what [`Receive::take`] hands the radio loop and the tap is fed at.
pub const MODEM_RATE: u32 = 12_000;

/// About ten seconds at [`MODEM_RATE`], as the Flex client keeps for its DAX receive audio.
pub(crate) const RING_CAP: usize = 120_000;

/// The most of an over [`TciTx`] holds at the modem rate: 1800 s, the longest over Nexus sends
/// (FST4 and FST4W-1800), as the sound card's own queue for long overs holds (`device.rs`). An over
/// is handed over whole at its key, so no legitimate one is cut; past it the oldest audio goes.
const TX_QUEUE_CAP: usize = MODEM_RATE as usize * 1_800;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the session's thread and the radio loop share about receive audio.
pub struct Receive {
    /// Modem-rate audio since the last take, RX gain applied.
    ring: Mutex<Vec<f32>>,
    /// When the last receive block for our transceiver arrived: the radio loop's receive floor.
    last_block: Mutex<Option<Instant>>,
    /// The operator's RX gain, as the bits of an `f32`.
    gain: AtomicU32,
    /// The receive tap's ring while TCI audio is what the station hears.
    tap: Mutex<Weak<SpscRing>>,
}

impl Default for Receive {
    fn default() -> Receive {
        Receive {
            ring: Mutex::new(Vec::new()),
            last_block: Mutex::new(None),
            gain: AtomicU32::new(1.0f32.to_bits()),
            tap: Mutex::new(Weak::new()),
        }
    }
}

impl Receive {
    /// The modem-rate receive audio since the last call.
    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *lock(&self.ring))
    }

    /// When the last receive block for this transceiver arrived, if one has.
    pub fn last_block(&self) -> Option<Instant> {
        *lock(&self.last_block)
    }

    /// The operator's RX gain, applied to every block from the next one on.
    pub fn set_gain(&self, gain: f32) {
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    /// Feed `ring` (the receive tap's) from the next block on, or stop feeding any.
    pub fn set_tap(&self, ring: Option<&Arc<SpscRing>>) {
        *lock(&self.tap) = ring.map_or_else(Weak::new, Arc::downgrade);
    }

    /// One block's modem-rate audio: kept for the radio loop and written to the tap.
    fn push(&self, samples: &[f32]) {
        {
            let mut ring = lock(&self.ring);
            ring.extend_from_slice(samples);
            if ring.len() > RING_CAP {
                let excess = ring.len() - RING_CAP;
                ring.drain(..excess);
            }
        }
        let tap = lock(&self.tap).upgrade();
        if let Some(tap) = tap {
            tap.push_slice(samples);
        }
    }
}

/// Nexus's transmit audio into the session: the radio loop's transmit route while the radio's
/// Audio over TCI is on (see the module notes).
pub struct TciTx {
    /// The session the audio is for, once it exists ([`Self::attach`]): audio is taken only while
    /// an over of ours that takes Nexus's audio is open on it.
    conn: OnceLock<Weak<Connection>>,
    /// The operator's TX level (f32 bits, 0–1), applied as the audio leaves. Unity until the sound
    /// card pushes the operator's level on install.
    level: AtomicU32,
    queue: Mutex<TxQueue>,
    /// When the session last pulled audio for an over of ours.
    pulled: Mutex<Option<Instant>>,
}

#[derive(Default)]
struct TxQueue {
    /// The over at the modem rate, as handed over, not yet pulled.
    modem: VecDeque<f32>,
    /// The resampler to the server's rate, and what it has made but not handed out yet.
    to_server: ToServer,
}

impl Default for TciTx {
    fn default() -> TciTx {
        TciTx {
            conn: OnceLock::new(),
            level: AtomicU32::new(1.0f32.to_bits()),
            queue: Mutex::default(),
            pulled: Mutex::new(None),
        }
    }
}

impl TciTx {
    /// The session this route carries audio into. Once; until then nothing is taken.
    pub fn attach(&self, conn: Weak<Connection>) {
        let _ = self.conn.set(conn);
    }

    /// Whether an over of ours that takes Nexus's audio is open on the session. Asked on the radio
    /// loop's thread only: the session's own thread must never hold the last reference to it.
    fn over_open(&self) -> bool {
        self.conn
            .get()
            .and_then(Weak::upgrade)
            .is_some_and(|c| c.audio_over_open())
    }

    /// Answer one TX_CHRONO: up to `out.len()` samples at `rate`, the TX level applied, and how many
    /// were written; the session sends silence for the rest. The session asks only while an over
    /// of ours that takes Nexus's audio is open. On the session's thread.
    fn pull(&self, rate: u32, out: &mut [f32]) -> usize {
        *lock(&self.pulled) = Some(Instant::now());
        let level = f32::from_bits(self.level.load(Ordering::Relaxed));
        let mut q = lock(&self.queue);
        let TxQueue { modem, to_server } = &mut *q;
        let n = to_server.fill(rate, out, &mut |buf| {
            let n = buf.len().min(modem.len());
            for (slot, s) in buf.iter_mut().zip(modem.drain(..n)) {
                *slot = s;
            }
            n
        });
        // The sound card's rule: level first, then the clamp the device format applies.
        for s in &mut out[..n] {
            *s = (*s * level).clamp(-1.0, 1.0);
        }
        n
    }

    /// How much of the over is still queued at the modem rate.
    pub fn queued(&self) -> usize {
        lock(&self.queue).modem.len()
    }

    /// When the session last pulled audio for an over of ours, if it has.
    pub fn last_pull(&self) -> Option<Instant> {
        *lock(&self.pulled)
    }
}

impl crate::backend::TxTee for TciTx {
    /// Queue an over. Called on the radio loop with the whole waveform: a copy, nothing more. Only
    /// while an over of ours that takes Nexus's audio is open: otherwise it goes, with anything
    /// already queued.
    fn feed(&self, mono12: &[f32]) {
        if !self.over_open() {
            self.flush();
            return;
        }
        let mut q = lock(&self.queue);
        q.modem.extend(mono12);
        if q.modem.len() > TX_QUEUE_CAP {
            let excess = q.modem.len() - TX_QUEUE_CAP;
            q.modem.drain(..excess);
        }
    }

    fn set_level(&self, level: f32) {
        self.level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Hard Stop TX: drop everything queued, and what the resampler has made ahead. Returns the
    /// count dropped at the modem rate.
    fn flush(&self) -> usize {
        let mut q = lock(&self.queue);
        let n = q.modem.len();
        q.modem.clear();
        q.to_server.reset();
        n
    }

    /// The rest of the over, played anew (a mid-over switch). What has not left is the queue at the
    /// modem rate: the server pulls in real time, so it has been handed only what was due (and what
    /// the resampler made ahead of it, at most one pull). The rebuild is made with the queue
    /// unlocked, so the session goes on answering the server while it is made, and then replaces
    /// what is still queued by then, sample for sample at the same place in the over: never more,
    /// so the over ends when it would have. The resampler runs on through the splice, which makes
    /// the audio there the resampled splice itself. Not for an over that is not open (Stop TX, or
    /// an over the session ended), and not for a queue emptied meanwhile (Stop TX) or one that grew
    /// (nothing here can tell where its over ends): `false`, as with nothing left.
    fn retune(&self, tail: &mut dyn FnMut(usize, usize) -> Vec<f32>) -> bool {
        let left = lock(&self.queue).modem.len();
        if left == 0 || !self.over_open() {
            return false;
        }
        let rebuilt = tail(left, 0);
        {
            let mut q = lock(&self.queue);
            let still = q.modem.len();
            if still == 0 || still > left {
                return false;
            }
            // `rebuilt` ends where the over ends, as the queue does. Short of what is still queued
            // (never, from the engine), it plays what it has and the over ends early.
            let rest = &rebuilt[rebuilt.len().saturating_sub(still)..];
            q.modem.clear();
            q.modem.extend(rest);
        }
        // An over the session ended meanwhile pulls none of it: nothing is booked as sent.
        self.over_open()
    }
}

/// The [`Streams`] a TCI session is given: its receive audio into [`Receive`], and Nexus's transmit
/// audio from [`TciTx`].
pub struct SessionAudio {
    receive: Arc<Receive>,
    /// The stream's rate, and the resampler from it to [`MODEM_RATE`].
    resampler: Option<(u32, CaptureResampler)>,
    tx: Arc<TciTx>,
}

impl SessionAudio {
    pub fn new(receive: Arc<Receive>, tx: Arc<TciTx>) -> SessionAudio {
        SessionAudio {
            receive,
            resampler: None,
            tx,
        }
    }
}

impl Streams for SessionAudio {
    fn receive(&mut self, block: &Block<'_>) {
        *lock(&self.receive.last_block) = Some(Instant::now());
        let rate = block.header.sample_rate;
        let resampler = match &mut self.resampler {
            Some((r, resampler)) if *r == rate => resampler,
            slot => {
                &mut slot
                    .insert((rate, CaptureResampler::new(rate, MODEM_RATE)))
                    .1
            }
        };
        let channel: Vec<f32> = block.channel(0).collect();
        let mut out = resampler.process(&channel);
        if out.is_empty() {
            return;
        }
        let gain =
            crate::device::clamp_rx_gain(f32::from_bits(self.receive.gain.load(Ordering::Relaxed)));
        if gain != 1.0 {
            for s in &mut out {
                *s *= gain;
            }
        }
        self.receive.push(&out);
    }

    fn transmit(&mut self, rate: u32, out: &mut [f32]) -> usize {
        self.tx.pull(rate, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_net::tci::frame::{
        decode, encode_block, BlockHeader, BlockKind, Channels, Frame, LengthMeaning, SampleFormat,
    };
    use tempo_net::tci::Trx;

    /// A receive block's bytes: `frames` of a 1 kHz tone at `amp` on channel 0, its negative on
    /// channel 1, at `rate`.
    fn block_bytes(rate: u32, frames: usize, amp: f32) -> Vec<u8> {
        let header = BlockHeader {
            trx: Trx::new(0).unwrap(),
            kind: BlockKind::RxAudio,
            sample_rate: rate,
            format: SampleFormat::Float32,
            channels: Channels::Two,
        };
        let samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::TAU * 1_000.0 * i as f32 / rate as f32).sin();
                [v, -v]
            })
            .collect();
        encode_block(&header, LengthMeaning::Values, &samples).unwrap()
    }

    fn feed(audio: &mut SessionAudio, bytes: &[u8]) {
        match decode(bytes).unwrap() {
            Frame::Block(block) => audio.receive(&block),
            _ => panic!("not a block"),
        }
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Channel 0 at 48 kHz arrives at the modem's 12 kHz, a quarter as many samples, with the
    /// tone's level kept (the second channel is never mixed in: it would cancel the tone here).
    #[test]
    fn channel_zero_arrives_at_the_modem_rate() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone(), Arc::default());
        assert_eq!(receive.last_block(), None);
        for _ in 0..10 {
            feed(&mut audio, &block_bytes(48_000, 2_048, 0.5));
        }
        let got = receive.take();
        let fed = 10 * 2_048 / 4;
        assert!(
            got.len() <= fed && got.len() + 64 >= fed,
            "{} samples for {fed}",
            got.len()
        );
        // Past the resampler's first window, the tone at its own level.
        let p = peak(&got[200..]);
        assert!((0.45..=0.55).contains(&p), "peak {p}");
        assert!(receive.last_block().is_some());
        assert!(receive.take().is_empty(), "each sample is taken once");
    }

    /// The operator's RX gain is applied as the sound card applies it (its clamp included), and a
    /// rate change builds the resampler again rather than smearing two rates.
    #[test]
    fn the_rx_gain_and_a_new_rate_apply_from_the_next_block() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone(), Arc::default());
        receive.set_gain(2.0);
        for _ in 0..10 {
            feed(&mut audio, &block_bytes(48_000, 2_048, 0.25));
        }
        let p = peak(&receive.take()[200..]);
        assert!((0.45..=0.55).contains(&p), "gain 2: peak {p}");
        receive.set_gain(50.0);
        for _ in 0..10 {
            feed(&mut audio, &block_bytes(24_000, 1_024, 0.05));
        }
        let got = receive.take();
        assert!(
            got.len() <= 5_120 && got.len() + 64 >= 5_120,
            "24 kHz: {} samples",
            got.len()
        );
        let p = peak(&got[200..]);
        assert!((0.36..=0.44).contains(&p), "gain clamped to 8: peak {p}");
    }

    /// What the radio loop does not take is bounded: the oldest goes first.
    #[test]
    fn audio_nobody_takes_is_bounded() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone(), Arc::default());
        for _ in 0..1_200 {
            feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        }
        assert_eq!(receive.take().len(), RING_CAP);
    }

    /// The tap's ring gets the same audio while it is given, and nothing once the tap has let it
    /// go, or once another ring is given.
    #[test]
    fn the_tap_is_fed_only_while_it_holds_its_ring() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone(), Arc::default());
        let ring = Arc::new(SpscRing::new(1 << 16));
        receive.set_tap(Some(&ring));
        feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        let taken = receive.take();
        assert!(!taken.is_empty());
        assert_eq!(ring.len(), taken.len(), "the tap got what the loop got");
        let before = ring.len();
        receive.set_tap(None);
        feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        assert_eq!(ring.len(), before, "no tap: nothing written");
        receive.set_tap(Some(&ring));
        let weak = Arc::downgrade(&ring);
        drop(ring);
        assert!(weak.upgrade().is_none());
        feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        assert!(!receive.take().is_empty(), "the loop's audio goes on");
    }
}
