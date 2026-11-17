//! The operator's split +/- and the licence: a press that would put the transmitter outside the
//! operator's privileges is refused when it is made, and never queued for the radio.
use super::*;
use crate::dualrx::ReceiverId;
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

#[test]
fn a_refused_press_with_no_split_leaves_simplex() {
    let mut e = station();
    assert!(e.request_operator_split(Some(14.351)).is_err());
    assert_eq!(e.split_tx_mhz(), None);
    assert_eq!(e.take_split_request(), None);
}

/// The check is the key-time gate itself: what it allows, the gate then keys, and what it
/// refuses, the gate would refuse, with the acknowledgement it borrowed put back.
#[test]
fn the_check_is_the_key_time_gate_and_leaves_the_acknowledgement_alone() {
    let mut e = station();
    e.rig_split_applied(14_342_000);
    assert!(e.split_tx_allowed(14.345, ReceiverId::Main));
    assert!(!e.split_tx_allowed(14.349, ReceiverId::Main));
    assert_eq!(
        e.tx_emission_mhz(),
        14.342,
        "the acknowledgement is unchanged"
    );
}

/// XIT, both ways, as the gate judges a split with XIT.
#[test]
fn the_check_judges_xit() {
    let mut e = station();
    e.request_xit(3_000);
    assert!(
        !e.split_tx_allowed(14.347, ReceiverId::Main),
        "14.347 + 3 kHz runs past 14.350"
    );
    assert!(e.split_tx_allowed(14.344, ReceiverId::Main));
}

/// ⛔ THE FM REPEATER SHIFT. A Technician on a 6 m machine at 51.200 with a 1 MHz minus shift
/// transmits 50.200. A split to 51.050 transmits 50.050, in the CW-only bottom of 6 m: the gate
/// refuses it, so the press must. A copy that judged the carrier alone allowed it.
#[test]
fn the_check_judges_the_fm_repeater_shift() {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.set_license_class("technician");
    e.repeater_tune(51.200, "minus", 1_000_000, 0.0)
        .expect("a 50.200 input is legal");
    assert!(
        e.split_tx_allowed(51.300, ReceiverId::Main),
        "control: input 50.300"
    );
    assert!(
        !e.split_tx_allowed(51.050, ReceiverId::Main),
        "input 50.050 is CW only"
    );
    assert_eq!(
        e.request_operator_split(Some(51.050)),
        Err(SPLIT_REFUSED_PRIVILEGES.to_string())
    );
}

/// ⛔ THE SUB UPLINK'S BOTH-SIDES RULE. An IC-9700 pass whose uplink mode the operator took back:
/// Nexus commands the uplink no word, so both sides of the carrier must be legal. 144.1005 is
/// legal above and not below (the PSK31 edge bird's row in the gate table), so a split there on
/// the Sub is refused. The dial's own word alone would have allowed it.
#[test]
fn the_check_judges_the_sub_uplinks_unknown_side() {
    use super::tx_gate_table::{released_on_the_sub, RS44};
    let mut e = released_on_the_sub(
        "keyboard",
        crate::settings::CwKeyerBackend::Cat,
        "afsk",
        RS44,
    );
    assert!(e.tx_allowed(), "control: RS-44 on the Sub keys");
    assert!(
        e.split_tx_allowed(145.965, ReceiverId::Sub),
        "control: the uplink itself"
    );
    assert!(!e.split_tx_allowed(144.1005, ReceiverId::Sub));
}

/// ⛔ A SPLIT REFUSED AT THE WRITE NEVER DROPS NEXUS TO SIMPLEX while the radio stays split. The
/// split the radio has, and its acknowledgement, are put back; with none, both stay simplex.
#[test]
fn a_split_refused_at_the_write_puts_the_standing_split_back() {
    let mut e = station();
    assert!(e.request_operator_split(Some(14.345)).is_ok());
    let _ = e.take_split_request();
    e.rig_split_applied(14_345_000);
    // A request past the edge from a source not judged when it asked (`request_split`), the
    // loop refusing it while keyed.
    e.request_split(Some(14.349));
    assert_eq!(e.take_split_request(), Some(Some(14.349)));
    e.split_refused_while_keyed(14.349);
    assert_eq!(
        e.split_tx_mhz(),
        Some(14.345),
        "the badge shows the split the radio has"
    );
    assert_eq!(
        e.tx_emission_mhz(),
        14.345,
        "the gate judges where the radio transmits"
    );
    assert_eq!(
        e.take_split_request(),
        None,
        "the refused request is discarded"
    );
    // A later QSY returns the radio to simplex, as for any split.
    e.set_frequency(7.200, "40m", "LSB");
    assert_eq!(e.take_split_request(), Some(None));
}

