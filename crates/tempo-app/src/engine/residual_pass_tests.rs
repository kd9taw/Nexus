//! WSJT-X's FT8 47 step as a decode job ([`DecodePass::Residual`]): it subtracts the period's
//! early decodes for the full pass (`lib/ft8_decode.f90:124-149`), decodes nothing and folds
//! nothing, and the full pass after it hands on no early decode again (`:121`, `:152-162`).

use super::*;

/// A station on 20 m FT8.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.write_all_txt = true;
    e
}

/// The period from its start: one strong CQ at 1000 Hz, DT 0, in light noise; the first `n`
/// samples, zero after, as the radio loop takes each pass's frame.
fn period(n: usize) -> Vec<f32> {
    let mode = modes::make_mode(modes::ModeKind::Ft8);
    let wave = mode.gen_wave(&mode.encode("CQ K2DEF FN42"), 12_000.0, 1000.0);
    let mut seed = 0x0047_u64;
    (0..180_000)
        .map(|i| {
            if i >= n {
                return 0.0;
            }
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.02
                + 0.05 * wave.get(i).copied().unwrap_or(0.0)
        })
        .collect()
}

/// What a pass hands on as it runs.
#[derive(Default)]
struct Collect(Vec<String>);
impl DecodeProgress for Collect {
    fn found(&mut self, d: StreamedDecode) {
        self.0.push(d.decode().message.trim().to_string());
    }
}

/// The 47 step decodes nothing and folds nothing, over audio full of a decode; the full pass after
/// it does not hand the early decode on again. Without the step, it does.
#[test]
fn the_47_step_decodes_nothing_and_the_full_pass_after_it_hands_on_no_early_decode() {
    for with_step in [true, false] {
        let mut e = station();
        let early = run_decode_job(e.build_decode_job(period(41 * 3456), 9, DecodePass::Early));
        assert_eq!(
            early
                .decodes()
                .iter()
                .map(|d| d.message.trim())
                .collect::<Vec<_>>(),
            vec!["CQ K2DEF FN42"],
            "premise: the early pass decodes the CQ"
        );
        assert!(matches!(
            e.apply_decode_result(early),
            DecodeApplied::Early { n: 1 }
        ));
        let rows = e.take_all_txt_pending().len();
        if with_step {
            let step =
                run_decode_job(e.build_decode_job(period(47 * 3456), 9, DecodePass::Residual));
            assert!(step.decodes().is_empty(), "the 47 step decodes nothing");
            assert!(
                matches!(e.apply_decode_result(step), DecodeApplied::Stale),
                "and folds nothing"
            );
            assert!(e.take_all_txt_pending().is_empty(), "no ALL.TXT row");
        }
        let mut full = Collect::default();
        let result = run_decode_job_streaming(
            e.build_decode_job(period(50 * 3456), 9, DecodePass::Boundary),
            &mut full,
        );
        if with_step {
            assert!(
                full.0.is_empty(),
                "after the 47 step the full pass hands on no early decode: {:?}",
                full.0
            );
            assert_eq!(
                result
                    .decodes()
                    .iter()
                    .map(|d| d.message.trim())
                    .collect::<Vec<_>>(),
                vec!["CQ K2DEF FN42"],
                "the early decode opens the full pass's list"
            );
        } else {
            assert_eq!(
                full.0,
                vec!["CQ K2DEF FN42".to_string()],
                "control: without the step the full pass hands it on again"
            );
        }
        // Either way the boundary folds it once: the early pass did.
        assert!(matches!(
            e.apply_decode_result(result),
            DecodeApplied::Boundary { n: 0, .. }
        ));
        assert_eq!(rows, 1, "one ALL.TXT row, from the early pass");
        assert!(e.take_all_txt_pending().is_empty(), "and no second one");
    }
}

/// What the full pass is asked as it runs: one count per sync candidate, the full pass's and its
/// rescue's alike, each taking `delay` (a slow computer). Its clock is the pass's: `delay` per
/// candidate from `start`, so the give-up is decided by the candidates tried, not by how busy the
/// machine running the test is.
struct Slow {
    asked: usize,
    delay: Duration,
    start: Instant,
}
impl DecodeProgress for Slow {
    fn found(&mut self, _: StreamedDecode) {}
    fn candidate(&mut self) {
        self.asked += 1;
    }
    fn now(&self) -> Instant {
        self.start + self.delay * self.asked as u32
    }
}

/// The period's early pass and 47 step, then its full pass with the rescue's deadline `rescue_at`
/// (none: run to its end) on a decoder made slower by `delay` per candidate: (the full pass's
/// messages, candidates asked, how long the full pass took).
fn full_pass_by(rescue_at: Option<Duration>, delay: Duration) -> (Vec<String>, usize, Duration) {
    let e = station();
    let early = run_decode_job(e.build_decode_job(period(41 * 3456), 9, DecodePass::Early));
    assert_eq!(
        early.decodes().len(),
        1,
        "premise: the early pass decodes the CQ"
    );
    run_decode_job(e.build_decode_job(period(47 * 3456), 9, DecodePass::Residual));
    let mut job = e.build_decode_job(period(50 * 3456), 9, DecodePass::Boundary);
    let start = Instant::now();
    if let Some(at) = rescue_at {
        job = job.with_rescue_give_up_at(start + at);
    }
    let mut slow = Slow {
        asked: 0,
        delay,
        start,
    };
    let result = run_decode_job_streaming(job, &mut slow);
    let took = slow.now() - start;
    let messages = result
        .decodes()
        .iter()
        .map(|d| d.message.trim().to_string())
        .collect();
    (messages, slow.asked, took)
}

