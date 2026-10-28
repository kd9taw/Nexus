//! "My station" profiles at the Engine (`engine/station_profiles.rs`): what a contact carries
//! under each profile, what it never loses, the activation's pick, a stale form, and the licence
//! refusal on another licensee's call.

use super::tests::dec_snr;
use super::*;
use crate::settings::{
    LicenseClass, StationProfile, HOME_STATION_PROFILE, STATION_PROFILE_SWITCH_REFUSED,
};
use crate::test_util::StoredLog;
use tempo_core::logbook::{extra_field, self_filled};

const ME: &str = "KD9TAW";

fn station() -> Engine {
    let mut e = Engine::new(ME, "EN52", 0);
    e.settings.op_state = "WI".into();
    e.settings.station_power_w = Some(100.0);
    e.settings.rig_model_name = "Icom IC-7300".into();
    e
}

fn portable() -> StationProfile {
    StationProfile {
        name: "Portable".into(),
        call: "KD9TAW/P".into(),
        grid: "EN43".into(),
        state: "WI".into(),
        county: "Sauk".into(),
        cq_zone: Some(4),
        itu_zone: Some(7),
        iota: String::new(),
        power_w: Some(10.0),
        rig: "IC-705".into(),
        antenna: "EFHW".into(),
        ..StationProfile::default()
    }
}

fn log(e: &mut Engine, call: &str) -> QsoRecord {
    let rec = e.qso_record(call.into(), None, None);
    log_rec(e, rec)
}

fn log_rec(e: &mut Engine, rec: QsoRecord) -> QsoRecord {
    let call = rec.call.clone();
    e.log_qso(rec);
    e.stored_log()
        .iter()
        .find(|r| r.call == call)
        .map(|r| QsoRecord::clone(r))
        .expect("logged")
}

/// What a contact says of the station: call, grid, state, county, zones, IOTA, park, rig,
/// antenna, power.
fn mine(r: &QsoRecord) -> Vec<Option<String>> {
    vec![
        r.station_callsign.clone(),
        r.my_grid.clone(),
        extra_field(r, "MY_STATE").map(str::to_string),
        extra_field(r, "MY_CNTY").map(str::to_string),
        extra_field(r, "MY_CQ_ZONE").map(str::to_string),
        extra_field(r, "MY_ITU_ZONE").map(str::to_string),
        extra_field(r, "MY_IOTA").map(str::to_string),
        r.ota.my_ref.clone(),
        r.my_rig.clone(),
        extra_field(r, "MY_ANTENNA").map(str::to_string),
        r.tx_power.map(|w| w.to_string()),
    ]
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

/// A station that never makes a profile: the contact carries exactly today's fields, the
/// snapshot offers no picker, and a save writes no profile keys.
#[test]
fn a_station_with_no_profiles_sees_no_change() {
    let mut e = station();
    let r = log(&mut e, "W1AW");
    assert_eq!(
        mine(&r),
        vec![
            s(ME),
            s("EN52"),
            s("WI"),
            None,
            None,
            None,
            None,
            None,
            s("Icom IC-7300"),
            None,
            s("100")
        ]
    );
    let snap = e.snapshot();
    assert!(snap.station_profiles.is_empty());
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains("stationProfiles"), "absent on the wire");
    let once = e.settings.clone();
    e.apply_settings(once);
    let before = serde_json::to_string(&e.settings).unwrap();
    assert!(!before.contains("stationProfiles") && !before.contains("activeStationProfile"));
    let again = e.settings.clone();
    e.apply_settings(again);
    assert_eq!(serde_json::to_string(&e.settings).unwrap(), before);
}

/// The first profile writes Home beside it; switching makes its call, grid, state and power the
/// station's and stamps its own fields, marked so the LoTW upload leaves the location out.
#[test]
fn a_profile_stamps_its_fields_on_new_contacts() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    assert_eq!(e.settings.station_profiles.len(), 2, "Home + Portable");
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME, "making one switches nothing");
    e.switch_station_profile(&id).unwrap();
    assert_eq!(e.settings.mycall, "KD9TAW/P");
    assert_eq!(e.snapshot().mycall, "KD9TAW/P", "the identity is rebound");
    let r = log(&mut e, "W1AW");
    assert_eq!(
        mine(&r),
        vec![
            s("KD9TAW/P"),
            s("EN43"),
            s("WI"),
            s("WI,Sauk"),
            s("4"),
            s("7"),
            None,
            None,
            s("Icom IC-7300"),
            s("EFHW"),
            s("10")
        ]
    );
    let marked = self_filled(&r);
    for tag in [
        "MY_GRIDSQUARE",
        "MY_STATE",
        "MY_CNTY",
        "MY_CQ_ZONE",
        "MY_ITU_ZONE",
    ] {
        assert!(marked.contains(&tag), "{tag} marked: {marked:?}");
    }
    let lotw = tempo_core::logbook::without_self_filled(&r);
    assert_eq!(extra_field(&lotw, "MY_CNTY"), None, "kept out of LoTW");
    assert_eq!(extra_field(&lotw, "MY_ANTENNA"), Some("EFHW"));
    // Home again: the profile kept what it had, and Home's contacts are Home's.
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    assert_eq!(
        (e.settings.mycall.as_str(), e.settings.mygrid.as_str()),
        (ME, "EN52")
    );
    let h = log(&mut e, "K1ABC");
    assert_eq!(h.station_callsign.as_deref(), Some(ME));
    assert_eq!(extra_field(&h, "MY_CNTY"), None);
}

/// A field the record arrives with is never overwritten by the profile.
#[test]
fn typed_values_are_never_overwritten() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    e.switch_station_profile(&id).unwrap();
    let mut rec = e.qso_record("W1AW".into(), None, None);
    rec.my_rig = s("KX3");
    rec.my_grid = s("FN31");
    rec.extra.push(("MY_CNTY".into(), "CT,Hartford".into()));
    rec.extra.push(("MY_ANTENNA".into(), "Dipole".into()));
    rec.extra.sort();
    let r = log_rec(&mut e, rec);
    assert_eq!(r.my_rig.as_deref(), Some("KX3"));
    assert_eq!(r.my_grid.as_deref(), Some("FN31"));
    assert_eq!(extra_field(&r, "MY_CNTY"), Some("CT,Hartford"));
    assert_eq!(extra_field(&r, "MY_ANTENNA"), Some("Dipole"));
    assert!(!self_filled(&r).contains(&"MY_CNTY"), "typed, not filled");
}

/// Switching mid-session changes only the contacts logged after it.
#[test]
fn a_switch_changes_only_later_contacts() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    let first = log(&mut e, "W1AW");
    e.switch_station_profile(&id).unwrap();
    let second = log(&mut e, "K1ABC");
    let first_again = e
        .stored_log()
        .iter()
        .find(|r| r.call == "W1AW")
        .map(|r| QsoRecord::clone(r))
        .unwrap();
    assert_eq!(first_again, first, "the earlier contact is untouched");
    assert_eq!(first.station_callsign.as_deref(), Some(ME));
    assert_eq!(second.station_callsign.as_deref(), Some("KD9TAW/P"));
}

/// A POTA activation of a profile's park puts the station on that profile, the activation's own
/// stamping keeps its park (and its directory grid), and Stop comes back to where it was.
#[test]
fn an_activation_picks_its_parks_profile_and_its_stamping_wins() {
    let mut e = station();
    let mut park = portable();
    park.name = "Devil's Lake".into();
    park.park = "US-1234".into();
    let id = e.save_station_profile(park).unwrap();
    e.set_activation("POTA", "US-1234", vec!["US-WI".into()])
        .unwrap();
    assert_eq!(e.settings.active_station_profile, id, "picked by itself");
    e.set_activation_grid("EN43ak").unwrap();
    let r = log(&mut e, "W1AW");
    assert_eq!(r.ota.my_ref.as_deref(), Some("US-1234"));
    assert_eq!(
        r.my_grid.as_deref(),
        Some("EN43AK"),
        "the activation's grid"
    );
    assert_eq!(extra_field(&r, "MY_CNTY"), Some("WI,Sauk"), "its own park");
    e.clear_activation();
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME);
    // Without an activation the profile's park tags nothing, and its place stays off the contact:
    // only a running activation of the park says a contact was made there.
    e.switch_station_profile(&id).unwrap();
    let p = log(&mut e, "K1ABC");
    assert_eq!(
        (p.ota.my_ref.as_deref(), p.ota.my_program.as_deref()),
        (None, None)
    );
    assert_eq!(extra_field(&p, "MY_CNTY"), None);
    assert_eq!(p.my_grid.as_deref(), Some("EN43"), "the station's grid");
}

