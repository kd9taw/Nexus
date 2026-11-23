//! An edited Tx6 goes on the air as the operator typed it, as WSJT-X's does, until WSJT-X would
//! write the box again.
//!
//! WSJT-X 3.0.2 keys the text of the Tx6 box itself: `MainWindow::guiUpdate` reads
//! `ui->tx6->text()` for the over (`widgets/mainwindow.cpp` 7801 and 7904) and hands it to the
//! packer, a CQ or free text alike. Editing the box keeps only its second word as the CQ type
//! (`on_tx6_editingFinished`, 10075-10086), and `genCQMsg` (9466) writes the box again as that
//! type, the Settings call and four characters of the Settings grid whenever `genStdMsgs` runs
//! (9566): when a caller is answered (`processMessage`, 9444), on Clear DX (9830), Generate Std
//! Msgs (10505), a report change (12197), and on a band or frequency change (12520, 13858). So the
//! typed text goes out until a station answers; the return to CQ after that contact is the
//! regenerated one. These tests hold the engine to that: what the screen shows, what keys, and the
//! 77 bits the packer makes of the typed text. The box itself is the cockpit's (its own tests).

use super::contest_ft_messages_tests::{air_form, bits77, engine_for, keyed};
use super::*;

const ME: &str = "K1ABC";
/// The Settings locator, which the typed Tx6 must not be replaced with.
const SETTINGS_GRID: &str = "FN31";

/// A station on 20 m FT8 with the Settings locator, TX on the even slots.
fn station(tier: Tier) -> Engine {
    let mut e = Engine::new(ME, SETTINGS_GRID, 0);
    e.set_tier(tier);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e
}

/// The 77 bits FT8 sends for `text`.
fn ft8_bits(text: &str) -> Vec<i8> {
    bits77(&modes::make_mode(modes::ModeKind::Ft8).encode(text)).expect("FT8 bits")
}

/// What the screen shows as the next over.
fn tx_now(e: &Engine) -> Option<String> {
    e.snapshot().qso.and_then(|q| q.tx_now)
}

/// ⭐ THE TYPED TX6 IS THE MESSAGE. A Tx6 of `CQ DX K1ABC FN42` with `FN31` in Settings shows and
/// keys `CQ DX K1ABC FN42` on FT8 and FT4, a standard CQ that decodes back verbatim, and the bits
/// on the air are the typed text's, not the Settings CQ's.
#[test]
fn an_edited_tx6_goes_on_the_air_as_typed() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        let mut e = station(tier);
        e.start_cq_as_typed("CQ DX K1ABC FN42")
            .expect("a directed run");
        let want = "CQ DX K1ABC FN42";
        assert_eq!(tx_now(&e).as_deref(), Some(want), "{tier:?}: the screen");
        assert_eq!(
            keyed(&mut e, 0).as_deref(),
            Some(want),
            "{tier:?}: the over"
        );
    }
    assert_eq!(
        air_form("CQ DX K1ABC FN42"),
        ("i3=1".to_string(), "CQ DX K1ABC FN42".to_string()),
        "a standard CQ on the air"
    );
    assert_ne!(
        ft8_bits("CQ DX K1ABC FN42"),
        ft8_bits("CQ DX K1ABC FN31"),
        "premise: the typed grid is in the bits"
    );
}

/// A typed CQ that is not directed, and one typed with no grid at all, are sent as typed too:
/// WSJT-X keys the box, and `CQ DX K1ABC` packs as a standard CQ with a blank grid.
#[test]
fn a_plain_and_a_gridless_tx6_go_on_the_air_as_typed() {
    for typed in ["CQ K1ABC FN42", "CQ DX K1ABC", "CQ K1ABC"] {
        let mut e = station(Tier::Ft8);
        e.start_cq_as_typed(typed).expect("a run");
        assert_eq!(tx_now(&e).as_deref(), Some(typed), "{typed}: the screen");
        assert_eq!(
            keyed(&mut e, 0).as_deref(),
            Some(typed),
            "{typed}: the over"
        );
        assert_eq!(
            air_form(typed),
            ("i3=1".to_string(), typed.to_string()),
            "{typed}: a standard CQ"
        );
    }
}

