//! What an operator action made while a decode is in flight does to the decodes of that period
//! and the next, set against WSJT-X — and that the action never waits for the decode.
//!
//! WSJT-X's window never waits for its decoder: the decoder is the separate `jt9` process
//! (`widgets/mainwindow.h:804`, its output read by `readFromStdout`, `mainwindow.cpp:850`), and a
//! decode's a-priori context (`mycall`, `hiscall`, `nQSOProgress`) is copied when that decode
//! starts (`decode()`, `mainwindow.cpp:5416`, `:5578-5581`). Line numbers are WSJT-X 3.0.2's.
//!
//! * **Double-click to work, and a logger's UDP Reply.** `doubleClickOnCall` (`:8831`) and
//!   `replyToCQ` (`:13279`, which calls `processMessage` at `:13336`) both end in
//!   `processMessage` (`:8918-9454`), which writes no decoder state. The decode in flight is
//!   shown as it is, and the next one runs with the new QSO's context and the a7 table it had.
//! * **A band change.** `band_changed` (`:12071-12076`) leaves the decoder's a7 table alone and
//!   hides a7 decodes for 1.5 periods instead (`no_a7_decodes`, read in `readFromStdout` at
//!   `:6184-6185`), "because they can be leftovers from the previous band". Nexus clears the
//!   table AND hides them, so no a7 decode from the old band reaches the next period here either.
//! * **A mode change.** `switch_mode` (`:11662-11666`) does the same for "the previous mode",
//!   after ANY mode switch: FT8 → FT4 → FT8 on one band shows no a7 decode until the hide ends.
//!   The next period is decoded by the new mode's decoder.
//! * **The period in flight** at a band or mode change is shown, as `readFromStdout` shows every
//!   line it reads, coloured for the band it was received on (`m_currentBandPeriod`, held 0.6
//!   periods after a dial change, `:4156-4165`). It reaches nothing else: WSJT-X sends it to no
//!   logger or PSK Reporter (`:13401`, `:7458-7459`), and here it never reaches the sequencer, so
//!   no line from the band or mode left starts, advances or logs a contact.
//! * **FT1's IR-HARQ buffers** are Nexus's own; WSJT-X has nothing like them. What must hold is
//!   that a reset asked for during a decode is made before the next FT1 decode reads them.
//!
//! The decode in flight is a real one: the recorded off-air FT8 period in
//! `crates/ft8/tests/fixtures/ft8_sample.wav`. The a7 table is shown carried, or cleared, with
//! the two-slot session `tests/decoder_ctx.rs` uses: a continuation the direct search band cannot
//! reach, recovered only by the a7 replay of the slot before.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempo_app::dto::{LateDecodes, Tier};
use tempo_app::engine::{
    engine_lock, run_decode_job, DecodeApplied, DecodePass, DecodeResult, Engine,
};
use tempo_app::settings::Settings;
use tempo_core::tempo_fast::{self, DecoderCtx};

/// Every test here uses the one process-global modem, so they take this in turn.
static SERIAL: Mutex<()> = Mutex::new(());

/// The FT8 slot's 0.5 s TX lead-in, in samples at 12 kHz.
const TX_START: usize = 6_000;

/// An action that does not wait for the decode is done well inside this.
const NO_WAIT: Duration = Duration::from_millis(100);

/// One decode, reduced to what is compared: the message and its a-priori type.
type Row = (String, i32);

/// The modem's statics back to their load-time image (see `tests/decoder_ctx.rs`).
fn wipe_process_modem_state() {
    DecoderCtx::new().scoped(|| {});
}

/// The operator's engine: FT8 on 20 m, decoding the whole passband.
fn operator() -> Engine {
    chain("KD9TAW", 200, 2900)
}

fn chain(mycall: &str, flow: u32, fhigh: u32) -> Engine {
    let mut eng = Engine::with_settings(Settings {
        mycall: mycall.to_string(),
        mygrid: "EN52".to_string(),
        band: "20m".to_string(),
        dial_mhz: 14.074,
        sideband: "USB".to_string(),
        decode_flow_hz: flow,
        decode_fhigh_hz: fhigh,
        ..Settings::default()
    });
    eng.set_tier(Tier::Ft8);
    eng.set_frequency(14.074, "20m", "USB");
    eng.set_rx_offset(1500.0);
    eng
}

