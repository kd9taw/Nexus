//! The unkey readback: the interlock state machine that turns "we sent `xmit 0`" into "the radio
//! reports READY and no transmitting client", and, for each other kind of transmission, what
//! proves that it ended ([`Profile`]).
//!
//! **A reply is not RF cessation.** `R<seq>|0|` to `xmit 0` means the radio started the
//! transition (D `TCPIP-xmit`). [`StopTracker`] accepts an unkey only after this whole ordered
//! sequence, every step on this connection and within [`TRANSITION_TIMEOUT_MS`]:
//!
//! 1. our `xmit 1` written in full, then its successful reply;
//! 2. `PTT_REQUESTED`, then `TRANSMITTING`, both with source `SW` and our handle;
//! 3. our `xmit 0` written in full, then its successful reply;
//! 4. `UNKEY_REQUESTED`, then `READY`, both still naming our handle;
//! 5. a later `READY` with `tx_client_handle=0x00000000`, no source, transmit allowed, no reason.
//!
//! The sequence was captured on a FLEX-6700 on 4.2.18 (AetherSDR
//! `docs/aetherd-flex-ptt-stop-evidence.md`, as the port plan cites it). In one capture the radio
//! cleared its owner in `NOT_READY` with reason `OUT_OF_BAND`, and that is correctly not an
//! unkey. Any step out of order, a partial or malformed interlock line during the attempt, a
//! foreign owner or a physical keying source, a failed reply or write, a second key, a clock that
//! runs backwards or a missed deadline fails the attempt, and a failed attempt never yields
//! evidence: the caller keeps treating the radio as keyed (spec §10.5; port plan §3.4).
//!
//! Before an attempt is armed, interlock lines only decide readiness: a whole idle sample makes
//! the tracker [`Phase::Idle`], and a partial one withdraws that until a whole one returns
//! (FlexLib 4.2.18 consumes interlock updates as deltas; fields are never merged across lines).
//!
//! Inputs are stamped by the caller at the transport ([`Stamp`]): every write and every raw line,
//! in the one order they happened on this connection. A stamp from another session is ignored; a
//! repeated or reordered one fails the attempt. Time is milliseconds on a monotonic clock, passed
//! in.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's),
//! `src/core/backends/flex/FlexPttStopTracker.h` and `src/core/backends/flex/FlexPttStopTracker.cpp`
//! at commit `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03), translated from C++/Qt to
//! Rust. Deliberate differences: upstream's transmit coordinator types are replaced by a plain
//! [`Operation`] and [`StopRequest`] the session mints, and the coordinator's dispatch and cleanup
//! fences are dropped (the session admits a start before arming, and owns the operation until
//! the proof is consumed); confirmed evidence is consumed by [`StopTracker::consume`] instead of a
//! coordinator acknowledgment; the owner-thread check is dropped (`&mut self` already makes the
//! tracker single-owner); the transition timeout is a constructor argument defaulting to the
//! upstream 5 s, so simulator tests can shorten it. Recorded in the repo-root NOTICE (AetherSDR
//! entry).
//!
//! **The readback per kind** is added for Nexus, with no upstream code taken. Each kind of
//! transmission has a [`Profile`]: the interlock source it keys under, and whose hand ends it.
//!
//! - `xmit 1` ([`KEY`]): the sequence above, unchanged.
//! - The tune carrier ([`TUNE`]): the same ordered sequence under the tune's own source, and the
//!   transmit status must agree: `tune=1` while keyed, `tune=0` after our stop's reply. Either one
//!   alone is not the end.
//! - A CWX send ([`CWX`]) and an ATU cycle ([`ATU`]): the radio ends these by its own hand, so the
//!   proof is a window ([`Window`]). From the start's reply on, every keyed sample names us with
//!   the kind's source, and the end is the radio holding idle: for CWX through a hold after the
//!   last word's expected end, for the ATU once a result ([`ATU_RESULTS`]) has been reported. Later
//!   CWX words join the open window ([`StopTracker::append`]). A `cwx clear` the radio took
//!   empties its buffer ([`StopTracker::cleared`]): from its reply the window waits only for the
//!   hold, and has a transition's time to see it.
//!
//! What fails an `xmit` attempt fails these too: a foreign owner, a physical keying source, a
//! partial line, a failed reply or write, a second start, a missed deadline. The facts behind the
//! three new profiles are AetherSDR's notes and code comments at the same commit, read for facts,
//! and none is confirmed on a real radio: each constant says where its facts come from and what
//! stands in where the notes are silent. Admission refuses those starts until a tester's bench
//! confirms their profiles.
//!
//! **An amplifier's reason** is added for Nexus too, for every kind, `xmit` included: a keying
//! report (PTT_REQUESTED, TRANSMITTING) whose only reason names an amplifier ([`amplifier_only`],
//! `reason=AMP:PG-XL` while the radio waits on a PowerGeniusXL in line, in AetherSDR's notes) is
//! ours when its source and its place in the order are (operator ruling, 2026-10-09). Any other
//! reason, two reasons, or an amplifier's reason on a release or idle report still fails, and the
//! idle that ends an attempt still carries no reason. ⚠️ Unconfirmed until a tester's bench with
//! an amplifier in line.

use std::num::NonZeroU64;

/// How long each transition may take before the attempt fails (upstream `kTransitionTimeoutMs`).
pub const TRANSITION_TIMEOUT_MS: u64 = 5000;

/// The longest raw line the tracker accepts.
pub const MAX_LINE: usize = 4096;

/// What keys the radio for one kind of transmission, and what proves that it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// The interlock's `source` while this kind keys the radio.
    pub source: &'static str,
    /// Whose hand ends it, and so what proves the end.
    pub ending: Ending,
}

/// Whose hand ends a kind of transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Ours: our stop, then the radio's ordered release (UNKEY_REQUESTED, READY still naming us,
    /// READY idle). With `tune_status`, the transmit status must agree as well: `tune=1` while
    /// keyed, `tune=0` after the stop's reply.
    OurStop { tune_status: bool },
    /// The radio's: it ends the transmission by itself, and the proof is a [`Window`]. With
    /// `atu_result`, the window must also see a result ([`ATU_RESULTS`]).
    Radio { atu_result: bool },
}

/// `xmit 1`: the sequence in this file's header, captured on a FLEX-6700 on 4.2.18.
pub const KEY: Profile = Profile {
    source: "SW",
    ending: Ending::OurStop { tune_status: false },
};

/// `transmit tune 1`, the radio's tune carrier. ⚠️ Unconfirmed until a tester's bench. From
/// AetherSDR's notes: the interlock reports `source=TUNE` during a TUNE
/// (`docs/pgxl-telemetry-source-evidence.md`, a FLEX-8600 on SmartSDR 4.2.20.41343); the
/// UNKEY_REQUESTED that follows carries no source (`docs/architecture/digital-voice-thumbdv-waveform.md`);
/// the transmit status carries `tune=` (its transmit decoder). Not in the notes: the READY steps
/// after the unkey, taken to be the `xmit` capture's.
pub const TUNE: Profile = Profile {
    source: "TUNE",
    ending: Ending::OurStop { tune_status: true },
};

/// `cwx send`, keyed by the radio's own break-in. ⚠️ Unconfirmed until a tester's bench, and its
/// source is not in AetherSDR's notes at all: `SW`, the source the radio reports for software
/// keying (`src/models/RadioModel.cpp`, after FlexLib's ParsePTTSource), stands in until the bench
/// reports the real one.
pub const CWX: Profile = Profile {
    source: "SW",
    ending: Ending::Radio { atu_result: false },
};

/// `atu start`, an ATU cycle. ⚠️ Unconfirmed until a tester's bench. AetherSDR's notes give the
/// results ([`ATU_RESULTS`]) and that a start keys the transmitter as a tune does
/// (`docs/agents/flex-protocol.md`), but not the source it keys under: `TUNE`, the tune carrier's,
/// stands in.
pub const ATU: Profile = Profile {
    source: "TUNE",
    ending: Ending::Radio { atu_result: true },
};

/// The `atu status` values that end a cycle: AetherSDR's list (`src/models/TransmitModel.cpp`,
/// after FlexLib's ParseATUTuneStatus) without `NONE`, the status before any cycle, and
/// `TUNE_IN_PROGRESS`. Any other value is not a result.
pub const ATU_RESULTS: [&str; 8] = [
    "TUNE_NOT_STARTED",
    "TUNE_BYPASS",
    "TUNE_SUCCESSFUL",
    "TUNE_OK",
    "TUNE_FAIL_BYPASS",
    "TUNE_FAIL",
    "TUNE_ABORTED",
    "TUNE_MANUAL_BYPASS",
];

/// The longest a tune carrier may be held: the radio loop's own ceiling.
pub const TUNE_HOLD_MAX_MS: u64 = 60_000;

