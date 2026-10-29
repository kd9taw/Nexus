//! ⛔ FT8/FT4 NEVER KEY AN OVER THE ENCODER REFUSES, on every path that can queue an operator's
//! text: the typed free-text box, a clicked Tx row (Tx 5 as typed, or filled from a macro), the
//! same click during an over, the contest's Operate screen, and the keying point itself for
//! anything that reaches it anyway.
//!
//! The texts are the ones the packer has no frame for (`modes::wsjt77::Refusal`). Typed `TNX 73`
//! beside W9XYZ/M makes `W9XYZ/M K1ABC TNX 73` (beside a plain call `W9XYZ K1ABC TNX 73`), to which
//! WSJT-X 3.0.2's own `genft8` answers `*** bad message ***` and 79 zero tones (`lib/ft8/genft8.f90`
//! 20-26): Nexus keyed those zeros, 13.14 s of one steady carrier. `TNX` beside W9XYZ/M makes
//! `W9XYZ/M K1ABC TNX`, which `pack77_4` writes from a hash and a call it never set
//! (`lib/77bit/packjt77.f90` 1412-1456): Nexus keyed a frame that decodes as `<...>`, and as
//! `<...> ZZZZZZZZZ/` the next time. WSJT-X keys a Tx box's text as it stands, refused or not
//! (`widgets/mainwindow.cpp` 7802-7803 stop only on an empty message; 7903, 7952-7962); the
//! operator ruled that Nexus never keys such an over and says why.
//!
//! Texts that the packer carries are untouched, whatever they decode as: `W9XYZ K1ABC TNX` goes out
//! as the 13-character free text `W9XYZ K1ABC T`, exactly as WSJT-X sends the same text.

use super::contest_ft_messages_tests::air_form;
use super::ft_midover::FtSwitch;
use super::*;
use crate::dto::Tier;

const ME: &str = "K1ABC";
const MY_GRID: &str = "FN42";
const SNR: i32 = -12;

/// A decode of the partner's over, as the engine's decoder would deliver it.
fn heard(text: &str) -> modes::Decode {
    modes::Decode {
        message: text.into(),
        sync: 1.0,
        snr: SNR,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

fn tier_name(t: Tier) -> &'static str {
    match t {
        Tier::Ft4 => "FT4",
        _ => "FT8",
    }
}

/// Operate's FT8 or FT4 with no contest: this station double-clicks `cq` (as it comes off the air).
pub(super) fn answering(tier: Tier, cq: &str) -> Engine {
    let mut e = Engine::new(ME, MY_GRID, 0);
    e.set_tier(tier);
    let cq = air_form(cq).1;
    e.ingest_decodes_for_test(&[heard(&cq)], 2);
    let Msg::Cq { de, .. } = Msg::parse(&cq) else {
        panic!("premise: a CQ, {cq:?}")
    };
    e.work_station(&de, None, Some(&cq), Some(SNR), None)
        .expect("the double-click is taken");
    e
}

/// The "Now sending" readout.
fn tx_now(e: &Engine) -> Option<String> {
    e.snapshot().qso.and_then(|q| q.tx_now)
}

/// `slot`'s over as the radio loop makes it (plan, build, commit): the text planned, and whether
/// anything was keyed.
fn over(e: &mut Engine, slot: u64) -> Option<(String, bool)> {
    let plan = e.plan_tx(slot)?;
    let TxWaveform::Native { text, .. } = &plan.waveform else {
        panic!("an FT8/FT4 plan is a native waveform");
    };
    let text = text.clone();
    let wave = plan.waveform.build();
    Some((text, !e.commit_tx(&plan, wave, slot).is_empty()))
}

/// The first over planned from `from` on, within two of this station's slots.
pub(super) fn next_over(e: &mut Engine, from: u64) -> Option<(String, bool)> {
    (from..from + 4).find_map(|s| over(e, s))
}

/// A refused over goes nowhere: the typed text is not queued, and the next over is the reply that
/// was queued before, keyed. Controls: text the packer carries is queued and keyed as WSJT-X sends
/// it (`TNX` beside a plain call is the free text `W9XYZ K1ABC T`, the same bits WSJT-X keys).
#[test]
fn the_typed_box_never_queues_or_keys_text_the_encoder_refuses() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for (cq, typed) in [
            ("CQ W9XYZ/M", "TNX 73"),
            ("CQ W9XYZ/M", "TNX"),
            ("CQ W9XYZ/M", "73 GL"),
            ("CQ W9XYZ EN37", "TNX 73"),
            ("CQ W9XYZ EN37", "TNX FER QSO 73"),
        ] {
            let what = format!("{} {cq:?} typed {typed:?}", tier_name(tier));
            let mut e = answering(tier, cq);
            let queued = tx_now(&e).expect("premise: the reply is queued");
            e.qso_freetext(typed);
            assert_eq!(
                (tx_now(&e), next_over(&mut e, 4)),
                (Some(queued.clone()), Some((queued, true))),
                "{what}: never queued; the reply keys"
            );
        }
        for (cq, typed, sent) in [
            ("CQ W9XYZ EN37", "TNX", "W9XYZ K1ABC TNX"),
            ("CQ W9XYZ/M", "73", "W9XYZ/M <K1ABC> 73"),
        ] {
            let mut e = answering(tier, cq);
            e.qso_freetext(typed);
            assert_eq!(
                next_over(&mut e, 4),
                Some((sent.to_string(), true)),
                "CONTROL {} {cq:?} typed {typed:?}",
                tier_name(tier)
            );
        }
    }
}

