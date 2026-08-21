//! Keyboard modes — the shared transmit-safety machinery.
//!
//! A keyboard mode is one the operator TYPES into while the transmitter is up:
//! RTTY today, PSK31 and the others to come. They differ in everything a modem
//! cares about (Baudot over FSK vs varicode over BPSK) and in nothing that
//! transmit safety cares about, so the safety machinery is written ONCE here and
//! instantiated per mode through [`KeyboardMode`], rather than re-derived —
//! which is the only way a second mode can be added without a second chance to
//! get a stuck carrier wrong.
//!
//! ⚠️ WHY THIS IS ITS OWN MODULE, and the thing to hold on to when editing it:
//! a latched keyboard mode is the only transmission in this app that keys with
//! NO PRECOMPUTED END. Every other over — an FT8 slot, a CW message, an SSTV
//! image, a one-shot RTTY over — computes its duration before the first bit goes
//! out, so the deadline that unkeys the rig exists even if everything above it
//! dies. A latch has no such deadline; it has a PREDICATE, re-evaluated on every
//! radio-loop tick, and three bounds that outlive a wedge:
//!
//! 1. [`KeyboardLatch::tick`] — the per-tick gate re-check. A latch outlives the
//!    moment it was granted and the gates do not, so every gate checked before
//!    the operator was allowed to key is checked again before EVERY chunk. Each
//!    one DROPS THE LATCH rather than holding the feed: holding is a fine answer
//!    for a queue and no answer at all for a transmitter that is already keyed.
//! 2. [`KeyboardMode::max_latch_ms`] — a hard per-over ceiling that no amount of
//!    typing can extend, for the case the ordinary watchdog cannot see (typing
//!    resets that watchdog, and a stuck key types forever).
//! 3. [`KeyboardMode::stream_ahead_chars`] — the radio loop renders only this far
//!    ahead of real time, so a loop that stops ticking expires into an unkey
//!    instead of holding PTT.
//!
//! The predicate deliberately takes its gate values as DATA ([`KbGates`]) and
//! returns its decisions as DATA ([`KbTickResult`]) instead of reaching into the
//! engine: that is what lets a mode with no engine at all — a test mode, the
//! next real mode — instantiate the identical machinery and prove it holds.

use std::collections::VecDeque;

/// The character alphabet a keyboard mode can put on the air.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    /// ITA2 / Baudot: uppercase-only, two shift planes (LTRS/FIGS), no lower
    /// case to lose. Anything with no ITA2 mapping simply does not exist on the
    /// air, so it is dropped rather than mangled.
    Ita2,
    /// Full-ASCII varicode (PSK31): every printable ASCII character plus CR/LF,
    /// case preserved — the whole point over Baudot. Anything outside ASCII
    /// (and any other control character) is dropped rather than mangled, the
    /// same rule as ITA2's.
    Varicode,
}

/// Everything the shared transmit-safety machinery needs to know about ONE
/// keyboard mode. Const-instantiated per mode (see [`RTTY`]) and passed by
/// value; a mode is a small pile of numbers and an alphabet, not a trait object,
/// because every difference between two keyboard modes so far is a VALUE.
///
/// Adding a mode is adding one of these plus its modem. Nothing in
/// [`KeyboardLatch`] may branch on which mode it holds — if a new mode needs
/// different behavior rather than a different number, the difference belongs in
/// this struct as a new field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyboardMode {
    /// Operator-facing name, used in the messages the machinery generates.
    pub name: &'static str,
    /// The alphabet [`kb_filter`] reduces operator text to.
    pub charset: Charset,
    /// HARD CEILING on one continuous latched over (ms), whatever the operator
    /// types — the bound that exists because typing resets the ordinary
    /// watchdog. No setting and no input may raise it.
    pub max_latch_ms: u64,
    /// Cap on the un-keyed type-ahead buffer (characters). A human types far
    /// slower than the mode drains, so this only bites a runaway producer — and
    /// it bounds how long a latch-off takes to drain on the air.
    pub type_buf_cap: usize,
    /// Bit times in ONE character frame — the mode's frame overhead made
    /// explicit. RTTY/ITA2 is 7.5: 1 start + 5 data + 1.5 stop.
    pub frame_bits: f64,
    /// How far ahead of real time the latched stream may render audio, in
    /// CHARACTER TIMES. See [`KeyboardMode::stream_ahead_ms`] for the bound in
    /// the milliseconds the radio loop actually reasons in.
    pub stream_ahead_chars: f64,
    /// Most characters the latched stream may render in ONE tick, whatever the
    /// look-ahead deficit says — the bound on what a single tick can commit
    /// after the loop has been stalled.
    pub stream_max_chunk: usize,
}

