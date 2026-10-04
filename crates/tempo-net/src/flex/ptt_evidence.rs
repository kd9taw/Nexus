//! The unkey readback: the interlock state machine that turns "we sent `xmit 0`" into "the radio
//! reports READY and no transmitting client".
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

use std::num::NonZeroU64;

/// How long each transition may take before the attempt fails (upstream `kTransitionTimeoutMs`).
pub const TRANSITION_TIMEOUT_MS: u64 = 5000;

/// The longest raw line the tracker accepts.
pub const MAX_LINE: usize = 4096;

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
}

impl Default for StopTracker {
    fn default() -> Self {
        StopTracker::new(TRANSITION_TIMEOUT_MS)
    }
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
        true
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
        self.key_sequence = key_sequence;
        self.stop_sequence = 0;
        self.deadline_ms = now_ms + self.timeout_ms;
        self.phase = Phase::AwaitKeyWrite;
        true
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

    /// A write that ended at the transport: the key (`keying`) or the unkey. Only a full write of
    /// the exact command counts; sequence zero is reserved. Any other transmit-capable write
    /// during an attempt fails it.
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
        } else {
            self.fail(Failure::UnsupportedActivity);
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
        if sequence != self.key_sequence && sequence != self.stop_sequence {
            return;
        }
        let result = rest.split_once('|').and_then(|(code, _)| number(code, 16));
        if result != Some(0) {
            self.fail(Failure::Reply);
        } else if sequence == self.key_sequence && self.phase == Phase::AwaitKeyReply {
            self.phase = Phase::AwaitPttRequested;
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
        if (handle != 0 && handle != self.client_handle) || (!source.is_empty() && source != "SW") {
            self.fail(Failure::Ownership);
            return;
        }
        if allowed != "1" || !reason.is_empty() {
            self.fail(Failure::State);
            return;
        }
        let ours = handle == self.client_handle;
        match self.phase {
            Phase::AwaitPttRequested if state == "PTT_REQUESTED" && source == "SW" && ours => {
                self.phase = Phase::AwaitTransmitting;
            }
            Phase::AwaitTransmitting if state == "TRANSMITTING" && source == "SW" && ours => {
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
            Phase::Transmitting if state == "TRANSMITTING" && source == "SW" && ours => {}
            Phase::AwaitUnkey if state == "UNKEY_REQUESTED" && source.is_empty() && ours => {
                self.phase = Phase::AwaitReady;
            }
            Phase::AwaitReady if state == "READY" && source.is_empty() && ours => {
                self.phase = Phase::AwaitOwnerClear;
            }
            Phase::AwaitOwnerClear | Phase::Confirmed if idle && self.stop_matches() => {
                self.phase = Phase::Confirmed;
            }
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
            let Some(body) = body.strip_prefix("interlock ") else {
                return;
            };
            // Interlock band and timing configuration is not a state sample.
            if body.starts_with("band ") {
                return;
            }
            match number(envelope, 16) {
                Some(h) if h == 0 || h == self.client_handle => {}
                _ => {
                    self.fail(Failure::Ownership);
                    return;
                }
            }
            self.interlock(body);
        }
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
