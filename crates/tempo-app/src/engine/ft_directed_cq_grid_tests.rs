//! A directed CQ (`CQ DX`, `CQ POTA`, `CQ NA` …) is built with the four-character grid a
//! standard FT8/FT4 message carries, whatever the Settings locator holds.
//!
//! WSJT-X 3.0.2 builds every CQ from `grid.left (4)` (`MainWindow::genCQMsg`), the directed Tx6
//! included, so a six-character locator in Settings is never in the text it shows, logs or sends.
//! Nexus cuts the grid where a QSO station is built, and the directed CQ must be built from that
//! same cut grid. The packer itself reads only four characters of a grid, so the frame on the air
//! carried four either way; what a six-character locator broke was the text: the TX line, ALL.TXT
//! and the UDP Tx message showed six characters the air never carried. These tests hold every
//! path a directed CQ is called from (the run's start, the return to CQ after a contact, the
//! return after Clear DX, and the Tx6 that follows a partner's 73) to the text WSJT-X would send, and
//! check the packer sends it as a standard (`i3=1`) CQ that decodes back verbatim.

use super::contest_ft_messages_tests::{air_form, keyed};
use super::*;
use crate::test_util::StoredLog;

const ME: &str = "KR4FQG";
/// The Settings locator: six characters, as the operator typed it.
const MY_GRID6: &str = "EM64ue";
const SNR: i32 = -12;

/// A station on 20 m FT8 with `grid` in Settings, TX on the even slots.
fn station(grid: &str, tier: Tier) -> Engine {
    let mut e = Engine::new(ME, grid, 0);
    e.set_tier(tier);
    e.set_frequency(14.074, "20m", "USB");
    e.set_tx_offset(1500.0);
    e
}

/// The over `slot` keys must be `want`, and the packer must send it as a standard message that a
/// receiving station decodes as exactly that text.
fn keys_standard(e: &mut Engine, slot: u64, want: &str, what: &str) {
    assert_eq!(keyed(e, slot).as_deref(), Some(want), "{what}: the over");
    assert_eq!(
        air_form(want),
        ("i3=1".to_string(), want.to_string()),
        "{what}: a standard CQ on the air, not free text"
    );
}

/// ⭐ THE DIRECTED CQ CARRIES FOUR CHARACTERS. A six-character locator in Settings calls
/// `CQ DX KR4FQG EM64`, as WSJT-X's Tx6 does, on FT8 and FT4 and for every directed word; a
/// four-character locator calls the same over.
#[test]
fn a_directed_cq_sends_four_characters_of_the_settings_grid() {
    for tier in [Tier::Ft8, Tier::Ft4] {
        for grid in [MY_GRID6, "EM64"] {
            for dir in ["DX", "POTA", "NA"] {
                let what = format!("{tier:?} {grid} CQ {dir}");
                let mut e = station(grid, tier);
                e.start_cq(Some(dir)).expect("a directed run");
                let want = format!("CQ {dir} {ME} EM64");
                assert_eq!(
                    e.snapshot().qso.and_then(|q| q.tx_now).as_deref(),
                    Some(want.as_str()),
                    "{what}: what the screen shows"
                );
                keys_standard(&mut e, 0, &want, &what);
            }
        }
    }
    // The plain CQ was already cut, and stays so.
    let mut e = station(MY_GRID6, Tier::Ft8);
    e.start_cq(None).expect("a plain run");
    keys_standard(&mut e, 0, "CQ KR4FQG EM64", "plain CQ");
}

