//! The SDR program's own spectrum in Nexus's RF scope and waterfall: TCI's IQ stream, behind the
//! radio's "SDR panadapter in the scope" switch (`RadioProfile::tci_pan`, off as it comes).
//!
//! ## What the operator sees
//! On a TCI radio with the switch on, the RF scope pane and the Phone and CW scopes draw the SDR
//! program's receiver as an RF panadapter: the receiver's whole IQ bandwidth (48 kHz at the
//! program's usual IQ rate) around its centre frequency, each station at its own RF frequency and
//! the dial where it is. Rows reach the scope through the spectrum feed's RF slot, the seam the Flex
//! and CI-V panadapters use, labelled [`SOURCE`], on a dB axis below the stream's full scale.
//!
//! ## The pieces, and the thread each runs on
//! - **The radio loop** calls [`reconcile`] every tick with the active radio's TCI client, if it
//!   has one. It reads the switch (one engine lock, as the Flex panadapter's reconcile takes) and
//!   renews the client's word ([`Pan`]: an atomic store). It never waits on the session and never
//!   touches a sample. A client the loop stops renewing (a radio switch: the outgoing client goes to
//!   the monitor pool) stops drawing within [`WANT_TTL_MS`] and stops its IQ [`STOP_GRACE_MS`]
//!   after that; the switch turned off stops both at once.
//! - **The session's thread** hands each IQ block of its transceiver to the pan's tap
//!   (`tempo_net::tci::session::IqTap`) with the centre frequency the server last reported:
//!   [`Intake::push`] checks it and keeps its samples, at most [`HELD`] pairs. No FFT, no feed.
//! - **The pan's own thread** ("tci-pan", started the first time the switch is on) sends
//!   `iq_start` and `iq_stop`, cuts the samples into windows of [`N`] pairs, runs tempo-core's
//!   complex FFT on each (`tempo_core::spectrum::iq_power`, microfft's kernel, already in the tree),
//!   averages the windows in power and publishes a row about every [`ROW_MS`]. At most
//!   [`MAX_WINDOWS`] windows go into a row and the rest are skipped, so the work is bounded whatever
//!   rate the server streams at; nothing is computed while the pan is not drawing.
//!
//! ## Where a row is
//! IQ is centred on the receiver's centre frequency (`dds`, TCI 2.0 §4.2), not on the dial. A bin's
//! RF frequency is that centre plus the bin's offset, which keeps its sign (I then Q, the spec's
//! order), so a station above the centre is drawn above it. The row's edges put the middle of each
//! bin on its frequency ([`Geometry::edges`]). Each block comes with the centre the server reported
//! before it, in stream order, so samples taken before a retune are never placed at the new centre:
//! a retune or a new IQ rate starts the windows and the row over.
//!
//! ## No stale rows
//! A row is built only from windows that arrived since the last row, and is published once. The
//! stream's samples are held to the wall clock ([`Intake::push`]): falling more than [`BEHIND_MS`]
//! behind means samples were lost, and nothing held or summed spans that gap. After a gap or a
//! stall the first [`SETTLE_MS`] of IQ is not drawn: a backlog the server or the network held back
//! arrives in a burst before the stream is live again, and it is late. A stream that runs ahead
//! (a server sending in bursts) moves the clock with it, so a loss after it is still seen. With no
//! IQ for [`STALL_MS`] while the transmitter is not on the air, the pan says so in the log and
//! empties the feed's RF slot, so the scope says its source stopped instead of holding an old row
//! as live.
//!
//! ## Starting and stopping
//! `iq_start` goes out once the switch is on and the session is ready. A server that sends no IQ
//! within [`START_WINDOW_MS`] does not offer it (piHPSDR's TCI has no `iq_start`): the pan sends
//! `iq_stop`, says why in the log once, draws nothing, and the scope keeps its previous source. It
//! asks again only when the switch is turned off and on, or on a new session. `iq_stop` goes out
//! when the switch is turned off, a little after a radio switch, and in the session's own close.
//! Nexus never sets the IQ rate: `iq_samplerate` is the server's per-client setting, left as the
//! server has it, and each block says its own rate.
//!
//! ## Bounds
//! What the server sends is outside data. Nothing is sized by a header field: a block's samples
//! are what its payload held, which the frame codec bounds (`tempo_net::tci::frame::MAX_PAYLOAD`),
//! and the pan holds at most [`HELD`] pairs between two looks. A block that is not two channels at
//! an IQ rate is dropped and counted ([`Counts`]), never a panic.
//!
//! ## Receive audio and transmit
//! IQ is a stream of its own: the tap sits beside the session's audio streams, not in front of
//! them, so receive audio and transmit audio go where they always went.
//!
//! Nexus's own design, not a port.

use std::collections::VecDeque;
use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::dto::{Spectrum, SpectrumScale};
use tempo_app::engine::{engine_lock, Engine, SpectrumFeed};
use tempo_core::spectrum::{iq_power, WindowN};
use tempo_net::tci::frame::{Block, BlockKind, Channels, IqRate};
use tempo_net::tci::session::{Connection, IqTap, Phase, SendError};
use tempo_net::tci::wire::{Command, Hz, StreamSetting};
use tempo_net::tci::Trx;

use super::{now_ms, TciDaemon};

/// The feed label for rows from this panadapter.
///
/// ⚠️ MUST match `isRfScopeSource` in `ui/src/waterfall.ts`. That predicate decides whether a row
/// spans absolute RF Hz or demodulated audio Hz, and an unknown label falls through to the audio
/// reading: the row is drawn against a 0–4000 Hz axis and nothing errors (`yaesu_wf::SOURCE` says
/// the same for the same reason).
pub const SOURCE: &str = "tci";

/// The analysis window: tempo-core's Balanced length, 42.7 ms at 48 kHz.
const WIN: WindowN = WindowN::Balanced;

/// Pairs (I and Q) in a window, and bins in a row: the Flex panadapter's row width.
pub const N: usize = WIN.n();

/// The most pairs held between two looks of the pan's thread: eight windows, 43 ms at 384 kHz.
pub const HELD: usize = 8 * N;

/// The most windows averaged into one row. At 384 kHz about twelve arrive between two rows; the
/// rest are skipped, which bounds the FFTs to this many a row whatever the server streams.
pub const MAX_WINDOWS: u32 = 8;

/// A row about every 66 ms: the Flex panadapter's 15 a second.
pub const ROW_MS: u64 = 66;

/// No IQ for this long while it runs: its source stopped. A block comes every 21 ms or so at 48 kHz.
pub const STALL_MS: u64 = 500;

/// After a stall or a gap, IQ is not drawn until the stream has run this long again: a backlog
/// the server or the network held back arrives in a burst first, and is late.
pub const SETTLE_MS: u64 = 250;

/// How far the stream's samples may fall behind the wall clock before they count as lost (a gap):
/// network jitter fits well inside it.
pub const BEHIND_MS: u64 = 250;

/// How long after `iq_start` the first IQ block may take before the server is taken not to offer
/// IQ.
pub const START_WINDOW_MS: u64 = 3_000;

/// How long the radio loop's word lasts. It renews it every tick (20 ms), and a CAT write that waits
/// for the server's readback (up to half a second) holds the loop about this long.
pub const WANT_TTL_MS: u64 = 500;

/// How long a word the radio loop stopped renewing (a radio switch) keeps the IQ stream up before
/// `iq_stop`: a loop stalled for a moment does not stop and start the stream.
pub const STOP_GRACE_MS: u64 = 2_000;

/// The dB axis rows are drawn on, below full scale (a full-scale complex tone is 0 dB): wider than
/// the audio axis's 120 dB, since an SDR's noise floor in a narrow bin can sit below −120 dBFS.
pub const LO_DB: f32 = -160.0;
/// The top of that axis: full scale.
pub const HI_DB: f32 = 0.0;

/// What a row's values mean, said on every frame.
pub(crate) const SCALE: SpectrumScale = SpectrumScale::Dbfs {
    lo_db: LO_DB,
    hi_db: HI_DB,
};

/// How long the pan's thread waits before it looks at the samples, the word and the clock again.
const POLL: Duration = Duration::from_millis(20);

/// How long teardown waits for the pan's thread before leaving it to finish by itself.
const JOIN_WAIT: Duration = Duration::from_millis(300);

/// The most log lines kept for the tests and the bench ([`Pan::said`]).
const SAID_KEPT: usize = 32;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The windows the pan runs on: the constants above, which tests shorten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Times {
    pub(crate) start_window_ms: u64,
    pub(crate) stall_ms: u64,
    pub(crate) want_ttl_ms: u64,
    pub(crate) stop_grace_ms: u64,
}

impl Default for Times {
    fn default() -> Times {
        Times {
            start_window_ms: START_WINDOW_MS,
            stall_ms: STALL_MS,
            want_ttl_ms: WANT_TTL_MS,
            stop_grace_ms: STOP_GRACE_MS,
        }
    }
}

/// Where a run of samples sits: the stream's rate and the receiver's centre frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    /// Hz: an IQ rate, which is also the span.
    pub rate: u32,
    /// Hz: the receiver's centre (`dds`).
    pub centre: i64,
}

impl Geometry {
    /// The row's edges in Hz: [`N`] bins across the rate, so that the middle of bin `j`
    /// (`lo + (j + ½)·rate/N`, as the scope reads a row) is the FFT's frequency for it, the
    /// centre plus `(j − N/2)·rate/N`.
    pub fn edges(self) -> (f64, f64) {
        let rate = f64::from(self.rate);
        let lo = self.centre as f64 - rate / 2.0 - rate / N as f64 / 2.0;
        (lo, lo + rate)
    }
}