/// How long past its hold the session ends a tune carrier by itself. The radio latches the
/// carrier, and the radio loop that would release it can stall for a decode's length.
pub const TUNE_MARGIN_MS: u64 = 2_000;

/// How long an ATU cycle may take, from its reply to its result and the idle interlock, before
/// the attempt fails. The bench is to measure the real cycle.
pub const ATU_CEILING_MS: u64 = 20_000;

/// How long the radio must hold idle past a CWX word's expected end and its break-in delay before
/// the word counts as ended.
pub const CWX_IDLE_MS: u64 = 300;

/// How long past a CWX word's expected end and its break-in delay the radio may take to fall idle
/// before the attempt fails.
pub const CWX_GRACE_MS: u64 = 1_000;

/// The timing of a radio-ended start's proof. Each CWX word brings its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// How long after its reply the transmission is on the air by design: a CWX word's keying
    /// time at the radio's speed. Nothing for an ATU cycle, whose end the radio reports.
    pub lasting_ms: u64,
    /// How long the radio must then hold idle before the end counts.
    pub hold_ms: u64,
    /// How long past `lasting_ms` the radio has to fall idle, with its result for the ATU, before
    /// the attempt fails.
    pub grace_ms: u64,
}

impl Window {
    /// A CWX word's window: `word_ms` of keying at the radio's speed, then its break-in delay.
    pub fn cwx(word_ms: u64, break_in_delay_ms: u64) -> Window {
        Window {
            lasting_ms: word_ms,
            hold_ms: break_in_delay_ms.saturating_add(CWX_IDLE_MS),
            grace_ms: break_in_delay_ms.saturating_add(CWX_GRACE_MS),
        }
    }

    /// An ATU cycle's window: its result and the idle interlock, within [`ATU_CEILING_MS`].
    pub fn atu() -> Window {
        Window {
            lasting_ms: 0,
            hold_ms: 0,
            grace_ms: ATU_CEILING_MS,
        }
    }
}

/// Where an input happened: the transport session (nonzero, never reused) and its position in
/// that session's single order of writes and lines (strictly increasing from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub session: u64,
    pub ordinal: u64,
}

/// One key attempt, minted by the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Operation(pub NonZeroU64);

/// The stop of one key attempt, minted by the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StopRequest {
    pub operation: Operation,
    pub attempt: NonZeroU64,
}

impl StopRequest {
    pub fn matches(&self, operation: Operation) -> bool {
        self.operation == operation
    }
}

/// Where the tracker is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Disconnected,
    AwaitIdle,
    Idle,
    AwaitKeyWrite,
    AwaitKeyReply,
    AwaitPttRequested,
    AwaitTransmitting,
    Transmitting,
    AwaitStopWrite,
    AwaitStopReply,
    AwaitUnkey,
    AwaitReady,
    AwaitOwnerClear,
    /// A tune's interlock has released, and its transmit status has not yet said `tune=0`.
    AwaitTuneOff,
    /// A radio-ended start is under way: its window is open.
    Window,
    Confirmed,
    Failed,
}

/// Why an attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    None,
    InvalidInput,
    Ordering,
    Timeout,
    Write,
    Reply,
    State,
    Ownership,
    Identity,
    UnsupportedActivity,
}

/// The readback for one connection.
#[derive(Debug, Clone)]
pub struct StopTracker {
    timeout_ms: u64,
    session: u64,
    highest_session: u64,
    last_ordinal: u64,
    client_handle: u32,
    key_sequence: u32,
    stop_sequence: u32,
    last_ms: u64,
    deadline_ms: u64,
    phase: Phase,
    failure: Failure,
    operation: Option<Operation>,
    stop: Option<StopRequest>,
    stop_written: bool,
    stop_replied: bool,
    /// What the armed attempt is.
    profile: Profile,
    /// The transmit status said `tune=1` between the start's reply and the stop's.
    tune_on: bool,
    /// ... and `tune=0` after the stop's reply.
    tune_off: bool,
    /// The open window's timing (the latest word's hold and grace).
    window: Window,
    /// Later starts in the window that await their reply: sequence, keying time, written.
    words: Vec<(u32, u64, bool)>,
    /// When the window's transmission is expected to have ended by design.
    expected_end_ms: u64,
    /// When the window fails unless the radio has settled.
    window_deadline_ms: u64,
    /// Since when every sample has been idle, in the window.
    idle_since_ms: Option<u64>,
    /// The window has seen an ATU result since the last `TUNE_IN_PROGRESS`.
    atu_result: bool,
}

impl Default for StopTracker {
    fn default() -> Self {
        StopTracker::new(TRANSITION_TIMEOUT_MS)
    }
}

/// Whether an interlock `reason` names an amplifier and nothing else: `AMP:<name>`, one name, as
/// the radio reports an amplifier in line it is waiting on (`AMP:PG-XL`, a PowerGeniusXL, in
/// AetherSDR's notes). An empty name, two reasons or any other reason is not.
pub fn amplifier_only(reason: &str) -> bool {
    reason
        .strip_prefix("AMP:")
        .is_some_and(|name| !name.is_empty() && !name.contains(','))
}

/// Upstream's guarded number parse: one to ten digits of `base`, fitting 32 bits. A missing or
/// malformed number is never zero.
fn number(text: &str, base: u32) -> Option<u32> {
    if text.is_empty() || text.len() > 10 {
        return None;
    }
    let mut value: u64 = 0;
    for c in text.chars() {
        let digit = c.to_digit(base)?;
        value = value * u64::from(base) + u64::from(digit);
        if value > u64::from(u32::MAX) {
            return None;
        }
    }
    Some(value as u32)
}

