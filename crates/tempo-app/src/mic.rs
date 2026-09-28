//! The browser microphone: a streamed Remote operator's voice on the air, and the transmit-safety
//! machinery that bounds it.
//!
//! ⚠️ THE PREMISE, and the thing to hold on to when editing this module. A microphone over is a
//! LATCHED transmission with NO PRECOMPUTED END, the class [`crate::keyboard`] names: nothing
//! computes when it ends before the first sample goes out. So it is built the same way. A predicate
//! re-checked on every radio-loop tick ([`MicLatch::tick`]), its gate values taken as DATA
//! ([`MicGates`]) and its decisions returned as DATA ([`MicTickResult`]), so that it runs, and is
//! tested, with no engine, no radio and no clock behind it. Every failure DROPS THE OVER and unkeys;
//! holding the feed is no answer for a transmitter that is already keyed.
//!
//! | # | Condition | On failure |
//! |---|---|---|
//! | G0 | an over is armed at all | `Idle`, no drop |
//! | G1 | `tx_enabled` | drop, `GateDown` |
//! | G2 | `tx_allowed` (licence privileges for this dial and mode) | drop, `GateDown` |
//! | G3 | not `tuning` | drop, `GateDown` |
//! | G4 | in the Phone section | drop, `GateDown` |
//! | G5 | the stream's transmit presence, for the session this over armed under | drop, `Presence` |
//! | G7 | the transmit route this over armed on | drop, `DeviceChanged` |
//! | C1 | under [`MicMode::max_latch_ms`] from the key | drop, `Ceiling` |
//! | C2 | under the wall-clock TX watchdog | drop, `Watchdog` (also disarms TX) |
//! | G6 | audio accepted within [`MicMode::gap_ms`] | drop, `AudioGap` |
//!
//! G6 is checked last because it is the only gate the arriving audio itself moves: this tick's
//! frames count before it is judged.
//!
//! ## The rules this holds (the audio design's §6, M1–M12)
//!
//! - **M1: the hold arms, audio keys.** [`MicLatch::arm`] keys nothing. The first frame accepted
//!   after the arm answers [`MicTick::Key`], and only then is PTT asserted; the audio follows a
//!   [`MicMode::lead_in_ms`] later. An armed over that never receives audio never keys.
//! - **M2: 200 ms without accepted audio ends the over** ([`MicMode::gap_ms`]), measured from the
//!   ARRIVAL of the last frame accepted, not from the last one sent.
//! - **M3: never more than [`MicMode::ahead_ms`] ahead of real time.** Each tick tops the output
//!   ring up to the look-ahead with what is due (the operator's ruling of 2026-09-27, "Top up to
//!   40 ms", over the design's 20 ms per push; see [`MicMode::max_push_ms`]). The ring holds 20 s;
//!   this bound, enforced here and again by the loop's own budget, is what keeps it short. A wedged
//!   loop underruns into silence, and M2 ends the over.
//! - **M4: late audio is dropped, never caught up.** A frame the page sent out of order is refused
//!   by the feed ([`MicFeed::push`]); a frame that arrives after its moment has been played is
//!   dropped; after a stalled loop the moments that went by are skipped, never played late or
//!   squeezed.
//! - **M5: push-to-talk only.** With no over armed the feed takes no audio at all, so a frame that
//!   arrives then is gone rather than waiting to key the next over. Nothing here measures a level.
//! - **M12: the backstops.** Every drop runs through one kill path: the feed's epoch moves first
//!   ([`MicFeed`] refuses everything from then until the next arm), the over's own buffer goes with
//!   the latch, and the caller arms its abort so the radio loop flushes the output ring and drops
//!   PTT.
//!
//! ## Where the transport moved the design (it was written for the relay's audio lane)
//!
//! The page's microphone reaches the station as a WebRTC Opus track (plan S6), not as `audioTx`
//! bundles on the relay. Three of the design's mechanics are therefore carried by something else,
//! and the rule is kept in each case:
//! - **Late (M4).** RTP carries no sender clock the station can trust, so there is no
//!   "transport-measured residence". Late means what it means to a listener instead: a frame whose
//!   moment on the playout clock has already gone out. That clock is anchored at the first accepted
//!   frame, so a link that slows down mid-over reads as late audio, and M2 ends the over.
//! - **The over epoch (M12).** An RTP packet carries no over id. The epoch is the feed's: it moves
//!   on every drop, and the feed refuses every frame until the next arm, so audio from a dead over
//!   cannot reach a live one.
//! - **The jitter buffer.** The design's 60 ms (§5.2) is the lead-in: audio is held until
//!   [`MicMode::lead_in_ms`] after the key, and the first [`MicMode::ahead_ms`] of the over is a
//!   silent pre-roll that gives the output ring its cushion. A frame may therefore arrive up to the
//!   lead-in late and still be played at its moment, and the voice reaches the air
//!   `lead_in_ms + ahead_ms` after it reached the station: the design's 60 + 40.
//!
//! Silence is what fills a moment whose frame never came: no concealment, so a loss can never
//! become a burst on the air (design §7), and nothing heard on the air was made up here.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Everything the machinery knows about the microphone over. One const instance, [`MIC`];
/// numbers, not behaviour, so a test can instantiate the same machinery over different ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicMode {
    /// Operator-facing name, used in the messages the machinery generates.
    pub name: &'static str,
    /// C1. HARD CEILING on one over, measured from the key. No setting and no audio may raise it.
    pub max_latch_ms: u64,
    /// G6 (M2). How long the over survives with no frame accepted.
    pub gap_ms: u64,
    /// M1. From the key to the first sample fed to the transmitter, so the rig's own changeover
    /// is done before any audio reaches it. Also how late a frame may arrive and still play.
    pub lead_in_ms: u64,
    /// M3. The furthest ahead of real time audio may ever be pushed. A safety bound, exactly as
    /// [`crate::keyboard::KeyboardMode::stream_ahead_ms`] is: raising it raises the stuck-carrier
    /// window by the same amount.
    pub ahead_ms: u64,
    /// M3. The most one tick may push, whatever the loop asks for. EQUAL to the look-ahead, by
    /// the operator's ruling of 2026-09-27 ("Top up to 40 ms"): the design's 20 ms per push assumed
    /// a radio loop that ticks every 20 ms exactly, and this one ticks every 20 ms PLUS its own
    /// work (`run_radio` sleeps 20 ms after each step), so 20 ms a push fed less than real time and
    /// the over came out chopped. Each tick now tops the ring up to the look-ahead; the look-ahead
    /// is what bounds a stuck carrier, and it is unchanged.
    pub max_push_ms: u64,
    /// The transmit route's rate: `AudioBackend::play` takes 12 kHz mono.
    pub rate_hz: u32,
    /// Most frames the feed holds between two radio-loop ticks. At 20 ms a frame this is half a
    /// second of a loop that has stopped reading, far past anything that could still be played.
    pub feed_cap: usize,
    /// A frame whose moment is further than this beyond what the over is playing is not the
    /// same stream (the page restarted its track): it is not accepted, and M2 ends the over.
    pub max_early_ms: u64,
}