/// A 15 s FT8 capture carrying `msg` at `f0` over a fixed noise floor (as `tests/decoder_ctx.rs`).
fn frame(msg: &str, f0: f32, mut seed: u32) -> Vec<f32> {
    let mode = modes::make_mode(modes::ModeKind::Ft8);
    let tones = mode.encode(msg);
    assert!(!tones.is_empty(), "{msg} must encode");
    let wave = mode.gen_wave(&tones, tempo_fast::SAMPLE_RATE, f0);
    let n = mode.frame_samples();
    let mut out = vec![0f32; n];
    for s in out.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *s = (((seed >> 8) % 31) as f32 - 15.0) / 32767.0;
    }
    for (i, &v) in wave.iter().enumerate() {
        if TX_START + i < n {
            out[TX_START + i] += v * 0.0305;
        }
    }
    out
}

/// A recorded period from a mode crate's fixtures, at the engine's capture scale and the mode's
/// frame length.
fn recorded(path: &str, kind: modes::ModeKind) -> Vec<f32> {
    let b = std::fs::read(path).expect("read the recorded period");
    let mut i = 12usize;
    let mut samples = Vec::new();
    while i + 8 <= b.len() {
        let size = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if &b[i..i + 4] == b"data" {
            let end = (i + 8 + size).min(b.len());
            samples = b[i + 8..end]
                .chunks_exact(2)
                .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32767.0)
                .collect();
            break;
        }
        i += 8 + size + (size & 1);
    }
    assert!(!samples.is_empty(), "no audio in {path}");
    samples.resize(modes::make_mode(kind).frame_samples(), 0.0);
    samples
}

fn ft8_recorded() -> Vec<f32> {
    recorded(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ft8/tests/fixtures/ft8_sample.wav"
        ),
        modes::ModeKind::Ft8,
    )
}

fn ft4_recorded() -> Vec<f32> {
    recorded(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ft4/tests/fixtures/ft4_sample.wav"
        ),
        modes::ModeKind::Ft4,
    )
}

fn rows(result: &DecodeResult) -> Vec<Row> {
    result
        .decodes()
        .iter()
        .map(|d| (d.message.clone(), d.nap))
        .collect()
}

/// The boundary job for `slot`, built as the radio loop builds it.
fn job(e: &Arc<Mutex<Engine>>, audio: &[f32], slot: u64) -> tempo_app::engine::DecodeJob {
    let mut eng = engine_lock(e);
    let job = eng.build_decode_job(audio.to_vec(), slot, DecodePass::Boundary);
    eng.begin_slot_capture();
    job
}

/// Decode `audio` as `slot` and fold it in, the decoder idle.
fn decode(e: &Arc<Mutex<Engine>>, audio: &[f32], slot: u64) -> (Vec<Row>, DecodeApplied) {
    let result = run_decode_job(job(e, audio, slot));
    let rows = rows(&result);
    (rows, engine_lock(e).apply_decode_result(result))
}

/// Decode `audio` as `slot` with a job from `factory`, a second engine that only builds jobs
/// (here: one searching 2000-2900 Hz). Nothing is folded.
fn decode_with(factory: &Engine, audio: &[f32], slot: u64) -> Vec<Row> {
    rows(&run_decode_job(factory.build_decode_job(
        audio.to_vec(),
        slot,
        DecodePass::Boundary,
    )))
}

/// Whether a decode is inside the modem now (the a7 reset guard is the modem lock).
fn modem_busy() -> bool {
    modes::Ft8A7ResetGuard::try_acquire().is_none()
}