impl StopTracker {
    /// A tracker whose transitions must each complete within `timeout_ms`.
    pub fn new(timeout_ms: u64) -> StopTracker {
        StopTracker {
            timeout_ms,
            session: 0,
            highest_session: 0,
            last_ordinal: 0,
            client_handle: 0,
            key_sequence: 0,
            stop_sequence: 0,
            last_ms: 0,
            deadline_ms: 0,
            phase: Phase::Disconnected,
            failure: Failure::None,
            operation: None,
            stop: None,
            stop_written: false,
            stop_replied: false,
            profile: KEY,
            tune_on: false,
            tune_off: false,
            window: Window::atu(),
            words: Vec::new(),
            expected_end_ms: 0,
            window_deadline_ms: 0,
            idle_since_ms: None,
            atu_result: false,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn failure(&self) -> Failure {
        self.failure
    }

    /// When the current transition must complete, or `0` for none.
    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }

    /// Forget the connection: every observation and pending attempt is dropped.
    pub fn disconnect(&mut self) {
        let highest = self.highest_session;
        let timeout = self.timeout_ms;
        *self = StopTracker::new(timeout);
        self.highest_session = highest;
    }

    /// Start tracking a new transport session. `session` must be nonzero and higher than any
    /// before it; handle zero is no client identity. Otherwise the tracker stays disconnected.
    pub fn reset(&mut self, session: u64, client_handle: u32) {
        self.disconnect();
        if session == 0 || session <= self.highest_session || client_handle == 0 {
            return;
        }
        self.highest_session = session;
        self.session = session;
        self.client_handle = client_handle;
        self.phase = Phase::AwaitIdle;
    }

    fn fail(&mut self, failure: Failure) {
        self.phase = Phase::Failed;
        self.failure = failure;
    }

    fn advance_clock(&mut self, now_ms: u64) -> bool {
        if matches!(self.phase, Phase::Disconnected | Phase::Failed) {
            return false;
        }
        if now_ms < self.last_ms || now_ms > u64::MAX - self.timeout_ms {
            self.fail(Failure::InvalidInput);
            return false;
        }
        self.last_ms = now_ms;
        if self.deadline_ms != 0 && now_ms >= self.deadline_ms {
            self.fail(Failure::Timeout);
            return false;
        }
        self.settle_window();
        true
    }

    /// An open window, at the time of the last input: confirmed once the radio has settled (idle,
    /// no start awaiting its reply, and the ATU's result reported) for its hold past the expected
    /// end; failed if it has not settled by the deadline.
    fn settle_window(&mut self) {
        if self.phase != Phase::Window {
            return;
        }
        let now = self.last_ms;
        let result =
            !matches!(self.profile.ending, Ending::Radio { atu_result: true }) || self.atu_result;
        match self.idle_since_ms {
            Some(idle) if self.words.is_empty() && result => {
                let end = idle
                    .max(self.expected_end_ms)
                    .saturating_add(self.window.hold_ms);
                if now >= end {
                    self.phase = Phase::Confirmed;
                    self.deadline_ms = now.saturating_add(self.timeout_ms);
                }
            }
            _ if now >= self.window_deadline_ms => self.fail(Failure::Timeout),
            _ => {}
        }
    }

    fn accept(&mut self, stamp: Stamp, now_ms: u64) -> bool {
        // A stale input from an earlier session is inert, not a failure of this one. A repeated or
        // reordered input of this session is a failure.
        if stamp.session == 0 || stamp.session != self.session || !self.advance_clock(now_ms) {
            return false;
        }
        if stamp.ordinal == 0 || stamp.ordinal <= self.last_ordinal {
            self.fail(Failure::Ordering);
            return false;
        }
        self.last_ordinal = stamp.ordinal;
        true
    }

    /// Arm for a key attempt from [`Phase::Idle`]. The sequence number must be nonzero and above
    /// any this tracker has seen. Returns whether it armed; on `false` nothing may be written.
    pub fn begin(&mut self, operation: Operation, key_sequence: u32, now_ms: u64) -> bool {
        self.begin_as(operation, KEY, key_sequence, now_ms)
    }

    /// [`Self::begin`] for a kind our own stop ends ([`Ending::OurStop`]: [`KEY`], [`TUNE`]).
    pub fn begin_as(
        &mut self,
        operation: Operation,
        profile: Profile,
        key_sequence: u32,
        now_ms: u64,
    ) -> bool {
        matches!(profile.ending, Ending::OurStop { .. })
            && self.arm(operation, profile, key_sequence, now_ms)
    }

    /// Arm for a start the radio ends by itself ([`Ending::Radio`]: [`CWX`], [`ATU`]), with its
    /// window's timing. `end` is what [`Self::evidence`] hands back once the window proves the
    /// end: a stop request the session mints for the operation, though no stop need be sent.
    pub fn begin_window(
        &mut self,
        operation: Operation,
        end: StopRequest,
        profile: Profile,
        key_sequence: u32,
        window: Window,
        now_ms: u64,
    ) -> bool {
        if !matches!(profile.ending, Ending::Radio { .. })
            || !end.matches(operation)
            || !self.arm(operation, profile, key_sequence, now_ms)
        {
            return false;
        }
        self.stop = Some(end);
        self.window = window;
        true
    }

    /// One more start in the open window (a later CWX word), with its own timing. Its sequence
    /// number must be above any this tracker has seen. Returns whether it was taken; on `false`
    /// nothing may be written.
    pub fn append(
        &mut self,
        operation: Operation,
        sequence: u32,
        window: Window,
        now_ms: u64,
    ) -> bool {
        if !self.advance_clock(now_ms)
            || self.phase != Phase::Window
            || self.operation != Some(operation)
            || sequence <= self.key_sequence
        {
            return false;
        }
        self.key_sequence = sequence;
        self.words.push((sequence, window.lasting_ms, false));
        self.window.hold_ms = window.hold_ms;
        self.window.grace_ms = window.grace_ms;
        // The word's write and reply get a transition's time, as a key's do.
        self.window_deadline_ms = self
            .window_deadline_ms
            .max(now_ms.saturating_add(self.timeout_ms));
        true
    }

    /// The radio took a `cwx clear` (its reply, at `now_ms`): nothing more is to be sent, so the
    /// open window no longer waits out the last word's expected end, only the radio holding idle
    /// for its hold, and has a transition's time from here to see it, as every stop does. A word
    /// still awaiting its reply keeps its own timing; outside an open window nothing changes.
    pub fn cleared(&mut self, now_ms: u64) {
        if !self.advance_clock(now_ms) || self.phase != Phase::Window || !self.words.is_empty() {
            return;
        }
        self.expected_end_ms = self.expected_end_ms.min(now_ms);
        self.window_deadline_ms = self
            .window_deadline_ms
            .max(now_ms.saturating_add(self.timeout_ms));
        self.settle_window();
    }

    fn arm(
        &mut self,
        operation: Operation,
        profile: Profile,
        key_sequence: u32,
        now_ms: u64,
    ) -> bool {
        if !self.advance_clock(now_ms) {
            return false;
        }
        if self.phase != Phase::Idle
            || key_sequence == 0
            || key_sequence <= self.key_sequence
            || key_sequence <= self.stop_sequence
        {
            return false;
        }
        self.operation = Some(operation);
        self.stop = None;
        self.stop_written = false;
        self.stop_replied = false;
        self.profile = profile;
        self.tune_on = false;
        self.tune_off = false;
        self.words.clear();
        self.idle_since_ms = None;
        self.atu_result = false;
        self.key_sequence = key_sequence;
        self.stop_sequence = 0;
        self.deadline_ms = now_ms + self.timeout_ms;
        self.phase = Phase::AwaitKeyWrite;
        true
    }

    /// The armed attempt's profile (the last one's once it is over).
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Bind the stop for the armed attempt. Allowed once, from the key's reply up to
    /// transmitting; anything else fails the attempt (the caller still writes its unkey).
    pub fn request_stop(
        &mut self,
        operation: Operation,
        request: StopRequest,
        stop_sequence: u32,
        now_ms: u64,
    ) -> bool {
        if !self.advance_clock(now_ms) {
            return false;
        }
        if self.operation != Some(operation)
            || !request.matches(operation)
            || stop_sequence <= self.key_sequence
        {
            self.fail(Failure::Identity);
            return false;
        }
        if self.stop.is_some()
            || self.phase < Phase::AwaitKeyReply
            || self.phase > Phase::Transmitting
        {
            self.fail(Failure::State);
            return false;
        }
        self.stop = Some(request);
        self.stop_sequence = stop_sequence;
        self.deadline_ms = now_ms + self.timeout_ms;
        if self.phase == Phase::Transmitting {
            self.phase = Phase::AwaitStopWrite;
        }
        true
    }

    /// A write that ended at the transport: the key, or a later word in an open window
    /// (`keying`), or the unkey. Only a full write of the exact command counts; sequence zero is
    /// reserved. Any other transmit-capable write during an attempt fails it.
    pub fn command_written(
        &mut self,
        stamp: Stamp,
        sequence: u32,
        keying: bool,
        full_write: bool,
        now_ms: u64,
    ) {
        if !self.accept(stamp, now_ms) {
            return;
        }
        if !full_write || sequence == 0 {
            self.fail(Failure::Write);
        } else if self.operation.is_none() {
            self.fail(Failure::Identity);
        } else if self.phase == Phase::AwaitKeyWrite && keying && sequence == self.key_sequence {
            self.phase = Phase::AwaitKeyReply;
        } else if !self.stop_written
            && !keying
            && sequence == self.stop_sequence
            && self.stop_matches()
        {
            self.stop_written = true;
            if self.phase == Phase::AwaitStopWrite {
                self.phase = Phase::AwaitStopReply;
            }
        } else if self.phase == Phase::Window && keying && self.word_written(sequence) {
            // Taken: the word now awaits its reply.
        } else {
            self.fail(Failure::UnsupportedActivity);
        }
    }

    /// A later word's write, in the open window: each is taken once.
    fn word_written(&mut self, sequence: u32) -> bool {
        match self.words.iter_mut().find(|w| w.0 == sequence && !w.2) {
            Some(word) => {
                word.2 = true;
                true
            }
            None => false,
        }
    }

    fn stop_matches(&self) -> bool {
        matches!((self.stop, self.operation), (Some(s), Some(o)) if s.matches(o))
    }

    fn response(&mut self, body: &str) {
        let Some((seq, rest)) = body.split_once('|') else {
            self.fail(Failure::Reply);
            return;
        };
        let Some(sequence) = number(seq, 10).filter(|s| *s != 0) else {
            self.fail(Failure::Reply);
            return;
        };
        let result = rest.split_once('|').and_then(|(code, _)| number(code, 16));
        if self.phase == Phase::Window {
            self.word_reply(sequence, result == Some(0));
            return;
        }
        if sequence != self.key_sequence && sequence != self.stop_sequence {
            return;
        }
        if result != Some(0) {
            self.fail(Failure::Reply);
        } else if sequence == self.key_sequence && self.phase == Phase::AwaitKeyReply {
            if matches!(self.profile.ending, Ending::Radio { .. }) {
                self.open_window();
            } else {
                self.phase = Phase::AwaitPttRequested;
            }
        } else if sequence == self.stop_sequence
            && self.stop_written
            && !self.stop_replied
            && self.stop_matches()
        {
            self.stop_replied = true;
            if self.phase == Phase::AwaitStopReply {
                self.phase = Phase::AwaitUnkey;
            }
        } else {
            self.fail(Failure::Ordering);
        }
    }

    /// The radio took a radio-ended start: from here its window proves the end. The tracker was
    /// idle when it armed, and no sample has arrived since (any would have failed the attempt), so
    /// the radio is idle as the window opens.
    fn open_window(&mut self) {
        self.phase = Phase::Window;
        self.deadline_ms = 0;
        self.expected_end_ms = self.last_ms.saturating_add(self.window.lasting_ms);
        self.window_deadline_ms = self.expected_end_ms.saturating_add(self.window.grace_ms);
        self.idle_since_ms = Some(self.last_ms);
    }

    /// A reply in the open window. A later word's sets when the transmission is expected to end;
    /// any other (a stop's, another command's) is not the window's.
    fn word_reply(&mut self, sequence: u32, ok: bool) {
        let Some(i) = self.words.iter().position(|w| w.0 == sequence) else {
            return;
        };
        if !self.words[i].2 {
            self.fail(Failure::Ordering);
        } else if !ok {
            self.fail(Failure::Reply);
        } else {
            let (_, lasting, _) = self.words.remove(i);
            self.expected_end_ms = self
                .expected_end_ms
                .max(self.last_ms)
                .saturating_add(lasting);
            self.window_deadline_ms = self.expected_end_ms.saturating_add(self.window.grace_ms);
        }
    }

    /// A transmit status line during a tune: `tune=1` between the start's reply and the stop's,
    /// `tune=0` after the stop's reply. Anything else in between fails the attempt. A line without
    /// `tune` says nothing about the carrier, and other kinds do not read it.
    fn transmit_status(&mut self, envelope: &str, body: &str) {
        if self.profile.ending != (Ending::OurStop { tune_status: true })
            || matches!(
                self.phase,
                Phase::Disconnected | Phase::AwaitIdle | Phase::Idle | Phase::Failed
            )
        {
            return;
        }
        let mut tune = None;
        for token in body.split(' ') {
            if let Some(value) = token.strip_prefix("tune=") {
                if tune.replace(value).is_some() {
                    self.fail(Failure::InvalidInput);
                    return;
                }
            }
        }
        let on = match tune {
            None => return,
            Some("1") => true,
            Some("0") => false,
            Some(_) => {
                self.fail(Failure::InvalidInput);
                return;
            }
        };
        if !self.envelope_is_ours(envelope) {
            self.fail(Failure::Ownership);
            return;
        }
        let keyed = self.phase > Phase::AwaitKeyReply;
        match on {
            true if keyed && !self.stop_replied => self.tune_on = true,
            false if self.stop_replied && self.tune_on => {
                self.tune_off = true;
                if self.phase == Phase::AwaitTuneOff {
                    self.phase = Phase::Confirmed;
                }
            }
            _ => self.fail(Failure::State),
        }
    }

    /// An `atu` status line during an ATU cycle: a result after the start's reply counts toward
    /// the end, and `TUNE_IN_PROGRESS` withdraws an earlier one. Before the reply, any status
    /// fails the attempt, as an interlock sample does.
    fn atu_status(&mut self, envelope: &str, body: &str) {
        if self.profile.ending != (Ending::Radio { atu_result: true })
            || !matches!(
                self.phase,
                Phase::AwaitKeyWrite | Phase::AwaitKeyReply | Phase::Window | Phase::Confirmed
            )
        {
            return;
        }
        let mut status = None;
        for token in body.split(' ') {
            if let Some(value) = token.strip_prefix("status=") {
                if status.replace(value).is_some() {
                    self.fail(Failure::InvalidInput);
                    return;
                }
            }
        }
        let Some(status) = status else { return };
        if !self.envelope_is_ours(envelope) {
            self.fail(Failure::Ownership);
            return;
        }
        match self.phase {
            Phase::Window if status == "TUNE_IN_PROGRESS" => self.atu_result = false,
            Phase::Window if ATU_RESULTS.contains(&status) => self.atu_result = true,
            Phase::Window => {}
            // A new cycle after the proof revokes it.
            Phase::Confirmed if status != "TUNE_IN_PROGRESS" => {}
            _ => self.fail(Failure::State),
        }
    }

    /// A status envelope's handle is no client's or ours.
    fn envelope_is_ours(&self, envelope: &str) -> bool {
        matches!(number(envelope, 16), Some(h) if h == 0 || h == self.client_handle)
    }

    fn interlock(&mut self, body: &str) {
        let mut state = None;
        let mut source = None;
        let mut handle_text = None;
        let mut allowed = None;
        let mut reason = None;
        let mut present = 0u32;
        let mut malformed = false;
        for token in body.split(' ') {
            let Some((key, value)) = token.split_once('=') else {
                malformed |= !token.is_empty();
                continue;
            };
            let (bit, slot) = match key {
                "state" => (1, &mut state),
                "source" => (2, &mut source),
                "tx_client_handle" => (4, &mut handle_text),
                "tx_allowed" => (8, &mut allowed),
                "reason" => (16, &mut reason),
                _ => continue,
            };
            if present & bit != 0 {
                self.fail(Failure::InvalidInput);
                return;
            }
            present |= bit;
            *slot = Some(value);
        }
        if present == 0 {
            return; // timing configuration, not a state line
        }
        let mut handle = 0;
        if let Some(text) = handle_text {
            match text
                .strip_prefix("0x")
                .filter(|d| d.len() == 8)
                .and_then(|d| number(d, 16))
            {
                Some(h) => handle = h,
                None => malformed = true,
            }
        }
        if malformed || allowed.is_some_and(|a| a != "0" && a != "1") {
            self.fail(Failure::InvalidInput);
            return;
        }
        let (state, source, allowed, reason) = (
            state.unwrap_or_default(),
            source.unwrap_or_default(),
            allowed.unwrap_or_default(),
            reason.unwrap_or_default(),
        );
        let idle = present == 31
            && state == "READY"
            && source.is_empty()
            && handle == 0
            && allowed == "1"
            && reason.is_empty();
        if matches!(self.phase, Phase::AwaitIdle | Phase::Idle) {
            // Before arming, a partial sample withdraws readiness and a later whole idle restores
            // it. Fields are never combined across lines into evidence.
            self.phase = if idle { Phase::Idle } else { Phase::AwaitIdle };
            return;
        }
        if present != 31 {
            self.fail(Failure::InvalidInput);
            return;
        }
        let keyed_by = self.profile.source;
        if (handle != 0 && handle != self.client_handle)
            || (!source.is_empty() && source != keyed_by)
        {
            self.fail(Failure::Ownership);
            return;
        }
        // A keying report whose only reason names an amplifier is ours when everything else is:
        // its source above, and its place in the order below. Any other reason, or one on any
        // other report, fails the attempt.
        let keying = matches!(state, "PTT_REQUESTED" | "TRANSMITTING");
        if allowed != "1" || !(reason.is_empty() || (keying && amplifier_only(reason))) {
            self.fail(Failure::State);
            return;
        }
        let ours = handle == self.client_handle;
        if self.phase == Phase::Window {
            // The radio keys and releases as it goes; every sample names us, keyed with the kind's
            // source and released with none, until it holds idle.
            match state {
                _ if idle => {
                    self.idle_since_ms.get_or_insert(self.last_ms);
                }
                "PTT_REQUESTED" | "TRANSMITTING" if source == keyed_by && ours => {
                    self.idle_since_ms = None;
                }
                "UNKEY_REQUESTED" | "READY" if source.is_empty() && ours => {
                    self.idle_since_ms = None;
                }
                _ => self.fail(Failure::State),
            }
            return;
        }
        match self.phase {
            Phase::AwaitPttRequested if state == "PTT_REQUESTED" && source == keyed_by && ours => {
                self.phase = Phase::AwaitTransmitting;
            }
            Phase::AwaitTransmitting if state == "TRANSMITTING" && source == keyed_by && ours => {
                self.phase = Phase::Transmitting;
                if self.stop.is_some() {
                    self.phase = if self.stop_replied {
                        Phase::AwaitUnkey
                    } else if self.stop_written {
                        Phase::AwaitStopReply
                    } else {
                        Phase::AwaitStopWrite
                    };
                } else {
                    self.deadline_ms = 0;
                }
            }
            // The same state again is not a new keying operation.
            Phase::Transmitting if state == "TRANSMITTING" && source == keyed_by && ours => {}
            Phase::AwaitUnkey if state == "UNKEY_REQUESTED" && source.is_empty() && ours => {
                self.phase = Phase::AwaitReady;
            }
            Phase::AwaitReady if state == "READY" && source.is_empty() && ours => {
                self.phase = Phase::AwaitOwnerClear;
            }
            // A tune's release is the end only once its transmit status has said `tune=0` too.
            Phase::AwaitOwnerClear if idle && self.stop_matches() => {
                let tune_status = self.profile.ending == (Ending::OurStop { tune_status: true });
                self.phase = if tune_status && !self.tune_off {
                    Phase::AwaitTuneOff
                } else {
                    Phase::Confirmed
                };
            }
            Phase::AwaitTuneOff | Phase::Confirmed if idle && self.stop_matches() => {}
            _ => self.fail(Failure::State),
        }
    }

    /// A raw line from the radio, exactly as it arrived (terminator stripped).
    pub fn observe(&mut self, stamp: Stamp, raw_line: &str, now_ms: u64) {
        if !self.accept(stamp, now_ms) {
            return;
        }
        if raw_line.is_empty() || raw_line.len() > MAX_LINE || raw_line.contains(['\n', '\r', '\0'])
        {
            self.fail(Failure::InvalidInput);
            return;
        }
        if let Some(body) = raw_line.strip_prefix('R') {
            self.response(body);
        } else if let Some(rest) = raw_line.strip_prefix('S') {
            let Some((envelope, body)) = rest.split_once('|') else {
                self.fail(Failure::InvalidInput);
                return;
            };
            if let Some(body) = body.strip_prefix("transmit ") {
                self.transmit_status(envelope, body);
            } else if let Some(body) = body.strip_prefix("atu ") {
                self.atu_status(envelope, body);
            } else if let Some(body) = body.strip_prefix("interlock ") {
                // Interlock band and timing configuration is not a state sample.
                if body.starts_with("band ") {
                    return;
                }
                if !self.envelope_is_ours(envelope) {
                    self.fail(Failure::Ownership);
                    return;
                }
                self.interlock(body);
            }
        }
        self.settle_window();
    }

    /// Let time pass: a missed deadline fails the attempt.
    pub fn poll(&mut self, now_ms: u64) {
        let _ = self.advance_clock(now_ms);
    }

    /// The stop request whose unkey the radio has confirmed, while the confirmation is current:
    /// confirmed, within the deadline, and matching the armed attempt. Only a candidate until the
    /// caller [`consume`](Self::consume)s it.
    pub fn evidence(&self, now_ms: u64) -> Option<StopRequest> {
        if self.phase != Phase::Confirmed || now_ms < self.last_ms || now_ms >= self.deadline_ms {
            return None;
        }
        self.stop.filter(|s| Some(s.operation) == self.operation)
    }

    /// The caller took the evidence: back to idle, ready for a fresh attempt with a higher
    /// sequence number. Nothing happens unless `request` is the confirmed one.
    pub fn consume(&mut self, request: StopRequest, now_ms: u64) -> bool {
        if self.evidence(now_ms) != Some(request) {
            return false;
        }
        self.phase = Phase::Idle;
        self.deadline_ms = 0;
        self.operation = None;
        self.stop = None;
        true
    }
}

#[cfg(test)]
mod tests {
    //! Translated from upstream's `flex_ptt_stop_tracker_test.cpp`. The FLEX-6700 4.2.18 frames
    //! below are the capture's shapes with the handle normalized (input frames, never a radio).
    //! Rows that test upstream's coordinator (the entered-writer barrier, a superseding stop
    //! token, the off-thread inspection) have no counterpart here and are noted where they fall.
    use super::*;