// The per-push cap is a cap inside the look-ahead, never a way past it.
const _: () = assert!(MIC.max_push_ms <= MIC.ahead_ms);

/// A frame this loud or louder counts as voiced (-40 dBFS): speech from a browser's microphone sits
/// far above it and a quiet room far below. Used ONLY by the no-power warning, which is display:
/// no level is ever a reason to key (M5).
pub const VOICE_FLOOR_RMS: f32 = 0.01;

impl MicMode {
    /// Samples at the route's rate in `ms` milliseconds.
    pub const fn samples(&self, ms: u64) -> u64 {
        self.rate_hz as u64 * ms / 1000
    }

    /// Whole samples at the route's rate in `d`.
    fn samples_in(&self, d: Duration) -> u64 {
        (d.as_nanos() * self.rate_hz as u128 / 1_000_000_000) as u64
    }
}

/// The browser microphone. 10 minutes matches RTTY and PSK31, one ceiling for every latched mode
/// (design §9, decision 2); 200 ms is five 40 ms bundles, or ten 20 ms frames, and this codebase's
/// meaning of "no longer current" (design M2).
///
/// The worst case after a total link death, which these numbers are chosen for: the gap (200 ms)
/// plus at most the look-ahead queued (40 ms), then the sound card's own buffer, about 280 ms of
/// RF after the last audio the operator spoke (design M2).
pub const MIC: MicMode = MicMode {
    name: "Remote microphone",
    max_latch_ms: 10 * 60 * 1000,
    gap_ms: 200,
    lead_in_ms: 60,
    ahead_ms: 40,
    max_push_ms: 40,
    rate_hz: 12_000,
    feed_cap: 25,
    max_early_ms: 500,
};

/// One decoded frame of the page's microphone.
#[derive(Debug, Clone, PartialEq)]
pub struct MicFrame {
    /// The RTP sequence number, extended (it never wraps).
    pub seq: u64,
    /// Where its first sample sits on the page's media clock, in samples at the route's rate.
    pub media: u64,
    /// When its packet reached the station.
    pub arrived: Instant,
    /// Mono samples at the route's rate.
    pub samples: Vec<f32>,
}

impl MicFrame {
    /// One past its last sample on the media clock.
    fn end(&self) -> u64 {
        self.media + self.samples.len() as u64
    }
}

/// Why the feed would not take a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// No over is armed (M5), or the one that was has just been dropped (M12).
    Closed,
    /// Its sequence number is not after the last one taken (M4).
    OutOfOrder,
    /// The radio loop has not read the feed for [`MicMode::feed_cap`] frames.
    Full,
}

/// Why an over ended, as the operator is told (the audio design's §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicEnded {
    /// The operator let go.
    Released,
    /// A stop at the station, TX turned off, leaving Phone, a tune, or the licence.
    Stopped,
    /// No audio for [`MicMode::gap_ms`] (M2).
    AudioGap,
    /// The stream's transmit presence lapsed (M7).
    Presence,
    /// The per-over ceiling (C1).
    Ceiling,
    /// The wall-clock TX watchdog (C2).
    Watchdog,
    /// The transmit route changed under the over (G7).
    RouteChanged,
}

/// The over as the stream shows it to the page. Published by the engine, read by the stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MicStatus {
    /// An over is armed (or keyed).
    pub armed: bool,
    /// The operator's audio has keyed the rig.
    pub keyed: bool,
    /// Display only: voice for two seconds, and the rig reports no power out.
    pub no_power_out: bool,
    /// Why the last over ended, until the next one is armed.
    pub ended: Option<MicEnded>,
}

#[derive(Default)]
struct FeedState {
    open: bool,
    epoch: u64,
    frames: VecDeque<MicFrame>,
    last_seq: Option<u64>,
    status: MicStatus,
}

/// The page's microphone, between the stream (which decodes it) and the radio loop (which plays
/// it). Cheap to clone; every clone is the same feed.
///
/// It takes audio only while an over is armed, in sequence order. The engine opens it on an arm
/// and closes it on every drop; closing moves the epoch and empties it in one step, so nothing a
/// dead over was sent survives into the next.
#[derive(Clone, Default)]
pub struct MicFeed(Arc<Mutex<FeedState>>);