impl KeyboardMode {
    /// One character's air time (ms) at `baud`.
    pub fn char_ms(&self, baud: f64) -> f64 {
        self.frame_bits * (1000.0 / baud)
    }

    /// THE LOOK-AHEAD SAFETY BOUND, in milliseconds at `baud` — how far past now
    /// the unkey deadline may be pushed by one feed.
    ///
    /// ⚠️ This is a safety bound, not a buffer-tuning knob. A one-shot over keys
    /// against a deadline computed before a single bit goes out, so the rig
    /// unkeys even if the loop then dies. A LATCHED over has no such precomputed
    /// end — its deadline is one the loop must keep pushing forward — so the
    /// only thing standing between a wedged loop and a stuck carrier is how far
    /// forward each push may reach. Raising this raises the stuck-carrier window
    /// by exactly the same amount.
    ///
    /// The floor is the loop's own 20 ms tick plus whatever a shared CAT read
    /// can stall it by, plus enough margin that the output ring never underruns
    /// to silence (which under a held PTT reads on the air as a dropout).
    pub fn stream_ahead_ms(&self, baud: f64) -> f64 {
        self.stream_ahead_chars * self.char_ms(baud)
    }
}

/// RTTY — the first keyboard mode, and the one every bound above was measured
/// on. Two characters ≈ 330 ms at 45.45 baud; 10 minutes is far past any human
/// keyboard-RTTY over and well past the 6-minute default watchdog, so the
/// ceiling never ends a legitimate over.
pub const RTTY: KeyboardMode = KeyboardMode {
    name: "RTTY",
    charset: Charset::Ita2,
    max_latch_ms: 10 * 60 * 1000,
    type_buf_cap: 1000,
    frame_bits: 7.5,
    stream_ahead_chars: 2.0,
    stream_max_chunk: 4,
};

/// PSK31 — the second keyboard mode, and the first proof Phase 0's machinery
/// really was mode-generic. Varicode characters are VARIABLE length (1–10 code
/// bits + the 2-bit separator, frequency-ordered so common text runs short);
/// `frame_bits` is the budgeting average the look-ahead reasons in — 10 bits
/// covers ordinary mixed-case text with margin (`e` is 4 bits on the wire, the
/// worst character 12) — while the radio loop paces `busy_until` on the REAL
/// rendered bit count, so the average only shapes the budget, never a deadline.
/// One char ≈ 320 ms at 31.25 Bd; the ceiling and cap match RTTY's rationale
/// (10 min is past any human over; 500 chars ≈ 2.7 min average air time, so a
/// single over stays well under the 6-minute default watchdog even worst-case).
/// `stream_max_chunk` is 2 — chars are ~2× RTTY's air time, so a smaller chunk
/// holds the stuck-carrier window near RTTY's while still draining 100 chars/s
/// against a 4 char/s modem.
pub const PSK31: KeyboardMode = KeyboardMode {
    name: "PSK31",
    charset: Charset::Varicode,
    max_latch_ms: 10 * 60 * 1000,
    type_buf_cap: 500,
    frame_bits: 10.0,
    stream_ahead_chars: 2.0,
    stream_max_chunk: 2,
};

/// Reduce operator text to what `mode` can actually put on the air — the ONE
/// filter every keyboard-mode TX path runs, so what is queued (or typed into a
/// latched stream) is exactly what keys, and no two paths can disagree about
/// which characters exist.
pub fn kb_filter(mode: KeyboardMode, text: &str) -> String {
    match mode.charset {
        Charset::Ita2 => text
            .chars()
            .map(|c| c.to_ascii_uppercase())
            .filter(|&c| tempo_core::rtty::encodable(c))
            .collect(),
        // Full-ASCII varicode: case survives (the point over Baudot). CR/LF
        // pass (both are in the table and Enter on a latched stream is a real
        // new line on the air); every other control character and everything
        // past ASCII is dropped, never mangled.
        Charset::Varicode => text
            .chars()
            .filter(|&c| c.is_ascii() && (c == '\r' || c == '\n' || !c.is_ascii_control()))
            .collect(),
    }
}

