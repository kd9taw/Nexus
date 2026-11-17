//! The operator's split +/- and the licence: a press that would put the transmitter outside the
//! operator's privileges is refused when it is made, and never queued for the radio.
use super::*;
use crate::settings::LicenseClass;

/// An Extra in Phone at 14.340 USB, split off.
fn station() -> Engine {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.settings.license_class = LicenseClass::Extra;
    e.set_operating_mode("phone", false);
    e.set_frequency(14.340, "20m", "USB");
    let _ = e.take_split_request();
    e
}

#[test]
fn a_split_press_past_the_band_edge_is_refused_and_nothing_is_queued() {
    let mut e = station();
    assert!(
        e.request_operator_split(Some(14.345)).is_ok(),
        "precondition: in band"
    );
    assert_eq!(e.take_split_request(), Some(Some(14.345)));
    // 14.349 USB runs past 14.350, the top of 20 m.
    let refused = e.request_operator_split(Some(14.349));
    assert_eq!(refused, Err(SPLIT_REFUSED_PRIVILEGES.to_string()));
    assert_eq!(
        e.split_tx_mhz(),
        Some(14.345),
        "the split stays where it was"
    );
    assert_eq!(
        e.take_split_request(),
        None,
        "nothing is queued for the radio"
    );
}

#[test]
fn a_split_press_under_the_licence_floor_is_refused() {
    let mut e = station();
    e.settings.license_class = LicenseClass::General;
    e.set_frequency(14.230, "20m", "USB");
    let _ = e.take_split_request();
    // A General's 20 m phone starts at 14.225.
    assert!(e.request_operator_split(Some(14.220)).is_err());
    assert_eq!(e.take_split_request(), None);
    assert!(
        e.request_operator_split(Some(14.235)).is_ok(),
        "in privileges is applied"
    );
    assert_eq!(e.take_split_request(), Some(Some(14.235)));
}

#[test]
fn split_off_is_never_refused() {
    let mut e = station();
    assert!(e.request_operator_split(Some(14.345)).is_ok());
    let _ = e.take_split_request();
    e.set_frequency(14.349, "20m", "USB"); // the dial itself out of privileges
    let _ = e.take_split_request();
    assert!(e.request_operator_split(None).is_ok());
}

/// The radio loop's check before it writes a split TX frequency into a keyed radio.
#[test]
fn the_loop_check_judges_the_split_and_its_xit() {
    let e = station();
    assert!(e.split_tx_allowed(14.345));
    assert!(!e.split_tx_allowed(14.349));
}