#[test]
fn a_split_refused_at_the_write_with_none_standing_stays_simplex() {
    let mut e = station();
    e.request_split(Some(14.349));
    let _ = e.take_split_request();
    e.split_refused_while_keyed(14.349);
    assert_eq!(e.split_tx_mhz(), None);
    assert_eq!(e.tx_emission_mhz(), 14.340, "simplex: the dial");
}

/// Once the radio is back on simplex, nothing stands to be put back.
#[test]
fn simplex_applied_clears_the_standing_split() {
    let mut e = station();
    e.rig_split_applied(14_345_000);
    e.rig_split_cleared();
    e.request_split(Some(14.349));
    let _ = e.take_split_request();
    e.split_refused_while_keyed(14.349);
    assert_eq!(e.split_tx_mhz(), None);
}

/// ⛔ A CONNECTION REFUSAL NEVER APPROVES A SPLIT. The Icom network connection refuses Nexus's
/// own keys, but a front-panel PTT still transmits on whatever split Nexus wrote: the licence is
/// judged as it is for any key, with the connection refusal masked out, not turned into a yes.
#[test]
fn a_connection_refusal_never_approves_a_split() {
    let mut e = station();
    e.settings.rig_conn = "icomlan".to_string();
    assert!(
        e.connection_tx_refusal().is_some(),
        "precondition: the connection refuses keys"
    );
    assert!(!e.split_tx_allowed(14.349, ReceiverId::Main));
    assert_eq!(
        e.request_operator_split(Some(14.349)),
        Err(SPLIT_REFUSED_PRIVILEGES.to_string())
    );
    assert_eq!(e.take_split_request(), None, "nothing queued");
    assert!(
        e.split_tx_allowed(14.345, ReceiverId::Main),
        "control: in privileges"
    );
}

/// ⛔ A SPOT'S "UP n" IS JUDGED WHEN IT IS ASKED. Past the edge, no split is queued, keyed or
/// not; the QSY's own return to simplex stands (so the radio's old split is not left behind).
#[test]
fn a_spot_split_past_the_edge_queues_simplex_not_the_split() {
    let mut e = station();
    assert!(e.request_operator_split(Some(14.345)).is_ok());
    let _ = e.take_split_request();
    e.rig_split_applied(14_345_000);
    e.work_spot_split("phone", 14.340, "20m", Some(9.0));
    assert_eq!(
        e.take_split_request(),
        Some(None),
        "the QSY's simplex, never the split"
    );
    assert_eq!(e.split_tx_mhz(), None);
    // Control: an in-band "UP 3" is queued.
    e.work_spot_split("phone", 14.340, "20m", Some(3.0));
    assert_eq!(e.take_split_request(), Some(Some(14.343)));
}

/// A bird whose uplink sits 4 kHz above the 144.1 edge, inverting (so its uplink is LSB).
const EDGE_UP_BIRD: tempo_core::doppler::Transponder = tempo_core::doppler::Transponder {
    uplink_centre_hz: 144_104_000,
    downlink_centre_hz: 435_640_000,
    invert: true,
    half_width_hz: 0,
};

