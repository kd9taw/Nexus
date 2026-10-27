//! WSJT-X's FT8 full pass starts from the early pass's residual: at its third decode moment, 47
//! half-symbols (13.536 s, `m_earlyDecode2`, `widgets/mainwindow.cpp:2223`), the decoder subtracts
//! the early pass's decodes (`lib/ft8_decode.f90:124-149`), and the full pass at 14.40 s starts
//! from that audio, so what the early decodes hid is found in its first subtraction pass and handed
//! on first, and no early decode is handed on again (`:121`, `:152-162`).
//!
//! The loop is the real `RadioLoop::step`; the decodes are the real decoder's, on real FT8 audio.

use super::ft_midover::HeldWorker;
use super::*;

/// K2DEF's strong CQ on 1500 Hz at DT 0; K1ABC's RR73 to us on the same 1500 Hz, a second later
/// and much weaker, so the CQ hides it from the first subtraction pass over raw audio; light noise.
/// The period from its start, 14.5 s of it.
fn hidden_reply_audio() -> Vec<f32> {
    let mode = tempo_core::modes::make_mode(tempo_core::modes::ModeKind::Ft8);
    let mut x = vec![0f32; 174_000];
    let mut seed = 0x5EED_1234_u64;
    for v in x.iter_mut() {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *v = ((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * (4_000.0 / 32_767.0);
    }
    let mut add = |msg: &str, t0: f32, amp: f32| {
        // `gen_wave` puts the tones 0.5 s into what it returns.
        let wave = mode.gen_wave(&mode.encode(msg), 12_000.0, 1500.0);
        let o = ((t0 - 0.5) * 12_000.0) as usize;
        for (i, v) in wave.iter().enumerate() {
            if let Some(s) = x.get_mut(o + i) {
                *s += amp * v;
            }
        }
    };
    add("CQ K2DEF FN42", 0.5, 1_000.0 / 32_767.0);
    add("W9XYZ K1ABC RR73", 1.5, 80.0 / 32_767.0);
    x
}

/// The scene of the tests below, up to the full pass: W9XYZ waits for K1ABC's RR73 in slot 1. The
/// audio arrives `ahead` ms ahead of the clock at the early pass's, the 47 step's and the full
/// pass's tick, as a sound card's buffers make the ring run a little ahead or behind. The early
/// pass's result lands `early_lands_at` ms into the period. Returns the scene and what the full
/// pass handed on, in its order, not yet given to the loop; `step_ran` says whether a 47 step was
/// dispatched.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    backend: MockBackend,
    rig: Rig,
    state: RadioLoop,
    station: StationSinks,
    worker: HeldWorker,
}

fn hidden_reply_period(ahead: [f64; 3], early_lands_at: f64) -> (Scene, Vec<String>, bool) {
    use tempo_core::modes::ModeKind;
    let engine = wsjt_engine(Tier::Ft8);
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let mut station = StationSinks::new();
    let worker = HeldWorker::install(&mut state);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 100.0); // slot 0: nothing queued yet
    awaiting_rr73(&engine, ModeKind::Ft8);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 15_000.0); // slot 1: theirs

    let audio = hidden_reply_audio();
    // The audio as it arrives, up to `until` ms into the period.
    let mut fed = 0;
    let mut feed_to = |b: &mut MockBackend, until: f64| {
        let n = (until / 1000.0 * 12_000.0) as usize;
        b.queue_capture(audio[fed..n].to_vec());
        fed = n;
    };

    // The early pass.
    feed_to(&mut backend, 11_820.0 + ahead[0]);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 15_000.0 + 11_820.0);
    let early = worker.decode();
    assert!(
        matches!(worker.last_job.get(), Some((DecodePass::Early, None))),
        "the early pass has no rescue"
    );
    assert_eq!(
        early
            .decodes()
            .iter()
            .map(|d| d.message.trim())
            .collect::<Vec<_>>(),
        vec!["CQ K2DEF FN42"],
        "premise: the early pass decodes the CQ only"
    );
    // The decoder is busy until the early result lands.
    let mut t = 11_840.0;
    while t < early_lands_at {
        tick(&mut state, &engine, b, r, s, 15_000.0 + t);
        assert!(state.decode_in_flight, "the early pass is still running");
        t += 20.0;
    }
    worker.land(early);
    tick(&mut state, &engine, b, r, s, 15_000.0 + t);

    // WSJT-X's 47 step, when there is one.
    let step_at = t.max(13_560.0) + 20.0;
    feed_to(&mut backend, step_at + ahead[1]);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    tick(&mut state, &engine, b, r, s, 15_000.0 + step_at);
    let step_ran = match worker.try_decode() {
        Some(step) => {
            assert!(step.decodes().is_empty(), "the 47 step decodes nothing");
            worker.land(step);
            tick(&mut state, &engine, b, r, s, 15_000.0 + step_at + 20.0);
            true
        }
        None => false,
    };
    assert!(
        !state.decode_in_flight,
        "nothing in flight before the full pass"
    );

    // The full pass.
    feed_to(&mut backend, 14_420.0 + ahead[2]);
    let (b, r, s) = (&mut backend, &mut rig, &mut station);
    let dispatched = std::time::Instant::now();
    tick(&mut state, &engine, b, r, s, 15_000.0 + 14_420.0);
    let ticked = std::time::Instant::now();
    let full = worker.decode();
    // Its rescue gives up at the next period's early-pass moment, 11.808 s into it: 12.388 s after
    // this tick, 14.42 s into this period.
    let Some((DecodePass::Boundary, Some(rescue_at))) = worker.last_job.get() else {
        panic!("the full pass carries its rescue's deadline");
    };
    let left = std::time::Duration::from_millis(15_000 + 11_808 - 14_420);
    assert!(
        rescue_at >= dispatched + left && rescue_at <= ticked + left,
        "the rescue gives up 12.388 s after the full pass's tick: {:?} after it",
        rescue_at - dispatched
    );
    let found = worker.found_messages();
    worker.land(full);
    tick(&mut state, &engine, b, r, s, 15_000.0 + 14_440.0);
    (
        Scene {
            engine,
            backend,
            rig,
            state,
            station,
            worker,
        },
        found,
        step_ran,
    )
}