/// Lower case and extra spaces are the same message: the packer upper-cases and collapses the
/// text (`split77`), so the screen shows what goes out.
#[test]
fn a_tx6_typed_in_lower_case_shows_what_is_packed() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("  cq dx  k1abc fn42 ").expect("a run");
    assert_eq!(tx_now(&e).as_deref(), Some("CQ DX K1ABC FN42"));
}

/// A six-character grid typed into Tx6 goes out as four, which is all the packer reads of it
/// (`packjt77.f90` `pack77_1`, `is_grid4(w(nwords)(1:4))`); WSJT-X's TX line shows the text the
/// packer sent back (`msgsent`), so Nexus shows and logs four characters as well, and the bits are
/// the bits of the six-character text.
#[test]
fn a_six_character_grid_typed_into_tx6_goes_out_as_four() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ DX K1ABC FN42ab").expect("a run");
    assert_eq!(tx_now(&e).as_deref(), Some("CQ DX K1ABC FN42"));
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ DX K1ABC FN42"));
    assert_eq!(ft8_bits("CQ DX K1ABC FN42"), ft8_bits("CQ DX K1ABC FN42AB"));
}

/// The typed CQ is called again on every over nobody answers, as WSJT-X keys the same Tx6 box
/// each period.
#[test]
fn the_typed_cq_repeats_until_answered() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ DX K1ABC FN42").expect("a run");
    for slot in [0, 2, 4] {
        assert_eq!(
            keyed(&mut e, slot).as_deref(),
            Some("CQ DX K1ABC FN42"),
            "slot {slot}"
        );
    }
}

/// The bits on the air are the typed text's: the over keyed packs to exactly what the text packs
/// to, on FT8.
#[test]
fn the_packed_bits_are_the_typed_texts() {
    for typed in ["CQ DX K1ABC FN42", "CQ K1ABC FN42", "TNX QSO 73"] {
        let mut e = station(Tier::Ft8);
        e.start_cq_as_typed(typed).expect("a run");
        let sent = keyed(&mut e, 0).expect("an over");
        assert_eq!(ft8_bits(&sent), ft8_bits(typed), "{typed}");
    }
}

/// Work a contact from the run: a caller, our report, their roger, our RR73, and the next period.
fn work_a_contact(e: &mut Engine) {
    e.hear_for_test("K1ABC W9XYZ EN37", -12, 2);
    assert_eq!(
        keyed(e, 2).as_deref(),
        Some("W9XYZ K1ABC -12"),
        "premise: the report"
    );
    e.hear_for_test("K1ABC W9XYZ R-05", -12, 4);
    assert_eq!(
        keyed(e, 4).as_deref(),
        Some("W9XYZ K1ABC RR73"),
        "premise: the RR73"
    );
    e.hear_for_test("CQ N0OTH EM48", -3, 6);
}

/// ⭐ THE RETURN TO CQ IS WSJT-X'S REGENERATED TX6. Answering the caller runs `genStdMsgs`
/// (`processMessage`, mainwindow.cpp 9444), whose `genCQMsg` (9466) writes Tx6 again as the typed
/// CQ type, the Settings call and the Settings grid cut to four: the direction is kept, the typed
/// grid is not.
#[test]
fn the_return_to_cq_after_a_contact_keeps_the_direction_and_takes_the_settings_grid() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ DX K1ABC FN42").expect("a run");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ DX K1ABC FN42"));
    work_a_contact(&mut e);
    assert_eq!(keyed(&mut e, 6).as_deref(), Some("CQ DX K1ABC FN31"));
}