/// The run returns to its directed CQ after a completed contact with the four-character grid.
#[test]
fn the_directed_cq_after_a_contact_keeps_four_characters() {
    for dir in ["DX", "POTA", "NA"] {
        let what = format!("CQ {dir} after a contact");
        let mut e = station(MY_GRID6, Tier::Ft8);
        e.start_cq(Some(dir)).expect("a directed run");
        let cq = format!("CQ {dir} {ME} EM64");
        keys_standard(&mut e, 0, &cq, &what);
        e.hear_for_test("KR4FQG K1ABC FN42", SNR, 2);
        assert_eq!(
            keyed(&mut e, 2).as_deref(),
            Some("K1ABC KR4FQG -12"),
            "premise: {what}: the report"
        );
        e.hear_for_test("KR4FQG K1ABC R-05", SNR, 4);
        assert_eq!(
            keyed(&mut e, 4).as_deref(),
            Some("K1ABC KR4FQG RR73"),
            "premise: {what}: the RR73"
        );
        e.hear_for_test("CQ N0OTH EM48", SNR, 6);
        assert_eq!(e.stored_log().len(), 1, "premise: {what}: logged");
        keys_standard(&mut e, 6, &cq, &what);
    }
}

/// Clear DX in a directed run returns to the directed CQ with the four-character grid.
#[test]
fn the_directed_cq_after_clear_dx_keeps_four_characters() {
    for dir in ["DX", "POTA", "NA"] {
        let what = format!("CQ {dir} after Clear DX");
        let mut e = station(MY_GRID6, Tier::Ft8);
        e.start_cq(Some(dir)).expect("a directed run");
        let cq = format!("CQ {dir} {ME} EM64");
        keys_standard(&mut e, 0, &cq, &what);
        e.hear_for_test("KR4FQG K1ABC FN42", SNR, 2);
        assert_eq!(
            keyed(&mut e, 2).as_deref(),
            Some("K1ABC KR4FQG -12"),
            "premise: {what}: the report"
        );
        // Clear DX turns Enable Tx off, as WSJT-X's does; the operator turns it back on.
        e.clear_dx_call();
        e.set_tx_enabled_by_operator(true);
        keys_standard(&mut e, 4, &cq, &what);
    }
}

/// The Tx6 the mid-over switch sends for the rest of our RRR when the partner's 73 folds during it
/// (WSJT-X's `m_ntx=6`) is the directed CQ with four characters.
#[test]
fn the_tx6_after_the_partners_73_keeps_four_characters() {
    let mut e = station(MY_GRID6, Tier::Ft8);
    e.settings.prefer_rrr = true;
    e.start_cq(Some("DX")).expect("a directed run");
    keys_standard(&mut e, 0, "CQ DX KR4FQG EM64", "the CQ");
    e.hear_for_test("KR4FQG K1ABC R-12", SNR, 2);
    e.call_station_ctx("K1ABC", None, Some("KR4FQG K1ABC R-12"), Some(SNR), None)
        .expect("premise: K1ABC worked");
    let _ = e.take_immediate_tx();
    assert_eq!(
        keyed(&mut e, 2).as_deref(),
        Some("K1ABC KR4FQG RRR"),
        "premise: our RRR"
    );
    e.hear_for_test("KR4FQG K1ABC 73", SNR, 2);
    assert_eq!(
        e.ft_next_over(2).as_deref(),
        Some("CQ DX KR4FQG EM64"),
        "the Tx6 after the partner's 73"
    );
}

/// What the packer makes of six characters: it reads only the first four characters of a grid
/// (`packjt77.f90` `pack77_1`: `is_grid4(w(nwords)(1:4))`), so a six-character CQ goes on the air
/// as the four-character one. The frame was always right, and cutting the text changes no frame;
/// the text Nexus showed, logged to ALL.TXT and reported over UDP was what was wrong.
#[test]
fn the_packer_sends_four_characters_of_a_six_character_grid() {
    for (typed, on_air) in [
        ("CQ DX KR4FQG EM64ue", "CQ DX KR4FQG EM64"),
        ("CQ POTA KR4FQG EM64ue", "CQ POTA KR4FQG EM64"),
        ("CQ FD K1ABC FN42ab", "CQ FD K1ABC FN42"),
    ] {
        assert_eq!(
            air_form(typed),
            ("i3=1".to_string(), on_air.to_string()),
            "{typed}"
        );
    }
}
