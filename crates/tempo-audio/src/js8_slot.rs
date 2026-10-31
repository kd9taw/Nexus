//! A JS8 frame keyed as JS8Call 3.0.3 keys one: its TX delay before its period, its wave on the slot
//! clock however late the radio answers, and nothing once its last tone has gone.
//!
//! JS8Call asks for PTT on the 100 ms GUI tick that finds the period `TxDelay` from its end
//! (`prepareSending`, `JS8_UI/mainwindow.cpp:2507-2572`: `time_is_in_tx_delay`, :2512-2516), or later
//! while the period is under its late threshold (`fraction_of_tx_slot < lateThreshold`,
//! :2533-2572), and starts its Modulator when the radio answers (`handle_transceiver_update`,
//! :5395-5417). The Modulator lays the frame on the clock (`Modulator::start`,
//! `JS8_Mode/Modulator.cpp:38-122`): from a key in the TX delay, silence up to the period's start
//! and the speed's start delay (:86-94); after the start, less silence (:95-101); later still, the
//! symbols already gone are skipped (:102-107). So the first tone sits at the period's start plus the
//! start delay on the wall clock, whatever the key took. Nexus played the whole frame from the key,
//! at the boundary, so every receiver read the key's latency as DT.
//!
//! The JS8 wave (`js8::phy::modulate`) is slot-positioned, the start delay's silence at its head as
//! FT8's lead-in is, so one placement does all three: silence in front of it for a key before its
//! period ([`placement`]), its samples skipped for a key after. JS8Call starts no frame past its late
//! threshold, and Nexus keys none: a frame decided past it waits for the next period
//! ([`past_late_start`]). A frame decided before it plays what is left of it however late the radio
//! answers the key, as JS8Call's Modulator does; only a key answered once the frame's last tone has
//! gone, inside its period or after it, is unkeyed at once with nothing sent ([`late_key`]), as
//! JS8Call stops TX there and as an FT8/FT4 over the radio keys after its period is.
//!
//! The radio loop's half, keying a frame in its TX delay and ending the RX period before it there, is
//! `service::js8_lead`; the engine's, the TX delay and the late start, is `Engine::js8_tx_delay_ms`
//! and `Engine::js8_late_start_ms`. Every other tier is untouched: each function here answers for
//! JS8 alone.

use tempo_app::dto::Tier;
use tempo_app::engine::Engine;
use tempo_core::tempo_fast;
use tempo_core::timing::now_unix_ms;

use crate::backend::AudioBackend;
use crate::rig::Rig;
use crate::slot::{KeyedOver, TX_TAIL_MS};

/// The start of the period JS8 frame `slot` is for, on the slot clock (`SlotClock::slot_index`).
pub(crate) fn frame_start_ms(eng: &Engine, slot: u64) -> f64 {
    slot as f64 * eng.active_slot_secs() * 1000.0
}

/// Whether a JS8 frame for `slot` decided now must not key: its period is at or past JS8Call's late
/// start (`Engine::js8_late_start_ms`) on the clock as it is now, re-read as the key re-reads it
/// (`now_ms` is the clock the caller's tick read). Decided before the frame is planned, so it is not
/// taken from the queue and goes out in the next period, as JS8Call's `prepareSending` keys nothing
/// past `lateThreshold` and its frame waits. A frame keyed in its TX delay is decided before its
/// period and is never past it. `false` for every other tier.
pub(crate) fn past_late_start(eng: &Engine, slot: u64, now_ms: f64) -> bool {
    if eng.tier() != Tier::Js8 {
        return false;
    }
    let reread_ms = (now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64).max(now_ms);
    reread_ms - frame_start_ms(eng, slot) >= eng.js8_late_start_ms()
}

/// Where a frame whose key the radio answered at `keyed_ms` begins, for the period starting at
/// `start_ms`: the samples of silence to play in front of its slot-positioned wave (a key before the
/// period: JS8Call's TX delay) and the samples of the wave to skip (a key after it: first the start
/// delay's silence, then the symbols gone), at 12 kHz.
pub(crate) fn placement(keyed_ms: f64, start_ms: f64) -> (usize, usize) {
    let samples = ((keyed_ms - start_ms).abs() * f64::from(tempo_fast::SAMPLE_RATE) / 1000.0)
        .round() as usize;
    if keyed_ms < start_ms {
        (samples, 0)
    } else {
        (0, samples)
    }
}

