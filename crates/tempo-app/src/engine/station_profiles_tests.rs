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
