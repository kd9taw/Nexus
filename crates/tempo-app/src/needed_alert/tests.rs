use super::*;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Step {
    at_ms: i64,
    #[serde(default)]
    settings: serde_json::Map<String, Value>,
    #[serde(default)]
    qso: QsoContext,
    #[serde(default)]
    watchlist: Vec<WatchEntry>,
    dial_mhz: Option<f64>,
    geo_scope: Option<HashSet<String>>,
    decodes: Vec<serde_json::Map<String, Value>>,
    alerts: Vec<Value>,
    sounds: Vec<u32>,
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Tick {
    refs: Vec<String>,
    sounds: Vec<u32>,
}

#[derive(Deserialize)]
struct Run {
    name: String,
    ticks: Vec<Tick>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    base_decode: serde_json::Map<String, Value>,
    base_settings: serde_json::Map<String, Value>,
    scenarios: Vec<Scenario>,
    pota: Vec<Run>,
}

fn table() -> Table {
    serde_json::from_str(include_str!("../../tests/fixtures/needed-alerts.json")).unwrap()
}

fn laid_over(
    base: &serde_json::Map<String, Value>,
    over: &serde_json::Map<String, Value>,
) -> Value {
    let mut whole = base.clone();
    whole.extend(over.clone());
    Value::Object(whole)
}

/// An alert as the table writes it: its toast's parts, and how it shows.
fn written(alert: &NeededAlert) -> Value {
    let shown = alert.kind.presentation();
    let mut value = serde_json::to_value(alert).unwrap();
    // The button's decode is held to the table on its own (`every_alert_works_the_decode_it_is_about`).
    value.as_object_mut().unwrap().remove("work");
    value["tone"] = shown.toast.into();
    value["ms"] = shown.ms.into();
    value["prominent"] = shown.prominent.into();
    value["action"] = shown.action.into();
    value
}

/// Every tone the alerts of one look sound, in order: each one's first tone as it is raised, then
/// the double beeps' second tones, 130 ms on.
fn sounded(alerts: &[NeededAlert]) -> Vec<u32> {
    let tones: Vec<(u32, bool)> = alerts
        .iter()
        .filter_map(|a| a.kind.presentation().tone)
        .collect();
    let firsts = tones.iter().map(|(hz, _)| *hz);
    let seconds = tones
        .iter()
        .filter(|(_, double)| *double)
        .map(|(hz, _)| hz * 3 / 2);
    firsts.chain(seconds).collect()
}

/// ★ ONE DECISION, TWO LANGUAGES: the station raises, step by step, the very alerts the desktop's
/// screen raises today — the same toasts with the same parts, and the same tones in the same order —
/// on every case of the shared table. The desktop's half is `ui/src/alerts.agreement.test.ts`, over
/// the same file, where the expected values were captured.
#[test]
fn the_station_raises_what_the_desktop_raises_on_every_case_in_the_shared_table() {
    let table = table();
    let mut wrong = Vec::new();
    for scenario in &table.scenarios {
        let mut needed = NeededAlerts::new();
        for (n, step) in scenario.steps.iter().enumerate() {
            let switches: Switches =
                serde_json::from_value(laid_over(&table.base_settings, &step.settings)).unwrap();
            let decodes: Vec<DecodeRow> = step
                .decodes
                .iter()
                .map(|d| serde_json::from_value(laid_over(&table.base_decode, d)).unwrap())
                .collect();
            let scene = Scene {
                switches: &switches,
                qso: &step.qso,
                watch: &step.watchlist,
                dial_mhz: step.dial_mhz,
                geo_scope: step.geo_scope.as_ref(),
                now_ms: step.at_ms,
            };
            let alerts = needed.decide(&decodes, &scene);
            let got: Vec<Value> = alerts.iter().map(written).collect();
            if got != step.alerts || sounded(&alerts) != step.sounds {
                wrong.push(format!(
                    "{}, step {}: {got:?} {:?}",
                    scenario.name,
                    n + 1,
                    sounded(&alerts)
                ));
            }
        }
    }
    for run in &table.pota {
        let mut parks = FreshParks::new();
        for (n, tick) in run.ticks.iter().enumerate() {
            let fresh = parks.tick(tick.refs.iter().map(String::as_str));
            let sounds = if fresh.is_empty() {
                vec![]
            } else {
                vec![NEW_PARK_TONE, NEW_PARK_TONE * 3 / 2]
            };
            if sounds != tick.sounds {
                wrong.push(format!("{}, tick {}: {fresh:?}", run.name, n + 1));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// ★ The table can tell a decider apart: it raises every kind, and holds silent steps too.
/// CONTROL: a decider raising nothing disagrees with it.
#[test]
fn the_shared_table_raises_every_kind_and_some_silence() {
    let table = table();
    let kinds: HashSet<String> = table
        .scenarios
        .iter()
        .flat_map(|s| &s.steps)
        .flat_map(|st| &st.alerts)
        .map(|a| a["kind"].as_str().unwrap().to_string())
        .collect();
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort();
    assert_eq!(
        kinds,
        ["cq", "myCall", "newDxcc", "newGrid", "rareGrid", "watch"]
    );
    let steps = table.scenarios.iter().flat_map(|s| &s.steps);
    assert!(steps.clone().any(|st| st.alerts.is_empty()));
    assert!(steps.filter(|st| !st.alerts.is_empty()).count() >= 20);
}

fn row(from: &str, message: &str) -> DecodeRow {
    serde_json::from_value(json!({
        "from": from, "snr": -10, "dtSec": 0.1, "freqHz": 1500.0, "message": message,
        "isCq": true, "directedToMe": false, "worked": false, "tier": "FT8", "rv": 0,
    }))
    .unwrap()
}

/// ★ A new one is announced once, however busy the band: a run of plain CQs passes the repeating
/// kinds' cap many times over, and the oldest of those go, never the new DXCC's dedup (an ATNO once
/// re-alerted every cycle that way). CONTROL: the CQs keep alerting while they are new.
#[test]
fn a_busy_band_never_brings_a_new_one_back() {
    let switches = Switches {
        alert_new: true,
        alert_cq: true,
        ..Switches::default()
    };
    let qso = QsoContext::default();
    let scene = |now_ms| Scene {
        switches: &switches,
        qso: &qso,
        watch: &[],
        dial_mhz: None,
        geo_scope: None,
        now_ms,
    };
    let mut needed = NeededAlerts::new();
    let mut new_one = row("VP8PJ", "CQ VP8PJ GD18");
    new_one.new_dxcc = true;
    new_one.country = Some("Falkland Islands".into());
    assert_eq!(
        needed.decide(&[new_one.clone()], &scene(0))[0].kind,
        NeededKind::NewDxcc
    );
    for n in 0..3 * REPEAT_CAP {
        let cq = row(&format!("W{n}AA"), &format!("CQ W{n}AA EM12"));
        assert_eq!(needed.decide(&[cq], &scene(n as i64)).len(), 1, "CQ {n}");
    }
    assert!(needed.repeat.len() <= REPEAT_CAP && needed.repeat.len() == needed.repeat_set.len());
    new_one.message = "CQ VP8PJ GD18 again".into();
    assert_eq!(needed.decide(&[new_one], &scene(1)), vec![]);
}

/// ★ The station's settings read as the screen reads them: a new DXCC on every band, both grid
/// kinds VHF and up, by default; and an absent scope falls back to those. CONTROL: `off` silences.
#[test]
fn the_settings_and_their_band_scopes_read_as_the_screen_reads_them() {
    let switches = Switches::of(&Settings::default());
    assert_eq!(switches.alert_dxcc_bands.as_deref(), Some("all"));
    assert_eq!(switches.alert_grid_bands.as_deref(), Some("vhf"));
    assert_eq!(switches.alert_rare_grid_bands.as_deref(), Some("vhf"));
    for (scope, dial, admits) in [
        (Some("off"), None, false),
        (Some("off"), Some(50.0), false),
        (Some("all"), Some(14.074), true),
        (Some("vhf"), Some(14.074), false),
        (Some("vhf"), Some(50.0), true),
        (Some("hf"), Some(50.0), false),
        (Some("hf"), Some(49.99), true),
        (Some("vhf"), None, true),
        (Some("vhf"), Some(0.0), true),
        (None, Some(14.074), false),
        (None, Some(144.174), true),
    ] {
        assert_eq!(
            band_scope_ok(scope, dial, "vhf"),
            admits,
            "{scope:?} {dial:?}"
        );
    }
}

/// ★ An alert's button works the decode it is about, as double-clicking its row does — its
/// message, SNR, offset and mode — and a station not parsed has none. CONTROL: two decodes of one
/// look each carry their own.
#[test]
fn every_alert_works_the_decode_it_is_about() {
    let switches = Switches {
        alert_cq: true,
        ..Switches::default()
    };
    let qso = QsoContext::default();
    let scene = Scene {
        switches: &switches,
        qso: &qso,
        watch: &[],
        dial_mhz: None,
        geo_scope: None,
        now_ms: 0,
    };
    let mut one = row("K1ABC", "CQ K1ABC FN42");
    one.snr = -7;
    one.freq_hz = 1234.0;
    let mut two = row("JA1XYZ", "CQ JA1XYZ PM95");
    two.tier = Tier::Ft4;
    let mut nobody = row("", "CQ <...> FN42");
    nobody.from = None;
    let alerts = NeededAlerts::new().decide(&[one, two, nobody], &scene);
    let works: Vec<_> = alerts.iter().map(|a| a.work.clone()).collect();
    assert_eq!(
        works,
        vec![
            Some(Work {
                message: "CQ K1ABC FN42".into(),
                snr: -7,
                freq_hz: 1234.0,
                tier: Tier::Ft8
            }),
            Some(Work {
                message: "CQ JA1XYZ PM95".into(),
                snr: -10,
                freq_hz: 1500.0,
                tier: Tier::Ft4
            }),
            None,
        ]
    );
    assert_eq!(
        serde_json::to_value(&alerts[0]).unwrap()["work"],
        json!({"message": "CQ K1ABC FN42", "snr": -7, "freqHz": 1234.0, "tier": "FT8"})
    );
}

/// ★ The geographic scope resolves as the desktop's `scopedEntities` did: continents expand through
/// the country file's table and union with the entities picked; an unknown code is dropped; and a
/// scope that admits nothing is no scope at all. CONTROL: an empty scope is `None`, a picked entity
/// alone is itself.
#[test]
fn the_geographic_scope_resolves_to_entity_names_and_never_fails_closed() {
    let table = [
        ("France", "EU"),
        ("Germany", "EU"),
        ("Japan", "AS"),
        ("United States", "NA"),
    ];
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let set = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect::<HashSet<_>>());
    assert_eq!(scoped_entities(&[], &[], table), None);
    assert_eq!(scoped_entities(&[], &names(&[""]), table), None);
    assert_eq!(
        scoped_entities(&[], &names(&["Japan"]), table),
        set(&["Japan"])
    );
    assert_eq!(
        scoped_entities(&names(&["EU"]), &names(&["Japan"]), table),
        set(&["France", "Germany", "Japan"])
    );
    assert_eq!(
        scoped_entities(&names(&["EU", "XX"]), &[], table),
        set(&["France", "Germany"])
    );
    assert_eq!(
        scoped_entities(&names(&["XX"]), &[], table),
        None,
        "an unknown code alone"
    );
    assert_eq!(
        scoped_entities(&names(&["AF"]), &[], table),
        None,
        "a continent nothing is on"
    );
}
