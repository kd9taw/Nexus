//! Decodes streamed from a running FT8/FT4 pass ([`Engine::apply_streamed_decode`]), as WSJT-X
//! hands each decode to its sequencer the moment the decoder finds it: each folds once, the
//! sequencer has it at once, the screen keeps the whole pass, a decode from before a band change
//! never folds, and a pass the best-caller choice compares is folded whole, as before.

use super::*;

/// A station on 20 m FT8 at 1500 Hz with ALL.TXT on.
fn station() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.settings.write_all_txt = true;
    e
}

fn decode(message: &str, snr: i32, freq: f32) -> modes::Decode {
    modes::Decode {
        message: message.into(),
        sync: 10.0,
        snr,
        dt: 0.1,
        freq,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: Some(modes::ModeKind::Ft8),
    }
}

/// `message`, just found by `pass` for the boundary of `slot`, in the engine's decode context.
fn found(e: &Engine, message: &str, snr: i32, pass: DecodePass, slot: u64) -> StreamedDecode {
    StreamedDecode {
        decode: decode(message, snr, 1500.0),
        pass,
        slot,
        epoch: e.decode_epoch,
    }
}

/// `pass`'s result for `slot`: `messages`, as its decoder returns them.
fn result(e: &Engine, messages: &[&str], pass: DecodePass, slot: u64) -> DecodeResult {
    DecodeResult {
        decodes: messages.iter().map(|m| decode(m, -10, 1500.0)).collect(),
        frame: vec![0.0; 180_000],
        pass,
        slot,
        epoch: e.decode_epoch,
        heard_on: None,
        failed: false,
    }
}

/// How many times `message` is in `slot`'s decode history and in ALL.TXT's pending rows.
fn copies(e: &mut Engine, message: &str, slot: u64) -> (usize, usize) {
    let history = e
        .decode_history
        .iter()
        .filter(|(s, d)| *s == slot && d.message == message)
        .count();
    let rows = e
        .station
        .all_txt_pending
        .iter()
        .filter(|r| r.ends_with(message))
        .count();
    (history, rows)
}

const A: &str = "CQ K1ABC FN31";
const B: &str = "CQ W2XYZ FN20";
const C: &str = "CQ N3QQ FM19";
const D: &str = "CQ K4ZZ EM73";

/// A message the early pass streamed, its result, the full pass's stream and its result all
/// carry: it folds once — one row, one ALL.TXT line, one observe. What only a result carries (not
/// streamed) folds with that result, as before.
#[test]
fn each_message_folds_once_across_the_stream_and_both_passes() {
    let mut e = station();
    let slot = 4;
    assert_eq!(
        e.apply_streamed_decode(found(&e, A, -5, DecodePass::Early, slot)),
        1
    );
    assert_eq!(
        e.apply_streamed_decode(found(&e, B, -5, DecodePass::Early, slot)),
        1
    );
    let early = result(&e, &[A, B], DecodePass::Early, slot);
    assert!(
        matches!(e.apply_decode_result(early), DecodeApplied::Early { n: 0 }),
        "the early result has nothing new"
    );
    assert_eq!(
        e.apply_streamed_decode(found(&e, A, -5, DecodePass::Boundary, slot)),
        0
    );
    assert_eq!(
        e.apply_streamed_decode(found(&e, B, -5, DecodePass::Boundary, slot)),
        0
    );
    assert_eq!(
        e.apply_streamed_decode(found(&e, C, -5, DecodePass::Boundary, slot)),
        1
    );
    let full = result(&e, &[A, B, C, D], DecodePass::Boundary, slot);
    assert!(
        matches!(
            e.apply_decode_result(full),
            DecodeApplied::Boundary { n: 1, .. }
        ),
        "the full result folds only what was not streamed"
    );
    for m in [A, B, C, D] {
        assert_eq!(
            copies(&mut e, m, slot),
            (1, 1),
            "{m}: one row, one ALL.TXT line"
        );
    }
}

/// The partner's RR73 reaches the sequencer the moment it is streamed: the next over is the 73
/// before the pass that found it has ended.
#[test]
fn a_streamed_reply_reaches_the_sequencer_before_its_pass_ends() {
    let mut e = station();
    e.hear_for_test("KD9TAW W1AW -10", -10, 2);
    e.call_station_ctx(
        "W1AW",
        Some("FN31"),
        Some("KD9TAW W1AW -10"),
        Some(-10),
        None,
    )
    .expect("premise: W1AW worked");
    assert_eq!(
        e.ft_next_over(4).as_deref(),
        Some("W1AW KD9TAW R-10"),
        "premise: the R-report is next"
    );
    let folded =
        e.apply_streamed_decode(found(&e, "KD9TAW W1AW RR73", -12, DecodePass::Boundary, 4));
    assert_eq!(folded, 1);
    assert_eq!(
        e.ft_next_over(4).as_deref(),
        Some("W1AW KD9TAW 73"),
        "the RR73 is in the sequencer: the 73 is next"
    );
}

