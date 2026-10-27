//! FT8 and FT4 overs on WSJT-X's window, with their tones on the slot clock, on the real loop.
//!
//! WSJT-X (upstream master, checked line by line) keys from the clock and ends an over on it: the
//! over is on the air while the slot position is under `txDuration` (FT8 13.64 s, FT4 6.04 s;
//! `helper_functions.cpp:6-7`, `widgets/mainwindow.cpp:5012-5015, 5055`), the GUI tick past it stops
//! the audio (`:5180`, `:5530`, `stopTx` `:5743-5753`), and PTT drops 200 ms later (`ptt0Timer` →
//! `stopTx2`, `:5753-5760`). Its Modulator lays the tones on the clock: silence up to their nominal
//! start, the symbols a late start has passed skipped (`Modulator/Modulator.cpp:56-57, 85-101`).
//! Nexus ended an over at key + wave + 250 ms and played its tones from the key, so keying latency
//! moved the whole over late and cut its end at the boundary.
//!
//! The loop is the real `RadioLoop::step`, on a rigctld that keys and a mock sound card. Every
//! scene runs at a synthetic time 30 s or more ahead of the real clock: the key re-reads the clock
//! and keeps the later of the two (`slot::slot_tx_phase`), so each key lands at exactly the phase
//! the scene ticks it at.

use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};

/// The loop on a rigctld that keys, for an engine with a CQ to send at its next own boundary.
struct Over {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    t: f64,
    /// The wall clock when the rigctld first received the key (`T 1`).
    key_at: Arc<Mutex<Option<f64>>>,
}

impl Over {
    /// `tier`, armed (TX on, as Call CQ arms it) with nothing sent yet; the CQ is queued by
    /// [`Self::cq`]. The loop's clock starts on an even (this station's) boundary of `tier`, at
    /// least 30 s ahead of the real clock.
    fn new(tier: Tier) -> Over {
        let key_at = Arc::new(Mutex::new(None));
        let noted = Arc::clone(&key_at);
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| {
            (line == "T 1").then(|| {
                noted.lock().unwrap().get_or_insert(now_unix_ms());
                state.keyed = true;
                "RPRT 0\n".to_string()
            })
        });
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        let period_ms = {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_tier(tier);
            e.set_frequency(14.074, "20m", "USB");
            e.active_slot_secs() * 1000.0
        };
        let pair = 2.0 * period_ms;
        let t = ((now_unix_ms() + 30_000.0) / pair).ceil() * pair;
        Over {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            t,
            key_at,
        }
    }

    /// Queue a CQ for the next own boundary (Call CQ's arm, its snappy first over drained).
    fn cq(&self) {
        let mut e = self.engine.lock().unwrap();
        e.broadcast("CQ KD9TAW EN52");
        let _ = e.take_immediate_tx();
    }

    /// Queue a CQ that keys at once if it can (Call CQ inside this station's own slot).
    fn cq_now(&self) {
        self.engine.lock().unwrap().broadcast("CQ KD9TAW EN52");
    }

    /// Call CQ inside this station's own slot: a CQ run, whose first over keys at once if it can
    /// and which plans its CQ again at every own boundary while TX is on.
    fn call_cq(&self) {
        self.engine
            .lock()
            .unwrap()
            .start_cq(None)
            .expect("premise: Call CQ");
    }

    /// Run the loop, a 20 ms tick at a time, to `until` ms (inclusive).
    fn run_to(&mut self, until: f64) {
        step_to(
            &self.engine,
            &mut self.state,
            &mut self.backend,
            &mut self.rig,
            &mut self.t,
            until,
        );
    }

    /// How many times `line` went on the wire.
    fn sent(&self, line: &str) -> usize {
        writes(&self.peer).iter().filter(|l| *l == line).count()
    }
}

/// FT8's slot-positioned wave, in samples: 0.5 s of lead-in and 79 symbols of 1920 (12.64 s).
const FT8_WAVE: usize = 6_000 + 79 * 1_920;

/// `got` is exactly `want`, or why not in a few words: an over is 150 000 samples, too many to print.
fn same_audio(got: &[f32], want: &[f32]) -> Result<(), String> {
    if got.len() != want.len() {
        return Err(format!("{} samples, expected {}", got.len(), want.len()));
    }
    match got.iter().zip(want).position(|(a, b)| a != b) {
        Some(i) => Err(format!("differs from sample {i}")),
        None => Ok(()),
    }
}

/// ⭐ AN ON-TIME FT8 OVER: TONES FROM 0.5 s, AUDIO STOPPED AT 13.64 s, PTT OFF AT 13.84 s, AS WSJT-X.
/// The over used to drop PTT at key + 13.14 s + 250 ms = 13.39 s, 0.45 s short of WSJT-X, with
/// 250 ms of room for the output's latency against WSJT-X's 500 ms.
#[test]
fn an_on_time_ft8_over_stops_its_audio_and_drops_ptt_at_wsjtxs_times() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0);
    assert_eq!(s.sent("T 1"), 1, "premise: the boundary keyed");
    assert_eq!(
        s.backend.played.len(),
        FT8_WAVE,
        "an on-time over plays its whole wave: the lead-in puts the tones at 0.5 s"
    );
    assert_eq!(s.state.tx_until_ms, Some(t0 + 13_840.0), "PTT off");
    let flushes = s.backend.flush_calls;
    s.run_to(t0 + 13_620.0);
    assert_eq!(s.backend.flush_calls, flushes, "the audio stopped early");
    s.run_to(t0 + 13_640.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the audio did not stop at 13.64 s"
    );
    assert_eq!(s.sent("T 0"), 0, "PTT dropped with the audio");
    s.run_to(t0 + 13_820.0);
    assert_eq!(s.sent("T 0"), 0, "PTT dropped before 13.84 s");
    s.run_to(t0 + 13_840.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 13.84 s");
    assert_eq!(s.state.tx_until_ms, None);
}