impl MicFeed {
    fn state(&self) -> MutexGuard<'_, FeedState> {
        // Nothing under this lock can panic halfway through a change that matters, and a drop must
        // always be able to close the feed, so a poisoned lock is taken as it stands.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A decoded frame arrived from the page.
    pub fn push(&self, frame: MicFrame) -> Result<(), Refused> {
        let mut s = self.state();
        if !s.open {
            return Err(Refused::Closed);
        }
        if s.last_seq.is_some_and(|last| frame.seq <= last) {
            return Err(Refused::OutOfOrder);
        }
        if s.frames.len() >= MIC.feed_cap {
            return Err(Refused::Full);
        }
        s.last_seq = Some(frame.seq);
        s.frames.push_back(frame);
        Ok(())
    }

    /// The over epoch: moves on every drop.
    pub fn epoch(&self) -> u64 {
        self.state().epoch
    }

    /// Whether an over is armed to take audio.
    pub fn is_open(&self) -> bool {
        self.state().open
    }

    /// The over as the engine last published it, for the page.
    pub fn status(&self) -> MicStatus {
        self.state().status
    }

    /// Publish the over's state for the page (the engine does, whenever it may have changed).
    pub(crate) fn publish(&self, status: MicStatus) {
        self.state().status = status;
    }

    /// An over was armed: take audio from now on, starting empty.
    pub(crate) fn open(&self) {
        let mut s = self.state();
        s.open = true;
        s.frames.clear();
        s.last_seq = None;
    }

    /// The over was dropped. The epoch moves FIRST and the feed empties in the same step (M12).
    pub(crate) fn close(&self) {
        let mut s = self.state();
        s.epoch = s.epoch.wrapping_add(1);
        s.open = false;
        s.frames.clear();
        s.last_seq = None;
    }

    /// Everything that arrived since the last call, oldest first.
    pub(crate) fn take(&self) -> Vec<MicFrame> {
        self.state().frames.drain(..).collect()
    }
}

/// What the radio loop should do this tick on behalf of the microphone over.
#[derive(Debug, Clone, PartialEq)]
pub enum MicTick {
    /// No over. The loop must not key on its behalf (and if it was keyed, the over has just been
    /// dropped and the abort armed).
    Idle,
    /// Armed and waiting for audio. Key nothing (M1).
    Armed,
    /// The first audio of the over has been accepted: assert PTT now. Audio follows the lead-in.
    Key,
    /// Keyed, and nothing is due: inside the lead-in, or the look-ahead is full. Stay keyed.
    Ahead,
    /// Keyed: push these samples to the transmit route (never empty).
    Samples(Vec<f32>),
}

/// Why [`MicLatch::tick`] decided the over has to end. The caller runs its kill path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicDrop {
    /// G1–G4: a transmit gate that was up at the arm has gone down.
    GateDown,
    /// G5: the stream's transmit presence lapsed, or is a different session's.
    Presence,
    /// G7: the transmit route changed under the over.
    DeviceChanged,
    /// C1: the per-over ceiling.
    Ceiling,
    /// C2: the wall-clock watchdog. Also disarms TX, so it stays stopped.
    Watchdog,
    /// G6: no audio accepted for [`MicMode::gap_ms`].
    AudioGap,
}

/// The gate values the predicate re-checks, sampled by the caller for this tick. Values, never
/// callbacks, so the predicate runs with nothing behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicGates {
    /// Normal transmit is armed (the TX-enable latch, the watchdog's disarm).
    pub tx_enabled: bool,
    /// The dial and mode are inside the operator's licence privileges.
    pub tx_allowed: bool,
    /// A tune carrier is up: someone else owns the transmitter.
    pub tuning: bool,
    /// The station is in the Phone section.
    pub in_section: bool,
    /// The stream's transmit presence is live, and is the session this over armed under.
    pub presence: bool,
    /// The radio loop's transmit-route generation: it moves whenever the route the audio goes out
    /// by is replaced (a device reopened, the Flex DAX route installed or removed).
    pub route: u64,
    /// The monotonic clock, for everything measured against a packet's arrival.
    pub now: Instant,
    /// Unix ms, for the ceiling.
    pub now_ms: u64,
    /// Unix secs, for the wall-clock watchdog.
    pub now_secs: u64,
    /// The watchdog ceiling in seconds; 0 disables it.
    pub watchdog_limit_secs: u64,
    /// When the current unattended-transmit run began, or `None` if its clock has not started.
    pub watchdog_start_secs: Option<u64>,
}

/// What one tick decided.
#[derive(Debug, Clone, PartialEq)]
pub struct MicTickResult {
    /// What to do this tick. Always [`MicTick::Idle`] when `drop` is set.
    pub tick: MicTick,
    /// Set when the over must end. The caller MUST run its kill path ([`MicLatch::drop_latch`],
    /// the feed's close, the abort) before anything else; the predicate does not clear the state
    /// itself, so every drop in the app goes through the one kill path.
    pub drop: Option<MicDrop>,
    /// The watchdog clock this tick started, if it reached that check.
    pub watchdog_start: Option<u64>,
}

impl MicTickResult {
    fn now(tick: MicTick) -> Self {
        Self {
            tick,
            drop: None,
            watchdog_start: None,
        }
    }

    fn dropped(why: MicDrop) -> Self {
        Self {
            tick: MicTick::Idle,
            drop: Some(why),
            watchdog_start: None,
        }
    }
}

/// A keyed over.
#[derive(Debug)]
struct Over {
    /// Unix ms of the key: the ceiling's anchor.
    keyed_ms: u64,
    /// When the first sample fed to the transmitter plays: the key, plus the lead-in.
    start: Instant,
    /// Samples fed so far, counted from `start`. The first `ahead_ms` of them are the pre-roll.
    cursor: u64,
    /// The media position of the first frame accepted.
    media0: u64,
    /// When the last frame accepted arrived (G6).
    last_arrival: Instant,
    /// The route the over keyed on (G7).
    route: u64,
    /// Frames accepted and not yet played, in media order.
    frames: VecDeque<MicFrame>,
    /// How much VOICED audio the over has accepted, in ms: frames at or above
    /// [`VOICE_FLOOR_RMS`]. Display only (the no-power warning); nothing keys on it (M5).
    voiced_ms: u64,
}

