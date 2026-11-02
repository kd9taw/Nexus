//! The radio switch notice: what the snapshot says when Nexus switched the active radio BY ITSELF
//! (`Engine::auto_radio_switch`), and the one writer of the switch policy.
//!
//! The record is an observation only. Each scene below also checks the switch itself happened
//! exactly as before, so a record that changed the routing would show here too.

use super::*;
use crate::settings::{RadioSwitchPolicy, RouteMode, RoutingRule};

/// Radio 0 "HF" covers every band (no list) and is active on 20 m; radio 1 "VHF" lists 2 m.
fn two_radios() -> (Engine, u32) {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.sync_active_from_flat();
    let vhf = e.add_radio();
    e.rename_radio(0, "HF");
    e.rename_radio(vhf, "VHF");
    e.set_radio_bands(vhf, vec!["2m".into()]);
    e.set_frequency(14.074, "20m", "USB");
    assert_eq!(e.settings.active_radio, 0, "baseline: on the HF radio");
    (e, vhf)
}

#[test]
fn a_band_the_routing_hands_to_another_radio_is_recorded_with_where_it_came_from() {
    let (mut e, vhf) = two_radios();
    assert_eq!(
        e.snapshot().auto_radio_switch,
        None,
        "nothing switched by itself yet"
    );

    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(
        e.settings.active_radio, vhf,
        "the routing switched, as before"
    );
    let note = e
        .snapshot()
        .auto_radio_switch
        .expect("the switch is recorded");
    assert_eq!((note.from_id, note.to_id), (0, vhf));
    assert_eq!(note.band, "2m");
    assert_eq!(note.mode, e.route_mode("2m", 144.174));

    // A second automatic switch is a NEW notice: the sequence moves, the ids say which way.
    e.set_frequency(14.074, "20m", "USB");
    assert_eq!(e.settings.active_radio, 0);
    let back = e.snapshot().auto_radio_switch.expect("recorded again");
    assert!(back.seq > note.seq, "a new switch has a new sequence");
    assert_eq!(
        (back.from_id, back.to_id, back.band.as_str()),
        (vhf, 0, "20m")
    );
}

#[test]
fn the_operators_own_radio_pick_and_a_tune_that_stays_are_not_recorded() {
    let (mut e, vhf) = two_radios();
    // The operator's own pick (the radio pill, Settings' Make active): never "by itself".
    e.set_active_radio(vhf);
    assert_eq!(e.settings.active_radio, vhf);
    assert_eq!(
        e.snapshot().auto_radio_switch,
        None,
        "an operator pick is not a notice"
    );

    // A tune the active radio keeps: no switch, no notice.
    e.set_frequency(144.200, "2m", "USB");
    assert_eq!(e.settings.active_radio, vhf);
    assert_eq!(e.snapshot().auto_radio_switch, None, "nothing switched");

    // Pinned: the routing does not switch, so there is nothing to tell.
    e.set_active_radio(0);
    e.set_radio_pegged(true);
    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(e.settings.active_radio, 0, "pinned stays");
    assert_eq!(
        e.snapshot().auto_radio_switch,
        None,
        "a pinned tune switches nothing"
    );
}

#[test]
fn a_rule_switch_records_the_mode_class_the_rule_matched() {
    let (mut e, vhf) = two_radios();
    // Both radios cover 2 m only through a rule: digital on 2 m goes to VHF.
    e.set_radio_bands(vhf, Vec::new());
    e.set_routing_rules(vec![RoutingRule {
        bands: vec!["2m".into()],
        mode: Some(RouteMode::Digital),
        context: None,
        radio: vhf,
    }]);
    e.settings.operating_mode = crate::settings::OperatingMode::Digital;
    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(e.settings.active_radio, vhf, "the rule switched, as before");
    let note = e.snapshot().auto_radio_switch.expect("recorded");
    assert_eq!(note.mode, RouteMode::Digital);
}