/// ⭐ FT4 THE SAME WAY: AUDIO STOPPED AT 6.04 s, PTT OFF AT 6.24 s (WSJT-X's `txDuration` 6.04 s and
/// its 200 ms). The over used to hold PTT to key + 6.548 s + 250 ms = 6.80 s. The 0.5 s lead-in is
/// Nexus's own (WSJT-X's Modulator starts FT4 at 0.3 s; `crates/modes/src/mode.rs` says why) and is
/// unchanged: the tones still start at 0.5 s, and end at 5.54 s.
#[test]
fn an_on_time_ft4_over_stops_its_audio_and_drops_ptt_at_wsjtxs_times() {
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.cq();
    s.run_to(t0);
    assert_eq!(s.sent("T 1"), 1, "premise: the boundary keyed");
    let played = &s.backend.played;
    assert!(
        played[..6_000].iter().all(|&x| x == 0.0) && played[6_000..6_100].iter().any(|&x| x != 0.0),
        "FT4's tones do not start 0.5 s into the slot, as before"
    );
    assert_eq!(s.state.tx_until_ms, Some(t0 + 6_240.0), "PTT off");
    let flushes = s.backend.flush_calls;
    s.run_to(t0 + 6_020.0);
    assert_eq!(s.backend.flush_calls, flushes, "the audio stopped early");
    s.run_to(t0 + 6_040.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the audio did not stop at 6.04 s"
    );
    s.run_to(t0 + 6_220.0);
    assert_eq!(s.sent("T 0"), 0, "PTT dropped before 6.24 s");
    s.run_to(t0 + 6_240.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 6.24 s");
}

/// ⭐ A LATE KEY LOSES ITS FIRST SYMBOLS, NOT ITS LAST: THE TONES STAY ON THE SLOT CLOCK AND THE OVER
/// ENDS WHERE AN ON-TIME ONE DOES. A boundary keyed 2 s late (a slow decode, a CAT read in the
/// boundary tick) used to play the whole over from the key: every tone 2 s late (DT +2) and the end
/// cut at the boundary. Now it plays from where the slot is, as WSJT-X's Modulator does.
#[test]
fn a_late_ft8_key_skips_its_first_symbols_and_ends_on_time() {
    let mut on_time = Over::new(Tier::Ft8);
    let t0 = on_time.t;
    on_time.cq();
    on_time.run_to(t0);
    assert_eq!(
        on_time.backend.played.len(),
        FT8_WAVE,
        "premise: the control"
    );

    let mut late = Over::new(Tier::Ft8);
    let t0 = late.t;
    late.cq();
    late.t = t0 + 2_000.0; // the loop's first tick in the slot, 2 s in: the boundary keys late
    late.run_to(t0 + 2_000.0);
    assert_eq!(late.sent("T 1"), 1, "premise: the late boundary keyed");
    let skipped = 24_000; // 2.000 s at 12 kHz: the 0.5 s lead-in and the first 9.375 symbols
    if let Err(why) = same_audio(&late.backend.played, &on_time.backend.played[skipped..]) {
        panic!("the late over is not the on-time over's audio from 2.0 s on: {why}");
    }
    assert_eq!(
        late.state.tx_until_ms,
        Some(t0 + 13_840.0),
        "a late key moved the end of the over"
    );
    late.run_to(t0 + 13_640.0);
    late.run_to(t0 + 13_840.0);
    assert_eq!(late.sent("T 0"), 1, "PTT did not drop at 13.84 s");
}

/// ⭐ A CALL CQ INSIDE THIS STATION'S OWN SLOT (THE LATE START) IS ON THE SLOT CLOCK TOO. It used to
/// trim the wave so its END met the boundary: a CQ at 3 s played from 1.14 s of the wave (tones
/// 1.86 s late) and held PTT to 15.25 s, 250 ms into the other station's slot. Now it plays from 3 s
/// of the wave, ends with the window, and still keys at once.
#[test]
fn a_late_start_ft8_cq_plays_on_the_slot_clock_and_ends_with_the_window() {
    let mut on_time = Over::new(Tier::Ft8);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.run_to(t0 + 2_980.0);
    assert_eq!(s.sent("T 1"), 0, "premise: nothing to send at the boundary");
    s.cq_now();
    s.run_to(t0 + 3_000.0);
    assert_eq!(s.sent("T 1"), 1, "the late start did not key at once");
    if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[36_000..]) {
        panic!("the late start is not the on-time over's audio from 3.0 s on: {why}");
    }
    assert_eq!(s.state.tx_until_ms, Some(t0 + 13_840.0), "PTT off");
}

// ── How late a late key may be: WSJT-X's 75 % of the period ────────────────────────────────────
// WSJT-X asserts PTT only while the slot is under 75 % of its period (`widgets/mainwindow.cpp:5114`,
// `:5126`, `fTR < 0.75`, "Allow late starts"): FT8 to 11.25 s, FT4 to 5.625 s, the symbols that remain
// sent on the clock. Later, the over waits for the next own slot. Nexus stopped a late start at its
// own budget (6.0 s of FT8's leading audio, 3.0 s of FT4's) and keyed a late boundary as long as any
// tone remained.

/// ⭐ A CALL CQ INSIDE THIS STATION'S OWN SLOT KEYS AT ONCE UNTIL 75 % OF IT, AS WSJT-X'S DOES: 6.5 s
/// and 11.2 s into an FT8 slot, the symbols that remain on the slot clock and the over ended with
/// the window. Both waited for the next own slot under the old budget.
#[test]
fn a_late_start_ft8_cq_keys_at_once_until_75_percent_of_the_slot() {
    let mut on_time = Over::new(Tier::Ft8);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut waited = Vec::new();
    for at_ms in [6_500.0, 11_200.0] {
        let mut s = Over::new(Tier::Ft8);
        let t0 = s.t;
        s.run_to(t0 + at_ms - 20.0);
        s.cq_now();
        s.run_to(t0 + at_ms);
        if s.sent("T 1") != 1 {
            waited.push(at_ms);
            continue;
        }
        let skipped = (at_ms * 12.0) as usize;
        if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[skipped..]) {
            panic!("the CQ {at_ms} ms in is not the on-time over's audio from there on: {why}");
        }
        assert_eq!(
            s.state.tx_until_ms,
            Some(t0 + 13_840.0),
            "{at_ms} ms in: PTT off"
        );
    }
    assert!(
        waited.is_empty(),
        "a CQ this many ms into its own slot did not key at once: {waited:?}"
    );
}

/// AND FROM 75 % IT WAITS FOR THIS STATION'S NEXT SLOT, AS WSJT-X'S DOES: a CQ 11.3 s into an FT8
/// slot keys nothing in it, and goes out whole, on time, at the next own boundary.
#[test]
fn a_late_start_past_75_percent_waits_for_the_next_own_slot() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.run_to(t0 + 11_280.0);
    s.cq_now();
    s.run_to(t0 + 29_980.0);
    assert_eq!(s.sent("T 1"), 0, "a CQ 11.3 s in keyed this slot");
    s.run_to(t0 + 30_000.0);
    assert_eq!(
        s.sent("T 1"),
        1,
        "and it keys at the next own boundary, on time"
    );
    assert_eq!(s.backend.played.len(), FT8_WAVE);
}

/// FT4 THE SAME WAY: 75 % of its 7.5 s slot is 5.625 s. A CQ 5.5 s in keys at once with the 40 ms of
/// tones still ahead of the clock (FT4's 0.5 s lead-in puts its last tone at 5.54 s) and ends with
/// the window; 5.7 s in, it waits for the next own slot. The old budget stopped FT4 at 3.0 s.
#[test]
fn an_ft4_late_start_keys_until_75_percent_and_later_waits() {
    let mut on_time = Over::new(Tier::Ft4);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_480.0);
    s.cq_now();
    s.run_to(t0 + 5_500.0);
    assert_eq!(s.sent("T 1"), 1, "a CQ 5.5 s in did not key at once");
    if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[66_000..]) {
        panic!("the CQ 5.5 s in is not the on-time over's audio from there on: {why}");
    }
    assert_eq!(s.state.tx_until_ms, Some(t0 + 6_240.0), "PTT off");

    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_680.0);
    s.cq_now();
    s.run_to(t0 + 14_980.0);
    assert_eq!(s.sent("T 1"), 0, "a CQ 5.7 s in keyed this slot");
    s.run_to(t0 + 15_000.0);
    assert_eq!(s.sent("T 1"), 1, "and it keys at the next own boundary");
    assert_eq!(
        s.backend.played.len(),
        on_time.backend.played.len(),
        "and whole"
    );
}

/// ⭐ A BOUNDARY WHOSE KEY COMES PAST 75 % OF THE SLOT IS NOT KEYED, AND ITS OVER WAITS: the loop's
/// first tick in the slot is 11.3 s in (a decode that drained that late, a stalled computer), where
/// WSJT-X never asserts PTT. Nothing keys, and the CQ, never planned, goes out whole at the next own
/// boundary. It used to key and send the symbols still ahead (to 13.14 s). The control, 11.2 s in,
/// keys and sends what remains on the slot clock, ending with the window.
#[test]
fn a_boundary_keyed_past_75_percent_waits_for_the_next_own_slot() {
    let mut on_time = Over::new(Tier::Ft8);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut late = Over::new(Tier::Ft8);
    let t0 = late.t;
    late.cq();
    late.t = t0 + 11_300.0; // the loop's first tick in the slot, 11.3 s in
    late.run_to(t0 + 29_980.0);
    assert_eq!(late.sent("T 1"), 0, "a boundary keyed 11.3 s in");
    late.run_to(t0 + 30_000.0);
    assert_eq!(
        late.sent("T 1"),
        1,
        "the CQ did not go out at the next own boundary"
    );
    // The same over, whole: the CQ's first frame, which a plan at 11.3 s would have spent.
    if let Err(why) = same_audio(&late.backend.played, &on_time.backend.played) {
        panic!("the next own boundary did not send the over that waited, whole: {why}");
    }

    let mut control = Over::new(Tier::Ft8);
    let t0 = control.t;
    control.cq();
    control.t = t0 + 11_200.0;
    control.run_to(t0 + 11_200.0);
    assert_eq!(control.sent("T 1"), 1, "control: a boundary 11.2 s in");
    if let Err(why) = same_audio(&control.backend.played, &on_time.backend.played[134_400..]) {
        panic!("control: not the on-time over's audio from 11.2 s on: {why}");
    }
    assert_eq!(
        control.state.tx_until_ms,
        Some(t0 + 13_840.0),
        "control: PTT off"
    );
}