/// Start the decode on its own thread and return once it is inside the modem: the lock held for
/// 20 ms running, which a reset in passing never does.
fn start_decoding(job: tempo_app::engine::DecodeJob) -> JoinHandle<DecodeResult> {
    let decode = std::thread::spawn(move || run_decode_job(job));
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut busy_since: Option<Instant> = None;
    loop {
        assert!(
            Instant::now() < deadline && !decode.is_finished(),
            "the decode was never seen inside the modem"
        );
        if modem_busy() {
            if busy_since.get_or_insert_with(Instant::now).elapsed() >= Duration::from_millis(20) {
                return decode;
            }
        } else {
            busy_since = None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// When the operator acts on the period in flight.
#[derive(Clone, Copy, PartialEq, Debug)]
enum When {
    /// While its decode runs.
    DuringTheDecode,
    /// After its job is built and before its decode starts: the decoder idle, as every action
    /// found it when it waited the decode out.
    DecoderIdle,
}

/// What a period in flight, and the operator's action on it, came to.
struct Outcome {
    /// How long the action held the Engine lock.
    held: Duration,
    /// The period in flight's decodes, and what folding them in did.
    in_flight: Vec<Row>,
    applied: DecodeApplied,
    /// What the screen then has: the live feed's messages, and a period shown from before a band
    /// or mode change.
    live: Vec<String>,
    late: Option<LateDecodes>,
}

/// Run the period `audio` as `slot` with `act` made on the engine `when` asked.
fn period_with(
    e: &Arc<Mutex<Engine>>,
    audio: &[f32],
    slot: u64,
    when: When,
    act: impl FnOnce(&mut Engine),
) -> Outcome {
    let job = job(e, audio, slot);
    let (held, result) = match when {
        When::DecoderIdle => {
            let t0 = Instant::now();
            act(&mut engine_lock(e));
            let held = t0.elapsed();
            (held, run_decode_job(job))
        }
        When::DuringTheDecode => {
            let decode = start_decoding(job);
            let t0 = Instant::now();
            act(&mut engine_lock(e));
            let held = t0.elapsed();
            assert!(
                held >= NO_WAIT || modem_busy(),
                "the decode ended before the action did, so this run did not act during it"
            );
            (held, decode.join().expect("the decode ends"))
        }
    };
    let in_flight = rows(&result);
    let applied = engine_lock(e).apply_decode_result(result);
    let snap = engine_lock(e).snapshot();
    Outcome {
        held,
        in_flight,
        applied,
        live: snap.recent_decodes.into_iter().map(|d| d.message).collect(),
        late: snap.late_decodes,
    }
}

/// The messages of a period shown from before a band or mode change.
fn late_messages(late: &Option<LateDecodes>) -> Vec<String> {
    late.iter()
        .flat_map(|l| l.rows.iter().map(|d| d.message.clone()))
        .collect()
}

/// The messages of `rows` but its a7 decodes: what WSJT-X shows of a period in the 1.5 periods
/// after a band or mode change.
fn without_a7(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .filter(|(_, nap)| *nap != 7)
        .map(|(m, _)| m.clone())
        .collect()
}

/// The continuation slot 3 carries: outside the 2000-2900 Hz search band, so only the a7 replay
/// of slot 1 can find it.
const CONTINUATION: &str = "KD9TAW W1AW R-10";

fn session_audio() -> (Vec<f32>, Vec<f32>) {
    (
        frame("KD9TAW W1AW FN31", 1500.0, 0x2452_1057),
        frame(CONTINUATION, 1500.0, 0x0BAD_5EED),
    )
}

/// What a double-click, a band change or a tier change made during the decode of a recorded
/// period came to: that period, and the next.
struct Session {
    period: Outcome,
    /// The next period's decodes searched 2000-2900 Hz: an a7 replay is the only way to the
    /// continuation.
    next_a7: Vec<Row>,
    /// The next period's decodes as the operator's engine makes them, and the message it would
    /// send after folding them in.
    next: Vec<Row>,
    tx_after: Option<String>,
}

/// Slot 1 (W1AW answers KD9TAW, seeding the a7 table), the recorded period as slot 2 with `act`
/// made `when` asked, then slot 3 (W1AW's report).
fn session(when: When, act: impl FnOnce(&mut Engine)) -> Session {
    let (wide, narrow) = session_audio();
    wipe_process_modem_state();
    let e = Arc::new(Mutex::new(operator()));
    let high = chain("W9XYZ", 2000, 2900);
    let (slot1, _) = decode(&e, &wide, 1);
    assert!(
        slot1.iter().any(|(m, _)| m == "KD9TAW W1AW FN31"),
        "slot 1 decodes directly: {slot1:?}"
    );
    let period = period_with(&e, &ft8_recorded(), 2, when, act);
    let next_a7 = decode_with(&high, &narrow, 3);
    let (next, _) = decode(&e, &narrow, 3);
    let tx_after = engine_lock(&e).snapshot().qso.and_then(|q| q.tx_now);
    Session {
        period,
        next_a7,
        next,
        tx_after,
    }
}

fn has_a7_continuation(rows: &[Row]) -> bool {
    rows.iter().any(|(m, nap)| m == CONTINUATION && *nap == 7)
}

#[test]
fn a_double_click_during_a_decode_leaves_that_period_and_the_next_as_wsjtx_has_them() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let click = |e: &mut Engine| {
        e.call_station_ctx(
            "W1AW",
            None,
            Some("KD9TAW W1AW FN31"),
            Some(-10),
            Some(1500.0),
        )
        .expect("the double-click starts the QSO");
    };
    let during = session(When::DuringTheDecode, click);
    let idle = session(When::DecoderIdle, click);

    assert!(
        during.period.held < NO_WAIT,
        "the double-click held the Engine lock {:?} while the decode ran",
        during.period.held
    );
    // That period: decoded and shown as it was, which is what WSJT-X does with the decode its
    // double-click lands in.
    assert!(
        during.period.in_flight.len() >= 18
            && during
                .period
                .in_flight
                .iter()
                .any(|(m, _)| m == "CQ F5RXL IN94"),
        "the recorded period decodes in full: {:?}",
        during.period.in_flight
    );
    assert_eq!(during.period.in_flight, idle.period.in_flight);
    assert!(
        matches!(during.period.applied, DecodeApplied::Boundary { .. }),
        "the period in flight is folded in"
    );
    // The next period: the a7 table the click found is still there (processMessage writes no
    // decoder state), and the QSO it started goes on as it does when the click waited.
    assert!(
        has_a7_continuation(&during.next_a7),
        "the a7 table outlived the double-click: {:?}",
        during.next_a7
    );
    assert_eq!(during.next_a7, idle.next_a7);
    assert_eq!(during.next, idle.next);
    assert_eq!(during.tx_after, idle.tx_after);
    assert!(during.tx_after.is_some(), "the QSO is running");
    // On screen: a double-click is no band or mode change, so the period in flight is the live
    // feed itself, not a period shown from before one.
    assert!(
        during.period.live.iter().any(|m| m == "CQ F5RXL IN94") && during.period.late.is_none(),
        "the period in flight is the live feed: live {:?}, late {:?}",
        during.period.live,
        during.period.late
    );
}

#[test]
fn a_band_change_during_a_decode_clears_the_a7_table_before_the_next_decode() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let qsy = |e: &mut Engine| e.set_frequency(7.074, "40m", "USB");
    let during = session(When::DuringTheDecode, qsy);
    let idle = session(When::DecoderIdle, qsy);
    let stayed = session(When::DecoderIdle, |_| {});

    assert!(
        during.period.held < NO_WAIT,
        "the band change held the Engine lock {:?} while the decode ran",
        during.period.held
    );
    // The control: with no band change the continuation comes back through the a7 replay, so a
    // reset that never happened would show.
    assert!(
        has_a7_continuation(&stayed.next_a7),
        "control: {:?}",
        stayed.next_a7
    );
    // That period: shown, as WSJT-X shows it, under the band, dial and mode it was heard on and
    // never under 40 m; and only there, so nothing of it is in the live feed of 40 m.
    for o in [&during.period, &idle.period] {
        let late = o.late.as_ref().expect("the period in flight is shown");
        assert_eq!(
            (late.band.as_str(), late.dial_mhz, late.tier),
            ("20m", 14.074, Tier::Ft8),
            "the period in flight is tagged with what it was heard on"
        );
        assert_eq!((late.period_start_ms, late.slot), (15_000, 2));
        assert_eq!(late_messages(&o.late), without_a7(&o.in_flight));
        assert!(
            late_messages(&o.late).iter().any(|m| m == "CQ F5RXL IN94")
                && matches!(o.applied, DecodeApplied::Late { n } if n >= 18),
            "the recorded period is shown in full: {:?}",
            late_messages(&o.late)
        );
        assert!(o.live.is_empty(), "20 m lines in 40 m's feed: {:?}", o.live);
    }
    // The next period: no a7 decode from the band left behind, as WSJT-X shows none.
    assert!(
        !during.next_a7.iter().any(|(m, _)| m == CONTINUATION),
        "an a7 decode from the old band reached the next period: {:?}",
        during.next_a7
    );
    assert_eq!(during.next_a7, idle.next_a7);
    assert_eq!(during.next, idle.next);
}

#[test]
fn a_tier_change_during_a_decode_decodes_the_next_period_with_the_new_decoder() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let run = |when: When| {
        wipe_process_modem_state();
        let e = Arc::new(Mutex::new(operator()));
        let period = period_with(&e, &ft8_recorded(), 2, when, |e| e.set_tier(Tier::Ft4));
        // The capture under way at the change was cut in two and is dropped; the FT4 period after
        // it is the first of the new mode.
        engine_lock(&e).begin_slot_capture();
        let (next, applied) = decode(&e, &ft4_recorded(), 5);
        assert!(matches!(applied, DecodeApplied::Boundary { .. }));
        let gave_way = engine_lock(&e).snapshot().late_decodes.is_none();
        (period, next, gave_way)
    };
    let (during, during_next, during_gave_way) = run(When::DuringTheDecode);
    let (idle, idle_next, _) = run(When::DecoderIdle);

    assert!(
        during.held < NO_WAIT,
        "the tier change held the Engine lock {:?} while the decode ran",
        during.held
    );
    // The FT8 period in flight was decoded by the FT8 decoder, and is shown under FT8 on 20 m at
    // 14.074 (not under FT4, whose dial is 14.080), sorting among the FT4 rows by its boundary in
    // FT4's slot numbering. Only there: none of it is in FT4's live feed.
    assert!(
        during.in_flight.iter().any(|(m, _)| m == "CQ F5RXL IN94"),
        "the FT8 decoder finished the FT8 period: {:?}",
        during.in_flight
    );
    let late = during
        .late
        .as_ref()
        .expect("the FT8 period in flight is shown");
    assert_eq!(
        (late.band.as_str(), late.dial_mhz, late.tier),
        ("20m", 14.074, Tier::Ft8)
    );
    assert_eq!((late.period_start_ms, late.slot), (15_000, 4));
    assert!(late.rows.iter().all(|r| r.tier == Tier::Ft8));
    // Made with the decoder idle, the change puts the FT4 decoder in before the FT8 period is
    // decoded, so that run shows whatever the FT4 decoder made of it, and only as a late period.
    for o in [&during, &idle] {
        assert_eq!(late_messages(&o.late), without_a7(&o.in_flight));
        assert!(matches!(o.applied, DecodeApplied::Late { .. }));
        assert!(o.live.is_empty(), "FT8 lines in FT4's feed: {:?}", o.live);
    }
    assert!(during_gave_way, "the first FT4 period replaces the FT8 one");
    // The next period is FT4, and the FT4 decoder is the one that decodes it.
    assert!(
        during_next.len() >= 14 && during_next.iter().any(|(m, _)| m == "CQ RU N9OY EN43"),
        "the FT4 period decodes in full: {during_next:?}"
    );
    assert_eq!(during_next, idle_next);
}