/// Clear DX is `genStdMsgs` too (`clearDX`, mainwindow.cpp 9830): the run calls the regenerated
/// CQ, the direction kept.
#[test]
fn clear_dx_keeps_the_direction_and_takes_the_settings_grid() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ DX K1ABC FN42").expect("a run");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ DX K1ABC FN42"));
    e.hear_for_test("K1ABC W9XYZ EN37", -12, 2);
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("W9XYZ K1ABC -12"),
        "premise"
    );
    // Clear DX turns Enable Tx off, as WSJT-X's does; the operator turns it back on.
    e.clear_dx_call();
    e.set_tx_enabled_by_operator(true);
    assert_eq!(keyed(&mut e, 4).as_deref(), Some("CQ DX K1ABC FN31"));
}

/// ⭐ A TX6 THAT IS NO CQ GOES OUT AS FREE TEXT, as WSJT-X keys any Tx6 that packs. This reverses
/// the earlier refusal of such a row. A free-text frame holds 13 characters (`packtext77`), so a
/// longer row goes out as its first 13, and that is what the screen shows, as WSJT-X's TX line
/// shows the `msgsent` the packer read back.
#[test]
fn a_tx6_that_is_no_cq_goes_out_as_free_text() {
    for (typed, on_air) in [
        ("TNX QSO 73", "TNX QSO 73"),
        ("TNX FOR THE QSO 73", "TNX FOR THE Q"),
    ] {
        let mut e = station(Tier::Ft8);
        e.start_cq_as_typed(typed).expect("free text goes out");
        assert_eq!(tx_now(&e).as_deref(), Some(on_air), "{typed}: the screen");
        assert_eq!(
            keyed(&mut e, 0).as_deref(),
            Some(on_air),
            "{typed}: the over"
        );
        assert_eq!(
            air_form(typed),
            ("i3=0 n3=0".to_string(), on_air.to_string()),
            "{typed}: free text on the air"
        );
    }
}

/// While Tx6 is free text the station is still calling CQ (WSJT-X: `m_bCallingCQ = 6 == m_ntx`,
/// mainwindow.cpp 8079), so a caller is answered; the return to CQ is then the regenerated one,
/// whose CQ type is the free text's second word where it is one a CQ can carry (`TNX QSO 73` gives
/// `CQ QSO`, as `on_tx6_editingFinished` reads it, 10082), else a plain CQ.
#[test]
fn a_free_text_tx6_answers_a_caller_and_returns_to_the_regenerated_cq() {
    for (typed, back) in [
        ("TNX QSO 73", "CQ QSO K1ABC FN31"),
        ("73 TO ALL 2026", "CQ TO K1ABC FN31"),
        ("QRT 73 TNX", "CQ K1ABC FN31"),
    ] {
        let mut e = station(Tier::Ft8);
        e.start_cq_as_typed(typed).expect("free text goes out");
        assert!(keyed(&mut e, 0).is_some(), "{typed}: premise");
        work_a_contact(&mut e);
        assert_eq!(keyed(&mut e, 6).as_deref(), Some(back), "{typed}");
    }
}

/// ⚠️ THE ONE DELIBERATE DIFFERENCE FROM WSJT-X: a CQ under somebody else's call is refused before
/// anything keys, with the reason, and the station is left as it was. WSJT-X would send it. An
/// empty row sends nothing (WSJT-X stops TX on one, mainwindow.cpp 7802-7803), and a row the
/// encoder cannot make a frame of is refused as Tx 5's is.
#[test]
fn a_cq_under_another_call_an_empty_row_and_an_unencodable_one_are_refused() {
    for typed in [
        "CQ DX W1XYZ FN42",
        "CQ W1XYZ FN42",
        "CQ W1XYZ",
        "CQ W1XYZ POTA",
        "CQ POTA W1XYZ",
        "",
        "   ",
        "TNX É 73",
    ] {
        let mut e = station(Tier::Ft8);
        let why = e
            .start_cq_as_typed(typed)
            .expect_err(&format!("{typed:?} is refused"));
        assert!(!why.is_empty(), "{typed:?}: a reason is given");
        assert_eq!(tx_now(&e), None, "{typed:?}: no CQ run");
        assert_eq!(keyed(&mut e, 0), None, "{typed:?}: nothing keys");
    }
}

