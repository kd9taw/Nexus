//! #512: a CQ or a call between standard callsigns with NO grid, as WSJT-X 3.0.2 sends, reads and
//! answers it.
//!
//! WSJT-X packs `CQ W1AW` and `W9XYZ K1ABC` as ordinary Type 1 messages with a blank grid
//! (`lib/77bit/packjt77.f90` `pack77_1` 1225-1236 and 1296-1302; `unpack77` 569-570 prints them
//! back as the two words). Its decode window does not read the received `i3`: `DecodedText`
//! (`Decoder/decodedtext.cpp` 83-87) calls `stdmsg`, which RE-PACKS the decoded text and calls it
//! standard when `pack77` gives `i3 > 0 or n3 > 0` (`lib/stdmsg.f90` 27-31). The text alone
//! therefore decides, and that is what these tests hold Nexus to: every line here goes through the
//! FT8 encoder and the vendored unpacker, and Nexus's parse must call it free text exactly when the
//! packer made it free text (`i3=0 n3=0`).

use super::contest_ft_messages_tests::{air_form, keyed};
use super::*;

const ME: &str = "K1ABC";
const MY_GRID: &str = "FN42";
const SNR: i32 = -12;

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

/// ⭐ THE PARSE AGREES WITH THE PACKER, line for line. A grid-less CQ (plain, directed, from a
/// `/P` or `/R` station) and a grid-less call between two standard calls go on the air as Type 1
/// (or Type 4, the slashed CQ), come back off it verbatim, and parse as a CQ or a call; free text
/// — including Nexus's own chat chunks that do not read as two calls — goes as `i3=0 n3=0` and
/// parses as free text. The chunk `A13DE W9XYZ` is the honest limit: its header passes `chkcall`,
/// so `pack77` sends it as a Type 1 call to `A13DE` and no receiver, WSJT-X included, can tell it
/// from one. The chat inbox keeps reading it as chat (`inbox.rs`).
#[test]
fn the_parse_calls_a_line_standard_exactly_when_the_packer_does() {
    for (text, kind) in [
        ("CQ W1AW", "i3=1"),
        ("CQ DX W1AW", "i3=1"),
        ("CQ NA W1AW", "i3=1"),
        ("CQ POTA W1AW", "i3=1"),
        ("CQ TEST W1AW", "i3=1"),
        ("CQ 040 W1AW", "i3=1"),
        ("CQ W1AW/P", "i3=4"),
        ("W9XYZ K1ABC", "i3=1"),
        ("K1ABC/R W9XYZ", "i3=1"),
        ("A79AA K2DEF", "i3=1"),
        ("A13DE W9XYZ", "i3=1"),
        ("TNX K1ABC", "i3=0 n3=0"),
        ("HELLO W9XYZ", "i3=0 n3=0"),
        ("73 K1ABC", "i3=0 n3=0"),
        ("W1AW 5NN", "i3=0 n3=0"),
        ("5/9 NJ2X", "i3=0 n3=0"),
        ("Q1ABC W1AW", "i3=0 n3=0"),
        ("CQ 1AB", "i3=0 n3=0"),
        ("CQ QRP", "i3=0 n3=0"),
        ("A23ON 7130", "i3=0 n3=0"),
    ] {
        let (air_kind, decoded) = air_form(text);
        assert_eq!(
            (air_kind.as_str(), decoded.as_str()),
            (kind, text),
            "{text}: on the air"
        );
        let parsed = Msg::parse(&decoded);
        assert_eq!(
            matches!(parsed, Msg::Other(_)),
            kind == "i3=0 n3=0",
            "{text}: parsed {parsed:?}"
        );
    }
}

/// The decode row WSJT-X double-clicks: a grid-less CQ from a standard call is a CQ and names its
/// caller, and a grid-less call names both stations — which is what the Band Activity CQ chip, the
/// double-click and the roster read.
#[test]
fn a_gridless_standard_cq_row_is_a_cq_with_its_caller() {
    let mut e = Engine::new(ME, MY_GRID, 0);
    e.ingest_decodes_for_test(
        &[heard("CQ W1AW"), heard("CQ DX W9XYZ"), heard("K1ABC W2XX")],
        0,
    );
    let rows = e.snapshot().recent_decodes;
    let by = |m: &str| {
        rows.iter()
            .find(|r| r.message == m)
            .unwrap_or_else(|| panic!("row {m}"))
    };
    for (m, from) in [("CQ W1AW", "W1AW"), ("CQ DX W9XYZ", "W9XYZ")] {
        assert!(by(m).is_cq, "{m} is a CQ");
        assert_eq!(by(m).from.as_deref(), Some(from), "{m}");
    }
    let call = by("K1ABC W2XX");
    assert!(!call.is_cq);
    assert_eq!(call.from.as_deref(), Some("W2XX"));
    assert!(call.directed_to_me, "a grid-less call to me is to me");
}

/// Double-clicking a grid-less CQ answers it as WSJT-X does (`processMessage`, mainwindow.cpp
/// 9361-9373, "just work them"): Tx1, this station's call and grid, and the over keys as written.
#[test]
fn a_double_click_on_a_gridless_standard_cq_answers_with_my_grid() {
    for cq in ["CQ W1AW", "CQ POTA W1AW"] {
        let mut e = Engine::new(ME, MY_GRID, 0);
        e.ingest_decodes_for_test(&[heard(cq)], 2);
        e.work_station("W1AW", None, Some(cq), Some(SNR), None)
            .expect("the double-click is taken");
        assert_eq!(
            (1..5).find_map(|s| keyed(&mut e, s)).as_deref(),
            Some("W1AW K1ABC FN42"),
            "{cq}"
        );
    }
}