/// What the radio loop should key this tick on behalf of a continuous-TX latch.
///
/// Four explicit answers rather than an empty-string sentinel: "nothing typed"
/// and "not transmitting" are opposite instructions to a keyed transmitter, and a
/// consumer that had to infer the difference from an empty buffer would sooner or
/// later infer it wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KbTick {
    /// Not streaming. The loop must not key on the latch's behalf — and if it was,
    /// the latch has just been dropped and the abort armed, so it unkeys.
    Idle,
    /// Streaming and every gate is up, but the loop asked for no characters this
    /// tick because its audio look-ahead is already full. Stay keyed, key nothing.
    /// This exists so the loop can run the gate re-check on EVERY tick without
    /// having to feed on every tick — the predicate is the safety, the feeding is
    /// only the feature.
    Ahead,
    /// Key these typed characters (never empty).
    Text(String),
    /// Latched with nothing typed: key the mode's IDLE (RTTY's LTRS diddle) to
    /// hold the carrier and the far end's sync. Never silence under a held PTT.
    Diddle,
}

/// Why [`KeyboardLatch::tick`] decided the over has to end. The caller turns this
/// into its mode's kill path — the abort plumbing is the caller's because the
/// flags that unkey a rig are engine state, but the DECISION is made here, once,
/// for every mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KbDrop {
    /// A transmit gate that was up when the operator keyed has gone down.
    GateDown,
    /// [`KeyboardMode::max_latch_ms`] — the hard per-over ceiling.
    Ceiling,
    /// The ordinary wall-clock TX watchdog. Unlike the other two this also
    /// DISARMS TX, so it stays stopped rather than being re-keyable.
    Watchdog,
}

/// The gate values the per-tick predicate re-checks, sampled by the caller for
/// this tick.
///
/// Every field is a value rather than a callback on purpose: the predicate must
/// be runnable — and therefore testable — with no engine, no radio and no clock
/// behind it. The caller is responsible for these meaning the same thing they
/// meant at the moment the operator was allowed to key; that correspondence is
/// what makes the re-check a re-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KbGates {
    /// Normal transmit is armed (the TX-enable latch, the watchdog's disarm).
    pub tx_enabled: bool,
    /// The current dial + mode is inside the operator's license privileges.
    pub tx_allowed: bool,
    /// A tune carrier is up — someone else owns the transmitter.
    pub tuning: bool,
    /// The app is still in this mode's section/cockpit.
    pub in_section: bool,
    /// Unix ms, for the hard ceiling.
    pub now_ms: u64,
    /// Unix secs, for the wall-clock watchdog.
    pub now_secs: u64,
    /// The watchdog ceiling in seconds; 0 disables it.
    pub watchdog_limit_secs: u64,
    /// When the current unattended-transmit run began, or `None` if the clock
    /// has not been started — see [`KbTickResult::watchdog_start`].
    pub watchdog_start_secs: Option<u64>,
}

/// What one tick of the predicate decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbTickResult {
    /// What to key this tick. Always [`KbTick::Idle`] when `drop` is set.
    pub tick: KbTick,
    /// Set when a bound went down. The caller MUST run its mode's kill path
    /// (the one that clears the latch and arms the abort) before honouring
    /// `tick` — the predicate deliberately does not clear the state itself, so
    /// that every drop in the app goes through the one kill path and none of
    /// them can forget the abort.
    pub drop: Option<KbDrop>,
    /// The watchdog clock this tick STARTED, if it reached that check — the
    /// `get_or_insert` half of the watchdog. `None` means the tick returned
    /// before the watchdog was consulted and the caller must leave its stored
    /// clock alone; starting it earlier would date an unattended run from a tick
    /// that never transmitted.
    pub watchdog_start: Option<u64>,
}

impl KbTickResult {
    fn keying(tick: KbTick) -> Self {
        Self {
            tick,
            drop: None,
            watchdog_start: None,
        }
    }

    fn dropped(why: KbDrop) -> Self {
        Self {
            tick: KbTick::Idle,
            drop: Some(why),
            watchdog_start: None,
        }
    }
}

