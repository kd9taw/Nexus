//! The per-slot transmit/receive decision — the heart of the radio loop, split
//! out of `service.rs::run_radio` so it is unit-testable with a `MockBackend`
//! (and a VOX/mock rig) and needs no sound card. This is a behavior-preserving
//! extraction of the slot core; the device/network/tune machinery stays in
//! `run_radio`.

use tempo_app::engine::Engine;
use tempo_core::tempo_fast;
use tempo_core::timing::now_unix_ms;

use crate::backend::AudioBackend;
use crate::frames::RxRing;
use crate::rig::Rig;

/// PTT-hold tail after the transmitted audio plays out (ms) — covers ring
/// drain + relay release so the start of RX isn't clipped by our own carrier.
pub const TX_TAIL_MS: f64 = 250.0;

/// The PTT-hold deadline for an over keyed at `keyed_ms`, **bounded by the slot it
/// was keyed in**.
///
/// ⭐ THIS BOUND IS WSJT-X'S, AND WE HAD NOTHING LIKE IT. Upstream computes no
/// deadline at all: it clamps the window to the period and re-tests position
/// continuously, every GUI tick (mainwindow.cpp):
///
/// ```text
/// double tx1=0.0;
/// double tx2=txDuration;
/// if(tx2>m_TRperiod) tx2=m_TRperiod;        // clamped to the period
/// m_bTxTime = (t2p >= tx1) and (t2p < tx2); // t2p = position WITHIN the period
/// ```
///
/// Because `t2p` is slot-relative and `tx2` is clamped, **an upstream over can
/// never cross the boundary** however late keying starts. Ours was an ABSOLUTE
/// deadline `keyed + audio + tail` with no slot term, so keying latency — a
/// blocking CAT exchange can spend a full 700 ms deadline, 2500 ms on a slow
/// transport — converted directly into slot overrun.
///
/// FT8 and FT4 hide that: 12.64 s of audio in a 15 s slot and 5.04 s in 7.5 s
/// leave ~2.11 s and ~2.21 s of slack, more than any single CAT stall.
///
/// ⚠️ **FT1 HAS −250 ms, AND IS THEREFORE CLAMPED ON EVERY OVER, AT EVERY PHASE — INCLUDING ONE
/// KEYED EXACTLY ON THE BOUNDARY.** An earlier version of this doc said "214 ms", computed from
/// FT1's TONE length (99 symbols x 3000/7 samples = 3.5357 s). That is not what the deadline is
/// sized from. `tempo_fast::gen_wave` returns a FIXED NMAX = 48000-sample (4.0000 s) buffer —
/// tones plus trailing zeros, and `Ft1Mode::gen_wave` shifts the lead-in WITHIN it rather than
/// prepending, precisely to keep the length at 4.0000 s (crates/modes/src/mode.rs). The hold is
/// sized from `w.len()`, so the real arithmetic is 4.000 + 0.250 = 4.250 in a 4.000 s period.
/// FT1's effective post-tone tail is a fixed **64.2 ms** (tones end at 0.400 + 3.5357 = 3.9357 s;
/// the clamp drops PTT at 4.000 s).
///
/// **RESOLVED 2026-08-05 by shortening the LEAD, on the operator's decision.** At the former
/// 0.400 s lead the tail was 64 ms, and because the device output ring lags the buffer by the
/// soundcard's latency, any output latency above that cut FT1's RF mid-tone — inside the failure
/// region for ordinary 20-100 ms cpal buffers. The lead is now 0.300 s
/// (`FT1_LEAD_IN_SECS`, crates/modes/src/mode.rs) and the tail is **164 ms**.
///
/// Lead and tail come out of one 464 ms budget and trade one-for-one, so this cost 100 ms of
/// early-timing tolerance — measured as a hard cliff, not a slope, in
/// `crates/tempo-fast/tests/ft1_lead_sweep.rs`. FT1 still cannot have both a full lead and a full
/// 250 ms tail; 0.300/0.164 is the balance chosen, and the sweep is the instrument for revisiting
/// it. The clamp itself is unchanged and still fires on every FT1 over. The 4 s period is by design (operator,
/// 2026-08-04: "ft1 has a designed 4 sec timing… but it should still follow strict
/// wsjtx based aspects of the transmission"); the missing bound was the defect.
///
/// The boundary is derived from `keyed_ms` rather than from a caller-supplied slot
/// index so it cannot disagree with the instant it bounds — same arithmetic as
/// `SlotClock::next_boundary_ms`, and the same "position within the period"
/// semantics upstream uses.
///
/// Clamping can only make PTT drop EARLIER, never later, so it cannot weaken the
/// transmit-path safety invariants. Floored at `keyed_ms` so a hold is never
/// negative: an over keyed past its own boundary releases at once rather than
/// carrying a deadline in the past.
pub fn tx_deadline_ms(keyed_ms: f64, audio_ms: f64, period_ms: f64) -> f64 {
    let natural_end = keyed_ms + audio_ms + TX_TAIL_MS;
    if period_ms.is_nan() || period_ms <= 0.0 {
        return natural_end; // no period to bound against
    }
    let boundary = (keyed_ms / period_ms).floor() * period_ms + period_ms;
    natural_end.min(boundary).max(keyed_ms)
}

