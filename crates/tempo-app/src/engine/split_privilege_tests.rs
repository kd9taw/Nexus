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

/// ⛔ A REQUEST MADE AFTER A QSY, REFUSED WHILE KEYED, GIVES WAY TO THE QSY'S OWN RETURN TO
/// SIMPLEX. The request had replaced that return, so discarding it alone would leave the radio
/// on the old VFO B, on the old band, after the dial moves; keeping it would write a split the
/// licence just refused into the radio at the unkey, where a front-panel PTT transmits on it.
/// The acknowledgement the radio really has is put back meanwhile.
#[test]
fn a_refused_request_after_a_qsy_queues_the_qsys_return_to_simplex() {
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
        Some(None),
        "the QSY's simplex is queued, never the refused split"
    );
}

/// …and a newer request made since the loop took the refused one is left queued for its own
/// judgement at the write: the refusal answers only for its own request.
#[test]
fn a_refused_request_after_a_qsy_leaves_a_newer_request_queued() {
    let mut e = station();
    e.rig_split_applied(14_345_000);
    e.set_frequency(7.200, "40m", "LSB");
    e.request_split(Some(7.301));
    assert_eq!(e.take_split_request(), Some(Some(7.301)));
    e.request_split(Some(7.205));
    e.split_refused_while_keyed(7.301);
    assert_eq!(e.take_split_request(), Some(Some(7.205)));
}

/// An IC-9700 on the native daemon, Main = downlink / Sub = uplink confirmed, General, Phone,
/// holding `tp`: a pass about to be picked.
fn ic9700_holding(tp: tempo_core::doppler::Transponder) -> Engine {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.rig_model = 3081;
    e.settings.rig_conn = "serial".to_string();
    e.settings.rig_addr = String::new();
    e.settings.icom_native_cat = true;
    e.settings.sync_active_from_flat();
    let ids: Vec<u32> = e.settings.radios.iter().map(|p| p.id).collect();
    for id in ids {
        e.settings
            .confirm_sat_uplink(id, crate::settings::SatVfoMap::MainDownSubUp);
    }
    e.set_license_class("general");
    e.set_operating_mode("phone", false);
    e.set_sat_transponder(Some(("TEST|linear".into(), 0, tp)));
    e
}