impl Over {
    /// The media position the next sample fed plays.
    fn position(&self, mode: &MicMode) -> i128 {
        self.media0 as i128 + self.cursor as i128 - mode.samples(mode.ahead_ms) as i128
    }
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// Armed, not keyed. `route` is the transmit route seen on the first tick after the arm.
    Armed {
        route: Option<u64>,
    },
    Keyed(Box<Over>),
}

/// The microphone over's latch: idle, armed, or keyed.
#[derive(Debug)]
pub struct MicLatch {
    mode: MicMode,
    phase: Phase,
}

impl Default for MicLatch {
    fn default() -> Self {
        Self::new(MIC)
    }
}

impl MicLatch {
    pub fn new(mode: MicMode) -> Self {
        Self {
            mode,
            phase: Phase::Idle,
        }
    }

    /// Arm an over. Keys nothing (M1). The caller runs its up-front gate FIRST and opens the feed.
    /// Returns whether this call armed it (false: an over is already armed or keyed, and a second
    /// arm must not restart anything).
    pub fn arm(&mut self) -> bool {
        if self.active() {
            return false;
        }
        self.phase = Phase::Armed { route: None };
        true
    }

    /// Drop the over NOW, whatever state it was in. Returns whether it was KEYED: the caller's cue
    /// to arm its abort. Conditional on purpose: an armed over has put nothing on the air, and a
    /// one-shot abort armed for nothing would cut an unrelated over riding the same PTT.
    pub fn drop_latch(&mut self) -> bool {
        let keyed = matches!(self.phase, Phase::Keyed(_));
        self.phase = Phase::Idle;
        keyed
    }

    /// An over is armed or keyed: it owns the transmitter.
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    /// The over has keyed the rig.
    pub fn keyed(&self) -> bool {
        matches!(self.phase, Phase::Keyed(_))
    }

    /// How much voiced audio the keyed over has accepted, in ms (0 when not keyed). For the
    /// no-power warning only.
    pub fn voiced_ms(&self) -> u64 {
        match &self.phase {
            Phase::Keyed(over) => over.voiced_ms,
            _ => 0,
        }
    }

    /// ⚠️ THE PER-TICK PREDICATE, and the pacer behind it. `arrived` is everything the feed took
    /// since the last tick; `budget` is the most the loop can take this tick, in samples.
    ///
    /// GATE FOR GATE, in the order of the table in the module header, and the order is part of the
    /// contract: the watchdog's clock must not be started by a tick a gate already ended, and the
    /// gap is judged only after this tick's audio has been counted.
    pub fn tick(
        &mut self,
        gates: MicGates,
        arrived: Vec<MicFrame>,
        budget: usize,
    ) -> MicTickResult {
        let mode = self.mode;
        // G0: nothing is armed, so there is nothing to gate, nothing to key, and nowhere for audio
        // to go. (The feed is closed while idle, so `arrived` is empty here in the app.)
        if !self.active() {
            return MicTickResult::now(MicTick::Idle);
        }
        // G1–G4: every gate the arm checked, checked again.
        if !gates.tx_enabled || !gates.tx_allowed || gates.tuning || !gates.in_section {
            return MicTickResult::dropped(MicDrop::GateDown);
        }
        // G5: the permit for THIS over (M7). A lapse, or another session's presence, ends it.
        if !gates.presence {
            return MicTickResult::dropped(MicDrop::Presence);
        }
        // G7: the route the over armed on. Recorded on its first tick, compared on every one after.
        match &mut self.phase {
            Phase::Armed { route } => {
                if *route.get_or_insert(gates.route) != gates.route {
                    return MicTickResult::dropped(MicDrop::DeviceChanged);
                }
            }
            Phase::Keyed(over) if over.route != gates.route => {
                return MicTickResult::dropped(MicDrop::DeviceChanged);
            }
            _ => {}
        }
        let mut watchdog_start = None;
        if let Phase::Keyed(over) = &self.phase {
            // C1: the hard per-over ceiling, from the key. No audio can extend it.
            if gates.now_ms.saturating_sub(over.keyed_ms) >= mode.max_latch_ms {
                return MicTickResult::dropped(MicDrop::Ceiling);
            }
            // C2: the ordinary wall-clock watchdog. An armed over has transmitted nothing, so only
            // a keyed one reaches it, and the first keyed tick that does starts its clock.
            if gates.watchdog_limit_secs > 0 {
                let start = gates.watchdog_start_secs.unwrap_or(gates.now_secs);
                watchdog_start = Some(start);
                if gates.now_secs.saturating_sub(start) >= gates.watchdog_limit_secs {
                    let mut res = MicTickResult::dropped(MicDrop::Watchdog);
                    res.watchdog_start = watchdog_start;
                    return res;
                }
            }
        }
        // M1: an armed over keys on the first audio it is given, and on nothing else.
        if let Phase::Armed { route } = self.phase {
            let mut arrived = arrived.into_iter();
            let Some(first) = arrived.next() else {
                return MicTickResult::now(MicTick::Armed);
            };
            let mut over = Over {
                keyed_ms: gates.now_ms,
                start: gates.now + Duration::from_millis(mode.lead_in_ms),
                cursor: 0,
                media0: first.media,
                last_arrival: first.arrived,
                route: route.unwrap_or(gates.route),
                frames: VecDeque::new(),
                voiced_ms: 0,
            };
            over.count_voice(&mode, &first);
            over.frames.push_back(first);
            for frame in arrived {
                over.accept(&mode, frame);
            }
            self.phase = Phase::Keyed(Box::new(over));
            return MicTickResult::now(MicTick::Key);
        }
        let Phase::Keyed(over) = &mut self.phase else {
            return MicTickResult::now(MicTick::Idle);
        };
        for frame in arrived {
            over.accept(&mode, frame);
        }
        // G6 (M2): the gap, from the arrival of the last frame the over accepted.
        if gates.now.saturating_duration_since(over.last_arrival)
            >= Duration::from_millis(mode.gap_ms)
        {
            return MicTickResult::dropped(MicDrop::AudioGap);
        }
        let tick = match over.feed(&mode, gates.now, budget) {
            Some(samples) => MicTick::Samples(samples),
            None => MicTick::Ahead,
        };
        MicTickResult {
            tick,
            drop: None,
            watchdog_start,
        }
    }
}