/// The CQ goes out under the Settings call exactly: a portable form of it typed into Tx6
/// (`CQ K1ABC/P`) is refused with the reason rather than accepted and then held at the key. A
/// grid and the word RR73 are no callsign to this check.
#[test]
fn only_the_settings_call_calls_cq() {
    let mut e = station(Tier::Ft8);
    assert!(e.start_cq_as_typed("CQ K1ABC/P").is_err());
    assert_eq!(keyed(&mut e, 0), None);
    e.start_cq_as_typed("CQ K1ABC FN42")
        .expect("a grid is no call");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ K1ABC FN42"));
}

/// `CQ <MYCALL> POTA`, the word order the packer does not read as a directed CQ, goes out as
/// WSJT-X's packer makes it: `pack77_1` reads the call and drops the word it cannot place, so a
/// receiving station decodes `CQ K1ABC`, and that is what the screen shows, as WSJT-X's TX line
/// shows `msgsent`.
#[test]
fn cq_mycall_pota_goes_out_as_the_packer_makes_it() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ K1ABC POTA").expect("it packs");
    let (_, decoded) = air_form("CQ K1ABC POTA");
    assert_eq!(
        tx_now(&e),
        Some(decoded.clone()),
        "the screen shows what is sent"
    );
    assert_eq!(keyed(&mut e, 0), Some(decoded));
}

/// The typed CQ goes through every gate a CQ run does. The identity backstop at the key wants the
/// station's own callsign and Settings grid before any standard message goes out, and a grid typed
/// into Tx6 does not stand in for a Settings grid that is missing.
#[test]
fn a_typed_cq_passes_the_identity_gate() {
    let mut e = Engine::new(ME, "", 0);
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    let _ = e.start_cq_as_typed("CQ DX K1ABC FN42");
    assert_eq!(
        keyed(&mut e, 0),
        None,
        "nothing keys without a Settings grid"
    );
}

/// A start that brings no text, the Remote page's Call CQ (`start_cq` with only the direction of
/// the CQ it shows), calls the Settings CQ: the typed grid is the Tx6 row's and nobody else's. The
/// strip's Call CQ sends the row ([`call_cq_outside_a_contest_sends_tx6_as_typed`]).
#[test]
fn a_start_with_no_text_calls_the_settings_cq() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ DX K1ABC FN42").expect("a run");
    e.start_cq(None).expect("a plain run");
    assert_eq!(tx_now(&e).as_deref(), Some("CQ K1ABC FN31"));
    e.start_cq(Some("DX")).expect("a directed run");
    assert_eq!(tx_now(&e).as_deref(), Some("CQ DX K1ABC FN31"));
}

/// ⭐ CALL CQ IS TX6. In WSJT-X calling CQ is transmitting the Tx6 box (`m_ntx = 6`, then
/// `ui->tx6->text()`, mainwindow.cpp 7801), so the strip's Call CQ sends the row's text as Tx 6
/// does: outside a contest, the typed CQ and free text alike.
#[test]
fn call_cq_outside_a_contest_sends_tx6_as_typed() {
    for (typed, on_air) in [
        ("CQ DX K1ABC FN42", "CQ DX K1ABC FN42"),
        ("CQ K1ABC FN31", "CQ K1ABC FN31"),
        ("TNX QSO 73", "TNX QSO 73"),
    ] {
        let mut e = station(Tier::Ft8);
        e.start_running_as_typed(typed).expect("a run");
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(on_air), "{typed}");
    }
    let mut e = station(Tier::Ft8);
    assert!(
        e.start_running_as_typed("CQ DX W1XYZ FN42").is_err(),
        "another call"
    );
    assert_eq!(keyed(&mut e, 0), None);
}