/// The latched stream is running: the operator is latched, or the latch is down
/// but what they typed is still going out.
///
/// A free function because the engine needs this answer from an `&self` it
/// cannot borrow mutably (it is one of the terms in the TX-ownership arbiter),
/// and there must not be a second definition of "streaming" anywhere.
pub fn streaming(latched: bool, type_buf: &VecDeque<char>) -> bool {
    latched || !type_buf.is_empty()
}

/// One keyboard mode's continuous-TX latch — a BORROWED VIEW of the three pieces
/// of state a latch is (the operator's intent, the un-keyed type-ahead, the
/// ceiling's anchor) plus the mode they are governed by.
///
/// It borrows rather than owns so that a host with many modes keeps its state in
/// one flat place and instantiates the machinery over whichever mode's fields it
/// means — and so that adding the second mode adds no way for the first mode's
/// state and the machinery's copy of it to disagree.
pub struct KeyboardLatch<'a> {
    mode: KeyboardMode,
    latched: &'a mut bool,
    type_buf: &'a mut VecDeque<char>,
    start_ms: &'a mut Option<u64>,
}

impl<'a> KeyboardLatch<'a> {
    pub fn new(
        mode: KeyboardMode,
        latched: &'a mut bool,
        type_buf: &'a mut VecDeque<char>,
        start_ms: &'a mut Option<u64>,
    ) -> Self {
        Self {
            mode,
            latched,
            type_buf,
            start_ms,
        }
    }

    /// See [`streaming`].
    pub fn streaming(&self) -> bool {
        streaming(*self.latched, self.type_buf)
    }

    /// Raise the latch. The caller runs its up-front TX gate FIRST — this only
    /// records intent and starts the ceiling's clock.
    ///
    /// Idempotent by contract, and that is load-bearing: re-latching must not
    /// restart the ceiling, or clicking TX again would buy another full over.
    /// Returns whether this call actually raised it (false = already up).
    pub fn latch_on(&mut self, now_ms: u64) -> bool {
        if *self.latched {
            return false;
        }
        *self.latched = true;
        *self.start_ms = Some(now_ms);
        true
    }

    /// Lower the operator's INTENT, leaving what they already typed to drain on
    /// the air (the MMTTY semantic — this is a sender coming down, not a stop).
    ///
    /// `start_ms` is deliberately LEFT SET: the drain is still a keyed
    /// transmission, so the hard ceiling has to bound the whole keyed period and
    /// not just the part with the intent up. [`KeyboardLatch::drop_latch`] is
    /// what clears it, together with everything else.
    pub fn latch_off(&mut self) {
        *self.latched = false;
    }

    /// Drop the latch NOW: clear the operator's intent AND the un-keyed
    /// type-ahead, and forget the ceiling's anchor. This is what every kill path
    /// calls.
    ///
    /// Returns whether the stream was actually running, which is the caller's
    /// cue to arm its transmit abort — conditional on purpose, because arming a
    /// one-shot abort when nothing was streaming would cut an unrelated over
    /// riding the same PTT.
    pub fn drop_latch(&mut self) -> bool {
        let was_streaming = self.streaming();
        *self.latched = false;
        *self.start_ms = None;
        self.type_buf.clear();
        was_streaming
    }

    /// Push already-filtered characters into the live transmission, up to the
    /// mode's buffer cap. Returns how many were accepted (0 = the cap is full,
    /// which the caller must not treat as an error — there is no un-send).
    pub fn type_chars(&mut self, filtered: &str) -> usize {
        let mut typed = 0usize;
        for c in filtered.chars() {
            if self.type_buf.len() >= self.mode.type_buf_cap {
                break;
            }
            self.type_buf.push_back(c);
            typed += 1;
        }
        typed
    }