    const IDLE: &str = "S0|interlock tx_client_handle=0x00000000 state=READY reason= source= tx_allowed=1 amplifier=";
    const REQUESTED: &str = "S0|interlock tx_client_handle=0x12345678 state=PTT_REQUESTED reason= source=SW tx_allowed=1 amplifier=";
    const TRANSMITTING: &str = "S0|interlock tx_client_handle=0x12345678 state=TRANSMITTING reason= source=SW tx_allowed=1 amplifier=";
    const UNKEY: &str = "S0|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source= tx_allowed=1 amplifier=";
    const READY_OWNED: &str = "S0|interlock tx_client_handle=0x12345678 state=READY reason= source= tx_allowed=1 amplifier=";
    const OUT_OF_BAND: &str = "S0|interlock tx_client_handle=0x00000000 state=NOT_READY reason=OUT_OF_BAND source= tx_allowed=0 amplifier=";

    fn op(n: u64) -> Operation {
        Operation(NonZeroU64::new(n).unwrap())
    }

    fn stop_of(operation: Operation) -> StopRequest {
        StopRequest {
            operation,
            attempt: NonZeroU64::new(1).unwrap(),
        }
    }

    struct Harness {
        now: u64,
        ordinal: u64,
        operation: Operation,
        stop: StopRequest,
        tracker: StopTracker,
    }