/// ⭐ THE ORDER, END TO END. We wait for K1ABC's RR73; the period also carries K2DEF's strong CQ on
/// our RX frequency, which hides the RR73. The early pass (11.808 s) decodes the CQ; the 47 step
/// (13.536 s) subtracts it; the full pass (14.40 s) finds the RR73 at once: it is the first decode
/// the full pass hands to the sequencer, and the CQ is not handed on again. The boundary then keys
/// the 73.
///
/// Before WSJT-X's residual, the full pass decoded raw audio: it found the CQ again and handed it
/// on first, and here never found the RR73 under it at all; the R-report went out again.
#[test]
fn ft8_the_full_pass_hands_on_what_the_early_decodes_hid_first() {
    use tempo_core::modes::ModeKind;
    let (mut sc, found, step_ran) = hidden_reply_period([0.0; 3], 11_840.0);
    assert!(step_ran, "the 47 step ran");
    assert!(
        found.iter().all(|m| m != "CQ K2DEF FN42"),
        "the full pass does not hand the early pass's CQ on again: {found:?}"
    );
    assert_eq!(
        found.first().map(String::as_str),
        Some("W9XYZ K1ABC RR73"),
        "the RR73 is the first decode the full pass hands on: {found:?}"
    );
    // The boundary: the RR73 was in its decision.
    let (b, r, s) = (&mut sc.backend, &mut sc.rig, &mut sc.station);
    tick(&mut sc.state, &sc.engine, b, r, s, 30_000.0);
    assert!(r.keyed, "keyed at the boundary");
    assert_eq!(
        wsjt_messages_played(ModeKind::Ft8, &b.played),
        vec!["K1ABC W9XYZ 73".to_string()],
        "the boundary keyed the 73"
    );
}

/// The period's three FT8 frames are taken from one sample, as WSJT-X's passes all read one buffer
/// from the period's start, so the residual still fits the full pass's audio when the sound card's
/// buffers put the ring a little ahead of the clock by a different amount at each tick: the
/// latest samples at the full pass's tick would start 20 ms later than the early pass's frame.
#[test]
fn ft8_the_periods_frames_line_up_when_the_audio_runs_ahead_of_the_clock() {
    let (_sc, found, step_ran) = hidden_reply_period([20.0, 0.0, 40.0], 11_840.0);
    assert!(step_ran, "the 47 step ran");
    assert_eq!(
        found,
        vec!["W9XYZ K1ABC RR73".to_string()],
        "the full pass started from the residual: the RR73 only"
    );
}

/// A 47 step that cannot start before WSJT-X's 14.3 s bail-out (here the early pass held the
/// decoder until 14.32 s) is passed over: the full pass is not held up and decodes the raw audio,
/// as before.
#[test]
fn ft8_a_47_step_that_cannot_start_before_14_3_s_is_passed_over() {
    let (sc, found, step_ran) = hidden_reply_period([0.0; 3], 14_320.0);
    assert!(!step_ran, "no 47 step after 14.3 s");
    assert_eq!(
        found.first().map(String::as_str),
        Some("CQ K2DEF FN42"),
        "the full pass decoded the raw audio: {found:?}"
    );
    assert_eq!(sc.state.dropped_decodes, 0, "nothing dropped");
    drop(sc.worker);
}