/// An activation of ANOTHER park keeps its park, and the profile's location stays off it.
#[test]
fn another_parks_activation_takes_none_of_the_profiles_location() {
    let mut e = station();
    let mut park = portable();
    park.park = "US-1234".into();
    let id = e.save_station_profile(park).unwrap();
    e.switch_station_profile(&id).unwrap();
    e.set_activation("POTA", "US-5678", vec![]).unwrap();
    assert_eq!(e.settings.active_station_profile, id, "nothing to pick");
    let r = log(&mut e, "W1AW");
    assert_eq!(r.ota.my_ref.as_deref(), Some("US-5678"), "the activation's");
    assert_eq!(extra_field(&r, "MY_CNTY"), None);
    assert_eq!(extra_field(&r, "MY_CQ_ZONE"), None);
    assert_eq!(
        r.my_grid, None,
        "no grid rather than a wrong one, as before"
    );
    assert_eq!(
        r.my_rig.as_deref(),
        Some("Icom IC-7300"),
        "what it was made with still is: the configured radio"
    );
}

/// An activation never picks a profile under another licensee's call.
#[test]
fn an_activation_never_picks_another_licensees_call() {
    let mut e = station();
    let mut club = portable();
    club.call = "W9ABC".into();
    club.park = "US-1234".into();
    e.save_station_profile(club).unwrap();
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME);
}

/// A Settings form loaded on Home and saved after the switch cannot switch the station back:
/// its call goes to Home, and the station stays on the profile.
#[test]
fn a_stale_form_save_does_not_switch_back() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    let stale = e.settings.clone();
    e.switch_station_profile(&id).unwrap();
    e.apply_settings(stale);
    assert_eq!(e.settings.active_station_profile, id);
    assert_eq!(e.settings.mycall, "KD9TAW/P");
    assert_eq!(
        e.snapshot().mycall,
        "KD9TAW/P",
        "identity stays the profile's"
    );
    // A save from the profile's own form edits the profile.
    let mut form = e.settings.clone();
    form.mygrid = "EN44".into();
    e.apply_settings(form);
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    let p = e
        .settings
        .station_profiles
        .iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(p.grid, "EN44");
}

/// ⛔ The licence class is the operator's own: on a profile with another licensee's call, every
/// transmit path refuses while a class is set; a portable form of the same call does not, nor
/// does a station with no class set.
#[test]
fn another_licensees_call_refuses_transmit_under_a_licence_class() {
    let mut e = station();
    e.set_frequency(14.074, "20m", "USB");
    assert!(e.set_license_class("general"));
    let mut club = portable();
    club.call = "W9ABC".into();
    let club = e.save_station_profile(club).unwrap();
    let mine = e.save_station_profile(portable()).unwrap();
    assert!(e.tx_allowed(), "control: Home transmits");
    e.switch_station_profile(&mine).unwrap();
    assert!(e.tx_allowed(), "KD9TAW/P is the same licence");
    e.switch_station_profile(&club).unwrap();
    assert_eq!(
        e.settings.mycall, "W9ABC",
        "the identity is the profile's call"
    );
    assert!(!e.tx_allowed(), "another licensee's call is refused");
    assert_eq!(
        e.connection_tx_refusal(),
        Some(crate::settings::STATION_PROFILE_CALL_TX_REFUSED)
    );
    e.settings.license_class = LicenseClass::Open;
    assert!(e.tx_allowed(), "Open gates no call");
    e.settings.license_class = LicenseClass::General;
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    assert!(e.tx_allowed(), "back home");
}

/// Home and the profile on air stay; others go.
#[test]
fn removing_keeps_home_and_the_active_profile() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    assert!(e.remove_station_profile(HOME_STATION_PROFILE).is_err());
    e.switch_station_profile(&id).unwrap();
    assert!(e.remove_station_profile(&id).is_err());
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    e.remove_station_profile(&id).unwrap();
    assert_eq!(e.settings.station_profiles.len(), 1);
}

/// A whole-settings save — the Settings form, or one from a computer driving the station — can
/// neither switch the profile nor rewrite the list: both have their own verbs.
#[test]
fn a_whole_save_cannot_switch_or_rewrite_profiles() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    // A payload that names another profile and the call it has does not put the station on it.
    let mut payload = e.settings.clone();
    payload.active_station_profile = id.clone();
    payload.mycall = "KD9TAW/P".into();
    payload.mygrid = "EN43".into();
    e.apply_settings(payload);
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME);
    assert_eq!(e.snapshot().mycall, ME);
    // A payload that rewrites the list, or adds to it, changes nothing in it.
    let mut payload = e.settings.clone();
    payload.station_profiles[1].call = "W9XYZ".into();
    payload.station_profiles.push(StationProfile {
        id: "p9".into(),
        ..portable()
    });
    e.apply_settings(payload);
    assert_eq!(e.settings.station_profiles.len(), 2);
    assert_eq!(e.settings.station_profiles[1].call, "KD9TAW/P");
}

// --- A switch waits for the run, the contact and the over ---------------------------------------

/// A station on FT8, 20 m, with Home and a portable profile.
fn ft8_station() -> (Engine, String) {
    let mut e = station();
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    let id = e.save_station_profile(portable()).unwrap();
    (e, id)
}

/// What the FT sequencer has queued to send next.
fn tx_now(e: &Engine) -> String {
    e.snapshot()
        .qso
        .as_ref()
        .and_then(|q| q.tx_now.clone())
        .unwrap_or_default()
}

/// Every over keyed in `slots`, as its text.
fn keyed(e: &mut Engine, slots: std::ops::Range<u64>) -> Vec<String> {
    let mut sent = Vec::new();
    for slot in slots {
        let text = tx_now(e);
        if !e.poll_tx(slot).is_empty() {
            sent.push(text);
        }
    }
    sent
}

/// ⛔ The club call cannot reach the air by way of a switch. On a club profile (W9ABC) under a
/// licence class nothing transmits; the operator presses Call CQ anyway, so the run is built and
/// armed as W9ABC with every slot skipped. Picking Home now is refused: the run would go on
/// sending "CQ W9ABC" under the operator's own class. Nothing keys, and the station stays on the
/// club profile until the run is ended.
#[test]
fn a_run_built_under_a_refused_club_call_keeps_the_switch_from_putting_it_on_the_air() {
    let (mut e, _) = ft8_station();
    assert!(e.set_license_class("general"));
    let mut club = portable();
    club.call = "W9ABC".into();
    let club = e.save_station_profile(club).unwrap();
    e.switch_station_profile(&club).unwrap();
    assert!(!e.tx_allowed(), "premise: the club call is refused");
    e.start_cq(None).unwrap();
    assert!(e.tx_enabled(), "premise: the run is armed");
    assert_eq!(
        keyed(&mut e, 0..4),
        Vec::<String>::new(),
        "premise: slots skipped"
    );
    assert!(tx_now(&e).contains("W9ABC"), "premise: {}", tx_now(&e));
    let switched = e.switch_station_profile(HOME_STATION_PROFILE);
    let sent = keyed(&mut e, 4..12);
    assert!(
        sent.iter().all(|t| !t.contains("W9ABC")),
        "nothing on the air under the club call: {sent:?}"
    );
    assert_eq!(switched, Err(STATION_PROFILE_SWITCH_REFUSED.to_string()));
    assert_eq!(e.settings.mycall, "W9ABC");
    assert_eq!(e.settings.active_station_profile, club);
    assert!(e.snapshot().station_profile_locked, "the picker waits");
    // The operator ends the run (Stop TX, S&P): nothing left to send, and the switch goes.
    e.halt_tx();
    assert!(
        e.switch_station_profile(HOME_STATION_PROFILE).is_err(),
        "the CQ is still queued"
    );
    e.set_mode("qso-monitor").unwrap();
    assert!(!e.snapshot().station_profile_locked);
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    assert_eq!(e.settings.mycall, ME);
}

/// ⛔ A contact keeps one call from its first over to the log. Mid-contact (our roger queued, TX on)
/// the switch to the portable profile is refused, so every over goes out as KD9TAW and the contact
/// is logged with STATION_CALLSIGN KD9TAW: the call the other station worked.
#[test]
fn a_switch_mid_contact_is_refused_and_the_log_carries_the_call_that_was_sent() {
    let (mut e, portable) = ft8_station();
    e.ingest_decodes_for_test(&[dec_snr("CQ W1AW FN31", -12)], 11);
    e.work_station("W1AW", None, Some("CQ W1AW FN31"), Some(-12), Some(1500.0))
        .unwrap();
    let mut sent = keyed(&mut e, 12..14);
    e.ingest_decodes_for_test(&[dec_snr("KD9TAW W1AW -15", -9)], 14);
    assert!(
        tx_now(&e).starts_with("W1AW KD9TAW R"),
        "premise: {}",
        tx_now(&e)
    );
    let switched = e.switch_station_profile(&portable);
    sent.extend(keyed(&mut e, 15..17));
    e.ingest_decodes_for_test(&[dec_snr("KD9TAW W1AW RR73", -9)], 17);
    sent.extend(keyed(&mut e, 18..20));
    let logged = e
        .stored_log()
        .iter()
        .find(|r| r.call == "W1AW")
        .map(|r| QsoRecord::clone(r))
        .expect("logged");
    assert!(
        sent.len() >= 3,
        "premise: Tx1, roger and 73 went out: {sent:?}"
    );
    let on_air: Vec<&str> = sent
        .iter()
        .map(|t| t.split_whitespace().nth(1).unwrap_or_default())
        .collect();
    assert!(on_air.iter().all(|c| *c == ME), "sent as {on_air:?}");
    assert_eq!(logged.station_callsign.as_deref(), Some(on_air[0]));
    assert_eq!(switched, Err(STATION_PROFILE_SWITCH_REFUSED.to_string()));
    assert_eq!(e.settings.mycall, ME);
}