/// A clicked Tx row whose over the encoder refuses (Tx 5 as typed, or filled from a macro) is not
/// queued, asks for no mid-over switch, and keys nothing; clicked on a station not being worked it
/// starts nothing either: no QSO, no TX. Controls: Tx 5 holding `TNX 73`, as WSJT-X's Tx 5 holds
/// it, keys as typed, and so does a long macro (on the air the free text's first 13 characters,
/// `TNX FER QSO 7`, as WSJT-X sends it).
#[test]
fn a_clicked_tx_row_the_encoder_refuses_is_never_queued_or_keyed() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for (cq, dx, row) in [
            ("CQ W9XYZ/M", "W9XYZ/M", "W9XYZ/M K1ABC TNX"),
            ("CQ W9XYZ/M", "W9XYZ/M", "W9XYZ/M K1ABC TNX 73"),
            ("CQ W9XYZ EN37", "W9XYZ", "W9XYZ K1ABC TNX 73"),
            ("CQ W9XYZ EN37", "W9XYZ", "W9XYZ K1ABC 73 GL"),
        ] {
            let what = format!("{} {cq:?} row {row:?}", tier_name(tier));
            let mut e = answering(tier, cq);
            let queued = tx_now(&e).expect("premise: the reply is queued");
            let _ = e.take_ft_restart();
            e.override_next_tx(dx, None, row);
            assert_eq!(
                (tx_now(&e), e.take_ft_restart(), next_over(&mut e, 4)),
                (Some(queued.clone()), false, Some((queued, true))),
                "{what}: never queued; no switch asked; the reply keys"
            );

            // The same click on a station this one is not working, its CQ heard.
            let mut e = Engine::new(ME, MY_GRID, 0);
            e.set_tier(tier);
            e.set_mode("qso-monitor").expect("premise: S&P");
            e.ingest_decodes_for_test(&[heard(&air_form(cq).1)], 2);
            e.settings.double_click_sets_tx = true;
            e.set_tx_enabled(false);
            e.override_next_tx(dx, None, row);
            assert_eq!(
                (
                    e.snapshot().qso.and_then(|q| q.dxcall),
                    e.tx_enabled(),
                    next_over(&mut e, 0)
                ),
                (None, false, None),
                "{what}, not worked: nothing started"
            );
        }
        for row in ["TNX 73", "TNX FER QSO 73 GL"] {
            let mut e = answering(tier, "CQ W9XYZ/M");
            e.override_next_tx("W9XYZ/M", None, row);
            assert_eq!(
                next_over(&mut e, 4),
                Some((row.to_string(), true)),
                "CONTROL {} row {row:?}",
                tier_name(tier)
            );
        }
    }
}

/// ARRL Field Day running on Operate's contest screen: the caller's exchange is in, and a Tx row
/// clicked with text the encoder refuses neither leaves the contest nor keys; the run answers the
/// caller with its rogered exchange. The typed box is not the contest's (a no-op there).
#[test]
fn the_contest_screen_never_keys_a_row_the_encoder_refuses() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        let what = tier_name(tier);
        let mut e = Engine::new(ME, MY_GRID, 0);
        let mut s = e.settings().clone();
        s.fd_event = "arrlfd".into();
        s.fd_class = "2B".into();
        s.fd_section = "EMA".into();
        s.contest_qth_state = "MA".into();
        s.tx_even = true;
        s.fd_active = true;
        e.apply_settings(s);
        e.set_tier(tier);
        e.set_mode("fieldday-run").expect("premise: the run");
        assert!(
            next_over(&mut e, 0).is_some_and(|(cq, keyed)| keyed && cq.starts_with("CQ ")),
            "{what}: premise, the run's CQ"
        );
        e.hear_for_test("K1ABC W9XYZ 1D WI", SNR, 2);
        e.override_next_tx("W9XYZ", None, "W9XYZ K1ABC TNX 73");
        e.qso_freetext("TNX 73");
        assert_eq!(
            (
                matches!(e.mode, Mode::FieldDay { .. }),
                next_over(&mut e, 2)
            ),
            (true, Some(("W9XYZ K1ABC R 2B EMA".to_string(), true))),
            "{what}: still the contest; its exchange keys"
        );
    }
}

