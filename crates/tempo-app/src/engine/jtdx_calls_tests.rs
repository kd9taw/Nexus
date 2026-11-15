//! The calls the JTDX-style FT8 decoder is given, as WSJT-X 3.0.2 gives them
//! (`mainwindow.cpp:5578-5592`): MY call always; the DX call whenever the DX Call box holds one,
//! in a QSO or not, with its grid; and the band-wide DX-call search (`lenabledxcsearch`) while a
//! DX call is set, Enable Tx is off, and that call is not the one last logged (cleared by a band
//! change, `:12121`). The standard decoder's AP context is untouched: it still has the calls only
//! inside a QSO.

use super::*;

/// An FT8 station on 20 m, monitoring.
fn station() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_license_class("extra");
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e
}

fn job(e: &Engine) -> DecodeJob {
    e.build_decode_job(vec![0.0; 180_000], 1, DecodePass::Boundary)
}

fn jtdx(e: &Engine) -> modes::JtdxRequest {
    job(e).jtdx.expect("the JTDX-style decoder runs FT8's full pass")
}

/// A contact with `call` on 20 m FT8, as the log holds one.
fn contact(call: &str) -> QsoRecord {
    let field = |tag: &str, value: &str| format!("<{tag}:{}>{value}", value.len());
    let text = [
        field("CALL", call),
        field("BAND", "20m"),
        field("MODE", "FT8"),
        field("FREQ", "14.074"),
        field("QSO_DATE", "20261115"),
        field("TIME_ON", "120000"),
    ]
    .concat();
    let mut lb = tempo_core::logbook::Logbook::new();
    lb.import_adif(&(text + "<EOR>"));
    QsoRecord::clone(&lb.records()[0])
}

#[test]
fn monitoring_my_call_is_given_and_no_dx_call() {
    let e = station();
    let r = jtdx(&e);
    assert_eq!((r.mycall.as_str(), r.mybcall.as_str()), ("K2DEF", "K2DEF"));
    assert_eq!(r.hiscall, "");
    assert!(!r.dx_search, "no DX call, no DX-call search");
    let std = job(&e);
    assert_eq!(
        (std.mycall.as_str(), std.hiscall.as_str()),
        ("", ""),
        "the standard decoder's AP context outside a QSO is as before"
    );
}

#[test]
fn a_dx_call_in_the_box_is_given_with_its_grid_and_searched_for() {
    let mut e = station();
    e.set_dx_box("W9XYZ", "EN37");
    let r = jtdx(&e);
    assert_eq!(r.mycall, "K2DEF");
    assert_eq!((r.hiscall.as_str(), r.hisbcall.as_str()), ("W9XYZ", "W9XYZ"));
    assert_eq!(r.hisgrid, "EN37");
    assert!(r.hiscall_std);
    assert!(r.dx_search, "a DX call set, Enable Tx off, not logged");
    e.set_dx_box("", "");
    let r = jtdx(&e);
    assert_eq!((r.hiscall.as_str(), r.hisgrid.as_str()), ("", ""));
    assert!(!r.dx_search);
}

#[test]
fn enable_tx_on_stops_the_dx_call_search_but_not_the_call() {
    let mut e = station();
    e.set_dx_box("W9XYZ", "");
    e.set_tx_enabled(true);
    let r = jtdx(&e);
    assert_eq!(r.hiscall, "W9XYZ");
    assert!(!r.dx_search);
}

#[test]
fn the_call_last_logged_is_not_searched_for_until_the_band_changes() {
    let mut e = station();
    e.set_dx_box("W9XYZ", "");
    e.log_qso(contact("W9XYZ"));
    assert!(!jtdx(&e).dx_search, "just logged");
    e.set_dx_box("N7GHI", "");
    assert!(jtdx(&e).dx_search, "another call is");
    e.set_dx_box("W9XYZ", "");
    e.set_frequency(7.074, "40m", "USB");
    assert!(jtdx(&e).dx_search, "a band change forgets the last logged call");
}

#[test]
fn in_a_qso_the_partner_is_the_dx_call() {
    let mut e = station();
    e.set_dx_box("W9XYZ", "EN37");
    e.call_station("N7GHI");
    let r = jtdx(&e);
    assert_eq!(r.hiscall, "N7GHI");
    let std = job(&e);
    assert_eq!(std.hiscall, "N7GHI", "the standard decoder's, as before");
}