/// Saving the profile the station is on is a switch of its call, grid, state and power: refused
/// mid-run in the same way, with nothing saved. An edit of another profile goes ahead.
#[test]
fn saving_the_profile_on_air_waits_for_the_run_too() {
    let (mut e, portable_id) = ft8_station();
    e.start_cq(None).unwrap();
    let mut home = e.settings.station_profiles[0].clone();
    home.call = "KD9TAW/M".into();
    assert_eq!(
        e.save_station_profile(home),
        Err(STATION_PROFILE_SWITCH_REFUSED.to_string())
    );
    assert_eq!(e.settings.mycall, ME);
    assert_eq!(e.settings.station_profiles[0].call, ME, "nothing saved");
    let mut other = portable();
    other.id = portable_id;
    other.antenna = "Vertical".into();
    e.save_station_profile(other).unwrap();
    assert_eq!(e.settings.station_profiles[1].antenna, "Vertical");
}

/// POTA Start during a run starts the activation and leaves the profile, the transmit gate and the
/// planned over alone; Stop during a run likewise. Idle, Start picks the park's profile.
#[test]
fn pota_start_and_stop_leave_a_live_run_alone() {
    let (mut e, _) = ft8_station();
    let mut park = portable();
    park.park = "US-1234".into();
    let park = e.save_station_profile(park).unwrap();
    e.start_cq(None).unwrap();
    let gate = e.tx_gate_gen;
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(
        e.activation().map(|a| a.1).as_deref(),
        Some("US-1234"),
        "started"
    );
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.tx_gate_gen, gate, "the planned over is not cancelled");
    assert!(tx_now(&e).contains(ME), "the run goes on as it was");
    e.clear_activation();
    e.halt_tx();
    e.set_mode("qso-monitor").unwrap();
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(e.settings.active_station_profile, park, "idle: picked");
    e.start_cq(None).unwrap();
    let gate = e.tx_gate_gen;
    e.clear_activation();
    assert_eq!(
        e.settings.active_station_profile, park,
        "Stop mid-run stays"
    );
    assert_eq!(e.tx_gate_gen, gate);
}

/// From a computer driving the station, POTA Start and Stop never change the call the station
/// transmits under: the activation starts and ends, and the profile stays. A park profile under
/// the same call is still picked.
#[test]
fn pota_start_and_stop_from_afar_never_switch_the_call() {
    let mut e = station();
    let mut park = portable();
    park.park = "US-1234".into();
    let park = e.save_station_profile(park).unwrap();
    e.set_activation_from("POTA", "US-1234", vec![], true)
        .unwrap();
    assert!(e.activation().is_some(), "the activation starts");
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME);
    e.clear_activation_from(true);
    // At the station it is picked; Stop from afar would put the call back, so it stays.
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(e.settings.active_station_profile, park);
    e.clear_activation_from(true);
    assert_eq!(
        e.settings.mycall, "KD9TAW/P",
        "stays: the call is set at the station"
    );
    // CONTROL: a park profile with the station's own call is picked from afar.
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    let mut same = portable();
    same.call = ME.into();
    same.park = "US-5678".into();
    let same = e.save_station_profile(same).unwrap();
    e.set_activation_from("POTA", "US-5678", vec![], true)
        .unwrap();
    assert_eq!(e.settings.active_station_profile, same);
    e.clear_activation_from(true);
    assert_eq!(
        e.settings.active_station_profile, HOME_STATION_PROFILE,
        "and back"
    );
}