/// CONTROL: FT2 KEEPS ITS OWN LATE START. WSJT-X's 75 % is FT8's and FT4's; FT2 still keys a late start
/// only while its whole over (2.52 s) fits before the next boundary: 1.2 s into its 3.75 s slot it
/// keys at once, 1.3 s in (75 % would reach 2.81 s) it waits for the next own slot.
#[test]
fn an_ft2_late_start_keeps_its_whole_over_rule() {
    let mut s = Over::new(Tier::Ft2);
    let t0 = s.t;
    s.run_to(t0 + 1_180.0);
    s.cq_now();
    s.run_to(t0 + 1_200.0);
    assert_eq!(s.sent("T 1"), 1, "a CQ 1.2 s in did not key at once");

    let mut s = Over::new(Tier::Ft2);
    let t0 = s.t;
    s.run_to(t0 + 1_280.0);
    s.cq_now();
    s.run_to(t0 + 7_480.0);
    assert_eq!(s.sent("T 1"), 0, "a CQ 1.3 s in keyed this slot");
    s.run_to(t0 + 7_500.0);
    assert_eq!(s.sent("T 1"), 1, "and it keys at the next own boundary");
}

// ── An FT4 late start with no tone left: keyed, and held to the window ─────────────────────────────
// FT4's tones end 5.54 s into its slot (Nexus's 0.5 s lead-in), before its latest key at 5.625 s.
// WSJT-X keys a late start there all the same (`widgets/mainwindow.cpp:5126-5176`): its Modulator
// finds every symbol behind it and sends none (`Modulator/Modulator.cpp:96-99`, `:281`), and PTT
// falls only at the window's end (`widgets/mainwindow.cpp:5180`, `:5530`, `stopTx` `:5743-5753`,
// `stopTx2` `:5758-5760`), 6.24 s into the slot. Nexus keyed it and dropped PTT on the next tick.

/// ⭐ AN FT4 CALL CQ 5.58 s INTO THIS STATION'S OWN SLOT, PAST ITS LAST TONE AND UNDER 75 %: KEYED AT
/// ONCE, NOTHING PLAYED, THE AUDIO STOPPED AT 6.04 s AND PTT OFF AT 6.24 s, WSJT-X'S WINDOW. It used
/// to drop PTT on the next tick, 20 ms after the key. The controls: 5.0 s in, with tones still ahead,
/// it keys and plays them; 5.7 s in, past 75 %, it waits for the next own slot.
#[test]
fn an_ft4_late_start_with_no_tone_left_keys_and_holds_ptt_to_the_window() {
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_560.0);
    s.cq_now();
    s.run_to(t0 + 5_580.0);
    assert_eq!(s.sent("T 1"), 1, "a CQ 5.58 s in did not key at once");
    // The tick, in ms into the slot, of the first flush (the audio stop) and of the unkey.
    let (flushes, mut audio_stop, mut ptt_off) = (s.backend.flush_calls, None, None);
    while ptt_off.is_none() && s.t < t0 + 7_500.0 {
        let at = s.t;
        s.run_to(at);
        if audio_stop.is_none() && s.backend.flush_calls > flushes {
            audio_stop = Some(at - t0);
        }
        if s.sent("T 0") > 0 {
            ptt_off = Some(at - t0);
        }
    }
    assert_eq!(
        (audio_stop, ptt_off),
        (Some(6_040.0), Some(6_240.0)),
        "the audio stop and PTT off, ms into the slot"
    );
    assert!(
        s.backend.played.is_empty(),
        "{} samples played: FT4's tones end at 5.54 s",
        s.backend.played.len()
    );

    let mut on_time = Over::new(Tier::Ft4);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 4_980.0);
    s.cq_now();
    s.run_to(t0 + 5_000.0);
    assert_eq!(
        s.sent("T 1"),
        1,
        "control: a CQ 5.0 s in did not key at once"
    );
    if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[60_000..]) {
        panic!("control: the CQ 5.0 s in is not the on-time over's audio from there on: {why}");
    }
    assert!(
        s.backend.played.iter().any(|&x| x != 0.0),
        "control: 5.0 s in, no tone played"
    );
    assert_eq!(s.state.tx_until_ms, Some(t0 + 6_240.0), "control: PTT off");

    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_680.0);
    s.cq_now();
    s.run_to(t0 + 14_980.0);
    assert_eq!(s.sent("T 1"), 0, "control: a CQ 5.7 s in keyed this slot");
    s.run_to(t0 + 15_000.0);
    assert_eq!(
        s.sent("T 1"),
        1,
        "control: and it keys at the next own boundary"
    );
}

/// THE HELD WINDOW ENDS AS ANY FT4 OVER'S DOES: Stop TX unkeys at once and nothing of it fires after;
/// TX Off lets it end on the window, at 6.24 s, and keys nothing next.
#[test]
fn stop_tx_cuts_the_held_window_at_once_and_tx_off_lets_it_end_on_the_window() {
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_560.0);
    s.cq_now();
    s.run_to(t0 + 5_780.0);
    assert_eq!(
        (s.sent("T 1"), s.sent("T 0")),
        (1, 0),
        "premise: keyed 5.58 s in and held"
    );
    let flushes = s.backend.flush_calls;
    s.engine.lock().unwrap().halt_tx();
    s.run_to(t0 + 5_800.0);
    assert_eq!(s.sent("T 0"), 1, "Stop TX did not unkey on the next tick");
    assert_eq!(s.backend.flush_calls, flushes + 1, "Stop TX did not flush");
    s.run_to(t0 + 7_480.0);
    assert_eq!(
        (s.sent("T 0"), s.backend.flush_calls),
        (1, flushes + 1),
        "the cut window unkeyed or flushed later"
    );

    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_560.0);
    s.cq_now();
    s.run_to(t0 + 5_780.0);
    assert_eq!(
        (s.sent("T 1"), s.sent("T 0")),
        (1, 0),
        "premise: keyed 5.58 s in and held"
    );
    s.engine.lock().unwrap().set_tx_enabled_by_operator(false);
    s.run_to(t0 + 6_220.0);
    assert_eq!(s.sent("T 0"), 0, "TX Off cut the held window before 6.24 s");
    s.run_to(t0 + 6_240.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 6.24 s");
    s.run_to(t0 + 15_100.0);
    assert_eq!(s.sent("T 1"), 1, "TX Off keyed the next over");
}

// ── This station's own FT4 boundary decided with no tone left: keyed, and held to the window ──────
// The same rule when the slot's own key comes late (a decode that drained late, a stalled computer)
// and finds it 5.54-5.625 s into the slot: past FT4's last tone, under its latest key. WSJT-X's GUI
// tick keys it as it keys a late start (`widgets/mainwindow.cpp:5126-5176`) and holds it to the
// window's end; Nexus keyed nothing there.