    impl Harness {
        fn new() -> Harness {
            let mut h = Harness {
                now: 100,
                ordinal: 0,
                operation: op(1),
                stop: stop_of(op(1)),
                tracker: StopTracker::default(),
            };
            h.tracker.reset(1, 0x1234_5678);
            h.feed(IDLE);
            h
        }
        fn feed(&mut self, line: &str) {
            self.ordinal += 1;
            let stamp = Stamp {
                session: 1,
                ordinal: self.ordinal,
            };
            self.tracker.observe(stamp, line, self.now);
        }
        fn written(&mut self, sequence: u32, key: bool, success: bool) {
            self.ordinal += 1;
            let stamp = Stamp {
                session: 1,
                ordinal: self.ordinal,
            };
            self.tracker
                .command_written(stamp, sequence, key, success, self.now);
        }
        fn start(&mut self) {
            assert!(
                self.tracker.begin(self.operation, 101, self.now),
                "arm the key"
            );
            self.written(101, true, true);
            self.feed("R101|0|");
            self.feed(REQUESTED);
            self.feed(TRANSMITTING);
            assert_eq!(
                self.tracker.phase(),
                Phase::Transmitting,
                "own PTT observed"
            );
        }
        fn stop_request(&mut self) {
            assert!(
                self.tracker
                    .request_stop(self.operation, self.stop, 102, self.now),
                "bind the stop"
            );
        }
        fn stop_reply(&mut self) {
            self.stop_request();
            self.written(102, false, true);
            self.feed("R102|0|");
        }
        fn complete(&mut self) {
            self.stop_reply();
            self.feed(UNKEY);
            self.feed(READY_OWNED);
            self.feed(IDLE);
        }
        fn evidence(&self) -> Option<StopRequest> {
            self.tracker.evidence(self.now)
        }
    }

    #[test]
    fn the_captured_sequence_confirms_and_hands_off() {
        // capturedSequenceAndHandoff.
        let mut h = Harness::new();
        h.start();
        h.stop_reply();
        assert_eq!(
            h.evidence(),
            None,
            "a successful xmit 0 reply is not stop proof"
        );
        h.feed(UNKEY);
        assert_eq!(h.evidence(), None, "UNKEY_REQUESTED is transitional");
        h.feed(READY_OWNED);
        assert_eq!(
            h.evidence(),
            None,
            "READY still naming us is not the release"
        );
        h.now += 427; // the measured gap, not a grace period
        h.feed(IDLE);
        assert_eq!(
            h.evidence(),
            Some(h.stop),
            "the owner clear completes the sequence"
        );
        assert!(h.tracker.consume(h.stop, h.now));
        assert_eq!(h.evidence(), None, "consumed: the candidate is retired");
        // A fresh operation and sequence can begin after consumption, even past the old deadline.
        h.now += 6000;
        h.tracker.poll(h.now);
        assert!(h.tracker.begin(op(2), 103, h.now));
        // The old proof does not match the new operation.
        assert!(!h.tracker.consume(h.stop, h.now));
    }

    #[test]
    fn missing_and_reordered_transitions_never_confirm() {
        // missingAndReorderedTransitions.
        let mut no_unkey = Harness::new();
        no_unkey.start();
        no_unkey.stop_reply();
        no_unkey.feed(READY_OWNED);
        no_unkey.feed(IDLE);
        assert_eq!(no_unkey.tracker.phase(), Phase::Failed);
        assert_eq!(
            no_unkey.evidence(),
            None,
            "idle cannot fill in a missing UNKEY_REQUESTED"
        );

        let mut no_owned_ready = Harness::new();
        no_owned_ready.start();
        no_owned_ready.stop_reply();
        no_owned_ready.feed(UNKEY);
        no_owned_ready.feed(IDLE);
        assert_eq!(
            no_owned_ready.tracker.phase(),
            Phase::Failed,
            "owner clear cannot skip READY"
        );

        let mut before_reply = Harness::new();
        before_reply.start();
        before_reply.stop_request();
        before_reply.written(102, false, true);
        before_reply.feed(UNKEY);
        before_reply.feed("R102|0|");
        before_reply.feed(READY_OWNED);
        before_reply.feed(IDLE);
        assert_eq!(
            before_reply.evidence(),
            None,
            "status before the stop's reply cannot be relabelled as later evidence"
        );

        // A stop bound before keying was observed does not wait for the key to show; a
        // pre-existing idle is not no-dispatch proof.
        let mut early = Harness::new();
        assert!(early.tracker.begin(early.operation, 101, early.now));
        early.written(101, true, true);
        assert!(early
            .tracker
            .request_stop(early.operation, early.stop, 102, early.now));
        early.feed(IDLE);
        assert_eq!(early.evidence(), None);

        let mut duplicate = Harness::new();
        duplicate.start();
        duplicate.stop_reply();
        let stamp = Stamp {
            session: 1,
            ordinal: duplicate.ordinal,
        };
        duplicate.tracker.observe(stamp, UNKEY, duplicate.now);
        assert_eq!(
            duplicate.tracker.failure(),
            Failure::Ordering,
            "a repeated ordinal poisons"
        );
    }