/// WSJT-X 3.0.2 hides a7 decodes for 1.5 periods after ANY mode switch (`switch_mode`,
/// `widgets/mainwindow.cpp:11664-11666`), an FT8 → FT4 → FT8 change on one band included, where
/// Nexus used to leave the a7 table, and what it replays, alone. The table here still holds slot
/// 1's W1AW, so the next FT8 period's continuation comes back through the a7 replay. The engine
/// that decodes it searches 2000-2900 Hz, so the a7 replay is its only way to that line, and what
/// it then shows is what the hide decides.
#[test]
fn an_ft8_ft4_ft8_switch_hides_a7_decodes_for_one_and_a_half_periods() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (wide, narrow) = session_audio();
    // What the decoder found in the next FT8 period, and what the screen then shows, after
    // `switch` on the engine that decodes it.
    let next_period = |switch: &dyn Fn(&mut Engine)| -> (Vec<Row>, Vec<String>) {
        wipe_process_modem_state();
        let seed = Arc::new(Mutex::new(operator()));
        let e = Arc::new(Mutex::new(chain("KD9TAW", 2000, 2900)));
        let (slot1, _) = decode(&seed, &wide, 1);
        assert!(
            slot1.iter().any(|(m, _)| m == "KD9TAW W1AW FN31"),
            "slot 1 decodes directly: {slot1:?}"
        );
        {
            let mut e = engine_lock(&e);
            switch(&mut e);
            // The next capture begins after the switch, as the radio loop's next boundary has it.
            e.begin_slot_capture();
        }
        let (decoded, applied) = decode(&e, &narrow, 3);
        assert!(matches!(applied, DecodeApplied::Boundary { .. }));
        let shown = engine_lock(&e)
            .snapshot()
            .recent_decodes
            .into_iter()
            .map(|d| d.message)
            .collect();
        (decoded, shown)
    };
    let ft8_ft4_ft8 = |e: &mut Engine| {
        e.set_tier(Tier::Ft4);
        e.set_tier(Tier::Ft8);
    };

    // The control: with no switch the a7 continuation is decoded and shown.
    let (decoded, shown) = next_period(&|_| {});
    assert!(has_a7_continuation(&decoded), "control: {decoded:?}");
    assert!(
        shown.iter().any(|m| m == CONTINUATION),
        "control: {shown:?}"
    );
    // FT8 → FT4 → FT8 inside 1.5 periods: the decoder still finds it, and the screen does not
    // show it, as 3.0.2's does not.
    let (decoded, shown) = next_period(&ft8_ft4_ft8);
    assert!(
        has_a7_continuation(&decoded),
        "the decoder found it: {decoded:?}"
    );
    assert!(
        !shown.iter().any(|m| m == CONTINUATION),
        "an a7 decode WSJT-X 3.0.2 hides is on screen: {shown:?}"
    );
    // On 3.0.2's clock the first timer to fire ends the hide: here the FT4 switch's, 1.5 FT4
    // periods (11.25 s) after it, not 1.5 FT8 periods after the switch back. Still hidden 5 s
    // before it (the margin covers the decode's own length)…
    let (decoded, shown) = next_period(&|e| {
        ft8_ft4_ft8(e);
        e.age_a7_hide(Duration::from_millis(11_250 - 5_000));
    });
    assert!(has_a7_continuation(&decoded), "{decoded:?}");
    assert!(
        !shown.iter().any(|m| m == CONTINUATION),
        "the a7 decode was shown before 1.5 FT4 periods: {shown:?}"
    );
    // …and shown again once it has fired.
    let (decoded, shown) = next_period(&|e| {
        ft8_ft4_ft8(e);
        e.age_a7_hide(Duration::from_millis(11_250));
    });
    assert!(has_a7_continuation(&decoded), "{decoded:?}");
    assert!(
        shown.iter().any(|m| m == CONTINUATION),
        "the a7 decode stayed hidden after 1.5 FT4 periods: {shown:?}"
    );
}