/// ⛔ A PICK'S FIRST UPLINK IS JUDGED WHEN IT IS ASKED, on the receiver it rides. A bird whose
/// uplink sits in 2 m's CW-only bottom (144.05) has no phone uplink a General may use: the pick
/// queues no split and the rail says why. RS-44's uplink is queued as before.
#[test]
fn a_picks_first_uplink_outside_the_licence_is_refused_when_asked() {
    let edge = tempo_core::doppler::Transponder {
        uplink_centre_hz: 144_050_000,
        downlink_centre_hz: 435_640_000,
        invert: true,
        half_width_hz: 0,
    };
    let mut e = ic9700_holding(edge);
    let binding = e.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    assert_eq!(e.split_tx_mhz(), None, "no uplink queued");
    assert!(
        !matches!(e.take_split_request(), Some(Some(_))),
        "nothing for the radio loop to write"
    );
    assert_eq!(binding.pending_uplink_mhz, None);
    assert_eq!(binding.note.as_deref(), Some(SPLIT_REFUSED_PRIVILEGES));

    // The same bird through the unjudged harness constructor: the uplink is queued, and the KEY
    // refuses it. So the request-time refusal is the gate's own answer, given earlier.
    let mut un = ic9700_holding(edge);
    un.sat_tune_nominal_unjudged(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    let up = un.split_tx_mhz().expect("unjudged: queued");
    un.rig_split_applied_on((up * 1e6).round() as u64, ReceiverId::Sub);
    assert!(
        !un.tx_allowed(),
        "the key-time gate refuses that uplink too"
    );

    let mut rs = ic9700_holding(super::tx_gate_table::RS44);
    let binding = rs.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    assert!(
        binding.pending_uplink_mhz.is_some(),
        "control: an in-band uplink is queued"
    );
    assert!(matches!(rs.take_split_request(), Some(Some(_))));
}

/// ⛔ AT THE INVERTING EDGE THE PICK JUDGES THE UPLINK AS THE KEY WILL: on the Sub, in the
/// uplink's own word. An inverting bird under a USB downlink keys its uplink in LSB, and LSB
/// voice at 144.101 runs into 2 m's CW-only bottom (below 144.100): a General may not key it.
/// Judged before the pick recorded it as the uplink sent, the check took it for a terrestrial
/// split, on Main and in the downlink's USB, and queued it for the radio loop to write unkeyed.
#[test]
fn a_picks_uplink_at_the_inverting_edge_is_judged_on_the_sub_in_its_own_word() {
    use super::tx_gate_table::EDGE_BIRD;
    use tempo_core::doppler::DownlinkClass;
    let mut e = ic9700_holding(EDGE_BIRD);
    let binding = e.sat_tune_nominal(DownlinkClass::Usb, 1_000_000);
    assert_eq!(e.split_tx_mhz(), None, "no uplink queued");
    assert!(
        !matches!(e.take_split_request(), Some(Some(_))),
        "nothing for the radio loop to write"
    );
    assert_eq!(binding.pending_uplink_mhz, None);
    assert_eq!(binding.note.as_deref(), Some(SPLIT_REFUSED_PRIVILEGES));

    // POSITIVE CONTROL: the same uplink queued without the pick's check is written to the Sub,
    // in LSB, and the key-time gate refuses it there.
    let mut un = ic9700_holding(EDGE_BIRD);
    un.sat_tune_nominal_unjudged(DownlinkClass::Usb, 1_000_000);
    let up_hz = (un.split_tx_mhz().expect("unjudged: queued") * 1e6).round() as u64;
    assert_eq!(
        un.sat_split_tx_vfo(up_hz, SatCatBackend::NativeCiv),
        Ok("Sub"),
        "the receiver the loop writes it to"
    );
    un.rig_split_applied_on(up_hz, ReceiverId::Sub);
    assert_eq!(un.tx_mode_effective(), "LSB", "the uplink's own word");
    assert!(!un.tx_allowed(), "the key refuses it");

    // CONTROL: the same uplink frequency on a NON-inverting bird keys USB, up and away from the
    // edge, and the pick queues it.
    let upright = tempo_core::doppler::Transponder {
        invert: false,
        ..EDGE_BIRD
    };
    let mut ok = ic9700_holding(upright);
    let binding = ok.sat_tune_nominal(DownlinkClass::Usb, 1_000_000);
    assert!(binding.pending_uplink_mhz.is_some(), "control: queued");
    assert!(matches!(ok.take_split_request(), Some(Some(_))));
}

/// The pick's check leaves the Doppler record as the pick's own seed writes it: with the uplink
/// refused, no uplink is recorded as sent, so the first tick corrects from nothing, judged
/// again when it asks.
#[test]
fn a_refused_picks_uplink_is_not_recorded_as_sent() {
    let mut e = ic9700_holding(super::tx_gate_table::EDGE_BIRD);
    e.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    let sent = e.sat_tune.as_ref().expect("a pass is held").sent;
    assert_eq!(sent.uplink_hz, 0, "the refused uplink was not sent");
    assert_eq!(sent.downlink_hz, 435_640_000, "the downlink was");
}

/// Under an UPLINK-ONLY mapping a refused pick writes neither leg, and the rail names the
/// licence, not the mapping (which is confirmed).
#[test]
fn an_uplink_only_picks_refusal_names_the_licence() {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.settings.ensure_radio_profiles();
    // A rig outside the Main/Sub satellite Icoms, whose A/B split carries a cross-band uplink.
    e.settings.rig_model = 1035;
    e.settings.sync_active_from_flat();
    let ids: Vec<u32> = e.settings.radios.iter().map(|p| p.id).collect();
    for id in ids {
        e.settings
            .confirm_sat_uplink(id, crate::settings::SatVfoMap::UplinkOnly);
    }
    e.set_license_class("general");
    e.set_operating_mode("phone", false);
    let edge = tempo_core::doppler::Transponder {
        uplink_centre_hz: 144_050_000,
        downlink_centre_hz: 435_640_000,
        invert: true,
        half_width_hz: 0,
    };
    e.set_sat_transponder(Some(("TEST|linear".into(), 0, edge)));
    let binding = e.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    assert_eq!(binding.pending_uplink_mhz, None);
    assert_eq!(binding.pending_downlink_mhz, None);
    assert_eq!(binding.note.as_deref(), Some(SPLIT_REFUSED_PRIVILEGES));

    // CONTROL: RS-44 under the same mapping is queued, with the mapping's own note.
    let mut rs = Engine::new("W9XYZ", "EN52", 0);
    rs.settings = e.settings.clone();
    rs.set_sat_transponder(Some(("TEST|linear".into(), 0, super::tx_gate_table::RS44)));
    let binding = rs.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    assert!(binding.pending_uplink_mhz.is_some(), "control: queued");
    assert_ne!(binding.note.as_deref(), Some(SPLIT_REFUSED_PRIVILEGES));
}

/// A DOPPLER STEER IS NOT A QSY. Mid-pass the dial is steered every few seconds without the
/// split being touched, so a keyed refusal of a later uplink step puts the uplink the radio has
/// back, and queues nothing: neither the refused step nor a return to simplex, which would put
/// the transmitter on the bird's own downlink.
#[test]
fn a_keyed_refusal_mid_pass_keeps_the_standing_uplink() {
    let mut e = ic9700_holding(super::tx_gate_table::RS44);
    e.sat_dial_owner = Some(("TEST|linear".into(), 0));
    e.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    let up = e.split_tx_mhz().expect("precondition: the uplink queued");
    assert_eq!(e.take_split_request(), Some(Some(up)));
    let up_hz = (up * 1e6).round() as u64;
    e.rig_split_applied_on(up_hz, ReceiverId::Sub);
    assert!(e.tx_allowed(), "precondition: the uplink keys");
    // A Doppler step moves the dial, and a later uplink step is refused at the write.
    let dial = e.settings().dial_mhz;
    e.steer_sat_dial(dial + 0.001);
    assert!(
        (e.settings().dial_mhz - dial - 0.001).abs() < 1e-9,
        "precondition: the steer moved the dial"
    );
    e.request_split(Some(up + 0.002));
    assert_eq!(e.take_split_request(), Some(Some(up + 0.002)));
    e.split_refused_while_keyed(up + 0.002);
    assert_eq!(e.take_split_request(), None, "nothing queued");
    assert_eq!(e.split_tx_mhz(), Some(up), "the radio's uplink stands");
    assert_eq!(e.tx_source(), ReceiverId::Sub);
    assert!(e.tx_allowed(), "and it is what the gate judges");
}

/// DISCLOSED, NOT CHANGED: a pick whose uplink is refused leaves the radio where its downlink
/// leg put it, on the downlink with no split, and nothing out of the licence queued. The rail
/// says why. Nexus's own keys are judged on that dial, which a General may key, as after a
/// split the rig rejected: whether a held pass with no uplink should refuse the key is the
/// operator's ruling (see `a_transponder_pick_has_never_been_what_stopped_the_mic`).
#[test]
fn a_refused_pick_leaves_the_radio_simplex_on_the_downlink() {
    let mut e = ic9700_holding(super::tx_gate_table::EDGE_BIRD);
    let binding = e.sat_tune_nominal(tempo_core::doppler::DownlinkClass::Usb, 1_000_000);
    assert_eq!(binding.note.as_deref(), Some(SPLIT_REFUSED_PRIVILEGES));
    assert!(
        (e.settings().dial_mhz - 435.640).abs() < 1e-9,
        "on the downlink"
    );
    assert_eq!(e.split_tx_mhz(), None, "Nexus shows no split");
    assert!(
        !matches!(e.take_split_request(), Some(Some(_))),
        "and asks the radio for none"
    );
    assert_eq!(e.tx_source(), ReceiverId::Main);
    assert!(
        e.tx_allowed(),
        "the dial is judged: inside the licence (the disclosed state)"
    );
}