/// In a contest WSJT-X has no activity for, WSJT-X is in its normal mode and keys Tx6 as typed:
/// the contest's Running calls the row's text, grid and all, and the contact is the contest's.
/// The CQ word stays the Running's direction, which the CQ after a contact is built from.
#[test]
fn in_a_contest_wsjtx_has_no_activity_for_the_running_calls_tx6_as_typed() {
    let mut e = engine_for("txqp", SETTINGS_GRID);
    e.set_tier(Tier::Ft8);
    assert!(e.contest_cq_typed(), "premise: the row is the operator's");
    e.start_running_as_typed("CQ QP K1ABC FN42")
        .expect("the run starts");
    assert!(
        matches!(e.mode, Mode::FieldDay { .. }),
        "the contest's Running"
    );
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ QP K1ABC FN42"));
    assert_eq!(
        e.cq_dir.as_deref(),
        Some("QP"),
        "the CQ word, for the CQ after a contact"
    );
    // The Contest screen's Running, entered again, calls the CQ rebuilt from that word.
    e.set_mode("fieldday-sp").expect("S&P");
    e.set_mode("fieldday-run").expect("Running");
    assert_eq!(keyed(&mut e, 2).as_deref(), Some("CQ QP K1ABC FN31"));
    // A CQ under another call is refused there too, and leaves the direction as it was.
    assert!(e.start_running_as_typed("CQ QP W1XYZ FN42").is_err());
    assert_eq!(e.cq_dir.as_deref(), Some("QP"));
}

/// In a contest WSJT-X has an activity for, Tx6 is the activity's CQ (`genCQMsg` writes
/// `CQ FD <call> <grid4>`, mainwindow.cpp 9504-9527), which the cockpit shows read-only: that is
/// what Call CQ calls, whatever text comes with it.
#[test]
fn in_a_contest_wsjtx_has_an_activity_for_call_cq_calls_the_activitys_cq() {
    let mut e = engine_for("arrlfd", SETTINGS_GRID);
    e.set_tier(Tier::Ft8);
    e.start_running_as_typed("CQ DX K1ABC FN42")
        .expect("the run starts");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ FD K1ABC FN31"));
}

/// ⛔ NO OVER GOES OUT AS ANOTHER STATION, whatever box it was typed in. A structured frame is read
/// by every receiver as sent by its sender word, and `QRZ` and `DE` name a sender as `CQ` does, so
/// each of these would put another station's call on the air as the sender; each is refused before
/// anything changes. WSJT-X would key them all; this is the identity guard's one difference.
const AS_ANOTHER_STATION: [&str; 9] = [
    "W1XYZ N0OTH FN42",
    "QRZ W1XYZ FN42",
    "DE W1XYZ FN42",
    "DE W1XYZ",
    "W9XYZ W1XYZ RR73",
    "W9XYZ W1XYZ -12",
    "<W9XYZ> W1XYZ",
    "W9XYZ W1XYZ R 2A EMA",
    "CQ DX W1XYZ FN42",
];

#[test]
fn tx6_never_goes_out_as_another_station() {
    for typed in AS_ANOTHER_STATION {
        let mut e = station(Tier::Ft8);
        let why = e
            .start_cq_as_typed(typed)
            .expect_err(&format!("{typed:?} is refused"));
        assert!(why.contains("not as your call K1ABC"), "{typed:?}: {why}");
        assert_eq!(tx_now(&e), None, "{typed:?}: no run");
        assert_eq!(keyed(&mut e, 0), None, "{typed:?}: nothing keys");
    }
    // CONTROL: the same frames from our own call go out as typed, as WSJT-X sends them.
    for typed in [
        "W9XYZ K1ABC RR73",
        "QRZ K1ABC FN42",
        "DE K1ABC FN42",
        "W9XYZ K1ABC -12",
    ] {
        let mut e = station(Tier::Ft8);
        e.start_cq_as_typed(typed).expect(typed);
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(typed), "{typed}");
    }
}

