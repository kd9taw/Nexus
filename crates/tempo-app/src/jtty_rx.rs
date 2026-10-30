//! JTTY receive — the engine side of WSJT-X's keyboard mode, RECEIVE ONLY. A CHILD module of
//! `engine` (declared with `#[path]` there, as `js8` is), so it reaches the engine's private
//! fields without widening any visibility.
//!
//! ⛔ NOTHING HERE TRANSMITS, AND NOTHING HERE MAY. JTTY's transmitter is `jtty_tx`, behind
//! every transmit guard; this file decodes, shows, logs and reports what was heard, and shows what
//! the transmitter sent. While JTTY transmits the tap stops and the reception ends (WSJT-X ends
//! it on Transmission), and a message decoded inside our own keyed time is our own echo: it is
//! shown, but never counts toward the two-decode rule.
//!
//! THE SHAPE — the armed-decoder pattern PSK and RTTY use, on JTTY's sample timeline:
//! - While armed, [`Engine::feed_rx_audio`] taps the radio loop's RX audio into [`JttyRx`] as
//!   16-bit PCM belonging to a RECEPTION (WSJT-X's receive context): a reception starts when
//!   the decoder is armed, and a new one starts when the dial moves (WSJT-X starts a new context
//!   on an RF retune, `MainWindow::updateJttyReceiveContext`) and when the decode thread has
//!   fallen so far behind that the tap overflows (WSJT-X's overrun: the audio between is lost,
//!   and a fresh reception beats decoding across a hole).
//! - [`JttyDriver`] runs on the decode thread (tempo-audio `jttyrx`). It takes the audio under a
//!   brief lock, runs [`jtty::Live`] OFF the lock (WSJT-X's `MainWindow::pumpJttyReceive`: one
//!   quarter-frame step per call, a frame and a step of audio, the history the retro searches
//!   need), and hands the updates back to [`Engine::apply_jtty_updates`].
//! - The updates become the receive window's lines and ALL.TXT `Rx` lines, both exactly as
//!   WSJT-X makes them (`JttyReceiveResultController.cpp`, `widgets/mainwindow.cpp`
//!   `write_all`), and then — the one deliberate difference — the outward and alerting actions.
//!
//! ⭐ THE TWO-DECODE RULE (operator ruling, 2026-10-12). JTTY's frames are guarded by a CRC-12
//! and the grammar, and that lets false decodes through: about 0.7 an hour on noise, a few more
//! beside strong signals and in crowds, some of them well-formed fabricated calls (`TU 0C9GFW CQ`,
//! `CQ DA2PKN CQ`), the same in stock WSJT-X (parity-lab measurements, 2026-10-09). WSJT-X
//! spots such a line to PSK Reporter. Here the receive window and ALL.TXT stay at WSJT-X's
//! parity, but a PSK Reporter spot, a station on the Needed board and the calls a lookup may be
//! offered for all wait until that call has been decoded in TWO SEPARATE MESSAGES (two message
//! ids) starting within [`CONFIRM_WINDOW_MS`] of each other. A CRC coincidence does not repeat a
//! call; a station on the air does.
//!
//! Only a call in the shape a JTTY call atom carries counts ([`jtty_call`]): WSJT-X's own strict
//! standard-call form with a letter after the digit. WSJT-X's looser test (`is_standard_callsign`)
//! also passes exchange tokens such as `1D` and `5NN`, which would let an ordinary Field Day
//! exchange (`K1ABC 1D EMA`) spot "1D"; the stricter test only ever refuses more.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use jtty::{SearchBand, Terminal, Update};

/// The modem's update record and its terminal states, as [`Engine::apply_jtty_updates`] takes
/// them, so a caller outside this crate needs no `jtty` dependency of its own.
pub use jtty::{Terminal as JttyTerminal, Update as JttyUpdate};

use super::{engine_lock, now_unix_millis, Engine};
use crate::dto::{JttyHeard, JttyLine, JttyState};

/// WSJT-X's JTTY receive defaults: Rx 1500 Hz (set again whenever JTTY is selected,
/// `on_actionJTTY_triggered`), F Tol 20 Hz (`JTTY_FTol` when its settings have none), and its
/// receive span's 200-2800 Hz before the cockpit reports its waterfall's range.
pub const JTTY_RX_HZ: f32 = 1500.0;
pub const JTTY_FTOL_HZ: f32 = 20.0;
pub const JTTY_LOW_HZ: i32 = 200;
pub const JTTY_HIGH_HZ: i32 = 2800;

/// How far apart two messages naming a call may start and still confirm it: two minutes.
///
/// WHY TWO MINUTES. What the window has to admit is a real station repeating its call, and JTTY
/// is RTTY-style operating: a run station's CQ comes round every few seconds to half a minute,
/// an exchange answers within seconds, and a ragchew names both calls at least once an over.
/// What it has to refuse is a false decode recurring, and a fabricated call is one of some 2^28
/// standard calls: at the measured 0.7 false decodes an hour, two naming the same call within
/// any window of minutes is not going to happen. So the window can be generous without letting
/// anything false through, and it is the board's own: a station "decoded by your radio" counts
/// as on the air for two minutes there (the Needed board's own-decode age), so a confirmation
/// never rests on a decode the board would already have let go.
pub const CONFIRM_WINDOW_MS: i64 = 120_000;

/// The tap's ceiling (~10 s at 12 kHz, the other decoders' `RX_TAP_CAP`). The decode thread
/// empties it every ~100 ms; reaching it means the thread stalled, and the reception restarts.
const JTTY_TAP_CAP: usize = 120_000;
/// Receive-window lines kept, oldest dropped.
const JTTY_LINES_CAP: usize = 500;
/// PSK Reporter spots waiting for the radio loop, oldest dropped.
const JTTY_SPOTS_CAP: usize = 256;
/// Receptions remembered for updates still in flight (the one feeding and those it replaced).
const JTTY_SESSIONS_KEPT: usize = 8;
/// How long a confirmed call stays known without being heard again.
const CONFIRMED_KEEP_MS: i64 = 6 * 60 * 60 * 1000;

/// What [`Engine::jtty_state`] reports as heard: the confirmed stations of the last ten minutes.
const HEARD_SHOWN_MS: i64 = 10 * 60 * 1000;

/// The receive audio kept for decoding again: WSJT-X's `Jtty::ReceiveHistory::capacity`, 180 s
/// at 12 kHz over every reception it still holds (3.3.0-beta4 `JttyReceiveHistory.hpp` 13).
const JTTY_HISTORY_SAMPLES: usize = 180 * 12_000;
/// The kept audio is held in pieces of at most a second, so letting the oldest go never moves
/// more than a second of samples (it runs under the engine lock, on every feed once full).
const JTTY_HISTORY_PIECE: usize = 12_000;
/// What a double-click on a waterfall row decodes again: from 5 s before that row, 40 s of it
/// (`MainWindow::startJttyReview`, 3.3.0-beta4 `widgets/mainwindow_jtty_receive.cpp` 685-728).
const JTTY_REVIEW_BEFORE_SAMPLES: i64 = 5 * 12_000;
const JTTY_REVIEW_SAMPLES: i64 = 40 * 12_000;
/// How much of a review the decode thread runs per tick (2 s of audio), so a long review never
/// holds up the live reception it shares the thread with.
const JTTY_REVIEW_CHUNK: usize = 2 * 12_000;
/// WSJT-X's words for a review (`startJttyReview`).
const JTTY_REVIEW_STARTED: &str =
    "Review uses retained audio; earlier message context may be missing.";
const JTTY_REVIEW_GONE: &str = "Audio for this waterfall row is no longer available.";
const JTTY_REVIEW_NOTHING: &str = "There is no receive audio to decode again yet.";

/// What WSJT-X says when receive audio was lost and decoding restarted.
const JTTY_AUDIO_LOST: &str = "JTTY receive audio was lost; decoding restarted.";
/// …and when the modem has no receive context free (it has eight).
const JTTY_NO_CONTEXT: &str = "JTTY cannot decode: the modem has no free receive context.";

/// One reception: WSJT-X's receive context and its span.
#[derive(Debug, Clone)]
struct Session {
    id: u64,
    /// UTC milliseconds of its sample 0.
    anchor_ms: i64,
    /// The dial it was received on, Hz (WSJT-X's `periodFrequency`).
    dial_hz: u64,
    /// Where it was searched when it began: what WSJT-X searches and admits by for a reception
    /// that is no longer the current one (`ReceiveSpan::center/tolerance/low/high`).
    band: SearchBand,
    /// Samples tapped into it so far: where the next one falls on its timeline.
    fed: i64,
}

/// One message in the receive window.
#[derive(Debug, Clone)]
struct Line {
    id: i64,
    start_ms: i64,
    freq_hz: f32,
    dial_hz: u64,
    snr: i32,
    text: String,
    terminal: Terminal,
    rx: bool,
    /// A message this station sent (`Engine::jtty_segment_started`).
    tx: bool,
    /// The review (decode again) that found it, numbered from 1; `None` for live reception.
    review: Option<u64>,
    /// Which of that review's decoders found it (one per reception): message numbers are each
    /// decoder's own. 0 for live reception.
    review_job: u64,
}

/// A stretch of retained receive audio decoded again (WSJT-X's review job): taken by the decode
/// thread, decoded by a receiver of its own, and shown under a heading. Never logged to ALL.TXT,
/// spotted or counted toward the two-decode rule, as WSJT-X's review is neither logged nor
/// spotted (`ReceiveResultController::apply`, `Source::Review`).
#[derive(Debug, Clone, PartialEq)]
pub struct JttyReview {
    pub of: JttyReviewOf,
    pub pcm: Vec<i16>,
}

/// Which review a decode belongs to, and where its audio sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JttyReviewOf {
    /// The review's number: its lines are shown together under one heading. One press of
    /// Decode is one review, however many receptions it decodes again.
    pub group: u64,
    /// This reception's decoder within the review, numbered across reviews: its lines are told
    /// apart from another decoder's by it, as message numbers are each decoder's own.
    pub job: u64,
    /// UTC milliseconds of the review audio's first sample, and the dial it was received on.
    pub anchor_ms: i64,
    pub dial_hz: u64,
    /// Where to search it: the operator's selection when it was asked for, as WSJT-X's.
    pub band: SearchBand,
}

/// A spot for PSK Reporter, as WSJT-X builds one for a JTTY message (`Results::Spot`): the
/// sender, the dial plus the audio frequency, the SNR, the message's start.
#[derive(Debug, Clone, PartialEq)]
pub struct JttySpot {
    pub call: String,
    pub freq_hz: u64,
    pub snr: i32,
    /// Unix seconds of the message's first frame.
    pub time_secs: u32,
    /// The sender's grid, when the message's third word is one (`CQ W1AW FN31`): WSJT-X spots
    /// it with the grid (`JttyReceiveResultController.cpp` 76-79, 3.3.0-beta4).
    pub grid: Option<String>,
}

/// The newest sender-shaped message for a call (the board's heard station, once confirmed).
#[derive(Debug, Clone, PartialEq)]
struct Sender {
    freq_hz: u64,
    snr: i32,
    grid: Option<String>,
    at_ms: i64,
}

/// Which calls have been decoded in two separate messages a short time apart.
#[derive(Debug, Default)]
struct Confirm {
    /// Per call not yet confirmed: the messages it was decoded in, `(message id, start ms)`.
    seen: HashMap<String, Vec<(i64, i64)>>,
    /// Confirmed calls, with when they were last decoded.
    confirmed: HashMap<String, i64>,
}

impl Confirm {
    fn is_confirmed(&self, call: &str) -> bool {
        self.confirmed.contains_key(call)
    }

    /// `call` was decoded in message `id`, which started at `at_ms`. True when this confirms it:
    /// another message, a different id, named it within [`CONFIRM_WINDOW_MS`].
    fn note(&mut self, call: &str, id: i64, at_ms: i64) -> bool {
        if let Some(last) = self.confirmed.get_mut(call) {
            *last = (*last).max(at_ms);
            return false;
        }
        let seen = self.seen.entry(call.to_string()).or_default();
        if seen.iter().any(|&(i, _)| i == id) {
            return false;
        }
        seen.retain(|&(_, t)| (at_ms - t).abs() <= CONFIRM_WINDOW_MS);
        if seen.is_empty() {
            seen.push((id, at_ms));
            self.prune(at_ms);
            return false;
        }
        self.seen.remove(call);
        self.confirmed.insert(call.to_string(), at_ms);
        true
    }

    /// Let go of what can no longer matter: a single sighting older than the window, a
    /// confirmed call not decoded for hours. Only when the maps have grown, so it is cheap.
    fn prune(&mut self, now_ms: i64) {
        if self.seen.len() > 1024 {
            self.seen
                .retain(|_, v| v.iter().any(|&(_, t)| now_ms - t <= CONFIRM_WINDOW_MS));
        }
        if self.confirmed.len() > 4096 {
            self.confirmed
                .retain(|_, &mut t| now_ms - t <= CONFIRMED_KEEP_MS);
        }
    }
}

