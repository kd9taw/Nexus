//! An SDRplay receiver in Nexus's RF scope, through SDRplay's SDRconnect program: the scope source
//! "SDRconnect" (Settings ▸ Radio ▸ Scope source, off as it comes).
//!
//! ## What the operator sees
//! The receiver hangs off the rig's RX OUT (or a splitter with T/R protection) and SDRconnect runs
//! beside Nexus with its WebSocket API on. With the scope source set to SDRconnect, the RF scope
//! pane and the Phone and CW scopes draw the receiver's whole IQ span around its centre, each
//! station at its own RF frequency, with Nexus's usual overlays (the dial, the passband, spots).
//! The receiver's centre follows the rig's dial: on every dial change, or only when the dial
//! leaves the middle of the span ([`Follow`]). When SDRconnect is not there, not streaming or not
//! drawable, the scope's status line says which ([`State::problem`]) and Nexus keeps trying.
//!
//! ## The pieces
//! - **The radio loop** owns one [`SdrconnectScope`] while the operator's scope source is
//!   SDRconnect, and tells it the dial and whether the station is on the air each tick
//!   ([`SdrconnectScope::want`], a store under a lock nothing else holds for long). It never waits
//!   on the socket.
//! - **The scope's thread** ("sdrconnect-scope") owns the WebSocket ([`tempo_net::sdrconnect`]):
//!   it connects, reconnecting on a ladder ([`Times::ladder_ms`]) while SDRconnect is away; asks
//!   for the IQ; reads SDRconnect's reports and IQ; and every [`LOOK_MS`] retunes the receiver if
//!   the dial asks for it ([`target`], at most one retune per [`RETUNE_MS`], the latest always
//!   sent) and turns the IQ into rows. The IQ goes through TCI's panadapter intake and rows
//!   ([`crate::tci::iq`]): the same placement, the same gap, stall and settle rules, the same
//!   dB axis and the same bound on work ([`crate::tci::iq::MAX_WINDOWS`] FFTs a row whatever the
//!   rate), published to the feed's RF slot as [`SOURCE`].
//! - **The feed's RF slot is this source's** while it exists ([`SpectrumFeed::claim_rf`]): the
//!   radio's own panadapter may keep streaming, and is not shown.
//!
//! ## The rate is checked before anything is drawn
//! The specification does not say whether the IQ on the socket comes at the receiver's
//! `device_sample_rate` or decimated. Drawn at the wrong rate, every station would sit at the wrong
//! frequency, so the scope measures the rate the IQ actually arrives at against the wall clock
//! ([`RateMeter`], over [`METER_MS`]) and draws only while it agrees with the reported rate within
//! [`RATE_TOLERANCE`]; otherwise it says both rates and draws nothing.
//!
//! ## Receive only
//! Nothing here can transmit, key, or reach the rig. The type holds no radio, no CAT, no audio
//! stream: the only thing it sends is what [`tempo_net::sdrconnect::Outbound`] can say, which is a
//! property read, the receiver's centre frequency, the IQ stream on or off, and starting the
//! receiver's stream. The rig's dial is read, never written, here. A test reads this file and
//! fails on any name of the transmit or CAT paths.
//!
//! ## While transmitting
//! A receiver on RX OUT may go quiet or overload under the rig's own carrier. Neither a stall nor
//! its end is read from the stream while the station is on the air: a stall is counted from the
//! end of the transmission. Nothing is sent to SDRconnect at a transmission.
//!
//! Nexus's own design, not a port.

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tempo_app::engine::SpectrumFeed;
use tempo_app::settings::SdrconnectFollow;
use tempo_net::sdrconnect::{
    iq_pairs, iq_samples, parse_binary, parse_text, Binary, ConnectError, Incoming, Link, Outbound,
    Prop, Report, Text,
};

use crate::tci::iq::{Counts, Geometry, Intake, Rows, Seen, SCALE};
use crate::tci::now_ms;

#[cfg(test)]
pub(crate) mod fake;

/// The feed label for rows from this source.
///
/// ⚠️ MUST be in `isRfScopeSource` in `ui/src/waterfall.ts`: an unknown label is drawn as audio
/// against a 0–4000 Hz axis, with nothing erroring.
pub const SOURCE: &str = "sdrconnect";

/// How the receiver's centre follows the dial: the operator's setting.
pub type Follow = SdrconnectFollow;

/// How often the scope's thread looks at the dial, the IQ held and the clock.
pub const LOOK_MS: u64 = 20;

/// At most one retune in this long: ten a second while the knob turns, the latest always sent.
pub const RETUNE_MS: u64 = 100;