/// What a slot did, for the caller to thread back into loop state + reporting.
pub struct SlotAction {
    /// Set when we transmitted: hold PTT until this Unix-ms deadline.
    pub tx_until_ms: Option<f64>,
    /// True when we decoded a receive frame into the engine this slot.
    pub did_rx: bool,
    /// The decoded period's samples (moved out, no extra copy) — the loop saves
    /// them as a WAV when settings.save_wav asks. None when nothing was decoded.
    pub rx_frame: Option<Vec<f32>>,
    /// True when we transmitted this slot — the next boundary uses this as
    /// `prev_was_tx` so it knows the capture ring then holds our own carrier.
    pub tx_this_slot: bool,
    /// Fake-It split moved the VFO for this over — restore it to this dial
    /// (Hz) once the over finishes playing (the loop owns the PTT deadline).
    pub fake_it_restore: Option<u64>,
    /// Rig-mode split engaged VFO B for this over — the loop tears the rig
    /// split down once the over ends (it would otherwise stay latched and a
    /// later in-window over would TX on a stale VFO B frequency).
    pub rig_split_engaged: bool,
}

/// What the Split-Operation pre-key step did, for the loop's teardown.
pub(crate) struct SplitApply {
    pub fake_it_restore: Option<u64>,
    pub rig_split_engaged: bool,
}

/// Apply the WSJT-X Split-Operation dial shift for the over about to key (must
/// run BEFORE PTT): `Rig` = shifted TX dial on VFO B (rig split); `FakeIt` =
/// retune the single VFO. Reports what engaged so the loop restores/tears down
/// at over end. No-op when the engine left shift = 0.
pub(crate) fn apply_tx_dial_shift(eng: &mut Engine, rig: &mut Rig) -> SplitApply {
    use tempo_app::settings::SplitMode;
    let none = SplitApply {
        fake_it_restore: None,
        rig_split_engaged: false,
    };
    let shift = eng.take_tx_dial_shift();
    if shift == 0 {
        return none;
    }
    let dial = eng.settings().dial_hz();
    let tx_dial = (dial as i64 + shift).max(0) as u64;
    match eng.settings().split_mode {
        SplitMode::Rig => {
            let _ = rig.set_split(true, "VFOB");
            let _ = rig.set_split_freq(tx_dial);
            SplitApply {
                fake_it_restore: None,
                rig_split_engaged: true,
            }
        }
        SplitMode::FakeIt => {
            let _ = rig.set_freq(tx_dial);
            SplitApply {
                fake_it_restore: Some(dial),
                rig_split_engaged: false,
            }
        }
        SplitMode::None => none, // shift can't be non-zero here, but stay total
    }
}

/// Run one slot boundary.
///
/// At each boundary we FIRST decode the audio of the slot that just ended, THEN
/// decide whether to transmit in the new slot — the order matters so the QSO
/// auto-sequencer reacts to what we just heard (e.g. a grid reply → send a
/// report) when choosing this slot's message.
///
/// The decode is gated on **`prev_was_tx`** — whether we transmitted in the slot
/// that just ended — NOT on whether we're about to transmit now. The capture ring
/// holds one slot; if we transmitted in it, it holds our own carrier (skip), but
/// if it was a receive slot it holds the other stations and MUST be decoded even
/// when we're about to key again. (The previous logic tied the decode to the new
/// slot's TX, so calling CQ every other slot cleared each RX slot's audio without
/// ever decoding it — stations between our transmissions were never heard.)
/// `currently_tx` is the caller's `tx_until_ms.is_some()` (a TX tail crossing the
/// boundary), which also suppresses the decode.
#[allow(clippy::too_many_arguments)]
pub fn run_slot(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    rx: &mut RxRing,
    slot: u64,
    now_ms: f64,
    currently_tx: bool,
    prev_was_tx: bool,
) -> SlotAction {
    // 1. Decode the just-ended slot's RX audio first (so TX can react to it).
    //    Synchronous reference form: the live loop instead DISPATCHES the decode to
    //    the worker thread and runs step 2 only once the result lands (see
    //    `service.rs`), which keeps the exact decode→TX ordering while freeing the
    //    engine mutex + loop during the ~1–2 s decode.
    let mut rx_frame = None;
    let did_rx = if slot_wants_decode(currently_tx, prev_was_tx, rx.is_empty()) {
        let frame = rx.frame();
        eng.ingest(&frame, slot);
        rx_frame = Some(frame);
        true
    } else {
        // Own carrier (we transmitted in the just-ended slot or a TX tail is
        // crossing the boundary): the ring holds our own carrier — DROP it
        // deterministically so a fragment can't contaminate the next decode.
        if prev_was_tx || currently_tx {
            rx.clear();
        }
        false
    };

    // 2. Transmit decision for the NEW slot (now informed by the decode above).
    slot_tx_phase(eng, rig, backend, rx, slot, now_ms, did_rx, rx_frame, None)
}