    #[test]
    fn strict_frames_and_ownership() {
        // strictFramesAndOwnership: no malformed or failed reply becomes result zero.
        for reply in [
            "R102|garbage|",
            "R102||",
            "R102|",
            "R102|100000000|",
            "R102|F3000001|",
            "R102|+0|",
            "R102| 0|",
            "R4294967296|0|",
        ] {
            let mut h = Harness::new();
            h.start();
            h.stop_request();
            h.written(102, false, true);
            h.feed(reply);
            assert_eq!(h.tracker.failure(), Failure::Reply, "{reply}");
        }
        // Malformed, partial, foreign or physically keyed status cannot authorize the handoff.
        for status in [
            "S0|interlock state=UNKEY_REQUESTED",
            "S0|interlock tx_client_handle=garbage state=UNKEY_REQUESTED reason= source= tx_allowed=1",
            "S0|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED state=READY reason= source= tx_allowed=1",
            "S0|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source=SW source= tx_allowed=1",
            "S0|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source= SW tx_allowed=1",
            "Sg|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source= tx_allowed=1",
            "S0|interlock tx_client_handle=0x87654321 state=UNKEY_REQUESTED reason= source= tx_allowed=1",
            "S87654321|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source= tx_allowed=1",
            "S0|interlock tx_client_handle=0x12345678 state=UNKEY_REQUESTED reason= source=MIC tx_allowed=1",
        ] {
            let mut h = Harness::new();
            h.start();
            h.stop_reply();
            h.feed(status);
            h.feed(READY_OWNED);
            h.feed(IDLE);
            assert_eq!(h.tracker.phase(), Phase::Failed, "{status}");
            assert_eq!(h.evidence(), None, "{status}");
        }
        // The capture's OUT_OF_BAND owner clear is not a qualified READY handoff.
        let mut oob = Harness::new();
        oob.start();
        oob.stop_reply();
        oob.feed(UNKEY);
        oob.feed(READY_OWNED);
        oob.feed(OUT_OF_BAND);
        assert_eq!(oob.evidence(), None);
        // Unrelated replies, timing lines, band configuration and other status do not stand in for
        // transitions, and do not disturb them.
        let mut unrelated = Harness::new();
        unrelated.start();
        unrelated.stop_reply();
        unrelated.feed("R999|0|");
        unrelated.feed("S0|interlock acc_tx_delay=0 tx_delay=0 timeout=0");
        unrelated.feed("S0|interlock band 24 band_name=GEN acc_tx_enabled=0");
        unrelated.feed("S0|atu status=TUNE_BYPASS atu_enabled=1 memories_enabled=1 using_mem=1");
        unrelated.feed(UNKEY);
        unrelated.feed(READY_OWNED);
        unrelated.feed(IDLE);
        assert!(unrelated.evidence().is_some());
    }

    #[test]
    fn partial_interlock_before_arming() {
        // partialInterlockBeforeArming.
        let deltas = [
            "S0|interlock tx_allowed=0",
            "S0|interlock tx_allowed=1",
            "S0|interlock state=READY",
            "S0|interlock source=",
            "S0|interlock reason=",
            "S0|interlock tx_client_handle=0x00000000",
        ];
        for delta in deltas {
            let mut h = Harness::new();
            h.feed(delta);
            assert_eq!(
                h.tracker.phase(),
                Phase::AwaitIdle,
                "{delta} withdraws idle"
            );
            assert_eq!(
                h.tracker.failure(),
                Failure::None,
                "{delta} does not poison"
            );
            assert!(
                !h.tracker.begin(h.operation, 101, h.now),
                "partial idle never arms"
            );
            h.feed(IDLE);
            assert_eq!(h.tracker.phase(), Phase::Idle, "a whole idle recovers");
            h.start();
            h.complete();
            assert!(
                h.evidence().is_some(),
                "the recovered session still proves a stop"
            );

            let mut active = Harness::new();
            active.start();
            active.feed(delta);
            active.feed(IDLE);
            assert_eq!(
                active.tracker.phase(),
                Phase::Failed,
                "{delta} during an attempt"
            );
            assert_eq!(active.evidence(), None);
        }
        // Initial partial updates never accumulate into an idle sample.
        let mut fresh = StopTracker::default();
        fresh.reset(1, 0x1234_5678);
        let mut ordinal = 0;
        for delta in deltas {
            ordinal += 1;
            fresh.observe(
                Stamp {
                    session: 1,
                    ordinal,
                },
                delta,
                100,
            );
            assert_eq!(fresh.phase(), Phase::AwaitIdle);
            assert_eq!(fresh.failure(), Failure::None);
        }
        ordinal += 1;
        fresh.observe(
            Stamp {
                session: 1,
                ordinal,
            },
            IDLE,
            100,
        );
        assert_eq!(fresh.phase(), Phase::Idle);
        // Malformed fields fail closed before arming.
        for malformed in [
            "S0|interlock tx_allowed=garbage",
            "S0|interlock tx_client_handle=garbage",
            "S0|interlock tx_allowed=1 tx_allowed=0",
            "S0|interlock state=READY junk",
        ] {
            let mut h = Harness::new();
            h.feed(malformed);
            h.feed(IDLE);
            assert_eq!(h.tracker.failure(), Failure::InvalidInput, "{malformed}");
        }
    }

    #[test]
    fn attempt_identity_and_reconnect() {
        // attemptIdentityAndReconnect. Upstream's superseding-token row has no counterpart: the
        // session mints one stop request per operation.
        let mut h = Harness::new();
        h.start();
        h.complete();
        assert!(
            !h.tracker.request_stop(h.operation, h.stop, 103, h.now),
            "a new stop cannot be attached to an old READY sequence"
        );
        // A stop request for another operation cannot bind this one.
        let mut wrong = Harness::new();
        wrong.start();
        assert!(!wrong
            .tracker
            .request_stop(wrong.operation, stop_of(op(9)), 102, wrong.now));
        assert_eq!(wrong.tracker.failure(), Failure::Identity);
        // After a reconnect, the old session's input is inert.
        let mut reconnect = Harness::new();
        reconnect.start();
        reconnect.complete();
        reconnect.tracker.reset(2, 0x8765_4321);
        reconnect.ordinal += 1;
        let stale = Stamp {
            session: 1,
            ordinal: reconnect.ordinal,
        };
        reconnect.tracker.observe(stale, IDLE, reconnect.now);
        assert_eq!(reconnect.tracker.phase(), Phase::AwaitIdle);
        assert_eq!(reconnect.evidence(), None);
        // A session identity cannot be reused, and handle zero identifies no client.
        reconnect.tracker.reset(2, 0x8765_4321);
        assert_eq!(reconnect.tracker.phase(), Phase::Disconnected);
        reconnect.tracker.reset(3, 0);
        assert_eq!(reconnect.tracker.phase(), Phase::Disconnected);
    }

    #[test]
    fn failed_writes_deadlines_and_late_events() {
        // failedWritesDeadlinesAndLateEvents. The off-thread row has no counterpart: the tracker
        // is single-owner by `&mut self`.
        let mut h = Harness::new();
        h.start();
        h.stop_request();
        h.written(102, false, false);
        h.feed("R102|0|");
        h.feed(UNKEY);
        h.feed(READY_OWNED);
        h.feed(IDLE);
        assert_eq!(h.tracker.failure(), Failure::Write);
        assert_eq!(
            h.evidence(),
            None,
            "a partial write cannot be repaired by a later idle"
        );

        let mut extra = Harness::new();
        extra.start();
        extra.stop_reply();
        extra.written(103, true, true);
        assert_eq!(
            extra.tracker.failure(),
            Failure::UnsupportedActivity,
            "a second key"
        );

        let mut timeout = Harness::new();
        timeout.start();
        timeout.stop_reply();
        timeout.now += TRANSITION_TIMEOUT_MS;
        timeout.tracker.poll(timeout.now);
        timeout.feed(UNKEY);
        timeout.feed(READY_OWNED);
        timeout.feed(IDLE);
        assert_eq!(timeout.tracker.failure(), Failure::Timeout);
        assert_eq!(timeout.evidence(), None, "the exact deadline refuses");

        let mut confirmed = Harness::new();
        confirmed.start();
        confirmed.complete();
        assert_eq!(
            confirmed
                .tracker
                .evidence(confirmed.now + TRANSITION_TIMEOUT_MS),
            None,
            "a candidate expires even if no poll runs"
        );
        confirmed.feed(TRANSMITTING);
        assert_eq!(
            confirmed.evidence(),
            None,
            "a new TX observation revokes the candidate"
        );

        let mut backwards = Harness::new();
        backwards.start();
        backwards.tracker.poll(backwards.now - 1);
        assert_eq!(
            backwards.tracker.failure(),
            Failure::InvalidInput,
            "the clock went back"
        );
        let mut overflow = Harness::new();
        overflow.tracker.poll(u64::MAX);
        assert_eq!(overflow.tracker.failure(), Failure::InvalidInput);
        let mut bounded = Harness::new();
        bounded.feed(&"x".repeat(MAX_LINE + 1));
        assert_eq!(
            bounded.tracker.failure(),
            Failure::InvalidInput,
            "input is bounded"
        );
    }