/// In [`Follow::Scroll`], the share of the span around its centre the dial may move in without a
/// retune.
pub const SCROLL_MIDDLE: f64 = 0.8;

/// The IQ's rate is measured over windows this long.
pub const METER_MS: u64 = 1_000;

/// No IQ for this long breaks a measurement window: a pause, not a rate.
pub const METER_PAUSE_MS: u64 = 250;

/// How far the measured rate may be from the reported one: arrival jitter is far smaller, a
/// decimated stream (a half or less) far larger.
pub const RATE_TOLERANCE: f64 = 0.2;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The windows the scope runs on, which the tests shorten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Times {
    /// After connecting, how long SDRconnect may take to send IQ before the scope says it sends
    /// none.
    pub(crate) start_window_ms: u64,
    /// No IQ kept for this long (off the air): the stream stopped.
    pub(crate) stall_ms: u64,
    /// The waits between connection attempts, in order; the last repeats. Back to the first once
    /// a connection has drawn.
    pub(crate) ladder_ms: [u64; 4],
}

impl Default for Times {
    fn default() -> Times {
        Times {
            start_window_ms: crate::tci::iq::START_WINDOW_MS,
            stall_ms: crate::tci::iq::STALL_MS,
            ladder_ms: [1_000, 2_000, 5_000, 10_000],
        }
    }
}

/// Where the scope is, for the operator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    /// Before the first connection attempt has finished.
    #[default]
    Connecting,
    /// Nothing answered at the address (SDRconnect is not running there, or its API is off), or
    /// the name did not resolve.
    NotAnswering(String),
    /// Something answered, but not as a WebSocket server.
    NotSdrconnect(String),
    /// The connection ended; Nexus reconnects by itself.
    Lost(String),
    /// Connected, and no IQ yet.
    Waiting,
    /// Connected, and no IQ in the start window: no receiver is streaming.
    NoIq,
    /// IQ arrives, but none of it can be drawn.
    Undrawable(Counts),
    /// IQ arrives at a rate other than the one SDRconnect reports (Hz each): drawn, every station
    /// would be at the wrong frequency.
    RateMismatch { reported: u32, measured: u32 },
    /// IQ arrived and then stopped.
    Stalled,
    /// Rows are being drawn.
    Drawing,
}

impl State {
    /// What the scope's status line says, or `None` when there is nothing to say. `at` is the
    /// address as the operator set it.
    pub fn problem(&self, at: &str) -> Option<String> {
        match self {
            State::Connecting | State::Waiting | State::Drawing => None,
            State::NotAnswering(why) => Some(format!(
                "SDRconnect is not answering at {at} ({why}). Start SDRplay's SDRconnect on that \
                 computer with its WebSocket API turned on, or check the address and port in \
                 Settings ▸ Radio ▸ Scope source. Nexus keeps trying."
            )),
            State::NotSdrconnect(why) => Some(format!(
                "Something at {at} answered, but not as SDRconnect's WebSocket API ({why}). Check \
                 the port in Settings ▸ Radio ▸ Scope source."
            )),
            State::Lost(why) => Some(format!(
                "The connection to SDRconnect at {at} ended ({why}). Nexus reconnects by itself."
            )),
            State::NoIq => Some(
                "SDRconnect is connected but sends no IQ. Pick the receiver in SDRconnect and \
                 start it."
                    .to_string(),
            ),
            State::Undrawable(counts) => Some(if counts.unplaced > 0 {
                "SDRconnect sends IQ but has not said the receiver's centre frequency, so it \
                 cannot be placed on the scope."
                    .to_string()
            } else if counts.malformed > 0 {
                "SDRconnect sends IQ the scope cannot read: no sample rate, or messages \
                 that are not whole I/Q pairs."
                    .to_string()
            } else {
                format!(
                    "SDRconnect's IQ arrives slower than the sample rate it reports, so samples \
                     are missing and nothing is drawn ({counts})."
                )
            }),
            State::RateMismatch { reported, measured } => Some(format!(
                "SDRconnect's IQ arrives at about {measured} samples a second but it reports a \
                 sample rate of {reported} Hz, so stations would be drawn at the wrong \
                 frequencies. Nothing is drawn."
            )),
            State::Stalled => {
                Some("SDRconnect stopped sending IQ. Nexus keeps listening.".to_string())
            }
        }
    }
}