/// The QSO as the screen shows it (state, DX call, next message), how many contacts were logged,
/// and whether TX is enabled.
type QsoNow = (Option<(String, Option<String>, Option<String>)>, u32, bool);

fn qso_now(e: &Engine) -> QsoNow {
    let s = e.snapshot();
    (
        s.qso.map(|q| (q.state, q.dxcall, q.tx_now)),
        s.logged_tick,
        e.tx_enabled(),
    )
}

/// WSJT-X's auto-sequencer reads every line it shows (`auto_sequence`, `:7434`), so after a knob
/// QSY, where nothing turns Enable Tx off (`band_changed` does only for a band picked from its
/// menu, `:12089-12097`), it would answer, advance or log from a line heard on the band left.
/// That part is not built: the period in flight is shown, and nothing else reads it.
#[test]
fn no_line_from_the_band_or_mode_left_starts_advances_or_logs_a_qso() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    type Act = fn(&mut Engine);
    // What the line does, the line the operator worked W1AW from (decoded the period before), how
    // the QSO got there, and the line heard in the period in flight.
    let cases: [(&str, Option<&str>, Act, &str); 3] = [
        (
            "starts",
            None,
            |e| e.set_mode("qso-run").expect("Call CQ"),
            "KD9TAW W1AW FN31",
        ),
        (
            "advances",
            Some("CQ W1AW FN31"),
            |e| {
                e.call_station_ctx("W1AW", None, Some("CQ W1AW FN31"), Some(-10), Some(1500.0))
                    .expect("working W1AW");
            },
            "KD9TAW W1AW -10",
        ),
        (
            "logs",
            Some("KD9TAW W1AW -10"),
            |e| {
                e.call_station_ctx(
                    "W1AW",
                    None,
                    Some("KD9TAW W1AW -10"),
                    Some(-12),
                    Some(1500.0),
                )
                .expect("working W1AW");
            },
            "KD9TAW W1AW RR73",
        ),
    ];
    let changes: [(&str, Act); 2] = [
        ("a band change", |e| e.set_frequency(7.074, "40m", "USB")),
        ("a mode change", |e| e.set_tier(Tier::Ft4)),
    ];
    for (does, worked_from, setup, line) in cases {
        let audio = frame(line, 1500.0, 0x0057_A47D);
        // The QSO before the period's fold (after the change, if any) and after it, and the
        // period's outcome.
        let run = |change: Option<Act>| {
            wipe_process_modem_state();
            let e = Arc::new(Mutex::new(operator()));
            if let Some(heard) = worked_from {
                let (rows, applied) = decode(&e, &frame(heard, 1500.0, 0x2452_1057), 1);
                assert!(
                    rows.iter().any(|(m, _)| m == heard)
                        && matches!(applied, DecodeApplied::Boundary { .. }),
                    "premise: {heard} is decoded and folded: {rows:?}"
                );
            }
            setup(&mut engine_lock(&e));
            let mut before = None;
            // W1AW's next over, two periods on, as a station on the other T/R period sends it.
            let o = period_with(&e, &audio, 3, When::DuringTheDecode, |e| {
                if let Some(change) = change {
                    change(e);
                }
                before = Some(qso_now(e));
            });
            let after = qso_now(&engine_lock(&e));
            (before.expect("the action ran"), after, o)
        };
        // The control: with no change the line does what it says.
        let (before, after, o) = run(None);
        assert!(
            o.in_flight.iter().any(|(m, _)| m == line),
            "premise: {line} decodes: {:?}",
            o.in_flight
        );
        assert_ne!(
            before, after,
            "control: with no change, {line} {does} a QSO"
        );
        for (what, change) in changes {
            let (before, after, o) = run(Some(change));
            assert!(
                late_messages(&o.late).iter().any(|m| m == line),
                "after {what}, {line} is shown: {:?}",
                o.late
            );
            assert_eq!(
                before, after,
                "after {what}, {line}, heard before it, {does} a QSO"
            );
        }
    }
}