    // ── The readback per kind (Nexus's own) ──────────────────────────────────────────────────
    //
    // The lines are the simulator's (`tempo-flexsim`'s bundled session), so these tests and the
    // simulator agree on what the radio says for each kind.

    /// The simulator's lines for `command`, for handle 0x12345678: each with the wait before it.
    fn radio_says(command: &str) -> Vec<(u64, String)> {
        use tempo_flexsim::session::Item;
        let session = tempo_flexsim::Session::v4_gui_client();
        let group = session.lookup(command).expect("the simulator answers it");
        let mut wait = 0;
        let mut lines = Vec::new();
        for item in &session.rule(group, 0).items {
            match item {
                Item::Wait(ms) => wait += ms,
                Item::Send(line) => {
                    lines.push((wait, line.replace("{h}", "12345678")));
                    wait = 0;
                }
                Item::Vita(_) => {}
            }
        }
        lines
    }

    impl Harness {
        /// Feed `lines` at their times.
        fn play(&mut self, lines: &[(u64, String)]) {
            for (wait, line) in lines {
                self.now += wait;
                self.feed(line);
            }
        }

        /// A tune started and keyed, its stop sent and answered, nothing of the release yet.
        fn tune_stopped(&mut self) {
            assert!(self.tracker.begin_as(self.operation, TUNE, 101, self.now));
            self.written(101, true, true);
            self.feed("R101|0|");
            self.play(&radio_says("transmit tune 1"));
            assert_eq!(self.tracker.phase(), Phase::Transmitting, "the tune keyed");
            self.stop_reply();
        }

        /// A radio-ended start armed, written and answered: its window is open.
        fn window_opened(&mut self, profile: Profile, window: Window) {
            assert!(self.tracker.begin_window(
                self.operation,
                self.stop,
                profile,
                101,
                window,
                self.now
            ));
            self.written(101, true, true);
            self.feed("R101|0|");
            assert_eq!(self.tracker.phase(), Phase::Window);
        }
    }

    #[test]
    fn a_tune_ends_only_when_its_status_and_the_interlock_agree() {
        // The simulator's release: `tune=0`, then the interlock's UNKEY_REQUESTED, READY naming
        // us and, 430 ms on, READY idle.
        let release = radio_says("transmit tune 0");
        assert_eq!(release[0].1, "S0|transmit tune=0");

        // The transmit status first: alone it is not the end.
        let mut h = Harness::new();
        h.tune_stopped();
        h.play(&release[..1]);
        assert_eq!(h.tracker.phase(), Phase::AwaitUnkey, "tune=0 alone: keyed");
        assert_eq!(h.evidence(), None);
        h.play(&release[1..]);
        assert_eq!(h.tracker.phase(), Phase::Confirmed);
        assert_eq!(h.evidence(), Some(h.stop));

        // The interlock first: alone it is not the end either.
        let mut h = Harness::new();
        h.tune_stopped();
        h.play(&release[1..]);
        assert_eq!(h.tracker.phase(), Phase::AwaitTuneOff, "idle alone: keyed");
        assert_eq!(h.evidence(), None);
        h.play(&release[..1]);
        assert_eq!(h.tracker.phase(), Phase::Confirmed);
        assert_eq!(h.evidence(), Some(h.stop));
    }

    #[test]
    fn a_tune_keys_under_its_own_source_and_its_status_must_agree() {
        // The tune's source is not xmit's: an attempt seeing SW, or a hardware source, fails.
        for source in ["SW", "MIC"] {
            let mut h = Harness::new();
            assert!(h.tracker.begin_as(h.operation, TUNE, 101, h.now));
            h.written(101, true, true);
            h.feed("R101|0|");
            h.feed(&REQUESTED.replace("source=SW", &format!("source={source}")));
            assert_eq!(h.tracker.failure(), Failure::Ownership, "{source}");
        }
        // No `tune=1` while keyed: the release alone proves nothing.
        let mut h = Harness::new();
        assert!(h.tracker.begin_as(h.operation, TUNE, 101, h.now));
        h.written(101, true, true);
        h.feed("R101|0|");
        let keyed: Vec<_> = radio_says("transmit tune 1")
            .into_iter()
            .filter(|(_, l)| l.contains("|interlock "))
            .collect();
        h.play(&keyed);
        h.stop_reply();
        h.play(&radio_says("transmit tune 0"));
        assert_eq!(h.tracker.failure(), Failure::State, "tune=0 with no tune=1");
        assert_eq!(h.evidence(), None);
        // The carrier back after the stop's reply, a malformed or doubled value, a foreign
        // envelope: each fails the attempt.
        for (line, why) in [
            ("S0|transmit tune=1", Failure::State),
            ("S0|transmit tune=2", Failure::InvalidInput),
            ("S0|transmit tune=0 tune=0", Failure::InvalidInput),
            ("S87654321|transmit tune=0", Failure::Ownership),
        ] {
            let mut h = Harness::new();
            h.tune_stopped();
            h.feed(line);
            assert_eq!(h.tracker.failure(), why, "{line}");
        }
        // A transmit line without `tune` says nothing about the carrier.
        let mut h = Harness::new();
        h.tune_stopped();
        h.feed("S0|transmit rfpower=50");
        h.play(&radio_says("transmit tune 0"));
        assert_eq!(h.evidence(), Some(h.stop));
    }

    #[test]
    fn a_cwx_operation_ends_only_after_the_break_in_window() {
        // A 300 ms word, break-in delay 300 ms: the window ends no sooner than the idle that
        // follows plus 300 + 300 ms.
        let window = Window::cwx(300, 300);
        let mut h = Harness::new();
        h.window_opened(CWX, window);
        h.play(&radio_says("cwx send \"CQ\" 1"));
        let idle = h.now;
        assert_eq!(h.evidence(), None, "the first idle is not the end");
        h.now = idle + 600 - 1;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), None, "inside the hold");
        h.now = idle + 600;
        h.tracker.poll(h.now);
        assert_eq!(h.tracker.phase(), Phase::Confirmed);
        assert_eq!(h.evidence(), Some(h.stop));
        assert!(h.tracker.consume(h.stop, h.now));
        assert_eq!(h.tracker.phase(), Phase::Idle);