/// ⭐ AN FT4 BOUNDARY DECIDED 5.58 s INTO THIS STATION'S OWN SLOT: KEYED, NOTHING PLAYED, THE AUDIO
/// STOPPED AT 6.04 s AND PTT OFF AT 6.24 s, AS FOR A CALL CQ THERE. It used to key nothing, and the
/// over it had planned was spent.
#[test]
fn an_ft4_boundary_with_no_tone_left_keys_and_holds_ptt_to_the_window() {
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.cq();
    s.t = t0 + 5_580.0; // the loop's first tick in the slot, 5.58 s in
    s.run_to(t0 + 5_580.0);
    assert_eq!(s.sent("T 1"), 1, "a boundary 5.58 s in did not key");
    // The tick, in ms into the slot, of the first flush (the audio stop) and of the unkey.
    let (flushes, mut audio_stop, mut ptt_off) = (s.backend.flush_calls, None, None);
    while ptt_off.is_none() && s.t < t0 + 7_500.0 {
        let at = s.t;
        s.run_to(at);
        if audio_stop.is_none() && s.backend.flush_calls > flushes {
            audio_stop = Some(at - t0);
        }
        if s.sent("T 0") > 0 {
            ptt_off = Some(at - t0);
        }
    }
    assert_eq!(
        (audio_stop, ptt_off),
        (Some(6_040.0), Some(6_240.0)),
        "the audio stop and PTT off, ms into the slot"
    );
    assert!(
        s.backend.played.is_empty(),
        "{} samples played: FT4's tones end at 5.54 s",
        s.backend.played.len()
    );
}

/// CONTROL: AN FT4 BOUNDARY DECIDED 5.70 s IN, PAST 75 %, STILL KEYS NOTHING: the CQ, never planned,
/// goes out whole at the next own boundary, as WSJT-X's waits.
#[test]
fn an_ft4_boundary_past_75_percent_still_waits_for_the_next_own_slot() {
    let mut on_time = Over::new(Tier::Ft4);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut late = Over::new(Tier::Ft4);
    let t0 = late.t;
    late.cq();
    late.t = t0 + 5_700.0; // the loop's first tick in the slot, 5.7 s in
    late.run_to(t0 + 14_980.0);
    assert_eq!(late.sent("T 1"), 0, "a boundary 5.7 s in keyed");
    late.run_to(t0 + 15_000.0);
    assert_eq!(
        late.sent("T 1"),
        1,
        "the CQ did not go out at the next own boundary"
    );
    if let Err(why) = same_audio(&late.backend.played, &on_time.backend.played) {
        panic!("the next own boundary did not send the over that waited, whole: {why}");
    }
}

/// STOP TX WHILE A BOUNDARY'S EMPTY OVER IS HELD UNKEYS AT ONCE, and nothing of it fires after: the
/// hold is the over's own.
#[test]
fn stop_tx_cuts_a_held_boundary_at_once() {
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.cq();
    s.t = t0 + 5_580.0;
    s.run_to(t0 + 5_780.0);
    assert_eq!(
        (s.sent("T 1"), s.sent("T 0")),
        (1, 0),
        "premise: keyed 5.58 s in and held"
    );
    let flushes = s.backend.flush_calls;
    s.engine.lock().unwrap().halt_tx();
    s.run_to(t0 + 5_800.0);
    assert_eq!(s.sent("T 0"), 1, "Stop TX did not unkey on the next tick");
    assert_eq!(s.backend.flush_calls, flushes + 1, "Stop TX did not flush");
    s.run_to(t0 + 7_480.0);
    assert_eq!(
        (s.sent("T 0"), s.backend.flush_calls),
        (1, flushes + 1),
        "the cut window unkeyed or flushed later"
    );
}

/// ⭐ STOP TX MID-OVER STILL CUTS AT ONCE, AND NOTHING OF THE CUT OVER FIRES AFTER IT. The window's
/// audio stop belongs to the over it was set for: once Stop TX has cut it, 13.64 s flushes nothing
/// and 13.84 s unkeys nothing.
#[test]
fn stop_tx_mid_over_cuts_at_once_and_the_window_does_not_fire_after() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0 + 4_980.0);
    assert_eq!(s.sent("T 1"), 1, "premise: keyed");
    let flushes = s.backend.flush_calls;
    s.engine.lock().unwrap().halt_tx();
    s.run_to(t0 + 5_000.0);
    assert_eq!(s.sent("T 0"), 1, "Stop TX did not unkey on the next tick");
    assert_eq!(s.backend.flush_calls, flushes + 1, "Stop TX did not flush");
    s.run_to(t0 + 14_000.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the cut over's window flushed later"
    );
    assert_eq!(s.sent("T 0"), 1, "the cut over's window unkeyed later");
}

/// TX OFF MID-OVER LETS THE OVER FINISH, AS IT ALWAYS DID (operator, 2026-07-31), NOW ON WSJT-X'S
/// WINDOW: the audio stops at 13.64 s and PTT drops at 13.84 s, and the next slot keys nothing.
#[test]
fn tx_off_mid_over_lets_it_finish_on_the_window() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0 + 4_980.0);
    s.engine.lock().unwrap().set_tx_enabled_by_operator(false);
    let flushes = s.backend.flush_calls;
    s.run_to(t0 + 13_620.0);
    assert_eq!(
        s.sent("T 0"),
        0,
        "PTT dropped before 13.64 s (TX Off, or a shorter hold)"
    );
    assert_eq!(
        s.backend.flush_calls, flushes,
        "the audio was cut before 13.64 s"
    );
    s.run_to(t0 + 13_640.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the audio did not stop at 13.64 s"
    );
    s.run_to(t0 + 13_840.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 13.84 s");
    s.run_to(t0 + 30_100.0);
    assert_eq!(s.sent("T 1"), 1, "TX Off keyed the next over");
}

// ── A busy decoder does not hold up the key ────────────────────────────────────────────────────
// An early decode still running at the boundary holds the modem, and building an over needed the
// modem too, so the key waited for the decode and the over went out late. WSJT-X's decoder is
// another process and never delays a key. The early pass builds the next over before it
// dispatches its decode, and a boundary whose plan asks for exactly that over keys it as built;
// and an FT8 or FT4 over is now built without the modem at all (`modes::wsjt77`), so one that
// changed after the early pass does not wait either.

/// Steer the loop's clock (and the engine's, which the loop follows) so the REAL clock reads
/// `phase_ms` into the odd slot before one of this station's even boundaries, and return that
/// boundary. The busy path commits its over against a fresh read of the real clock
/// (`commit_tx`'s slot check), so these scenes run on it rather than ahead of it.
fn steer_into_the_odd_slot(over: &mut Over, phase_ms: f64) -> f64 {
    let period = over.engine.lock().unwrap().active_slot_secs() * 1000.0;
    let pair = 2.0 * period;
    let real = now_unix_ms();
    let offset = (real.rem_euclid(pair) - (period + phase_ms)).round() as i64;
    over.engine
        .lock()
        .unwrap()
        .set_clock_offset_ms(Some(offset));
    over.state.clock_offset_ms = offset;
    ((real - offset as f64) / pair).ceil() * pair
}

/// Move the steered clock to `before_ms` short of `boundary`, within the same slot.
fn steer_to_before(over: &mut Over, boundary: f64, before_ms: f64) {
    let offset = (now_unix_ms() - (boundary - before_ms)).round() as i64;
    over.engine
        .lock()
        .unwrap()
        .set_clock_offset_ms(Some(offset));
    over.state.clock_offset_ms = offset;
}