/// Each half of the guard is needed alone. The TYPED text: a free-text frame keeps 13
/// characters, so `CQ POTA DE W1XYZ` goes out as `CQ POTA DE W1`, and only what was typed still
/// names W1XYZ. The FRAME: `W1XYZ N0OTH FN42` opens with no CQ, QRZ or DE, and only the frame it
/// packs to (N0OTH calling W1XYZ) shows whose call it carries.
#[test]
fn each_half_of_the_identity_guard_is_needed() {
    assert_eq!(
        air_form("CQ POTA DE W1XYZ"),
        ("i3=0 n3=0".to_string(), "CQ POTA DE W1".to_string()),
        "premise: the frame no longer names W1XYZ"
    );
    let mut e = station(Tier::Ft8);
    assert!(
        e.start_cq_as_typed("CQ POTA DE W1XYZ").is_err(),
        "the typed half"
    );
    assert_eq!(
        air_form("W1XYZ N0OTH FN42").0,
        "i3=1",
        "premise: a standard frame"
    );
    let mut e = station(Tier::Ft8);
    assert!(
        e.start_cq_as_typed("W1XYZ N0OTH FN42").is_err(),
        "the frame half"
    );
}

/// The same guard holds Tx 5: a Tx-slot press, the free-text Tx 5, and a logger's FreeText, each
/// refused with the reason when the over would go out as another station.
#[test]
fn tx5_never_goes_out_as_another_station() {
    for typed in ["W1XYZ N0OTH FN42", "DE W1XYZ FN42", "W9XYZ W1XYZ RR73"] {
        let mut e = station(Tier::Ft8);
        assert!(
            e.override_next_tx_checked("W1XYZ", None, typed).is_err(),
            "{typed}: a Tx-slot press"
        );
        assert_eq!(keyed(&mut e, 0), None, "{typed}: nothing keys");
        let mut e = station(Tier::Ft8);
        e.start_cq(None).expect("a run");
        assert!(e.qso_freetext_checked(typed).is_err(), "{typed}: free text");
        e.set_tx_enabled_by_operator(true);
        assert!(
            e.udp_free_text(typed, true).is_err(),
            "{typed}: a logger's FreeText"
        );
        assert_eq!(
            keyed(&mut e, 0).as_deref(),
            Some("CQ K1ABC FN31"),
            "{typed}: the run's CQ is untouched"
        );
    }
    // CONTROL: our own overs, and free text that names no sender.
    let mut e = station(Tier::Ft8);
    e.override_next_tx_checked("W1XYZ", None, "W1XYZ K1ABC FN31")
        .expect("our Tx1");
    let mut e = station(Tier::Ft8);
    e.start_cq(None).expect("a run");
    e.qso_freetext_checked("TNX 73 GL").expect("free text");
}

/// A start the run refuses leaves no typed CQ behind: the next run, entered another way, calls
/// the CQ built from the CQ word and Settings, not the typed grid.
#[test]
fn a_refused_start_leaves_no_typed_cq_behind() {
    let mut e = station(Tier::Wspr);
    assert!(
        e.start_cq_as_typed("CQ DX K1ABC FN42").is_err(),
        "premise: no CQ run on a beacon"
    );
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-run").expect("the screen's run");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ DX K1ABC FN31"));
}

