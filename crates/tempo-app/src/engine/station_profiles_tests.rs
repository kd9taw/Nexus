//! "My station" profiles at the Engine (`engine/station_profiles.rs`): what a contact carries
//! under each profile, what it never loses, the activation's pick, a stale form, and the licence
//! refusal on another licensee's call.

use super::*;
use crate::settings::{LicenseClass, StationProfile, HOME_STATION_PROFILE};
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
            s("IC-705"),
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
    // Without an activation, the profile's park still tags the contact, with the profile's grid.
    e.switch_station_profile(&id).unwrap();
    let p = log(&mut e, "K1ABC");
    assert_eq!(p.ota.my_ref.as_deref(), Some("US-1234"));
    assert_eq!(p.ota.my_program.as_deref(), Some("POTA"));
    assert_eq!(p.my_grid.as_deref(), Some("EN43"));
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
        Some("IC-705"),
        "what it was made with still is"
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