/// Step the steered clock back when it has run past the early pass in the odd slot before
/// `boundary`: to a moment after that pass (FT8 11.808 s, FT4's only pass 6.048 s) and short of the
/// next (FT8's 47 step, 13.536 s). The busy scenes wait here for the early pass's decode to come
/// back, and every decode in this test binary waits for one process-wide modem lock: under load
/// another test's decode holds this one past the boundary, the loop keys that boundary while the
/// scene is still setting it up, and the scene's own boundary keys a second time. Held, the
/// boundary keys only in the tick the scene measures.
fn hold_after_the_early_pass(over: &mut Over, boundary: f64) {
    let (period, held_at) = {
        let e = over.engine.lock().unwrap();
        let held_at = match e.tier() {
            Tier::Ft8 => 12_000.0,
            Tier::Ft4 => 6_300.0,
            tier => panic!("no early pass to hold after on {tier:?}"),
        };
        (e.active_slot_secs() * 1000.0, held_at)
    };
    let before_ms = period - held_at;
    if now_unix_ms() - over.state.clock_offset_ms as f64 > boundary - before_ms {
        steer_to_before(over, boundary, before_ms);
    }
}

impl Over {
    /// One loop tick on the real clock; returns when the tick ended.
    fn tick_now(&mut self) -> std::time::Instant {
        self.tick_read_early(0.0)
    }

    /// One loop tick on the real clock, its clock read `early_ms` before the tick runs on (a tick
    /// that spent that long on CAT before it reached what is measured); returns when it ended.
    fn tick_read_early(&mut self, early_ms: f64) -> std::time::Instant {
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                now_unix_ms() - early_ms,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        std::time::Instant::now()
    }
}