impl Over {
    /// Count an accepted frame toward the over's voiced audio, if it carries voice.
    fn count_voice(&mut self, mode: &MicMode, frame: &MicFrame) {
        if frame.samples.is_empty() {
            return;
        }
        let power = frame.samples.iter().map(|s| s * s).sum::<f32>() / frame.samples.len() as f32;
        if power.sqrt() >= VOICE_FLOOR_RMS {
            self.voiced_ms += frame.samples.len() as u64 * 1000 / u64::from(mode.rate_hz);
        }
    }

    /// Take a frame into the over, unless its moment has already gone out (M4) or it is too far
    /// ahead to be the same stream. Only an accepted frame moves the gap's clock.
    fn accept(&mut self, mode: &MicMode, frame: MicFrame) {
        let playing = self.position(mode);
        if (frame.end() as i128) <= playing {
            return; // late: its moment has been played, or skipped
        }
        if frame.media as i128 > playing + mode.samples(mode.max_early_ms) as i128 {
            return; // not this stream: the page's media clock jumped
        }
        if self
            .frames
            .back()
            .is_some_and(|last| frame.media < last.end())
        {
            return; // overlaps what the over already holds: out of order on the media clock
        }
        self.last_arrival = self.last_arrival.max(frame.arrived);
        self.count_voice(mode, &frame);
        self.frames.push_back(frame);
    }