/// ⭐ A restart mid-activation. The activation put the station on the park's profile and the
/// settings kept it there; the launch takes the activation up PAUSED. A contact made then carries
/// no park and none of the park's county or zones; Stop still takes the station back to Home, as it
/// would have before the restart.
#[test]
fn a_restart_mid_activation_tags_nothing_and_stop_still_goes_back() {
    let dir = std::env::temp_dir().join(format!("nexus-profiles-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("activation.json");
    let mut before = station();
    before.set_activation_path(path.clone());
    let mut park = portable();
    park.park = "US-1234".into();
    let park = before.save_station_profile(park).unwrap();
    before.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(
        before.settings.active_station_profile, park,
        "premise: picked"
    );

    let mut e = Engine::new(ME, "EN52", 0);
    e.apply_restored_settings(before.settings.clone());
    e.set_activation_path(path.clone());
    e.restore_activation();
    assert_eq!(e.activation(), None, "premise: paused");
    assert_eq!(
        e.settings.active_station_profile, park,
        "premise: still on it"
    );
    let r = log(&mut e, "W1AW");
    assert_eq!(r.ota.my_ref, None, "no park");
    assert_eq!(
        [
            extra_field(&r, "MY_CNTY"),
            extra_field(&r, "MY_CQ_ZONE"),
            extra_field(&r, "MY_ITU_ZONE"),
        ],
        [None, None, None],
        "none of the park's place"
    );
    e.clear_activation();
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A contact held for the confirm takes the profile it was made under; switching before the
/// confirm does not add the new profile's antenna, county or zones to it.
#[test]
fn a_held_contact_is_not_restamped_by_the_profile_at_the_confirm() {
    let mut e = station();
    e.settings.prompt_to_log = true;
    let id = e.save_station_profile(portable()).unwrap();
    let rec = e.qso_record("W1HLD".into(), None, Some(-5));
    e.hold_pending_log(super::remote_logging::HeldQso::new(
        rec,
        super::remote_logging::GridSource::LookedUp,
    ));
    e.switch_station_profile(&id).unwrap();
    let shown = e.pending_log().cloned().unwrap();
    let key = e.pending_qso_log_key().unwrap_or_default();
    assert!(e.confirm_pending_log(&key, shown));
    let r = &e.stored_log()[0];
    assert_eq!(
        r.station_callsign.as_deref(),
        Some(ME),
        "premise: held at Home"
    );
    assert_eq!(
        [
            extra_field(r, "MY_ANTENNA"),
            extra_field(r, "MY_CNTY"),
            extra_field(r, "MY_CQ_ZONE"),
        ],
        [None, None, None]
    );
}

// --- Nothing else changes the call while the run, the contact or the message is under way --------

/// A station on the club profile (W9ABC) under a General licence, its CQ run armed and every slot
/// skipped by the licence refusal.
fn club_run() -> (Engine, String) {
    let (mut e, _) = ft8_station();
    assert!(e.set_license_class("general"));
    let mut club = portable();
    club.name = "Club".into();
    club.call = "W9ABC".into();
    let club = e.save_station_profile(club).unwrap();
    e.switch_station_profile(&club).unwrap();
    assert!(!e.tx_allowed(), "premise: the club call is refused");
    e.start_cq(None).unwrap();
    assert!(tx_now(&e).contains("W9ABC"), "premise: {}", tx_now(&e));
    assert_eq!(
        keyed(&mut e, 0..2),
        Vec::<String>::new(),
        "premise: slots skipped"
    );
    (e, club)
}

fn profile_call(e: &Engine, id: &str) -> String {
    e.settings
        .station_profiles
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.call.clone())
        .unwrap_or_default()
}

/// ⛔ The reviewer's sequence. Refused on the club call, the operator presses Call CQ, then reads
/// the refusal and types their own call into Settings ▸ Station and presses Save. The station's call
/// stays W9ABC, so the licence refusal still stands and nothing goes out as "CQ W9ABC". The rest
/// of the form is saved. Once the run is ended, the same save changes the call.
#[test]
fn a_settings_save_mid_run_keeps_the_stations_call() {
    let (mut e, club) = club_run();
    let mut form = e.settings.clone();
    form.mycall = ME.into();
    form.op_name = "Club op".into();
    e.apply_settings(form);
    let sent = keyed(&mut e, 2..10);
    assert!(
        sent.iter().all(|t| !t.contains("W9ABC")),
        "nothing on the air under the club call: {sent:?}"
    );
    assert_eq!(
        (
            e.settings.mycall.as_str(),
            profile_call(&e, &club).as_str(),
            e.snapshot().mycall.as_str(),
        ),
        ("W9ABC", "W9ABC", "W9ABC")
    );
    assert_eq!(e.settings.active_station_profile, club);
    assert!(!e.tx_allowed(), "still refused");
    assert_eq!(
        e.settings.op_name, "Club op",
        "the rest of the form is saved"
    );
    // CONTROL: the run ended, the same save changes the club profile's call.
    e.halt_tx();
    e.set_mode("qso-monitor").unwrap();
    let mut form = e.settings.clone();
    form.mycall = "W9ABD".into();
    e.apply_settings(form);
    assert_eq!(e.settings.mycall, "W9ABD");
    assert_eq!(profile_call(&e, &club), "W9ABD");
}

/// ⛔ A form loaded on Home before the switch to the club call, edited and saved mid-run: Home's
/// call is not rewritten either (it would lift the club call's refusal).
#[test]
fn a_stale_form_save_mid_run_changes_no_profiles_call() {
    let (mut e, club) = club_run();
    let mut stale = e.settings.clone();
    stale.active_station_profile = HOME_STATION_PROFILE.into();
    stale.mycall = "W9ABC".into();
    e.apply_settings(stale);
    assert_eq!(profile_call(&e, HOME_STATION_PROFILE), ME);
    assert_eq!(e.settings.mycall, "W9ABC");
    assert_eq!(e.settings.active_station_profile, club);
    let sent = keyed(&mut e, 2..10);
    assert!(sent.iter().all(|t| !t.contains("W9ABC")), "{sent:?}");
}

/// ⛔ A settings restore (or reset) mid-run is refused with the reason, and nothing changes; idle,
/// it goes.
#[test]
fn a_settings_restore_mid_run_is_refused() {
    let (e, club) = club_run();
    let mut bundle = e.settings.clone();
    bundle.active_station_profile = HOME_STATION_PROFILE.into();
    bundle.mycall = ME.into();
    assert_eq!(
        e.station_profile_restore_refusal(&Settings::default()),
        Some(crate::settings::STATION_PROFILE_RESTORE_REFUSED),
        "a reset too"
    );
    let engine = std::sync::Mutex::new(e);
    let restored = restore_with_session_rows(&engine, bundle.clone(), |_| ());
    let mut e = engine.into_inner().unwrap();
    assert_eq!(
        restored,
        Err(crate::settings::STATION_PROFILE_RESTORE_REFUSED.to_string())
    );
    assert_eq!(e.settings.mycall, "W9ABC");
    assert_eq!(e.settings.active_station_profile, club);
    let sent = keyed(&mut e, 2..10);
    assert!(sent.iter().all(|t| !t.contains("W9ABC")), "{sent:?}");
    // CONTROL: idle, the restore goes.
    e.halt_tx();
    e.set_mode("qso-monitor").unwrap();
    let engine = std::sync::Mutex::new(e);
    assert_eq!(restore_with_session_rows(&engine, bundle, |_| ()), Ok(()));
    assert_eq!(engine.into_inner().unwrap().settings.mycall, ME);
}

/// ⛔ Editing Home's call to the club's mid-run would lift the club call's refusal: refused, and
/// nothing is saved. An edit of Home that leaves the refusal standing goes.
#[test]
fn an_edit_of_home_that_lifts_the_refusal_waits_for_the_run() {
    let (mut e, _) = club_run();
    let mut home = e.settings.station_profiles[0].clone();
    home.call = "W9ABC".into();
    assert_eq!(
        e.save_station_profile(home),
        Err(STATION_PROFILE_SWITCH_REFUSED.to_string())
    );
    assert_eq!(profile_call(&e, HOME_STATION_PROFILE), ME, "nothing saved");
    let sent = keyed(&mut e, 2..10);
    assert!(sent.iter().all(|t| !t.contains("W9ABC")), "{sent:?}");
    let mut home = e.settings.station_profiles[0].clone();
    home.antenna = "Beam".into();
    e.save_station_profile(home).unwrap();
    assert_eq!(e.settings.station_profiles[0].antenna, "Beam");
}

/// ⛔ A JS8 message queued on the club profile is built with W9ABC. With TX off, the switch is
/// refused while it is queued; cleared, the switch goes.
#[test]
fn a_queued_js8_message_keeps_the_profile() {
    let mut e = station();
    let mut club = portable();
    club.call = "W9ABC".into();
    let club = e.save_station_profile(club).unwrap();
    e.switch_station_profile(&club).unwrap();
    e.js8_send(None, "HELLO FROM THE CLUB".into()).unwrap();
    assert!(!e.tx_enabled(), "premise: queued, TX off");
    assert_eq!(
        e.switch_station_profile(HOME_STATION_PROFILE),
        Err(crate::settings::STATION_PROFILE_JS8_QUEUED_REFUSED.to_string())
    );
    assert_eq!(e.settings.mycall, "W9ABC");
    assert!(e.snapshot().station_profile_locked);
    e.js8_drop_queue();
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    assert_eq!(e.settings.mycall, ME);
}

/// ⛔ With Auto-log and the log prompt both off, a finished FT contact waits for Log QSO. The
/// switch is refused until it is logged, and it is logged under the call its overs went out
/// with; then the switch goes.
#[test]
fn a_finished_contact_not_yet_logged_keeps_the_profile() {
    let (mut e, portable) = ft8_station();
    e.settings.auto_log = false;
    e.settings.prompt_to_log = false;
    e.ingest_decodes_for_test(&[dec_snr("CQ W1AW FN31", -12)], 11);
    e.work_station("W1AW", None, Some("CQ W1AW FN31"), Some(-12), Some(1500.0))
        .unwrap();
    let mut sent = keyed(&mut e, 12..14);
    e.ingest_decodes_for_test(&[dec_snr("KD9TAW W1AW -15", -9)], 14);
    sent.extend(keyed(&mut e, 15..17));
    e.ingest_decodes_for_test(&[dec_snr("KD9TAW W1AW RR73", -9)], 17);
    sent.extend(keyed(&mut e, 18..20));
    assert!(sent.len() >= 3, "premise: {sent:?}");
    assert!(e.stored_log().is_empty(), "premise: not logged by itself");
    // "Disable TX after 73": the radio loop turns TX off once our 73 has played.
    assert!(e.take_pending_tx_disable(), "premise: TX goes off after 73");
    e.set_tx_enabled(false);
    assert_eq!(
        keyed(&mut e, 20..22),
        Vec::<String>::new(),
        "the next slots: off the air"
    );
    assert!(!e.on_air(), "premise: off the air");
    let switched = e.switch_station_profile(&portable);
    assert!(e.log_current_qso(), "Log QSO");
    let logged = QsoRecord::clone(&e.stored_log()[0]);
    assert_eq!(logged.station_callsign.as_deref(), Some(ME));
    assert_eq!(
        switched,
        Err(crate::settings::STATION_PROFILE_UNLOGGED_REFUSED.to_string())
    );
    e.switch_station_profile(&portable).unwrap();
    assert_eq!(e.settings.mycall, "KD9TAW/P");
}

/// Stop during a run cannot take the station back to Home: the station stays on the park's
/// profile, Stop says once which profile to pick to go back, and the return is kept, so the next
/// activation of the park, stopped when idle, goes back to Home.
#[test]
fn a_stop_that_cannot_go_back_says_so_and_keeps_the_way_back() {
    let (mut e, _) = ft8_station();
    let mut park = portable();
    park.park = "US-1234".into();
    let park = e.save_station_profile(park).unwrap();
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(e.settings.active_station_profile, park, "premise: picked");
    e.start_cq(None).unwrap();
    let told = e.clear_activation_from(false);
    assert_eq!(e.settings.active_station_profile, park, "stays mid-run");
    assert_eq!(
        told.map(|p| (p.id, p.call)),
        Some((HOME_STATION_PROFILE.to_string(), ME.to_string()))
    );
    assert_eq!(
        e.clear_activation_from(false),
        None,
        "told once: nothing to stop now"
    );
    e.halt_tx();
    e.set_mode("qso-monitor").unwrap();
    e.set_activation("POTA", "US-1234", vec![]).unwrap();
    assert_eq!(e.clear_activation_from(false), None);
    assert_eq!(e.settings.active_station_profile, HOME_STATION_PROFILE);
    assert_eq!(e.settings.mycall, ME);
}

// --- A whole save writes no call into a profile the station is not on ---------------------------

/// Home (KD9TAW) and a club profile (W9ABC) under a General licence, the station idle.
fn home_and_club() -> (Engine, String) {
    let mut e = station();
    e.set_frequency(14.074, "20m", "USB");
    assert!(e.set_license_class("general"));
    let mut club = portable();
    club.name = "Club".into();
    club.call = "W9ABC".into();
    let club = e.save_station_profile(club).unwrap();
    (e, club)
}

/// ⛔ The reviewer's sequence. A computer driving the station opens Settings while the shack is on
/// Home; at the shack the operator picks the club profile; the far computer saves an unrelated
/// change. Its form still names Home, and the save from afar carries the shack's call (W9ABC).
/// Home's call stays KD9TAW, so the club call's licence refusal stands, and the rest of the form is
/// saved. The reverse (a stale form naming the club while the shack is on Home) leaves the club's
/// call alone, and so does a form that names a profile the station is not on on purpose.
#[test]
fn a_save_naming_a_profile_the_station_is_not_on_writes_no_call_into_it() {
    let (mut e, club) = home_and_club();
    let mut far_form = e.settings.clone();
    e.switch_station_profile(&club).unwrap();
    far_form.mycall = "W9ABC".into(); // what a save from afar takes from the shack
    far_form.op_name = "Far op".into();
    e.apply_settings(far_form);
    assert_eq!(
        (
            profile_call(&e, HOME_STATION_PROFILE),
            e.settings.mycall.clone(),
            e.settings.active_station_profile.clone(),
            e.connection_tx_refusal(),
            e.settings.op_name.clone(),
        ),
        (
            ME.to_string(),
            "W9ABC".to_string(),
            club.clone(),
            Some(crate::settings::STATION_PROFILE_CALL_TX_REFUSED),
            "Far op".to_string(),
        )
    );
    // The reverse: the station on Home, a form still on the club profile with Home's call.
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    let mut stale = e.settings.clone();
    stale.active_station_profile = club.clone();
    stale.mycall = ME.into();
    stale.mygrid = "FN31".into();
    e.apply_settings(stale);
    assert_eq!(
        (
            profile_call(&e, &club),
            e.settings.mycall.clone(),
            e.settings.mygrid.clone()
        ),
        ("W9ABC".to_string(), ME.to_string(), "EN52".to_string()),
        "the club profile keeps its call; the station keeps Home's call and grid"
    );
    // CONTROL: a form on the profile the station is on edits it.
    let mut form = e.settings.clone();
    form.mygrid = "EN44".into();
    e.apply_settings(form);
    let home_grid = e
        .settings
        .station_profiles
        .iter()
        .find(|p| p.id == HOME_STATION_PROFILE)
        .map(|p| p.grid.clone());
    assert_eq!(
        (e.settings.mygrid.as_str(), home_grid.as_deref()),
        ("EN44", Some("EN44"))
    );
}

/// ⛔ At the station: Settings opened on Home, the top bar switched to the club profile, and the
/// operator types the club's call into the Station tab's call field (it still shows Home's) and
/// saves. Home's call stays KD9TAW, and the club call stays refused.
#[test]
fn a_form_left_open_across_a_switch_does_not_rewrite_homes_call() {
    let (mut e, club) = home_and_club();
    let mut open_form = e.settings.clone();
    e.switch_station_profile(&club).unwrap();
    open_form.mycall = "W9ABC".into();
    e.apply_settings(open_form);
    assert_eq!(
        (profile_call(&e, HOME_STATION_PROFILE), e.tx_allowed()),
        (ME.to_string(), false)
    );
}

/// ⛔ A restored settings bundle whose active id names no profile, with the club's call in the
/// flat field: the station is put on Home with Home's call, never left transmitting the club call
/// unrefused.
#[test]
fn a_restore_with_a_dangling_active_id_lands_on_home() {
    let (mut e, _club) = home_and_club();
    let mut bundle = e.settings.clone();
    bundle.active_station_profile = "p9".into();
    bundle.mycall = "W9ABC".into();
    e.apply_restored_settings(bundle);
    assert_eq!(
        (
            e.settings.active_station_profile.as_str(),
            e.settings.mycall.as_str(),
            e.snapshot().mycall.as_str(),
        ),
        (HOME_STATION_PROFILE, ME, ME)
    );
}

// --- The keying-time call check --------------------------------------------------------------------
//
// Every message that carries the call it was built with is checked when it is about to go out: the
// call in it must be the station's call now, or it is not sent and the operator is told why. The
// switch refusals above keep the call from changing while a message waits; these tests change the
// call directly, as any path that slipped past them would, and show that the message still does
// not go out under the old call. Each has a control where the calls match and it keys.

const CALL_CHANGED: &str = crate::settings::STATION_CALL_CHANGED_TX_REFUSED;

/// The station's call becomes the profile `id`'s, with nothing refusing it: the switch's own
/// effect, without its lock.
fn call_becomes(e: &mut Engine, id: &str) {
    e.settings.active_station_profile = id.to_string();
    e.settings.sync_flat_from_active_station();
}

/// A station on FT8 with Home and KD9TAW/P, its CQ run started under KD9TAW.
fn ft8_run() -> (Engine, String) {
    let (mut e, id) = ft8_station();
    e.start_cq(None).unwrap();
    assert_eq!(tx_now(&e), "CQ KD9TAW EN52", "premise");
    (e, id)
}

/// ⛔ FT8: a CQ run built under KD9TAW does not key once the station's call is KD9TAW/P, and the
/// cockpit's transmit refusal says why. CONTROL: with the call unchanged the run keys its CQ.
#[test]
fn an_ft_run_built_under_another_call_does_not_key() {
    let (mut control, _) = ft8_run();
    assert_eq!(
        keyed(&mut control, 0..4),
        vec!["CQ KD9TAW EN52".to_string(); 2],
        "CONTROL: the calls match and the run keys"
    );
    assert_eq!(control.snapshot().radio.tx_refusal, None);
    let (mut e, id) = ft8_run();
    call_becomes(&mut e, &id);
    assert_eq!(keyed(&mut e, 0..4), Vec::<String>::new(), "nothing keyed");
    assert_eq!(e.snapshot().radio.tx_refusal.as_deref(), Some(CALL_CHANGED));
}

/// A Field Day run on FT8 with Home and KD9TAW/P, started under KD9TAW.
fn field_day_run() -> (Engine, String) {
    let (mut e, id) = ft8_station();
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-run").unwrap();
    assert!(matches!(e.mode, Mode::FieldDay { .. }), "premise");
    (e, id)
}

/// Overs keyed in `slots`, counted.
fn overs(e: &mut Engine, slots: std::ops::Range<u64>) -> usize {
    slots.filter(|&slot| !e.poll_tx(slot).is_empty()).count()
}

/// ⛔ Field Day: the run's CQ, built under KD9TAW, does not key once the call is KD9TAW/P.
/// CONTROL: unchanged, it keys.
#[test]
fn a_field_day_run_built_under_another_call_does_not_key() {
    let (mut control, _) = field_day_run();
    assert_eq!(overs(&mut control, 0..4), 2, "CONTROL: the run keys");
    let (mut e, id) = field_day_run();
    call_becomes(&mut e, &id);
    assert_eq!(overs(&mut e, 0..4), 0, "nothing keyed");
    assert_eq!(e.snapshot().radio.tx_refusal.as_deref(), Some(CALL_CHANGED));
}

/// A JS8 station with Home and KD9TAW/P, TX on, one message queued under KD9TAW, and the slot of
/// the next period.
fn js8_queued() -> (Engine, String, u64) {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_offset(1500.0);
    e.set_tx_enabled(true);
    e.js8_send(None, "HELLO FROM HOME".into()).unwrap();
    let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
    let slot = tempo_core::timing::now_unix_ms() as u64 / period_ms + 1;
    (e, id, slot)
}

/// ⛔ JS8: a message queued under KD9TAW (its frames carry the call) is not keyed once the station's
/// call is KD9TAW/P; it is dropped, and the JS8 cockpit says why. CONTROL: unchanged, it keys.
#[test]
fn a_js8_message_queued_under_another_call_does_not_key() {
    let (mut control, _, slot) = js8_queued();
    assert!(control.plan_tx(slot).is_some(), "CONTROL: it keys");
    let (mut e, id, slot) = js8_queued();
    call_becomes(&mut e, &id);
    assert!(e.plan_tx(slot).is_none(), "nothing keyed");
    let js8 = e.js8_state();
    assert_eq!(
        (js8.queue.len(), js8.last_error.as_deref()),
        (0, Some(CALL_CHANGED)),
        "the message is dropped, and the reason shown"
    );
}

/// RTTY text as the demodulator hands it over.
fn rtty_heard(e: &mut Engine, text: &str) {
    let chars: Vec<_> = text
        .chars()
        .map(|ch| tempo_core::rtty::DecodedChar {
            ch,
            confidence: 0.9,
        })
        .collect();
    e.push_rtty_decode(&chars, 0.0, true);
}

/// RTTY Auto on Home (KD9TAW) with KD9TAW/P beside it: Auto CQ goes out, W1AW answers, our
/// reply goes out, and the sequencer is between overs waiting for W1AW's exchange.
fn rtty_auto_between_overs() -> (Engine, String) {
    let mut e = station();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    let id = e.save_station_profile(portable()).unwrap();
    e.set_operating_mode("rtty", false);
    e.set_frequency(14.080, "20m", "LSB");
    e.set_rtty_auto(true).unwrap();
    e.rtty_auto_cq().unwrap();
    let cq = e.poll_rtty_one().expect("premise: the CQ keys");
    assert!(cq.contains("KD9TAW"), "premise: {cq}");
    e.rtty_auto_service(); // the CQ has gone out
    rtty_heard(&mut e, "KD9TAW DE W1AW W1AW K\n");
    e.rtty_auto_service();
    let reply = e.poll_rtty_one().expect("premise: our reply keys");
    assert!(
        reply.contains("W1AW") && reply.contains("KD9TAW"),
        "premise: {reply}"
    );
    e.rtty_auto_service(); // …and has gone out: between overs, nothing queued
    assert!(e.rtty_queue.is_empty(), "premise: between overs");
    (e, id)
}

/// W1AW's exchange arrives, and the sequencer answers it.
fn w1aw_sends_the_exchange(e: &mut Engine) -> Option<String> {
    rtty_heard(e, "KD9TAW DE W1AW 599 BOB CT 599 BOB CT KD9TAW DE W1AW K\n");
    e.rtty_auto_service();
    e.poll_rtty_one()
}

/// ⛔ The reviewer's RTTY sequence. RTTY Auto is between overs, its contact with W1AW under way
/// under KD9TAW; the operator picks KD9TAW/P in the top bar (the switch is allowed between
/// overs). Nothing more goes out under KD9TAW: the contact ends, the cockpit says why, and
/// nothing is logged under KD9TAW/P for a contact the air carried as KD9TAW. CONTROL: without the
/// switch, the next over keys under KD9TAW.
#[test]
fn rtty_auto_between_overs_does_not_send_the_old_call_after_a_switch() {
    let (mut control, _) = rtty_auto_between_overs();
    let next = w1aw_sends_the_exchange(&mut control).unwrap_or_default();
    assert!(
        next.contains("KD9TAW"),
        "CONTROL: the next over keys: {next:?}"
    );
    let (mut e, id) = rtty_auto_between_overs();
    e.switch_station_profile(&id).unwrap();
    assert_eq!(e.settings.mycall, "KD9TAW/P", "premise: switched");
    let sent = w1aw_sends_the_exchange(&mut e);
    let st = e.rtty_state();
    assert_eq!(
        (
            sent,
            st.seq_state.as_str(),
            st.keyer_error.as_deref(),
            e.stored_log().len(),
        ),
        (None, "idle", Some(CALL_CHANGED), 0)
    );
}

/// ⛔ An RTTY over typed or sent from a macro under KD9TAW and still queued is not keyed once the
/// call is KD9TAW/P. CONTROL: unchanged, it keys.
#[test]
fn an_rtty_over_queued_under_another_call_does_not_key() {
    let make = || {
        let mut e = station();
        let id = e.save_station_profile(portable()).unwrap();
        e.set_operating_mode("rtty", false);
        e.set_frequency(14.080, "20m", "LSB");
        e.rtty_send_text("CQ CQ DE KD9TAW KD9TAW K").unwrap();
        (e, id)
    };
    let (mut control, _) = make();
    assert!(control.poll_rtty_one().is_some(), "CONTROL: it keys");
    let (mut e, id) = make();
    call_becomes(&mut e, &id);
    assert_eq!(e.poll_rtty_one(), None, "nothing keyed");
    assert_eq!(
        (e.rtty_queue.len(), e.rtty_state().keyer_error),
        (0, Some(CALL_CHANGED.to_string()))
    );
}

/// ⛔ CW: a macro's `{MYCALL}` is expanded when it is queued, so a message queued under KD9TAW is
/// not keyed once the call is KD9TAW/P; the rest of it is dropped and the CW cockpit says why.
/// CONTROL: unchanged, its first word keys.
#[test]
fn a_cw_macro_queued_under_another_call_does_not_key() {
    let make = || {
        let mut e = station();
        let id = e.save_station_profile(portable()).unwrap();
        e.set_operating_mode("cw", false);
        e.set_frequency(14.030, "20m", "CW");
        e.send_cw("CQ DE {MYCALL} K");
        assert!(e.cw_queue.contains(&"KD9TAW".to_string()), "premise");
        (e, id)
    };
    let (mut control, _) = make();
    assert_eq!(
        control.poll_cw_word().map(|w| w.0).as_deref(),
        Some("CQ"),
        "CONTROL: it keys"
    );
    let (mut e, id) = make();
    call_becomes(&mut e, &id);
    assert_eq!(e.poll_cw_word(), None, "nothing keyed");
    assert_eq!(
        (e.cw_queue.len(), e.cw_keyer_error().as_deref()),
        (0, Some(CALL_CHANGED))
    );
}

/// ⛔ PSK: a line queued under KD9TAW is not keyed once the call is KD9TAW/P. CONTROL: unchanged,
/// it keys.
#[test]
fn a_psk_line_queued_under_another_call_does_not_key() {
    let make = || {
        let mut e = station();
        let id = e.save_station_profile(portable()).unwrap();
        e.set_operating_mode("keyboard", false);
        e.set_frequency(14.070, "20m", "USB");
        e.psk_send_text("cq cq de kd9taw kd9taw k").unwrap();
        (e, id)
    };
    let (mut control, _) = make();
    assert!(control.poll_psk_one().is_some(), "CONTROL: it keys");
    let (mut e, id) = make();
    call_becomes(&mut e, &id);
    assert_eq!(e.poll_psk_one(), None, "nothing keyed");
    assert_eq!(
        (e.psk_queue.len(), e.psk_state().keyer_error),
        (0, Some(CALL_CHANGED.to_string()))
    );
}

/// ⛔ SSTV: a picture whose ID was drawn under KD9TAW is not sent once the call is KD9TAW/P; it is
/// dropped and the SSTV cockpit says why. CONTROL: unchanged, it keys.
#[test]
fn an_sstv_picture_made_under_another_call_does_not_key() {
    let make = || {
        let mut e = station();
        let id = e.save_station_profile(portable()).unwrap();
        e.set_frequency(14.230, "20m", "USB");
        e.set_operating_mode("phone", false);
        e.sstv_send_as(vec![0.0; 24_000], "PD-120".into(), ME)
            .unwrap();
        (e, id)
    };
    let (mut control, _) = make();
    assert!(control.poll_sstv_tx().is_some(), "CONTROL: it keys");
    let (mut e, id) = make();
    call_becomes(&mut e, &id);
    assert!(e.poll_sstv_tx().is_none(), "nothing keyed");
    assert_eq!(
        (e.sstv_sending(), e.sstv_tx_notice()),
        (false, Some(CALL_CHANGED))
    );
}

/// ⛔ APRS: a beacon rendered under KD9TAW (its AX.25 source) is not keyed once the call is
/// KD9TAW/P; it is dropped and the APRS status line says why. CONTROL: unchanged, it keys.
#[test]
fn an_aprs_beacon_made_under_another_call_does_not_key() {
    let make = || {
        let mut e = station();
        let id = e.save_station_profile(portable()).unwrap();
        e.set_operating_mode("phone", false);
        e.aprs_tune(144.390).unwrap();
        e.aprs_beacon(43.0, -89.4, '/', '>', "test", &[]).unwrap();
        (e, id)
    };
    let (mut control, _) = make();
    assert!(control.poll_aprs_tx().is_some(), "CONTROL: it keys");
    let (mut e, id) = make();
    call_becomes(&mut e, &id);
    assert!(e.poll_aprs_tx().is_none(), "nothing keyed");
    assert_eq!(e.aprs_tx_notice(), Some(CALL_CHANGED));
}

/// ⛔ JTTY: an over planned under KD9TAW is not keyed once the call is KD9TAW/P; it is dropped and
/// the JTTY cockpit says why. CONTROL: unchanged, it keys.
#[test]
fn a_jtty_over_planned_under_another_call_does_not_key() {
    use crate::engine::jtty_tx::{send_text, JttyTick};
    let make = || {
        let mut e = station();
        let id = e.save_station_profile(portable()).unwrap();
        e.set_operating_mode("jtty", false);
        e.set_frequency(14.090, "20m", "USB");
        e.set_tx_enabled(true);
        let m = std::sync::Mutex::new(e);
        send_text(&m, "CQ CQ DE KD9TAW K", "").unwrap();
        (m, id)
    };
    let (control, _) = make();
    assert!(
        matches!(
            engine_lock(&control).poll_jtty(true, 0.0),
            JttyTick::Frame(_)
        ),
        "CONTROL: it keys"
    );
    let (m, id) = make();
    let mut e = engine_lock(&m);
    call_becomes(&mut e, &id);
    assert!(
        matches!(e.poll_jtty(true, 0.0), JttyTick::Idle),
        "nothing keyed"
    );
    assert_eq!(e.jtty_state().tx.notice.as_deref(), Some(CALL_CHANGED));
}

// --- A contact set aside for Log QSO keeps the station it was made under -------------------------

/// ⛔ The reviewer's sequence. During a run under KD9TAW a caller goes quiet after the reports
/// crossed, and the run sets the contact aside for Log QSO. The operator presses S&P, picks
/// KD9TAW/P, then presses Log QSO: the contact is logged under KD9TAW and Home's grid and place,
/// the station its overs went out under, not the profile on when the button is pressed.
#[test]
fn a_contact_set_aside_logs_the_station_it_was_made_under() {
    let (mut e, id) = ft8_station();
    e.set_mode("qso-run").unwrap();
    e.poll_tx(0); // our CQ
    e.ingest_decodes_for_test(&[dec_snr("KD9TAW W1AW -10", -8)], 1);
    e.poll_tx(2); // our R-08
    e.ingest_decodes_for_test(&[], 3);
    for slot in [4u64, 6, 8] {
        e.poll_tx(slot);
        e.ingest_decodes_for_test(&[], slot + 1);
    }
    assert!(e.stalled_qso.is_some(), "premise: set aside");
    e.halt_tx();
    e.set_mode("qso-monitor").unwrap();
    e.switch_station_profile(&id).unwrap();
    assert_eq!(e.settings.mycall, "KD9TAW/P", "premise: switched");
    assert!(e.log_current_qso(), "Log QSO");
    let r = e.stored_log().last().map(|r| QsoRecord::clone(r)).unwrap();
    assert_eq!(r.call, "W1AW");
    assert_eq!(
        (
            r.station_callsign.as_deref(),
            r.my_grid.as_deref(),
            extra_field(&r, "MY_CNTY"),
            r.tx_power,
        ),
        (Some(ME), Some("EN52"), None, Some(100.0))
    );
}

/// ⛔ JTTY: while the Log QSO prompt for the contact a TU just ended is asked, the profile waits
/// (the screen logs it under the call the station has when it is answered). Answered, it goes.
#[test]
fn a_jtty_log_prompt_keeps_the_profile_until_it_is_answered() {
    use crate::engine::jtty_tx::{send_text, JttyTick};
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    e.set_operating_mode("jtty", false);
    e.set_frequency(14.090, "20m", "USB");
    e.set_tx_enabled(true);
    let m = std::sync::Mutex::new(e);
    send_text(&m, "TU 73", "W1AW").unwrap();
    for _ in 0..1000 {
        if matches!(engine_lock(&m).poll_jtty(true, 0.0), JttyTick::Idle) {
            break;
        }
    }
    let mut e = engine_lock(&m);
    e.set_tx_enabled(false);
    let prompt = e
        .jtty_state()
        .tx
        .log_prompt
        .clone()
        .expect("premise: asked");
    assert_eq!(prompt.call, "W1AW", "premise");
    assert_eq!(
        e.switch_station_profile(&id),
        Err(crate::settings::STATION_PROFILE_JTTY_PROMPT_REFUSED.to_string())
    );
    let _ = e.jtty_log_prompt_answer(prompt.id); // Not now
    e.switch_station_profile(&id).unwrap();
    assert_eq!(e.settings.mycall, "KD9TAW/P");
}

// --- MY_RIG: the radio's model wins ---------------------------------------------------------------

/// MY_RIG on a profile contact: a configured radio's model name, as Nexus has always logged it;
/// the profile's rig only when no radio is configured.
#[test]
fn a_configured_radio_names_the_rig_and_the_profile_only_without_one() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    e.switch_station_profile(&id).unwrap();
    let with_radio = log(&mut e, "W1AW");
    e.settings.rig_model_name = "None / VOX".into();
    let without = log(&mut e, "K1ABC");
    assert_eq!(
        (with_radio.my_rig.as_deref(), without.my_rig.as_deref()),
        (Some("Icom IC-7300"), Some("IC-705"))
    );
}

/// ⛔ SSTV: a picture whose preset text says "DE KD9TAW" is refused once the call is KD9TAW/P.
/// CONTROL: unchanged, or with no preset text, it is not.
#[test]
fn an_sstv_picture_whose_text_names_another_call_is_refused() {
    let mut e = station();
    let id = e.save_station_profile(portable()).unwrap();
    let text = vec![ME.to_string()];
    assert_eq!(e.sstv_overlay_call_refusal(&text), None, "CONTROL");
    call_becomes(&mut e, &id);
    assert_eq!(
        (
            e.sstv_overlay_call_refusal(&text),
            e.sstv_overlay_call_refusal(&[])
        ),
        (Some(CALL_CHANGED), None)
    );
}

/// ⛔ Review 1's and review 2's sequence, held by the keying-time call check alone: the club run
/// (W9ABC, refused under a General licence) is armed with every slot skipped, and the station's call
/// becomes Home's by a path that slipped past every lock (here, directly). The licence refusal
/// lifts, and the run's "CQ W9ABC" still never keys. CONTROL: a run built under Home keys.
#[test]
fn a_club_run_never_keys_once_the_call_is_home_whatever_moved_it() {
    let (mut e, _club) = club_run();
    call_becomes(&mut e, HOME_STATION_PROFILE);
    assert!(e.tx_allowed(), "premise: the licence refusal has lifted");
    let sent = keyed(&mut e, 2..10);
    assert_eq!(sent, Vec::<String>::new(), "nothing keyed: {sent:?}");
    // CONTROL: S&P, then Call CQ again under Home: it keys.
    e.halt_tx();
    e.set_mode("qso-monitor").unwrap();
    e.start_cq(None).unwrap();
    assert_eq!(keyed(&mut e, 10..14), vec!["CQ KD9TAW EN52".to_string(); 2]);
}

// --- The keying-time call check on a station with no profiles ------------------------------------
//
// The check runs on every station. With no profiles the call changes in Settings: a message made
// before the edit is not sent under the old call. A call stored with other capitals or spaces is the
// same call and keys.

/// The operator edits the callsign in Settings and saves, while a message waits.
fn settings_call_edit(e: &mut Engine, call: &str) {
    let mut s = e.settings().clone();
    s.mycall = call.into();
    e.apply_settings(s);
    assert_eq!(e.settings.mycall, call, "premise: the save took the call");
}

/// One mode family on a station with no profiles: a message made under KD9TAW and waiting
/// (`make`), whether the next try keys it (`key`), and what its cockpit says (`notice`).
struct Waiting {
    name: &'static str,
    make: fn() -> Engine,
    key: fn(&mut Engine) -> bool,
    notice: fn(&Engine) -> Option<String>,
}

fn plain_ft8() -> Engine {
    let mut e = station();
    e.set_tier(Tier::Ft8);
    e.set_frequency(14.074, "20m", "USB");
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e
}

fn plain_ft8_run() -> Engine {
    let mut e = plain_ft8();
    e.start_cq(None).unwrap();
    assert_eq!(tx_now(&e), "CQ KD9TAW EN52", "premise");
    e
}

fn plain_field_day_run() -> Engine {
    let mut e = plain_ft8();
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-run").unwrap();
    assert!(matches!(e.mode, Mode::FieldDay { .. }), "premise");
    e
}

fn ft_notice(e: &Engine) -> Option<String> {
    e.snapshot().radio.tx_refusal
}

fn plain_rtty_auto_between_overs() -> Engine {
    let mut e = station();
    e.settings.auto_log = true;
    e.settings.prompt_to_log = false;
    e.set_operating_mode("rtty", false);
    e.set_frequency(14.080, "20m", "LSB");
    e.set_rtty_auto(true).unwrap();
    e.rtty_auto_cq().unwrap();
    assert!(e.poll_rtty_one().is_some(), "premise: the CQ keys");
    e.rtty_auto_service();
    rtty_heard(&mut e, "KD9TAW DE W1AW W1AW K\n");
    e.rtty_auto_service();
    assert!(e.poll_rtty_one().is_some(), "premise: our reply keys");
    e.rtty_auto_service();
    assert!(e.rtty_queue.is_empty(), "premise: between overs");
    e
}

fn plain_jtty() -> Engine {
    let mut e = station();
    e.set_operating_mode("jtty", false);
    e.set_frequency(14.090, "20m", "USB");
    e.set_tx_enabled(true);
    let m = std::sync::Mutex::new(e);
    crate::engine::jtty_tx::send_text(&m, "CQ CQ DE KD9TAW K", "").unwrap();
    m.into_inner().unwrap()
}

/// Every mode family that keys a message made earlier under the station's call.
const WAITING: &[Waiting] = &[
    Waiting {
        name: "FT8 run",
        make: plain_ft8_run,
        key: |e| keyed(e, 0..4).len() == 2,
        notice: ft_notice,
    },
    Waiting {
        name: "Field Day",
        make: plain_field_day_run,
        key: |e| overs(e, 0..4) == 2,
        notice: ft_notice,
    },
    Waiting {
        name: "JS8",
        make: || {
            let mut e = station();
            e.js8_enter();
            e.set_frequency(14.078, "20m", "USB");
            e.set_tx_offset(1500.0);
            e.set_tx_enabled(true);
            e.js8_send(None, "HELLO FROM HOME".into()).unwrap();
            e
        },
        key: |e| {
            let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
            let slot = tempo_core::timing::now_unix_ms() as u64 / period_ms + 1;
            e.plan_tx(slot).is_some()
        },
        notice: |e| e.js8_state().last_error,
    },
    Waiting {
        name: "RTTY Auto",
        make: plain_rtty_auto_between_overs,
        key: |e| w1aw_sends_the_exchange(e).is_some(),
        notice: |e| e.rtty_state().keyer_error,
    },
    Waiting {
        name: "RTTY text",
        make: || {
            let mut e = station();
            e.set_operating_mode("rtty", false);
            e.set_frequency(14.080, "20m", "LSB");
            e.rtty_send_text("CQ CQ DE KD9TAW KD9TAW K").unwrap();
            e
        },
        key: |e| e.poll_rtty_one().is_some(),
        notice: |e| e.rtty_state().keyer_error,
    },
    Waiting {
        name: "PSK",
        make: || {
            let mut e = station();
            e.set_operating_mode("keyboard", false);
            e.set_frequency(14.070, "20m", "USB");
            e.psk_send_text("cq cq de kd9taw kd9taw k").unwrap();
            e
        },
        key: |e| e.poll_psk_one().is_some(),
        notice: |e| e.psk_state().keyer_error,
    },
    Waiting {
        name: "CW",
        make: || {
            let mut e = station();
            e.set_operating_mode("cw", false);
            e.set_frequency(14.030, "20m", "CW");
            e.send_cw("CQ DE {MYCALL} K");
            assert!(e.cw_queue.contains(&ME.to_string()), "premise");
            e
        },
        key: |e| e.poll_cw_word().is_some(),
        notice: |e| e.cw_keyer_error(),
    },
    Waiting {
        name: "SSTV picture",
        make: || {
            let mut e = station();
            e.set_frequency(14.230, "20m", "USB");
            e.set_operating_mode("phone", false);
            e.sstv_send_as(vec![0.0; 24_000], "PD-120".into(), ME)
                .unwrap();
            e
        },
        key: |e| e.poll_sstv_tx().is_some(),
        notice: |e| e.sstv_tx_notice().map(String::from),
    },
    Waiting {
        name: "SSTV preset text",
        make: station,
        key: |e| e.sstv_overlay_call_refusal(&[ME.to_string()]).is_none(),
        notice: |e| {
            e.sstv_overlay_call_refusal(&[ME.to_string()])
                .map(String::from)
        },
    },
    Waiting {
        name: "APRS",
        make: || {
            let mut e = station();
            e.set_operating_mode("phone", false);
            e.aprs_tune(144.390).unwrap();
            e.aprs_beacon(43.0, -89.4, '/', '>', "test", &[]).unwrap();
            e
        },
        key: |e| e.poll_aprs_tx().is_some(),
        notice: |e| e.aprs_tx_notice().map(String::from),
    },
    Waiting {
        name: "JTTY",
        make: plain_jtty,
        key: |e| {
            matches!(
                e.poll_jtty(true, 0.0),
                crate::engine::jtty_tx::JttyTick::Frame(_)
            )
        },
        notice: |e| e.jtty_state().tx.notice,
    },
];

/// What each family does on a station with no profiles after `change`: keyed, and the notice.
fn waiting_after(change: fn(&mut Engine)) -> Vec<(&'static str, bool, Option<String>)> {
    WAITING
        .iter()
        .map(|w| {
            let mut e = (w.make)();
            assert!(
                e.settings.station_profiles.is_empty(),
                "premise: no profiles"
            );
            change(&mut e);
            let keyed = (w.key)(&mut e);
            (w.name, keyed, (w.notice)(&e))
        })
        .collect()
}

fn every_family(keyed: bool, notice: Option<&str>) -> Vec<(&'static str, bool, Option<String>)> {
    WAITING
        .iter()
        .map(|w| (w.name, keyed, notice.map(String::from)))
        .collect()
}

/// ⛔ No profiles: the callsign is edited in Settings (KD9TAW to KD9TAW/P) while a message made
/// under KD9TAW waits. In every mode family that message is not keyed, and its cockpit says why.
#[test]
fn without_profiles_a_settings_call_edit_drops_the_message_made_under_the_old_call() {
    assert_eq!(
        waiting_after(|e| settings_call_edit(e, "KD9TAW/P")),
        every_family(false, Some(CALL_CHANGED))
    );
}

/// CONTROL for the test above: no profiles, the call unchanged (a Settings save that leaves it as it
/// is included), and every family keys its message with no notice.
#[test]
fn without_profiles_an_unchanged_call_keys_every_family() {
    assert_eq!(waiting_after(|_| {}), every_family(true, None));
    assert_eq!(
        waiting_after(|e| settings_call_edit(e, ME)),
        every_family(true, None)
    );
}

/// ⛔ No false refusal: the stored call differs from the one the message was made under only by
/// capitals and surrounding spaces. That is the same call, and every family keys.
#[test]
fn without_profiles_a_call_stored_in_other_capitals_or_spaces_still_keys() {
    assert_eq!(
        waiting_after(|e| e.settings.mycall = "  kd9taw ".into()),
        every_family(true, None)
    );
}

/// ⛔ The RTTY Auto contact under way when the call is edited ends without logging anything under
/// the new call. CONTROL: unchanged, the contact goes on.
#[test]
fn without_profiles_rtty_auto_logs_nothing_under_an_edited_call() {
    let mut control = plain_rtty_auto_between_overs();
    assert!(w1aw_sends_the_exchange(&mut control).is_some(), "CONTROL");
    let mut e = plain_rtty_auto_between_overs();
    settings_call_edit(&mut e, "KD9TAW/P");
    assert_eq!(
        (
            w1aw_sends_the_exchange(&mut e),
            e.rtty_state().seq_state,
            e.stored_log().len()
        ),
        (None, "idle".to_string(), 0)
    );
}

/// The check by value: the calls compared as the transmit path reads them.
#[test]
fn the_call_check_compares_calls_as_the_transmit_path_reads_them() {
    let cases: &[(&str, &str)] = &[
        ("KD9TAW", "KD9TAW"),
        ("kd9taw ", " KD9TAW"),
        ("<KD9TAW/P>", "KD9TAW/P"),
        ("PJ4/KD9TAW", "pj4/kd9taw"),
        ("", "KD9TAW"),
        ("", ""),
        ("KD9TAW", "KD9TAW/P"),
        ("KD9TAW/P", "KD9TAW"),
        ("PJ4/KD9TAW", "KD9TAW"),
        ("KD9TAW", ""),
    ];
    let mut e = station();
    let got: Vec<_> = cases
        .iter()
        .map(|(built, now)| {
            e.settings.mycall = (*now).into();
            e.sent_call_refusal(built)
        })
        .collect();
    let refused = Some(CALL_CHANGED);
    assert_eq!(
        got,
        vec![None, None, None, None, None, None, refused, refused, refused, refused]
    );
}

/// ⛔ A compound call, no profiles: a CQ run under PJ4/KD9TAW and the contact a caller starts with
/// it (our call hashed in the report) key as before. Then a Settings edit to KD9TAW stops the run.
#[test]
fn without_profiles_a_compound_call_keys_and_an_edit_away_from_it_does_not() {
    let make = || {
        let mut e = plain_ft8();
        e.settings.mycall = "PJ4/KD9TAW".into();
        e.app.set_identity("PJ4/KD9TAW", "EN52");
        e.set_mode("qso-run").unwrap();
        e
    };
    let mut e = make();
    let mut sent = keyed(&mut e, 0..1);
    e.ingest_decodes_for_test(&[dec_snr("<PJ4/KD9TAW> W1AW FN31", -8)], 1);
    sent.extend(keyed(&mut e, 2..3));
    assert_eq!(
        (sent, ft_notice(&e)),
        (
            vec![
                "CQ PJ4/KD9TAW".to_string(),
                "W1AW <PJ4/KD9TAW> -08".to_string()
            ],
            None
        )
    );
    let mut e = make();
    settings_call_edit(&mut e, "KD9TAW");
    assert_eq!(
        (keyed(&mut e, 0..4), ft_notice(&e)),
        (Vec::<String>::new(), Some(CALL_CHANGED.to_string()))
    );
}

/// ⛔ Field Day, no profiles: the exchange that carries the call (our answer to a caller) keys when
/// the stored call differs only in capitals and spaces.
#[test]
fn without_profiles_a_field_day_exchange_keys_under_a_call_stored_in_other_capitals() {
    /// Field Day overs keyed in `slots`, as their text.
    fn fd_keyed(e: &mut Engine, slots: std::ops::Range<u64>) -> Vec<String> {
        let mut sent = Vec::new();
        for slot in slots {
            let text = match &e.mode {
                Mode::FieldDay { station, .. } => station.outgoing().map(|m| m.to_text()),
                _ => None,
            };
            if !e.poll_tx(slot).is_empty() {
                sent.push(text.unwrap_or_default());
            }
        }
        sent
    }
    let mut e = plain_field_day_run();
    e.settings.mycall = " kd9taw".into();
    let mut sent = fd_keyed(&mut e, 0..1);
    e.ingest_decodes_for_test(&[dec_snr("KD9TAW W1AW 2A CT", -8)], 1);
    sent.extend(fd_keyed(&mut e, 2..3));
    assert_eq!(sent.len(), 2, "both overs key: {sent:?}");
    assert!(sent[1].contains("W1AW KD9TAW"), "the exchange: {sent:?}");
    assert_eq!(ft_notice(&e), None);
}

/// ⛔ A station that had no call yet: what it queued then carries no call and is not refused once the
/// call is set; a run started after the call is set keys under it.
#[test]
fn a_message_queued_before_the_call_was_set_is_not_refused_for_it() {
    let mut e = Engine::new("", "EN52", 0);
    e.set_operating_mode("cw", false);
    e.set_frequency(14.030, "20m", "CW");
    e.send_cw("TEST");
    assert_eq!(
        e.cw_queue_call.as_deref(),
        Some(""),
        "premise: queued with no call"
    );
    settings_call_edit(&mut e, ME);
    assert_eq!(
        (e.poll_cw_word().map(|w| w.0), e.cw_keyer_error()),
        (Some("TEST".to_string()), None)
    );
}