/// What the pan did with the IQ blocks the session handed it. Each block counts once, in exactly
/// one of the first five; `gaps` and `overflow` count what happened to the stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Kept for drawing.
    pub blocks: u64,
    /// Not two channels at an IQ rate, or empty: dropped.
    pub malformed: u64,
    /// Dropped: the server had not reported the receiver's centre, so it could not be placed.
    pub unplaced: u64,
    /// Dropped as late: the block that showed a gap, and any within [`SETTLE_MS`] of a gap or a
    /// stall.
    pub late: u64,
    /// Arrived while the pan was not taking IQ (after its `iq_stop`).
    pub ignored: u64,
    /// The stream fell more than [`BEHIND_MS`] behind its clock: samples were lost.
    pub gaps: u64,
    /// Pairs dropped because the pan's thread fell behind.
    pub overflow: u64,
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} blocks kept, {} malformed, {} with no centre frequency, {} late, {} gaps, {} pairs \
             dropped behind",
            self.blocks, self.malformed, self.unplaced, self.late, self.gaps, self.overflow
        )
    }
}

/// When IQ last arrived since the pan's `iq_start`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Seen {
    /// Any IQ block of ours, whether it could be drawn or not.
    pub(crate) any: Option<u64>,
    /// The last block kept for drawing.
    pub(crate) kept: Option<u64>,
}

/// The stream's own clock: anchored at `at`, `pairs` kept since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamClock {
    at: u64,
    pairs: u64,
}

/// Milliseconds `pairs` last at `rate`.
fn duration_ms(pairs: usize, rate: u32) -> u64 {
    (pairs as u64).saturating_mul(1_000) / u64::from(rate.max(1))
}

/// Pairs due in `ms` at `rate`.
fn pairs_in(ms: u64, rate: u32) -> u64 {
    ms.saturating_mul(u64::from(rate)) / 1_000
}

/// What the pan's thread takes with the samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Taken {
    /// Where the samples sit.
    pub(crate) geometry: Option<Geometry>,
    /// The samples do not follow on from those taken before: a partial window is dropped.
    pub(crate) broken: bool,
    /// The windows summed for the next row are dropped too: a gap, a stall, a retune, a new rate.
    pub(crate) restart: bool,
}

/// The session thread's half: each IQ block checked, placed and kept, on a clock passed in.
pub(crate) struct Intake {
    /// Taking IQ: from the pan's `iq_start` to its `iq_stop`.
    accepting: bool,
    /// Pairs kept since the pan's thread last took them, I then Q, oldest first: at most [`HELD`]
    /// pairs, in a buffer allocated once at that size.
    samples: Vec<f32>,
    geometry: Option<Geometry>,
    clock: Option<StreamClock>,
    /// After a stall: nothing is kept before this time.
    settle_until: Option<u64>,
    seen: Seen,
    broken: bool,
    restart: bool,
    counts: Counts,
    stall_ms: u64,
}

impl Intake {
    pub(crate) fn new(stall_ms: u64) -> Intake {
        Intake {
            accepting: false,
            samples: Vec::with_capacity(2 * HELD),
            geometry: None,
            clock: None,
            settle_until: None,
            seen: Seen::default(),
            broken: true,
            restart: true,
            counts: Counts::default(),
            stall_ms,
        }
    }

    /// Start or stop taking IQ: either way the stream starts over.
    pub(crate) fn accept(&mut self, on: bool) {
        self.accepting = on;
        self.samples.clear();
        self.geometry = None;
        self.clock = None;
        self.settle_until = None;
        self.seen = Seen::default();
        self.broken = true;
        self.restart = true;
    }

    /// After a stall or a gap: nothing is kept for [`SETTLE_MS`], and the clock starts again after.
    fn settle(&mut self, now: u64) {
        self.settle_until = Some(now.saturating_add(SETTLE_MS));
        self.clock = None;
        self.samples.clear();
        self.broken = true;
        self.restart = true;
    }

    /// One IQ block of our transceiver, arrived at `now`, with the centre the server reported
    /// before it. Never sized by a header field: the block's samples are what its payload held.
    pub(crate) fn push(&mut self, block: &Block<'_>, centre: Option<Hz>, now: u64) {
        let header = block.header;
        let iq = header.kind == BlockKind::Iq
            && header.channels == Channels::Two
            && IqRate::new(header.sample_rate).is_some();
        self.push_pairs(
            iq.then_some(header.sample_rate),
            centre.map(Hz::get),
            block.frames(),
            block.samples(),
            now,
        );
    }

    /// `pairs` IQ pairs (`samples`, I then Q, full scale 1) arrived at `now`, at `rate` around
    /// `centre`, each as the source last reported them before the samples: any IQ source's
    /// way in. No rate (not IQ at a rate this source streams) or no pairs is malformed; no centre
    /// is unplaced.
    pub(crate) fn push_pairs(
        &mut self,
        rate: Option<u32>,
        centre: Option<i64>,
        pairs: usize,
        samples: impl Iterator<Item = f32>,
        now: u64,
    ) {
        if !self.accepting {
            self.counts.ignored += 1;
            return;
        }
        self.seen.any = Some(now);
        let Some(rate) = rate.filter(|r| *r > 0 && pairs > 0) else {
            self.counts.malformed += 1;
            return;
        };
        let Some(centre) = centre else {
            self.counts.unplaced += 1;
            return;
        };
        let geometry = Geometry { rate, centre };
        if self.geometry != Some(geometry) {
            // A retune or a new rate: what is held was taken somewhere else. A new rate also counts
            // the stream's time in other units, so its clock starts again at this block.
            if self.geometry.is_some_and(|g| g.rate != geometry.rate) {
                self.clock = Some(StreamClock {
                    at: now.saturating_sub(duration_ms(pairs, geometry.rate)),
                    pairs: 0,
                });
            }
            self.geometry = Some(geometry);
            self.samples.clear();
            self.broken = true;
            self.restart = true;
        }
        // A stall: nothing kept for the stall time. What comes first after one may be a backlog
        // the server or the network held back, arriving late.
        let stalled = self
            .seen
            .kept
            .is_some_and(|at| now.saturating_sub(at) >= self.stall_ms);
        if stalled && self.settle_until.is_none() {
            self.settle(now);
        }
        if let Some(until) = self.settle_until {
            if now < until {
                self.counts.late += 1;
                return;
            }
            self.settle_until = None;
        }
        let rate = geometry.rate;
        let clock = *self.clock.get_or_insert(StreamClock {
            at: now.saturating_sub(duration_ms(pairs, rate)),
            pairs: 0,
        });
        let due = pairs_in(now.saturating_sub(clock.at), rate);
        let have = clock.pairs.saturating_add(pairs as u64);
        if have.saturating_add(pairs_in(BEHIND_MS, rate)) < due {
            // Behind: samples were lost. Nothing held or summed spans the gap, and what follows it
            // may be a burst held back, as after a stall.
            self.counts.gaps += 1;
            self.counts.late += 1;
            self.settle(now);
            return;
        }
        if have > due {
            // Ahead of the clock: a server that sends early, in a burst, or whose clock runs fast.
            // The clock follows the stream, so a loss after it still reads as one.
            self.clock = Some(StreamClock {
                at: now.saturating_sub(duration_ms(pairs, rate)),
                pairs: 0,
            });
        }
        // Kept, the newest first when it cannot all be: the pan's thread has fallen behind.
        let keep = pairs.min(HELD);
        if self.samples.len() / 2 + keep > HELD {
            self.counts.overflow += (self.samples.len() / 2) as u64;
            self.samples.clear();
            self.broken = true;
        }
        self.counts.overflow += (pairs - keep) as u64;
        self.samples
            .extend(samples.skip(2 * (pairs - keep)).take(2 * keep));
        if let Some(c) = self.clock.as_mut() {
            c.pairs = c.pairs.saturating_add(pairs as u64);
        }
        self.seen.kept = Some(now);
        self.counts.blocks += 1;
    }

    /// When IQ last arrived, and was last kept.
    pub(crate) fn seen(&self) -> Seen {
        self.seen
    }

    /// What was done with the IQ so far.
    pub(crate) fn counts(&self) -> Counts {
        self.counts
    }

    /// Where the samples held sit.
    pub(crate) fn geometry(&self) -> Option<Geometry> {
        self.geometry
    }

    /// Hand the samples kept so far to the pan's thread, through `into`: the two buffers swap, so
    /// neither ever grows past its first allocation.
    pub(crate) fn take(&mut self, into: &mut Vec<f32>) -> Taken {
        into.clear();
        std::mem::swap(&mut self.samples, into);
        let taken = Taken {
            geometry: self.geometry,
            broken: self.broken,
            restart: self.restart,
        };
        self.broken = false;
        self.restart = false;
        taken
    }
}

/// A row value from a power relative to full scale: linear in dB from [`LO_DB`] (0) to [`HI_DB`]
/// (1).
fn row_value(power: f32) -> f32 {
    let db = 10.0 * power.max(1e-30).log10();
    ((db - LO_DB) / (HI_DB - LO_DB)).clamp(0.0, 1.0)
}

/// The pan thread's half: windows, their power, and the rows, on a clock passed in.
pub(crate) struct Rows {
    /// The feed label the rows carry: [`SOURCE`] for TCI's.
    source: &'static str,
    geometry: Option<Geometry>,
    /// The pairs of a window not yet whole, I then Q.
    rest: Vec<f32>,
    power: Vec<f32>,
    /// Each bin's power summed over this row's windows.
    sum: Vec<f32>,
    windows: u32,
    /// Windows past [`MAX_WINDOWS`] in a row, not computed.
    pub(crate) skipped: u64,
    /// When the next row is due: [`ROW_MS`] after the last was due, so rows keep their rate on
    /// the pan thread's 20 ms looks.
    next_row: Option<u64>,
}

impl Default for Rows {
    fn default() -> Rows {
        Rows {
            source: SOURCE,
            geometry: None,
            rest: Vec::with_capacity(2 * N),
            power: vec![0.0; N],
            sum: vec![0.0; N],
            windows: 0,
            skipped: 0,
            next_row: None,
        }
    }
}

impl Rows {
    /// Rows labelled `source` in the feed (the UI reads the label as an RF scope source).
    pub(crate) fn labelled(source: &'static str) -> Rows {
        Rows {
            source,
            ..Rows::default()
        }
    }

    /// Drop what is summed and the partial window: the next row starts from what comes next.
    pub(crate) fn restart(&mut self) {
        self.rest.clear();
        self.sum.fill(0.0);
        self.windows = 0;
        self.next_row = None;
    }