    /// This tick's samples, or `None` when nothing is due.
    ///
    /// Everything is measured on the playout clock that starts at `start`: sample `n` of the over
    /// plays `n / rate` after it. A moment that has already gone by is skipped, never played late
    /// (M4); nothing reaches further than `ahead_ms` past now (M3), or past the loop's budget.
    fn feed(&mut self, mode: &MicMode, now: Instant, budget: usize) -> Option<Vec<f32>> {
        if now < self.start {
            return None; // the lead-in (M1)
        }
        let elapsed = mode.samples_in(now - self.start);
        // The moments that went by while the loop was not feeding are gone.
        self.cursor = self.cursor.max(elapsed);
        let end = (elapsed + mode.samples(mode.ahead_ms)).min(self.cursor + budget as u64);
        if end <= self.cursor {
            return None; // fed as far ahead as it may be
        }
        let mut out = Vec::with_capacity((end - self.cursor) as usize);
        while self.cursor < end {
            let at = self.position(mode);
            // Frames wholly behind the moment being fed are done with.
            while self.frames.front().is_some_and(|f| (f.end() as i128) <= at) {
                self.frames.pop_front();
            }
            let sample = self
                .frames
                .front()
                .filter(|f| f.media as i128 <= at)
                .map(|f| f.samples[(at - f.media as i128) as usize])
                // The pre-roll, and any moment whose frame never came: silence, never made up.
                .unwrap_or(0.0);
            out.push(sample);
            self.cursor += 1;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of `n` samples at `level`, at media position `media`, arriving at `at`.
    fn frame(seq: u64, media: u64, at: Instant, level: f32) -> MicFrame {
        MicFrame {
            seq,
            media,
            arrived: at,
            samples: vec![level; MIC.samples(20) as usize],
        }
    }

    /// Every gate up, the route at 1, presence live, no watchdog.
    fn up(now: Instant) -> MicGates {
        MicGates {
            tx_enabled: true,
            tx_allowed: true,
            tuning: false,
            in_section: true,
            presence: true,
            route: 1,
            now,
            now_ms: 1_000_000,
            now_secs: 1_000,
            watchdog_limit_secs: 0,
            watchdog_start_secs: None,
        }
    }

    const PLENTY: usize = 100_000;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A latch armed and keyed on a first frame arriving at `t0`, and the tick that keyed it.
    fn keyed(t0: Instant) -> MicLatch {
        let mut latch = MicLatch::default();
        assert!(latch.arm());
        let res = latch.tick(up(t0), vec![frame(1, 0, t0, 0.5)], PLENTY);
        assert_eq!(res.tick, MicTick::Key, "precondition: the first frame keys");
        latch
    }

    /// Everything pushed from `from` to `to` in 20 ms ticks, a frame arriving every 20 ms on the
    /// media clock (media position `20 ms × k`) at `arrive(k)`, as the loop would drive it.
    fn drive(
        latch: &mut MicLatch,
        t0: Instant,
        ticks: std::ops::Range<u64>,
        mut arrive: impl FnMut(u64) -> Option<Instant>,
    ) -> (Vec<f32>, Option<(u64, MicDrop)>) {
        let mut out = Vec::new();
        for k in ticks {
            let now = t0 + ms(20 * k);
            let frames: Vec<MicFrame> = arrive(k)
                .map(|at| frame(k + 1, MIC.samples(20) * k, at, 0.5))
                .into_iter()
                .collect();
            let res = latch.tick(up(now), frames, PLENTY);
            if let Some(why) = res.drop {
                return (out, Some((k, why)));
            }
            if let MicTick::Samples(s) = res.tick {
                out.extend(s);
            }
        }
        (out, None)
    }

    // ── M1: the hold arms, audio keys ─────────────────────────────────────────────────────────

    #[test]
    fn arming_without_audio_never_keys() {
        let t0 = Instant::now();
        let mut latch = MicLatch::default();
        assert!(latch.arm(), "the arm was refused");
        assert!(!latch.arm(), "a second arm must not restart the over");
        for k in 0..100 {
            let res = latch.tick(up(t0 + ms(20 * k)), Vec::new(), PLENTY);
            assert_eq!(
                res.tick,
                MicTick::Armed,
                "tick {k}: an armed over with no audio did something other than wait"
            );
            assert_eq!(
                res.drop, None,
                "tick {k}: an armed over was dropped for no reason"
            );
        }
        assert!(!latch.keyed(), "an over with no audio keyed");
        // Positive control: the first frame keys.
        let res = latch.tick(
            up(t0 + ms(2_000)),
            vec![frame(1, 0, t0 + ms(1_990), 0.5)],
            PLENTY,
        );
        assert_eq!(
            res.tick,
            MicTick::Key,
            "the first accepted frame did not key"
        );
        assert!(latch.keyed());
    }

    #[test]
    fn the_audio_follows_the_lead_in_and_a_silent_pre_roll() {
        let t0 = Instant::now();
        let mut latch = keyed(t0);
        // Inside the lead-in nothing is fed.
        let res = latch.tick(up(t0 + ms(MIC.lead_in_ms - 1)), Vec::new(), PLENTY);
        assert_eq!(res.tick, MicTick::Ahead, "audio was fed inside the lead-in");
        // At the lead-in the output ring is filled to the look-ahead, and that first stretch is
        // the pre-roll: silence, so the voice starts only once the ring has its cushion.
        let res = latch.tick(up(t0 + ms(MIC.lead_in_ms)), Vec::new(), PLENTY);
        let MicTick::Samples(first) = res.tick else {
            panic!("nothing was fed at the end of the lead-in: {:?}", res.tick);
        };
        assert_eq!(
            first.len() as u64,
            MIC.samples(MIC.ahead_ms),
            "the first push is the look-ahead"
        );
        assert!(
            first.iter().all(|&s| s == 0.0),
            "the pre-roll carried audio"
        );
        // The frame that keyed the over plays right after it.
        let res = latch.tick(up(t0 + ms(MIC.lead_in_ms + 20)), Vec::new(), PLENTY);
        let MicTick::Samples(voice) = res.tick else {
            panic!("the keying frame was never played: {:?}", res.tick);
        };
        assert!(
            voice.contains(&0.5),
            "the keying frame's audio did not follow the pre-roll"
        );
    }

    // ── M5: push-to-talk only ─────────────────────────────────────────────────────────────────

    #[test]
    fn audio_without_an_armed_over_never_keys() {
        let t0 = Instant::now();
        let feed = MicFeed::default();
        // A loud tone arrives with nothing armed: the feed does not take it.
        assert_eq!(feed.push(frame(1, 0, t0, 1.0)), Err(Refused::Closed));
        assert!(feed.take().is_empty(), "audio with no over armed was kept");
        // Across every gate combination an idle latch keys nothing, whatever arrives.
        let mut latch = MicLatch::default();
        for bits in 0u8..32 {
            let mut g = up(t0 + ms(bits as u64 * 20));
            g.tx_enabled = bits & 1 != 0;
            g.tx_allowed = bits & 2 != 0;
            g.tuning = bits & 4 != 0;
            g.in_section = bits & 8 != 0;
            g.presence = bits & 16 != 0;
            let res = latch.tick(g, vec![frame(bits as u64 + 2, 0, g.now, 1.0)], PLENTY);
            assert_eq!(
                res.tick,
                MicTick::Idle,
                "gates {bits:05b}: an unarmed over did something"
            );
            assert!(
                !latch.keyed(),
                "gates {bits:05b}: audio keyed an unarmed over"
            );
        }
        // Positive control: the same audio into an armed over is taken, and keys it.
        feed.open();
        assert_eq!(feed.push(frame(40, 0, t0, 1.0)), Ok(()));
        assert!(latch.arm());
        assert_eq!(latch.tick(up(t0), feed.take(), PLENTY).tick, MicTick::Key);
    }

    // ── M2: the gap ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn two_hundred_ms_without_audio_ends_the_over_and_one_hundred_eighty_does_not() {
        let t0 = Instant::now();
        // Audio stops after the frame arriving at 1000 ms (tick 50).
        let mut latch = keyed(t0);
        let (_, dropped) = drive(&mut latch, t0, 1..200, |k| {
            (k <= 50).then(|| t0 + ms(20 * k))
        });
        let (at, why) = dropped.expect("the over survived its audio stopping");
        assert_eq!(why, MicDrop::AudioGap);
        // The last accepted frame arrived at 1000 ms; the drop is the first tick at or past 1200.
        assert_eq!(
            at * 20,
            1_000 + MIC.gap_ms,
            "the gap did not end the over at {} ms",
            MIC.gap_ms
        );

        // Positive control: a 180 ms pause (frames stop, then resume) does not end it.
        let mut latch = keyed(t0);
        let (_, dropped) = drive(&mut latch, t0, 1..200, |k| {
            let t = 20 * k;
            (!(1_000 < t && t < 1_180)).then(|| t0 + ms(t))
        });
        assert_eq!(dropped, None, "a 180 ms pause ended the over");
    }

    // ── M3: the look-ahead ────────────────────────────────────────────────────────────────────

    #[test]
    fn a_push_never_reaches_past_the_look_ahead_or_the_budget() {
        let t0 = Instant::now();
        let mut latch = keyed(t0);
        // A stalled loop clock with an unlimited budget: however often it is asked, nothing is fed
        // beyond `ahead_ms` of real time.
        let now = t0 + ms(MIC.lead_in_ms + 100);
        let mut fed = 0u64;
        for _ in 0..50 {
            if let MicTick::Samples(s) = latch.tick(up(now), Vec::new(), PLENTY).tick {
                fed += s.len() as u64;
            }
        }
        let limit = MIC.samples(MIC.ahead_ms);
        assert!(fed > 0, "nothing was fed at all, so this proves nothing");
        assert!(
            fed <= limit,
            "fed {fed} samples ahead of a stalled clock; the bound is {limit}"
        );
        // And no single push is larger than the budget it was given.
        let mut latch = keyed(t0);
        for k in 0..50u64 {
            let budget = MIC.samples(MIC.max_push_ms) as usize;
            if let MicTick::Samples(s) = latch
                .tick(up(t0 + ms(MIC.lead_in_ms + 30 * k)), Vec::new(), budget)
                .tick
            {
                assert!(
                    s.len() <= budget,
                    "a push of {} samples, budget {budget}",
                    s.len()
                );
            }
        }
    }

    // ── M4: late and out-of-order audio ───────────────────────────────────────────────────────

    #[test]
    fn late_and_out_of_order_frames_are_dropped_not_played() {
        let t0 = Instant::now();
        let feed = MicFeed::default();
        feed.open();
        assert_eq!(feed.push(frame(5, 0, t0, 0.5)), Ok(()));
        assert_eq!(
            feed.push(frame(5, 240, t0, 0.5)),
            Err(Refused::OutOfOrder),
            "a repeat was taken"
        );
        assert_eq!(
            feed.push(frame(4, 240, t0, 0.5)),
            Err(Refused::OutOfOrder),
            "a reordered frame was taken"
        );
        assert_eq!(
            feed.push(frame(6, 240, t0, 0.5)),
            Ok(()),
            "the next frame in order was refused"
        );

        // A frame that arrives after its moment has gone out is never played; one in time is.
        // Frame 10 (media 200 ms, due on the air at 60 + 40 + 200 = 300 ms) is held back and
        // delivered at 800 ms; frame 20 (media 400 ms) arrives in time, carrying a marker level.
        let mut latch = keyed(t0);
        let (marker, late_marker) = (0.25, 0.125);
        let mut out = Vec::new();
        for k in 1..40u64 {
            let now = t0 + ms(20 * k);
            let frames = if k == 10 {
                Vec::new()
            } else {
                let level = if k == 20 { marker } else { 0.5 };
                vec![frame(k + 1, MIC.samples(20) * k, now, level)]
            };
            if let MicTick::Samples(s) = latch.tick(up(now), frames, PLENTY).tick {
                out.extend(s);
            }
        }
        let late = frame(11, MIC.samples(20) * 10, t0 + ms(800), late_marker);
        if let MicTick::Samples(s) = latch.tick(up(t0 + ms(800)), vec![late], PLENTY).tick {
            out.extend(s);
        }
        assert!(out.contains(&marker), "an in-time frame was not played");
        assert!(
            !out.contains(&late_marker),
            "a late frame was played after its moment"
        );
    }

    #[test]
    fn a_link_that_falls_behind_ends_the_over_though_audio_still_arrives() {
        // From 400 ms the link slows by 100 ms: every frame still arrives, well inside the gap,
        // but each one after its moment has gone out (the lead-in allows 60). Late audio is not
        // accepted (M4), so it cannot hold the over open either (M2): the over ends 200 ms after
        // the last frame that was on time, with audio still arriving.
        let t0 = Instant::now();
        // Slot k (media 20k ms) is delivered at tick k up to slot 20, and `delay` late after it,
        // stamped with the moment it really arrives.
        let run = |delay: u64| -> Option<(u64, MicDrop)> {
            let mut latch = keyed(t0);
            for j in 1..100u64 {
                let now = t0 + ms(20 * j);
                let slot = if j <= 20 {
                    Some(j)
                } else {
                    j.checked_sub(delay / 20).filter(|&k| k > 20)
                };
                let frames: Vec<MicFrame> = slot
                    .map(|k| frame(k + 1, MIC.samples(20) * k, now, 0.5))
                    .into_iter()
                    .collect();
                if let Some(why) = latch.tick(up(now), frames, PLENTY).drop {
                    return Some((j, why));
                }
            }
            None
        };
        let (at, why) = run(100).expect("late audio held the over open");
        assert_eq!(why, MicDrop::AudioGap);
        assert_eq!(
            at * 20,
            400 + MIC.gap_ms,
            "the over did not end {} ms after its last on-time frame",
            MIC.gap_ms
        );
        // Positive control: the same frames, on time, keep it going.
        assert_eq!(run(0), None, "on-time audio ended the over");
    }

    #[test]
    fn a_stalled_loop_skips_the_moments_that_went_by_rather_than_catching_up() {
        let t0 = Instant::now();
        let mut latch = keyed(t0);
        let (fed_before, dropped) = drive(&mut latch, t0, 1..20, |k| Some(t0 + ms(20 * k)));
        assert_eq!(dropped, None);
        // The loop stalls 300 ms; the audio keeps arriving meanwhile.
        let stalled: Vec<MicFrame> = (20..35)
            .map(|k| frame(k + 1, MIC.samples(20) * k, t0 + ms(20 * k), 0.5))
            .collect();
        let now = t0 + ms(700);
        let res = latch.tick(up(now), stalled, PLENTY);
        assert_eq!(
            res.drop, None,
            "a stalled loop with audio arriving ended the over"
        );
        let MicTick::Samples(after) = res.tick else {
            panic!("nothing was fed after the stall: {:?}", res.tick);
        };
        // Fed in all, never more than real time since the audio began plus the look-ahead: the
        // stall's moments were skipped, not squeezed in afterwards.
        let fed = (fed_before.len() + after.len()) as u64;
        let real = MIC.samples(700 - MIC.lead_in_ms + MIC.ahead_ms);
        assert!(
            fed <= real,
            "fed {fed} samples by 700 ms; real time allows {real}"
        );
        assert!(
            after.len() as u64 <= MIC.samples(MIC.ahead_ms),
            "the first push after the stall caught up {} samples",
            after.len()
        );
    }

    // ── G1–G5, G7, C1, C2 ─────────────────────────────────────────────────────────────────────

    #[test]
    fn every_gate_drops_the_over_armed_or_keyed() {
        let t0 = Instant::now();
        for (name, gate, why) in [
            ("TX off", 0, MicDrop::GateDown),
            ("outside privileges", 1, MicDrop::GateDown),
            ("a tune carrier", 2, MicDrop::GateDown),
            ("left Phone", 3, MicDrop::GateDown),
            ("presence lapsed", 4, MicDrop::Presence),
            ("the route changed", 5, MicDrop::DeviceChanged),
        ] {
            for keyed_first in [false, true] {
                let mut latch = if keyed_first {
                    keyed(t0)
                } else {
                    let mut l = MicLatch::default();
                    assert!(l.arm());
                    assert_eq!(l.tick(up(t0), Vec::new(), PLENTY).tick, MicTick::Armed);
                    l
                };
                let mut g = up(t0 + ms(40));
                match gate {
                    0 => g.tx_enabled = false,
                    1 => g.tx_allowed = false,
                    2 => g.tuning = true,
                    3 => g.in_section = false,
                    4 => g.presence = false,
                    _ => g.route = 2,
                }
                let res = latch.tick(g, vec![frame(9, 480, g.now, 0.5)], PLENTY);
                assert_eq!(
                    res.drop,
                    Some(why),
                    "{name} (keyed: {keyed_first}): not dropped"
                );
                assert_eq!(
                    res.tick,
                    MicTick::Idle,
                    "{name} (keyed: {keyed_first}): kept going"
                );
                assert_eq!(
                    latch.drop_latch(),
                    keyed_first,
                    "{name}: the kill path must arm the abort exactly when the rig was keyed"
                );
                assert!(!latch.active());
            }
        }
    }

    #[test]
    fn the_ceiling_is_counted_from_the_key_and_audio_cannot_extend_it() {
        let t0 = Instant::now();
        let mut latch = keyed(t0);
        let key_ms = up(t0).now_ms;
        let mut g = up(t0 + ms(20));
        g.now_ms = key_ms + MIC.max_latch_ms - 1;
        let res = latch.tick(g, vec![frame(2, 240, g.now, 0.5)], PLENTY);
        assert_eq!(res.drop, None, "the over ended before its ceiling");
        g.now = t0 + ms(40);
        g.now_ms = key_ms + MIC.max_latch_ms;
        let res = latch.tick(g, vec![frame(3, 480, g.now, 0.5)], PLENTY);
        assert_eq!(
            res.drop,
            Some(MicDrop::Ceiling),
            "audio carried the over past its ceiling"
        );
    }

    #[test]
    fn the_watchdog_starts_with_the_keyed_over_and_trips_it() {
        let t0 = Instant::now();
        let mut latch = MicLatch::default();
        assert!(latch.arm());
        let mut g = up(t0);
        g.watchdog_limit_secs = 60;
        g.now_secs = 100;
        // An armed over transmits nothing: it does not start an unattended-transmit clock.
        assert_eq!(latch.tick(g, Vec::new(), PLENTY).watchdog_start, None);
        let res = latch.tick(g, vec![frame(1, 0, t0, 0.5)], PLENTY);
        assert_eq!(res.tick, MicTick::Key);
        let mut g2 = g;
        g2.now = t0 + ms(20);
        let res = latch.tick(g2, vec![frame(2, 240, g2.now, 0.5)], PLENTY);
        assert_eq!(
            res.watchdog_start,
            Some(100),
            "the keyed over did not start the clock"
        );
        let mut late = g2;
        late.now = t0 + ms(40);
        late.watchdog_start_secs = Some(100);
        late.now_secs = 160;
        let res = latch.tick(late, vec![frame(3, 480, late.now, 0.5)], PLENTY);
        assert_eq!(res.drop, Some(MicDrop::Watchdog));
        assert_eq!(res.tick, MicTick::Idle);
    }

    // ── M12: the backstops ────────────────────────────────────────────────────────────────────

    #[test]
    fn dropping_retires_the_epoch_and_nothing_from_the_dead_over_survives() {
        let t0 = Instant::now();
        let feed = MicFeed::default();
        feed.open();
        assert_eq!(feed.push(frame(1, 0, t0, 0.5)), Ok(()));
        let before = feed.epoch();
        feed.close();
        assert_ne!(feed.epoch(), before, "the drop did not move the epoch");
        assert!(feed.take().is_empty(), "a queued frame survived the drop");
        assert_eq!(
            feed.push(frame(2, 240, t0, 0.5)),
            Err(Refused::Closed),
            "a frame in flight from the dead over was taken"
        );
        // The next over starts clean: its first frame is the only thing it holds.
        feed.open();
        assert_eq!(
            feed.push(frame(1, 0, t0, 0.5)),
            Ok(()),
            "a re-armed over refused its first frame"
        );
        assert_eq!(feed.take().len(), 1);
    }
}