/// The screen polls the snapshot a few times a second and a pass streams faster than that, so
/// every decode of the pass streaming now is in the snapshot until the pass ends; the next pass
/// replaces them, as each pass's batch always has.
#[test]
fn the_screen_keeps_every_decode_of_the_pass_streaming() {
    let mut e = station();
    for m in [A, B, C] {
        e.apply_streamed_decode(found(&e, m, -5, DecodePass::Boundary, 4));
    }
    let shown = |e: &Engine| -> Vec<String> {
        e.snapshot()
            .recent_decodes
            .into_iter()
            .filter(|r| !r.mine)
            .map(|r| r.message)
            .collect()
    };
    assert_eq!(shown(&e), [A, B, C], "the whole pass so far");
    let full = result(&e, &[A, B, C, D], DecodePass::Boundary, 4);
    e.apply_decode_result(full);
    assert_eq!(shown(&e), [A, B, C, D], "its end joins it");
    e.apply_streamed_decode(found(&e, D, -5, DecodePass::Early, 6));
    assert_eq!(shown(&e), [D], "the next pass replaces it");
}

/// A decode streamed before a band change never folds: its period is shown under the band it was
/// heard on ([`Engine::fold_late`]), and what the stream wrote to ALL.TXT before the change is not
/// written again.
#[test]
fn a_band_change_drops_a_streamed_decode_and_writes_its_period_once() {
    let mut e = station();
    e.apply_streamed_decode(found(&e, A, -5, DecodePass::Early, 4));
    let late = found(&e, B, -5, DecodePass::Early, 4);
    let mut early = result(&e, &[A, B], DecodePass::Early, 4);
    early.heard_on = Some(HeardOn {
        band: "20m".into(),
        dial_mhz: 14.074,
        tier: Tier::Ft8,
        period_secs: 15.0,
    });
    e.set_frequency(21.074, "15m", "USB");
    assert_eq!(e.apply_streamed_decode(late), 0, "built before the change");
    assert!(matches!(
        e.apply_decode_result(early),
        DecodeApplied::Late { n: 2 }
    ));
    let rows = e.take_all_txt_pending();
    for m in [A, B] {
        assert_eq!(
            rows.iter().filter(|r| r.ends_with(m)).count(),
            1,
            "{m} written once: {rows:?}"
        );
    }
}

/// The operator's best-caller choice "strongest" compares every answer a pass brings: streamed
/// one at a time it would take the first, so the pass is folded whole when it ends and the
/// strongest is answered, as before. "First" streams: the first answer is taken at once.
#[test]
fn a_cq_run_choosing_the_strongest_caller_still_compares_the_whole_pass() {
    for (choice, answered) in [("strongest", "K2BBB"), ("first", "K1AAA")] {
        let mut e = station();
        e.settings.best_caller = choice.into();
        e.set_mode("qso-run").expect("premise: the CQ run starts");
        let weak = found(&e, "KD9TAW K1AAA FN31", -18, DecodePass::Boundary, 4);
        let strong = found(&e, "KD9TAW K2BBB FN42", 5, DecodePass::Boundary, 4);
        let first = e.apply_streamed_decode(weak);
        e.apply_streamed_decode(strong);
        let mut whole = result(
            &e,
            &["KD9TAW K1AAA FN31", "KD9TAW K2BBB FN42"],
            DecodePass::Boundary,
            4,
        );
        whole.decodes[0].snr = -18;
        whole.decodes[1].snr = 5;
        e.apply_decode_result(whole);
        assert_eq!(
            first,
            usize::from(choice == "first"),
            "{choice}: the first answer folded at once only when the choice is per decode"
        );
        assert_eq!(
            e.ft_next_over(6).as_deref().map(|t| t.split(' ').next()),
            Some(Some(answered)),
            "{choice}: {answered} answered"
        );
    }
}

/// An early pass past its give-up moment stops at its next sync candidate with what it found, and
/// the decoder is free again ([`DecodeJob::with_give_up_at`]).
#[test]
fn an_early_pass_past_its_give_up_moment_stops_at_its_next_candidate() {
    struct Count(usize, usize);
    impl DecodeProgress for Count {
        fn found(&mut self, _: StreamedDecode) {
            self.0 += 1;
        }
        fn candidate(&mut self) {
            self.1 += 1;
        }
    }
    let mode = modes::make_mode(modes::ModeKind::Ft8);
    let mut frame = vec![0f32; 180_000];
    for (i, msg) in [A, B, C, D].iter().enumerate() {
        let wave = mode.gen_wave(&mode.encode(msg), 12_000.0, 600.0 + 400.0 * i as f32);
        for (s, v) in frame.iter_mut().zip(wave) {
            *s += 0.05 * v;
        }
    }
    let e = station();
    let early = |e: &Engine| e.build_decode_job(frame.clone(), 2, DecodePass::Early);
    let mut whole = Count(0, 0);
    let all = run_decode_job_streaming(early(&e), &mut whole);
    assert_eq!(
        all.decodes().len(),
        4,
        "control: the whole pass decodes all four"
    );
    let mut cut = Count(0, 0);
    let past = Instant::now();
    let given_up = run_decode_job_streaming(early(&e).with_give_up_at(past), &mut cut);
    assert_eq!(
        cut.1, 1,
        "asked once, at the first candidate, and stopped there"
    );
    assert!(
        given_up.decodes().len() <= 1 && cut.0 == given_up.decodes().len(),
        "what it found before it stopped, and no more: {:?}",
        given_up.decodes()
    );
    assert!(
        whole.1 > 1,
        "control: the whole pass asked after every candidate"
    );
}