        // The hold starts after the word's expected end, however early the radio is idle.
        let mut h = Harness::new();
        h.window_opened(CWX, Window::cwx(5_000, 300));
        let opened = h.now;
        h.now = opened + 5_000 + 600 - 1;
        h.tracker.poll(h.now);
        assert_eq!(
            h.evidence(),
            None,
            "idle all along, but the word is not over"
        );
        h.now += 1;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), Some(h.stop));

        // A key inside the hold starts it again (inside the word's deadline: a radio still keyed
        // past the word's end + break-in delay + 1 s fails the window, below).
        let mut h = Harness::new();
        h.window_opened(CWX, window);
        h.play(&radio_says("cwx send \"CQ\" 1"));
        h.now += 100;
        h.play(&radio_says("cwx send \"CQ\" 1"));
        let idle = h.now;
        h.now = idle + 599;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), None, "the hold restarted");
        h.now = idle + 600;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), Some(h.stop));
    }

    #[test]
    fn a_later_word_joins_the_open_window() {
        let mut h = Harness::new();
        h.window_opened(CWX, Window::cwx(300, 300));
        h.play(&radio_says("cwx send \"CQ\" 1"));
        assert!(h
            .tracker
            .append(h.operation, 102, Window::cwx(800, 300), h.now));
        h.written(102, true, true);
        // Settled, but the word's reply is outstanding: not the end.
        h.now += 2_000;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), None, "a word awaits its reply");
        h.feed("R102|0|");
        let replied = h.now;
        h.now = replied + 800 + 600 - 1;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), None, "the second word's end and hold");
        h.now += 1;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), Some(h.stop));
        // An append outside an open window, or for another operation, is not taken.
        let mut h = Harness::new();
        assert!(!h
            .tracker
            .append(h.operation, 102, Window::cwx(300, 300), h.now));
        h.window_opened(CWX, Window::cwx(300, 300));
        assert!(!h.tracker.append(op(9), 102, Window::cwx(300, 300), h.now));
        assert!(!h
            .tracker
            .append(h.operation, 101, Window::cwx(300, 300), h.now));
        // A word's failed reply, or a reply before its write, fails the window.
        for (written, reply, why) in [
            (true, "R102|5000007B|", Failure::Reply),
            (false, "R102|0|", Failure::Ordering),
        ] {
            let mut h = Harness::new();
            h.window_opened(CWX, Window::cwx(300, 300));
            assert!(h
                .tracker
                .append(h.operation, 102, Window::cwx(300, 300), h.now));
            if written {
                h.written(102, true, true);
            }
            h.feed(reply);
            assert_eq!(h.tracker.failure(), why, "{reply}");
        }
    }

    #[test]
    fn a_window_fails_on_a_foreign_or_physical_source_and_at_its_deadline() {
        for line in [
            "S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= source=MIC tx_allowed=1 amplifier=",
            "S0|interlock tx_client_handle=0x87654321 state=TRANSMITTING reason= source=SW tx_allowed=1 amplifier=",
            "S0|interlock tx_client_handle=0x12345678 state=TRANSMITTING reason= source=TUNE tx_allowed=1 amplifier=",
            "S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= source=SW tx_allowed=1 amplifier=",
            // A reason other than an amplifier's (an amplifier's alone is ours: below).
            "S0|interlock tx_client_handle=0x12345678 state=TRANSMITTING reason=PA_RANGE source=SW tx_allowed=1 amplifier=",
            "S0|interlock state=TRANSMITTING",
        ] {
            let mut h = Harness::new();
            h.window_opened(CWX, Window::cwx(300, 300));
            h.feed(line);
            assert_eq!(h.tracker.phase(), Phase::Failed, "{line}");
            h.now += 10_000;
            h.tracker.poll(h.now);
            assert_eq!(h.evidence(), None, "{line}");
        }
        // Still keyed at the word's end + break-in delay + 1 s: the deadline.
        let mut h = Harness::new();
        h.window_opened(CWX, Window::cwx(300, 300));
        let opened = h.now;
        h.feed(TRANSMITTING);
        h.now = opened + 300 + 300 + 1_000 - 1;
        h.tracker.poll(h.now);
        assert_eq!(h.tracker.phase(), Phase::Window);
        h.now += 1;
        h.tracker.poll(h.now);
        assert_eq!(h.tracker.failure(), Failure::Timeout);
    }

    /// `line` with `reason` in place of its empty reason.
    fn with_reason(line: &str, reason: &str) -> String {
        line.replace(" reason= ", &format!(" reason={reason} "))
    }

    /// ⭐ A keying report whose only reason names an amplifier (`reason=AMP:PG-XL`, a PGXL in line)
    /// is ours when its source and its place in the order are: an over confirms through it, and a
    /// CWX window keeps going. Any other reason, two reasons or no name fails, and so does an
    /// amplifier's reason on a release, on the idle that would end it, out of order or under
    /// another source.
    #[test]
    fn an_amplifiers_reason_on_a_keying_report_is_ours() {
        assert!(amplifier_only("AMP:PG-XL"));
        let keyed = |h: &mut Harness, reason: &str| {
            assert!(h.tracker.begin(h.operation, 101, h.now));
            h.written(101, true, true);
            h.feed("R101|0|");
            h.feed(&with_reason(REQUESTED, reason));
        };
        // The over: both keying reports carry it, and the captured release confirms.
        let mut h = Harness::new();
        keyed(&mut h, "AMP:PG-XL");
        h.feed(&with_reason(TRANSMITTING, "AMP:PG-XL"));
        assert_eq!(h.tracker.phase(), Phase::Transmitting);
        h.complete();
        assert_eq!(h.evidence(), Some(h.stop));
        // Any other reason on the same report.
        for reason in [
            "PA_RANGE",
            "ANT:ANT2",
            "AMP:PG-XL,ANT:ANT2",
            "AMP:",
            "amp:PG-XL",
        ] {
            let mut h = Harness::new();
            keyed(&mut h, reason);
            assert_eq!(
                (h.tracker.phase(), h.tracker.failure()),
                (Phase::Failed, Failure::State),
                "{reason}"
            );
        }
        // On a release or on the idle that would end it.
        let release = [UNKEY, READY_OWNED, IDLE];
        for at in 0..release.len() {
            let mut h = Harness::new();
            h.start();
            h.stop_reply();
            for (i, line) in release.iter().enumerate() {
                if i == at {
                    h.feed(&with_reason(line, "AMP:PG-XL"));
                } else {
                    h.feed(line);
                }
            }
            assert_eq!(h.tracker.phase(), Phase::Failed, "{}", release[at]);
            assert_eq!(h.evidence(), None);
        }
        // Out of order, or under another source.
        for (line, why) in [
            (with_reason(TRANSMITTING, "AMP:PG-XL"), Failure::State),
            (
                with_reason(&REQUESTED.replace("source=SW", "source=MIC"), "AMP:PG-XL"),
                Failure::Ownership,
            ),
        ] {
            let mut h = Harness::new();
            assert!(h.tracker.begin(h.operation, 101, h.now));
            h.written(101, true, true);
            h.feed("R101|0|");
            h.feed(&line);
            assert_eq!(h.tracker.failure(), why, "{line}");
        }
        // In a CWX window: the keying report with an amplifier's reason is ours, and the radio's
        // release ends the word as it would with no amplifier.
        let mut h = Harness::new();
        h.window_opened(CWX, Window::cwx(300, 300));
        let word: Vec<(u64, String)> = radio_says("cwx send \"CQ\" 1")
            .into_iter()
            .map(|(wait, line)| {
                let keying =
                    line.contains("state=PTT_REQUESTED") || line.contains("state=TRANSMITTING");
                (
                    wait,
                    if keying {
                        with_reason(&line, "AMP:PG-XL")
                    } else {
                        line
                    },
                )
            })
            .collect();
        h.play(&word);
        assert_eq!(h.tracker.phase(), Phase::Window);
        h.now += 600;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), Some(h.stop));
    }

    #[test]
    fn an_atu_cycle_ends_on_its_result_and_the_idle_interlock() {
        let cycle = radio_says("atu start");
        let result = cycle
            .iter()
            .position(|(_, l)| l == "S0|atu status=TUNE_SUCCESSFUL")
            .expect("the profile's result");
        let mut h = Harness::new();
        h.window_opened(ATU, Window::atu());
        h.play(&cycle[..=result]);
        assert_eq!(h.evidence(), None, "the result, still keyed");
        h.play(&cycle[result + 1..]);
        assert_eq!(h.tracker.phase(), Phase::Confirmed);
        assert_eq!(h.evidence(), Some(h.stop));

        // The idle interlock with no result is not the end; past the ceiling it fails.
        let mut h = Harness::new();
        h.window_opened(ATU, Window::atu());
        let opened = h.now;
        h.feed("S0|atu status=TUNE_IN_PROGRESS");
        h.now = opened + ATU_CEILING_MS - 1;
        h.tracker.poll(h.now);
        assert_eq!(h.evidence(), None);
        assert_eq!(h.tracker.phase(), Phase::Window);
        h.now += 1;
        h.tracker.poll(h.now);
        assert_eq!(h.tracker.failure(), Failure::Timeout);

        // A result reported before the start's reply cannot be this cycle's.
        let mut h = Harness::new();
        assert!(h
            .tracker
            .begin_window(h.operation, h.stop, ATU, 101, Window::atu(), h.now));
        h.written(101, true, true);
        h.feed("S0|atu status=TUNE_SUCCESSFUL");
        assert_eq!(h.tracker.failure(), Failure::State);
        // A new cycle after the proof revokes it.
        let mut h = Harness::new();
        h.window_opened(ATU, Window::atu());
        h.play(&cycle);
        assert!(h.evidence().is_some());
        h.feed("S0|atu status=TUNE_IN_PROGRESS");
        assert_eq!(h.evidence(), None);
    }

    #[test]
    fn each_kind_arms_only_through_its_own_door() {
        let mut h = Harness::new();
        assert!(!h.tracker.begin_as(h.operation, CWX, 101, h.now));
        assert!(!h.tracker.begin_as(h.operation, ATU, 101, h.now));
        assert!(!h
            .tracker
            .begin_window(h.operation, h.stop, KEY, 101, Window::atu(), h.now));
        assert!(!h
            .tracker
            .begin_window(h.operation, h.stop, TUNE, 101, Window::atu(), h.now));
        assert!(
            !h.tracker
                .begin_window(h.operation, stop_of(op(9)), CWX, 101, Window::atu(), h.now),
            "the end request is the operation's"
        );
        assert_eq!(h.tracker.phase(), Phase::Idle, "nothing armed");
        // The profiles' facts, as the simulator and the bench checklist name them.
        assert_eq!(
            (KEY.source, TUNE.source, CWX.source, ATU.source),
            ("SW", "TUNE", "SW", "TUNE")
        );
    }

    #[test]
    fn the_timeout_is_a_parameter() {
        let mut t = StopTracker::new(300);
        t.reset(1, 0x1234_5678);
        t.observe(
            Stamp {
                session: 1,
                ordinal: 1,
            },
            IDLE,
            0,
        );
        assert!(t.begin(op(1), 1, 0));
        t.poll(299);
        assert_eq!(t.phase(), Phase::AwaitKeyWrite);
        t.poll(300);
        assert_eq!(t.failure(), Failure::Timeout);
    }
}
