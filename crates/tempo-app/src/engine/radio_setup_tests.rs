//! The radio setup (Settings ▸ Radio ▸ Radios ▸ Set up radios…): its "Where would this go?"
//! preview, its read-only Test CAT for a radio not in use, and a station that opens the setup and
//! never presses Finish keeping its settings exactly as they were.

use super::radio_probe::{probe_radio_read_only, RadioProbeState};
use super::*;
use crate::settings::{RadioRoutingDraft, RouteMode, RoutingDraft, RoutingRule};
use std::sync::Arc;
use std::time::Duration;

/// FTdx10 (0, every band, the radio in use on 20 m), IC-9700 (lists 2 m, 70 cm) and FT-991A (lists
/// 2 m, 70 cm), with no rules: the station before the setup.
fn station() -> (Engine, u32, u32) {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.settings.ensure_radio_profiles();
    e.settings.rig_model = 1042;
    e.settings.sync_active_from_flat();
    let ic9700 = e.add_radio();
    e.set_active_radio(ic9700);
    e.settings.rig_model = 3081;
    e.settings.sync_active_from_flat();
    let ft991a = e.add_radio();
    e.set_active_radio(ft991a);
    e.settings.rig_model = 1035;
    e.settings.sync_active_from_flat();
    e.rename_radio(0, "FTdx10");
    e.rename_radio(ic9700, "IC-9700");
    e.rename_radio(ft991a, "FT-991A");
    e.set_radio_bands(ic9700, vec!["2m".into(), "70cm".into()]);
    e.set_radio_bands(ft991a, vec!["2m".into(), "70cm".into()]);
    e.set_active_radio(0);
    e.set_frequency(14.074, "20m", "USB");
    assert_eq!(e.settings.active_radio, 0, "baseline: on the FTdx10");
    (e, ic9700, ft991a)
}

/// What the setup would show before Finish: the FTdx10 now lists HF only, the IC-9700 adds 6 m,
/// 2 m is split by mode, 70 cm CW goes to the FT-991A, and the FTdx10 is the default.
fn draft(ic9700: u32, ft991a: u32) -> RoutingDraft {
    let hf: Vec<String> = [
        "160m", "80m", "60m", "40m", "30m", "20m", "17m", "15m", "12m", "10m",
    ]
    .iter()
    .map(|b| b.to_string())
    .collect();
    let rule = |bands: &[&str], mode: RouteMode, radio: u32| RoutingRule {
        bands: bands.iter().map(|b| b.to_string()).collect(),
        mode: Some(mode),
        context: None,
        radio,
    };
    RoutingDraft {
        radios: vec![
            RadioRoutingDraft {
                id: 0,
                bands: hf,
                enabled: true,
            },
            RadioRoutingDraft {
                id: ic9700,
                bands: vec!["6m".into(), "2m".into(), "70cm".into()],
                enabled: true,
            },
            RadioRoutingDraft {
                id: ft991a,
                bands: vec!["2m".into(), "70cm".into()],
                enabled: true,
            },
        ],
        rules: vec![
            rule(&["2m", "70cm"], RouteMode::Digital, ic9700),
            rule(&["2m"], RouteMode::Fm, ft991a),
            rule(&["70cm"], RouteMode::Cw, ft991a),
        ],
        default_radio: Some(0),
    }
}

/// Finish, as the setup does it: the same verbs, one per thing that changed.
fn write(e: &mut Engine, d: &RoutingDraft) {
    for r in &d.radios {
        e.set_radio_bands(r.id, r.bands.clone());
    }
    e.set_routing_rules(d.rules.clone());
    e.set_default_radio(d.default_radio);
}

fn settings_json(e: &Engine) -> String {
    serde_json::to_string(&e.settings).expect("settings serialize")
}

const BANDS: [(&str, f64); 7] = [
    ("20m", 14.074),
    ("40m", 7.074),
    ("10m", 28.074),
    ("6m", 50.313),
    ("2m", 144.174),
    ("70cm", 432.174),
    ("23cm", 1296.174),
];
const MODES: [RouteMode; 5] = [
    RouteMode::Digital,
    RouteMode::Fm,
    RouteMode::Ssb,
    RouteMode::Cw,
    RouteMode::Rtty,
];