/// During an over (FT8, the Tx1 to W9XYZ on the air in slot 4): a Tx-row click with text the encoder
/// refuses asks for no switch, and asked anyway it finds nothing to switch to. And where a refused
/// text is queued by any other way, the switch never builds it into the rest of the over (it used to
/// switch the rest to 79 zero tones, the steady carrier): the over goes out as it began.
#[test]
fn an_over_on_the_air_is_never_switched_to_text_the_encoder_refuses() {
    let mut e = answering(Tier::Ft8, "CQ W9XYZ EN37");
    let tx1 = next_over(&mut e, 4).expect("premise: our Tx1 on the air");
    assert_eq!(tx1, ("W9XYZ K1ABC FN42".to_string(), true), "premise");
    let _ = e.take_ft_restart();
    e.override_next_tx("W9XYZ", None, "W9XYZ K1ABC TNX 73");
    let asked = e.take_ft_restart();
    let mut handed = None;
    let switched = e.ft_midover_switch(4, 4.0 * 15_000.0 + 4_000.0, true, |tail| {
        handed = Some(tail(40_000, 0));
        true
    });
    assert_eq!(
        (asked, switched, handed.is_some()),
        (false, None, false),
        "the click: no switch asked, none made"
    );

    // Queued by another way (a sequencer, a future path), straight into the station.
    let Mode::Qso { station, .. } = &mut e.mode else {
        panic!("premise: a QSO");
    };
    station.override_next(Msg::Other("W9XYZ K1ABC TNX 73".into()));
    let mut handed = None;
    let switched = e.ft_midover_switch(4, 4.0 * 15_000.0 + 5_000.0, true, |tail| {
        handed = Some(tail(40_000, 0));
        true
    });
    assert_eq!(
        (switched, handed.is_some()),
        (
            Some(FtSwitch::NotReplaced {
                from: "W9XYZ K1ABC FN42".into(),
                to: "W9XYZ K1ABC TNX 73".into()
            }),
            false
        ),
        "queued another way: the over goes out as it began"
    );
}

/// The keying point, for an over that reaches it however it was queued: the QSO's next over, and a
/// frame in the chat broadcast queue on an FT tier. Nothing is
/// keyed, and nothing records an over that never went: no ALL.TXT line.
#[test]
fn the_keying_point_never_keys_an_over_the_encoder_refuses() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        let what = tier_name(tier);
        let mut e = answering(tier, "CQ W9XYZ/M");
        e.settings.write_all_txt = true;
        let _ = e.take_all_txt_pending();
        let Mode::Qso { station, .. } = &mut e.mode else {
            panic!("premise: a QSO");
        };
        station.override_next(Msg::Other("W9XYZ/M K1ABC TNX".into()));
        assert_eq!(
            (next_over(&mut e, 4), e.take_all_txt_pending().len()),
            (None, 0),
            "{what}: the QSO's over"
        );

        let mut e = Engine::new(ME, MY_GRID, 0);
        e.set_tier(tier);
        e.settings.write_all_txt = true;
        e.set_tx_enabled(true);
        e.broadcast_queue.push_back("W9XYZ/M K1ABC/P".into());
        assert_eq!(
            (next_over(&mut e, 0), e.take_all_txt_pending().len()),
            (None, 0),
            "{what}: a chat broadcast frame"
        );
        e.broadcast_queue.push_back("W9XYZ K1ABC".into());
        assert_eq!(
            next_over(&mut e, 0),
            Some(("W9XYZ K1ABC".to_string(), true)),
            "CONTROL {what}: a frame the packer carries"
        );
    }
}

// ---- What the operator is told ------------------------------------------------------------------