/// ⛔ A DOPPLER STEP IS JUDGED WHEN IT IS ASKED, on the receiver it rides. Approaching, the
/// uplink steps down; past the edge it is refused and nothing is queued, the uplink the radio has
/// stands. In band, steps under PTT are queued exactly as before.
#[test]
fn a_doppler_step_out_of_privilege_is_refused_and_one_in_band_is_queued_under_ptt() {
    use super::tx_gate_table::{loop_applies_sat_split, sat_pass, RS44};
    let mut e = sat_pass(3081, "general", "phone", EDGE_UP_BIRD);
    assert_eq!(
        loop_applies_sat_split(&mut e, SatCatBackend::NativeCiv),
        Some("Sub")
    );
    let held = e.split_tx_mhz();
    let _ = e.take_split_request();
    // Approaching fast: the uplink steps a couple of kHz down, under the edge in LSB.
    let c = e
        .sat_doppler_tick(-7.0, 2_000_000, true)
        .expect("a correction");
    assert!(
        c.uplink_hz.is_some_and(|hz| hz < 144_102_000),
        "precondition: {c:?}"
    );
    assert_eq!(
        e.take_split_request(),
        None,
        "the refused step is not queued"
    );
    assert_eq!(e.split_tx_mhz(), held, "the uplink the radio has stands");

    let mut rs = sat_pass(3081, "general", "phone", RS44);
    assert_eq!(
        loop_applies_sat_split(&mut rs, SatCatBackend::NativeCiv),
        Some("Sub")
    );
    let _ = rs.take_split_request();
    let c = rs
        .sat_doppler_tick(5.0, 2_000_000, true)
        .expect("a correction");
    assert!(c.uplink_hz.is_some());
    assert!(
        matches!(rs.take_split_request(), Some(Some(_))),
        "an in-band step under PTT is queued"
    );
}

/// ⛔ A SPLIT OFF THE RADIO DID NOT CONFIRM IS UNKNOWN, never simplex: the gate refuses until a
/// write settles it, and the next QSY sends `S 0` again.
#[test]
fn an_unconfirmed_split_off_holds_transmit_until_it_is_settled() {
    let mut e = station();
    e.settings.license_class = LicenseClass::Extra;
    e.rig_split_applied(14_345_000);
    e.rig_split_unknown();
    assert!(!e.tx_allowed(), "unknown split: refused");
    e.set_frequency(14.300, "20m", "USB");
    assert_eq!(
        e.take_split_request(),
        Some(None),
        "the QSY sends S 0 again"
    );
    e.rig_split_cleared();
    assert!(e.tx_allowed(), "settled: simplex at 14.300 keys");
}

/// The same for the other connection refusal: a station profile on another licensee's call while
/// a licence class is set. It refuses Nexus's keys; it never approves a split.
#[test]
fn a_station_profile_refusal_never_approves_a_split() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    assert!(e.set_license_class("extra"));
    e.set_operating_mode("phone", false);
    e.set_frequency(14.340, "20m", "USB");
    let club = crate::settings::StationProfile {
        name: "Club".into(),
        call: "W9ABC".into(),
        grid: "EN52".into(),
        ..crate::settings::StationProfile::default()
    };
    let club = e.save_station_profile(club).unwrap();
    e.switch_station_profile(&club).unwrap();
    let _ = e.take_split_request();
    assert_eq!(
        e.connection_tx_refusal(),
        Some(crate::settings::STATION_PROFILE_CALL_TX_REFUSED),
        "precondition: the profile's call refuses keys"
    );
    assert!(!e.split_tx_allowed(14.349, ReceiverId::Main));
    assert_eq!(
        e.request_operator_split(Some(14.349)),
        Err(SPLIT_REFUSED_PRIVILEGES.to_string())
    );
    assert_eq!(e.take_split_request(), None, "nothing queued");
    assert!(
        e.split_tx_allowed(14.345, ReceiverId::Main),
        "control: in privileges"
    );
}

/// ⚠️ A REQUEST MADE AFTER A QSY, REFUSED WHILE KEYED, IS KEPT FOR THE UNKEY: the QSY's return to
/// simplex was replaced by it, so discarding it would leave the radio on the old VFO B, on the
/// old band, after the dial moves. The acknowledgement the radio really has is put back meanwhile.
#[test]
fn a_refused_request_after_a_qsy_stays_queued_for_the_unkey() {
    let mut e = station();
    e.rig_split_applied(14_345_000);
    // Cross-band: a QSY to 40 m, then a split request there that the loop refuses while keyed.
    e.set_frequency(7.200, "40m", "LSB");
    e.request_split(Some(7.301));
    assert_eq!(e.take_split_request(), Some(Some(7.301)));
    e.split_refused_while_keyed(7.301);
    assert_eq!(
        e.tx_emission_mhz(),
        14.345,
        "the gate judges the VFO B the radio still has"
    );
    assert_eq!(
        e.take_split_request(),
        Some(Some(7.301)),
        "kept, for the first unkeyed tick"
    );
}