    /// Cut `samples` (what `taken` describes) into windows and sum each window's power.
    pub(crate) fn feed(&mut self, taken: &Taken, samples: &[f32]) {
        if taken.restart || taken.geometry != self.geometry {
            self.restart();
            self.geometry = taken.geometry;
        } else if taken.broken {
            self.rest.clear();
        }
        let mut samples = samples;
        if !self.rest.is_empty() {
            let need = (2 * N - self.rest.len()).min(samples.len());
            self.rest.extend_from_slice(&samples[..need]);
            samples = &samples[need..];
            if self.rest.len() == 2 * N {
                let window = std::mem::take(&mut self.rest);
                self.window(&window);
                self.rest = window;
                self.rest.clear();
            }
        }
        let mut windows = samples.chunks_exact(2 * N);
        for window in windows.by_ref() {
            self.window(window);
        }
        self.rest.extend_from_slice(windows.remainder());
    }

    fn window(&mut self, iq: &[f32]) {
        if self.windows >= MAX_WINDOWS {
            self.skipped += 1;
            return;
        }
        if iq_power(iq, WIN, &mut self.power) {
            for (s, p) in self.sum.iter_mut().zip(&self.power) {
                *s += p;
            }
            self.windows += 1;
        }
    }

    /// The next row, once it is due and a window has arrived since the last: the windows' mean
    /// power per bin, on the dB axis. Never the same windows twice.
    pub(crate) fn row(&mut self, now: u64) -> Option<Spectrum> {
        let geometry = self.geometry?;
        let due = self.next_row.unwrap_or(now);
        if self.windows == 0 || now < due {
            return None;
        }
        let n = self.windows as f32;
        let row = self.sum.iter().map(|p| row_value(p / n)).collect();
        self.sum.fill(0.0);
        self.windows = 0;
        self.next_row = Some(due.saturating_add(ROW_MS).max(now));
        let (lo_hz, hi_hz) = geometry.edges();
        Some(Spectrum {
            row,
            lo_hz,
            hi_hz,
            source: self.source.to_string(),
        })
    }
}

/// Where the pan's IQ stream is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stream {
    /// Not asked for.
    #[default]
    Idle,
    /// `iq_start` went out at `since`; no IQ yet.
    Starting { since: u64 },
    /// IQ has arrived.
    Running,
    /// The server sent no IQ in the window after `iq_start`: it does not offer it. Asked again only
    /// after the switch is turned off and on, or on a new session.
    Refused,
}

/// The radio loop's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    /// Renewed within its time: the active radio's client, with the switch on.
    On,
    /// Not renewed since `since`: a radio switch left this client behind (or the loop stalled).
    Lapsed { since: u64 },
    /// The switch is off.
    Off,
}

/// What the pan's thread does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Act {
    /// Take IQ from now on, and send `iq_start`.
    Start,
    /// Send `iq_stop`, and take no more.
    Stop,
    /// Empty the feed's RF slot: the rows there are no longer the band.
    Clear,
    /// One line for the log: whether it is a warning, and its words.
    Say(bool, String),
}

/// What the pan's thread knows at a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Inputs {
    pub(crate) now: u64,
    pub(crate) want: Want,
    /// The session has said `ready;` and is not closing.
    pub(crate) ready: bool,
    /// A transmission is on the air (ours or another program's): the receiver may pause its IQ.
    pub(crate) transmitting: bool,
    pub(crate) seen: Seen,
    pub(crate) counts: Counts,
    pub(crate) geometry: Option<Geometry>,
}

/// When the pan starts and stops its IQ, draws, and says so: a pure step over [`Inputs`].
#[derive(Debug, Default)]
pub(crate) struct Control {
    pub(crate) stream: Stream,
    /// No IQ kept for the stall time while running.
    pub(crate) stalled: bool,
    /// Rows may be published: the switch is on, IQ is running and not stalled.
    pub(crate) drawing: bool,
    /// When the stream began running, or a transmission last ended: a stall is counted from the
    /// latest of these and the last block kept.
    quiet_from: u64,
}

impl Control {
    pub(crate) fn step(&mut self, i: &Inputs, t: &Times) -> Vec<Act> {
        let mut acts = Vec::new();
        let running = matches!(self.stream, Stream::Starting { .. } | Stream::Running);
        match i.want {
            Want::Off => {
                if running {
                    acts.push(Act::Stop);
                    acts.push(Act::Say(
                        false,
                        format!(
                            "the panadapter is switched off: iq_stop sent ({})",
                            i.counts
                        ),
                    ));
                }
                // The operator's off clears a refusal too: on again asks again.
                self.stream = Stream::Idle;
                self.stalled = false;
            }
            Want::Lapsed { since } => {
                if running && i.now.saturating_sub(since) >= t.stop_grace_ms {
                    acts.push(Act::Stop);
                    acts.push(Act::Say(
                        false,
                        format!(
                            "this radio's panadapter is no longer drawn (another radio is on \
                             screen): iq_stop sent ({})",
                            i.counts
                        ),
                    ));
                    self.stream = Stream::Idle;
                    self.stalled = false;
                }
            }
            Want::On => match self.stream {
                Stream::Idle if i.ready => {
                    acts.push(Act::Start);
                    self.stream = Stream::Starting { since: i.now };
                }
                Stream::Idle | Stream::Refused => {}
                Stream::Starting { since } => {
                    if i.seen.any.is_some() {
                        self.stream = Stream::Running;
                        self.stalled = false;
                        self.quiet_from = i.now;
                        let at = i.geometry.map_or(String::new(), |g| {
                            format!(" at {} Hz around {} Hz", g.rate, g.centre)
                        });
                        acts.push(Act::Say(
                            false,
                            format!("the SDR program's IQ is flowing{at}: the panadapter draws it"),
                        ));
                    } else if i.now.saturating_sub(since) >= t.start_window_ms {
                        acts.push(Act::Stop);
                        acts.push(Act::Say(
                            true,
                            format!(
                                "the SDR program sent no IQ within {:.1} s of iq_start, so its \
                                 TCI server does not offer the panadapter (piHPSDR's has no \
                                 iq_start). The scope keeps its previous source; turn the \
                                 panadapter switch off and on to ask again.",
                                t.start_window_ms as f64 / 1_000.0
                            ),
                        ));
                        self.stream = Stream::Refused;
                    }
                }
                // Under a transmission a receiver may pause its IQ: neither a stall nor its end is
                // read from it, and a stall is counted from the transmission's end.
                Stream::Running if i.transmitting => self.quiet_from = i.now,
                Stream::Running => {
                    let last = i.seen.kept.unwrap_or(0).max(self.quiet_from);
                    let quiet = i.now.saturating_sub(last) >= t.stall_ms;
                    if quiet && !self.stalled {
                        self.stalled = true;
                        let why = if i.seen.any.is_some_and(|at| at > last) {
                            format!("its IQ cannot be drawn ({})", i.counts)
                        } else {
                            "it stopped sending its IQ".to_string()
                        };
                        acts.push(Act::Say(
                            true,
                            format!(
                                "the SDR program's panadapter stopped: {why}. The scope says its \
                                 source stopped."
                            ),
                        ));
                    } else if !quiet && self.stalled {
                        self.stalled = false;
                        acts.push(Act::Say(
                            false,
                            "the SDR program's IQ is flowing again".to_string(),
                        ));
                    }
                }
            },
        }
        let drawing = i.want == Want::On && self.stream == Stream::Running && !self.stalled;
        // Rows of ours left in the feed are not the band once the switch is off or the IQ has
        // stopped. A lapsed word leaves them: another radio's source may own the slot by now, and
        // ours, no longer renewed, age out by themselves.
        if self.drawing && !drawing && !matches!(i.want, Want::Lapsed { .. }) {
            acts.push(Act::Clear);
        }
        self.drawing = drawing;
        acts
    }
}

/// What the pan says about itself, for the bench and the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Status {
    pub stream: Stream,
    pub drawing: bool,
    pub stalled: bool,
    /// Rows published.
    pub rows: u64,
    /// Windows past [`MAX_WINDOWS`] in a row, not computed.
    pub skipped: u64,
    pub counts: Counts,
    pub geometry: Option<Geometry>,
}

/// What the session's thread, the pan's thread and the radio loop share.
pub(crate) struct Shared {
    /// The radio loop's word: wanted until this time on [`now_ms`]'s clock; 0, switched off.
    want_until: AtomicU64,
    pub(crate) intake: Mutex<Intake>,
    /// Wakes the pan's thread: its teardown.
    wake: Condvar,
    quit: AtomicBool,
    times: Mutex<Times>,
    status: Mutex<Status>,
    said: Mutex<VecDeque<String>>,
    /// The thread rows were built on.
    #[cfg(test)]
    pub(crate) built_on: Mutex<Option<String>>,
}

impl Shared {
    fn new(times: Times) -> Shared {
        Shared {
            want_until: AtomicU64::new(0),
            intake: Mutex::new(Intake::new(times.stall_ms)),
            wake: Condvar::new(),
            quit: AtomicBool::new(false),
            times: Mutex::new(times),
            status: Mutex::new(Status::default()),
            said: Mutex::new(VecDeque::new()),
            #[cfg(test)]
            built_on: Mutex::new(None),
        }
    }

    fn want(&self, now: u64) -> Want {
        match self.want_until.load(Ordering::Acquire) {
            0 => Want::Off,
            until if now < until => Want::On,
            until => Want::Lapsed { since: until },
        }
    }

    fn say(&self, server: SocketAddr, warning: bool, words: &str) {
        let line = format!("TCI ({server}): {words}");
        if warning {
            tempo_core::applog::warn("cat", &line);
        } else {
            tempo_core::applog::info("cat", &line);
        }
        let mut said = lock(&self.said);
        if said.len() >= SAID_KEPT {
            said.pop_front();
        }
        said.push_back(words.to_string());
    }
}

impl IqTap for Shared {
    fn iq(&self, block: &Block<'_>, centre: Option<Hz>) {
        lock(&self.intake).push(block, centre, now_ms());
    }
}

/// The pan's thread, once started.
enum Worker {
    NotStarted,
    Running(JoinHandle<()>),
    /// It could not be started: not tried again for this client.
    Failed,
}