/// Hand a keyed JS8 frame for `slot` to the output on the slot clock ([`placement`]) and say when it
/// ends. PTT is held from the key for what was handed over and [`TX_TAIL_MS`], the frame's end on the
/// slot clock, and never past the end of the frame's own period, which a key in the TX delay lies
/// before (the tier's other overs are bounded by the period their key is in,
/// `slot::tx_deadline_ms`).
pub(crate) fn play_frame(
    eng: &Engine,
    backend: &mut impl AudioBackend,
    waves: &[Vec<f32>],
    keyed_ms: f64,
    slot: u64,
) -> KeyedOver {
    let period_ms = eng.active_slot_secs() * 1000.0;
    let start_ms = frame_start_ms(eng, slot);
    let (silence, mut skip) = placement(keyed_ms, start_ms);
    if silence > 0 {
        backend.play(&vec![0.0; silence]);
    }
    let mut handed = silence;
    for w in waves {
        let rest = &w[skip.min(w.len())..];
        skip = skip.saturating_sub(w.len());
        if !rest.is_empty() {
            backend.play(rest);
            handed += rest.len();
        }
    }
    backend.mark_unit_end();
    let audio_ms = handed as f64 * 1000.0 / f64::from(tempo_fast::SAMPLE_RATE);
    KeyedOver {
        tx_until_ms: (keyed_ms + audio_ms + TX_TAIL_MS)
            .min(start_ms + period_ms)
            .max(keyed_ms),
        audio_until_ms: None,
    }
}

/// How far into its period the radio answered JS8 frame `slot`'s key at `keyed_ms`, and where in
/// the period the frame's last tone ends (`Engine::js8_frame_end_ms`), which the key is at or past:
/// `Some((into_ms, limit_ms))` when the frame must go out with nothing, `None` when it plays.
/// Before its last tone a frame plays what is left of it on the slot clock, past JS8Call's late
/// start too: JS8Call starts its Modulator on the radio's answer however late it comes
/// (`mainwindow.cpp:5395-5417`), and the Modulator skips the symbols already gone
/// (`Modulator.cpp:102-107`). Once the last tone has gone, inside the period or after it, nothing
/// of the frame is left, and Nexus unkeys at once: JS8Call's GUI tick stops TX once its period
/// passes the frame's length (`mainwindow.cpp:2575-2576`, `2681-2682`), and Nexus keys no FT8/FT4
/// over the radio answers after its period. `None` for every other tier.
pub(crate) fn late_key(eng: &Engine, keyed_ms: f64, slot: u64) -> Option<(f64, f64)> {
    if eng.tier() != Tier::Js8 {
        return None;
    }
    let start_ms = frame_start_ms(eng, slot);
    let end_ms = eng.js8_frame_end_ms();
    // In samples, as `play_frame` skips them: late exactly when none of the frame is left to play.
    let (_, skip) = placement(keyed_ms, start_ms);
    let frame = placement(start_ms + end_ms, start_ms).1;
    (skip >= frame).then_some((keyed_ms - start_ms, end_ms))
}