/// The engine's JTTY receive state. Session-only: nothing here is persisted.
#[derive(Debug)]
pub struct JttyRx {
    /// The decoder is armed. The app never launches armed.
    armed: bool,
    /// The operator explicitly stopped the decoder this session, so entering the JTTY view must
    /// not restart it behind them (the APRS/SSTV/PSK decline memory).
    auto_arm_declined: bool,
    /// The decoder is running because the operator opened the JTTY screen, not because they
    /// pressed Arm (`Engine::leave_decoder_screen` stops only such an arm).
    auto_armed: bool,
    rx_hz: f32,
    ftol_hz: f32,
    low_hz: i32,
    high_hz: i32,
    /// Receptions updates may still arrive for, oldest first.
    sessions: VecDeque<Session>,
    next_session: u64,
    /// The last of `sessions` is the one the tap feeds. False until the first audio after an
    /// arm, and after a disarm.
    feeding: bool,
    /// Audio not yet taken by the decode thread, per reception, oldest first.
    audio: Vec<(u64, Vec<i16>)>,
    lines: VecDeque<Line>,
    confirm: Confirm,
    /// Per call, its newest sender-shaped message.
    senders: HashMap<String, Sender>,
    /// Per call not yet confirmed, the spot its newest complete sender-shaped message would
    /// have made: sent if the call is confirmed while it is still recent.
    held_spots: HashMap<String, JttySpot>,
    /// Spots for PSK Reporter, taken by the radio loop.
    spots: Vec<JttySpot>,
    notice: Option<String>,
    /// Our own keyed stretches, each widened by a frame (UTC ms): a message that starts inside
    /// one is our own transmission decoded back, never a sighting.
    keyed: VecDeque<(i64, i64)>,
    /// The id the next sent line takes: negative, so it never meets a decoder's message id.
    next_sent_id: i64,
    /// The receive audio kept for decoding again, oldest first: (reception, its first sample,
    /// the samples), at most [`JTTY_HISTORY_SAMPLES`] in all.
    history: VecDeque<(u64, i64, Vec<i16>)>,
    /// Reviews asked for and not yet taken by the decode thread, and the last review's number.
    reviews: Vec<JttyReview>,
    last_review: u64,
    /// The last review decoder's number ([`JttyReviewOf::job`]).
    last_job: u64,
}

impl Default for JttyRx {
    fn default() -> Self {
        Self {
            armed: false,
            auto_arm_declined: false,
            auto_armed: false,
            rx_hz: JTTY_RX_HZ,
            ftol_hz: JTTY_FTOL_HZ,
            low_hz: JTTY_LOW_HZ,
            high_hz: JTTY_HIGH_HZ,
            sessions: VecDeque::new(),
            next_session: 0,
            feeding: false,
            audio: Vec::new(),
            lines: VecDeque::new(),
            confirm: Confirm::default(),
            senders: HashMap::new(),
            held_spots: HashMap::new(),
            spots: Vec::new(),
            notice: None,
            keyed: VecDeque::new(),
            next_sent_id: -1,
            history: VecDeque::new(),
            reviews: Vec::new(),
            last_review: 0,
            last_job: 0,
        }
    }
}

impl JttyRx {
    /// The receive state at launch, with the F Tol the operator last set (Settings
    /// `jtty_ftol_hz`, WSJT-X's `JTTY_FTol`), held to the range [`Engine::set_jtty_rx`] accepts.
    pub(super) fn with_ftol(ftol_hz: u32) -> Self {
        Self {
            ftol_hz: ftol_hz.clamp(1, 500) as f32,
            ..Self::default()
        }
    }

    /// Where the current reception searches: the operator's selection now.
    fn band(&self) -> SearchBand {
        SearchBand {
            low_hz: self.low_hz,
            high_hz: self.high_hz,
            center_hz: self.rx_hz,
            tolerance_hz: self.ftol_hz,
        }
    }

    fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// The reception the tap is feeding, if any.
    fn current(&self) -> Option<&Session> {
        self.sessions.back().filter(|_| self.feeding)
    }

    /// The decode thread lost reception `session`: if the tap is still feeding it, drop what is
    /// buffered for it and let the next audio start a new reception.
    fn restart(&mut self, session: u64) {
        if self.current().map(|s| s.id) == Some(session) {
            self.feeding = false;
            self.audio.retain(|(s, _)| *s != session);
        }
    }

    /// A stretch of our own transmission, `from_ms..=to_ms` (already widened), for the echo rule.
    /// Stretches older than the confirmation window can no longer matter and are let go.
    pub(super) fn note_keyed(&mut self, from_ms: i64, to_ms: i64) {
        self.keyed.push_back((from_ms, to_ms));
        while self
            .keyed
            .front()
            .is_some_and(|&(_, end)| to_ms - end > 2 * CONFIRM_WINDOW_MS)
        {
            self.keyed.pop_front();
        }
    }

    /// Whether a message starting at `start_ms` started inside our own keyed time.
    fn in_keyed(&self, start_ms: i64) -> bool {
        self.keyed
            .iter()
            .any(|&(from, to)| (from..=to).contains(&start_ms))
    }

    /// What this station sent, shown as WSJT-X shows it when it starts on the air: a line in the
    /// QSO Frequency pane at the Tx offset, its SNR column blank.
    pub(super) fn push_sent(&mut self, at_ms: i64, dial_hz: u64, tx_hz: f32, text: &str) {
        let id = self.next_sent_id;
        self.next_sent_id -= 1;
        self.lines.push_back(Line {
            id,
            start_ms: at_ms,
            freq_hz: tx_hz,
            dial_hz,
            snr: 0,
            text: text.to_string(),
            terminal: Terminal::Complete,
            rx: true,
            tx: true,
            review: None,
            review_job: 0,
        });
        cap_lines(&mut self.lines, false);
    }

    fn start_session(&mut self, anchor_ms: i64, dial_hz: u64) -> u64 {
        self.next_session += 1;
        let session = Session {
            id: self.next_session,
            anchor_ms,
            dial_hz,
            band: self.band(),
            fed: 0,
        };
        self.sessions.push_back(session);
        while self.sessions.len() > JTTY_SESSIONS_KEPT {
            self.sessions.pop_front();
        }
        self.feeding = true;
        self.next_session
    }

    /// Keep `pcm`, reception `session`'s audio from sample `first`, for decoding again; the
    /// oldest goes once more than [`JTTY_HISTORY_SAMPLES`] are kept.
    fn remember(&mut self, session: u64, first: i64, pcm: &[i16]) {
        let mut at_sample = first;
        let mut rest = pcm;
        while !rest.is_empty() {
            let room = match self.history.back_mut() {
                Some((s, at, kept))
                    if *s == session
                        && *at + kept.len() as i64 == at_sample
                        && kept.len() < JTTY_HISTORY_PIECE =>
                {
                    let n = rest.len().min(JTTY_HISTORY_PIECE - kept.len());
                    kept.extend_from_slice(&rest[..n]);
                    n
                }
                _ => {
                    let n = rest.len().min(JTTY_HISTORY_PIECE);
                    self.history
                        .push_back((session, at_sample, rest[..n].to_vec()));
                    n
                }
            };
            at_sample += room as i64;
            rest = &rest[room..];
        }
        let mut total: usize = self.history.iter().map(|(_, _, p)| p.len()).sum();
        while total > JTTY_HISTORY_SAMPLES {
            let over = total - JTTY_HISTORY_SAMPLES;
            let Some((_, at, front)) = self.history.front_mut() else {
                break;
            };
            if front.len() <= over {
                total -= front.len();
                self.history.pop_front();
            } else {
                front.drain(..over);
                *at += over as i64;
                total -= over;
            }
        }
    }

    /// The span of reception `session` still kept: its first sample and one past its last.
    fn kept(&self, session: u64) -> Option<(i64, i64)> {
        let mine = || self.history.iter().filter(|(s, _, _)| *s == session);
        let (_, first, _) = mine().next()?;
        let (_, at, last) = mine().next_back()?;
        Some((*first, at + last.len() as i64))
    }

    /// Reception `session`'s kept audio from sample `first` to `end`.
    fn kept_pcm(&self, session: u64, first: i64, end: i64) -> Vec<i16> {
        let mut out = Vec::with_capacity((end - first).max(0) as usize);
        for (_, at, p) in self.history.iter().filter(|(s, _, _)| *s == session) {
            let lo = first.max(*at);
            let hi = end.min(at + p.len() as i64);
            if hi > lo {
                out.extend_from_slice(&p[(lo - at) as usize..(hi - at) as usize]);
            }
        }
        out
    }
}

/// Hold the receive window to [`JTTY_LINES_CAP`] lines of the kind just added, the oldest of that
/// kind going first: live copy and what a review found are capped each on its own, so decoding
/// again never pushes the live copy out of the window.
fn cap_lines(lines: &mut VecDeque<Line>, review: bool) {
    let kind = |l: &Line| l.review.is_some() == review;
    let mut over = lines
        .iter()
        .filter(|l| kind(l))
        .count()
        .saturating_sub(JTTY_LINES_CAP);
    if over == 0 {
        return;
    }
    lines.retain(|l| {
        if over > 0 && kind(l) {
            over -= 1;
            false
        } else {
            true
        }
    });
}

/// One reception's audio for the decode thread: what the tap took since the last take, in
/// order, and where to search it.
#[derive(Debug, Clone, PartialEq)]
pub struct JttyAudio {
    pub session: u64,
    pub pcm: Vec<i16>,
    pub band: SearchBand,
}

/// The dial in whole hertz, as WSJT-X carries an operating frequency.
fn dial_hz(dial_mhz: f64) -> u64 {
    (dial_mhz * 1e6).round().max(0.0) as u64
}