#[test]
fn where_would_this_go_answers_what_the_engine_routes_once_the_setup_is_finished() {
    let (before, ic9700, ft991a) = station();
    let d = draft(ic9700, ft991a);
    let untouched = settings_json(&before);
    let (mut after, _, _) = station();
    write(&mut after, &d);

    // The preview of the unsaved draft against the saved one, for every band × mode.
    let mut differs_from_today = 0;
    for (band, _) in BANDS {
        for mode in MODES {
            let preview = before.route_preview_draft(&d, band, mode);
            assert_eq!(
                preview,
                after.route_preview(band, mode),
                "{band} {mode:?}: the preview must be the routing Finish saves"
            );
            if preview != before.route_preview(band, mode) {
                differs_from_today += 1;
            }
        }
    }
    assert!(
        differs_from_today > 0,
        "control: the draft must route something differently from today, or this table proves nothing"
    );
    assert_eq!(
        settings_json(&before),
        untouched,
        "a preview stores nothing"
    );

    // And the engine's own QSY funnel lands where the preview said, for the modes a test can set
    // straight from the operating mode.
    use crate::settings::OperatingMode;
    for (band, dial) in BANDS {
        for (op, mode) in [
            (OperatingMode::Digital, RouteMode::Digital),
            (OperatingMode::Cw, RouteMode::Cw),
            (OperatingMode::Rtty, RouteMode::Rtty),
        ] {
            let (mut fresh, _, _) = station();
            let expect = fresh.route_preview_draft(&d, band, mode);
            write(&mut fresh, &d);
            fresh.settings.operating_mode = op;
            assert_eq!(fresh.route_mode(band, dial), mode, "{band}: test setup");
            fresh.set_frequency(dial, band, "USB");
            assert_eq!(
                fresh.settings.active_radio, expect,
                "{band} {mode:?}: set_frequency went where the preview said"
            );
        }
    }
}

#[test]
fn a_draft_that_turns_a_radio_off_previews_the_routing_skipping_it() {
    let (e, ic9700, ft991a) = station();
    let mut d = draft(ic9700, ft991a);
    assert_eq!(e.route_preview_draft(&d, "2m", RouteMode::Fm), ft991a);
    d.radios[2].enabled = false;
    assert_ne!(
        e.route_preview_draft(&d, "2m", RouteMode::Fm),
        ft991a,
        "a radio switched off in the draft is never the answer"
    );
    // The radio in use cannot be switched off (set_radio_enabled refuses it), so neither can a draft.
    d.radios[0].enabled = false;
    assert_eq!(e.route_preview_draft(&d, "20m", RouteMode::Ssb), 0);
}

#[test]
fn a_station_that_opens_the_setup_and_never_finishes_keeps_its_settings_file_exactly() {
    let (mut e, ic9700, ft991a) = station();
    // A real multi-radio file: rules, a default, a satellite rule and the policy as an existing
    // station has them.
    e.set_routing_rules(vec![
        RoutingRule {
            bands: vec!["2m".into()],
            mode: Some(RouteMode::Fm),
            context: None,
            radio: ft991a,
        },
        RoutingRule {
            bands: vec![],
            mode: None,
            context: Some(crate::settings::RouteContext::Satellite),
            radio: ic9700,
        },
    ]);
    e.set_default_radio(Some(0));
    let file = settings_json(&e);
    let loaded: crate::settings::Settings = serde_json::from_str(&file).expect("the file loads");
    e.settings = loaded;
    assert_eq!(settings_json(&e), file, "the file round-trips as stored");

    // Everything the setup asks of the station before Finish: previews of a draft, and the
    // read-only Test CAT of each radio.
    let d = draft(ic9700, ft991a);
    for (band, _) in BANDS {
        for mode in MODES {
            let _ = e.route_preview_draft(&d, band, mode);
            let _ = e.route_preview(band, mode);
        }
    }
    let shared = Arc::new(Mutex::new(e));
    for id in [0, ic9700, ft991a] {
        let _ = probe_radio_read_only(
            &shared,
            id,
            Duration::from_millis(30),
            Duration::from_millis(5),
        );
    }
    let e = engine_lock(&shared);
    assert_eq!(
        settings_json(&e),
        file,
        "Cancel writes nothing: the settings are byte for byte the stored file"
    );
}