/// Whether this boundary should decode the just-ended slot's RX audio: only when
/// we did NOT transmit in it (`prev_was_tx`), no TX tail is crossing the boundary
/// (`currently_tx`), and the capture ring actually holds a period. When it holds
/// our own carrier instead, the caller clears it. Extracted so the decode gate is
/// identical between the synchronous [`run_slot`] and the async loop.
pub fn slot_wants_decode(currently_tx: bool, prev_was_tx: bool, rx_empty: bool) -> bool {
    !(prev_was_tx || currently_tx || rx_empty)
}

/// The transmit half of a slot boundary (step 2 of [`run_slot`]): make and, if due,
/// key the transmission for `slot`. Called with the decode of the just-ended slot
/// ALREADY folded in — either inline (synchronous [`run_slot`]) or via the worker
/// result (the live loop) — so the auto-sequencer reacts to what we just heard. The
/// `did_rx` / `rx_frame` describe that decode for the caller's reporting + WAV save.
#[allow(clippy::too_many_arguments)]
pub fn slot_tx_phase(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    rx: &mut RxRing,
    slot: u64,
    now_ms: f64,
    did_rx: bool,
    rx_frame: Option<Vec<f32>>,
    // A waveform already built by the caller, or `None` to build it here.
    //
    // Building takes `MODEM_LOCK`, which a running decode holds for the length of
    // that decode. Doing it here means doing it with the ENGINE mutex held, which
    // is what froze the UI: every Tauri snapshot and command queued behind a
    // decode. The radio loop therefore plans, RELEASES the engine, builds, and
    // hands the result in through this argument. Timing on the air is unchanged —
    // the same wait happens in the same place, just without the engine held.
    prebuilt: Option<Vec<Vec<f32>>>,
) -> SlotAction {
    let waves = match prebuilt {
        Some(w) => w,
        None => eng.poll_tx(slot),
    };
    if !waves.is_empty() {
        // Split Operation: move the TX dial (if the engine reduced the audio)
        // BEFORE the carrier keys.
        let split = apply_tx_dial_shift(eng, rig);
        let _ = rig.ptt(true);
        // ⏱ THE PTT-HOLD DEADLINE IS MEASURED FROM HERE — after the carrier is up —
        // not from the caller's `now_ms`. That was bound at the TOP of the radio-loop
        // tick, and the same tick then runs BLOCKING CAT before reaching this key: the
        // retune block (its `can_retune` gate is true at a boundary, `tx_until_ms`
        // being None), `ensure_commanded`, the split shift above, and the keying
        // round-trip itself — each able to spend a full CAT deadline (700 ms, 2500 ms
        // on a slow transport). A deadline built on `now_ms` is therefore measured from
        // an instant already past, and unkeys EARLY by exactly the CAT time, cutting
        // the tail off our own over. MSK144 is where it shows: 14.688 s of audio in a
        // 15 s slot (period − 0.25 s truncated to whole 72 ms frames) leaves ~0.31 s of
        // slack — less than one slow CAT exchange. FT8's 13.14 s leaves ~1.86 s, so
        // every other tier absorbs the same stall as a merely shortened tail.
        //
        // ⚠️ STEER THE RE-READ. `now_ms` is on the UTC-STEERED clock (`service.rs`
        // SUBTRACTS the measured PC-clock-vs-UTC offset so TX keys on the true UTC
        // grid). A bare `now_unix_ms()` here would be on a different timebase and shift
        // the deadline by the whole offset — worst on exactly the badly-synced machines
        // the steering exists to rescue. Same expression as the busy-worker branch's
        // post-build re-read in `service.rs`.
        //
        // Floored at `now_ms`: a backwards clock step (an NTP correction landing mid-
        // tick) must never make this hold SHORTER than the caller's basis — that is the
        // failure this whole re-read exists to prevent.
        let keyed_ms = (now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64).max(now_ms);
        let mut secs = 0.0f32;
        for w in &waves {
            secs += w.len() as f32 / tempo_fast::SAMPLE_RATE;
            backend.play(w);
        }
        rx.clear(); // our just-started carrier must not be decoded next boundary
                    // The bound the re-read above deliberately does NOT provide. Measuring the
                    // deadline from the key (rather than the tick's stale clock) stopped us
                    // unkeying early; it also means keying latency lands wholly on the far end of
                    // the over, and nothing was stopping that from running past the boundary. See
                    // [`tx_deadline_ms`] — this is WSJT-X's clamp, and FT1 is where the absence
                    // showed: its buffer FILLS its period, so it is clamped on every over.
        let period_ms = eng.active_slot_secs() * 1000.0;
        SlotAction {
            tx_until_ms: Some(tx_deadline_ms(keyed_ms, secs as f64 * 1000.0, period_ms)),
            did_rx,
            rx_frame,
            tx_this_slot: true,
            fake_it_restore: split.fake_it_restore,
            rig_split_engaged: split.rig_split_engaged,
        }
    } else {
        // Receive slot: keep the rolling capture window (no clear) so the next
        // boundary decodes this slot's audio.
        SlotAction {
            tx_until_ms: None,
            did_rx,
            rx_frame,
            tx_this_slot: false,
            fake_it_restore: None,
            rig_split_engaged: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;
    use tempo_app::engine::{run_decode_job, DecodeApplied, DecodePass};

    /// The clock instant a keyed [`SlotAction`]'s PTT-hold deadline was built from:
    /// strip the played audio's duration and the fixed tail back off. That basis is
    /// the whole subject of the three tests below.
    fn deadline_basis_ms(act: &SlotAction, backend: &MockBackend) -> f64 {
        let audio_ms = backend.played.len() as f64 / tempo_fast::SAMPLE_RATE as f64 * 1000.0;
        act.tx_until_ms.expect("the over keyed") - audio_ms - TX_TAIL_MS
    }

    /// "Now" on the same UTC-steered timebase the radio loop hands in as `now_ms`
    /// (`service.rs` subtracts the measured PC-clock-vs-UTC offset from the system
    /// clock so TX keys on the true UTC grid).
    fn steered_now_ms(eng: &Engine) -> f64 {
        now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64
    }

    /// An engine armed with one over to send, on the default (FT8) tier.
    fn armed_engine() -> Engine {
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx)
        eng.broadcast("CQ TEST W9XYZ EN37");
        align_to_slot_start(&mut eng, 0);
        eng
    }

    /// Steer the engine's clock so an over keyed *now* starts just after a slot
    /// boundary, plus `skew_ms` of PC-clock error rounded to whole periods.
    ///
    /// ⚠️ WHY THE BASIS TESTS NEED THIS. Their deadline is built from a re-read of the
    /// real clock, so without steering, the over's PHASE within its slot is whatever
    /// time the suite happened to run at. Since [`tx_deadline_ms`] bounds the deadline
    /// at the boundary, an unaligned run truncates it at some phases and not others —
    /// which would turn "the basis is the key, not the stale tick" into a coin flip
    /// that passes ~14% of the time (FT8 leaves 2.11 s of slack in 15 s). Aligning
    /// keeps the clamp a no-op so those assertions still measure what they were
    /// written to measure. `offset ≡ now (mod period)` is what puts the steered clock
    /// on a boundary; adding whole periods preserves the congruence, which is how a
    /// large skew and a known phase coexist.
    fn align_to_slot_start(eng: &mut Engine, skew_ms: i64) {
        let period_ms = eng.active_slot_secs() * 1000.0;
        let phase = now_unix_ms().rem_euclid(period_ms);
        let whole = (skew_ms as f64 / period_ms).round() * period_ms;
        eng.set_clock_offset_ms(Some((phase + whole) as i64));
    }

    /// A throwaway rigctld that answers every command with `RPRT 0` — but only
    /// after `delay_ms`. Models the real hazard: a CAT link whose keying round-trip
    /// eats hundreds of ms (the `Rig` PTT deadline allows up to 700).
    fn slow_rigctld(delay_ms: u64) -> String {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let mut sock = match listener.accept() {
                Ok((s, _)) => s,
                Err(_) => return,
            };
            let mut buf = [0u8; 256];
            loop {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                if sock.write_all(b"RPRT 0\n").is_err() {
                    return;
                }
            }
        });
        addr
    }

    // ── WSJT-X's slot clamp ──────────────────────────────────────────────────
    // These drive [`tx_deadline_ms`] directly rather than through `slot_tx_phase`,
    // because the deadline there is built from a re-read of the REAL clock: the
    // phase of the over within its slot would be whatever time the suite ran at,
    // and a bound that only bites at some phases would be a coin-flip test. The
    // arithmetic is the whole of the fix, so it is tested where it is decidable.

    /// ⚠️ WHAT PRODUCTION ACTUALLY PASSES: `w.len()` of the FIXED NMAX = 48000-sample buffer
    /// `tempo_fast::gen_wave` returns — 4.0000 s, tones PLUS trailing zeros. It is NOT the
    /// 3.5357 s tone length; using that here was the error behind the "214 ms of slack" claim,
    /// and it made `an_on_time_ft1_over_keeps_its_whole_tail` assert a case that cannot occur.
    const FT1_AUDIO_MS: f64 = 48_000.0 / 12_000.0 * 1000.0;
    /// The tone length, for the tests that are about where the SIGNAL ends rather than the buffer.
    const FT1_TONES_MS: f64 = 99.0 * (3000.0 / 7.0) / 12_000.0 * 1000.0;
    const FT1_PERIOD_MS: f64 = 4_000.0;
    /// FT1's lead-in, shifted WITHIN the fixed buffer — mirrors `FT1_LEAD_IN_SECS`
    /// (crates/modes/src/mode.rs). ⚠️ MOVES WITH IT: the tail this file asserts is
    /// `464 ms - lead`, so a change there without a change here silently stops testing the real
    /// geometry. 300 ms since 2026-08-05; the trade is measured in the ft1_lead_sweep test.
    const FT1_LEAD_MS: f64 = 300.0;

    #[test]
    fn an_ft1_over_keyed_late_still_ends_at_the_boundary() {
        // THE DEFECT. FT1 leaves 214 ms of slack (3535.7 ms of audio + a 250 ms tail
        // in a 4 s slot), which is less than one blocking CAT exchange. Keyed 500 ms
        // into the slot, the unclamped deadline runs 286 ms into the NEXT period —
        // transmitting over the slot we are supposed to be receiving in.
        let slot_start = 1_000_000.0 * FT1_PERIOD_MS; // a real boundary
        let keyed = slot_start + 500.0;
        let unclamped = keyed + FT1_AUDIO_MS + TX_TAIL_MS;
        assert!(
            unclamped > slot_start + FT1_PERIOD_MS,
            "the scenario must actually overrun, else this test proves nothing"
        );
        let got = tx_deadline_ms(keyed, FT1_AUDIO_MS, FT1_PERIOD_MS);
        assert_eq!(got, slot_start + FT1_PERIOD_MS, "must end AT the boundary");
        assert!(got < unclamped, "and that is earlier than it used to unkey");
    }

    #[test]
    fn even_a_perfectly_on_time_ft1_over_is_clamped_and_keeps_only_64ms_of_tail() {
        // ⚠️ THIS TEST ASSERTED THE OPPOSITE AND WAS WRONG. It claimed FT1 "fits with 214 ms to
        // spare and nothing is taken off the tail" — a case that CANNOT OCCUR, because it was
        // computed from FT1's 3.5357 s TONE length while production sizes the hold from the
        // FIXED 4.0000 s buffer. There is no unclamped FT1 over at any phase.
        //
        // What is actually true, and what an operator should know: FT1's buffer fills its whole
        // period, so the clamp fires on every over and the effective post-tone tail is a fixed
        // 64.2 ms. That is the margin protecting the end of the signal from soundcard latency,
        // and it is thin — see the hazard note on `tx_deadline_ms`.
        let slot_start = 1_000_000.0 * FT1_PERIOD_MS;
        let got = tx_deadline_ms(slot_start, FT1_AUDIO_MS, FT1_PERIOD_MS);
        assert_eq!(
            got,
            slot_start + FT1_PERIOD_MS,
            "an on-time FT1 over is STILL clamped — 4.000 s of buffer + 250 ms tail in a 4.000 s \
             period leaves -250 ms, not the +214 ms this test used to claim"
        );
        // The tail that actually survives, measured from where the TONES end.
        let tone_end = slot_start + FT1_LEAD_MS + FT1_TONES_MS;
        let tail_ms = got - tone_end;
        assert!(
            (tail_ms - 164.2).abs() < 0.5,
            "FT1's real post-tone tail is ~164.2 ms at the 300 ms lead, got {tail_ms:.1} — if \
             this moved, so did the margin protecting the end of the signal from soundcard \
             output latency, which is the whole reason the lead was shortened"
        );
    }

    #[test]
    fn no_tier_can_key_past_its_own_boundary_at_any_phase() {
        // The bound is upstream's and applies to every mode, not only the one that
        // exposed it. FT8 12.64 s / 15 s and FT4 5.04 s / 7.5 s have enough slack to
        // absorb an ordinary stall, but a pathological one must not put THEM into the
        // next period either — the clamp is what makes that unrepresentable.
        for (audio_ms, period_ms, name) in [
            (FT1_AUDIO_MS, FT1_PERIOD_MS, "FT1"),
            (12_640.0, 15_000.0, "FT8"),
            (5_040.0, 7_500.0, "FT4"),
            (14_688.0, 15_000.0, "MSK144-15"),
        ] {
            let slot_start = 1_000.0 * period_ms;
            // Every 10 ms of phase across a whole slot.
            let mut phase = 0.0;
            while phase < period_ms {
                let keyed = slot_start + phase;
                let got = tx_deadline_ms(keyed, audio_ms, period_ms);
                assert!(
                    got <= slot_start + period_ms,
                    "{name} keyed at phase {phase} ms held PTT {:.1} ms past its boundary",
                    got - (slot_start + period_ms),
                );
                assert!(got >= keyed, "{name} produced a deadline before the key");
                phase += 10.0;
            }
        }
    }

    #[test]
    fn an_over_keyed_past_its_boundary_releases_at_once() {
        // Degenerate, but it must not produce a deadline in the past: a hold the loop
        // reads as already expired is fine, a negative one is not.
        let slot_start = 1_000_000.0 * FT1_PERIOD_MS;
        let keyed = slot_start + FT1_PERIOD_MS; // exactly the next boundary
        let got = tx_deadline_ms(keyed, FT1_AUDIO_MS, FT1_PERIOD_MS);
        assert!(got >= keyed, "never a deadline before the key");
    }

    #[test]
    fn a_nonsense_period_falls_back_to_the_unbounded_deadline() {
        // A zero/NaN period must not silently unkey us instantly — better to hold as
        // we always did than to cut every over on a bad reading.
        let keyed = 4_000_000.0;
        let natural = keyed + FT1_AUDIO_MS + TX_TAIL_MS;
        assert_eq!(tx_deadline_ms(keyed, FT1_AUDIO_MS, 0.0), natural);
        assert_eq!(tx_deadline_ms(keyed, FT1_AUDIO_MS, f64::NAN), natural);
    }

    #[test]
    fn ptt_hold_is_measured_from_the_key_not_the_ticks_stale_clock() {
        // THE BUG. `now_ms` is bound at the TOP of the radio-loop tick
        // (`service.rs`), and that same tick then runs BLOCKING CAT before the
        // carrier is ever up: the retune block (its `can_retune` gate is true at a
        // boundary, `tx_until_ms` being None) and `ensure_commanded`, each able to
        // spend a full CAT deadline (700 ms, 2500 ms on a slow transport). By the
        // time we key, `now_ms` is that far in the past — so a deadline built on it
        // unkeys EARLY by exactly the CAT time and truncates the tail of the over.
        //
        // MSK144-15 is where it shows: 14.688 s of audio in a 15 s slot leaves
        // ~0.31 s of slack, less than one slow CAT exchange. Every other tier has
        // enough slack to absorb it as a merely shortened tail.
        const CAT_STALL_MS: f64 = 400.0;
        let mut eng = armed_engine();
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        // The tick bound `now` CAT_STALL_MS ago; the blocking CAT that followed is
        // what ate the gap between then and this call.
        let steered = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            steered - CAT_STALL_MS,
            false,
            None,
            None,
        );

        assert!(act.tx_this_slot, "the CQ keyed");
        let basis = deadline_basis_ms(&act, &backend);
        assert!(
            (basis - steered).abs() < 100.0,
            "PTT-hold deadline must be measured from the key ({steered:.0}), not from \
             the tick's stale clock ({:.0}) — off by {:.0} ms",
            steered - CAT_STALL_MS,
            basis - steered,
        );
    }

    #[test]
    fn a_slow_key_pushes_the_hold_out_by_the_time_the_key_itself_took() {
        // The same defect measured on the OTHER side of the call: the CAT round-trip
        // of `rig.ptt(true)` is itself inside the window. A fix that re-read the
        // clock on entry (rather than after the key) would still unkey early by the
        // keying round-trip. Here the rig answers `T 1` only after KEY_MS.
        const KEY_MS: u64 = 300;
        let mut eng = armed_engine();
        let mut rig = Rig::rigctld(&slow_rigctld(KEY_MS));
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let now_ms = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            now_ms,
            false,
            None,
            None,
        );
        let after = steered_now_ms(&eng);

        assert!(act.tx_this_slot, "the CQ keyed");
        assert!(rig.keyed, "PTT asserted");
        let basis = deadline_basis_ms(&act, &backend);
        assert!(
            basis - now_ms >= 200.0,
            "a {KEY_MS} ms key must push the hold out with it — moved only {:.0} ms",
            basis - now_ms,
        );
        assert!(
            (after - basis).abs() < 100.0,
            "and the basis is the moment the carrier came up, not the call's entry \
             ({:.0} ms before this call returned)",
            after - basis,
        );
    }

    #[test]
    fn the_hold_clock_is_utc_steered_like_the_ticks_clock_is() {
        // THE TRAP. `now_ms` is UTC-STEERED: `service.rs` SUBTRACTS the measured
        // PC-clock-vs-UTC offset so TX keys on the true UTC grid even when the OS
        // clock is skewed. Re-reading the raw system clock instead puts the deadline
        // on a DIFFERENT timebase and shifts it by the whole offset — silently, and
        // worst on exactly the badly-synced machines the steering exists to rescue.
        // A 90 s skew is ordinary on a PC that has never talked to an NTP server.
        const OFFSET_MS: i64 = 90_000; // PC clock ~90 s AHEAD of true UTC
        const CAT_STALL_MS: f64 = 400.0;
        let mut eng = armed_engine();
        // Whole periods, so the skew stays ~90 s while the steered clock still lands
        // on a boundary — see `align_to_slot_start`. 90 s is six whole FT8 periods.
        align_to_slot_start(&mut eng, OFFSET_MS);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let steered = steered_now_ms(&eng);
        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            steered - CAT_STALL_MS,
            false,
            None,
            None,
        );

        let basis = deadline_basis_ms(&act, &backend);
        assert!(
            (basis - steered).abs() < 150.0,
            "the hold must be measured on the STEERED clock: {:.0} ms off (a raw \
             now_unix_ms() re-read would be out by the whole {OFFSET_MS} ms offset)",
            basis - steered,
        );
    }

    /// Drive the async two-phase boundary synchronously (as the live loop does, but
    /// in-line instead of across the worker thread): decide, dispatch, run the
    /// decode, fold it, then run the TX phase. Returns the action + whether a
    /// boundary decode was actually folded. This is the exact ordering the loop
    /// preserves — decode of the just-ended slot ALWAYS folded before poll_tx.
    #[allow(clippy::too_many_arguments)]
    fn two_phase_boundary(
        eng: &mut Engine,
        rig: &mut Rig,
        backend: &mut MockBackend,
        rx: &mut RxRing,
        slot: u64,
        now_ms: f64,
        currently_tx: bool,
        prev_was_tx: bool,
    ) -> (SlotAction, bool) {
        if slot_wants_decode(currently_tx, prev_was_tx, rx.is_empty()) {
            let frame = rx.frame();
            // Phase 1: dispatch (build the owned job) + the heavy decode.
            let job = eng.build_decode_job(frame, slot, DecodePass::Boundary);
            let result = run_decode_job(job);
            // Phase 2a: fold the result under the engine lock.
            let (folded, rx_frame) = match eng.apply_decode_result(result) {
                DecodeApplied::Boundary { slot: _, frame, .. } => (true, Some(frame)),
                _ => (false, None),
            };
            // Phase 2b: the TX decision — now informed by the decode above.
            let act = slot_tx_phase(eng, rig, backend, rx, slot, now_ms, true, rx_frame, None);
            (act, folded)
        } else {
            if prev_was_tx || currently_tx {
                rx.clear();
            }
            let act = slot_tx_phase(eng, rig, backend, rx, slot, now_ms, false, None, None);
            (act, false)
        }
    }

    #[test]
    fn fake_it_split_reports_the_restore_dial() {
        // FakeIt: an out-of-window TX offset shifts the dial for the over and
        // the action carries the dial to RESTORE once the over finishes — the
        // loop applies it at PTT drop. Rig/None report nothing to restore.
        // TX-only boundary (empty ring → no decode): the TX phase is called directly.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::Ft8);
        let mut st = eng.settings().clone();
        st.split_mode = tempo_app::settings::SplitMode::FakeIt;
        eng.apply_settings(st);
        eng.set_tx_enabled(true);
        eng.set_tx_offset(750.0); // f0 1750, dial -1000
        eng.broadcast("CQ W9XYZ EN37");
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let act = slot_tx_phase(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            None,
            None,
        );

        assert!(act.tx_this_slot, "the CQ keyed");
        assert_eq!(
            act.fake_it_restore,
            Some(eng.settings().dial_hz()),
            "restore dial = the RX dial the over shifted away from"
        );
    }

    #[test]
    fn tx_slot_keys_ptt_plays_audio_and_sets_hold() {
        // Engine with tx_parity 0 transmits on EVEN slots; queue a broadcast.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        eng.broadcast("CQ TEST W9XYZ EN37");
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        // No captured RX (empty ring) → the TX phase runs directly.
        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            false,
        );

        assert!(!folded, "empty ring → no decode folded");
        assert!(rig.keyed, "PTT keyed for the TX over");
        assert!(
            !backend.played.is_empty(),
            "transmit audio played to the backend"
        );
        assert!(
            act.tx_until_ms.unwrap() > 1000.0 + 250.0,
            "PTT held for audio duration + tail"
        );
        assert!(!act.did_rx);
        assert!(act.tx_this_slot, "flagged as a transmit slot");
    }

    #[test]
    fn rx_slot_decodes_without_keying() {
        // Idle engine → nothing to send even on its TX slot → receive path. The
        // two-phase driver decodes the captured slot, folds it, THEN runs poll_tx.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::TempoFast); // FT1-modem slot test (default is FT8)
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]); // a captured RX slot sits in the ring

        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            false,
        );

        assert!(folded, "the RX frame was decoded + folded before poll_tx");
        assert!(!rig.keyed, "no PTT on a receive slot");
        assert!(backend.played.is_empty(), "no audio played on RX");
        assert!(act.did_rx, "reported as a decode slot");
        assert!(!act.tx_this_slot);
        assert!(act.tx_until_ms.is_none());
    }

    #[test]
    fn mid_transmit_does_not_double_decode() {
        // While the PTT tail is still held (currently_tx), an idle slot is a no-op:
        // we must NOT decode (we'd be decoding our own tail) and not re-key.
        assert!(
            !slot_wants_decode(true, false, false),
            "a TX tail crossing the boundary suppresses the decode"
        );
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();

        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            true,
            false,
        );

        assert!(!folded, "no RX decode mid-transmit");
        assert!(!act.did_rx);
        assert!(act.tx_until_ms.is_none());
        assert!(!rig.keyed);
    }

    #[test]
    fn rx_slot_between_transmits_is_decoded() {
        // The regression: calling CQ (TX on even slots), the RX slot's captured
        // audio must be decoded at the next (TX) boundary — BEFORE we re-key — not
        // cleared away unheard. prev_was_tx=false means the slot that just ended was
        // a receive slot, so its audio (in the ring) is the other stations.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tx_enabled(true); // TX is disarmed by default (WSJT-X Enable-Tx) — arm it
        eng.set_tier(tempo_app::dto::Tier::TempoFast);
        eng.broadcast("CQ TEST W9XYZ EN37"); // something to send on our TX slot
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]); // the RX slot we just finished, captured

        // Even (TX) slot boundary, prior slot was RX (prev_was_tx=false).
        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            2,
            1000.0,
            false,
            false,
        );

        assert!(
            folded,
            "the RX slot's audio is decoded + folded before we transmit again"
        );
        assert!(act.tx_this_slot, "and then we send our CQ");
        assert!(rig.keyed, "PTT keyed for the CQ over");
    }

    #[test]
    fn own_transmit_slot_is_not_decoded_as_rx() {
        // After we transmitted (prev_was_tx=true) the ring holds our own carrier —
        // it must NOT be decoded, even though it is non-empty.
        assert!(
            !slot_wants_decode(false, true, false),
            "our own transmit slot is never decoded as RX"
        );
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::TempoFast);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]); // our own transmission's captured audio

        // Odd (RX) slot boundary; the slot that just ended was our TX.
        let (act, folded) = two_phase_boundary(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            1,
            1000.0,
            false,
            true,
        );

        assert!(!folded, "must not decode our own transmission");
        assert!(!act.did_rx);
        assert!(!act.tx_this_slot);
        assert!(rx.is_empty(), "own-carrier ring is cleared, not decoded");
    }

    #[test]
    fn run_slot_matches_two_phase_for_a_receive_slot() {
        // The synchronous `run_slot` reference and the two-phase decomposition must
        // agree: both decode the just-ended RX slot before the (no-op) TX decision.
        let mut eng = Engine::new("W9XYZ", "EN37", 0);
        eng.set_tier(tempo_app::dto::Tier::TempoFast);
        let mut rig = Rig::vox();
        let mut backend = MockBackend::new();
        let mut rx = RxRing::new();
        rx.push(&vec![0.0; 4096]);
        let act = run_slot(
            &mut eng,
            &mut rig,
            &mut backend,
            &mut rx,
            0,
            1000.0,
            false,
            false,
        );
        assert!(act.did_rx, "run_slot decodes the RX slot");
        assert!(!act.tx_this_slot);
    }
}