/// WSJT-X's `stripJttyFillerText` (`widgets/JttyMessages.hpp`): every run of the keep-alive
/// filler (blocks of five `<`, with the space around them) becomes one space, then the text is
/// trimmed and its spaces collapsed. The filler is what a sender's live typing transmits while
/// the typist pauses; it is never shown, logged or spotted.
fn strip_filler(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        let mut j = i;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        let mut end = None;
        while j + 5 <= c.len() && c[j..j + 5].iter().all(|&ch| ch == '<') {
            j += 5;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            end = Some(j);
        }
        match end {
            Some(e) => {
                out.push(' ');
                i = e;
            }
            None => {
                out.push(c[i]);
                i += 1;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A token in the shape a JTTY call atom carries, the only kind of call the outward actions
/// count: WSJT-X's strict standard-call form (`Radio.cpp`, `strict_standard_callsign_re`) — a
/// one- or two-character prefix holding a letter (`K`, `KD`, `9A`, `3D`, `K1`), the call-area
/// digit, then one to three letters. The suffix letter is required here, so exchange tokens
/// (`1D`, `5NN`, `599`) and grids (`EN52`) are never calls.
pub fn jtty_call(token: &str) -> bool {
    let b = token.as_bytes();
    let al = |c: u8| c.is_ascii_uppercase();
    let di = |c: u8| c.is_ascii_digit();
    (1..=2).any(|k| {
        if b.len() < k + 2 {
            return false;
        }
        let (p1, p2) = b.split_at(k);
        let prefix = match p1 {
            [a] => al(*a),
            [a, b] => (al(*a) && di(*b)) || ((al(*a) || di(*a)) && al(*b)),
            _ => false,
        };
        prefix && di(p2[0]) && (2..=4).contains(&p2.len()) && p2[1..].iter().all(|&c| al(c))
    })
}

/// WSJT-X's decoded-grid pattern (`Radio::decoded_grid_pattern`): a four- or six-character
/// locator, field letters A-R, subsquare letters A-X, and never `RR73`.
fn decoded_grid(token: &str) -> bool {
    let b = token.as_bytes();
    let field = |c: u8| matches!(c.to_ascii_uppercase(), b'A'..=b'R');
    let sub = |c: u8| matches!(c.to_ascii_uppercase(), b'A'..=b'X');
    let four = b.len() >= 4
        && field(b[0])
        && field(b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && !b[..4].eq_ignore_ascii_case(b"RR73");
    match b.len() {
        4 => four,
        6 => four && sub(b[4]) && sub(b[5]),
        _ => false,
    }
}

/// The sender of a message in WSJT-X's spot shape (`JttyReceiveResultController.cpp`): the
/// first word `CQ`, `DE` or a call, the second a call — the sender — never the operator's own,
/// and a locator in third place if one is there.
fn sender<'a>(text: &'a str, mycall: &str) -> Option<(&'a str, Option<&'a str>)> {
    let mut words = text.split_whitespace();
    let (first, second) = (words.next()?, words.next()?);
    let leads =
        first.eq_ignore_ascii_case("CQ") || first.eq_ignore_ascii_case("DE") || jtty_call(first);
    if !(leads && jtty_call(second)) || second.eq_ignore_ascii_case(mycall) {
        return None;
    }
    Some((second, words.next().filter(|g| decoded_grid(g))))
}

/// WSJT-X's decode-pane wrap (`Jtty::wrapMessage`): lines of at most 40 characters, broken at
/// the last space that fits (or hard at 40 when none does), continuations indented two.
fn wrap(text: &str) -> String {
    const WIDTH: usize = 40;
    let c: Vec<char> = text.chars().collect();
    if c.len() <= WIDTH {
        return text.to_string();
    }
    let mut out = String::new();
    let mut start = 0;
    while c.len() - start > WIDTH {
        match (1..=WIDTH).rev().find(|&i| c[start + i] == ' ') {
            Some(at) => {
                out.extend(&c[start..start + at]);
                out.push_str("\n  ");
                start += at + 1;
            }
            None => {
                out.extend(&c[start..start + WIDTH]);
                out.push_str("\n  ");
                start += WIDTH;
            }
        }
    }
    out.extend(&c[start..]);
    out
}

/// The line WSJT-X's decode pane prints for a message (`Jtty::formatJttyDecodeLine` over
/// `Jtty::wrapMessage`), without the optional time label: the frequency right-aligned in 4,
/// the SNR in 3, two spaces, the wrapped text.
pub fn jtty_pane_line(freq_hz: f32, snr: i32, text: &str) -> String {
    format!(
        "{:>4} {:>3}  {}",
        crate::alltxt::q_round(freq_hz),
        snr,
        wrap(text)
    )
}

/// The line WSJT-X prints for a message it SENT (`Jtty::formatJttyTxLine`): the Tx offset
/// right-aligned in 4 and the SNR column left blank, so sent and received lines line up.
pub fn jtty_sent_pane_line(tx_hz: f32, text: &str) -> String {
    format!("{:>4}      {}", crate::alltxt::q_round(tx_hz), wrap(text))
}

fn terminal_label(t: Terminal) -> &'static str {
    match t {
        Terminal::Growing => "growing",
        Terminal::Complete => "complete",
        Terminal::Expired => "expired",
        Terminal::ReceptionEnded => "ended",
        Terminal::Unknown(_) => "ended",
    }
}

impl Engine {
    /// Arm or disarm the JTTY decoder by an EXPLICIT operator act. Session-only (never
    /// persisted, so the app never launches armed). The receive window is kept either way, as
    /// WSJT-X keeps its panes across Monitor; Clear empties it. Disarming ends the reception
    /// (its open messages close as ended) and is REMEMBERED for the session, so re-entering
    /// the view cannot restart the decoder behind the operator; an explicit Arm retires that.
    pub fn set_jtty_armed(&mut self, on: bool) {
        if on {
            if !self.jtty.armed {
                self.jtty.notice = None;
            }
            self.jtty.auto_arm_declined = false;
        } else {
            self.jtty.audio.clear();
            self.jtty.feeding = false;
            self.jtty.auto_arm_declined = true;
        }
        self.jtty.auto_armed = false;
        self.jtty.armed = on;
    }

    /// Arm the decoder because the operator ENTERED the JTTY view (the PSK/APRS/SSTV auto-arm
    /// doctrine): only an upgrade from disarmed, refused once the operator has stopped it this
    /// session, and refused while Settings says opening the view does not start the receiver
    /// (`jtty_rx_auto_arm`). Returns whether this call armed it. RX ONLY — arming a decoder
    /// keys nothing.
    pub fn jtty_auto_arm(&mut self) -> bool {
        if self.jtty.armed || self.jtty.auto_arm_declined || !self.settings.jtty_rx_auto_arm {
            return false;
        }
        self.jtty.notice = None;
        self.jtty.armed = true;
        self.jtty.auto_armed = true;
        true
    }

    /// The decoder is running because the JTTY screen started it ([`Engine::jtty_auto_arm`]).
    pub(super) fn jtty_auto_armed(&self) -> bool {
        self.jtty.armed && self.jtty.auto_armed
    }

    /// Stop the decoder because the operator left the JTTY screen for another section
    /// (`Engine::leave_decoder_screen`, which decides whether to). An automatic stop: no decline
    /// is remembered, so the screen starts it again, and the receive window is kept.
    pub(super) fn jtty_auto_disarm(&mut self) {
        self.jtty.audio.clear();
        self.jtty.feeding = false;
        self.jtty.auto_armed = false;
        self.jtty.armed = false;
    }

    /// Whether the JTTY decoder is armed (the decode thread's gate).
    pub fn jtty_armed(&self) -> bool {
        self.jtty.armed
    }

    /// The JTTY Rx frequency, Hz: the RX offset a WSJT-X UDP Status carries in JTTY.
    pub fn jtty_rx_hz(&self) -> f32 {
        self.jtty.rx_hz
    }

    /// WSJT-X's Rx frequency and F Tol: the channel always searched. Takes effect at the next
    /// search of the current reception, without restarting it (WSJT-X keeps its context when
    /// only the audio frequency moves). F Tol is also the one to keep (Settings `jtty_ftol_hz`,
    /// WSJT-X's `JTTY_FTol`); the caller persists the settings.
    pub fn set_jtty_rx(&mut self, rx_hz: f32, ftol_hz: f32) -> Result<(), String> {
        if !(100.0..=5000.0).contains(&rx_hz) {
            return Err(format!("JTTY Rx {rx_hz} Hz is outside 100-5000 Hz."));
        }
        if !(1.0..=500.0).contains(&ftol_hz) {
            return Err(format!("JTTY F Tol {ftol_hz} Hz is outside 1-500 Hz."));
        }
        self.jtty.rx_hz = rx_hz;
        self.jtty.ftol_hz = ftol_hz;
        self.settings.jtty_ftol_hz = self.jtty_ftol_setting();
        Ok(())
    }

    /// JTTY was selected (the section entered from another one): the Rx frequency goes back to
    /// 1500 Hz, as WSJT-X sets it whenever JTTY is selected (`on_actionJTTY_triggered`).
    pub(super) fn jtty_selected(&mut self) {
        self.jtty.rx_hz = JTTY_RX_HZ;
    }

    /// The decoder's F Tol, as Settings keeps it (whole hertz).
    pub(super) fn jtty_ftol_setting(&self) -> u32 {
        self.jtty.ftol_hz.round() as u32
    }

    /// A restored Settings bundle's F Tol, for the decoder ([`JttyRx::with_ftol`]'s range).
    pub(super) fn adopt_jtty_ftol(&mut self, ftol_hz: u32) {
        self.jtty.ftol_hz = ftol_hz.clamp(1, 500) as f32;
    }

    /// The waterfall's range, inside which the fixed 1200-1500 and 1500-1800 Hz channels are
    /// searched (WSJT-X searches them within its wide graph).
    pub fn set_jtty_band(&mut self, low_hz: i32, high_hz: i32) -> Result<(), String> {
        if !(0..high_hz).contains(&low_hz) || high_hz > 6000 {
            return Err(format!(
                "JTTY waterfall range {low_hz}-{high_hz} Hz is not a range inside 0-6000 Hz."
            ));
        }
        self.jtty.low_hz = low_hz;
        self.jtty.high_hz = high_hz;
        Ok(())
    }

    /// Empty the receive window (WSJT-X's Erase). What has been heard stays heard.
    pub fn jtty_clear(&mut self) {
        self.jtty.lines.clear();
        self.jtty.notice = None;
    }

    /// The RX tap: `samples` (12 kHz, as the radio loop captured them `now_ms`) appended to the
    /// reception the tap feeds, starting a new one when there is none, the dial has moved, or
    /// the decode thread has fallen behind by more than [`JTTY_TAP_CAP`]. Disarmed costs one
    /// branch.
    pub(super) fn jtty_feed(&mut self, samples: &[f32], now_ms: i64) {
        if !self.jtty.armed || samples.is_empty() {
            return;
        }
        // While JTTY transmits, nothing is tapped and the reception ends (WSJT-X ends it on
        // Transmission): what the receiver hears then is our own signal. The next audio after
        // the transmission starts a new reception.
        if self.jtty_sending() {
            self.jtty.feeding = false;
            return;
        }
        let dial = dial_hz(self.settings.dial_mhz);
        let j = &mut self.jtty;
        let buffered: usize = j.audio.iter().map(|(_, a)| a.len()).sum();
        let overflow = buffered + samples.len() > JTTY_TAP_CAP;
        if overflow {
            j.audio.clear();
            j.notice = Some(JTTY_AUDIO_LOST.to_string());
        }
        let id = match j.current() {
            Some(s) if s.dial_hz == dial && !overflow => s.id,
            _ => {
                let lead_ms = (samples.len() as i64 * 1000) / 12_000;
                j.start_session(now_ms - lead_ms, dial)
            }
        };
        let pcm = tempo_core::channel::capture_to_i16(samples);
        // Kept for decoding again (WSJT-X's `ReceiveHistory`): the reception's samples in order.
        if let Some(session) = j.sessions.back_mut() {
            let first = session.fed;
            session.fed += pcm.len() as i64;
            j.remember(id, first, &pcm);
        }
        match j.audio.last_mut() {
            Some((s, a)) if *s == id => a.extend_from_slice(&pcm),
            _ => j.audio.push((id, pcm)),
        }
    }

    /// DECODE AGAIN — WSJT-X's review of retained audio (`startJttyReview`, 3.3.0-beta4
    /// `widgets/mainwindow_jtty_receive.cpp` 685-728). With `at_utc_ms` (a double-click on a
    /// waterfall row, the row's time), the reception that row belongs to, from 5 s before it, 40 s
    /// of it; without (the Decode button), every reception's audio still kept. Searched where the
    /// operator's selection is now, and decoded by a receiver of its own on the decode thread. A
    /// new review replaces one still running. RECEIVE ONLY: what it finds is shown and nothing
    /// else (module doc).
    pub fn jtty_decode_again(&mut self, at_utc_ms: Option<i64>) -> Result<(), String> {
        let band = self.jtty.band();
        let j = &mut self.jtty;
        let mut jobs: Vec<(u64, i64, i64)> = Vec::new();
        match at_utc_ms {
            Some(at) => {
                let found = j.sessions.iter().rev().find_map(|s| {
                    let (first, end) = j.kept(s.id)?;
                    let sample = (at - s.anchor_ms) * 12;
                    (sample > first && sample <= end).then_some((s.id, first, end, sample))
                });
                let Some((session, kept_first, kept_end, sample)) = found else {
                    return Err(JTTY_REVIEW_GONE.to_string());
                };
                let first = kept_first.max(sample - JTTY_REVIEW_BEFORE_SAMPLES);
                let end = kept_end.min(first + JTTY_REVIEW_SAMPLES);
                jobs.push((session, first, end));
            }
            None => {
                for s in &j.sessions {
                    if let Some((first, end)) = j.kept(s.id) {
                        jobs.push((s.id, first, end));
                    }
                }
            }
        }
        jobs.retain(|(_, first, end)| end > first);
        if jobs.is_empty() {
            return Err(JTTY_REVIEW_NOTHING.to_string());
        }
        // A new review replaces any not yet taken (WSJT-X's `cancelJttyReview`); the decode
        // thread drops one it is running when it takes these.
        j.reviews.clear();
        j.last_review += 1;
        let group = j.last_review;
        for (session, first, end) in jobs {
            let Some(s) = j.session(session).cloned() else {
                continue;
            };
            j.last_job += 1;
            let pcm = j.kept_pcm(session, first, end);
            j.reviews.push(JttyReview {
                of: JttyReviewOf {
                    group,
                    job: j.last_job,
                    anchor_ms: s.anchor_ms + first / 12,
                    dial_hz: s.dial_hz,
                    band,
                },
                pcm,
            });
        }
        j.notice = Some(JTTY_REVIEW_STARTED.to_string());
        Ok(())
    }

    /// The reviews asked for since the last take (the decode thread).
    pub fn take_jtty_reviews(&mut self) -> Vec<JttyReview> {
        std::mem::take(&mut self.jtty.reviews)
    }

    /// Show what a review decoded, as WSJT-X shows a review: in both panes under its heading,
    /// the QSO Frequency pane by the review's own Rx and F Tol, and NOTHING else — no ALL.TXT
    /// line, no spot, no sighting toward the two-decode rule, no heard station.
    pub fn apply_jtty_review(&mut self, review: &JttyReviewOf, updates: &[Update]) {
        let band = review.band;
        for u in updates {
            let text = strip_filler(&u.text);
            if text.is_empty() {
                continue;
            }
            let j = &mut self.jtty;
            let near = (u.freq_hz - band.center_hz).abs() < band.tolerance_hz;
            match j
                .lines
                .iter_mut()
                .rev()
                .find(|l| l.review_job == review.job && l.id == u.message_id)
            {
                Some(line) => {
                    line.text = text;
                    line.freq_hz = u.freq_hz;
                    line.snr = u.snr_db;
                    line.terminal = u.terminal;
                    line.rx = line.rx || near;
                }
                None => {
                    j.lines.push_back(Line {
                        id: u.message_id,
                        start_ms: review.anchor_ms + (u.start_s * 1000.0 + 0.5) as i64,
                        freq_hz: u.freq_hz,
                        dial_hz: review.dial_hz,
                        snr: u.snr_db,
                        text,
                        terminal: u.terminal,
                        rx: near,
                        tx: false,
                        review: Some(review.group),
                        review_job: review.job,
                    });
                    cap_lines(&mut j.lines, true);
                }
            }
        }
    }

    /// Drain the tap for the decode thread: each reception's audio in order, with where to
    /// search it — the operator's selection now for the reception being fed, and an ended
    /// reception's own (WSJT-X's `ReceiveSpan`) for the audio it left behind.
    pub fn take_jtty_audio(&mut self) -> Vec<JttyAudio> {
        let current = self.jtty.current().map(|s| s.id);
        let live = self.jtty.band();
        std::mem::take(&mut self.jtty.audio)
            .into_iter()
            .map(|(session, pcm)| {
                let band = if Some(session) == current {
                    live
                } else {
                    self.jtty.session(session).map_or(live, |s| s.band)
                };
                JttyAudio { session, pcm, band }
            })
            .collect()
    }

    /// Apply what the decoder reported for `session`, in order — WSJT-X's
    /// `ReceiveResultController::apply` for live audio, plus the two-decode rule (module doc).
    /// An update for a reception no longer remembered is dropped: it cannot be placed in time.
    pub fn apply_jtty_updates(&mut self, session: u64, updates: &[Update]) {
        let Some(ctx) = self.jtty.session(session).cloned() else {
            return;
        };
        let mycall = self.settings.mycall.trim().to_string();
        let uploading = self.pskreporter_uploading();
        let all_txt = self.settings.write_all_txt;
        let current = self.jtty.current().map(|s| s.id) == Some(session);
        // Our own keyed stretch still going counts as keyed time too.
        let keyed_since = self.jtty_keyed_since_ms();
        let (center, tolerance) = if current {
            (self.jtty.rx_hz, self.jtty.ftol_hz)
        } else {
            (ctx.band.center_hz, ctx.band.tolerance_hz)
        };
        for u in updates {
            let text = strip_filler(&u.text);
            if text.is_empty() {
                continue;
            }
            let j = &mut self.jtty;
            // WSJT-X stamps a message once, at its first update: the reception's anchor plus
            // its start, rounded to the millisecond (`qRound64(startSeconds * 1000)`). ALL.TXT
            // and the spot both carry that stamp.
            let start_ms = match j
                .lines
                .iter_mut()
                .rev()
                .find(|l| l.id == u.message_id && l.review.is_none())
            {
                Some(line) => {
                    line.text = text.clone();
                    line.freq_hz = u.freq_hz;
                    line.snr = u.snr_db;
                    line.terminal = u.terminal;
                    line.rx = line.rx || (u.freq_hz - center).abs() < tolerance;
                    line.start_ms
                }
                None => {
                    let start_ms = ctx.anchor_ms + (u.start_s * 1000.0 + 0.5) as i64;
                    j.lines.push_back(Line {
                        id: u.message_id,
                        start_ms,
                        freq_hz: u.freq_hz,
                        dial_hz: ctx.dial_hz,
                        snr: u.snr_db,
                        text: text.clone(),
                        terminal: u.terminal,
                        rx: (u.freq_hz - center).abs() < tolerance,
                        tx: false,
                        review: None,
                        review_job: 0,
                    });
                    cap_lines(&mut j.lines, false);
                    start_ms
                }
            };
            // ALL.TXT: a message once, when it is finished (complete, expired or cut off).
            if u.terminal != Terminal::Growing && all_txt {
                let pending = &mut self.station.all_txt_pending;
                pending.push(crate::alltxt::jtty_rx_line(
                    start_ms,
                    ctx.dial_hz,
                    u.freq_hz,
                    u.snr_db,
                    &text,
                ));
                let len = pending.len();
                if len > 5000 {
                    pending.drain(0..len - 5000);
                }
            }
            // OUR OWN ECHO never counts: a message that started inside our own keyed time is our
            // transmission decoded back (a receiver that hears us while we key), and it must not
            // confirm a call, spot one or put one on the board. Shown and logged above, as
            // received, and nothing more.
            let j = &mut self.jtty;
            if j.in_keyed(start_ms) || keyed_since.is_some_and(|since| start_ms >= since) {
                continue;
            }
            // The two-decode rule: every call this message names is one sighting of it.
            for call in text.split_whitespace().filter(|w| jtty_call(w)) {
                if j.confirm.note(call, u.message_id, start_ms) {
                    // A complete message from it was held back for want of this: send it
                    // now, if it is still within the window of this sighting.
                    if let Some(spot) = j.held_spots.remove(call) {
                        let age_ms = (start_ms - i64::from(spot.time_secs) * 1000).abs();
                        if uploading && age_ms <= CONFIRM_WINDOW_MS {
                            j.spots.push(spot);
                        }
                    }
                }
            }
            let Some((call, grid)) = sender(&text, &mycall) else {
                continue;
            };
            let rf = ctx.dial_hz + crate::alltxt::q_round(u.freq_hz).max(0) as u64;
            j.senders.insert(
                call.to_string(),
                Sender {
                    freq_hz: rf,
                    snr: u.snr_db,
                    grid: grid.map(str::to_string),
                    at_ms: start_ms,
                },
            );
            if j.senders.len() > 1024 {
                j.senders
                    .retain(|_, s| start_ms - s.at_ms <= HEARD_SHOWN_MS);
            }
            // PSK Reporter: WSJT-X spots a live message once it is complete.
            if u.terminal == Terminal::Complete {
                let spot = JttySpot {
                    call: call.to_string(),
                    freq_hz: rf,
                    snr: u.snr_db,
                    time_secs: (start_ms.max(0) / 1000) as u32,
                    grid: grid.map(str::to_string),
                };
                if j.confirm.is_confirmed(call) {
                    if uploading {
                        j.spots.push(spot);
                    }
                } else {
                    j.held_spots.insert(call.to_string(), spot);
                }
            }
            if j.spots.len() > JTTY_SPOTS_CAP {
                let over = j.spots.len() - JTTY_SPOTS_CAP;
                j.spots.drain(0..over);
            }
            if j.held_spots.len() > 1024 {
                j.held_spots.retain(|_, s| {
                    start_ms / 1000 - i64::from(s.time_secs) <= CONFIRM_WINDOW_MS / 1000
                });
            }
        }
    }

    /// The spots waiting for PSK Reporter (taken by the radio loop, which sends them with the
    /// rest when uploading is on). Only confirmed calls are ever here.
    pub fn take_jtty_spots(&mut self) -> Vec<JttySpot> {
        std::mem::take(&mut self.jtty.spots)
    }

    /// The confirmed stations decoded within `max_age_ms` of `now_ms`, newest first: what the
    /// Needed board hears from this radio's JTTY decoder.
    pub fn jtty_heard(&self, now_ms: i64, max_age_ms: i64) -> Vec<JttyHeard> {
        let mut heard: Vec<JttyHeard> = self
            .jtty
            .senders
            .iter()
            .filter(|(call, s)| {
                self.jtty.confirm.is_confirmed(call) && now_ms - s.at_ms <= max_age_ms
            })
            .map(|(call, s)| JttyHeard {
                call: call.clone(),
                freq_hz: s.freq_hz,
                band: crate::bandplan::band_for_dial(s.freq_hz as f64 / 1e6)
                    .unwrap_or("")
                    .to_string(),
                snr: s.snr,
                grid: s.grid.clone(),
                at_utc_ms: s.at_ms,
            })
            .collect();
        heard.sort_by(|a, b| b.at_utc_ms.cmp(&a.at_utc_ms).then(a.call.cmp(&b.call)));
        heard
    }

    /// The live JTTY state for the cockpit poll.
    pub fn jtty_state(&self) -> JttyState {
        self.jtty_state_at(now_unix_millis() as i64)
    }

    fn jtty_state_at(&self, now_ms: i64) -> JttyState {
        let j = &self.jtty;
        JttyState {
            armed: j.armed,
            rx_hz: j.rx_hz,
            ftol_hz: j.ftol_hz,
            low_hz: j.low_hz,
            high_hz: j.high_hz,
            lines: j
                .lines
                .iter()
                .map(|l| JttyLine {
                    id: l.id,
                    start_utc_ms: l.start_ms,
                    freq_hz: l.freq_hz,
                    dial_hz: l.dial_hz,
                    snr: l.snr,
                    text: l.text.clone(),
                    line: if l.tx {
                        jtty_sent_pane_line(l.freq_hz, &l.text)
                    } else {
                        jtty_pane_line(l.freq_hz, l.snr, &l.text)
                    },
                    state: terminal_label(l.terminal).to_string(),
                    rx: l.rx,
                    confirmed_calls: l
                        .text
                        .split_whitespace()
                        .filter(|w| jtty_call(w) && j.confirm.is_confirmed(w))
                        .map(str::to_string)
                        .collect(),
                    tx: l.tx,
                    review: l.review,
                })
                .collect(),
            heard: self.jtty_heard(now_ms, HEARD_SHOWN_MS),
            notice: j.notice.clone(),
            tx: self.jtty_tx_state(),
        }
    }
}

/// TEST SUPPORT: hold this while a test holds a receive context. The modem has eight for the whole
/// process (`MAX_CONTEXTS`), the test harness runs tests side by side, and a test that finds them
/// all taken decodes nothing: the decoding tests run one at a time.
#[cfg(test)]
pub(crate) fn receive_contexts() -> std::sync::MutexGuard<'static, ()> {
    static CONTEXTS: Mutex<()> = Mutex::new(());
    CONTEXTS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The decode thread's half: one [`jtty::Live`] reception, fed from [`Engine::take_jtty_audio`]
/// and pumped OFF the engine lock. Owned by the thread (tempo-audio `jttyrx`), which calls
/// [`tick`](Self::tick) every ~100 ms; a test drives it the same way.
#[derive(Debug, Default)]
pub struct JttyDriver {
    live: Option<jtty::Live>,
    /// The engine reception the live one is decoding.
    session: Option<u64>,
    /// A reception that lost the audio it needs: the rest of its audio is let go.
    dead: Option<u64>,
    /// Reviews waiting, oldest first, and the one being decoded: its receiver and how much of
    /// its audio has been pushed.
    reviews: VecDeque<JttyReview>,
    reviewing: Option<(JttyReview, jtty::Live, usize)>,
}

impl JttyDriver {
    /// Take the tapped audio, decode it, hand back what it decoded. The engine lock is held
    /// only to take and to apply; every search runs with it released (a decode must never hold
    /// the lock the radio loop unkeys under).
    pub fn tick(&mut self, engine: &Mutex<Engine>) {
        let (armed, keyed, audio, reviews) = {
            let mut e = engine_lock(engine);
            (
                e.jtty_armed(),
                e.jtty_sending(),
                e.take_jtty_audio(),
                e.take_jtty_reviews(),
            )
        };
        let mut out: Vec<(u64, Vec<Update>)> = Vec::new();
        let mut notice = None;
        let mut restart = None;
        for chunk in audio {
            if self.dead == Some(chunk.session) {
                continue;
            }
            if self.session != Some(chunk.session) {
                self.finish(&mut out);
                if self.live.is_none() {
                    self.live = jtty::Live::new();
                }
                let Some(live) = self.live.as_mut() else {
                    notice = Some(JTTY_NO_CONTEXT);
                    continue;
                };
                live.begin(chunk.session as i64);
                self.session = Some(chunk.session);
            }
            let Some(live) = self.live.as_mut() else {
                continue;
            };
            live.push(&chunk.pcm);
            match live.pump(chunk.band) {
                Ok(updates) => out.push((chunk.session, updates)),
                Err(_) => {
                    // WSJT-X restarts a reception that lost audio it needs
                    // (`restartJttyReception`). The engine owns the timeline, so this one ends
                    // here, the rest of its audio is let go, and the engine starts a new one.
                    out.push((chunk.session, live.end()));
                    self.session = None;
                    self.dead = Some(chunk.session);
                    notice = Some(JTTY_AUDIO_LOST);
                    restart = Some(chunk.session);
                }
            }
        }
        // Disarmed, or transmitting: the reception ends here, with what it had (WSJT-X ends it on
        // Transmission; the audio from before the key was decoded above).
        if !armed || keyed {
            self.finish(&mut out);
        }
        // Decode again: new reviews replace what is running, and the review runs a chunk a tick.
        if !reviews.is_empty() {
            self.reviewing = None;
            self.reviews = reviews.into();
        }
        let reviewed = self.review_step(&mut notice);
        if out.iter().all(|(_, u)| u.is_empty()) && notice.is_none() && reviewed.is_none() {
            return;
        }
        let mut e = engine_lock(engine);
        for (session, updates) in &out {
            e.apply_jtty_updates(*session, updates);
        }
        if let Some((review, updates)) = &reviewed {
            e.apply_jtty_review(review, updates);
        }
        if let Some(n) = notice {
            e.jtty.notice = Some(n.to_string());
        }
        if let Some(session) = restart {
            e.jtty.restart(session);
        }
    }

    /// One tick of decoding again: the next [`JTTY_REVIEW_CHUNK`] of the review running (a new
    /// one started when none is), and what it decoded; the review ends once all of it is in.
    fn review_step(&mut self, notice: &mut Option<&str>) -> Option<(JttyReviewOf, Vec<Update>)> {
        if self.reviewing.is_none() {
            let review = self.reviews.pop_front()?;
            let Some(mut live) = jtty::Live::new() else {
                *notice = Some(JTTY_NO_CONTEXT);
                self.reviews.clear();
                return None;
            };
            live.begin(review.of.job as i64);
            self.reviewing = Some((review, live, 0));
        }
        let (review, live, pushed) = self.reviewing.as_mut()?;
        let end = (*pushed + JTTY_REVIEW_CHUNK).min(review.pcm.len());
        live.push(&review.pcm[*pushed..end]);
        *pushed = end;
        let mut updates = live.pump(review.of.band).unwrap_or_default();
        let of = review.of;
        if *pushed >= review.pcm.len() {
            updates.extend(live.end());
            self.reviewing = None;
        }
        Some((of, updates))
    }

    /// End the reception being decoded, keeping its last updates.
    fn finish(&mut self, out: &mut Vec<(u64, Vec<Update>)>) {
        if let (Some(session), Some(live)) = (self.session.take(), self.live.as_mut()) {
            out.push((session, live.end()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    /// 2023-11-14 22:13:20.100 UTC: the radio loop delivers the first 100 ms of a reception at
    /// this instant, so the reception's sample 0 is 22:13:20.000.
    const T0_MS: i64 = 1_700_000_000_100;

    fn rx_engine() -> Engine {
        Engine::with_settings(Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            dial_mhz: 14.090,
            band: "20m".into(),
            pskreporter: true,
            write_all_txt: true,
            ..Settings::default()
        })
    }

    /// Armed, with the first 100 ms of audio delivered at [`T0_MS`]: the reception it began.
    fn begun(e: &mut Engine) -> u64 {
        e.set_jtty_armed(true);
        e.jtty_feed(&[0.0; 1200], T0_MS);
        e.jtty.sessions.back().expect("a reception").id
    }

    fn upd(id: i64, text: &str, start_s: f64, terminal: Terminal) -> Update {
        Update {
            message_id: id,
            text: text.into(),
            freq_hz: 1500.2,
            start_s,
            latest_s: start_s,
            terminal,
            snr_db: -12,
        }
    }

    /// UTC ms of a message starting `start_s` into the [`begun`] reception.
    fn at(start_s: f64) -> i64 {
        T0_MS - 100 + (start_s * 1000.0) as i64
    }

    fn spots(e: &mut Engine) -> Vec<(String, u64, i32, u32)> {
        e.take_jtty_spots()
            .into_iter()
            .map(|s| (s.call, s.freq_hz, s.snr, s.time_secs))
            .collect()
    }

    fn heard_calls(e: &Engine, now_ms: i64) -> Vec<String> {
        e.jtty_heard(now_ms, CONFIRM_WINDOW_MS)
            .into_iter()
            .map(|h| h.call)
            .collect()
    }

    // ── the pure pieces, by value ──

    #[test]
    fn a_call_is_the_shape_a_call_atom_carries() {
        for call in [
            "K1ABC", "KD9TAW", "W1AW", "9A1AA", "3D2AB", "2E1ABC", "KH6ABC", "K1A", "DA2PKN",
            "0C9GFW",
        ] {
            assert!(jtty_call(call), "{call} is a call");
        }
        // Exchange tokens, grids, words and partial calls are not, though WSJT-X's looser
        // `is_standard_callsign` passes `1D`, `5NN` and `K1`.
        for not in [
            "1D", "5NN", "K1", "KH6", "599", "73", "EN52", "FN42", "CQ", "TU", "AGN?", "QRZ?",
            "K1ABC/P", "K1ABCDE", "k1abc", "",
        ] {
            assert!(!jtty_call(not), "{not:?} is not a call");
        }
    }

    #[test]
    fn the_sender_is_wsjtxs_spot_shape() {
        let s = |t: &str| sender(t, "KD9TAW").map(|(c, g)| (c.to_string(), g.map(str::to_string)));
        assert_eq!(s("CQ K1ABC CQ"), Some(("K1ABC".into(), None)));
        assert_eq!(
            s("CQ KD8XYZ EN61"),
            Some(("KD8XYZ".into(), Some("EN61".into())))
        );
        assert_eq!(
            s("DE W1AW FN31PR"),
            Some(("W1AW".into(), Some("FN31PR".into())))
        );
        assert_eq!(s("W1AW K1ABC"), Some(("K1ABC".into(), None)));
        assert_eq!(
            s("W1AW K1ABC RR73"),
            Some(("K1ABC".into(), None)),
            "RR73 is not a grid"
        );
        assert_eq!(
            s("W1AW K1ABC RR73AB"),
            Some(("K1ABC".into(), None)),
            "nor anything starting RR73"
        );
        assert_eq!(
            s("W1AW K1ABC RR72AB"),
            Some(("K1ABC".into(), Some("RR72AB".into())))
        );
        // Not the shape: a call action led by TU, a bare call, an exchange, the operator's own.
        for t in [
            "TU K1ABC CQ",
            "K1ABC",
            "K1ABC TU",
            "K1ABC 1D EMA",
            "599 K1ABC",
            "CQ KD9TAW EN52",
            "W1AW kd9taw",
        ] {
            assert_eq!(s(t), None, "{t}");
        }
    }

    #[test]
    fn filler_is_stripped_as_wsjtx_strips_it() {
        assert_eq!(strip_filler("CQ K1ABC <<<<< <<<<< CQ"), "CQ K1ABC CQ");
        assert_eq!(strip_filler("<<<<< <<<<<"), "");
        assert_eq!(strip_filler("HI<<<<<THERE"), "HI THERE");
        // Fewer than five is text, and seven leaves two.
        assert_eq!(strip_filler("A <<<< B"), "A <<<< B");
        assert_eq!(strip_filler("A <<<<<<< B"), "A << B");
        assert_eq!(strip_filler("  CQ   K1ABC  "), "CQ K1ABC");
    }

    #[test]
    fn the_pane_line_is_wsjtxs() {
        assert_eq!(
            jtty_pane_line(1500.2, -12, "CQ K1ABC CQ"),
            "1500 -12  CQ K1ABC CQ"
        );
        assert_eq!(jtty_pane_line(987.5, 5, "TU"), " 988   5  TU");
        // Wrapped at the last space inside 40 characters, continuations indented two.
        assert_eq!(
            jtty_pane_line(
                1500.0,
                -3,
                "HELLO FROM THE SHACK IN EN52 RUNNING 100 W TO A VERTICAL"
            ),
            "1500  -3  HELLO FROM THE SHACK IN EN52 RUNNING 100\n  W TO A VERTICAL"
        );
        // No space to break at: cut at 40.
        let solid = "A".repeat(45);
        assert_eq!(
            jtty_pane_line(1500.0, -3, &solid),
            format!("1500  -3  {}\n  {}", "A".repeat(40), "A".repeat(5))
        );
    }

    // ── the two-decode rule ──

    /// THE RULE, both ways. A call decoded once — here a well-formed fabricated one, the shape a
    /// crowded corpus produced in stock WSJT-X and Nexus alike — is shown and logged as WSJT-X
    /// shows and logs it, and raises nothing outward: no PSK Reporter spot, no heard station
    /// for the Needed board, no call a lookup may be offered for. The same call in a second
    /// message thirty seconds later (the positive control) raises all three, and the first
    /// message's held spot goes out with the second's.
    #[test]
    fn a_call_decoded_once_raises_nothing_outward_and_twice_does() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        e.apply_jtty_updates(s, &[upd(1, "CQ DA2PKN CQ", 3.0, Terminal::Complete)]);
        assert_eq!(spots(&mut e), vec![], "one decode spots nothing");
        assert_eq!(
            heard_calls(&e, at(4.0)),
            Vec::<String>::new(),
            "one decode hears nothing"
        );
        let st = e.jtty_state_at(at(4.0));
        assert_eq!(st.lines.len(), 1, "the window shows it, as WSJT-X's does");
        assert_eq!(
            st.lines[0].confirmed_calls,
            Vec::<String>::new(),
            "no lookup is offered"
        );
        assert!(st.heard.is_empty());
        assert_eq!(
            e.take_all_txt_pending().len(),
            1,
            "ALL.TXT logs it, as WSJT-X's does"
        );

        e.apply_jtty_updates(s, &[upd(2, "CQ DA2PKN CQ", 33.0, Terminal::Complete)]);
        let t = |start_s: f64| (at(start_s) / 1000) as u32;
        assert_eq!(
            spots(&mut e),
            vec![
                ("DA2PKN".to_string(), 14_091_500, -12, t(3.0)),
                ("DA2PKN".to_string(), 14_091_500, -12, t(33.0)),
            ],
            "twice: the held spot and this one"
        );
        let heard = e.jtty_heard(at(34.0), CONFIRM_WINDOW_MS);
        assert_eq!(heard.len(), 1);
        assert_eq!(
            (
                heard[0].call.as_str(),
                heard[0].freq_hz,
                heard[0].band.as_str(),
                heard[0].at_utc_ms
            ),
            ("DA2PKN", 14_091_500, "20m", at(33.0))
        );
        let st = e.jtty_state_at(at(34.0));
        for line in &st.lines {
            assert_eq!(
                line.confirmed_calls,
                vec!["DA2PKN".to_string()],
                "{}",
                line.text
            );
        }
        assert_eq!(st.heard.len(), 1);
    }

    /// The window is two minutes from start to start, and a call's sightings must be in two
    /// different messages: a message growing frame by frame is ONE sighting however many
    /// updates it sends.
    #[test]
    fn two_sightings_must_be_two_messages_inside_the_window() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        // One message, three updates.
        e.apply_jtty_updates(
            s,
            &[
                upd(1, "CQ KD8XYZ", 2.0, Terminal::Growing),
                upd(1, "CQ KD8XYZ EN61", 2.0, Terminal::Growing),
                upd(1, "CQ KD8XYZ EN61", 2.0, Terminal::Complete),
            ],
        );
        assert_eq!(spots(&mut e), vec![]);
        assert!(heard_calls(&e, at(3.0)).is_empty());
        // A second message 120.5 s after the first is outside the window…
        e.apply_jtty_updates(s, &[upd(2, "CQ KD8XYZ EN61", 122.5, Terminal::Complete)]);
        assert_eq!(spots(&mut e), vec![], "outside the window");
        assert!(heard_calls(&e, at(123.0)).is_empty());
        // …and a third inside it of the second confirms.
        e.apply_jtty_updates(s, &[upd(3, "CQ KD8XYZ EN61", 140.0, Terminal::Complete)]);
        let got = spots(&mut e);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(heard_calls(&e, at(141.0)), vec!["KD8XYZ".to_string()]);
        // The board's heard station ages out as the board's own decodes do.
        assert!(heard_calls(&e, at(140.0) + CONFIRM_WINDOW_MS + 1).is_empty());
    }

    /// A call confirmed by messages that only NAME it (an exchange sent to it, a TU) is a real
    /// call, so its first message in the spot shape spots at once. An exchange token is never a
    /// call (WSJT-X's own test would spot `1D` from `K1ABC 1D EMA`), the operator is never
    /// spotted, and a message still growing is not spotted until it completes.
    #[test]
    fn what_counts_and_what_is_spotted() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        e.apply_jtty_updates(
            s,
            &[
                upd(1, "K1ABC 1D EMA", 5.0, Terminal::Complete),
                upd(2, "K1ABC 1D EMA", 12.0, Terminal::Complete),
            ],
        );
        assert_eq!(spots(&mut e), vec![], "nothing in these is a sender");
        e.apply_jtty_updates(s, &[upd(3, "CQ K1ABC", 20.0, Terminal::Growing)]);
        assert_eq!(spots(&mut e), vec![], "not complete yet");
        assert_eq!(
            heard_calls(&e, at(21.0)),
            vec!["K1ABC".to_string()],
            "but heard"
        );
        e.apply_jtty_updates(s, &[upd(3, "CQ K1ABC CQ", 20.0, Terminal::Complete)]);
        assert_eq!(
            spots(&mut e),
            vec![(
                "K1ABC".to_string(),
                14_091_500,
                -12,
                (at(20.0) / 1000) as u32
            )]
        );
        // The operator's own call, confirmed or not, is never spotted or heard.
        e.apply_jtty_updates(
            s,
            &[
                upd(4, "CQ KD9TAW CQ", 30.0, Terminal::Complete),
                upd(5, "W1AW KD9TAW", 40.0, Terminal::Complete),
            ],
        );
        assert_eq!(spots(&mut e), vec![]);
        assert!(!heard_calls(&e, at(41.0)).contains(&"KD9TAW".to_string()));
        assert!(
            !e.jtty.confirm.is_confirmed("1D") && !e.jtty.confirm.is_confirmed("EMA"),
            "exchange tokens are never counted"
        );
    }

    /// PSK Reporter spots wait on the upload switch, as WSJT-X's do on its own; the board and
    /// the lookups do not.
    #[test]
    fn spots_need_the_upload_switch() {
        let mut e = rx_engine();
        e.settings.pskreporter = false;
        let s = begun(&mut e);
        e.apply_jtty_updates(
            s,
            &[
                upd(1, "CQ K1ABC CQ", 3.0, Terminal::Complete),
                upd(2, "CQ K1ABC CQ", 13.0, Terminal::Complete),
            ],
        );
        assert_eq!(spots(&mut e), vec![]);
        assert_eq!(heard_calls(&e, at(14.0)), vec!["K1ABC".to_string()]);
    }

    // ── the receive window and ALL.TXT ──

    /// One ALL.TXT line per message, written when it finishes, byte for byte WSJT-X's: stamped
    /// with the reception's anchor plus the message's start, the dial the reception was on. A
    /// message still growing writes nothing; expired and cut-off messages are logged too; the
    /// keep-alive filler never reaches the file; and the switch off writes nothing.
    #[test]
    fn a_finished_message_writes_one_wsjtx_all_txt_line() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        e.apply_jtty_updates(s, &[upd(7, "CQ KD8XYZ", 2.5, Terminal::Growing)]);
        assert_eq!(e.take_all_txt_pending(), Vec::<String>::new());
        e.apply_jtty_updates(
            s,
            &[
                upd(7, "CQ KD8XYZ <<<<< EN61", 2.5, Terminal::Complete),
                upd(8, "TU K1ABC", 9.25, Terminal::Expired),
                upd(9, "QRZ?", 30.0, Terminal::ReceptionEnded),
            ],
        );
        assert_eq!(
            e.take_all_txt_pending(),
            vec![
                "231114_221322    14.090 Rx JTTY     0  0.0 1500 -12  CQ KD8XYZ EN61".to_string(),
                "231114_221329    14.090 Rx JTTY     0  0.0 1500 -12  TU K1ABC".to_string(),
                "231114_221350    14.090 Rx JTTY     0  0.0 1500 -12  QRZ?".to_string(),
            ]
        );
        e.settings.write_all_txt = false;
        e.apply_jtty_updates(s, &[upd(10, "CQ W1AW CQ", 40.0, Terminal::Complete)]);
        assert_eq!(
            e.take_all_txt_pending(),
            Vec::<String>::new(),
            "switched off"
        );
    }

    /// The receive window: one line per message, replaced as it grows; its pane line is
    /// WSJT-X's; it belongs to the RX pane when it was within F Tol of the Rx frequency when
    /// first heard, and stays there as it drifts (WSJT-X's `shouldApplyToQsoHistory`).
    #[test]
    fn the_window_keeps_wsjtxs_lines_and_rx_pane() {
        let mut e = rx_engine();
        // The premise, stated rather than taken from the default: F Tol 50 Hz, so 1530 Hz is
        // within it and 1560 Hz is not.
        e.set_jtty_rx(JTTY_RX_HZ, 50.0).unwrap();
        let s = begun(&mut e);
        let mut far = upd(2, "CQ 3D2AB CQ", 4.0, Terminal::Complete);
        far.freq_hz = 1300.0;
        let mut near = upd(1, "CQ K1ABC", 2.0, Terminal::Growing);
        near.freq_hz = 1530.0;
        e.apply_jtty_updates(s, &[near.clone(), far]);
        near.text = "CQ K1ABC CQ".into();
        near.terminal = Terminal::Complete;
        near.freq_hz = 1560.0; // drifted outside F Tol
        e.apply_jtty_updates(s, &[near]);
        let st = e.jtty_state_at(at(5.0));
        let rows: Vec<_> = st
            .lines
            .iter()
            .map(|l| {
                (
                    l.id,
                    l.text.as_str(),
                    l.line.as_str(),
                    l.state.as_str(),
                    l.rx,
                    l.start_utc_ms,
                    l.dial_hz,
                )
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                (
                    1,
                    "CQ K1ABC CQ",
                    "1560 -12  CQ K1ABC CQ",
                    "complete",
                    true,
                    at(2.0),
                    14_090_000
                ),
                (
                    2,
                    "CQ 3D2AB CQ",
                    "1300 -12  CQ 3D2AB CQ",
                    "complete",
                    false,
                    at(4.0),
                    14_090_000
                ),
            ]
        );
        // Clear empties the window, and what was heard stays heard.
        e.jtty_clear();
        assert!(e.jtty_state_at(at(5.0)).lines.is_empty());
    }

    /// The tap: arming starts a reception when audio arrives; its audio is taken in order with
    /// the operator's search band; a dial move starts a new reception (and the ended one keeps
    /// the band it began with); a stalled decode thread restarts the reception and says so;
    /// disarming stops the tap at once.
    #[test]
    fn the_tap_keeps_each_reception_whole() {
        let mut e = rx_engine();
        e.jtty_feed(&[0.25; 600], T0_MS);
        assert!(e.take_jtty_audio().is_empty(), "disarmed: nothing tapped");
        e.set_jtty_armed(true);
        e.jtty_feed(&[0.25; 600], T0_MS);
        e.jtty_feed(&[-0.25; 600], T0_MS + 50);
        let taken = e.take_jtty_audio();
        assert_eq!(taken.len(), 1);
        let first = taken[0].session;
        assert_eq!(taken[0].pcm.len(), 1200);
        assert_eq!(
            (taken[0].pcm[0], taken[0].pcm[1199]),
            (8192, -8192),
            "capture_to_i16 scale"
        );
        assert_eq!(e.jtty.session(first).unwrap().anchor_ms, T0_MS - 50);
        assert_eq!(taken[0].band, e.jtty.band());

        // The dial moves mid-take: two receptions, each whole, the old one with its own band.
        e.jtty_feed(&[0.0; 300], T0_MS + 100);
        e.set_jtty_rx(1700.0, 100.0).unwrap();
        e.settings.dial_mhz = 7.078;
        e.jtty_feed(&[0.0; 400], T0_MS + 125);
        let taken = e.take_jtty_audio();
        assert_eq!(
            taken
                .iter()
                .map(|a| (a.session, a.pcm.len()))
                .collect::<Vec<_>>(),
            vec![(first, 300), (first + 1, 400)]
        );
        assert_eq!(
            taken[0].band.center_hz, JTTY_RX_HZ,
            "the ended reception keeps its band"
        );
        assert_eq!(taken[1].band.center_hz, 1700.0);
        assert_eq!(e.jtty.session(first + 1).unwrap().dial_hz, 7_078_000);

        // A decode thread that stops taking: past the cap the reception restarts.
        for k in 0..(JTTY_TAP_CAP / 1200) {
            e.jtty_feed(&[0.0; 1200], T0_MS + 200 + k as i64);
        }
        assert!(e.jtty_state_at(T0_MS).notice.is_none());
        e.jtty_feed(&[0.0; 1200], T0_MS + 99_000);
        assert_eq!(
            e.jtty_state_at(T0_MS).notice.as_deref(),
            Some(JTTY_AUDIO_LOST)
        );
        let taken = e.take_jtty_audio();
        assert_eq!(
            taken
                .iter()
                .map(|a| (a.session, a.pcm.len()))
                .collect::<Vec<_>>(),
            vec![(first + 2, 1200)]
        );

        e.set_jtty_armed(false);
        e.jtty_feed(&[0.0; 1200], T0_MS + 100_000);
        assert!(e.take_jtty_audio().is_empty(), "disarmed: the tap stops");
        assert!(!e.jtty_auto_arm(), "an explicit stop is remembered");
        let mut fresh = rx_engine();
        assert!(
            fresh.jtty_auto_arm() && fresh.jtty_armed(),
            "a view entry arms a fresh one"
        );
    }

    /// A reception the decode thread lost is restarted, as WSJT-X restarts one: what is buffered
    /// for it goes, and the next audio starts a new reception. Losing a reception that is no
    /// longer the one being fed changes nothing.
    #[test]
    fn a_lost_reception_restarts() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        e.jtty.restart(s + 7);
        e.jtty_feed(&[0.0; 1200], T0_MS + 100);
        let taken = e.take_jtty_audio();
        assert_eq!(
            taken
                .iter()
                .map(|a| (a.session, a.pcm.len()))
                .collect::<Vec<_>>(),
            vec![(s, 2400)]
        );
        e.jtty_feed(&[0.0; 1200], T0_MS + 200);
        e.jtty.restart(s);
        assert!(
            e.take_jtty_audio().is_empty(),
            "its buffered audio is let go"
        );
        e.jtty_feed(&[0.0; 1200], T0_MS + 300);
        assert_eq!(
            e.take_jtty_audio()
                .iter()
                .map(|a| a.session)
                .collect::<Vec<_>>(),
            vec![s + 1]
        );
    }

    #[test]
    fn the_selection_is_validated() {
        let mut e = rx_engine();
        assert!(e.set_jtty_rx(50.0, 50.0).is_err());
        assert!(e.set_jtty_rx(1500.0, 0.0).is_err());
        assert!(e.set_jtty_rx(1500.0, 600.0).is_err());
        assert!(e.set_jtty_band(2800, 200).is_err());
        assert!(e.set_jtty_band(-1, 2800).is_err());
        assert!(e.set_jtty_band(100, 7000).is_err());
        assert_eq!(
            (e.jtty.rx_hz, e.jtty.ftol_hz, e.jtty.low_hz, e.jtty.high_hz),
            (1500.0, 20.0, 200, 2800),
            "WSJT-X's defaults: Rx 1500, F Tol 20 (its JTTY_FTol default), span 200-2800"
        );
        e.set_jtty_rx(1200.0, 20.0).unwrap();
        e.set_jtty_band(300, 2500).unwrap();
        let st = e.jtty_state_at(T0_MS);
        assert_eq!(
            (st.rx_hz, st.ftol_hz, st.low_hz, st.high_hz),
            (1200.0, 20.0, 300, 2500)
        );
    }

    /// The cockpit's contract, on the wire: camelCase, every field there.
    #[test]
    fn the_state_serializes_as_the_cockpit_reads_it() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        e.apply_jtty_updates(
            s,
            &[
                upd(1, "CQ K1ABC FN42", 3.0, Terminal::Complete),
                upd(2, "CQ K1ABC FN42", 13.0, Terminal::Complete),
            ],
        );
        let v = serde_json::to_value(e.jtty_state_at(at(14.0))).unwrap();
        for key in [
            "armed", "rxHz", "ftolHz", "lowHz", "highHz", "lines", "heard", "notice",
        ] {
            assert!(v.get(key).is_some(), "{key} in {v}");
        }
        let line = &v["lines"][1];
        for key in [
            "id",
            "startUtcMs",
            "freqHz",
            "dialHz",
            "snr",
            "text",
            "line",
            "state",
            "rx",
            "confirmedCalls",
        ] {
            assert!(line.get(key).is_some(), "{key} in {line}");
        }
        assert_eq!(
            v["heard"][0],
            serde_json::json!({
                "call": "K1ABC", "freqHz": 14_091_500, "band": "20m", "snr": -12,
                "grid": "FN42", "atUtcMs": at(13.0)
            })
        );
    }

    // ── through the real decoder ──

    /// `msgs` as the radio loop would capture them (f32, the capture scale), over noise.
    fn captured(secs: f32, msgs: &[(&str, f32, f32)], seed: u64) -> Vec<f32> {
        let n = (secs * 12_000.0) as usize;
        let mut state = seed;
        let mut uniform = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 11) as f32 / (1u64 << 53) as f32
        };
        let mut out: Vec<f32> = (0..n)
            .map(|_| ((0..12).map(|_| uniform()).sum::<f32>() - 6.0) * 0.03)
            .collect();
        for &(text, f0, start) in msgs {
            let enc = jtty::encode(text, jtty::Profile::Unknown).expect("encodes");
            let wave = jtty::gen_wave(&enc.tones, f0).expect("a waveform");
            let at = (start * 12_000.0) as usize;
            for (i, w) in wave.iter().enumerate() {
                if let Some(s) = out.get_mut(at + i) {
                    *s += w * 0.025;
                }
            }
        }
        out
    }

    /// The audio through the whole receive side: the tap, the decode thread's driver (100 ms
    /// pieces, as the radio loop delivers them), the engine's lines; then a disarm, which ends
    /// the reception. Returns the engine for its state.
    fn receive_live(audio: &[f32]) -> Engine {
        receive_live_with(audio, |_| {})
    }

    /// [`receive_live`], with the engine set up by `setup` first (a search band, say).
    fn receive_live_with(audio: &[f32], setup: impl FnOnce(&mut Engine)) -> Engine {
        let mut e = rx_engine();
        setup(&mut e);
        let engine = Mutex::new(e);
        engine_lock(&engine).set_jtty_armed(true);
        let _contexts = receive_contexts();
        let mut driver = JttyDriver::default();
        for (k, piece) in audio.chunks(1200).enumerate() {
            engine_lock(&engine).jtty_feed(piece, T0_MS + 100 * k as i64);
            driver.tick(&engine);
        }
        engine_lock(&engine).set_jtty_armed(false);
        driver.tick(&engine);
        engine.into_inner().unwrap()
    }

    /// DECODE AGAIN (WSJT-X's review of retained audio, `startJttyReview`): the Decode button
    /// decodes every reception's kept audio again and a waterfall row's double-click 40 s of it
    /// from 5 s before the row, each by a receiver of its own; the lines show under the review's
    /// number, at the times they were heard, and NOTHING else happens — no ALL.TXT line, no spot,
    /// no sighting. The live reception of the same audio is the positive control: it logs.
    #[test]
    fn decode_again_shows_what_was_heard_and_logs_nothing() {
        let audio = captured(
            30.0,
            &[
                ("CQ W1AW FN31", 1500.0, 2.0),
                ("CQ K1ABC FN42", 1500.0, 15.0),
            ],
            7,
        );
        let engine = Mutex::new(rx_engine());
        assert_eq!(
            engine_lock(&engine).jtty_decode_again(None),
            Err(JTTY_REVIEW_NOTHING.to_string()),
            "nothing kept yet"
        );
        engine_lock(&engine).set_jtty_armed(true);
        let _contexts = receive_contexts();
        let mut driver = JttyDriver::default();
        for (k, piece) in audio.chunks(1200).enumerate() {
            engine_lock(&engine).jtty_feed(piece, T0_MS + 100 * k as i64);
            driver.tick(&engine);
        }
        let mut e = engine_lock(&engine);
        assert!(
            !e.take_all_txt_pending().is_empty(),
            "CONTROL: live reception logs"
        );
        e.jtty_clear();
        e.take_jtty_spots();
        // What live reception counted toward hearing a station: a review adds nothing to it.
        let senders = e.jtty.senders.clone();
        let heard = e.jtty_state().heard;
        assert!(
            !senders.is_empty(),
            "CONTROL: live reception counts its senders"
        );
        let sample0 = T0_MS - 100;
        e.jtty_decode_again(None).unwrap();
        assert_eq!(e.jtty_state().notice.as_deref(), Some(JTTY_REVIEW_STARTED));
        drop(e);
        for _ in 0..40 {
            driver.tick(&engine);
        }
        let mut e = engine_lock(&engine);
        let st = e.jtty_state();
        let found: Vec<(&str, Option<u64>)> = st
            .lines
            .iter()
            .filter(|l| l.state == "complete")
            .map(|l| (l.text.as_str(), l.review))
            .collect();
        assert_eq!(
            found,
            [("CQ W1AW FN31", Some(1)), ("CQ K1ABC FN42", Some(1))],
            "{:?}",
            st.lines
        );
        let starts: Vec<i64> = st.lines.iter().map(|l| l.start_utc_ms - sample0).collect();
        assert!(
            (starts[0] - 2_000).abs() < 1_000 && (starts[1] - 15_000).abs() < 1_000,
            "heard at their own times: {starts:?}"
        );
        assert!(e.take_all_txt_pending().is_empty(), "a review logs nothing");
        assert!(e.take_jtty_spots().is_empty(), "nor spots");
        assert_eq!(
            e.jtty.senders, senders,
            "nor counts a sender toward the two-decode rule"
        );
        assert_eq!(e.jtty_state().heard, heard, "nor hears a station");
        assert!(
            st.lines.iter().all(|l| l.confirmed_calls.is_empty()),
            "nor confirms a call"
        );
        // A row's double-click: from 5 s before it, so the message at 15 s and not the one at 2.
        e.jtty_clear();
        e.jtty_decode_again(Some(sample0 + 16_000)).unwrap();
        drop(e);
        for _ in 0..40 {
            driver.tick(&engine);
        }
        let mut e = engine_lock(&engine);
        let st = e.jtty_state();
        let found: Vec<(&str, Option<u64>)> = st
            .lines
            .iter()
            .filter(|l| l.state == "complete")
            .map(|l| (l.text.as_str(), l.review))
            .collect();
        assert_eq!(found, [("CQ K1ABC FN42", Some(2))], "{:?}", st.lines);
        // A row older than anything kept: WSJT-X's words.
        assert_eq!(
            e.jtty_decode_again(Some(sample0 - 60_000)),
            Err(JTTY_REVIEW_GONE.to_string())
        );
        assert!(e.take_all_txt_pending().is_empty());
    }

    /// The audio kept for decoding again is WSJT-X's 180 s, the oldest let go first, across
    /// receptions, and a reception's samples stay in their places on its timeline.
    #[test]
    fn the_kept_audio_is_wsjtxs_three_minutes() {
        let mut j = JttyRx::default();
        let second = 12_000;
        j.remember(1, 0, &vec![1; 100 * second]);
        j.remember(2, 0, &vec![2; 50 * second]);
        j.remember(2, 50 * second as i64, &vec![3; 50 * second]);
        let total: usize = j.history.iter().map(|(_, _, p)| p.len()).sum();
        assert_eq!(total, JTTY_HISTORY_SAMPLES);
        assert_eq!(j.kept(1), Some((20 * second as i64, 100 * second as i64)));
        assert_eq!(j.kept(2), Some((0, 100 * second as i64)));
        let pcm = j.kept_pcm(2, 49 * second as i64, 51 * second as i64);
        assert_eq!(pcm.len(), 2 * second);
        assert_eq!((pcm[0], pcm[pcm.len() - 1]), (2, 3));
    }

    /// THE KEPT AUDIO IS HELD IN PIECES OF AT MOST A SECOND, so letting the oldest go moves at most
    /// a second of samples, under the engine lock the keying path also takes, rather than the
    /// whole three minutes on every 20 ms feed; and what is read back across the pieces is the
    /// audio as it was fed, in its places.
    #[test]
    fn the_kept_audio_is_held_in_pieces_and_read_back_whole() {
        let mut j = JttyRx::default();
        let feed = 240; // 20 ms at 12 kHz, the radio loop's step
        let feeds = 200 * 12_000 / feed;
        for k in 0..feeds {
            let at = (k * feed) as i64;
            let pcm: Vec<i16> = (0..feed)
                .map(|n| ((k * feed + n) % 30_000) as i16)
                .collect();
            j.remember(1, at, &pcm);
        }
        // A single long delivery is split the same way.
        j.remember(2, 0, &vec![7; 5 * 12_000]);
        let total: usize = j.history.iter().map(|(_, _, p)| p.len()).sum();
        assert_eq!(total, JTTY_HISTORY_SAMPLES);
        assert!(
            j.history
                .iter()
                .all(|(_, _, p)| p.len() <= JTTY_HISTORY_PIECE),
            "no piece over a second"
        );
        let (first, end) = j.kept(1).unwrap();
        assert_eq!(end, (feeds * feed) as i64);
        assert_eq!(first, end - (JTTY_HISTORY_SAMPLES - 5 * 12_000) as i64);
        let pcm = j.kept_pcm(1, first, end);
        assert_eq!(pcm.len() as i64, end - first);
        assert!(
            pcm.iter()
                .enumerate()
                .all(|(n, &v)| v == ((first as usize + n) % 30_000) as i16),
            "every sample in its place"
        );
        assert_eq!(j.kept(2), Some((0, 5 * 12_000)));
    }

    /// ONE DECODE PRESS IS ONE REVIEW: every reception it decodes again shows under the same
    /// heading, while each reception's decoder keeps its own message numbers, so two messages
    /// numbered alike by two decoders stay two lines. And a review never pushes the live copy
    /// out: review lines and live lines are each kept to the window's 500 on their own.
    #[test]
    fn one_press_is_one_review_and_never_pushes_out_the_live_copy() {
        let mut e = rx_engine();
        let a = e.jtty.start_session(T0_MS, 14_090_000);
        e.jtty.remember(a, 0, &vec![1; 12_000]);
        let b = e.jtty.start_session(T0_MS + 60_000, 14_090_000);
        e.jtty.remember(b, 0, &vec![2; 12_000]);
        e.jtty_decode_again(None).unwrap();
        let reviews = e.take_jtty_reviews();
        assert_eq!(reviews.len(), 2, "both receptions");
        assert_eq!(
            reviews[0].of.group, reviews[1].of.group,
            "under one heading"
        );
        assert_ne!(reviews[0].of.job, reviews[1].of.job, "each its own decoder");
        e.apply_jtty_review(
            &reviews[0].of,
            &[upd(1, "CQ W1AW FN31", 0.0, Terminal::Complete)],
        );
        e.apply_jtty_review(
            &reviews[1].of,
            &[upd(1, "CQ K1ABC FN42", 0.0, Terminal::Complete)],
        );
        let texts: Vec<String> = e
            .jtty_state()
            .lines
            .iter()
            .map(|l| l.text.clone())
            .collect();
        assert_eq!(texts, ["CQ W1AW FN31", "CQ K1ABC FN42"]);
        // A full live window, then a review: the live lines all stay, and so does the review's.
        e.jtty_clear();
        for k in 0..JTTY_LINES_CAP {
            e.jtty
                .push_sent(T0_MS + k as i64, 14_090_000, 1500.0, &format!("LINE {k}"));
        }
        e.apply_jtty_review(
            &reviews[0].of,
            &[upd(9, "CQ W1AW FN31", 0.0, Terminal::Complete)],
        );
        let st = e.jtty_state();
        assert_eq!(
            st.lines.iter().filter(|l| l.review.is_none()).count(),
            JTTY_LINES_CAP
        );
        assert_eq!(st.lines.iter().filter(|l| l.review.is_some()).count(), 1);
        // …and the review's own are held to 500 too.
        let many: Vec<Update> = (0..JTTY_LINES_CAP as i64 + 20)
            .map(|k| upd(100 + k, "CQ W1AW FN31", 0.0, Terminal::Complete))
            .collect();
        e.apply_jtty_review(&reviews[0].of, &many);
        let st = e.jtty_state();
        assert_eq!(
            st.lines.iter().filter(|l| l.review.is_none()).count(),
            JTTY_LINES_CAP
        );
        assert_eq!(
            st.lines.iter().filter(|l| l.review.is_some()).count(),
            JTTY_LINES_CAP
        );
    }

    /// A SPOT CARRIES THE GRID when the message names one, as WSJT-X's does (its third word, when
    /// grid-shaped): the held spot and the confirming one alike; a message without one, none.
    #[test]
    fn a_spot_carries_the_senders_grid() {
        let mut e = rx_engine();
        let s = begun(&mut e);
        e.apply_jtty_updates(
            s,
            &[
                upd(1, "CQ W1AW FN31", 3.0, Terminal::Complete),
                upd(2, "CQ W1AW FN31", 13.0, Terminal::Complete),
                upd(3, "CQ W1AW CQ", 23.0, Terminal::Complete),
            ],
        );
        let grids: Vec<Option<String>> = e.take_jtty_spots().into_iter().map(|s| s.grid).collect();
        assert_eq!(
            grids,
            [Some("FN31".to_string()), Some("FN31".to_string()), None]
        );
    }

    /// OUR OWN ECHO NEVER CONFIRMS A CALL. Two messages naming W1AW that started inside our own
    /// keyed time — our transmission decoded back by a receiver that hears us while we key — are
    /// shown and logged as received, and confirm nothing, spot nothing and put nobody on the
    /// board. The control: the same two messages outside it confirm W1AW, spot it and hear it.
    #[test]
    fn our_own_echo_never_confirms_a_call() {
        for keyed in [true, false] {
            let mut e = rx_engine();
            let s = begun(&mut e);
            if keyed {
                e.jtty.note_keyed(at(2.0), at(30.0));
            }
            e.apply_jtty_updates(
                s,
                &[
                    upd(1, "CQ W1AW CQ", 3.0, Terminal::Complete),
                    upd(2, "CQ W1AW CQ", 13.0, Terminal::Complete),
                ],
            );
            let st = e.jtty_state_at(at(14.0));
            assert_eq!(st.lines.len(), 2, "keyed {keyed}: both shown");
            assert_eq!(
                e.take_all_txt_pending().len(),
                2,
                "keyed {keyed}: both logged"
            );
            let confirmed: Vec<Vec<String>> =
                st.lines.iter().map(|l| l.confirmed_calls.clone()).collect();
            let spotted = spots(&mut e).len();
            let heard = heard_calls(&e, at(14.0));
            if keyed {
                assert_eq!(
                    confirmed,
                    vec![Vec::<String>::new(); 2],
                    "our echo confirmed W1AW"
                );
                assert_eq!(
                    (spotted, heard.len()),
                    (0, 0),
                    "our echo spotted or heard W1AW"
                );
            } else {
                assert_eq!(confirmed[1], vec!["W1AW".to_string()], "control");
                assert_eq!((spotted, heard), (2, vec!["W1AW".to_string()]), "control");
            }
        }
    }

    /// …and while the transmission is still going, what started after it began is ours too.
    #[test]
    fn a_message_starting_while_we_are_still_keyed_is_our_echo() {
        let mut e = rx_engine();
        e.set_jtty_armed(true);
        let now = now_unix_millis() as i64;
        e.jtty_feed(&[0.0; 1200], now);
        let s = e.jtty.sessions.back().expect("a reception").id;
        e.set_jtty_sending(true);
        e.apply_jtty_updates(
            s,
            &[
                upd(1, "CQ W1AW CQ", 0.5, Terminal::Complete),
                upd(2, "CQ W1AW CQ", 1.0, Terminal::Complete),
            ],
        );
        assert!(
            heard_calls(&e, now + 2_000).is_empty(),
            "our echo was heard"
        );
        assert!(spots(&mut e).is_empty());
    }

    /// While JTTY transmits the tap takes nothing (what the receiver hears is our own signal) and
    /// the decode driver ENDS the reception, as WSJT-X ends it on Transmission: a message still
    /// arriving is closed as ended. After the transmission the next audio starts a new
    /// reception. The control: the same audio with nothing keyed leaves the message growing.
    #[test]
    fn the_reception_ends_while_jtty_transmits() {
        // Three frames of a message from 2 s; 5 s of it is two frames in.
        let audio = captured(5.0, &[("CQ CQ DE W1AW W1AW K", 1500.0, 2.0)], 3);
        let _contexts = receive_contexts();
        for keyed in [false, true] {
            let engine = Mutex::new(rx_engine());
            engine_lock(&engine).set_jtty_armed(true);
            let mut driver = JttyDriver::default();
            for (k, piece) in audio.chunks(1200).enumerate() {
                engine_lock(&engine).jtty_feed(piece, T0_MS + 100 * k as i64);
                driver.tick(&engine);
            }
            let first = engine_lock(&engine).jtty.current().map(|s| s.id);
            engine_lock(&engine).set_jtty_sending(keyed);
            engine_lock(&engine).jtty_feed(&[0.5; 1200], T0_MS + 5_000);
            driver.tick(&engine);
            let mut g = engine_lock(&engine);
            let state: Vec<String> = g
                .jtty_state_at(T0_MS)
                .lines
                .iter()
                .map(|l| l.state.clone())
                .collect();
            if keyed {
                assert_eq!(
                    state,
                    vec!["ended".to_string()],
                    "the reception was not ended"
                );
                assert!(
                    g.take_jtty_audio().is_empty(),
                    "the tap took our own signal"
                );
                g.set_jtty_sending(false);
                g.jtty_feed(&[0.0; 1200], T0_MS + 9_000);
                assert_ne!(
                    g.jtty.current().map(|s| s.id),
                    first,
                    "a new reception after it"
                );
            } else {
                assert_eq!(
                    state,
                    vec!["growing".to_string()],
                    "control: still arriving"
                );
            }
        }
    }

    /// The engine's receive side decodes exactly what decoding the same audio as a file does:
    /// every message, its text, frequency, SNR and how it ended — including the one the disarm
    /// cuts off — and each stamped with the reception's anchor plus its start.
    #[test]
    fn the_receive_side_decodes_what_the_file_decode_does() {
        let audio = captured(
            24.0,
            &[
                ("CQ KD9TAW EN52", 1500.0, 1.3),
                ("CQ K1ABC CQ", 1300.0, 6.2),
                ("WB9XYZ 599 0123", 1520.0, 11.0),
                ("CQ KH6ABC KH6ABC", 1500.0, 21.5),
            ],
            11,
        );
        let e = receive_live(&audio);
        let pcm = tempo_core::channel::capture_to_i16(&audio);
        let _contexts = receive_contexts();
        let mut rx = jtty::Receiver::new().expect("a receive context");
        rx.begin(1, 0, 0);
        rx.process(&pcm, 0, pcm.len() as i64, u32::MAX, e.jtty.band())
            .unwrap();
        rx.end(jtty::EndReason::ReceptionEnded);
        let mut last = HashMap::new();
        for u in rx.take_updates() {
            last.insert(u.message_id, u);
        }
        let mut want: Vec<_> = last
            .into_values()
            .map(|u| {
                (
                    T0_MS - 100 + (u.start_s * 1000.0 + 0.5) as i64,
                    strip_filler(&u.text),
                    u.freq_hz.to_bits(),
                    u.snr_db,
                    terminal_label(u.terminal).to_string(),
                )
            })
            .collect();
        want.sort();
        let st = e.jtty_state_at(T0_MS);
        let mut got: Vec<_> = st
            .lines
            .iter()
            .map(|l| {
                (
                    l.start_utc_ms,
                    l.text.clone(),
                    l.freq_hz.to_bits(),
                    l.snr,
                    l.state.clone(),
                )
            })
            .collect();
        got.sort();
        let texts: Vec<&str> = want.iter().map(|w| w.1.as_str()).collect();
        for text in ["CQ KD9TAW EN52", "CQ K1ABC CQ", "WB9XYZ 599 0123"] {
            assert!(texts.contains(&text), "the file decodes {text}: {want:?}");
        }
        assert!(
            want.iter().any(|w| w.4 == "ended"),
            "the disarm cuts one off: {want:?}"
        );
        assert_eq!(got, want);
    }

    /// THE RULE THROUGH THE DECODER (the positive control is the second half). One transmission
    /// of a CQ decodes, shows and is logged, and raises no spot and no heard station; the same
    /// station calling again ten seconds later is spotted and heard.
    #[test]
    fn one_real_transmission_is_not_spotted_and_two_are() {
        let once = receive_live(&captured(12.0, &[("CQ W1AW CQ", 1500.0, 2.0)], 5));
        let mut e = once;
        assert_eq!(
            e.jtty_state_at(T0_MS)
                .lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            vec!["CQ W1AW CQ"]
        );
        assert_eq!(e.take_all_txt_pending().len(), 1);
        assert_eq!(spots(&mut e), vec![]);
        assert!(heard_calls(&e, T0_MS + 12_000).is_empty());

        let mut e = receive_live(&captured(
            22.0,
            &[("CQ W1AW CQ", 1500.0, 2.0), ("CQ W1AW CQ", 1500.0, 12.0)],
            5,
        ));
        let got = spots(&mut e);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            got.iter()
                .all(|s| s.0 == "W1AW" && (14_091_490..=14_091_510).contains(&s.1)),
            "{got:?}"
        );
        assert_eq!(heard_calls(&e, T0_MS + 22_000), vec!["W1AW".to_string()]);
    }

    /// The searched band as WSJT-X searches it: a station at 1300 Hz, well outside Rx 1500 ± 50,
    /// is decoded in the fixed 1200-1500 Hz channel while the waterfall the cockpit reports covers
    /// that channel, and not once the waterfall starts above it. Moving Rx onto it brings it back
    /// even then: the decoder keeps only the fixed channels inside the waterfall
    /// (`jtty_mdecode`'s `channel_window`), never the Rx channel.
    #[test]
    fn the_fixed_channels_are_searched_inside_the_waterfall() {
        let audio = captured(10.0, &[("CQ K1ABC CQ", 1300.0, 2.0)], 9);
        let texts = |e: Engine| -> Vec<String> {
            e.jtty_state_at(T0_MS)
                .lines
                .into_iter()
                .map(|l| l.text)
                .collect()
        };
        assert_eq!(texts(receive_live(&audio)), vec!["CQ K1ABC CQ"]);
        assert_eq!(
            texts(receive_live_with(&audio, |e| e
                .set_jtty_band(1550, 2800)
                .unwrap())),
            Vec::<String>::new(),
            "the 1200-1500 Hz channel is off the waterfall"
        );
        assert_eq!(
            texts(receive_live_with(&audio, |e| {
                e.set_jtty_band(1550, 2800).unwrap();
                e.set_jtty_rx(1300.0, 50.0).unwrap();
            })),
            vec!["CQ K1ABC CQ"],
            "the Rx channel, off the waterfall"
        );
    }

    /// ⭐ WSJT-X keeps F Tol and not Rx: `JTTY_FTol` is saved (20 Hz when its settings have none),
    /// and the Rx frequency is set back to 1500 Hz whenever JTTY is selected
    /// (`on_actionJTTY_triggered`). Here F Tol set in the cockpit lands in Settings, a station
    /// built from those Settings decodes with it, and selecting JTTY again (from another section
    /// or from PSK, the same section's other modem) puts Rx back at 1500 while F Tol stays. A
    /// view re-asserting the JTTY section is not a selection.
    #[test]
    fn f_tol_is_kept_and_rx_goes_back_to_1500_when_jtty_is_selected() {
        let fresh = rx_engine();
        assert_eq!(
            fresh.settings().jtty_ftol_hz,
            20,
            "WSJT-X's JTTY_FTol default"
        );
        assert_eq!(fresh.jtty_state().ftol_hz, 20.0);

        let mut e = rx_engine();
        e.set_operating_mode("jtty", false);
        e.set_jtty_rx(1200.0, 100.0).unwrap();
        assert_eq!(e.settings().jtty_ftol_hz, 100, "F Tol is the one kept");
        let st = e.jtty_state();
        assert_eq!((st.rx_hz, st.ftol_hz), (1200.0, 100.0));

        e.set_operating_mode("jtty", false);
        assert_eq!(
            e.jtty_state().rx_hz,
            1200.0,
            "a re-assert is not a selection"
        );

        e.set_operating_mode("phone", false);
        assert_eq!(e.jtty_state().rx_hz, 1200.0, "leaving changes nothing");
        e.set_operating_mode("jtty", false);
        let st = e.jtty_state();
        assert_eq!((st.rx_hz, st.ftol_hz), (1500.0, 100.0), "selected again");

        e.set_jtty_rx(1300.0, 100.0).unwrap();
        e.set_operating_mode("keyboard", false);
        assert_eq!(e.jtty_state().rx_hz, 1300.0, "PSK leaves JTTY's Rx alone");
        e.set_operating_mode("jtty", false);
        assert_eq!(e.jtty_state().rx_hz, 1500.0, "selected again, from PSK");

        let relaunched = Engine::with_settings(e.settings().clone());
        let st = relaunched.jtty_state();
        assert_eq!((st.rx_hz, st.ftol_hz), (1500.0, 100.0), "the next launch");
    }

    /// The cockpit is F Tol's one writer. A Settings save sent from a panel loaded before the
    /// operator changed it keeps the live value (the waterfall markers' carve-out), and a
    /// restored bundle's value is the decoder's. A value out of the cockpit's range, hand-edited
    /// into a settings file, is held to it.
    #[test]
    fn a_settings_save_keeps_the_live_f_tol_and_a_restore_brings_its_own() {
        let mut e = rx_engine();
        let stale = e.settings().clone();
        e.set_jtty_rx(1500.0, 150.0).unwrap();
        e.apply_settings(stale.clone());
        assert_eq!(
            e.settings().jtty_ftol_hz,
            150,
            "the form's stale 20 did not land"
        );
        assert_eq!(e.jtty_state().ftol_hz, 150.0);

        let mut bundle = stale;
        bundle.jtty_ftol_hz = 300;
        e.apply_restored_settings(bundle.clone());
        assert_eq!(e.settings().jtty_ftol_hz, 300);
        assert_eq!(
            e.jtty_state().ftol_hz,
            300.0,
            "the decoder took the bundle's"
        );

        bundle.jtty_ftol_hz = 0;
        assert_eq!(Engine::with_settings(bundle).jtty_state().ftol_hz, 1.0);
    }

    /// The Settings opt-out (`jtty_rx_auto_arm`, PSK's twin): entering the view does not start
    /// the receiver, and the explicit Arm still does.
    #[test]
    fn the_auto_arm_opt_out_is_honoured_and_arm_still_works() {
        let mut e = rx_engine();
        assert!(e.settings().jtty_rx_auto_arm, "on by default");
        assert!(e.jtty_auto_arm(), "control: the default arms on entry");

        let mut e = rx_engine();
        e.settings.jtty_rx_auto_arm = false;
        assert!(!e.jtty_auto_arm(), "setting off: never auto-arm");
        assert!(!e.jtty_state().armed);
        e.set_jtty_armed(true);
        assert!(e.jtty_state().armed, "the explicit Arm still works");
    }
}