/// What the scope says about itself, for the radio loop, the bench and the tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    /// Rows published.
    pub rows: u64,
    /// Retunes sent.
    pub retunes: u64,
    /// The last centre frequency SDRconnect reported (Hz).
    pub centre: Option<u64>,
    /// The last sample rate SDRconnect reported (Hz).
    pub rate: Option<u32>,
    /// The rate the IQ was last measured arriving at (pairs a second).
    pub measured_rate: Option<u32>,
    /// The API version SDRconnect reported.
    pub api_version: Option<String>,
    /// Whether SDRconnect lets this client set the receiver.
    pub can_control: Option<bool>,
    /// What the intake did with the IQ, this connection.
    pub counts: Counts,
    /// Where the IQ held sits.
    pub geometry: Option<Geometry>,
    /// Windows past the bound in a row, not computed.
    pub skipped: u64,
    /// Binary messages of other types (audio, display bins, the secondary receiver), not used.
    pub other: u64,
    /// Connections made.
    pub connections: u64,
}

/// The radio loop's word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Want {
    /// The rig's dial (Hz); 0, not known.
    dial_hz: u64,
    follow: Option<Follow>,
    transmitting: bool,
}

struct Shared {
    want: Mutex<Want>,
    status: Mutex<Status>,
    quit: AtomicBool,
    /// Wakes the thread from a wait between connection attempts: its teardown.
    sleep: Mutex<()>,
    wake: Condvar,
    times: Times,
}

impl Shared {
    fn quitting(&self) -> bool {
        self.quit.load(Ordering::Acquire)
    }