/// A decode in flight: it holds the modem until told to let go, or `hold` passes. Returns the
/// way to tell it, and the thread, which returns when it let go.
fn a_decode_holding_the_modem(
    hold: Duration,
) -> (
    std::sync::mpsc::Sender<()>,
    std::thread::JoinHandle<std::time::Instant>,
) {
    let (holding, held) = std::sync::mpsc::channel();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let decode = std::thread::spawn(move || {
        let modem = loop {
            match tempo_core::modes::Ft8A7ResetGuard::try_acquire() {
                Some(modem) => break modem,
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        holding.send(()).unwrap();
        let _ = wait.recv_timeout(hold);
        drop(modem);
        std::time::Instant::now()
    });
    held.recv().unwrap();
    (go, decode)
}

/// ⭐ A DECODE STILL RUNNING AT THE BOUNDARY DOES NOT DELAY THE KEY OF THE OVER BUILT BEFORE IT. The
/// early pass at 11.8 s builds the CQ the boundary is about to send before it dispatches its
/// decode; that decode is still holding the modem when the slot starts; the boundary keys the CQ
/// on time, with every tone in place (0.5 s), and the decode lets go only afterwards.
///
/// The second scene is the over that was NOT built ahead (the TX frequency changed after the early
/// pass). It used to wait for the decode, as every over did; its build no longer needs the modem,
/// so it keys before the decode lets go too (the scenes of every such change, FT8 and FT4, are
/// `an_over_changed_after_the_early_pass_keys_on_time_while_a_decode_holds_the_modem`).
///
/// ⚠️ THE MODEM LOCK IS THE WHOLE PROCESS'S, so every other test's decode waits while this scene
/// holds it (one of them gives its decode a second, `in_flight_guard_serializes_dispatch`). It is
/// held for the boundary tick only: let go as soon as that tick ends, or after 500 ms at most if a
/// key ever waits for it again.
#[test]
fn a_busy_decoder_does_not_delay_the_key_of_the_over_built_before_it() {
    for built_ahead in [true, false] {
        let what = if built_ahead {
            "built ahead"
        } else {
            "not built ahead"
        };
        let mut s = Over::new(Tier::Ft8);
        s.cq();
        let boundary = steer_into_the_odd_slot(&mut s, 11_700.0);
        let slot = (boundary / 15_000.0) as u64;
        let give_up = std::time::Instant::now() + Duration::from_secs(10);
        // Until the boundary's own tick the clock is held after the early pass (`held`).
        let tick_until = |s: &mut Over, done: &dyn Fn(&Over) -> bool, why: &str, held: bool| loop {
            if held {
                hold_after_the_early_pass(s, boundary);
            }
            let ended = s.tick_now();
            if done(s) {
                break ended;
            }
            assert!(std::time::Instant::now() < give_up, "{what}: {why}");
            std::thread::sleep(Duration::from_millis(5));
        };
        s.tick_now(); // the odd slot's boundary, with nothing captured yet: no decode, no key
        for _ in 0..400 {
            s.backend.queue_capture(vec![0.0; 240]);
        }
        tick_until(&mut s, &|s| s.state.decode_in_flight, "no early pass", true);
        // The early pass dispatched its decode in that tick, and built the CQ before it.
        let hint = s
            .engine
            .lock()
            .unwrap()
            .next_over_hint(slot)
            .expect("the CQ");
        assert!(
            s.engine.lock().unwrap().prebuilt_wave(&hint).is_some(),
            "{what}: the early pass did not build the next over before its decode"
        );
        // Its decode of silence comes back at once; the one below stands in for a slow one.
        tick_until(
            &mut s,
            &|s| !s.state.decode_in_flight,
            "the early decode never came back",
            true,
        );
        if !built_ahead {
            s.engine.lock().unwrap().set_tx_offset(1_600.0);
        }
        // A decode still running at the boundary, holding the modem.
        let (go, decode) = a_decode_holding_the_modem(Duration::from_millis(500));
        s.state.decode_in_flight = true;
        steer_to_before(&mut s, boundary, 30.0);
        // The rig's own reads (health, dial, S-meter, its PTT) are held off past the boundary: the
        // clock moved forward just now, which makes them all due, and one landing in the boundary
        // tick delays the key by its own CAT round trips (hundreds of ms against this rigctld).
        // The tones on the clock absorb that too, but this scene measures the decode's hold on the
        // modem and nothing else.
        let later = boundary + 60_000.0;
        s.state.last_rig_poll = later;
        s.state.last_freq_poll = later;
        s.state.last_smeter_poll = later;
        s.state.last_ptt_poll = later;
        let keyed_tick_ended = tick_until(
            &mut s,
            &|s| s.state.last_slot == Some(slot),
            "no boundary",
            false,
        );
        let tick_end_wall = now_unix_ms();
        let _ = go.send(());
        let decode_let_go = decode.join().unwrap();
        assert_eq!(s.sent("T 1"), 1, "{what}: the boundary did not key");
        // On the slot clock whatever the key cost: what was skipped is where the slot was between
        // the key reaching the radio and the tick ending (the carrier came up in between).
        let offset = s.state.clock_offset_ms as f64;
        let key_wall = s.key_at.lock().unwrap().expect("the key's time");
        let lo = ((key_wall - offset - boundary) * 12.0).floor() as usize;
        let hi = ((tick_end_wall - offset - boundary) * 12.0).ceil() as usize;
        assert_eq!(
            s.state.tx_until_ms,
            Some(boundary + 13_840.0),
            "{what}: the over's end moved"
        );
        let skipped = FT8_WAVE - s.backend.played.len();
        assert!(
            (lo..=hi).contains(&skipped),
            "{what}: {skipped} samples skipped, expected the key's place in the slot, {lo}..={hi}"
        );
        assert!(
            keyed_tick_ended < decode_let_go,
            "{what}: the key waited for the decode to let go of the modem"
        );
        assert!(skipped < FT8_WAVE, "{what}: the over was dropped");
    }
}

/// FT4's slot-positioned wave, in samples: 0.5 s of lead-in and the 72 576-sample frame.
const FT4_WAVE: usize = 6_000 + 72_576;

/// ⭐ AN OVER CHANGED AFTER THE EARLY PASS KEYS ON TIME WHILE A DECODE HOLDS THE MODEM, ON FT8 AND
/// FT4. After the early pass (11.8 s, 5.5 s) the operator changes what the boundary sends: a new TX
/// frequency for the CQ the early pass built, or a CQ called only after it (nothing was queued when
/// it ran). Either way the boundary builds its over at the key, with a decode still holding the
/// modem. That build waited for the decode: the over went out late by however long the decode
/// overran, and on the slot clock lost that much of its start. WSJT-X's decoder is another process
/// and never delays a key. Built without the modem (`modes::wsjt77`), the over keys before the
/// decode lets go, plays from where the slot is at the key, and ends where an on-time over ends;
/// and it is the over the plan asked for, sample for sample.
#[test]
fn an_over_changed_after_the_early_pass_keys_on_time_while_a_decode_holds_the_modem() {
    for (tier, period, early_at, wave_len, end) in [
        (Tier::Ft8, 15_000.0, 11_700.0, FT8_WAVE, 13_840.0),
        (Tier::Ft4, 7_500.0, 5_400.0, FT4_WAVE, 6_240.0),
    ] {
        for called_late in [false, true] {
            let what = format!(
                "{tier:?}, {}",
                if called_late {
                    "CQ called after the early pass"
                } else {
                    "TX frequency changed after the early pass"
                }
            );
            let mut s = Over::new(tier);
            if !called_late {
                s.cq();
            }
            let boundary = steer_into_the_odd_slot(&mut s, early_at);
            let slot = (boundary / period) as u64;
            let give_up = std::time::Instant::now() + Duration::from_secs(10);
            // Until the boundary's own tick the clock is held after the early pass (`held`).
            let tick_until = |s: &mut Over, done: &dyn Fn(&Over) -> bool, why: &str, held: bool| loop {
                if held {
                    hold_after_the_early_pass(s, boundary);
                }
                let ended = s.tick_now();
                if done(s) {
                    break ended;
                }
                assert!(std::time::Instant::now() < give_up, "{what}: {why}");
                std::thread::sleep(Duration::from_millis(5));
            };
            s.tick_now(); // the odd slot's boundary, with nothing captured yet: no decode, no key
            for _ in 0..400 {
                s.backend.queue_capture(vec![0.0; 240]);
            }
            tick_until(&mut s, &|s| s.state.decode_in_flight, "no early pass", true);
            tick_until(
                &mut s,
                &|s| !s.state.decode_in_flight,
                "the early decode never came back",
                true,
            );
            if called_late {
                s.cq();
            } else {
                s.engine.lock().unwrap().set_tx_offset(1_600.0);
            }
            // What the boundary will send, built now as the plan would build it, and not built ahead.
            let over = s
                .engine
                .lock()
                .unwrap()
                .next_over_hint(slot)
                .expect("the CQ");
            assert!(
                s.engine.lock().unwrap().prebuilt_wave(&over).is_none(),
                "{what}: premise: the boundary's over was built ahead"
            );
            let want = over.build();
            // A decode still running at the boundary, holding the modem.
            let (go, decode) = a_decode_holding_the_modem(Duration::from_millis(500));
            s.state.decode_in_flight = true;
            steer_to_before(&mut s, boundary, 30.0);
            // The rig's own reads held off past the boundary, as in the scene above.
            let later = boundary + 60_000.0;
            s.state.last_rig_poll = later;
            s.state.last_freq_poll = later;
            s.state.last_smeter_poll = later;
            s.state.last_ptt_poll = later;
            let keyed_tick_ended = tick_until(
                &mut s,
                &|s| s.state.last_slot == Some(slot),
                "no boundary",
                false,
            );
            let tick_end_wall = now_unix_ms();
            let _ = go.send(());
            let decode_let_go = decode.join().unwrap();
            assert_eq!(s.sent("T 1"), 1, "{what}: the boundary did not key");
            assert!(
                keyed_tick_ended < decode_let_go,
                "{what}: the key waited for the decode to let go of the modem"
            );
            let offset = s.state.clock_offset_ms as f64;
            let key_wall = s.key_at.lock().unwrap().expect("the key's time");
            let lo = ((key_wall - offset - boundary) * 12.0).floor() as usize;
            let hi = ((tick_end_wall - offset - boundary) * 12.0).ceil() as usize;
            let skipped = wave_len - s.backend.played.len();
            assert!(
                (lo..=hi).contains(&skipped),
                "{what}: {skipped} samples skipped, expected the key's place in the slot, {lo}..={hi}"
            );
            if let Err(why) = same_audio(&s.backend.played, &want[skipped..]) {
                panic!("{what}: the over played is not the one planned, from the key on: {why}");
            }
            assert_eq!(
                s.state.tx_until_ms,
                Some(boundary + end),
                "{what}: the over's end moved"
            );
        }
    }
}

/// THE LATE START DECIDES ON THE CLOCK AT ITS DECISION, NOT ON ITS TICK'S, as WSJT-X decides on a
/// clock it has just read. A tick that read its clock 2 s before it reached the CQ (CAT earlier in
/// the tick) finds the slot 11.5 s in: it keys nothing, though its own clock said 9.5 s. At 10.5 s it
/// keys. On the real clock, steered into this station's own slot.
#[test]
fn a_late_start_decides_on_the_clock_at_the_decision_not_on_its_ticks() {
    for (at_ms, keys) in [(11_500.0, false), (10_500.0, true)] {
        let mut s = Over::new(Tier::Ft8);
        let even = steer_into_the_odd_slot(&mut s, 0.0);
        steer_to_before(&mut s, even - 15_000.0, 15_000.0 - at_ms);
        // The rig's own reads held off, as in the busy scene: the steering made them all due.
        let later = even + 60_000.0;
        s.state.last_rig_poll = later;
        s.state.last_freq_poll = later;
        s.state.last_smeter_poll = later;
        s.state.last_ptt_poll = later;
        s.tick_now(); // this slot's boundary, with nothing to send
        assert_eq!(s.sent("T 1"), 0, "premise: nothing to send at the boundary");
        s.cq_now();
        s.tick_read_early(2_000.0);
        assert_eq!(
            s.sent("T 1"),
            usize::from(keys),
            "a CQ {at_ms} ms in, its tick's clock 2 s behind"
        );
    }
}

// ── A key the radio answers past the end of its slot: nothing sent, unkeyed at once ──────────────
// The late start's own decision (75 % of the slot) is its last slot check before the key. Fake It's
// write-back, Split Operation's commands and the key come after it, and on a slow link they take
// seconds (each command up to its 2.5 s deadline). The tones are laid on the clock where the carrier
// is, so a Call CQ whose carrier came up in the other station's period sent the rest of its over
// there, on their slot clock. WSJT-X starts an over's audio only when the radio reports PTT on and
// the over is still needed (`widgets/mainwindow.cpp:8852`), and its GUI tick ends the over as soon
// as the clock has left its window (`:5055`, `:5180`, `:5530`), a window in this station's own
// period (`:5016-5019`).

/// Lines a scene's rigctld got, each with the wall clock it came at.
type Lines = Arc<Mutex<Vec<(String, f64)>>>;

/// Put the scene on a rigctld that logs every line with the wall clock it came at, in place of the
/// one `Over::new` made, on a slow link: each of Split Operation's commands (`S`, `I`, `X`) answered
/// after `split_ms`, the key after `key_ms`.
fn a_timed_rigctld(s: &mut Over, split_ms: u64, key_ms: u64) -> Lines {
    let lines: Lines = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&lines);
    s.peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| {
        noted
            .lock()
            .unwrap()
            .push((line.to_string(), now_unix_ms()));
        let stall = if ["S ", "I ", "X "].iter().any(|p| line.starts_with(p)) {
            split_ms
        } else if line == "T 1" {
            state.keyed = true;
            key_ms
        } else {
            return None;
        };
        std::thread::sleep(Duration::from_millis(stall));
        Some("RPRT 0\n".to_string())
    });
    s.rig = Rig::rigctld(&s.peer.address);
    s.rig.set_slow_transport(true);
    lines
}

