//! FT8/FT4 decodes reach the sequencer as the decoder finds them, as WSJT-X's do: its decoder
//! calls back with each new decode (`lib/ft8_decode.f90:233-238`, `lib/ft4_decode.f90:456`), jt9
//! writes and flushes that line at once (`lib/decoder.f90:1699-1802`, `flush(6)` at :1793), and the
//! GUI reads each line as it arrives and hands it to the auto-sequencer
//! (`widgets/mainwindow.cpp:850` `readyReadStandardOutput` → `readFromStdout` :6132-6143, which
//! calls `auto_sequence` per line at :7434). The candidates on the RX frequency are tried first
//! (`lib/ft8/sync8.f90:155-163`, `lib/ft4/getcandidates4.f90:70-81`), so a reply there reaches the
//! sequencer within the first moments of its pass.
//!
//! The loop is the real `RadioLoop::step`; the decodes are the real decoder's, on real FT8/FT4
//! audio.

use super::ft_midover::HeldWorker;
use super::*;

/// One scene of this module at a time. Every test in this binary decodes behind one process-wide
/// modem lock, and the real-time Flex DAX scenes beside these must decode a period inside about a
/// second and a half: these scenes' decodes come one after another rather than all at once.
static ONE_SCENE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_scene_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE_SCENE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The full pass's audio, `len` samples from the slot's start: `reply` at 1500 Hz (the RX
/// frequency) among `others` CQs spread over the rest of the band, in light noise. Enough signals
/// that the pass goes on decoding long after the RX-frequency candidate.
fn busy_slot_audio(
    kind: tempo_core::modes::ModeKind,
    reply: &str,
    others: usize,
    len: usize,
) -> Vec<f32> {
    let mode = tempo_core::modes::make_mode(kind);
    let mut frame = vec![0f32; len];
    let mut seed = 0xB05E_u64;
    for s in frame.iter_mut() {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *s = ((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02;
    }
    let mut add = |msg: &str, f0: f32, amp: f32| {
        let wave = mode.gen_wave(&mode.encode(msg), 12_000.0, f0);
        for (s, v) in frame.iter_mut().zip(wave) {
            *s += amp * v;
        }
    };
    add(reply, 1500.0, 0.03);
    // 50 Hz apart from 300 Hz, never within 100 Hz of the reply.
    let freqs = (0..)
        .map(|i| 300.0 + 50.0 * i as f32)
        .filter(|f| (f - 1500.0).abs() > 100.0)
        .take(others);
    for (i, f0) in freqs.enumerate() {
        let call = format!(
            "K{}{}{}",
            i % 10,
            (b'A' + (i / 10 % 26) as u8) as char,
            (b'A' + (i * 7 % 26) as u8) as char
        );
        add(&format!("CQ {call} EM{}{}", i % 10, i / 10 % 10), f0, 0.02);
    }
    frame
}

/// ⭐ THE STREAM, ON THE REAL DECODER. K1ABC's RR73 is on the RX frequency of a busy period; the
/// full pass finds it among its first candidates and goes on decoding the rest of the band. The
/// sequencer has the RR73 while the pass is still running, and the boundary keys the 73.
///
/// Five other stations: enough to outlast a loop tick, and few enough to be a good neighbour.
/// Every test in this binary decodes behind one process-wide modem lock and one a7 table, which a
/// full pass fills with each pair it decodes and later passes replay: a busier frame here held
/// other tests' decodes past their own deadlines.
#[test]
fn ft8_a_reply_on_the_rx_frequency_reaches_the_sequencer_while_its_pass_is_still_decoding() {
    use tempo_core::modes::ModeKind;
    let _scene = one_scene_at_a_time();
    let engine = wsjt_engine(Tier::Ft8);
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let mut station = StationSinks::new();
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 100.0); // slot 0: nothing queued yet
    awaiting_rr73(&engine, ModeKind::Ft8);
    tick(&mut state, &engine, b, r, s, 15_020.0); // slot 1: theirs

    backend.queue_capture(busy_slot_audio(
        ModeKind::Ft8,
        "W9XYZ K1ABC RR73",
        5,
        172_800,
    ));
    state.early_done_slot = Some(1);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 29_400.0);
    assert!(
        state.decode_in_flight,
        "the full pass dispatched at 14.40 s"
    );

    // 20 ms ticks of real time, the loop's own cadence, until the sequencer has the reply or the
    // pass has ended.
    let started = std::time::Instant::now();
    let mut now = 29_401.0;
    let has_reply = |e: &Arc<Mutex<Engine>>| {
        e.lock().unwrap().ft_next_over(2).as_deref() == Some("K1ABC W9XYZ 73")
    };
    while state.decode_in_flight
        && !has_reply(&engine)
        && started.elapsed() < std::time::Duration::from_secs(20)
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
        tick(&mut state, &engine, b, r, s, now);
        now += 1.0;
    }
    assert!(has_reply(&engine), "the RR73 reached the sequencer");
    assert!(
        state.decode_in_flight,
        "it reached the sequencer while the rest of its pass was still decoding"
    );

    // The boundary, the pass still running: its decision has the reply.
    tick(&mut state, &engine, b, r, s, 30_000.0);
    assert!(r.keyed, "keyed at the boundary");
    assert_eq!(
        wsjt_messages_played(ModeKind::Ft8, &b.played),
        vec!["K1ABC W9XYZ 73".to_string()],
        "the boundary keyed the 73"
    );
    let played_at_key = b.played.len();
    b.tx_unplayed = played_at_key;
    assert!(
        drain_decode(&mut state, &engine, b, r, s, 30_001.0, 45_000.0),
        "the pass ended"
    );
    assert_eq!(
        b.played.len(),
        played_at_key,
        "its end is housekeeping only: no second over"
    );
    assert_eq!(b.retunes, 0, "nothing to switch: the 73 was keyed");
}