#[test]
fn the_read_only_probe_never_selects_switches_or_keys_a_radio() {
    let (mut e, ic9700, ft991a) = station();
    e.set_tx_enabled(true);
    let before = settings_json(&e);
    let tx_before = (e.tx_enabled(), e.snapshot().radio.transmitting);
    let shared = Arc::new(Mutex::new(e));

    // The monitor's next read of the IC-9700, as its poll thread reports it.
    let monitor = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            let mut e = engine_lock(&shared);
            e.observe_radio_freq(ic9700, 144_174_000);
            e.observe_radio_cat(ic9700, Some(true));
        })
    };
    let got = probe_radio_read_only(
        &shared,
        ic9700,
        Duration::from_secs(5),
        Duration::from_millis(10),
    );
    monitor.join().unwrap();
    assert_eq!(got.state, RadioProbeState::Answered);
    assert_eq!(got.ok, Some(true));
    assert!((got.dial_mhz.unwrap() - 144.174).abs() < 1e-9);

    // No new report in the time allowed: says so, with the last known reading.
    let stale = probe_radio_read_only(
        &shared,
        ic9700,
        Duration::from_millis(40),
        Duration::from_millis(10),
    );
    assert_eq!(stale.state, RadioProbeState::NoFreshReport);
    assert_eq!(stale.ok, Some(true));

    // A report that the monitor could not read it.
    {
        let shared2 = Arc::clone(&shared);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            engine_lock(&shared2).observe_radio_cat(ft991a, Some(false));
        });
        let silent = probe_radio_read_only(
            &shared,
            ft991a,
            Duration::from_secs(5),
            Duration::from_millis(10),
        );
        t.join().unwrap();
        assert_eq!(silent.state, RadioProbeState::NotAnswering);
    }

    // The radio in use is test_cat's, and is answered without asking the radio loop anything.
    let in_use = probe_radio_read_only(
        &shared,
        0,
        Duration::from_secs(5),
        Duration::from_millis(10),
    );
    assert_eq!(in_use.state, RadioProbeState::InUse);

    let mut e = engine_lock(&shared);
    // The live reads above changed only the monitor's cache, never the settings.
    assert_eq!(e.settings.active_radio, 0, "the probe switched no radio");
    assert_eq!(settings_json(&e), before, "the probe wrote no setting");
    assert!(
        !e.take_cat_reprobe(),
        "the probe asked the radio loop for no CAT re-open of the radio in use"
    );
    assert_eq!(
        (e.tx_enabled(), e.snapshot().radio.transmitting),
        tx_before,
        "the probe keyed nothing and left the transmit latch as it was"
    );
}

#[test]
fn the_probe_names_a_radio_it_cannot_read_without_waiting() {
    let (mut e, ic9700, ft991a) = station();
    assert!(
        e.set_radio_enabled(ft991a, false),
        "an idle radio can be turned off"
    );
    if let Some(p) = e.settings.radios.iter_mut().find(|p| p.id == ic9700) {
        p.rig_model = 0;
    }
    let shared = Mutex::new(e);
    let long = Duration::from_secs(30);
    let step = Duration::from_millis(10);
    assert_eq!(
        probe_radio_read_only(&shared, ft991a, long, step).state,
        RadioProbeState::Disabled
    );
    assert_eq!(
        probe_radio_read_only(&shared, ic9700, long, step).state,
        RadioProbeState::NoCat
    );
    assert_eq!(
        probe_radio_read_only(&shared, 99, long, step).state,
        RadioProbeState::Unknown
    );
    // An OmniRig radio has CAT with no model number of Nexus's: not "no CAT", but not watched by the
    // monitor while another radio is in use, so there is nothing to read from here.
    if let Some(p) = engine_lock(&shared)
        .settings
        .radios
        .iter_mut()
        .find(|p| p.id == ic9700)
    {
        p.rig_conn = "omnirig".into();
    }
    assert_eq!(
        probe_radio_read_only(&shared, ic9700, long, step).state,
        RadioProbeState::NotMonitored
    );
}

/// One station of the setup's single-change fixture (`ui/src/settings/__fixtures__/
/// radio-setup-single-changes.json`, written by `radioSetup.test.ts` from the setup's own compiler).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureStation {
    name: String,
    radios: Vec<FixtureRadio>,
    active_radio: u32,
    default_radio: Option<u32>,
    rules: Vec<RoutingRule>,
    cases: Vec<FixtureCase>,
}
#[derive(serde::Deserialize)]
struct FixtureRadio {
    id: u32,
    bands: Vec<String>,
    enabled: bool,
}
/// One cell of the "Where two radios share a band" step changed to `radio` (None = "No rule"), and
/// the whole rule list Finish writes for it.
#[derive(serde::Deserialize)]
struct FixtureCase {
    band: String,
    mode: RouteMode,
    radio: Option<u32>,
    rules: Vec<RoutingRule>,
}