/// An FT1 RV0 frame too weak to decode alone and the RV1 retransmission that recovers it through
/// the IR-HARQ buffer, as slot 2 and slot 4 audio. Found by trying a few, so the control holds on
/// the machine the test runs on: through the engine's own decode jobs, the RV0 decodes nothing and
/// the RV1 after it decodes the message as a combined (`rv == 1`) decode.
fn harq_pair(e: &Arc<Mutex<Engine>>) -> (Vec<f32>, Vec<f32>) {
    fn gauss(seed: &mut u64) -> f32 {
        let mut u = || {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((*seed >> 11) as f64) / ((1u64 << 53) as f64)
        };
        let (u1, u2) = ((u() + 1e-12).min(1.0), u());
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
    let frame = |rv: i32, amp: f32, mut seed: u64| -> Vec<f32> {
        let wave = tempo_core::tx::build_rv(HARQ_MSG, tempo_fast::SAMPLE_RATE, 1500.0, rv).wave;
        // The 0.3 s lead-in FT1's own waveform carries (`modes`' `FT1_LEAD_IN_SECS`).
        let lead = 3_600;
        (0..tempo_fast::NMAX)
            .map(|i| {
                let s = i.checked_sub(lead).and_then(|j| wave.get(j)).copied();
                (s.unwrap_or(0.0) * amp + 300.0 * gauss(&mut seed)) / 32767.0
            })
            .collect()
    };
    for (seed0, seed1) in [(9, 10), (13, 14), (3, 4), (39, 40), (23, 24)] {
        for amp in [67.0, 65.0, 69.0, 63.0, 71.0] {
            let (rv0, rv1) = (frame(0, amp, seed0), frame(1, amp, seed1));
            tempo_fast::harq_reset();
            if decode_rv(e, &rv0, 2).is_none() && decode_rv(e, &rv1, 4) == Some(1) {
                return (rv0, rv1);
            }
        }
    }
    panic!("no RV0/RV1 pair here is recovered by IR-HARQ combining alone");
}

const HARQ_MSG: &str = "KD9TAW W1AW EN52";

/// Decode `audio` as `slot` and fold it in, the decoder idle: the redundancy version `HARQ_MSG`
/// was decoded at, if it was.
fn decode_rv(e: &Arc<Mutex<Engine>>, audio: &[f32], slot: u64) -> Option<i32> {
    let result = run_decode_job(job(e, audio, slot));
    let rv = result
        .decodes()
        .iter()
        .find(|d| d.message == HARQ_MSG)
        .map(|d| d.rv.unwrap_or(-1));
    engine_lock(e).apply_decode_result(result);
    rv
}

/// FT1's IR-HARQ buffers are Nexus's own (WSJT-X has none), so there is no WSJT-X behaviour to
/// match: what has to hold is that a reset asked for while a decode runs is made before the next
/// FT1 decode, exactly as when the asker waited for the decode. Call CQ (`set_mode("qso-run")`)
/// asks for it, as a double-click, a logger's Reply and a Chat QSY do.
#[test]
fn an_ir_harq_reset_asked_for_during_a_decode_is_made_before_the_next_ft1_decode() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let ft1 = || {
        let mut e = operator();
        e.set_tier(Tier::TempoFast);
        Arc::new(Mutex::new(e))
    };
    let (rv0, rv1) = harq_pair(&ft1());
    // RV0 is buffered; then an FT8 period runs (here is where Call CQ lands); then the RV1.
    let run = |reset: Option<When>| {
        wipe_process_modem_state();
        let e = ft1();
        assert_eq!(
            decode_rv(&e, &rv0, 2),
            None,
            "premise: RV0 alone decodes nothing"
        );
        engine_lock(&e).set_tier(Tier::Ft8);
        let call_cq = |e: &mut Engine| e.set_mode("qso-run").expect("Call CQ");
        let held = match reset {
            Some(when) => period_with(&e, &ft8_recorded(), 3, when, call_cq).held,
            None => period_with(&e, &ft8_recorded(), 3, When::DecoderIdle, |_| {}).held,
        };
        engine_lock(&e).set_tier(Tier::TempoFast);
        (held, decode_rv(&e, &rv1, 4))
    };
    let (_, kept) = run(None);
    let (_, idle) = run(Some(When::DecoderIdle));
    let (held, during) = run(Some(When::DuringTheDecode));

    assert_eq!(
        kept,
        Some(1),
        "control: with no reset the RV1 recovers the message"
    );
    assert_eq!(
        idle, None,
        "a reset made with the decoder idle clears the RV0"
    );
    assert!(
        held < NO_WAIT,
        "Call CQ held the Engine lock {held:?} while the decode ran"
    );
    assert_eq!(
        during, None,
        "the reset Call CQ asked for during the decode was not made before the next FT1 decode"
    );
}