/// ⭐ THE BOX WSJT-X WRITES AGAIN MID-RUN IS KEYED AT THE NEXT OVER. Generate Std Msgs (10505), a
/// frequency change (12520, 13858) or a mode switch (10803) rewrite the Tx6 box while the station
/// calls CQ; WSJT-X keys the box at the next period, so Nexus does, and the box and the TX line
/// agree. Outside calling CQ it changes nothing, and a rewrite as another station is refused.
#[test]
fn a_tx6_rewritten_mid_run_is_keyed_at_the_next_over() {
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("CQ DX K1ABC FN42").expect("a run");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ DX K1ABC FN42"));
    // A QSY mid-run: the cockpit writes the box again and hands it over.
    e.set_frequency(14.080, "20m", "USB");
    e.rewrite_cq("CQ DX K1ABC FN31").expect("rewritten");
    assert_eq!(
        tx_now(&e).as_deref(),
        Some("CQ DX K1ABC FN31"),
        "the TX line"
    );
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("CQ DX K1ABC FN31"),
        "the next over"
    );
    // As another station: refused, and the over stands.
    assert!(e.rewrite_cq("CQ DX W1XYZ FN31").is_err());
    assert_eq!(e.cq_dir.as_deref(), Some("DX"), "the CQ word stands");
    assert_eq!(keyed(&mut e, 4).as_deref(), Some("CQ DX K1ABC FN31"));
    // In a contact the box is not the over: nothing changes.
    e.hear_for_test("K1ABC W9XYZ EN37", -12, 6);
    assert_eq!(
        keyed(&mut e, 6).as_deref(),
        Some("W9XYZ K1ABC -12"),
        "premise"
    );
    e.rewrite_cq("CQ DX K1ABC FN31").expect("no-op");
    assert_eq!(tx_now(&e).as_deref(), Some("W9XYZ K1ABC -12"));
    // Not running: nothing starts.
    let mut e = station(Tier::Ft8);
    e.rewrite_cq("CQ DX K1ABC FN31").expect("no-op");
    assert_eq!(tx_now(&e), None);
}

/// ⛔ The DXpedition frame (`i3=0 n3=1`) is the Fox's, sent by its hashed third call: typed with
/// another station's call there it would go out as that Fox, so Tx 6, a Tx-slot press and a
/// logger's FreeText each refuse it. From our own call it goes out.
#[test]
fn the_dxpedition_frame_never_goes_out_as_another_station() {
    let fox = "W9XYZ RR73; N0OTH <W1XYZ> -12";
    assert_eq!(
        air_form(fox).0,
        "i3=0 n3=1",
        "premise: the DXpedition frame"
    );
    let mut e = station(Tier::Ft8);
    assert!(e.start_cq_as_typed(fox).is_err(), "Tx 6");
    assert_eq!(keyed(&mut e, 0), None);
    let mut e = station(Tier::Ft8);
    assert!(
        e.override_next_tx_checked("W9XYZ", None, fox).is_err(),
        "a Tx-slot press"
    );
    assert_eq!(keyed(&mut e, 0), None);
    let mut e = station(Tier::Ft8);
    e.start_cq(None).expect("a run");
    e.set_tx_enabled_by_operator(true);
    assert!(e.udp_free_text(fox, true).is_err(), "a logger's FreeText");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ K1ABC FN31"));
    // CONTROL: our own call as the Fox.
    let mut e = station(Tier::Ft8);
    e.start_cq_as_typed("W9XYZ RR73; N0OTH <K1ABC> -12")
        .expect("our own frame");
}

/// A rewritten box changes the over and nothing else: a run that `CQ max calls` stopped stays
/// stopped through a QSY, a Generate or a mode switch, and one still calling keeps its count.
#[test]
fn a_rewrite_keeps_the_runs_count_of_cqs() {
    let mut e = station(Tier::Ft8);
    let mut s = e.settings().clone();
    s.cq_max_calls = Some(2);
    e.apply_settings(s);
    e.start_cq_as_typed("CQ DX K1ABC FN42").expect("a run");
    assert_eq!(keyed(&mut e, 0).as_deref(), Some("CQ DX K1ABC FN42"));
    e.rewrite_cq("CQ DX K1ABC FN31").expect("rewritten");
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("CQ DX K1ABC FN31"),
        "the second CQ, rewritten"
    );
    e.rewrite_cq("CQ DX K1ABC FN31")
        .expect("a rewrite at the cap");
    assert_eq!(keyed(&mut e, 4), None, "two CQs, and no more");
    assert_eq!(keyed(&mut e, 6), None);
}