/// ⭐⭐ THE OPERATOR'S REPEATED OVER (ALL.TXT, 21 Oct 00:44:30: our R-report went out again
/// although the partner's RR73 for the period before it was decoded, just after the key). The full
/// pass found K1ABC's RR73 before the boundary and is still running at it. The RR73 is in the
/// boundary's decision: the 73 keys, not the R-report again, and the pass's end changes nothing.
fn a_reply_found_before_the_boundary_is_in_its_decision(tier: Tier) {
    use tempo_core::modes::ModeKind;
    let _scene = one_scene_at_a_time();
    let (kind, period, full_at, len) = match tier {
        Tier::Ft8 => (ModeKind::Ft8, 15_000.0, 14_400.0, 172_800),
        _ => (ModeKind::Ft4, 7_500.0, 6_048.0, 72_576),
    };
    let engine = wsjt_engine(tier);
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let mut station = StationSinks::new();
    let worker = HeldWorker::install(&mut state);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 100.0); // slot 0: nothing queued yet
    awaiting_rr73(&engine, kind);
    tick(&mut state, &engine, b, r, s, period + 20.0); // slot 1: theirs
    assert!(!r.keyed, "odd slot: not ours");

    backend.queue_capture(wsjt_slot_audio(kind, "W9XYZ K1ABC RR73", len));
    state.early_done_slot = Some(1);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, period + full_at);
    assert!(
        state.decode_in_flight && state.full_done_slot == Some(1),
        "the full pass dispatched"
    );
    let pass = worker.decode();
    assert!(
        pass.decodes()
            .iter()
            .any(|d| d.message.trim() == "W9XYZ K1ABC RR73"),
        "premise: the pass finds the RR73"
    );
    // The RR73 is handed on the moment the pass finds it; the pass goes on.
    worker.stream();
    tick(&mut state, &engine, b, r, s, period + full_at + 20.0);
    assert!(state.decode_in_flight, "the pass's end has not landed");

    tick(&mut state, &engine, b, r, s, 2.0 * period);
    assert!(r.keyed, "keyed at the boundary, on time");
    assert_eq!(
        wsjt_messages_played(kind, &b.played),
        vec!["K1ABC W9XYZ 73".to_string()],
        "the boundary keyed the 73: the RR73 was in its decision"
    );
    assert_eq!(state.dropped_decodes, 0, "nothing dropped");
    let played_at_key = b.played.len();
    b.tx_unplayed = played_at_key;

    worker.land(pass);
    tick(&mut state, &engine, b, r, s, 2.0 * period + 400.0);
    assert!(!state.decode_in_flight, "the pass's end landed");
    assert_eq!(
        b.played.len(),
        played_at_key,
        "its end is housekeeping only: no second over"
    );
    assert_eq!(b.retunes, 0, "nothing switched");
    assert_eq!(
        engine.lock().unwrap().snapshot().qso.and_then(|q| q.tx_now),
        None,
        "the 73 went out: nothing left to send"
    );
}