/// Undo is the operator's own pick of the radio they were on (`set_active_radio`), so it brings
/// that radio's own dial back and is not itself a notice.
#[test]
fn undo_through_the_radio_pick_returns_the_previous_radio_and_its_dial() {
    let (mut e, vhf) = two_radios();
    e.set_frequency(14.080, "20m", "USB");
    e.set_frequency(144.174, "2m", "USB");
    let note = e.snapshot().auto_radio_switch.expect("recorded");
    assert_eq!(e.settings.active_radio, vhf);

    e.set_active_radio(note.from_id);
    assert_eq!(e.settings.active_radio, 0, "back on the HF radio");
    assert!(
        (e.settings.dial_mhz - 14.080).abs() < 1e-9,
        "the HF radio's own dial came back: {}",
        e.settings.dial_mhz
    );
    assert_eq!(e.settings.band, "20m");
    assert_eq!(
        e.snapshot().auto_radio_switch.map(|n| n.seq),
        Some(note.seq),
        "the Undo is the operator's pick, not a new notice"
    );
}

#[test]
fn a_form_save_cannot_move_the_switch_policy() {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_radio_switch_policy(RadioSwitchPolicy::Tell);
    let mut stale = e.settings().clone();
    stale.radio_switch_policy = RadioSwitchPolicy::Act;
    e.apply_settings(stale);
    assert_eq!(
        e.settings().radio_switch_policy,
        RadioSwitchPolicy::Tell,
        "a stale form moved it"
    );

    e.set_radio_switch_policy(RadioSwitchPolicy::Act);
    let mut stale = e.settings().clone();
    stale.radio_switch_policy = RadioSwitchPolicy::Tell;
    e.apply_settings(stale);
    assert_eq!(
        e.settings().radio_switch_policy,
        RadioSwitchPolicy::Act,
        "a stale form moved it"
    );
}

/// ONE RADIO, ONE WINDOW: the radio the routing (or the operator) picks is driven by another
/// Nexus window. The station stays on its radio, the notice says the switch was refused, and once
/// the other window lets go, the same tune takes the radio.
#[test]
fn a_switch_to_a_radio_another_window_drives_is_refused_and_said() {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("nexus-claims-engine-{pid}"));
    crate::radio_claim::init(dir.clone());
    let (mut e, vhf) = two_radios();
    let (hf_port, vhf_port) = (
        format!("/dev/nexus-route-hf-{pid}"),
        format!("/dev/nexus-route-vhf-{pid}"),
    );
    for p in e.settings.radios.iter_mut() {
        p.rig_model = 1029;
        p.ptt_method = "cat".into();
        p.serial_port = if p.id == vhf {
            vhf_port.clone()
        } else {
            hf_port.clone()
        };
    }
    let vhf_endpoint = format!("serial:{vhf_port}");
    let other_window = crate::radio_claim::Claims::new(dir.clone());
    other_window
        .claim(&vhf_endpoint)
        .expect("the other window drives the VHF radio");

    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(
        e.settings.active_radio, 0,
        "the station stayed on its radio"
    );
    let note = e.snapshot().auto_radio_switch.expect("the refusal is said");
    assert!(note.refused);
    assert_eq!(
        (note.from_id, note.to_id, note.band.as_str()),
        (0, vhf, "2m")
    );
    // The operator's own pick (and Remote's) goes through the same door.
    e.set_active_radio(vhf);
    assert_eq!(e.settings.active_radio, 0, "a pick is refused too");

    // The other window closes. (A child another test forks at that instant can hold a copy of the
    // handle until its exec, a few ms.)
    drop(other_window);
    let claims = crate::radio_claim::claims().expect("set up above");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while claims.held_elsewhere(&vhf_endpoint) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    e.set_frequency(14.074, "20m", "USB");
    e.set_frequency(144.174, "2m", "USB");
    assert_eq!(e.settings.active_radio, vhf, "now the routing takes it");
    let note = e.snapshot().auto_radio_switch.expect("recorded");
    assert!(!note.refused);
    // (That the claim moves with a switch is `radio_claim`'s own test: this process's claims are
    // shared by every engine test running beside this one, so which one holds last is theirs.)
}