/// A TCI client's panadapter: the radio loop's word ([`TciDaemon::want_pan`]), the tap the
/// session hands IQ to, and the thread that draws it, started the first time the switch is on.
pub struct Pan {
    shared: Arc<Shared>,
    worker: Mutex<Worker>,
}

impl Default for Pan {
    fn default() -> Pan {
        Pan {
            shared: Arc::new(Shared::new(Times::default())),
            worker: Mutex::new(Worker::NotStarted),
        }
    }
}

impl Pan {
    /// Where the pan is: its stream, whether it draws, and what it did with the IQ.
    pub fn status(&self) -> Status {
        *lock(&self.shared.status)
    }

    /// What the pan has said in the log, oldest first (the last few).
    pub fn said(&self) -> Vec<String> {
        lock(&self.shared.said).iter().cloned().collect()
    }

    /// Shorter windows, for the tests. Before the pan's thread starts.
    #[cfg(test)]
    pub(crate) fn set_times(&self, times: Times) {
        *lock(&self.shared.times) = times;
        lock(&self.shared.intake).stall_ms = times.stall_ms;
    }

    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    fn want(
        &self,
        on: bool,
        feed: &SpectrumFeed,
        conn: &Arc<Connection>,
        server: SocketAddr,
        trx: Trx,
    ) {
        let ttl = lock(&self.shared.times).want_ttl_ms;
        let until = if on {
            now_ms().saturating_add(ttl).max(1)
        } else {
            0
        };
        self.shared.want_until.store(until, Ordering::Release);
        if !on {
            return;
        }
        let mut worker = lock(&self.worker);
        if !matches!(*worker, Worker::NotStarted) {
            return;
        }
        let (shared, conn, feed) = (self.shared.clone(), Arc::downgrade(conn), feed.clone());
        *worker = match std::thread::Builder::new()
            .name("tci-pan".into())
            .spawn(move || draw(&shared, &conn, &feed, server, trx))
        {
            Ok(handle) => Worker::Running(handle),
            Err(e) => {
                tempo_core::applog::warn(
                    "cat",
                    &format!("TCI ({server}): the panadapter could not start its thread ({e})"),
                );
                Worker::Failed
            }
        };
    }
}