/// [`a_timed_rigctld`] with the engine in Split Operation Rig and a 750 Hz TX offset, which needs a
/// dial shift: three split commands go out before every key.
fn on_a_slow_split(s: &mut Over, split_ms: u64, key_ms: u64) -> Lines {
    let lines = a_timed_rigctld(s, split_ms, key_ms);
    let mut e = s.engine.lock().unwrap();
    let mut st = e.settings().clone();
    st.split_mode = tempo_app::settings::SplitMode::Rig;
    e.apply_settings(st);
    e.set_tx_offset(750.0);
    lines
}

/// A Call CQ `at_ms` into this station's own slot of `tier`, on the real (steered) clock and a slow
/// split ([`on_a_slow_split`]).
struct SlowLateStart {
    s: Over,
    /// The over the CQ was to send, whole and slot-positioned.
    want: Vec<f32>,
    /// The split's and the key's lines, with where the slot was when each came, ms from its start.
    lines: Vec<(String, f64)>,
    /// Where the slot was when the tick that keyed it returned.
    returned_ms: f64,
    own: f64,
    period: f64,
}

fn late_start_on_a_slow_split(tier: Tier, at_ms: f64, split_ms: u64, key_ms: u64) -> SlowLateStart {
    let mut s = Over::new(tier);
    let lines = on_a_slow_split(&mut s, split_ms, key_ms);
    let period = s.engine.lock().unwrap().active_slot_secs() * 1000.0;
    let even = steer_into_the_odd_slot(&mut s, 0.0);
    let own = even - 2.0 * period;
    steer_to_before(&mut s, own + period, period - at_ms);
    // The rig's own reads held off, as in the busy scene: the steering made them all due.
    let later = even + 60_000.0;
    s.state.last_rig_poll = later;
    s.state.last_freq_poll = later;
    s.state.last_smeter_poll = later;
    s.state.last_ptt_poll = later;
    s.tick_now(); // this slot's boundary, with nothing to send
    assert_eq!(s.sent("T 1"), 0, "premise: nothing to send at the boundary");
    s.call_cq();
    let want = s
        .engine
        .lock()
        .unwrap()
        .next_over_hint((own / period) as u64)
        .expect("the CQ")
        .build();
    lines.lock().unwrap().clear();
    s.tick_now();
    let offset = s.state.clock_offset_ms as f64;
    let returned_ms = now_unix_ms() - offset - own;
    let lines = lines
        .lock()
        .unwrap()
        .iter()
        .filter(|(l, _)| ["S ", "I ", "X ", "T "].iter().any(|p| l.starts_with(p)))
        .map(|(l, t)| (l.clone(), t - offset - own))
        .collect();
    SlowLateStart {
        s,
        want,
        lines,
        returned_ms,
        own,
        period,
    }
}

impl SlowLateStart {
    fn sent(&self) -> Vec<&str> {
        self.lines.iter().map(|(l, _)| l.as_str()).collect()
    }

    /// Where in the slot `line` came, the first time.
    fn came(&self, line: &str) -> f64 {
        self.lines
            .iter()
            .find(|(l, _)| l == line)
            .unwrap_or_else(|| panic!("no {line:?} on the wire: {:?}", self.sent()))
            .1
    }
}

/// ⭐ A CALL CQ 10.0 s INTO THIS STATION'S OWN FT8 SLOT WHOSE SPLIT AND KEY TAKE 6 s: THE CARRIER COMES
/// UP 16.0 s IN, IN THE OTHER STATION'S PERIOD. Nothing is played and the radio is unkeyed in the
/// same tick, at once after the key's answer. Only that over is dropped: TX stays on, a lane of its
/// own says so, and the next own slot sends the CQ whole. The rest of the over used to go out from
/// their slot's 1.0 s on their slot clock, PTT held to their 13.84 s. The control: the same split
/// taking 2 s puts the carrier at 12.0 s, and the rest of the over goes out from there, ending on
/// this slot's window.
#[test]
fn a_late_start_ft8_cq_a_slow_split_keys_past_its_period_sends_nothing_and_is_unkeyed_at_once() {
    std::thread::scope(|s| {
        s.spawn(|| {
            let late = late_start_on_a_slow_split(Tier::Ft8, 10_000.0, 1_900, 300);
            a_slow_late_start_is_dropped_and_the_next_own_slot_sends_it_whole(
                late, 300, 13_840.0, "FT8, 6 s",
            );
        });
        s.spawn(|| {
            let control = late_start_on_a_slow_split(Tier::Ft8, 10_000.0, 600, 200);
            a_slow_late_start_plays_in_its_own_period(control, 200, 13_840.0, "FT8, 2 s");
        });
    });
}

/// FT4 THE SAME WAY: a Call CQ 2.0 s into its own slot, the split and key taking 6 s, the carrier
/// 8.0 s in, half a second into the other station's period: nothing played, unkeyed at once, only
/// that over dropped, and the next own slot sends the CQ whole. The control, 2 s: the carrier at
/// 4.0 s and the rest of the over from there, ending at 6.24 s.
#[test]
fn a_late_start_ft4_cq_a_slow_split_keys_past_its_period_sends_nothing_and_is_unkeyed_at_once() {
    std::thread::scope(|s| {
        s.spawn(|| {
            let late = late_start_on_a_slow_split(Tier::Ft4, 2_000.0, 1_900, 300);
            a_slow_late_start_is_dropped_and_the_next_own_slot_sends_it_whole(
                late, 300, 6_240.0, "FT4, 6 s",
            );
        });
        s.spawn(|| {
            let control = late_start_on_a_slow_split(Tier::Ft4, 2_000.0, 600, 200);
            a_slow_late_start_plays_in_its_own_period(control, 200, 6_240.0, "FT4, 2 s");
        });
    });
}

fn a_slow_late_start_is_dropped_and_the_next_own_slot_sends_it_whole(
    mut late: SlowLateStart,
    key_ms: u64,
    ptt_off_ms: f64,
    what: &str,
) {
    let answered = late.came("T 1") + key_ms as f64;
    assert!(
        answered >= late.period,
        "premise, {what}: the carrier came up {answered} ms into its slot"
    );
    assert!(
        late.s.backend.played.is_empty(),
        "{what}: {} samples played in the other station's period, PTT held to {:?} ms into it",
        late.s.backend.played.len(),
        late.s.state.tx_until_ms.map(|t| t - late.own - late.period)
    );
    let sent = late.sent();
    assert_eq!(
        sent[sent.len() - 2..],
        ["T 1", "T 0"],
        "{what}: the key, then the unkey in the same tick: {sent:?}"
    );
    assert!(
        late.came("T 0") - answered < 250.0,
        "{what}: the unkey came {} ms after the key's answer",
        late.came("T 0") - answered
    );
    assert_eq!(late.s.state.tx_until_ms, None, "{what}: held");
    let over_dropped = |s: &Over| {
        serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap()["radio"]
            .get("slotOverDropped")
            .cloned()
    };
    {
        let e = late.s.engine.lock().unwrap();
        assert!(e.tx_enabled(), "{what}: TX went off");
        assert_eq!(
            e.snapshot().radio.slot_key_refused,
            None,
            "{what}: on the refused key's line"
        );
    }
    let lane = over_dropped(&late.s)
        .unwrap_or_else(|| panic!("{what}: nothing says the over was dropped"));
    assert_eq!(lane["count"], 1, "{what}: {lane}");
    // ⭐ AND TX STAYS ON: the CQ run's next own slot keys again, the link answering at once now, and
    // sends the CQ whole and on time, ended on its window, as WSJT-X's next own period does with
    // Enable Tx still ticked. The lane goes with the over that went out.
    let fast = a_timed_rigctld(&mut late.s, 0, 0);
    // The new link stands in for the same radio, whose once-per-connection probes (`\dump_state`
    // twice, `u TUNER`) a station on the air ran long before: run here, at the boundary, their read
    // windows held the key up 0.66 s.
    late.s.state.rx_ranges_probed = true;
    late.s.state.tuner_probed = true;
    let next_own = late.own + 2.0 * late.period;
    steer_to_before(&mut late.s, next_own, -20.0);
    late.s.tick_now();
    let offset = late.s.state.clock_offset_ms as f64;
    let lines: Vec<(String, f64)> = fast
        .lock()
        .unwrap()
        .iter()
        .map(|(l, t)| (l.clone(), (t - offset - next_own).round()))
        .collect();
    assert_eq!(
        late.s.sent("T 1"),
        1,
        "{what}: the next own slot keyed nothing: {lines:?}"
    );
    let played = &late.s.backend.played;
    let skipped = late.want.len().saturating_sub(played.len());
    assert!(
        skipped < 6_000,
        "{what}: {} of {} samples played at the next own slot: not the whole CQ; the link got \
         (ms from the slot's start) {lines:?}",
        played.len(),
        late.want.len()
    );
    if let Err(why) = same_audio(played, &late.want[skipped..]) {
        panic!("{what}: the next own slot did not send the CQ: {why}");
    }
    assert_eq!(
        late.s.state.tx_until_ms,
        Some(next_own + ptt_off_ms),
        "{what}: not ended on the next own slot's window"
    );
    assert_eq!(
        over_dropped(&late.s),
        None,
        "{what}: the lane is still up after an over that went out"
    );
}