#[test]
fn ft8_a_reply_found_before_the_boundary_is_in_its_decision() {
    a_reply_found_before_the_boundary_is_in_its_decision(Tier::Ft8);
}

/// FT4: the same at FT4's moments, the 6.048 s pass and the 7.5 s boundary.
#[test]
fn ft4_a_reply_found_before_the_boundary_is_in_its_decision() {
    a_reply_found_before_the_boundary_is_in_its_decision(Tier::Ft4);
}

/// CONTROL: a period with no reply in it. The pass streams what it finds (a station working
/// someone else), the boundary repeats the R-report exactly as before, and nothing switches.
#[test]
fn a_period_with_no_reply_keys_the_same_over_as_before() {
    use tempo_core::modes::ModeKind;
    let _scene = one_scene_at_a_time();
    let engine = wsjt_engine(Tier::Ft8);
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let mut station = StationSinks::new();
    let worker = HeldWorker::install(&mut state);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 100.0);
    awaiting_rr73(&engine, ModeKind::Ft8);
    tick(&mut state, &engine, b, r, s, 15_020.0);
    backend.queue_capture(wsjt_slot_audio(ModeKind::Ft8, "W2DEF K1ABC RR73", 172_800));
    state.early_done_slot = Some(1);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 29_400.0);
    let pass = worker.decode();
    assert!(
        pass.decodes()
            .iter()
            .any(|d| d.message.trim() == "W2DEF K1ABC RR73"),
        "premise: the pass finds K1ABC's RR73 to another station"
    );
    worker.stream();
    tick(&mut state, &engine, b, r, s, 29_420.0);
    tick(&mut state, &engine, b, r, s, 30_000.0);
    assert!(r.keyed, "keyed at the boundary");
    assert_eq!(
        wsjt_messages_played(ModeKind::Ft8, &b.played),
        vec!["K1ABC W9XYZ R-10".to_string()],
        "the R-report again, as before: a reply to someone else moves nothing"
    );
    worker.land(pass);
    tick(&mut state, &engine, b, r, s, 30_400.0);
    assert_eq!(b.retunes, 0, "nothing switched");
}

/// When each slowed early-pass candidate was tried, by the one worker below.
static SLOW_CANDIDATES: Mutex<Vec<std::time::Instant>> = Mutex::new(Vec::new());