/// ⭐ ONE CHANGED CELL MOVES ONLY THAT CELL. For every single change the setup can make to a shared
/// band's cell, on a station with a rule aimed at a radio that is turned off (the engine passes over
/// it to the next rule) and on one with every kind of rule, the rules Finish writes route every
/// OTHER band and mode exactly as the stored rules did, by the engine's own `route_radio`, both as
/// the radios are and with every radio turned back on (no rule behind a turned-off radio was lost);
/// and the changed cell goes to the radio it names when that radio can take it.
#[test]
fn one_changed_cell_moves_only_that_cell() {
    let fixture: Vec<FixtureStation> = serde_json::from_str(include_str!(
        "../../../../ui/src/settings/__fixtures__/radio-setup-single-changes.json"
    ))
    .expect("fixture parses");
    // Parser sanity: a fixture that read as nothing would pass every assertion below.
    assert_eq!(fixture.len(), 2);
    assert!(fixture.iter().all(|st| st.cases.len() > 10));
    let bands = [
        "160m", "80m", "40m", "20m", "10m", "6m", "2m", "70cm", "23cm",
    ];
    let mut checked = 0;
    for st in &fixture {
        let mut s = Engine::new("KD9TAW", "EN52", 0).settings.clone();
        s.ensure_radio_profiles();
        while s.radios.len() < st.radios.len() {
            s.add_radio_profile();
        }
        for r in &st.radios {
            let p = s
                .radios
                .iter_mut()
                .find(|p| p.id == r.id)
                .expect("fixture radio id exists");
            p.bands = r.bands.clone();
            p.enabled = r.enabled;
        }
        s.active_radio = st.active_radio;
        s.default_radio = st.default_radio;
        s.routing_rules = st.rules.clone();
        let usable = |id: u32| st.radios.iter().any(|r| r.id == id && r.enabled);
        // The same station with every radio turned back on: a rule aimed at a radio that is off
        // today must still be there for the day it is plugged back in.
        let mut all_on = s.clone();
        all_on.radios.iter_mut().for_each(|p| p.enabled = true);
        for c in &st.cases {
            for (before, on) in [(&s, false), (&all_on, true)] {
                let mut after = before.clone();
                after.routing_rules = c.rules.clone();
                for band in bands {
                    for mode in MODES {
                        if band.eq_ignore_ascii_case(&c.band) && mode == c.mode {
                            continue;
                        }
                        assert_eq!(
                            after.route_radio(band, mode),
                            before.route_radio(band, mode),
                            "{} (every radio on: {on}): changing {} {:?} to {:?} moved {band} {mode:?}",
                            st.name,
                            c.band,
                            c.mode,
                            c.radio,
                        );
                        checked += 1;
                    }
                }
            }
            if let Some(id) = c.radio.filter(|id| usable(*id)) {
                let want = (id != st.active_radio).then_some(id);
                let mut after = s.clone();
                after.routing_rules = c.rules.clone();
                assert_eq!(
                    after.route_radio(&c.band, c.mode),
                    want,
                    "{}: {} {:?} set to {id}",
                    st.name,
                    c.band,
                    c.mode
                );
            }
        }
    }
    assert!(checked > 2000, "checked {checked}");
}

/// The review's case, by name: the IC-9700 (turned off) has a 2 m Digital rule with a 2 m Digital
/// fallback to the FT-991A behind it. Changing only 2 m FM keeps 2 m FT8 on the FT-991A.
#[test]
fn a_fallback_behind_a_turned_off_radio_survives_a_change_to_another_cell() {
    let fixture: Vec<FixtureStation> = serde_json::from_str(include_str!(
        "../../../../ui/src/settings/__fixtures__/radio-setup-single-changes.json"
    ))
    .expect("fixture parses");
    let st = &fixture[0];
    let c = st
        .cases
        .iter()
        .find(|c| c.band == "2m" && c.mode == RouteMode::Fm && c.radio == Some(0))
        .expect("the 2 m FM → radio 0 case");
    let mut s = Engine::new("KD9TAW", "EN52", 0).settings.clone();
    s.ensure_radio_profiles();
    s.add_radio_profile();
    s.add_radio_profile();
    for r in &st.radios {
        let p = s.radios.iter_mut().find(|p| p.id == r.id).unwrap();
        p.bands = r.bands.clone();
        p.enabled = r.enabled;
    }
    s.active_radio = 0;
    s.routing_rules = c.rules.clone();
    assert!(!s.radios[2].enabled, "the IC-9700 is off");
    assert_eq!(s.route_radio("2m", RouteMode::Digital), Some(1));
}