/// ⛔ WHAT A TX ROW SENDS IS WHAT IT SAYS. A Tx-slot row that the station would read, or the packer
/// would send, as another message than its text is refused at the press with the reason, as Tx 6
/// and Tx 5 refuse one, and nothing keys: `CQ W1XYZ K1ABC FN31` reads as `CQ K1ABC FN31` and names
/// W1XYZ where a CQ names its caller; `CQ K1ABC POTA` packs as `CQ K1ABC`. Our own sender does not
/// excuse it. CONTROLS: the rows this station draws key as they read, a report written `-5` is the
/// report `-05`, and free text longer than a frame still keys.
#[test]
fn a_tx_row_that_would_go_out_as_another_message_is_refused_at_the_press() {
    for row in [
        "CQ W1XYZ K1ABC FN31",
        "CQ K1ABC POTA",
        "W1AW K1ABC FN31 EXTRA",
    ] {
        let mut e = station(Tier::Ft8);
        let why = e
            .override_next_tx_checked("W1AW", Some("FN31"), row)
            .expect_err(&format!("{row:?} is refused"));
        assert!(!why.is_empty(), "{row}");
        assert_eq!(keyed(&mut e, 0), None, "{row}: nothing keys");
    }
    for (row, sent) in [
        ("W1AW K1ABC FN31", "W1AW K1ABC FN31"),
        ("W1AW K1ABC -05", "W1AW K1ABC -05"),
        ("W1AW K1ABC -5", "W1AW K1ABC -05"),
        ("W1AW K1ABC RR73", "W1AW K1ABC RR73"),
        // Free text is exempt: it keys, as it did (a Tx-slot press shows the row on the TX line).
        ("TNX FOR THE QSO 73", "TNX FOR THE QSO 73"),
    ] {
        let mut e = station(Tier::Ft8);
        e.override_next_tx_checked("W1AW", Some("FN31"), row)
            .unwrap_or_else(|why| panic!("{row}: {why}"));
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(sent), "{row}");
    }
}

/// A Tx row typed with a six-character grid goes out with the four the packer reads of it, as
/// WSJT-X sends it and as Tx 6 does, and the TX line shows the four: the one difference between
/// a row and what it sends that is not refused. CONTROLS: a row whose other words would change on
/// the air is refused as before, six-character grid or not, and nothing keys.
#[test]
fn a_tx_row_with_a_six_character_grid_goes_out_with_four() {
    for (row, sent) in [
        ("W1AW K1ABC FN31AB", "W1AW K1ABC FN31"),
        ("W1AW K1ABC R FN31ab", "W1AW K1ABC R FN31"),
    ] {
        let mut e = station(Tier::Ft8);
        e.override_next_tx_checked("W1AW", Some("FN31"), row)
            .unwrap_or_else(|why| panic!("{row}: {why}"));
        assert_eq!(tx_now(&e).as_deref(), Some(sent), "{row}: the TX line");
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(sent), "{row}: the over");
        assert_eq!(ft8_bits(sent), ft8_bits(row), "{row}: the same frame");
    }
    // A six-character grid the frame carries whole, or one that is free text, is not cut: the EU
    // VHF exchange (`i3=5`) sends all six, and free text sends every character typed.
    for (row, kind) in [
        ("<W1AW> <K1ABC> 570001 FN31AB", "i3=5"),
        ("TNX FN31AB", "i3=0 n3=0"),
    ] {
        assert_eq!(air_form(row).0, kind, "premise: {row}");
        let mut e = station(Tier::Ft8);
        e.override_next_tx_checked("W1AW", Some("FN31"), row)
            .unwrap_or_else(|why| panic!("{row}: {why}"));
        assert_eq!(keyed(&mut e, 0).as_deref(), Some(row), "{row}: keys whole");
    }
    for row in ["CQ W1XYZ K1ABC FN31AB", "W1AW K1ABC/QRP FN31AB"] {
        let mut e = station(Tier::Ft8);
        assert!(
            e.override_next_tx_checked("W1AW", Some("FN31"), row)
                .is_err(),
            "{row}: still refused"
        );
        assert_eq!(keyed(&mut e, 0), None, "{row}: nothing keys");
    }
}