impl Drop for Pan {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        self.shared.wake.notify_all();
        let worker = std::mem::replace(&mut *lock(&self.worker), Worker::Failed);
        if let Worker::Running(handle) = worker {
            // It looks at its quit flag every POLL; one that does not finish in time (a session
            // write in flight) finishes by itself and is not waited for.
            let deadline = Instant::now() + JOIN_WAIT;
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

impl TciDaemon {
    /// The radio loop's word on this client's panadapter, each tick ([`reconcile`]): draw the SDR
    /// program's IQ while `on`. The first `on` starts the pan's thread; nothing here waits on the
    /// session or touches a sample.
    pub fn want_pan(&self, on: bool, feed: &SpectrumFeed) {
        if let Some(conn) = self.conn.as_ref() {
            self.pan.want(on, feed, conn, self.server, self.trx);
        }
    }

    /// The panadapter: its status and what it has said.
    pub fn pan(&self) -> &Pan {
        &self.pan
    }
}

/// The radio loop's tick for the panadapter of the active radio's TCI client, if it has one: read
/// its switch and renew the client's word. A client no longer passed here (a radio switch, which
/// moves it to the monitor pool) is not renewed, so its word lapses by itself.
pub fn reconcile(daemon: Option<&TciDaemon>, engine: &Arc<Mutex<Engine>>, feed: &SpectrumFeed) {
    let Some(daemon) = daemon else {
        return;
    };
    let on = engine_lock(engine).settings().tci_pan;
    daemon.want_pan(on, feed);
}

/// The pan's thread: the tap first, then a step every [`POLL`] until the client goes.
fn draw(
    shared: &Arc<Shared>,
    conn: &Weak<Connection>,
    feed: &SpectrumFeed,
    server: SocketAddr,
    trx: Trx,
) {
    let tap: Arc<dyn IqTap> = shared.clone();
    if !conn.upgrade().is_some_and(|c| c.set_iq_tap(Some(tap))) {
        return;
    }
    let times = *lock(&shared.times);
    let slice = u8::try_from(trx.get()).ok();
    let mut control = Control::default();
    let mut rows = Rows::default();
    let mut samples: Vec<f32> = Vec::with_capacity(2 * HELD);
    loop {
        {
            let intake = lock(&shared.intake);
            if !shared.quit.load(Ordering::Acquire) {
                drop(shared.wake.wait_timeout(intake, POLL));
            }
        }
        if shared.quit.load(Ordering::Acquire) {
            break;
        }
        let Some(c) = conn.upgrade() else {
            break;
        };
        let now = now_ms();
        let (taken, seen, counts, geometry) = {
            let mut intake = lock(&shared.intake);
            let taken = intake.take(&mut samples);
            (taken, intake.seen, intake.counts, intake.geometry)
        };
        let inputs = Inputs {
            now,
            want: shared.want(now),
            ready: c.read(|s| s.phase == Phase::Ready),
            transmitting: c.ours_on_air() || c.read(|s| s.foreign),
            seen,
            counts,
            geometry,
        };
        let mut gone = false;
        for act in control.step(&inputs, &times) {
            match act {
                Act::Start => {
                    lock(&shared.intake).accept(true);
                    match c.send(Command::Stream(StreamSetting::IqStart(trx))) {
                        Ok(()) => {}
                        // Not ready after all (closing): asked again once it is, if ever.
                        Err(SendError::NotReady | SendError::AudioInUse) => {
                            lock(&shared.intake).accept(false);
                            control.stream = Stream::Idle;
                        }
                        Err(SendError::Closed) => gone = true,
                    }
                }
                Act::Stop => {
                    lock(&shared.intake).accept(false);
                    let _ = c.send(Command::Stream(StreamSetting::IqStop(trx)));
                }
                Act::Clear => feed.clear_rf(),
                Act::Say(warning, words) => shared.say(server, warning, &words),
            }
        }
        drop(c);
        if gone {
            break;
        }
        if control.drawing {
            rows.feed(&taken, &samples);
            if let Some(row) = rows.row(now) {
                feed.publish_rf_frame(row, SCALE, slice);
                lock(&shared.status).rows += 1;
                #[cfg(test)]
                lock(&shared.built_on)
                    .get_or_insert_with(|| std::thread::current().name().unwrap_or("").into());
            }
        } else {
            rows.restart();
        }
        let mut status = lock(&shared.status);
        status.stream = control.stream;
        status.drawing = control.drawing;
        status.stalled = control.stalled;
        status.skipped = rows.skipped;
        status.counts = counts;
        status.geometry = geometry;
    }
    // The client is going: rows of ours left in the feed are no longer the band.
    if control.drawing {
        feed.clear_rf();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::{registry, Options, Target};
    use tempo_app::settings::TciDialect;
    use tempo_net::tci::frame::{
        decode, encode_block, BlockHeader, Frame, LengthMeaning, SampleFormat, HEADER_LEN,
        MAX_FRAME,
    };
    use tempo_tcisim::{
        Carrier, Closer, Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Keyer,
        Receive as SimReceive, Simulator, Source as SimSource,
    };

    /// The longest any one expected thing may take here.
    const WAIT: Duration = Duration::from_secs(10);

    /// A tone's level in the tests: about −30 dB below full scale.
    const AMP: f32 = 0.03;

    fn trx0() -> Trx {
        Trx::new(0).expect("trx 0")
    }

    /// One IQ block of `samples` (I then Q, or one channel) at `rate`, as a server sends it.
    fn iq_bytes(rate: u32, channels: Channels, samples: &[f32]) -> Vec<u8> {
        let header = BlockHeader {
            trx: trx0(),
            kind: BlockKind::Iq,
            sample_rate: rate,
            format: SampleFormat::Float32,
            channels,
        };
        encode_block(&header, LengthMeaning::Values, samples).expect("an IQ block")
    }

    fn block(bytes: &[u8]) -> Block<'_> {
        match decode(bytes) {
            Ok(Frame::Block(b)) => b,
            other => panic!("not a block: {other:?}"),
        }
    }

    /// Pairs `first..first + pairs` of tones `offsets` Hz from the centre, each at [`AMP`]:
    /// `e^{+j·2π·f·t}`, I the cosine and Q the sine, so a positive offset is above the centre.
    fn tones(offsets: &[f64], rate: u32, first: u64, pairs: usize) -> Vec<f32> {
        (0..pairs)
            .flat_map(|i| {
                let t = (first + i as u64) as f64 / f64::from(rate);
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for f in offsets {
                    let ph = std::f64::consts::TAU * f * t;
                    re += ph.cos();
                    im += ph.sin();
                }
                let a = f64::from(AMP);
                [(re * a) as f32, (im * a) as f32]
            })
            .collect()
    }

    /// The pan's two halves on a manual clock, as the session's thread and the pan's thread run
    /// them: a block of 20 ms every 20 ms, and the pan's look straight after it.
    struct Line {
        intake: Intake,
        rows: Rows,
        buf: Vec<f32>,
        now: u64,
        /// Pairs of the stream so far, so each tone runs on without a phase jump.
        sent: u64,
        drawn: Vec<Spectrum>,
    }

    impl Line {
        fn new() -> Line {
            let mut intake = Intake::new(STALL_MS);
            intake.accept(true);
            Line {
                intake,
                rows: Rows::default(),
                buf: Vec::with_capacity(2 * HELD),
                now: 10_000,
                sent: 0,
                drawn: Vec::new(),
            }
        }

        /// One look of the pan's thread.
        fn look(&mut self) {
            let taken = self.intake.take(&mut self.buf);
            self.rows.feed(&taken, &self.buf);
            if let Some(row) = self.rows.row(self.now) {
                self.drawn.push(row);
            }
        }

        fn push(&mut self, rate: u32, centre: i64, offsets: &[f64]) {
            let pairs = (rate / 50) as usize;
            let bytes = iq_bytes(rate, Channels::Two, &tones(offsets, rate, self.sent, pairs));
            self.intake.push(&block(&bytes), Hz::new(centre), self.now);
            self.sent += pairs as u64;
        }

        /// `ms` of IQ in step with the clock, centred on `centre`.
        fn stream(&mut self, ms: u64, rate: u32, centre: i64, offsets: &[f64]) {
            for _ in 0..ms / 20 {
                self.now += 20;
                self.push(rate, centre, offsets);
                self.look();
            }
        }

        /// `ms` with no IQ: those samples never arrive.
        fn silence(&mut self, ms: u64, rate: u32) {
            for _ in 0..ms / 20 {
                self.now += 20;
                self.sent += u64::from(rate / 50);
                self.look();
            }
        }

        /// `blocks` blocks arriving at once: a backlog the server or the network held back.
        fn burst(&mut self, blocks: usize, rate: u32, centre: i64, offsets: &[f64]) {
            for _ in 0..blocks {
                self.push(rate, centre, offsets);
            }
            self.look();
        }

        fn rows(&mut self) -> Vec<Spectrum> {
            std::mem::take(&mut self.drawn)
        }
    }

    /// The RF frequency the scope puts the middle of bin `j` at (`tuneSnap.ts`: `lo + (j + ½)·w`).
    fn bin_hz(row: &Spectrum, j: usize) -> f64 {
        row.lo_hz + (j as f64 + 0.5) * (row.hi_hz - row.lo_hz) / row.row.len() as f64
    }

    /// The bin the scope draws `hz` in.
    fn bin_of(row: &Spectrum, hz: f64) -> usize {
        ((hz - row.lo_hz) / (row.hi_hz - row.lo_hz) * row.row.len() as f64) as usize
    }

    /// A row value in dB below full scale.
    fn db(v: f32) -> f32 {
        LO_DB + v * (HI_DB - LO_DB)
    }

    /// Whether the row holds a signal at `hz`: within 20 dB of a tone at [`AMP`].
    fn heard(row: &Spectrum, hz: f64) -> bool {
        db(row.row[bin_of(row, hz)]) > -50.0
    }

    /// The loudest bin within `bins` of `hz`, which must be a signal.
    fn peak_near(row: &Spectrum, hz: f64, bins: usize) -> usize {
        let at = bin_of(row, hz);
        let j = (at.saturating_sub(bins)..=(at + bins).min(row.row.len() - 1))
            .max_by(|a, b| row.row[*a].total_cmp(&row.row[*b]))
            .expect("bins");
        assert!(
            db(row.row[j]) > -50.0,
            "nothing near {hz} Hz: the loudest bin there reads {} dB",
            db(row.row[j])
        );
        j
    }

    // ── Where a row is ───────────────────────────────────────────────────────────────────────

    /// ⭐ RIGHT BIN, BOTH SIDES: each carrier lands in the bin whose middle the scope puts at its RF
    /// frequency, so its offset from the dial is its own: one below the dial (where LSB listens),
    /// one above it (USB), one above the receiver's centre. The IQ panadapter is the RF spectrum,
    /// the same in every mode; the mode only decides which side of the dial is heard (the
    /// simulator test flips the program to LSB).
    #[test]
    fn a_carrier_lands_in_its_own_bin_at_its_offset_from_the_dial() {
        let (centre, dial) = (14_080_000i64, 14_074_000i64);
        // On the 48 kHz / 2048-bin grid (23.4375 Hz): −320, −192 and +256 bins from the centre.
        let carriers = [14_072_500i64, 14_075_500, 14_086_000];
        let offsets: Vec<f64> = carriers.iter().map(|c| (c - centre) as f64).collect();
        let mut line = Line::new();
        line.stream(600, 48_000, centre, &offsets);
        let rows = line.rows();
        assert!(rows.len() >= 6, "{} rows in 600 ms", rows.len());
        for row in &rows {
            assert_eq!((row.row.len(), row.source.as_str()), (N, SOURCE));
            assert!(
                (row.hi_hz - row.lo_hz - 48_000.0).abs() < 1e-6,
                "the span is the IQ rate"
            );
            for &c in &carriers {
                let j = peak_near(row, c as f64, 8);
                let at = bin_hz(row, j);
                assert!((at - c as f64).abs() < 0.01, "{c} Hz drawn at {at} Hz");
                assert_eq!(
                    (at - dial as f64).round() as i64,
                    c - dial,
                    "{c} Hz: its offset from the dial"
                );
                assert!(
                    (db(row.row[j]) - 20.0 * AMP.log10()).abs() < 1.0,
                    "{c} Hz reads {} dB",
                    db(row.row[j])
                );
            }
        }
    }

    /// ⭐ A RETUNE NEVER DRAWS SAMPLES AT A CENTRE THEY WERE NOT TAKEN AT: the receiver's centre
    /// moves under a fixed carrier four times, and every row draws the carrier at its one RF
    /// frequency.
    #[test]
    fn a_retune_never_draws_samples_at_a_centre_they_were_not_taken_at() {
        let carrier = 14_074_750i64;
        let mut line = Line::new();
        for centre in [14_080_000i64, 14_077_000, 14_083_000, 14_071_000] {
            line.stream(300, 48_000, centre, &[(carrier - centre) as f64]);
        }
        let rows = line.rows();
        assert!(rows.len() >= 12, "{} rows", rows.len());
        for row in &rows {
            let j = (0..N)
                .max_by(|a, b| row.row[*a].total_cmp(&row.row[*b]))
                .unwrap();
            let at = bin_hz(row, j);
            assert!(
                (at - carrier as f64).abs() < 0.01,
                "the carrier drawn at {at} Hz (row {} to {} Hz)",
                row.lo_hz,
                row.hi_hz
            );
        }
    }

    /// The edges put the middle of bin `j` on the FFT's frequency for it, at every IQ rate.
    #[test]
    fn the_edges_put_each_bins_middle_on_its_frequency() {
        for rate in IqRate::HZ {
            let g = Geometry {
                rate,
                centre: 7_074_000,
            };
            let (lo, hi) = g.edges();
            let w = (hi - lo) / N as f64;
            assert!((w - f64::from(rate) / N as f64).abs() < 1e-9);
            for j in [0, 1, N / 2, N - 1] {
                let want = 7_074_000.0 + (j as f64 - (N / 2) as f64) * f64::from(rate) / N as f64;
                assert!(
                    (lo + (j as f64 + 0.5) * w - want).abs() < 1e-6,
                    "{rate} Hz, bin {j}"
                );
            }
        }
    }

    // ── No stale rows ────────────────────────────────────────────────────────────────────────

    /// ⭐ A LOST STRETCH OF IQ SHOWS IN NO ROW ON EITHER SIDE OF IT. The control first: with nothing
    /// lost, a row spans the change from tone A to tone B and hears both, so the check can see a
    /// mixed row. Then 300 ms of IQ never arrives (a server that dropped frames): no row after the
    /// gap holds anything from before it.
    #[test]
    fn a_lost_stretch_of_iq_shows_in_no_row_on_either_side_of_it() {
        let (centre, a, b) = (14_080_000i64, 14_086_000.0, 14_072_500.0);
        let (oa, ob) = (a - centre as f64, b - centre as f64);
        // 320 ms: the change falls mid-window.
        let mut line = Line::new();
        line.stream(320, 48_000, centre, &[oa]);
        line.stream(320, 48_000, centre, &[ob]);
        assert!(
            line.rows().iter().any(|r| heard(r, a) && heard(r, b)),
            "control: with nothing lost, a row spans the change"
        );
        let mut line = Line::new();
        line.stream(320, 48_000, centre, &[oa]);
        line.silence(300, 48_000);
        // Up to the gap, A alone: the last row of it comes at its own time, inside the gap.
        assert!(line.rows().iter().all(|r| heard(r, a) && !heard(r, b)));
        line.stream(600, 48_000, centre, &[ob]);
        let after = line.rows();
        assert!(
            after.len() >= 3,
            "the stream is drawn again: {} rows",
            after.len()
        );
        for row in &after {
            assert!(
                !heard(row, a),
                "a row after the gap holds IQ from before it"
            );
            assert!(heard(row, b));
        }
        assert_eq!(line.intake.counts.gaps, 1);
    }

    /// ⭐ A LOSS AFTER A BURST IS SEEN ALL THE SAME: a server that sent 600 ms early, in one burst,
    /// ran ahead of the clock; the clock follows it, so a 300 ms loss later still reads as a gap, and
    /// no row spans it.
    #[test]
    fn a_loss_after_a_burst_is_seen_all_the_same() {
        let (centre, a, b) = (14_080_000i64, 14_086_000.0, 14_072_500.0);
        let (oa, ob) = (a - centre as f64, b - centre as f64);
        let mut line = Line::new();
        line.stream(300, 48_000, centre, &[oa]);
        line.burst(30, 48_000, centre, &[oa]);
        line.stream(320, 48_000, centre, &[oa]);
        line.silence(300, 48_000);
        assert!(
            line.rows().iter().all(|r| !heard(r, b)),
            "up to the loss, A alone"
        );
        line.stream(600, 48_000, centre, &[ob]);
        assert_eq!(
            line.intake.counts.gaps, 1,
            "the loss is seen: {}",
            line.intake.counts
        );
        let after = line.rows();
        assert!(after.len() >= 3, "{} rows", after.len());
        for row in &after {
            assert!(!heard(row, a), "a row spans the loss");
            assert!(heard(row, b));
        }
    }

    /// ⭐ A BACKLOG AFTER A STALL IS LATE AND SHOWS IN NO ROW: the stream stops for 700 ms, the half
    /// second of tone A held back arrives at once, then the live stream is tone B. Nothing of A is
    /// drawn after the stall; B is, once the stream has run a little.
    #[test]
    fn a_backlog_after_a_stall_is_late_and_shows_in_no_row() {
        let (centre, a, b) = (14_080_000i64, 14_086_000.0, 14_072_500.0);
        let (oa, ob) = (a - centre as f64, b - centre as f64);
        let mut line = Line::new();
        line.stream(300, 48_000, centre, &[oa]);
        // The windows already summed make one last row at its time; after it, nothing.
        line.silence(ROW_MS + 20, 48_000);
        assert!(!line.rows().is_empty());
        line.silence(700, 48_000);
        assert!(line.rows().is_empty(), "nothing is drawn from nothing");
        line.burst(25, 48_000, centre, &[oa]);
        line.stream(600, 48_000, centre, &[ob]);
        let after = line.rows();
        assert!(
            after.len() >= 3,
            "the live stream is drawn: {} rows",
            after.len()
        );
        for row in &after {
            assert!(!heard(row, a), "a row drew the backlog");
            assert!(heard(row, b));
        }
        assert!(line.intake.counts.late >= 25, "{}", line.intake.counts);
    }

    /// ⭐ A ROW IS PUBLISHED ONCE, AND ONLY FROM WHAT IS NEW: about 15 a second while IQ arrives,
    /// none at all once it stops, however long the pan keeps looking.
    #[test]
    fn a_row_is_built_once_and_only_from_iq_that_arrived_since_the_last() {
        let mut line = Line::new();
        line.stream(1_000, 48_000, 14_080_000, &[6_000.0]);
        line.silence(ROW_MS + 20, 48_000);
        let n = line.rows().len();
        assert!((13..=16).contains(&n), "{n} rows in a second");
        line.silence(2_000, 48_000);
        assert!(line.rows().is_empty(), "no IQ, no row");
    }

    /// The work is bounded whatever the rate: at 384 kHz at most [`MAX_WINDOWS`] windows go into a
    /// row and the rest are skipped, not computed.
    #[test]
    fn the_ffts_a_row_costs_are_bounded_at_the_fastest_rate() {
        let mut line = Line::new();
        line.stream(500, 384_000, 14_080_000, &[60_000.0]);
        let rows = line.rows();
        assert!(rows.len() >= 6, "{} rows", rows.len());
        assert!(line.rows.skipped > 0, "windows past the bound were skipped");
        for row in &rows {
            assert!((row.hi_hz - row.lo_hz - 384_000.0).abs() < 1e-6);
            let j = peak_near(row, 14_140_000.0, 4);
            assert!((bin_hz(row, j) - 14_140_000.0).abs() < 0.01);
        }
    }

    // ── Bounds ───────────────────────────────────────────────────────────────────────────────

    /// ⭐ A MALFORMED IQ BLOCK IS DROPPED AND COUNTED, AND NOTHING GROWS PAST ITS BOUND: one
    /// channel, no samples, a block that is not IQ, no centre to place it on, a block before the
    /// pan's `iq_start`; and ten of the largest blocks a frame can carry with nobody taking them.
    /// Every block counts once.
    #[test]
    fn a_malformed_block_is_dropped_and_counted_and_nothing_grows_past_its_bound() {
        let mut intake = Intake::new(STALL_MS);
        let centre = Hz::new(14_080_000);
        let pair = iq_bytes(48_000, Channels::Two, &[0.1, 0.1]);
        intake.push(&block(&pair), centre, 1);
        assert_eq!(intake.counts.ignored, 1, "before iq_start");
        intake.accept(true);
        let mono = iq_bytes(48_000, Channels::One, &[0.1; 64]);
        let empty = iq_bytes(48_000, Channels::Two, &[]);
        let audio = encode_block(
            &BlockHeader {
                trx: trx0(),
                kind: BlockKind::RxAudio,
                sample_rate: 48_000,
                format: SampleFormat::Float32,
                channels: Channels::Two,
            },
            LengthMeaning::Values,
            &[0.1; 64],
        )
        .unwrap();
        for bytes in [&mono, &empty, &audio] {
            intake.push(&block(bytes), centre, 2);
        }
        assert_eq!(intake.counts.malformed, 3);
        intake.push(&block(&pair), None, 3);
        assert_eq!(intake.counts.unplaced, 1);
        assert!(intake.samples.is_empty(), "nothing kept");
        // The largest float32 IQ block: 64 KiB of payload, 8192 pairs.
        let big = iq_bytes(48_000, Channels::Two, &vec![0.01; 2 * 8_192]);
        for _ in 0..10 {
            intake.push(&block(&big), centre, 4);
        }
        assert!(intake.samples.len() <= 2 * HELD);
        assert_eq!(intake.samples.capacity(), 2 * HELD, "never grown");
        assert!(intake.counts.overflow > 0);
        let c = intake.counts;
        assert_eq!(c.blocks, 10, "{c}");
        assert_eq!(c.blocks + c.malformed + c.unplaced + c.late + c.ignored, 15);
    }

    /// A small deterministic generator for the fuzz below (xorshift64*).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    /// A frame near the spec: mostly an IQ block a server could send (any IQ rate, any format,
    /// two channels, a payload that fits its `length` either way), and often not quite one (an
    /// audio kind, a rate or format or channel count the spec does not have, a `length` that fits
    /// nothing, a receiver past the bound, a payload of any size).
    fn near_spec(rng: &mut Rng) -> Vec<u8> {
        let mut pick = |xs: &[u32]| xs[rng.below(xs.len() as u64) as usize];
        let kind = pick(&[0, 0, 0, 0, 0, 0, 1, 3, 9]);
        let rate = pick(&[
            48_000,
            96_000,
            192_000,
            384_000,
            48_000,
            48_000,
            12_000,
            0,
            u32::MAX,
        ]);
        let format = pick(&[3, 3, 3, 0, 0, 2, 1, 4]);
        let channels = pick(&[2, 2, 2, 2, 2, 2, 1, 0, 3]);
        let receiver = pick(&[0, 0, 0, 0, 0, 1, 16, u32::MAX]);
        let bytes = [2usize, 3, 4, 4].get(format as usize).copied().unwrap_or(4);
        let per_frame = bytes * (channels.max(1) as usize);
        let frames = rng.below((64 * 1024 / per_frame) as u64 + 1) as usize;
        let payload = if rng.below(6) == 0 {
            rng.below(70_000) as usize
        } else {
            frames * per_frame
        };
        let length = match rng.below(6) {
            0 => frames as u32,
            1 => rng.next() as u32,
            _ => (frames * channels.max(1) as usize) as u32,
        };
        let mut out = Vec::with_capacity(HEADER_LEN + payload);
        for w in [receiver, rate, format, 0, 0, length, kind, channels] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.resize(HEADER_LEN, 0);
        for _ in 0..payload {
            out.push(rng.next() as u8);
        }
        out
    }

    /// ⭐ FUZZ: ANY FRAME NEVER PANICS AND NEVER GROWS PAST ITS BOUND. Arbitrary bytes and frames near
    /// the spec, through the frame codec and into the intake as the session hands them, with any
    /// centre and the clock jumping as it likes, and the pan's thread drawing as it goes: no panic,
    /// the held samples within [`HELD`] pairs in a buffer never reallocated, every block counted
    /// once, every row [`N`] values in 0..1.
    #[test]
    fn any_frame_never_panics_and_nothing_grows_past_its_bound() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut intake = Intake::new(STALL_MS);
        intake.accept(true);
        let mut rows = Rows::default();
        let mut buf = Vec::with_capacity(2 * HELD);
        let (mut now, mut handed, mut drawn) = (1_000u64, 0u64, 0u32);
        for case in 0..6_000u32 {
            let bytes = if rng.below(3) == 0 {
                let n = rng.below(MAX_FRAME as u64 + 128) as usize;
                (0..n).map(|_| rng.next() as u8).collect()
            } else {
                near_spec(&mut rng)
            };
            now += match rng.below(10) {
                0 => rng.below(2_000),
                1 => 0,
                _ => rng.below(40),
            };
            if let Ok(Frame::Block(b)) = decode(&bytes) {
                let centre = match rng.below(4) {
                    0 => None,
                    1 => Hz::new(rng.below(1 << 40) as i64),
                    _ => Hz::new(14_080_000),
                };
                intake.push(&b, centre, now);
                handed += 1;
            }
            assert!(
                intake.samples.len() <= 2 * HELD && intake.samples.len().is_multiple_of(2),
                "case {case}: {} values held",
                intake.samples.len()
            );
            assert_eq!(intake.samples.capacity(), 2 * HELD, "case {case}");
            let c = intake.counts;
            assert_eq!(
                c.blocks + c.malformed + c.unplaced + c.late + c.ignored,
                handed,
                "case {case}: {c}"
            );
            if rng.below(8) == 0 {
                let taken = intake.take(&mut buf);
                rows.feed(&taken, &buf);
                if let Some(row) = rows.row(now) {
                    drawn += 1;
                    assert_eq!(row.row.len(), N);
                    assert!(row.row.iter().all(|v| (0.0..=1.0).contains(v)));
                    assert!(row.lo_hz.is_finite() && row.hi_hz > row.lo_hz);
                }
            }
        }
        assert!(handed > 600 && drawn > 50, "{handed} blocks, {drawn} rows");
    }

    // ── When the pan starts, stops, draws and says so ────────────────────────────────────────

    fn times() -> Times {
        Times::default()
    }

    /// A step of the control with these inputs at `now`.
    fn step(c: &mut Control, now: u64, want: Want, seen: Seen) -> Vec<Act> {
        c.step(
            &Inputs {
                now,
                want,
                ready: true,
                transmitting: false,
                seen,
                counts: Counts::default(),
                geometry: None,
            },
            &times(),
        )
    }

    fn says(acts: &[Act]) -> usize {
        acts.iter().filter(|a| matches!(a, Act::Say(..))).count()
    }

    /// ⭐ IQ STARTS ONLY WITH THE SWITCH ON AND THE SESSION READY, AND A SERVER THAT SENDS NONE IS
    /// TOLD ONCE: no IQ within the start window is `iq_stop`, one warning, nothing drawn, and no
    /// second `iq_start` until the switch goes off and on again.
    #[test]
    fn iq_starts_with_the_switch_on_and_a_server_that_sends_none_is_said_once() {
        let mut c = Control::default();
        assert!(step(&mut c, 0, Want::Off, Seen::default()).is_empty());
        let not_ready = c.step(
            &Inputs {
                now: 1,
                want: Want::On,
                ready: false,
                transmitting: false,
                seen: Seen::default(),
                counts: Counts::default(),
                geometry: None,
            },
            &times(),
        );
        assert!(not_ready.is_empty(), "not before ready");
        assert_eq!(step(&mut c, 2, Want::On, Seen::default()), [Act::Start]);
        assert!(step(&mut c, 2 + START_WINDOW_MS - 1, Want::On, Seen::default()).is_empty());
        let refused = step(&mut c, 2 + START_WINDOW_MS, Want::On, Seen::default());
        assert_eq!(refused.first(), Some(&Act::Stop));
        assert!(matches!(refused.get(1), Some(Act::Say(true, w)) if w.contains("sent no IQ")));
        assert_eq!(c.stream, Stream::Refused);
        assert!(!c.drawing);
        for t in 1..200 {
            assert!(
                step(
                    &mut c,
                    2 + START_WINDOW_MS + 20 * t,
                    Want::On,
                    Seen::default()
                )
                .is_empty(),
                "said once, asked once"
            );
        }
        assert!(step(&mut c, 9_000, Want::Off, Seen::default()).is_empty());
        assert_eq!(step(&mut c, 9_020, Want::On, Seen::default()), [Act::Start]);
    }

    /// ⭐ THE SWITCH OFF STOPS THE IQ AND EMPTIES THE SCOPE AT ONCE; A LAPSED WORD (A RADIO SWITCH)
    /// STOPS THE DRAWING AT ONCE AND THE IQ AFTER THE GRACE, AND LEAVES THE FEED TO WHOEVER OWNS IT.
    #[test]
    fn the_switch_off_stops_at_once_and_a_lapsed_word_after_its_grace() {
        let flowing = |at| Seen {
            any: Some(at),
            kept: Some(at),
        };
        let mut c = Control::default();
        step(&mut c, 0, Want::On, Seen::default());
        let acts = step(&mut c, 20, Want::On, flowing(20));
        assert!(c.drawing && says(&acts) == 1, "{acts:?}");
        let off = step(&mut c, 40, Want::Off, flowing(40));
        assert_eq!(off.first(), Some(&Act::Stop));
        assert!(off.contains(&Act::Clear), "{off:?}");
        assert!(!c.drawing);
        assert_eq!(c.stream, Stream::Idle);

        let mut c = Control::default();
        step(&mut c, 0, Want::On, Seen::default());
        step(&mut c, 20, Want::On, flowing(20));
        assert!(c.drawing);
        let lapsed = Want::Lapsed { since: 30 };
        assert!(step(&mut c, 40, lapsed, flowing(40)).is_empty(), "no Clear");
        assert!(!c.drawing, "not drawn once lapsed");
        assert!(step(&mut c, 30 + STOP_GRACE_MS - 1, lapsed, flowing(40)).is_empty());
        let stop = step(&mut c, 30 + STOP_GRACE_MS, lapsed, flowing(40));
        assert_eq!(stop.first(), Some(&Act::Stop));
        assert!(!stop.contains(&Act::Clear));
        // Renewed again before the grace: drawn again, the stream never stopped.
        let mut c = Control::default();
        step(&mut c, 0, Want::On, Seen::default());
        step(&mut c, 20, Want::On, flowing(20));
        step(&mut c, 40, lapsed, flowing(40));
        assert!(step(&mut c, 60, Want::On, flowing(60)).is_empty());
        assert!(c.drawing);
    }

    /// ⭐ IQ THAT STOPS IS SAID ONCE AND EMPTIES THE SCOPE, AND IQ THAT RESUMES IS DRAWN AGAIN; NO IQ
    /// UNDER A TRANSMISSION IS NOT A STALL (a receiver may pause its IQ while it transmits).
    #[test]
    fn iq_that_stops_is_said_once_and_a_transmission_is_no_stall() {
        let flowing = |at| Seen {
            any: Some(at),
            kept: Some(at),
        };
        let mut c = Control::default();
        step(&mut c, 0, Want::On, Seen::default());
        step(&mut c, 20, Want::On, flowing(20));
        assert!(step(&mut c, 20 + STALL_MS - 1, Want::On, flowing(20)).is_empty());
        let stalled = step(&mut c, 20 + STALL_MS, Want::On, flowing(20));
        assert!(stalled.contains(&Act::Clear), "{stalled:?}");
        assert!(matches!(stalled.iter().find(|a| matches!(a, Act::Say(..))),
            Some(Act::Say(true, w)) if w.contains("stopped sending")));
        assert!(c.stalled && !c.drawing);
        for t in 1..100 {
            assert!(step(&mut c, 20 + STALL_MS + 20 * t, Want::On, flowing(20)).is_empty());
        }
        let back = step(&mut c, 9_000, Want::On, flowing(9_000));
        assert!(matches!(back.as_slice(), [Act::Say(false, w)] if w.contains("again")));
        assert!(c.drawing);

        // Under a transmission: no stall, however long.
        let mut c = Control::default();
        step(&mut c, 0, Want::On, Seen::default());
        step(&mut c, 20, Want::On, flowing(20));
        for t in 2..200u64 {
            let acts = c.step(
                &Inputs {
                    now: 20 * t,
                    want: Want::On,
                    ready: true,
                    transmitting: true,
                    seen: flowing(20),
                    counts: Counts::default(),
                    geometry: None,
                },
                &times(),
            );
            assert!(acts.is_empty(), "{acts:?}");
        }
        assert!(c.drawing && !c.stalled);
        // The stall counts from the end of the transmission.
        assert!(step(&mut c, 4_000 + STALL_MS - 30, Want::On, flowing(20)).is_empty());
        assert!(step(&mut c, 4_000 + STALL_MS, Want::On, flowing(20)).contains(&Act::Clear));
    }

    // ── Against the simulated SDR program, with the real thread ──────────────────────────────

    /// The windows, short enough for a test.
    fn short() -> Times {
        Times {
            start_window_ms: 600,
            stall_ms: 300,
            want_ttl_ms: 300,
            stop_grace_ms: 800,
        }
    }

    fn sim_with(dialect: SimDialect, carriers: &[i64]) -> Simulator {
        let receive = SimReceive {
            carriers: carriers
                .iter()
                .map(|hz| Carrier {
                    hz: *hz as u64,
                    dbfs: -30.0,
                })
                .collect(),
            ..SimReceive::default()
        };
        Simulator::start(SimConfig {
            receive,
            ..SimConfig::dialect(dialect)
        })
        .expect("the simulator starts")
    }

    /// A client of the simulated program, as the radio loop starts one, with the short windows.
    fn client(sim: &Simulator) -> TciDaemon {
        registry::forget((sim.addr(), trx0()));
        let target = Target {
            host: "127.0.0.1".into(),
            port: sim.addr().port(),
            trx: 0,
            dialect: TciDialect::Auto,
            audio: true,
            rigctld_port: 0,
        };
        let d = TciDaemon::start_receiving(&target, Options::default()).expect("the client");
        d.pan.set_times(short());
        d
    }

    /// The radio loop's word every 20 ms for `ms`.
    fn tick(d: &TciDaemon, feed: &SpectrumFeed, on: bool, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            d.want_pan(on, feed);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The radio loop's word every 20 ms until `done`, or fail saying `what`.
    fn tick_until(
        d: &TciDaemon,
        feed: &SpectrumFeed,
        on: bool,
        what: &str,
        done: impl Fn() -> bool,
    ) {
        let deadline = Instant::now() + WAIT;
        while !done() {
            assert!(Instant::now() < deadline, "{what}: {:?}", d.pan.status());
            d.want_pan(on, feed);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The newest frame in the RF slot, as the RF scope pane reads it; empty when the slot holds
    /// none fresh.
    fn rf(feed: &SpectrumFeed) -> tempo_app::dto::SpectrumFrame {
        feed.rf_frame_after(0).expect("the pane's read answers")
    }

    fn as_row(f: &tempo_app::dto::SpectrumFrame) -> Spectrum {
        Spectrum::from(f.clone())
    }

    /// Whether `done` holds within half a second: well inside the three seconds after which the
    /// RF scope pane calls a silent slot stale by itself.
    fn soon(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_millis(500);
        while !done() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    fn sent(sim: &Simulator, text: &str) -> usize {
        sim.commands_from(0).iter().filter(|c| *c == text).count()
    }

    /// ⭐ THE SIMULATED PROGRAM'S CARRIERS LAND IN THE RIGHT BINS: below the dial, above it and above
    /// the receiver's centre (the simulator centres its IQ 6 kHz above the dial), in USB and again
    /// after the program's operator switches to LSB, on the RF slot the scope reads, as a frame
    /// that says what it is.
    #[test]
    fn the_programs_carriers_land_in_their_bins_in_usb_and_in_lsb() {
        let carriers = [14_072_500i64, 14_075_500, 14_086_000];
        let sim = sim_with(SimDialect::expert_sdr3(), &carriers);
        let (dial, centre) = (
            sim.radio().trx[0].vfo[0] as i64,
            sim.radio().trx[0].dds as i64,
        );
        assert_eq!(
            (dial, centre),
            (14_074_000, 14_080_000),
            "the simulator's own tuning"
        );
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        for mode in ["usb", "lsb"] {
            sim.operator(&format!("modulation:0,{mode};"));
            let before = rf(&feed).seq;
            tick_until(&d, &feed, true, "no row", || rf(&feed).seq > before + 3);
            let frame = rf(&feed);
            assert_eq!(frame.source, SOURCE);
            assert_eq!(frame.scale, SCALE);
            assert_eq!(frame.slice, Some(0));
            let row = as_row(&frame);
            assert_eq!(row.row.len(), N);
            assert!(
                ((row.lo_hz + row.hi_hz) / 2.0 - centre as f64 + 48_000.0 / N as f64 / 2.0).abs()
                    < 1e-6,
                "centred on the program's centre"
            );
            for c in carriers {
                let j = peak_near(&row, c as f64, 8);
                assert!(
                    (bin_hz(&row, j) - c as f64).abs() < 0.01,
                    "{mode}: {c} Hz drawn at {} Hz",
                    bin_hz(&row, j)
                );
                assert_eq!((bin_hz(&row, j) - dial as f64).round() as i64, c - dial);
            }
        }
    }

    /// ⭐ IQ STARTS ONLY WITH THE SWITCH ON, AND THE SWITCH OFF STOPS IT: nothing asked while the
    /// word is off; `iq_start` and rows once it is on; `iq_stop` once it is off again, and the RF
    /// slot emptied at once, so the scope says its source stopped.
    #[test]
    fn iq_starts_only_with_the_switch_on_and_the_switch_off_stops_it() {
        let sim = sim_with(SimDialect::expert_sdr3(), &[14_086_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        tick(&d, &feed, false, 400);
        assert_eq!(
            sent(&sim, "iq_start:0"),
            0,
            "nothing asked with the switch off"
        );
        tick_until(&d, &feed, true, "no row", || rf(&feed).source == SOURCE);
        assert_eq!(sent(&sim, "iq_start:0"), 1);
        tick_until(&d, &feed, false, "no iq_stop", || {
            sent(&sim, "iq_stop:0") == 1
        });
        // Emptied by the pan in the step that sent the stop, far inside the three seconds after
        // which the pane would call a silent slot stale by itself.
        assert!(
            soon(|| rf(&feed).bins.is_empty()),
            "the RF slot is emptied at once"
        );
        let rows = d.pan.status().rows;
        tick(&d, &feed, false, 300);
        assert_eq!(d.pan.status().rows, rows, "nothing drawn once off");
        assert_eq!(d.pan.status().stream, Stream::Idle);
    }

    /// ⭐ A PROGRAM WITH NO IQ (piHPSDR's TCI has no `iq_start`): one `iq_start`, then `iq_stop`, one
    /// line saying why, nothing drawn, and the scope keeps its previous source (the audio row). Not
    /// asked again while the switch stays on; asked again once it goes off and on.
    #[test]
    fn a_program_with_no_iq_leaves_the_scope_on_its_source_and_says_why_once() {
        let sim = sim_with(SimDialect::pihpsdr(), &[14_086_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        feed.publish_audio(Spectrum {
            row: vec![0.5; 512],
            lo_hz: 0.0,
            hi_hz: 4_000.0,
            source: "audio".into(),
        });
        tick_until(&d, &feed, true, "never refused", || {
            d.pan.status().stream == Stream::Refused
        });
        tick(&d, &feed, true, 1_000);
        assert_eq!(sent(&sim, "iq_start:0"), 1, "asked once");
        assert_eq!(sent(&sim, "iq_stop:0"), 1);
        let why: Vec<String> = d
            .pan
            .said()
            .into_iter()
            .filter(|w| w.contains("sent no IQ"))
            .collect();
        assert_eq!(why.len(), 1, "said once: {:?}", d.pan.said());
        assert!(rf(&feed).bins.is_empty(), "nothing drawn");
        feed.publish_audio(Spectrum {
            row: vec![0.5; 512],
            lo_hz: 0.0,
            hi_hz: 4_000.0,
            source: "audio".into(),
        });
        assert_eq!(
            feed.row().expect("a row").source,
            "audio",
            "the previous source stays"
        );
        assert_eq!(d.pan.status().rows, 0);
        tick(&d, &feed, false, 100);
        tick_until(&d, &feed, true, "not asked again", || {
            sent(&sim, "iq_start:0") == 2
        });
    }

    /// ⭐ A RADIO SWITCH: THE CLIENT LEFT BEHIND STOPS DRAWING AT ONCE AND STOPS ITS IQ AFTER THE
    /// GRACE. The radio loop stops renewing the word, as it does for a client moved to the monitor
    /// pool: no row after the word's time, `iq_stop` after the grace and not before.
    #[test]
    fn a_client_left_behind_by_a_radio_switch_stops_drawing_then_stops_its_iq() {
        let sim = sim_with(SimDialect::expert_sdr3(), &[14_086_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        tick_until(&d, &feed, true, "no row", || d.pan.status().rows >= 5);
        let left = Instant::now();
        // The word's time (300 ms) and a look of the pan's thread.
        std::thread::sleep(Duration::from_millis(400));
        let rows = d.pan.status().rows;
        assert!(!d.pan.status().drawing);
        assert_eq!(sent(&sim, "iq_stop:0"), 0, "not before the grace");
        assert!(sim.wait_for(WAIT, |log| log.iter().any(
            |l| matches!(&l.event, SimEvent::Command { text, .. } if text == "iq_stop:0")
        )));
        // The word's 300 ms from the last renewal (up to 20 ms before `left`), then the 800 ms grace.
        assert!(
            left.elapsed() >= Duration::from_millis(1_000),
            "{:?}",
            left.elapsed()
        );
        assert_eq!(d.pan.status().rows, rows, "no row once left behind");
    }

    /// ⭐ THE SESSION'S END EMPTIES THE SCOPE AT ONCE: the client goes while drawing, and the RF slot
    /// is empty straight away rather than holding the last row for three seconds. Its close sends
    /// `iq_stop` after the transmitter's stop, and the program, still sending IQ, reads both and
    /// then the close frame.
    #[test]
    fn the_sessions_end_empties_the_scope_at_once() {
        let sim = sim_with(SimDialect::expert_sdr3(), &[14_086_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        tick_until(&d, &feed, true, "no row", || rf(&feed).source == SOURCE);
        drop(d);
        assert!(
            soon(|| rf(&feed).bins.is_empty()),
            "the RF slot is emptied at once"
        );
        assert!(sim.wait_for(WAIT, |log| log
            .iter()
            .any(|l| matches!(l.event, SimEvent::Closed { .. }))));
        let log = sim.log();
        let read_at = |text: &str| {
            log.iter().position(
                |l| matches!(&l.event, SimEvent::Command { conn: 0, text: t, .. } if t == text),
            )
        };
        let stop = read_at("trx:0,false").expect("the program never read the stop");
        let iq_stop = read_at("iq_stop:0").expect("the program never read iq_stop");
        assert!(stop < iq_stop, "iq_stop before the transmitter's stop");
        let by = log.iter().find_map(|l| match l.event {
            SimEvent::Closed { conn: 0, by } => Some(by),
            _ => None,
        });
        assert_eq!(by, Some(Closer::Client), "the close frame was never read");
    }

    /// ⭐ IQ THAT STOPS UNDER THE PAN IS SAID AND EMPTIES THE SCOPE; IQ THAT COMES BACK IS DRAWN
    /// AGAIN. The program's IQ is stopped behind the pan's back (`iq_stop` sent on the session):
    /// within the stall time no row is published, the RF slot is empty and the log says so.
    #[test]
    fn iq_that_stops_under_the_pan_is_said_and_empties_the_scope() {
        let sim = sim_with(SimDialect::expert_sdr3(), &[14_086_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        tick_until(&d, &feed, true, "no row", || d.pan.status().rows >= 3);
        let conn = d.conn.as_ref().expect("the session");
        conn.send(Command::Stream(StreamSetting::IqStop(trx0())))
            .expect("sent");
        tick_until(&d, &feed, true, "never stalled", || d.pan.status().stalled);
        assert!(
            soon(|| rf(&feed).bins.is_empty()),
            "the RF slot is emptied at once"
        );
        assert!(d.pan.said().iter().any(|w| w.contains("stopped sending")));
        let rows = d.pan.status().rows;
        tick(&d, &feed, true, 400);
        assert_eq!(d.pan.status().rows, rows, "no stale row while stopped");
        conn.send(Command::Stream(StreamSetting::IqStart(trx0())))
            .expect("sent");
        tick_until(&d, &feed, true, "never drawn again", || {
            d.pan.status().rows > rows + 2
        });
        assert!(d.pan.said().iter().any(|w| w.contains("again")));
    }

    /// ⭐ RECEIVE AUDIO AND A KEY STILL WORK WITH IQ FLOWING, AT THE FASTEST IQ RATE: the program's
    /// IQ at 384 kHz draws rows while the receive audio carries the carrier 1 kHz above the dial,
    /// and the SDR program's tune carrier keys and unkeys through the same session.
    #[test]
    fn receive_audio_and_a_key_still_work_with_iq_flowing() {
        let sim = sim_with(SimDialect::expert_sdr3(), &[14_075_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        d.conn
            .as_ref()
            .unwrap()
            .send(Command::Stream(StreamSetting::IqSampleRate(
                IqRate::new(384_000).unwrap(),
            )))
            .expect("sent");
        tick_until(&d, &feed, true, "no row at 384 kHz", || {
            d.pan.status().geometry.is_some_and(|g| g.rate == 384_000) && d.pan.status().rows >= 5
        });
        let receive = d.receive().expect("the receive path");
        let _ = receive.take();
        tick(&d, &feed, true, 600);
        let heard = receive.take();
        assert!(
            heard.len() > 4_000,
            "{} samples of receive audio",
            heard.len()
        );
        let level = tempo_tcisim::synth::tone_level(&heard[1_000..], 12_000, 1_000.0);
        assert!(level > 0.01, "the carrier in the receive audio: {level}");
        let rows = d.pan.status().rows;
        d.tune_on(5_000).expect("the tune carrier keys");
        tick_until(&d, &feed, true, "never keyed", || {
            sim.log().iter().any(|l| {
                matches!(
                    l.event,
                    SimEvent::Keyed {
                        by: Keyer::Client(0),
                        source: SimSource::Tune,
                        ..
                    }
                )
            })
        });
        assert!(d.tune_off());
        tick_until(&d, &feed, true, "never unkeyed", || {
            sim.log()
                .iter()
                .any(|l| matches!(l.event, SimEvent::Unkeyed { .. }))
        });
        tick(&d, &feed, true, 300);
        assert!(d.pan.status().rows > rows, "IQ drawn throughout");
    }

    /// ⭐ THE FFT RUNS ON THE PAN'S OWN THREAD, NOT THE RADIO LOOP'S: the rows are built on
    /// "tci-pan", and the loop's word (here, this test's thread) only stores the word.
    #[test]
    fn the_rows_are_built_on_the_pans_own_thread() {
        let sim = sim_with(SimDialect::expert_sdr3(), &[14_086_000]);
        let d = client(&sim);
        let feed = SpectrumFeed::default();
        tick_until(&d, &feed, true, "no row", || d.pan.status().rows >= 1);
        let built_on = lock(&d.pan.shared().built_on).clone();
        assert_eq!(built_on.as_deref(), Some("tci-pan"));
        assert_ne!(
            built_on.as_deref(),
            std::thread::current().name(),
            "not the caller's"
        );
    }
}
