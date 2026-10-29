//! A LoTW upload and the station profiles: each contact signed with the TQSL Station Location of
//! the profile it was logged under (`station::lotw_runs`), and each sent with the grid stored on
//! it when TQSL signs from the ADIF (`station::lotw_batch_adif`).

use super::*;
use crate::settings::{StationProfile, HOME_STATION_PROFILE};
use crate::station::{lotw_batch_adif, lotw_runs, LotwFallback, LotwRun};
use crate::test_util::StoredLog;

const ME: &str = "KD9TAW";

fn station() -> Engine {
    let mut e = Engine::new(ME, "EN52", 0);
    e.settings.op_state = "WI".into();
    e.settings.lotw_station_location = "Home".into();
    e
}

fn profile(name: &str, call: &str, grid: &str, location: &str) -> StationProfile {
    StationProfile {
        name: name.into(),
        call: call.into(),
        grid: grid.into(),
        state: "WI".into(),
        lotw_location: location.into(),
        ..StationProfile::default()
    }
}

/// Log a contact with `call` under the profile the station is on.
fn log(e: &mut Engine, call: &str) -> QsoRecord {
    let rec = e.qso_record(call.into(), None, None);
    e.log_qso(rec);
    e.stored_log()
        .iter()
        .find(|r| r.call == call)
        .map(|r| QsoRecord::clone(r))
        .expect("logged")
}

/// The batch as the upload splits it: the station's profiles with the active one's live call and
/// grid, and the station's own location (`None` signing from the ADIF).
fn runs(e: &Engine, batch: Vec<QsoRecord>) -> (Vec<LotwRun>, LotwFallback) {
    let mut s = e.settings.clone();
    s.sync_active_station_from_flat();
    let station = (!s.lotw_use_adif_location).then(|| s.lotw_station_location.clone());
    lotw_runs(batch, station.as_deref(), &s.station_profiles)
}

/// Each run as (location, the calls it signs).
fn shape(runs: &[LotwRun]) -> Vec<(Option<String>, Vec<String>)> {
    runs.iter()
        .map(|r| {
            (
                r.location.clone(),
                r.batch.iter().map(|q| q.call.clone()).collect(),
            )
        })
        .collect()
}

fn some(v: &str) -> Option<String> {
    Some(v.to_string())
}

/// ★ Two profiles naming two locations: every contact is signed with the location of the profile
/// it was logged under, one TQSL run per location, whichever profile the station is on at upload.
/// A profile naming none signs with the station's own location.
#[test]
fn each_contact_is_signed_with_its_own_profiles_location() {
    let mut e = station();
    let portable = e
        .save_station_profile(profile("Portable", "KD9TAW/P", "EN43", "Portable EN43"))
        .unwrap();
    let club = e
        .save_station_profile(profile("Club", "W9ABC", "EN53", "Club W9ABC"))
        .unwrap();
    let cabin = e
        .save_station_profile(profile("Cabin", ME, "EN56", ""))
        .unwrap();
    let home1 = log(&mut e, "K1HOM");
    e.switch_station_profile(&portable).unwrap();
    let p1 = log(&mut e, "K1POR");
    e.settings.license_class = crate::settings::LicenseClass::Open;
    e.switch_station_profile(&club).unwrap();
    let c1 = log(&mut e, "K1CLB");
    e.switch_station_profile(&cabin).unwrap();
    let cab = log(&mut e, "K1CAB");
    e.switch_station_profile(&portable).unwrap();
    let p2 = log(&mut e, "K2POR");
    // Uploaded from Home, after all of it.
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    let home2 = log(&mut e, "K2HOM");
    assert_eq!(
        (p1.station_callsign.as_deref(), p1.my_grid.as_deref()),
        (Some("KD9TAW/P"), Some("EN43")),
        "premise: the contact carries its profile's call and grid"
    );

    let (runs, fallback) = runs(&e, vec![home1, p1, c1, cab, p2, home2]);
    assert_eq!(
        fallback,
        LotwFallback::default(),
        "every contact fits one profile"
    );
    assert_eq!(
        shape(&runs),
        vec![
            (
                some("Home"),
                vec!["K1HOM".into(), "K1CAB".into(), "K2HOM".into()]
            ),
            (some("Portable EN43"), vec!["K1POR".into(), "K2POR".into()]),
            (some("Club W9ABC"), vec!["K1CLB".into()]),
        ],
        "Home and Cabin name no location of their own: the station's signs them"
    );
}