    /// ⚠️ THE PER-TICK PREDICATE — the whole of transmit safety for every
    /// keyboard mode, in one place.
    ///
    /// Every gate the caller checked before the operator was allowed to key is
    /// re-checked here before every chunk, because a latch outlives the moment it
    /// was granted and the gates do not. Each failure DROPS THE LATCH (not merely
    /// the feed) and unkeys — the queued-over path's equivalent gate merely HOLDS
    /// its queue, and holding is not an option for a transmitter already keyed.
    ///
    /// GATE-FOR-GATE, in evaluation order, and the order is part of the contract
    /// (the watchdog's clock must not be started by a tick that a gate or the
    /// ceiling already ended):
    ///
    /// | # | Condition | On failure |
    /// |---|---|---|
    /// | G0 | the stream is running at all | `Idle` — no drop, no abort |
    /// | G1 | `tx_enabled` | drop, `GateDown` |
    /// | G2 | `tx_allowed` (license privileges) | drop, `GateDown` |
    /// | G3 | not `tuning` | drop, `GateDown` |
    /// | G4 | `in_section` (still this mode's cockpit) | drop, `GateDown` |
    /// | C1 | under [`KeyboardMode::max_latch_ms`] | drop, `Ceiling` |
    /// | C2 | under the wall-clock watchdog | drop, `Watchdog` (also disarms TX) |
    ///
    /// Past C2 it is only about WHAT to key: a zero budget means the loop is
    /// already fed far enough ahead (`Ahead`), an empty buffer means idle fill
    /// (`Diddle`, never silence under a held PTT), otherwise up to `max_chars`
    /// typed characters.
    pub fn tick(&mut self, gates: KbGates, max_chars: usize) -> KbTickResult {
        // G0 — nothing is running, so there is nothing to gate and nothing to cut.
        if !self.streaming() {
            return KbTickResult::keying(KbTick::Idle);
        }
        // G1..G4 — every gate the up-front check made, made again.
        if !gates.tx_enabled || !gates.tx_allowed || gates.tuning || !gates.in_section {
            return KbTickResult::dropped(KbDrop::GateDown);
        }
        // C1 — the hard per-over cap no typing can extend.
        if let Some(start) = *self.start_ms {
            if gates.now_ms.saturating_sub(start) >= self.mode.max_latch_ms {
                return KbTickResult::dropped(KbDrop::Ceiling);
            }
        }
        // C2 — the ordinary wall-clock TX watchdog. Typing restarts its clock
        // (the caller does that on a keystroke); walking away does not.
        let mut watchdog_start = None;
        if gates.watchdog_limit_secs > 0 {
            let start = gates.watchdog_start_secs.unwrap_or(gates.now_secs);
            watchdog_start = Some(start);
            if gates.now_secs.saturating_sub(start) >= gates.watchdog_limit_secs {
                let mut res = KbTickResult::dropped(KbDrop::Watchdog);
                res.watchdog_start = watchdog_start;
                return res;
            }
        }
        // Every gate above has been re-checked; from here it is only about what
        // to key. A zero budget means the loop is already fed far enough ahead.
        let tick = if max_chars == 0 {
            KbTick::Ahead
        } else if self.type_buf.is_empty() {
            KbTick::Diddle
        } else {
            let n = max_chars.min(self.type_buf.len());
            KbTick::Text(self.type_buf.drain(..n).collect())
        };
        KbTickResult {
            tick,
            drop: None,
            watchdog_start,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SECOND keyboard mode, existing only here. The point of Phase 0 is that
    /// the machinery is genuinely mode-generic rather than RTTY with the serial
    /// numbers filed off, and the only way to show that is to instantiate it
    /// over a mode that shares no number with RTTY and watch every bound still
    /// hold. Every value below is deliberately different from [`RTTY`]'s.
    const TESTMODE: KeyboardMode = KeyboardMode {
        name: "TESTMODE",
        charset: Charset::Ita2,
        max_latch_ms: 5_000,
        type_buf_cap: 6,
        frame_bits: 10.0,
        stream_ahead_chars: 3.0,
        stream_max_chunk: 2,
    };

    /// The three pieces of state a latch is, owned by the test rather than by an
    /// engine — which is the whole claim: no engine is required.
    #[derive(Default)]
    struct Host {
        latched: bool,
        type_buf: VecDeque<char>,
        start_ms: Option<u64>,
    }

    impl Host {
        fn latch(&mut self, mode: KeyboardMode) -> KeyboardLatch<'_> {
            KeyboardLatch::new(
                mode,
                &mut self.latched,
                &mut self.type_buf,
                &mut self.start_ms,
            )
        }
    }

    /// Every gate up, no watchdog, at `now_ms`.
    fn gates_up(now_ms: u64) -> KbGates {
        KbGates {
            tx_enabled: true,
            tx_allowed: true,
            tuning: false,
            in_section: true,
            now_ms,
            now_secs: now_ms / 1000,
            watchdog_limit_secs: 0,
            watchdog_start_secs: None,
        }
    }

    #[test]
    fn a_second_mode_instantiates_the_same_latch() {
        let mut h = Host::default();
        // Nothing keys before the operator asks.
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(1_000), 4).tick,
            KbTick::Idle,
            "an un-latched mode keyed on its own"
        );
        assert!(h.latch(TESTMODE).latch_on(1_000));
        assert!(
            !h.latch(TESTMODE).latch_on(9_999),
            "re-latching must be idempotent — it would buy another full ceiling"
        );
        assert_eq!(h.start_ms, Some(1_000), "re-latching restarted the ceiling");
        // Latched with nothing typed idles on the mode's fill, not silence.
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(1_020), 4).tick,
            KbTick::Diddle
        );
        // …and a full look-ahead keys nothing while staying up.
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(1_040), 0).tick,
            KbTick::Ahead
        );
        // Typed characters go out in budget-sized chunks, in order.
        assert_eq!(h.latch(TESTMODE).type_chars("HELLO"), 5);
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(1_060), 2).tick,
            KbTick::Text("HE".to_string())
        );
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(1_080), 9).tick,
            KbTick::Text("LLO".to_string())
        );
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(1_100), 4).tick,
            KbTick::Diddle
        );
    }

    #[test]
    fn a_second_mode_drops_on_every_gate() {
        // G1..G4, one at a time: each must drop the latch outright rather than
        // hold the feed, and each must report the stream WAS running so the
        // caller arms its abort.
        for (name, break_gate) in [
            ("TX off", 0),
            ("outside privileges", 1),
            ("a tune carrier", 2),
            ("left the section", 3),
        ] {
            let mut h = Host::default();
            h.latch(TESTMODE).latch_on(1_000);
            h.latch(TESTMODE).type_chars("AB");
            let mut g = gates_up(1_020);
            match break_gate {
                0 => g.tx_enabled = false,
                1 => g.tx_allowed = false,
                2 => g.tuning = true,
                _ => g.in_section = false,
            }
            let res = h.latch(TESTMODE).tick(g, 4);
            assert_eq!(res.drop, Some(KbDrop::GateDown), "{name}: no drop");
            assert_eq!(res.tick, KbTick::Idle, "{name}: kept keying");
            assert!(
                h.latch(TESTMODE).drop_latch(),
                "{name}: the kill path must see a running stream and arm the abort"
            );
            assert!(!h.latched && h.type_buf.is_empty() && h.start_ms.is_none());
        }
    }

    #[test]
    fn a_second_modes_ceiling_is_its_own_and_typing_cannot_extend_it() {
        let mut h = Host::default();
        h.latch(TESTMODE).latch_on(1_000);
        // One millisecond under: still running, even at RTTY's ceiling — the
        // bound is the MODE's, not a constant the machinery remembers.
        let almost = 1_000 + TESTMODE.max_latch_ms - 1;
        assert_eq!(
            h.latch(TESTMODE).tick(gates_up(almost), 4).tick,
            KbTick::Diddle
        );
        // The test mode must not share RTTY's ceiling or this proves nothing —
        // and being a const block, a future edit that made them equal fails to
        // COMPILE rather than quietly turning this test vacuous.
        const {
            assert!(TESTMODE.max_latch_ms < RTTY.max_latch_ms);
        }
        // Typing at the moment of the check does not buy a millisecond.
        h.latch(TESTMODE).type_chars("AAAA");
        let over = 1_000 + TESTMODE.max_latch_ms;
        let res = h.latch(TESTMODE).tick(gates_up(over), 4);
        assert_eq!(res.drop, Some(KbDrop::Ceiling));
        assert_eq!(res.tick, KbTick::Idle);
        // The ceiling bounds the whole KEYED period: intent down, buffer still
        // draining on the air, still bounded.
        let mut h2 = Host::default();
        h2.latch(TESTMODE).latch_on(1_000);
        h2.latch(TESTMODE).type_chars("A LONG TAIL");
        h2.latch(TESTMODE).latch_off();
        assert!(
            h2.start_ms.is_some(),
            "lowering intent must not forget the ceiling the drain is still under"
        );
        assert!(
            h2.latch(TESTMODE).streaming(),
            "the drain is a transmission"
        );
        assert_eq!(
            h2.latch(TESTMODE).tick(gates_up(over), 4).drop,
            Some(KbDrop::Ceiling),
            "the drain outran the ceiling unbounded"
        );
    }

    #[test]
    fn a_second_modes_watchdog_starts_late_and_trips() {
        let mut h = Host::default();
        h.latch(TESTMODE).latch_on(1_000);
        // The clock is started by the tick that reaches the check, and reported
        // rather than assumed — a tick a gate already ended must not date an
        // unattended run.
        let mut g = gates_up(1_020);
        g.watchdog_limit_secs = 60;
        g.now_secs = 100;
        let res = h.latch(TESTMODE).tick(g, 4);
        assert_eq!(res.watchdog_start, Some(100), "the clock was not started");
        assert_eq!(res.tick, KbTick::Diddle);
        // A tick that a gate ends first reports no clock at all.
        let mut blocked = g;
        blocked.tx_enabled = false;
        assert_eq!(
            h.latch(TESTMODE).tick(blocked, 4).watchdog_start,
            None,
            "a tick a gate ended started the unattended-run clock anyway"
        );
        // Walking away trips it, and the trip carries the clock forward.
        let mut late = g;
        late.watchdog_start_secs = Some(100);
        late.now_secs = 160;
        let res = h.latch(TESTMODE).tick(late, 4);
        assert_eq!(res.drop, Some(KbDrop::Watchdog));
        assert_eq!(res.tick, KbTick::Idle);
        assert_eq!(res.watchdog_start, Some(100));
    }

    #[test]
    fn a_second_mode_bounds_its_own_type_ahead() {
        let mut h = Host::default();
        h.latch(TESTMODE).latch_on(1_000);
        assert_eq!(
            h.latch(TESTMODE).type_chars("ABCDEFGHIJ"),
            TESTMODE.type_buf_cap,
            "the type-ahead ran past the mode's cap"
        );
        assert_eq!(h.type_buf.len(), TESTMODE.type_buf_cap);
        assert_eq!(
            h.latch(TESTMODE).type_chars("MORE"),
            0,
            "a full cap accepted more"
        );
    }

    #[test]
    fn the_look_ahead_bound_is_per_mode_milliseconds() {
        // Seam 4's claim, on the generic side: the bound is the mode's numbers
        // and the baud, with no constant of its own.
        assert_eq!(RTTY.char_ms(45.45), 7.5 * (1000.0 / 45.45));
        assert_eq!(RTTY.stream_ahead_ms(45.45), 2.0 * (7.5 * (1000.0 / 45.45)));
        assert_eq!(TESTMODE.char_ms(50.0), 10.0 * (1000.0 / 50.0));
        assert_eq!(
            TESTMODE.stream_ahead_ms(50.0),
            3.0 * (10.0 * (1000.0 / 50.0))
        );
        // Faster baud, tighter bound — the stuck-carrier window scales with the
        // mode's character time, which is the reason it is expressed in them.
        assert!(RTTY.stream_ahead_ms(75.0) < RTTY.stream_ahead_ms(45.45));
    }

    #[test]
    fn the_ita2_filter_is_the_mode_descriptors_charset() {
        assert_eq!(kb_filter(RTTY, "cq de w1abc"), "CQ DE W1ABC");
        // No lower case survives, and what ITA2 cannot carry is dropped rather
        // than mangled into something else on the air.
        assert_eq!(kb_filter(RTTY, "abc\u{263A}123"), "ABC123");
        // The filter follows the CHARSET, not the mode's name.
        assert_eq!(kb_filter(TESTMODE, "cq"), kb_filter(RTTY, "cq"));
    }

    #[test]
    fn the_varicode_filter_keeps_case_and_ascii_punctuation() {
        // Full ASCII survives verbatim — case, brackets, the brag-line
        // characters Baudot cannot carry. This is PSK31's reason to exist.
        let brag = "Name is Op, QTH: [EN53] = 5w @ dipole; hw? BTU de KD9TAW k";
        assert_eq!(kb_filter(PSK31, brag), brag);
        // Enter is a real new line on the air; other control chars and
        // anything past ASCII are dropped, never mangled.
        assert_eq!(kb_filter(PSK31, "a\r\nb"), "a\r\nb");
        assert_eq!(kb_filter(PSK31, "a\u{263A}b\x07c"), "abc");
    }
}