/// The loop's own worker, with each early-pass sync candidate slowed by 50 ms: a decoder far
/// slower than this box, as on the laptops that drop a period's full decode today. Run to its end,
/// such a pass takes seconds. (It holds the process-wide modem lock while it sleeps, as a slow
/// decoder would, so the scene below keeps it short for the other tests in this run.)
fn run_with_a_slow_early_pass(job: WorkerJob, out: &mut dyn FnMut(WorkerOut)) {
    struct Slowed<'a>(&'a mut dyn FnMut(WorkerOut), bool);
    impl tempo_app::engine::DecodeProgress for Slowed<'_> {
        fn found(&mut self, d: tempo_app::engine::StreamedDecode) {
            (self.0)(WorkerOut::Found(d));
        }
        fn candidate(&mut self) {
            if self.1 {
                SLOW_CANDIDATES
                    .lock()
                    .unwrap()
                    .push(std::time::Instant::now());
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    match job {
        WorkerJob::Slot(j) => {
            let early = j.pass() == DecodePass::Early;
            let result =
                tempo_app::engine::run_decode_job_streaming(j, &mut Slowed(&mut *out, early));
            out(WorkerOut::Done(result));
        }
        other => run_worker_job(other, out),
    }
}

/// ⭐ WSJT-X's EARLY-PASS GIVE-UP (`lib/ft8_decode.f90:240-242`, 13.4 s into the period). An early
/// pass far too slow for this period stops at 13.4 s, so the decoder is free at 14.40 s: the full
/// pass runs, finds K1ABC's RR73, and the boundary keys the 73. Without the give-up the slow pass
/// would hold the decoder through the full pass's moment and the boundary, which then drops the
/// period's decode and repeats the R-report. The loop first ticks in this period at 13.3 s (as
/// after a stall), so the early pass starts there, 0.1 s before its give-up: past that moment it
/// tries at most one more candidate (one, too, if the modem was busy until after it).
#[test]
fn ft8_a_slow_early_pass_gives_up_at_13_4_s_and_the_full_pass_runs() {
    use tempo_core::modes::ModeKind;
    let _scene = one_scene_at_a_time();
    let engine = wsjt_engine(Tier::Ft8);
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    state.decode = DecodeWorker::spawn_with("slow-early-test", run_with_a_slow_early_pass);
    let mut station = StationSinks::new();
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 100.0);
    awaiting_rr73(&engine, ModeKind::Ft8);
    tick(&mut state, &engine, b, r, s, 15_020.0);
    backend.queue_capture(busy_slot_audio(
        ModeKind::Ft8,
        "W9XYZ K1ABC RR73",
        5,
        172_800,
    ));
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    // The early pass, due since 11.808 s, starts at the first tick in the period: 13.3 s. Its
    // give-up is 0.1 s after the loop dispatches it, so no earlier than this.
    SLOW_CANDIDATES.lock().unwrap().clear();
    let give_up = std::time::Instant::now() + std::time::Duration::from_millis(100);
    tick(&mut state, &engine, b, r, s, 28_300.0);
    assert!(
        state.decode_in_flight && state.early_done_slot == Some(1),
        "the early pass dispatched"
    );
    let dispatched = std::time::Instant::now();
    let mut now = 28_301.0;
    while state.decode_in_flight && dispatched.elapsed() < std::time::Duration::from_secs(8) {
        std::thread::sleep(std::time::Duration::from_millis(20));
        tick(&mut state, &engine, b, r, s, now);
        now += 1.0;
    }
    assert!(
        !state.decode_in_flight,
        "the slow early pass ended (gave up) within 8 s"
    );
    let tried = SLOW_CANDIDATES.lock().unwrap().clone();
    assert!(
        !tried.is_empty(),
        "premise: the early pass tried candidates"
    );
    let after = tried.iter().filter(|t| **t > give_up).count();
    assert!(
        after <= 1,
        "it gave up at 13.4 s into the period: {after} of its {} candidates were tried after it",
        tried.len()
    );
    // The full pass's moment: the decoder is free.
    tick(&mut state, &engine, b, r, s, 29_400.0);
    assert!(
        state.decode_in_flight && state.full_done_slot == Some(1),
        "the full pass ran at 14.40 s"
    );
    assert!(
        drain_decode(&mut state, &engine, b, r, s, 29_401.0, 29_990.0),
        "the full pass ended before the boundary"
    );
    tick(&mut state, &engine, b, r, s, 30_020.0);
    assert!(r.keyed, "keyed at the boundary");
    assert_eq!(state.dropped_decodes, 0, "no period dropped");
    assert_eq!(
        wsjt_messages_played(ModeKind::Ft8, &b.played),
        vec!["K1ABC W9XYZ 73".to_string()],
        "the full pass's RR73 was in the boundary's decision"
    );
}