/// No profiles, or none naming a location: the whole batch is one run with the station's own
/// location, as before profiles; signing from the ADIF stays one run with no location.
#[test]
fn a_station_without_profile_locations_signs_as_before() {
    let mut e = station();
    let a = log(&mut e, "K1AAA");
    let b = log(&mut e, "K1BBB");
    let one = |e: &Engine| shape(&runs(e, vec![a.clone(), b.clone()]).0);
    let want = vec![(some("Home"), vec!["K1AAA".to_string(), "K1BBB".into()])];
    assert_eq!(one(&e), want, "no profiles");
    assert_eq!(
        runs(&e, vec![a.clone(), b.clone()]).1,
        LotwFallback::default(),
        "no profile names a location: nothing to tell the operator"
    );
    e.save_station_profile(profile("Portable", "KD9TAW/P", "EN43", ""))
        .unwrap();
    assert_eq!(one(&e), want, "a profile naming no location");
    e.settings.lotw_use_adif_location = true;
    assert_eq!(
        one(&e),
        vec![(None, vec!["K1AAA".to_string(), "K1BBB".into()])],
        "signing from the ADIF"
    );
}

/// A contact two profiles fit alike (same call and grid) is signed with their location when they
/// name the same one; when they name different ones it is not guessed: the station's own signs it,
/// and the count says so. A contact with no call or grid of its own takes the station's.
#[test]
fn a_contact_two_profiles_fit_is_never_guessed() {
    let mut e = station();
    e.save_station_profile(profile("Park A", "KD9TAW/P", "EN43", "Park A"))
        .unwrap();
    let b = e
        .save_station_profile(profile("Park B", "KD9TAW/P", "EN43", "Park A"))
        .unwrap();
    let mut rec = e.qso_record("K1TIE".into(), None, None);
    rec.station_callsign = some("KD9TAW/P");
    rec.my_grid = some("EN43wb");
    let (same, fallback) = runs(&e, vec![rec.clone()]);
    assert_eq!(
        (shape(&same), fallback),
        (
            vec![(some("Park A"), vec!["K1TIE".to_string()])],
            LotwFallback::default()
        ),
        "the same location named twice; a longer grid is the same square"
    );
    let mut other = e.settings.station_profiles.clone();
    other.iter_mut().find(|p| p.id == b).unwrap().lotw_location = "Park B".into();
    e.settings.station_profiles = other;
    let (tie, fallback) = runs(&e, vec![rec.clone()]);
    assert_eq!(
        (shape(&tie), fallback),
        (
            vec![(some("Home"), vec!["K1TIE".to_string()])],
            LotwFallback {
                tie: 1,
                unmatched: 0
            }
        )
    );
    let mut bare = rec;
    bare.my_grid = None;
    let (bare, fallback) = runs(&e, vec![bare]);
    assert_eq!(
        (shape(&bare), fallback),
        (
            vec![(some("Home"), vec!["K1TIE".to_string()])],
            LotwFallback {
                tie: 0,
                unmatched: 1
            }
        ),
        "no grid of its own: the station's, and counted"
    );
}

/// The `MY_GRIDSQUARE` values of a batch file, in order.
fn grids(adif: &str) -> Vec<String> {
    adif.split("<MY_GRIDSQUARE:")
        .skip(1)
        .map(|f| {
            let (len, rest) = f.split_once('>').expect("a field");
            rest[..len.parse::<usize>().expect("a length")].to_string()
        })
        .collect()
}

/// ★ Signing from the ADIF, a contact goes with the grid stored on it, never the grid the station
/// has at upload: a portable contact keeps its portable grid after the station is back home, and
/// a home contact keeps its own after the station's grid changes. Only a contact with no grid of
/// its own takes the live one. CONTROL: a home contact uploaded with the station where it was
/// logged is sent exactly as before; with a named Station Location a filled grid is still left out.
#[test]
fn signing_from_the_adif_sends_the_grid_stored_on_the_contact() {
    let mut e = station();
    e.settings.lotw_use_adif_location = true;
    let portable = e
        .save_station_profile(profile("Portable", "KD9TAW/P", "EN43", ""))
        .unwrap();
    // The home contact carries no place but its grid: a state Nexus filled now goes as stored
    // (the place test below), and this control is about the grid.
    e.settings.op_state = String::new();
    let home = log(&mut e, "K1HOM");
    e.switch_station_profile(&portable).unwrap();
    let away = log(&mut e, "K1POR");
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    assert_eq!(
        tempo_core::logbook::self_filled(&away),
        ["MY_GRIDSQUARE", "MY_STATE"],
        "premise: Nexus filled the portable grid from the profile"
    );
    let mut bare = e.qso_record("K1BAR".into(), None, None);
    bare.my_grid = None;

    // CONTROL: home again, the station's grid what it was: the home contact as it always went.
    let (call, grid) = (e.settings.mycall.clone(), e.settings.mygrid.clone());
    let control = lotw_batch_adif(std::slice::from_ref(&home), true, &call, &grid);
    let before = tempo_core::logbook::adif_record_with_station(
        &tempo_core::logbook::without_self_filled(&home),
        &call,
        &grid,
    );
    assert_eq!(
        control,
        format!("{}{before}", tempo_core::logbook::adif_header())
    );
    assert_eq!(grids(&control), ["EN52"]);

    // The portable contact uploaded from home.
    let back_home = lotw_batch_adif(std::slice::from_ref(&away), true, &call, &grid);
    assert_eq!(grids(&back_home), ["EN43"], "{back_home}");

    // The station's grid then changes (a move, a GPS fix).
    let moved = lotw_batch_adif(&[home.clone(), away.clone(), bare], true, ME, "EN61");
    assert_eq!(grids(&moved), ["EN52", "EN43", "EN61"], "{moved}");
    assert_eq!(moved.matches("<MY_GRIDSQUARE:").count(), 3, "one each");

    // A named Station Location still leaves out a grid Nexus filled.
    let named = lotw_batch_adif(&[home, away], false, ME, "EN61");
    assert!(grids(&named).is_empty(), "{named}");
}