fn a_slow_late_start_plays_in_its_own_period(
    control: SlowLateStart,
    key_ms: u64,
    ptt_off_ms: f64,
    what: &str,
) {
    let answered = control.came("T 1") + key_ms as f64;
    assert!(
        control.returned_ms < control.period,
        "premise, {what}: the carrier came up {answered}..{} ms into its slot",
        control.returned_ms
    );
    let sent = control.sent();
    assert_eq!(
        sent.last(),
        Some(&"T 1"),
        "{what}: not the key alone: {sent:?}"
    );
    let played = &control.s.backend.played;
    let skipped = control.want.len() - played.len();
    let (lo, hi) = (
        (answered * 12.0).floor() as usize,
        (control.returned_ms * 12.0).ceil() as usize,
    );
    assert!(
        played.iter().any(|&x| x != 0.0) && (lo..=hi).contains(&skipped),
        "{what}: {skipped} samples skipped, the carrier came up {lo}..={hi} samples in"
    );
    if let Err(why) = same_audio(played, &control.want[skipped..]) {
        panic!("{what}: not the rest of the over from where the carrier came up: {why}");
    }
    assert_eq!(
        control.s.state.tx_until_ms,
        Some(control.own + ptt_off_ms),
        "{what}: not ended on this slot's window"
    );
    assert!(
        control.s.engine.lock().unwrap().tx_enabled(),
        "{what}: TX went off"
    );
}

/// ⭐ AN FT8 OVER'S END IS TESTED FIRST IN THE TICK, SO A STALL CANNOT HOLD PTT INTO THE OTHER STATION'S
/// PERIOD. A double-click or a logger's Reply holds the Engine lock while a decode in flight finishes
/// (seconds on a slow computer). A tick that came after 13.84 s with the lock held kept PTT up until
/// it was let go: here 1.5 s, to 15.34 s, a third of a second into the other station's period. WSJT-X
/// re-tests its window every 100 ms on a thread its CAT and its decoder never stall (`m_bTxTime`,
/// `widgets/mainwindow.cpp:870`, `:5055`, `:5180`, `:5530`). Now PTT drops first thing in that tick,
/// and only then does the tick wait for the lock.
#[test]
fn an_ft8_overs_end_does_not_wait_for_the_engine_lock() {
    let mut s = Over::new(Tier::Ft8);
    let lines = a_timed_rigctld(&mut s, 0, 0);
    let t0 = s.t;
    s.cq();
    s.run_to(t0 + 13_820.0);
    assert_eq!(
        (s.sent("T 1"), s.sent("T 0")),
        (1, 0),
        "premise: keyed and held"
    );
    let (holding, held) = std::sync::mpsc::channel();
    let engine = Arc::clone(&s.engine);
    let decode_wait = std::thread::spawn(move || {
        let lock = engine.lock().unwrap();
        holding.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(1_500));
        let let_go = now_unix_ms();
        drop(lock);
        let_go
    });
    held.recv().unwrap();
    s.run_to(t0 + 13_840.0);
    let let_go = decode_wait.join().unwrap();
    let unkeyed = lines
        .lock()
        .unwrap()
        .iter()
        .find(|(l, _)| l == "T 0")
        .map(|(_, t)| *t)
        .expect("PTT never dropped");
    assert!(
        unkeyed < let_go - 1_000.0,
        "PTT dropped {:.0} ms after the Engine lock was let go, not before it",
        unkeyed - let_go
    );
    assert_eq!(s.state.tx_until_ms, None);
}

/// A mock sound card whose flushes another thread can count while the loop waits elsewhere.
struct CountedFlushes {
    inner: MockBackend,
    flushes: Arc<std::sync::atomic::AtomicUsize>,
}

impl AudioBackend for CountedFlushes {
    fn capture(&mut self) -> Vec<f32> {
        self.inner.capture()
    }
    fn play(&mut self, samples: &[f32]) {
        self.inner.play(samples);
    }
    fn prepare_tx(&mut self) -> Result<(), String> {
        self.inner.prepare_tx()
    }
    fn flush_output(&mut self) -> usize {
        self.flushes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.flush_output()
    }
}

/// ⭐ AND ITS AUDIO STOP: the tick at 13.64 s that finds the Engine lock held stops the over's audio
/// before it waits for the lock, as WSJT-X's window test stops its Modulator on time. It used to
/// stop it only once the lock was let go.
#[test]
fn an_ft8_overs_audio_stop_does_not_wait_for_the_engine_lock() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0 + 13_620.0);
    assert_eq!(
        (s.sent("T 1"), s.sent("T 0")),
        (1, 0),
        "premise: keyed and held"
    );
    let flushes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut backend = CountedFlushes {
        inner: std::mem::take(&mut s.backend),
        flushes: Arc::clone(&flushes),
    };
    let (holding, held) = std::sync::mpsc::channel();
    let engine = Arc::clone(&s.engine);
    let counted = Arc::clone(&flushes);
    let decode_wait = std::thread::spawn(move || {
        let lock = engine.lock().unwrap();
        holding.send(()).unwrap();
        let give_up = std::time::Instant::now() + Duration::from_millis(1_500);
        while counted.load(std::sync::atomic::Ordering::SeqCst) == 0
            && std::time::Instant::now() < give_up
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        let stopped_while_held = counted.load(std::sync::atomic::Ordering::SeqCst);
        drop(lock);
        stopped_while_held
    });
    held.recv().unwrap();
    let sinks = no_sinks();
    let mut reopen_audio = |_: &Transport| -> Result<CountedFlushes, String> {
        Ok(CountedFlushes {
            inner: MockBackend::new(),
            flushes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    };
    let mut reopen_rig = mock_reopen_rig();
    s.state
        .step(
            &s.engine,
            &mut backend,
            &mut s.rig,
            &sinks,
            t0 + 13_640.0,
            &mut reopen_audio,
            &mut reopen_rig,
            &mut StationSinks::new(),
        )
        .unwrap();
    assert_eq!(
        decode_wait.join().unwrap(),
        1,
        "the over's audio did not stop at 13.64 s while the Engine lock was held"
    );
    assert_eq!(
        flushes.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the audio stopped twice"
    );
}