/// A JS8 frame whose key came once its last tone had gone ([`late_key`]): nothing is played (the
/// output is flushed, though it holds nothing of the frame), the radio is unkeyed at once unless
/// the loop already held it (the operator's own PTT, or Hold PTT between frames, which ends at its
/// own deadline), and the cockpit says why. TX stays on: the rest of the message goes out in the
/// next periods, the policy for an FT8/FT4 over the radio keys after its period. An unkey the radio
/// does not take halts TX, as at any slot over's end.
pub(crate) fn unkey_a_late_key(
    eng: &mut Engine,
    rig: &mut Rig,
    backend: &mut impl AudioBackend,
    held: bool,
    into_ms: f64,
    limit_ms: f64,
) {
    backend.flush_output();
    let unkey = (!held).then(|| rig.ptt(false));
    eng.js8_note_late_key(into_ms, limit_ms);
    if let Some(why) = unkey.and_then(|unkey| crate::slot::slot_unkey_failure(&unkey, true)) {
        eng.halt_tx_for_failed_unkey(&why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;
    use tempo_app::engine::Js8Speed;

    /// A JS8 station at `speed`, its clock steered to UTC by nothing.
    fn js8_engine(speed: Js8Speed) -> Engine {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.js8_enter();
        e.js8_set_speed(speed.index()).unwrap();
        e
    }

    /// A slot-positioned stand-in for a JS8 frame at `speed`: the start delay's silence, then a tone
    /// for every symbol whose value is its index (so a sample tells where in the frame it is).
    fn stand_in(speed: Js8Speed) -> Vec<f32> {
        let delay = speed.delay_ms() as usize * 12;
        let mut w = vec![0.0; delay];
        w.extend((0..79 * speed.nsps()).map(|i| 1.0 + i as f32));
        w
    }

    /// ⭐ THE FIRST TONE ON THE SLOT CLOCK, TO THE SAMPLE. A Normal frame (500 ms of start delay) whose
    /// key the radio answered 0.2 s and 0.05 s before its period (in JS8Call's TX delay), or 0.1 s and
    /// 0.6 s after: what is handed to the card puts every sample of the frame where a frame keyed on
    /// the boundary has it, so its first tone is at the period's start and 500 ms, 6000 samples, on
    /// the wall clock. Before the period, silence up to it in front of the whole frame (JS8Call's
    /// Modulator, `Modulator.cpp:86-94`); 0.1 s after, 1200 samples of the start delay skipped
    /// (:95-101); 0.6 s after, the start delay and the first 1200 samples of the symbols skipped
    /// (:102-107). PTT is held to the frame's end on the slot clock and the tail, whatever the key.
    #[test]
    fn the_first_tone_sits_at_the_period_start_and_the_start_delay_whatever_the_key() {
        let e = js8_engine(Js8Speed::Normal);
        let slot = 1_000_000;
        let start = frame_start_ms(&e, slot);
        let frame = stand_in(Js8Speed::Normal);
        for (key_ms, silence, skip) in [
            (-200.0, 2_400, 0),
            (-50.0, 600, 0),
            (100.0, 0, 1_200),
            (600.0, 0, 7_200),
        ] {
            let key_s = key_ms / 1000.0;
            let keyed = start + key_ms;
            assert_eq!(placement(keyed, start), (silence, skip), "key at {key_s} s");
            let mut backend = MockBackend::new();
            let over = play_frame(&e, &mut backend, std::slice::from_ref(&frame), keyed, slot);
            let played = &backend.played;
            // Sample k of what plays goes out k / 12 ms after the key.
            let at_ms = |k: usize| keyed + k as f64 / 12.0;
            if skip <= 6_000 {
                let first = played.iter().position(|&x| x != 0.0).expect("a tone plays");
                assert_eq!(
                    at_ms(first),
                    start + 500.0,
                    "key at {key_s} s: the first tone is not at the period's start + 500 ms"
                );
            }
            assert_eq!(
                played[..silence],
                vec![0.0; silence][..],
                "key at {key_s} s: silence up to the period"
            );
            assert!(
                played[silence..] == frame[skip..],
                "key at {key_s} s: the frame from its own place on the slot clock"
            );
            assert_eq!(
                over.tx_until_ms,
                start + 13_140.0 + TX_TAIL_MS,
                "key at {key_s} s: PTT held to the frame's end and the tail"
            );
            assert_eq!(over.audio_until_ms, None);
        }
    }

    /// A JS8 frame is one unit, its TX-delay silence and all, marked where it ends.
    #[test]
    fn a_frame_marks_its_end() {
        let e = js8_engine(Js8Speed::Normal);
        let slot = 1_000_000;
        let start = frame_start_ms(&e, slot);
        for key_ms in [-200.0, 600.0] {
            let mut backend = MockBackend::new();
            play_frame(
                &e,
                &mut backend,
                &[stand_in(Js8Speed::Normal)],
                start + key_ms,
                slot,
            );
            assert!(!backend.played.is_empty(), "premise: played");
            assert_eq!(
                backend.unit_ends,
                vec![backend.played.len()],
                "key at {key_ms} ms"
            );
        }
    }

    /// The PTT hold is bounded by the end of the frame's OWN period: a frame keyed in its TX delay
    /// lies before that period, so a bound taken from the period its key is in would drop PTT at the
    /// frame's very start.
    #[test]
    fn a_frame_keyed_before_its_period_is_held_into_that_period() {
        for speed in Js8Speed::ALL {
            let e = js8_engine(speed);
            let slot = 1_000_000;
            let start = frame_start_ms(&e, slot);
            let period_ms = f64::from(speed.period_s()) * 1000.0;
            let mut backend = MockBackend::new();
            let over = play_frame(&e, &mut backend, &[stand_in(speed)], start - 200.0, slot);
            let end = start + f64::from(speed.delay_ms()) + 79.0 * speed.nsps() as f64 / 12.0;
            assert!(
                (over.tx_until_ms - (end + TX_TAIL_MS)).abs() < 1e-6,
                "{speed:?}: held to {} ms into the period, expected {}",
                over.tx_until_ms - start,
                end + TX_TAIL_MS - start
            );
            assert!(
                over.tx_until_ms < start + period_ms,
                "{speed:?}: inside its own period"
            );
        }
    }

    /// A rigctld that answers the key (`T 1`) after `key_ms` and everything else at once, and logs
    /// each line it gets.
    fn rigctld(key_ms: u64) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log_w = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut w = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let t = line.trim().to_string();
                    if t.is_empty() {
                        continue;
                    }
                    log_w.lock().unwrap().push(t.clone());
                    if t == "T 1" {
                        std::thread::sleep(std::time::Duration::from_millis(key_ms));
                    }
                    if w.write_all(b"RPRT 0\n").is_err() {
                        return;
                    }
                }
            }
        });
        (addr, log)
    }

    /// The steered clock (`service.rs` subtracts the measured PC-clock offset), as the loop reads it.
    fn steered_now_ms(eng: &Engine) -> f64 {
        now_unix_ms() - eng.clock_offset_ms().unwrap_or(0) as f64
    }

    /// Steer `eng`'s clock so that it reads `into_ms` into a period now (before its start for a
    /// negative `into_ms`), and say that period's slot.
    fn steer_into_period(eng: &mut Engine, into_ms: f64) -> u64 {
        let period_ms = eng.active_slot_secs() * 1000.0;
        let phase = now_unix_ms().rem_euclid(period_ms);
        let offset = (phase - into_ms).round() as i64;
        eng.set_clock_offset_ms(Some(offset));
        assert_eq!(eng.clock_offset_ms(), Some(offset), "premise: steered");
        ((steered_now_ms(eng) - into_ms) / period_ms).round() as u64
    }

    /// A JS8 station at Normal, TX on, with a message queued.
    fn sending_engine() -> Engine {
        sending_engine_at(Js8Speed::Normal)
    }

    /// …at `speed`.
    fn sending_engine_at(speed: Js8Speed) -> Engine {
        let mut e = Engine::with_settings(tempo_app::settings::Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            ..Default::default()
        });
        e.set_license_class("extra");
        e.js8_enter();
        e.js8_set_speed(speed.index()).unwrap();
        e.set_frequency(14.078, "20m", "USB");
        e.set_tx_enabled(true);
        e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
            .unwrap();
        e
    }

    /// The TX phase for `slot` on `eng`'s steered clock as it reads now, on a rigctld that answers
    /// the key after `key_ms`: the action, what played, the lines on the wire, and the steered
    /// clock read just before and just after.
    fn tx_phase(
        eng: &mut Engine,
        slot: u64,
        key_ms: u64,
        prebuilt: Option<Vec<Vec<f32>>>,
    ) -> (crate::slot::SlotAction, MockBackend, Vec<String>, f64, f64) {
        tx_phase_held(eng, slot, key_ms, 0, prebuilt, false)
    }

    /// The mock sound card, taking `open_ms` to open its TX stream (`prepare_tx`, just before the
    /// key is sent): a key held up, as a slow CAT link holds one up, for as long as a test needs.
    struct SlowCard {
        card: MockBackend,
        open_ms: u64,
    }

    impl AudioBackend for SlowCard {
        fn capture(&mut self) -> Vec<f32> {
            self.card.capture()
        }
        fn play(&mut self, samples: &[f32]) {
            self.card.play(samples);
        }
        fn prepare_tx(&mut self) -> Result<(), String> {
            std::thread::sleep(std::time::Duration::from_millis(self.open_ms));
            self.card.prepare_tx()
        }
        fn flush_output(&mut self) -> usize {
            self.card.flush_output()
        }
    }

    /// [`tx_phase`] with the loop already holding the transmitter (`held`) or not, on a sound card
    /// that takes `open_ms` to open ([`SlowCard`]).
    fn tx_phase_held(
        eng: &mut Engine,
        slot: u64,
        key_ms: u64,
        open_ms: u64,
        prebuilt: Option<Vec<Vec<f32>>>,
        held: bool,
    ) -> (crate::slot::SlotAction, MockBackend, Vec<String>, f64, f64) {
        let (addr, log) = rigctld(key_ms);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = SlowCard {
            card: MockBackend::new(),
            open_ms,
        };
        let mut rx = crate::frames::RxRing::new();
        let before = steered_now_ms(eng);
        let act = crate::slot::slot_tx_phase(
            eng,
            &mut rig,
            &mut backend,
            &mut rx,
            slot,
            before,
            false,
            None,
            prebuilt,
            held,
        );
        let after = steered_now_ms(eng);
        let sent = log.lock().unwrap().clone();
        (act, backend.card, sent, before, after)
    }

    /// ⭐ A KEY THE RADIO ANSWERS PAST JS8CALL'S LATE START, INSIDE ITS PERIOD, PLAYS THE REST, as
    /// JS8Call's Modulator skips the symbols already gone however late the radio answers before the
    /// frame's last tone (`Modulator.cpp:102-107`). Decided 1.8 s into a Normal period (under its
    /// 2.16 s late start), the key answered 500 ms later, 2.3 s in: the first sample played is the
    /// frame's sample at the key's place in the period and the rest follows sample for sample, PTT
    /// is held to the frame's end on the slot clock and its tail (13.39 s), TX stays on, and
    /// nothing is flushed. Control: the same key answered at once plays the same way from 1.8 s.
    /// Nexus unkeyed the late key and sent nothing of the frame.
    #[test]
    fn a_key_answered_past_the_late_start_inside_its_period_plays_the_rest() {
        for key_ms in [500, 0] {
            let mut e = sending_engine();
            let slot = steer_into_period(&mut e, 1_800.0);
            let frame = stand_in(Js8Speed::Normal);
            let (act, backend, sent, before, after) =
                tx_phase(&mut e, slot, key_ms, Some(vec![frame.clone()]));
            let start = frame_start_ms(&e, slot);
            // The key was answered after the rigctld's wait and before the clock read after the
            // call.
            let key_from = before + key_ms as f64 - start;
            assert!(
                before - start < 2_160.0,
                "premise: decided under the late start, {} ms in",
                before - start
            );
            assert!(
                key_ms == 0 || key_from >= 2_160.0,
                "premise: answered past it, {key_from} ms in"
            );
            assert_eq!(sent, ["T 1"], "{key_ms} ms: not keyed, or unkeyed");
            let played = &backend.played;
            let first = frame.len() - played.len();
            let (lo, hi) = (
                (key_from * 12.0).floor() as usize,
                ((after - start) * 12.0).ceil() as usize,
            );
            assert!(
                (lo..=hi).contains(&first),
                "{key_ms} ms: the first sample played is the frame's sample {first}, expected the \
                 key's place in the period, {lo}..={hi}"
            );
            assert!(
                played[..] == frame[first..],
                "{key_ms} ms: not the rest of the frame"
            );
            let end = start + 500.0 + 79.0 * 1_920.0 / 12.0 + TX_TAIL_MS;
            let until = act.tx_until_ms.expect("no over held");
            assert!(
                (until - end).abs() < 0.1,
                "{key_ms} ms: PTT held to {} ms into the period, expected {}",
                until - start,
                end - start
            );
            assert!(
                act.tx_this_slot && e.tx_enabled(),
                "{key_ms} ms: TX went off"
            );
            assert_eq!(
                backend.flush_calls, 0,
                "{key_ms} ms: the output was flushed"
            );
            assert_eq!(e.js8_state().last_error, None, "{key_ms} ms");
        }
    }

    /// ⭐ A KEY THE RADIO ANSWERS AT OR AFTER ITS PERIOD'S END SENDS NOTHING, AND TX STAYS ON: by
    /// then nothing of the frame is left on its period's clock. A JS8 60 frame (4 s periods)
    /// decided 0.2 s into its period, under its 0.636 s late start, on a sound card that takes 4 s
    /// to open: the key is answered 4.2 s in, after the period has ended, so the radio is unkeyed
    /// at once, nothing plays, no over is held, and the cockpit says why. The next period keys as
    /// ever: the next frame of the message in its TX delay, held to its end on the slot clock.
    #[test]
    fn a_key_answered_after_its_period_has_ended_sends_nothing_and_the_next_period_keys() {
        let mut e = sending_engine_at(Js8Speed::Ultra);
        let slot = steer_into_period(&mut e, 200.0);
        let (act, backend, sent, before, _) = tx_phase_held(
            &mut e,
            slot,
            0,
            4_000,
            Some(vec![stand_in(Js8Speed::Ultra)]),
            false,
        );
        let start = frame_start_ms(&e, slot);
        assert!(
            (0.0..636.0).contains(&(before - start)),
            "premise: decided under the late start, {} ms in",
            before - start
        );
        assert_eq!(sent, ["T 1", "T 0"], "keyed, then unkeyed at once");
        assert!(
            backend.played.is_empty(),
            "{} samples played",
            backend.played.len()
        );
        assert!(
            act.tx_until_ms.is_none() && !act.tx_this_slot,
            "an over held"
        );
        assert!(e.tx_enabled(), "TX went off");
        let line = e.js8_state().last_error.expect("the cockpit says why");
        assert!(
            line.starts_with("JS8 frame not sent: the radio keyed 4.")
                && line.contains("after its last tone had ended (2.6 s in)"),
            "{line}"
        );
        let queued = e.js8_state().queue.len();
        let next = steer_into_period(&mut e, -200.0);
        let (act, _, sent, _, _) = tx_phase(&mut e, next, 0, None);
        let start = frame_start_ms(&e, next);
        let end = start + 100.0 + 79.0 * 384.0 / 12.0 + TX_TAIL_MS;
        let until = act.tx_until_ms.expect("the next period held no over");
        assert_eq!(sent, ["T 1"], "the next period did not key");
        assert!(
            act.tx_this_slot && (until - end).abs() < 0.1,
            "the next frame is held to {} ms into its period, expected {}",
            until - start,
            end - start
        );
        assert_eq!(e.js8_state().queue.len(), queued - 1, "the next frame went");
    }

    /// A late key while the loop already holds the transmitter (the operator's own PTT, or Hold PTT
    /// between frames) keeps the same rule. Before the frame's last tone the rest plays: answered
    /// 2.3 s into a Normal period, the frame from its place on the slot clock, the key left up.
    /// After the last tone nothing plays and the cockpit says why, but the key is left where it
    /// was, the holder's to end, as an FT8/FT4 over refused past its period leaves it: a JS8 60
    /// frame answered 4.2 s into its 4 s period.
    #[test]
    fn a_late_key_over_a_held_transmitter_plays_the_rest_and_after_its_period_leaves_it_held() {
        let mut e = sending_engine();
        let slot = steer_into_period(&mut e, 1_800.0);
        let frame = stand_in(Js8Speed::Normal);
        let (act, backend, sent, before, after) =
            tx_phase_held(&mut e, slot, 500, 0, Some(vec![frame.clone()]), true);
        let start = frame_start_ms(&e, slot);
        assert!(
            before - start < 2_160.0 && before + 500.0 - start >= 2_160.0,
            "premise: decided under the late start and answered past it, from {} ms in",
            before - start
        );
        assert_eq!(sent, ["T 1"], "inside the period");
        let first = frame.len() - backend.played.len();
        let (lo, hi) = (
            ((before + 500.0 - start) * 12.0).floor() as usize,
            ((after - start) * 12.0).ceil() as usize,
        );
        assert!(
            (lo..=hi).contains(&first) && backend.played[..] == frame[first..],
            "held: the frame from its sample {first}, expected the rest from {lo}..={hi}"
        );
        assert!(act.tx_this_slot && e.js8_state().last_error.is_none());

        let mut e = sending_engine_at(Js8Speed::Ultra);
        let slot = steer_into_period(&mut e, 200.0);
        let (act, backend, sent, before, _) = tx_phase_held(
            &mut e,
            slot,
            0,
            4_000,
            Some(vec![stand_in(Js8Speed::Ultra)]),
            true,
        );
        assert!(
            before - frame_start_ms(&e, slot) < 636.0,
            "premise: decided under the late start"
        );
        assert_eq!(sent, ["T 1"], "after the period: the held key was let go");
        assert!(
            backend.played.is_empty() && !act.tx_this_slot && act.tx_until_ms.is_none(),
            "after the period: {} samples played",
            backend.played.len()
        );
        assert!(e.tx_enabled(), "TX went off");
        let line = e.js8_state().last_error.expect("the cockpit says why");
        assert!(line.starts_with("JS8 frame not sent"), "{line}");
    }

    /// ⭐ A FRAME DECIDED PAST THE LATE START KEYS NOTHING AND STAYS QUEUED, as JS8Call's frame waits
    /// for the next period. Decided 2.3 s or 12 s (80 %) into a Normal period, nothing goes on the
    /// wire and nothing is taken from the queue; decided 1.0 s in (control), the frame keys and
    /// leaves the queue. (A boundary 80 % in used to key and play the whole frame from the key.)
    #[test]
    fn a_frame_decided_past_the_late_start_keys_nothing_and_stays_queued() {
        for (into_ms, keys) in [(2_300.0, false), (12_000.0, false), (1_000.0, true)] {
            let mut e = sending_engine();
            let queued = e.js8_state().queue.len();
            let slot = steer_into_period(&mut e, into_ms);
            let (act, backend, sent, _, _) = tx_phase(&mut e, slot, 0, None);
            if keys {
                assert_eq!(sent, ["T 1"], "{into_ms} ms in");
                assert!(act.tx_this_slot && !backend.played.is_empty());
                assert_eq!(e.js8_state().queue.len(), queued - 1, "the frame went");
            } else {
                assert!(sent.is_empty(), "{into_ms} ms in: {sent:?}");
                assert!(!act.tx_this_slot && backend.played.is_empty());
                assert_eq!(e.js8_state().queue.len(), queued, "the frame waits");
            }
        }
    }

    /// The first tone through the TX phase on the steered clock, for keys 0.2 s and 0.05 s before
    /// the period and 0.1 s and 0.6 s into it, the key answered at once: what plays is the frame's
    /// silence up to the period, or the frame from its place in it, by however far the clock had got
    /// when the key was answered (between the reads before and after the call).
    #[test]
    fn through_the_tx_phase_the_frame_is_on_the_steered_slot_clock() {
        for into_ms in [-200.0, -50.0, 100.0, 600.0] {
            let mut e = sending_engine();
            let slot = steer_into_period(&mut e, into_ms);
            let frame = stand_in(Js8Speed::Normal);
            let (act, backend, sent, before, after) =
                tx_phase(&mut e, slot, 0, Some(vec![frame.clone()]));
            let start = frame_start_ms(&e, slot);
            assert_eq!(sent, ["T 1"], "{into_ms} ms");
            assert!(act.tx_this_slot);
            let played = &backend.played;
            // The played stream, read on the slot clock: where its first sample sits in the frame.
            let offset = frame.len() as i64 - played.len() as i64;
            let (lo, hi) = (
                ((before - start) * 12.0).floor() as i64,
                ((after - start) * 12.0).ceil() as i64,
            );
            assert!(
                (lo..=hi).contains(&offset),
                "{into_ms} ms: the frame starts {offset} samples into the period, expected {lo}..={hi}"
            );
            if offset < 0 {
                let silence = (-offset) as usize;
                assert!(played[..silence].iter().all(|&x| x == 0.0), "{into_ms} ms");
                assert!(
                    played[silence..] == frame[..],
                    "{into_ms} ms: the whole frame"
                );
            } else {
                assert!(
                    played[..] == frame[offset as usize..],
                    "{into_ms} ms: the rest"
                );
            }
        }
    }

    /// The frame's last tone, by value at every speed (its start delay and 79 symbols, Normal
    /// 13.14 s): a key answered 1 ms before it plays the rest, the last 12 samples; one at it sends
    /// nothing and says how far into the period it came and where the last tone ended, and so do
    /// one too close to it to leave a whole sample and one at the period's end. A key at JS8Call's
    /// late start, or in the TX delay before the period, plays. Every other tier: never late here.
    #[test]
    fn a_key_at_its_frames_last_tone_is_late_and_one_just_before_it_is_not() {
        let slot = 1_000_000;
        for speed in Js8Speed::ALL {
            let e = js8_engine(speed);
            let start = frame_start_ms(&e, slot);
            let end = f64::from(speed.delay_ms()) + 79.0 * speed.nsps() as f64 / 12.0;
            let period_ms = f64::from(speed.period_s()) * 1000.0;
            assert_eq!(
                late_key(&e, start + end - 1.0, slot),
                None,
                "{speed:?}: 1 ms before the last tone's end"
            );
            let frame = stand_in(speed);
            let mut backend = MockBackend::new();
            play_frame(
                &e,
                &mut backend,
                std::slice::from_ref(&frame),
                start + end - 1.0,
                slot,
            );
            assert!(
                backend.played[..] == frame[frame.len() - 12..],
                "{speed:?}: 1 ms before the last tone's end, {} samples played",
                backend.played.len()
            );
            assert_eq!(
                late_key(&e, start + end, slot),
                Some((end, end)),
                "{speed:?}: at the last tone's end"
            );
            assert!(
                late_key(&e, start + end - 0.03, slot).is_some(),
                "{speed:?}: 0.03 ms before the last tone's end, no whole sample left to play"
            );
            assert_eq!(
                late_key(&e, start + period_ms, slot),
                Some((period_ms, end)),
                "{speed:?}: at the period's end"
            );
            assert_eq!(
                late_key(&e, start + e.js8_late_start_ms(), slot),
                None,
                "{speed:?}: at JS8Call's late start"
            );
            assert_eq!(
                late_key(&e, start - 200.0, slot),
                None,
                "{speed:?}: in the TX delay"
            );
        }
        let mut ft8 = Engine::new("KD9TAW", "EN52", 0);
        ft8.set_tier(Tier::Ft8);
        let start = frame_start_ms(&ft8, slot);
        assert_eq!(
            late_key(&ft8, start + 20_000.0, slot),
            None,
            "FT8: not here"
        );
    }

    /// ⭐ A KEY THE RADIO ANSWERS AFTER THE FRAME'S LAST TONE SENDS NOTHING, EVEN INSIDE ITS PERIOD,
    /// as JS8Call's GUI tick stops TX once its period passes the frame's length
    /// (`mainwindow.cpp:2575-2576`, `2681-2682`). A JS8 60 frame (its last tone 2.628 s into its 4
    /// s period) decided 0.2 s in, on a sound card that takes 2.8 s to open: the key is answered
    /// about 3.0 s in, inside the period but with none of the frame left, so the radio is unkeyed
    /// at once, nothing plays, no over is held, TX stays on, and the cockpit says why. Nexus keyed
    /// it, played nothing, held PTT for the tail, and said nothing.
    #[test]
    fn a_key_answered_after_the_frames_last_tone_sends_nothing_inside_its_period_too() {
        let mut e = sending_engine_at(Js8Speed::Ultra);
        let slot = steer_into_period(&mut e, 200.0);
        let (act, backend, sent, before, after) = tx_phase_held(
            &mut e,
            slot,
            0,
            2_800,
            Some(vec![stand_in(Js8Speed::Ultra)]),
            false,
        );
        let start = frame_start_ms(&e, slot);
        assert!(
            (0.0..636.0).contains(&(before - start)) && after - start < 4_000.0,
            "premise: decided under the late start, {} ms in, and answered inside the period, \
             before {} ms in",
            before - start,
            after - start
        );
        assert_eq!(sent, ["T 1", "T 0"], "keyed, then unkeyed at once");
        assert!(
            backend.played.is_empty(),
            "{} samples played",
            backend.played.len()
        );
        assert!(
            act.tx_until_ms.is_none() && !act.tx_this_slot,
            "an over held"
        );
        assert!(e.tx_enabled(), "TX went off");
        let line = e.js8_state().last_error.expect("the cockpit says why");
        assert!(
            line.starts_with("JS8 frame not sent: the radio keyed 3.")
                && line.contains("after its last tone had ended (2.6 s in)"),
            "{line}"
        );
    }
}
