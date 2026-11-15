//! ⛔ AN UNCHECKED DECODE NEVER MOVES AN AUTOMATIC QSO. The JTDX-style FT8 decoder's FT8S/FT8SD and
//! matched filters accept a message on correlation with the one they expected, with no LDPC and no
//! CRC (JTDX marks them `^`; `modes::Decode::unchecked`). Such a decode is shown, marked, and
//! nothing automatic acts on it: it starts no QSO, advances none, so it logs none, and it is not
//! handed to other programs (WSJT-X UDP) or uploaded (PSK Reporter). The operator may still click
//! it and work the station by hand. The same message error-checked behaves as always.

use super::*;

/// An FT8 station on 20 m.
fn station() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_license_class("extra");
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e
}

fn decode(message: &str, unchecked: bool) -> modes::Decode {
    modes::Decode {
        message: message.into(),
        sync: 0.0,
        snr: -12,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        unchecked,
        mode: Some(modes::ModeKind::Ft8),
    }
}

/// The full pass of `slot` carrying `message`, folded as the radio loop folds it.
fn full_pass(e: &mut Engine, message: &str, unchecked: bool, slot: u64) {
    let result = DecodeResult {
        decodes: vec![decode(message, unchecked)],
        frame: vec![0.0; 180_000],
        pass: DecodePass::Boundary,
        slot,
        epoch: e.decode_epoch,
        heard_on: None,
        failed: false,
        logged_adif: Vec::new(),
        jtdx_took: None,
    };
    let _ = e.apply_decode_result(result);
}

fn qso_state(e: &Engine) -> Option<String> {
    e.snapshot().qso.map(|q| q.state)
}

/// Calling CQ; W9XYZ answers.
fn answered_cq(unchecked: bool) -> Engine {
    let mut e = station();
    e.start_cq(None).expect("premise: the CQ run starts");
    let before = qso_state(&e);
    full_pass(&mut e, "K2DEF W9XYZ EN37", unchecked, 1);
    assert_eq!(before.as_deref(), Some("CallingCq"), "premise: calling CQ");
    e
}

#[test]
fn an_unchecked_reply_to_my_cq_does_not_start_the_qso() {
    let e = answered_cq(true);
    assert_eq!(
        e.qso_dxcall(),
        None,
        "no station taken from an unchecked decode"
    );
    assert_eq!(qso_state(&e).as_deref(), Some("CallingCq"));
}

#[test]
fn the_same_reply_error_checked_starts_it() {
    let e = answered_cq(false);
    assert_eq!(e.qso_dxcall(), Some("W9XYZ"));
}

/// Working W9XYZ (S&P), who answers with a report.
fn reported(unchecked: bool) -> Engine {
    let mut e = station();
    e.call_station("W9XYZ");
    let before = qso_state(&e);
    full_pass(&mut e, "K2DEF W9XYZ -10", unchecked, 1);
    assert!(before.is_some(), "premise: a QSO");
    e
}

#[test]
fn an_unchecked_report_does_not_advance_the_qso() {
    let before = qso_state(&{
        let mut e = station();
        e.call_station("W9XYZ");
        e
    });
    assert_eq!(qso_state(&reported(true)), before);
}

#[test]
fn the_same_report_error_checked_advances_it() {
    let before = qso_state(&{
        let mut e = station();
        e.call_station("W9XYZ");
        e
    });
    assert_ne!(qso_state(&reported(false)), before);
}

#[test]
fn an_unchecked_decode_is_shown_marked_and_handed_to_no_other_program() {
    let mut e = station();
    full_pass(&mut e, "CQ W9XYZ EN37", true, 1);
    let rows = e.snapshot().recent_decodes;
    let row = rows
        .iter()
        .find(|r| r.message == "CQ W9XYZ EN37")
        .expect("shown");
    assert!(row.unchecked, "marked");
    assert!(
        e.last_wire_decodes.is_empty(),
        "not sent to WSJT-X UDP clients or PSK Reporter"
    );
    full_pass(&mut e, "CQ N7GHI DN31", false, 2);
    assert_eq!(e.last_wire_decodes.len(), 1, "a checked one is, as always");
}

// ⭐ A REPEAT OF A CHECKED MESSAGE COUNTS AS CHECKED. A station calling CQ over and over is
// error-checked the first time; the JTDX-style decoder then often finds the later copies of the
// same parity through FT8SD1, which matches the message it decoded there 30 s before and so comes
// out unchecked. Such a copy, word for word a message decoded WITH the error check from the same
// station on the same parity within `UNCHECKED_REPEAT_PERIODS` same-parity periods, is treated as
// checked ([`Engine::error_checked`]). It stays marked `^` on screen.