/// The refusal in the operator's words, the over's text as it would have gone: what the typed box's
/// command and the Tx row's command return, and the native screen shows (`withErrorToast`). Text the
/// packer carries is no refusal.
#[test]
fn the_refusal_says_why_in_plain_words() {
    let words = |mode: &str, text: &str| {
        format!(
            "{mode} cannot encode \"{text}\", so nothing goes out. After two calls only a report \
             (-12, R-12), RRR, RR73 or 73 fits; free text alone, as in Tx 5, holds 13 characters."
        )
    };
    for tier in [Tier::Ft8, Tier::Ft4] {
        let mode = tier_name(tier);
        let mut e = answering(tier, "CQ W9XYZ/M");
        assert_eq!(
            e.qso_freetext_checked("TNX 73"),
            Err(words(mode, "W9XYZ/M K1ABC TNX 73")),
            "{mode}: the typed box"
        );
        assert_eq!(
            e.override_next_tx_refusal("W9XYZ/M", "W9XYZ/M K1ABC TNX"),
            Some(words(mode, "W9XYZ/M K1ABC TNX")),
            "{mode}: the Tx row"
        );
        assert_eq!(
            (
                e.override_next_tx_refusal("W9XYZ/M", "TNX 73"),
                e.qso_freetext_checked("73")
            ),
            (None, Ok(())),
            "CONTROL {mode}"
        );
    }
}

// ---- Q65, MSK144 and FST4 -----------------------------------------------------------------------

/// ⛔ Q65, MSK144 and FST4 never queue or key a text `pack77` writes no frame for
/// (`modes::wsjt77::q65_refusal`). Their vendored encoders (`genq65`, `genmsk_128_90`, `genfst4`)
/// read that unwritten frame with no `err=`: the first time, gfortran stops the whole process
/// (`Bad value during integer read`), which no Rust guard can catch; after an earlier over they
/// send that over's frame. Typed `TNX 73` beside W9XYZ makes `W9XYZ K1ABC TNX 73`, one of them.
/// The refused over is never built here: on the old code building it stopped the test process.
/// Controls: text the packer carries is queued and keyed (`TNX` beside a plain call is the free
/// text `W9XYZ K1ABC T`, as WSJT-X sends it).
#[test]
fn q65_msk144_and_fst4_never_queue_or_key_a_text_with_no_frame() {
    let words = |mode: &str, text: &str| {
        format!(
            "{mode} cannot encode \"{text}\", so nothing goes out. After two calls only a report \
             (-12, R-12), RRR, RR73 or 73 fits; free text alone, as in Tx 5, holds 13 characters."
        )
    };
    for (tier, mode) in [
        (Tier::Q65, "Q65"),
        (Tier::Msk144, "MSK144"),
        (Tier::Fst4, "FST4"),
    ] {
        let mut e = answering(tier, "CQ W9XYZ EN37");
        let queued = tx_now(&e).expect("premise: the reply is queued");
        assert_eq!(
            (e.qso_freetext_checked("TNX 73"), tx_now(&e)),
            (Err(words(mode, "W9XYZ K1ABC TNX 73")), Some(queued)),
            "{mode}: the typed box"
        );
        assert_eq!(
            e.override_next_tx_refusal("W9XYZ", "W9XYZ K1ABC 73 GL"),
            Some(words(mode, "W9XYZ K1ABC 73 GL")),
            "{mode}: the Tx row"
        );
        assert_eq!(
            e.qso_freetext_checked("TNXÉ 73"),
            Err(format!(
                "{mode} cannot encode \"W9XYZ K1ABC TNXÉ 73\", so nothing goes out. It holds a \
                 character other than plain letters, digits and punctuation."
            )),
            "{mode}: a character the packer is not judged on"
        );

        // The keying point, for an over that reaches it anyway: nothing planned, nothing recorded.
        e.settings.write_all_txt = true;
        let _ = e.take_all_txt_pending();
        let Mode::Qso { station, .. } = &mut e.mode else {
            panic!("premise: a QSO");
        };
        station.override_next(Msg::Other("W9XYZ K1ABC TNX 73".into()));
        assert_eq!(
            (
                (4..8).find_map(|s| e.plan_tx(s)).is_none(),
                e.take_all_txt_pending().len()
            ),
            (true, 0),
            "{mode}: the keying point"
        );

        let mut e = answering(tier, "CQ W9XYZ EN37");
        assert_eq!(e.qso_freetext_checked("TNX"), Ok(()), "CONTROL {mode}");
        assert_eq!(
            next_over(&mut e, 4),
            Some(("W9XYZ K1ABC TNX".to_string(), true)),
            "CONTROL {mode}: keyed"
        );
    }
}