    /// Wait `ms`, or until teardown.
    fn wait(&self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        let mut guard = lock(&self.sleep);
        while !self.quitting() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            guard = self
                .wake
                .wait_timeout(guard, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn set_state(&self, state: State) {
        lock(&self.status).state = state;
    }
}

/// The rate the IQ arrives at, against the wall clock, for the reported rate it came under.
#[derive(Debug, Default)]
pub(crate) struct RateMeter {
    /// The window's start, and the pairs that arrived after it.
    from: Option<u64>,
    pairs: u64,
    /// When IQ last arrived.
    last: u64,
    /// The reported rate the measurement is for.
    reported: Option<u32>,
    measured: Option<u32>,
}

impl RateMeter {
    /// `pairs` arrived at `now` under the `reported` rate. The pairs of the message that opens a
    /// window arrived before it and are not counted.
    pub(crate) fn push(&mut self, pairs: usize, reported: Option<u32>, now: u64) {
        if reported != self.reported {
            self.reported = reported;
            self.measured = None;
            self.from = None;
        }
        let paused = now.saturating_sub(self.last) > METER_PAUSE_MS;
        self.last = now;
        match self.from {
            Some(from) if !paused => {
                self.pairs = self.pairs.saturating_add(pairs as u64);
                let span = now.saturating_sub(from);
                if span >= METER_MS {
                    let rate = self.pairs.saturating_mul(1_000) / span;
                    self.measured = Some(u32::try_from(rate).unwrap_or(u32::MAX));
                    self.from = Some(now);
                    self.pairs = 0;
                }
            }
            _ => {
                self.from = Some(now);
                self.pairs = 0;
            }
        }
    }

    /// The last measurement.
    pub(crate) fn measured(&self) -> Option<u32> {
        self.measured
    }

    /// Whether the IQ arrives at the reported rate: `None` until measured.
    pub(crate) fn agrees(&self) -> Option<bool> {
        let (reported, measured) = (self.reported?, self.measured?);
        let off = (f64::from(measured) - f64::from(reported)).abs() / f64::from(reported.max(1));
        Some(off <= RATE_TOLERANCE)
    }
}

/// Where the receiver's centre should be, given the dial, where it is and its span: `None` when
/// the dial is not known.
pub(crate) fn target(
    follow: Follow,
    dial: u64,
    centre: Option<u64>,
    rate: Option<u32>,
) -> Option<u64> {
    if dial == 0 {
        return None;
    }
    match (follow, centre, rate) {
        (Follow::Scroll, Some(c), Some(r)) => {
            let half = f64::from(r) * SCROLL_MIDDLE / 2.0;
            Some(if (dial as f64 - c as f64).abs() <= half {
                c
            } else {
                dial
            })
        }
        _ => Some(dial),
    }
}

/// The SDRconnect scope source: one thread on one SDRconnect, for as long as it is held.
///
/// Dropping it tells the thread to quit and does not wait for it: the radio loop drops it when the
/// operator changes the address, and the thread may be in a connect for up to
/// [`tempo_net::sdrconnect::CONNECT_TIMEOUT`]. The thread then ends by itself, closing its socket,
/// and publishes nothing more to the feed.
pub struct SdrconnectScope {
    shared: Arc<Shared>,
    feed: SpectrumFeed,
}

impl SdrconnectScope {
    /// Start drawing SDRconnect at `host`:`port` on `feed`, whose RF slot it takes. `None` when the
    /// thread could not be started.
    pub fn start(host: &str, port: u16, feed: &SpectrumFeed) -> Option<SdrconnectScope> {
        SdrconnectScope::start_with(host, port, feed, Times::default())
    }

    pub(crate) fn start_with(
        host: &str,
        port: u16,
        feed: &SpectrumFeed,
        times: Times,
    ) -> Option<SdrconnectScope> {
        let shared = Arc::new(Shared {
            want: Mutex::new(Want::default()),
            status: Mutex::new(Status::default()),
            quit: AtomicBool::new(false),
            sleep: Mutex::new(()),
            wake: Condvar::new(),
            times,
        });
        feed.claim_rf(Some(SOURCE));
        let (thread_shared, thread_feed, host) = (shared.clone(), feed.clone(), host.to_string());
        let thread = std::thread::Builder::new()
            .name("sdrconnect-scope".into())
            .spawn(move || run(&thread_shared, &host, port, &thread_feed));
        match thread {
            Ok(_) => Some(SdrconnectScope {
                shared,
                feed: feed.clone(),
            }),
            Err(e) => {
                feed.claim_rf(None);
                tempo_core::applog::warn(
                    "cat",
                    &format!("SDRconnect: the scope could not start its thread ({e})"),
                );
                None
            }
        }
    }

    /// The radio loop's word, each tick: the rig's dial (Hz, 0 when not known), how the receiver
    /// follows it, and whether the station is on the air.
    pub fn want(&self, dial_hz: u64, follow: Follow, transmitting: bool) {
        *lock(&self.shared.want) = Want {
            dial_hz,
            follow: Some(follow),
            transmitting,
        };
    }

    /// Where the scope is.
    pub fn status(&self) -> Status {
        lock(&self.shared.status).clone()
    }
}

impl Drop for SdrconnectScope {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        {
            let _guard = lock(&self.shared.sleep);
            self.shared.wake.notify_all();
        }
        // Not joined: the thread looks at its quit flag every LOOK_MS and ends by itself.
        self.feed.claim_rf(None);
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, ConnectError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| ConnectError::Unreachable(format!("{host} does not resolve: {e}")))?
        .next()
        .ok_or_else(|| ConnectError::Unreachable(format!("{host} does not resolve")))
}

/// The scope's thread: connect, draw, and on the ladder again, until teardown.
fn run(shared: &Shared, host: &str, port: u16, feed: &SpectrumFeed) {
    let ladder = shared.times.ladder_ms;
    let mut rung = 0;
    while !shared.quitting() {
        match resolve(host, port).and_then(Link::connect) {
            Err(ConnectError::Unreachable(why)) => shared.set_state(State::NotAnswering(why)),
            Err(ConnectError::Handshake(why)) => shared.set_state(State::NotSdrconnect(why)),
            Ok(link) => {
                lock(&shared.status).connections += 1;
                let (drew, why) = session(shared, link, feed);
                if shared.quitting() {
                    break;
                }
                shared.set_state(State::Lost(why));
                if drew {
                    rung = 0;
                }
            }
        }
        shared.wait(ladder[rung.min(ladder.len() - 1)]);
        rung += 1;
    }
}

/// One connection, until it ends or teardown: whether it drew, and why it ended.
fn session(shared: &Shared, mut link: Link, feed: &SpectrumFeed) -> (bool, String) {
    let times = shared.times;
    let mut intake = Intake::new(times.stall_ms);
    intake.accept(true);
    let mut rows = Rows::labelled(SOURCE);
    let mut meter = RateMeter::default();
    let mut samples: Vec<f32> = Vec::new();
    let opened = now_ms();
    let mut centre: Option<u64> = None;
    let mut rate: Option<u32> = None;
    let mut can_control: Option<bool> = None;
    let mut asked_start = false;
    let mut last_sent: Option<u64> = None;
    let mut sent_at: Option<u64> = None;
    let mut quiet_from = opened;
    let mut drawing = false;
    let mut drew = false;
    let mut other = 0u64;
    let mut next_look = opened;
    {
        let mut status = lock(&shared.status);
        status.state = State::Waiting;
        status.counts = Counts::default();
        status.geometry = None;
    }
    let hello = Prop::ALL
        .into_iter()
        .map(Outbound::Get)
        .chain([Outbound::IqStream(true)]);
    for message in hello {
        if let Err(why) = link.send(&message) {
            return (false, why);
        }
    }
    let end = loop {
        if shared.quitting() {
            let _ = link.send(&Outbound::IqStream(false));
            link.close();
            break String::new();
        }
        let wait = next_look.saturating_sub(now_ms()).max(1);
        match link.read(Duration::from_millis(wait)) {
            Ok(Some(Incoming::Text(text))) => match parse_text(&text) {
                Text::Report(Report::Centre(hz)) => centre = Some(hz),
                Text::Report(Report::Rate(hz)) => rate = Some(hz),
                Text::Report(Report::CanControl(yes)) => can_control = Some(yes),
                Text::Report(Report::ApiVersion(v)) => lock(&shared.status).api_version = Some(v),
                Text::Report(Report::Started(false)) if !asked_start => {
                    // Headless SDRconnect starts with its receiver stopped: start it, once.
                    asked_start = true;
                    if let Err(why) = link.send(&Outbound::StartStream) {
                        break why;
                    }
                }
                Text::Report(Report::Started(_)) | Text::Other | Text::Malformed => {}
            },
            Ok(Some(Incoming::Binary(bytes))) => match parse_binary(&bytes) {
                Binary::Iq(payload) => {
                    let now = now_ms();
                    meter.push(iq_pairs(payload), rate, now);
                    intake.push_pairs(
                        rate,
                        centre.and_then(|c| i64::try_from(c).ok()),
                        iq_pairs(payload),
                        iq_samples(payload),
                        now,
                    );
                }
                Binary::Malformed => intake.push_pairs(None, None, 0, std::iter::empty(), now_ms()),
                Binary::Other(_) => other += 1,
            },
            Ok(None) => {}
            Err(why) => break why,
        }
        let now = now_ms();
        if now < next_look {
            continue;
        }
        next_look = now + LOOK_MS;
        let want = *lock(&shared.want);
        // Follow the dial: the latest target, at most once a RETUNE_MS, and not when SDRconnect
        // says this client may not set the receiver.
        if let Some(follow) = want.follow {
            let goal = target(follow, want.dial_hz, centre, rate);
            let due = sent_at.is_none_or(|at| now.saturating_sub(at) >= RETUNE_MS);
            if let Some(goal) = goal.filter(|g| Some(*g) != centre && Some(*g) != last_sent) {
                if due && can_control != Some(false) {
                    if let Err(why) = link.send(&Outbound::SetCentre(goal)) {
                        break why;
                    }
                    last_sent = Some(goal);
                    sent_at = Some(now);
                    lock(&shared.status).retunes += 1;
                }
            }
        }
        // Under a transmission the receiver may go quiet: a stall is counted from its end.
        if want.transmitting {
            quiet_from = now;
        }
        let seen: Seen = intake.seen();
        let state = match seen.any {
            None if now.saturating_sub(opened) >= times.start_window_ms => State::NoIq,
            None => State::Waiting,
            Some(any) => {
                let last = seen.kept.unwrap_or(0).max(quiet_from);
                let quiet = seen.kept.is_none() || now.saturating_sub(last) >= times.stall_ms;
                if !quiet {
                    match (meter.agrees(), rate, meter.measured()) {
                        (Some(true), _, _) => State::Drawing,
                        (Some(false), Some(reported), Some(measured)) => {
                            State::RateMismatch { reported, measured }
                        }
                        // Not measured yet: nothing drawn until the rate is known to be right.
                        _ => State::Waiting,
                    }
                } else if seen.kept.is_none() || any > last {
                    if seen.kept.is_none() && now.saturating_sub(opened) < times.start_window_ms {
                        State::Waiting
                    } else {
                        State::Undrawable(intake.counts())
                    }
                } else {
                    State::Stalled
                }
            }
        };
        let taken = intake.take(&mut samples);
        if state == State::Drawing {
            rows.feed(&taken, &samples);
            if let Some(row) = rows.row(now) {
                if !shared.quitting() {
                    feed.publish_rf_frame(row, SCALE, None);
                    lock(&shared.status).rows += 1;
                    drew = true;
                }
            }
        } else {
            rows.restart();
            if drawing && !shared.quitting() {
                // The rows left in the slot are no longer the band.
                feed.clear_rf_of(SOURCE);
            }
        }
        drawing = state == State::Drawing;
        let mut status = lock(&shared.status);
        status.state = state;
        status.centre = centre;
        status.rate = rate;
        status.measured_rate = meter.measured();
        status.can_control = can_control;
        status.counts = intake.counts();
        status.geometry = intake.geometry();
        status.skipped = rows.skipped;
        status.other = other;
    };
    // At teardown the slot was handed back already, and may be the next scope's.
    if !shared.quitting() {
        feed.clear_rf_of(SOURCE);
    }
    (drew, end)
}

#[cfg(test)]
mod tests;