/// The messages handed to other programs (WSJT-X UDP, PSK Reporter) for the last period.
fn wired(e: &Engine) -> Vec<String> {
    e.last_wire_decodes
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn the_unchecked_copies_of_a_repeated_cq_reach_other_programs_and_a_loggers_reply() {
    let mut e = station();
    full_pass(&mut e, "CQ W9XYZ EN37", false, 1);
    assert_eq!(
        wired(&e),
        ["CQ W9XYZ EN37"],
        "premise: the checked first copy"
    );
    for (copy, slot) in [(2, 3), (3, 5)] {
        full_pass(&mut e, "CQ W9XYZ EN37", true, slot);
        assert_eq!(
            wired(&e),
            ["CQ W9XYZ EN37"],
            "copy {copy}: handed to WSJT-X UDP and PSK Reporter"
        );
        let row = e
            .snapshot()
            .recent_decodes
            .into_iter()
            .find(|r| r.message == "CQ W9XYZ EN37")
            .expect("shown");
        assert!(row.unchecked, "copy {copy}: still marked `^` on screen");
    }
    // A logger's auto-reply on the third copy (Nexus answers no CQ by itself: qso.rs) works it.
    e.answer_udp_reply("W9XYZ", "CQ W9XYZ EN37", -12, Some(1500.0))
        .expect("the logger's Reply is taken");
    assert_eq!(e.qso_dxcall(), Some("W9XYZ"));
    assert!(e.tx_enabled());
}

#[test]
fn an_unchecked_guess_with_new_content_stays_gated() {
    // The CQ was checked; the caller's next over, found only by a guess, is new content.
    for guess in ["CQ W9XYZ EN38", "CQ DX W9XYZ EN37", "N7GHI W9XYZ -05"] {
        let mut e = station();
        full_pass(&mut e, "CQ W9XYZ EN37", false, 1);
        full_pass(&mut e, guess, true, 3);
        assert!(wired(&e).is_empty(), "{guess:?} is not a repeat");
    }
    // An unchecked answer to my CQ from a station whose CQ was checked starts nothing.
    let mut e = station();
    full_pass(&mut e, "CQ W9XYZ EN37", false, 1);
    e.start_cq(None).expect("premise: the CQ run starts");
    full_pass(&mut e, "K2DEF W9XYZ EN37", true, 3);
    assert_eq!(e.qso_dxcall(), None, "no station taken from the guess");
}

#[test]
fn a_repeat_of_a_message_only_ever_unchecked_stays_gated() {
    let mut e = station();
    for slot in [1, 3, 5, 7] {
        full_pass(&mut e, "CQ W9XYZ EN37", true, slot);
        assert!(
            wired(&e).is_empty(),
            "slot {slot}: never checked, never handed on"
        );
    }
}

#[test]
fn a_repeat_counts_only_on_the_same_parity_and_within_the_window() {
    // The other parity is another transmission schedule, not a repeat.
    let mut e = station();
    full_pass(&mut e, "CQ W9XYZ EN37", false, 1);
    full_pass(&mut e, "CQ W9XYZ EN37", true, 2);
    assert!(wired(&e).is_empty(), "the other parity");
    // The last same-parity period inside the window counts; the next one does not, and the
    // unchecked copies in between do not stretch it.
    let last = 1 + 2 * UNCHECKED_REPEAT_PERIODS;
    let mut e = station();
    full_pass(&mut e, "CQ W9XYZ EN37", false, 1);
    full_pass(&mut e, "CQ W9XYZ EN37", true, last);
    assert_eq!(wired(&e), ["CQ W9XYZ EN37"], "within the window");
    full_pass(&mut e, "CQ W9XYZ EN37", true, last + 2);
    assert!(
        wired(&e).is_empty(),
        "past it: counted from the checked copy"
    );
    // A band change forgets what was checked.
    let mut e = station();
    full_pass(&mut e, "CQ W9XYZ EN37", false, 1);
    e.set_frequency(7.074, "40m", "USB");
    full_pass(&mut e, "CQ W9XYZ EN37", true, 3);
    assert!(wired(&e).is_empty(), "checked on another band");
}

/// The shapes the JTDX-style decoder marks though they passed the CRC (both calls unresolved
/// hashes; a CQ with no call: `ft8::var`) come marked every time it decodes them, so they never
/// have a checked copy for a repeat to lean on: a repeat of one stays gated.
#[test]
fn a_repeat_of_a_marked_noise_shape_stays_gated() {
    for shape in ["<...> <...> R 521761 CG33TI", "CQ GI576RBFOEB"] {
        let mut e = station();
        for slot in [1, 3, 5] {
            full_pass(&mut e, shape, true, slot);
            assert!(wired(&e).is_empty(), "{shape:?} slot {slot}: never handed on");
        }
    }
}