/// ⭐ THE RESCUE GIVES UP AT ITS MOMENT on a slow computer. Past its deadline it never starts, and
/// the full pass's result stands alone; a deadline met mid-rescue stops it at its next sync
/// candidate, with the job done within one candidate of the deadline. Without a deadline the
/// rescue runs to its end. The pass's clock is the candidates' ([`Slow`]), so a busy test machine
/// moves none of it.
#[test]
fn a_slow_computer_gives_the_rescue_up_at_its_moment() {
    let fast = Duration::ZERO;
    let (whole, whole_asked, _) = full_pass_by(None, fast);
    let (alone, alone_asked, _) = full_pass_by(Some(Duration::ZERO), fast);
    assert_eq!(
        alone, whole,
        "the full pass's result, the rescue given up or not"
    );
    assert!(
        whole_asked > alone_asked + 10,
        "given up before it starts, the rescue tries none of the band's candidates: \
         {alone_asked} asked, against {whole_asked} when it may run"
    );
    // The slow computer: 5 ms a candidate. The full pass alone takes `base`; the rescue may run
    // 150 ms past it, 30 of its candidates, a fraction of what it needs.
    let delay = Duration::from_millis(5);
    let (_, slow_alone_asked, base) = full_pass_by(Some(Duration::ZERO), delay);
    assert_eq!(slow_alone_asked, alone_asked, "the same full pass, slower");
    let budget = base + Duration::from_millis(150);
    let (cut, cut_asked, took) = full_pass_by(Some(budget), delay);
    assert_eq!(cut, whole, "what was found stands");
    assert!(
        cut_asked > alone_asked && cut_asked < whole_asked,
        "the rescue ran, and was given up: {cut_asked} candidates, against {alone_asked} without \
         it and {whole_asked} in all"
    );
    assert_eq!(
        cut_asked,
        alone_asked + 30,
        "given up at the first candidate past its deadline"
    );
    assert!(
        took >= budget && took < budget + delay,
        "given up within a candidate of its deadline: {took:?} against {budget:?}"
    );
}

/// The period's early pass and 47 step, then its full pass with no rescue deadline on a decoder
/// at full speed, a band change made before the full pass was built (`before`: its capture was
/// cut in two) or while it runs (`during`): (candidates asked, the full pass's result).
fn full_pass_across_a_change(before: bool, during: bool) -> (usize, DecodeResult) {
    let mut e = station();
    let early = run_decode_job(e.build_decode_job(period(41 * 3456), 9, DecodePass::Early));
    assert_eq!(
        early.decodes().len(),
        1,
        "premise: the early pass decodes the CQ"
    );
    run_decode_job(e.build_decode_job(period(47 * 3456), 9, DecodePass::Residual));
    if before {
        e.set_frequency(7.074, "40m", "USB");
    }
    let job = e.build_decode_job(period(50 * 3456), 9, DecodePass::Boundary);
    if during {
        e.set_frequency(7.074, "40m", "USB");
    }
    let mut asked = Slow {
        asked: 0,
        delay: Duration::ZERO,
        start: Instant::now(),
    };
    let result = run_decode_job_streaming(job, &mut asked);
    (asked.asked, result)
}

/// ⭐ A PERIOD NOBODY WILL SEE GETS NO RESCUE. A full pass whose capture a band change cut in two
/// is dropped when it lands, so its rescue never starts and the decoder is free for the new band;
/// a change made while the period decodes leaves it shown under what it was heard on, as WSJT-X
/// shows it, so its rescue still runs.
#[test]
fn a_band_change_gives_up_the_rescue_of_a_period_that_will_not_be_shown() {
    let (whole, _) = full_pass_across_a_change(false, false);
    let (_, alone, _) = full_pass_by(Some(Duration::ZERO), Duration::ZERO);
    assert!(
        whole > alone + 10,
        "premise: the rescue tries the band's candidates: {whole} asked, against {alone} without it"
    );
    let (cut, _) = full_pass_across_a_change(true, false);
    assert_eq!(
        cut, alone,
        "a capture cut in two by the change: the rescue never starts"
    );
    let (both, _) = full_pass_across_a_change(true, true);
    assert_eq!(both, alone, "nor after a second change");
    let (shown, result) = full_pass_across_a_change(false, true);
    assert_eq!(
        shown, whole,
        "a period heard whole before the change: the rescue runs"
    );
    assert!(
        result.heard_on.is_some(),
        "and that period is shown under what it was heard on"
    );
}