/// The value of the ADIF field `tag` in each record of `adif`, in order.
fn values(adif: &str, tag: &str) -> Vec<String> {
    adif.split(&format!("<{tag}:"))
        .skip(1)
        .map(|f| {
            let (len, rest) = f.split_once('>').expect("a field");
            rest[..len.parse::<usize>().expect("a length")].to_string()
        })
        .collect()
}

/// The own-station place fields beside the grid that Nexus fills on a contact itself.
const PLACE: [&str; 5] = [
    "MY_STATE",
    "MY_CNTY",
    "MY_CQ_ZONE",
    "MY_ITU_ZONE",
    "MY_IOTA",
];

/// ★ Signing from the ADIF, a contact goes with the rest of the place Nexus filled on it too — its
/// state, county, CQ and ITU zones and IOTA, as stored — never the station's at upload: a /P
/// contact keeps its portable place after the profile is edited and the station is home with
/// another state. Each goes exactly as it would had the operator typed it. CONTROL: a contact
/// with none of them stored (only the grid Nexus filled) is sent byte for byte as before, with no
/// place field; with a named Station Location every filled one is still left out.
#[test]
fn signing_from_the_adif_sends_the_place_stored_on_the_contact() {
    let mut e = station();
    e.settings.lotw_use_adif_location = true;
    e.settings.op_state = String::new();
    let home = log(&mut e, "K1HOM");
    assert_eq!(
        tempo_core::logbook::self_filled(&home),
        ["MY_GRIDSQUARE"],
        "premise: the home contact carries no place but the grid"
    );
    let at = StationProfile {
        state: "MN".into(),
        county: "Itasca".into(),
        cq_zone: Some(4),
        itu_zone: Some(7),
        iota: "NA-001".into(),
        ..profile("Portable", "KD9TAW/P", "EN37", "")
    };
    let portable = e.save_station_profile(at.clone()).unwrap();
    e.switch_station_profile(&portable).unwrap();
    let away = log(&mut e, "K1POR");
    let filled = tempo_core::logbook::self_filled(&away);
    for tag in PLACE {
        assert!(
            filled.contains(&tag),
            "premise: Nexus filled {tag}: {filled:?}"
        );
    }
    // The live place then changes: the profile is edited, and the station is home in another state.
    e.save_station_profile(StationProfile {
        id: portable.clone(),
        state: "ND".into(),
        county: "Cass".into(),
        cq_zone: Some(5),
        itu_zone: Some(8),
        iota: String::new(),
        ..at
    })
    .unwrap();
    e.switch_station_profile(HOME_STATION_PROFILE).unwrap();
    e.settings.op_state = "IL".into();
    let (call, grid) = (e.settings.mycall.clone(), e.settings.mygrid.clone());

    let sent = lotw_batch_adif(&[home.clone(), away.clone()], true, &call, &grid);
    let want = [
        ("MY_STATE", "MN"),
        ("MY_CNTY", "MN,Itasca"),
        ("MY_CQ_ZONE", "4"),
        ("MY_ITU_ZONE", "7"),
        ("MY_IOTA", "NA-001"),
    ];
    for (tag, value) in want {
        assert_eq!(
            values(&sent, tag),
            [value],
            "{tag}, the portable contact's only: {sent}"
        );
    }
    for (tag, live) in [
        ("MY_STATE", "ND"),
        ("MY_STATE", "IL"),
        ("MY_CNTY", "ND,Cass"),
        ("MY_CQ_ZONE", "5"),
        ("MY_ITU_ZONE", "8"),
    ] {
        assert!(
            !values(&sent, tag).contains(&live.to_string()),
            "never the live {tag} {live}: {sent}"
        );
    }
    assert_eq!(grids(&sent), ["EN52", "EN37"], "{sent}");

    // Exactly as the operator's own typed place goes: the builder of before, given the contact
    // with only its grid left marked as Nexus's.
    let mut typed = away.clone();
    for (k, v) in &mut typed.extra {
        if k == tempo_core::logbook::SELF_FILLED_FIELD {
            *v = "MY_GRIDSQUARE".into();
        }
    }
    let typed_before = tempo_core::logbook::adif_record_with_station(
        &tempo_core::logbook::without_self_filled(&typed),
        &call,
        "EN37",
    );
    assert_eq!(
        lotw_batch_adif(std::slice::from_ref(&away), true, &call, &grid),
        format!("{}{typed_before}", tempo_core::logbook::adif_header())
    );

    // CONTROL: the contact with no place stored, byte for byte as before and with no place field.
    let control = lotw_batch_adif(std::slice::from_ref(&home), true, &call, &grid);
    let before = tempo_core::logbook::adif_record_with_station(
        &tempo_core::logbook::without_self_filled(&home),
        &call,
        &grid,
    );
    assert_eq!(
        control,
        format!("{}{before}", tempo_core::logbook::adif_header())
    );
    for tag in PLACE {
        assert!(values(&control, tag).is_empty(), "{tag}: {control}");
    }

    // A named Station Location still leaves out every place field Nexus filled.
    let named = lotw_batch_adif(&[home, away], false, &call, &grid);
    for tag in PLACE {
        assert!(values(&named, tag).is_empty(), "{tag}: {named}");
    }
}

/// A contact with the station call and grid it carries, as an import or the log wrote them.
fn made(e: &Engine, call: &str, station: &str, grid: &str) -> QsoRecord {
    let mut r = e.qso_record(call.into(), None, None);
    r.station_callsign = some(station);
    r.my_grid = some(grid);
    r
}

/// ★ A profile fits a contact on its 4-character grid square: a /P contact made in another
/// subsquare of the profile's square (a GPS fix that moved) is still the profile's. Only when
/// profiles of one call share a square and name different locations does the whole grid (6
/// characters or more, in any case) tell them apart; a contact whose grid tells none of them
/// apart (4 characters, or another subsquare) is not guessed: the station's own signs it, counted.
#[test]
fn a_profile_fits_on_its_grid_square_and_the_whole_grid_breaks_a_tie() {
    let mut e = station();
    e.save_station_profile(profile("Portable", "KD9TAW/P", "EN43AB", "Portable"))
        .unwrap();
    let (r, f) = runs(&e, vec![made(&e, "K1POR", "KD9TAW/P", "EN43CD")]);
    assert_eq!(
        (shape(&r), f),
        (
            vec![(some("Portable"), vec!["K1POR".to_string()])],
            LotwFallback::default()
        ),
        "another subsquare of the profile's square"
    );

    // Home at EN43AB, and a Club profile on the same call at EN43CD.
    e.settings.mygrid = "EN43AB".into();
    e.save_station_profile(profile("Club", ME, "EN43CD", "Club"))
        .unwrap();
    let batch = vec![
        made(&e, "K1CLB", ME, "EN43CD"),
        made(&e, "K1HOM", ME, "en43ab"),
        made(&e, "K1SQR", ME, "EN43"),
        made(&e, "K1ELS", ME, "EN43EF"),
    ];
    let (r, f) = runs(&e, batch);
    assert_eq!(
        shape(&r),
        vec![
            (some("Club"), vec!["K1CLB".to_string()]),
            (
                some("Home"),
                vec!["K1HOM".to_string(), "K1SQR".into(), "K1ELS".into()]
            ),
        ]
    );
    assert_eq!(
        f,
        LotwFallback {
            tie: 2,
            unmatched: 0
        },
        "EN43 and EN43EF tell Home and Club apart by nothing: counted; EN43ab is Home's"
    );
}

/// ★ The call decides as much as the grid: Home and a Club profile at the same grid with different
/// calls each sign their own contacts, with no tie. And an imported contact written in lower case
/// fits as its upper-case self does.
#[test]
fn the_call_decides_with_the_grid_and_case_never_does() {
    let mut e = station();
    e.save_station_profile(profile("Club", "W9ABC", "EN52", "Club"))
        .unwrap();
    let batch = vec![
        made(&e, "K1HOM", ME, "EN52"),
        made(&e, "K1CLB", "W9ABC", "EN52"),
        made(&e, "K1IMP", "w9abc", "en52xx"),
    ];
    let (r, f) = runs(&e, batch);
    assert_eq!(
        (shape(&r), f),
        (
            vec![
                (some("Home"), vec!["K1HOM".to_string()]),
                (some("Club"), vec!["K1CLB".to_string(), "K1IMP".into()]),
            ],
            LotwFallback::default()
        )
    );
}
