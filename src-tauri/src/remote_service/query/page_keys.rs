//! ★ WHAT THE HOSTED PAGE TAKES, KEY FOR KEY. The page reads each collection below with a parser
//! (`ui/src/remote-web/*.ts`) that refuses an object carrying a key it does not list, and the
//! station fills those objects from types it shares with the desktop. A field the desktop gains
//! therefore reaches every Remote browser unless the station keeps it off, and the browser then
//! refuses the whole read: the park list's `states`, added for the desktop's log form, emptied the
//! Remote log form's park search exactly that way.
//!
//! Each test reads its collection through the station's own [`Publisher`] and holds every object
//! the page checks to the page's list, copied from its parser. The shared values the station sends
//! whole (a hunter-feed spot, a rare-DX alert, an APRS packet, a band-plan channel) are written
//! here field by field, every optional field set: a field added to one of those types does not
//! compile here until it has a value, and then it is sent. A key the page does not list stays at
//! the station. A key the page should show goes into its parser first, as optional, and is sent
//! only once that page is deployed.
use super::{Publisher, Request, Sources};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const ID: &str = "10000000-0000-4000-8000-000000000001";

fn sources() -> Sources {
    Sources {
        needs: Default::default(),
        spots: Default::default(),
        live_paths: Default::default(),
        region_paths: crate::SharedRegionPaths(Default::default()),
        ota: Default::default(),
        health: Default::default(),
        propagation: Default::default(),
        memories: Default::default(),
        navigation: Default::default(),
        sstv: Default::default(),
        parks: Default::default(),
        pounces: Default::default(),
    }
}

fn engine() -> crate::SharedEngine {
    Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
        Default::default(),
    )))
}

/// The page a browser is sent for `collection`, exactly as the station encodes it.
fn page(engine: &crate::SharedEngine, sources: &Sources, collection: &str, search: &str) -> Value {
    let request: Request =
        serde_json::from_value(json!({ "requestId": ID, "collection": collection,
        "cursor": null, "search": search, "unconfirmed": false, "after": null }))
        .unwrap();
    let text = Publisher::default()
        .read(&request, engine, Some(sources), Instant::now())
        .unwrap_or_else(|e| panic!("{collection}: the station refused the read: {e}"));
    serde_json::from_str(&text).unwrap()
}

/// `value` as the page's parser checks it: every key it lists, and no other.
fn takes(what: &str, value: &Value, listed: &[&str]) {
    let Some(object) = value.as_object() else {
        panic!("{what}: the page expects an object, the station sent {value}");
    };
    let unlisted: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|k| !listed.contains(k))
        .collect();
    let missing: Vec<&&str> = listed
        .iter()
        .filter(|k| !object.contains_key(**k))
        .collect();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "{what}: the page refuses this object. Sent and not listed by the page: {unlisted:?}. \
         Listed by the page and not sent: {missing:?}"
    );
}

/// The Remote log form's park search (`parks.ts`): six keys a park, the same six for the exact
/// match. The desktop's own answer also says where the park is (`states`); that stays on the
/// desktop.
#[test]
fn a_park_is_sent_with_only_the_keys_the_page_takes() {
    const PARK: &[&str] = &[
        "reference",
        "name",
        "grid",
        "location",
        "latitude",
        "longitude",
    ];
    let index = tempo_core::pota::ParkIndex::parse_csv(
        "reference,name,grid,location\nUS-0823,Fort Union Trading Post NHS,DN98,\"US-MT,US-ND\"\n",
    );
    let desktop =
        serde_json::to_value(crate::ParkDto::from(index.lookup("US-0823").unwrap())).unwrap();
    assert_eq!(
        desktop["states"],
        json!(["US-MT", "US-ND"]),
        "scene guard: the desktop's park carries a key the page does not list"
    );
    let sources = sources();
    *sources.parks.lock().unwrap() = index;
    let page = page(&engine(), &sources, "parks", "US-0823");
    let source = &page["meta"]["source"];
    takes("parks: the source", source, &["parkCount", "exact"]);
    takes("parks: the exact match", &source["exact"], PARK);
    let rows = page["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "scene guard: the search finds the park");
    takes("parks: a row", &rows[0], PARK);
}

/// The hunter feeds (`ota.ts`): fourteen keys a spot, three each for the activation and the hunt.
/// A spot also knows its park's states, and so do the hunt and the activation (the log form's and
/// the desktop strip's): none of it is sent.
#[test]
fn a_spot_the_activation_and_the_hunt_carry_only_the_keys_the_page_takes() {
    let places = vec!["US-MT".to_string(), "US-ND".to_string()];
    let now = crate::now_unix();
    let spot = propagation::OtaSpot {
        program: "POTA".into(),
        reference: "US-0823".into(),
        name: "Fort Union Trading Post NHS".into(),
        activator: "K2ABC".into(),
        freq_khz: 14_062.0,
        mode: "CW".into(),
        spotter: Some("W1AW".into()),
        comment: Some("QRT at 2200".into()),
        grid: Some("DN98".into()),
        lat: Some(47.999),
        lon: Some(-104.04),
        spot_time_unix: Some(now),
        states: places.clone(),
    };
    let sources = sources();
    sources
        .ota
        .lock()
        .unwrap()
        .insert("POTA".into(), (now, vec![spot]));
    let engine = engine();
    {
        let mut e = engine.lock().unwrap();
        e.set_activation("POTA", "US-0823", places.clone()).unwrap();
        e.set_hunt_target("K2ABC", "POTA", "US-0823", places.clone())
            .unwrap();
        assert_eq!(
            (e.snapshot().hunt.map(|h| h.states), e.activation_places()),
            (Some(places.clone()), places),
            "scene guard: the station's hunt and activation know where the park is"
        );
    }
    let page = page(&engine, &sources, "ota", "");
    let source = &page["meta"]["source"];
    takes(
        "ota: the source",
        source,
        &["feeds", "activation", "hunt", "parkCount", "huntedCount"],
    );
    takes(
        "ota: the activation",
        &source["activation"],
        &["program", "reference", "qsoCount"],
    );
    takes(
        "ota: the hunt",
        &source["hunt"],
        &["program", "reference", "call"],
    );
    let feeds = source["feeds"].as_array().unwrap();
    for feed in feeds {
        takes(
            "ota: a feed",
            feed,
            &["program", "status", "sourceAgeMs", "spots"],
        );
    }
    let spots = feeds[0]["spots"].as_array().unwrap();
    assert_eq!(spots.len(), 1, "scene guard: the POTA feed sends its spot");
    takes(
        "ota: a spot",
        &spots[0],
        &[
            "program",
            "reference",
            "name",
            "activator",
            "freqKhz",
            "mode",
            "spotter",
            "comment",
            "grid",
            "lat",
            "lon",
            "spotTimeUnix",
            "newPark",
            "bandOpen",
        ],
    );
}

/// The rare-DX alerts (`pounce.ts`): seven keys an alert, and the threshold alone beside them.
#[test]
fn a_rare_dx_alert_carries_only_the_keys_the_page_takes() {
    let sources = sources();
    sources
        .pounces
        .lock()
        .unwrap()
        .push_back(propagation::pounce::Pounce {
            call: "3Y0J".into(),
            band: "20m".into(),
            mode: "CW".into(),
            freq_mhz: Some(14.025),
            tags: vec![propagation::NeedTag::NewEntity],
            entity: "Bouvet Island".into(),
            at_unix: 1_700_000_000,
        });
    let page = page(&engine(), &sources, "pounce", "");
    takes(
        "pounce: the source",
        &page["meta"]["source"],
        &["threshold"],
    );
    let rows = page["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "scene guard: the alert is sent");
    takes(
        "pounce: an alert",
        &rows[0],
        &[
            "call", "band", "mode", "freqMhz", "tags", "entity", "atUnix",
        ],
    );
}

/// The JS8 panel's context (`js8.ts`): the band plan's channels, seven keys each, as the desktop's
/// own band plan writes them.
#[test]
fn a_js8_channel_carries_only_the_keys_the_page_takes() {
    const CHANNEL: &[&str] = &["band", "group", "dialMhz", "mode", "label", "note", "tx"];
    let channel = tempo_app::bandplan::BandChannel {
        band: "20m".into(),
        group: "HF".into(),
        dial_mhz: 14.078,
        mode: "USB".into(),
        label: "20 m".into(),
        note: "JS8 calling".into(),
        tx: true,
    };
    takes(
        "js8Context: a band plan channel",
        &serde_json::to_value(channel).unwrap(),
        CHANNEL,
    );
    let page = page(&engine(), &sources(), "js8Context", "");
    takes(
        "js8Context: the meta",
        &page["meta"],
        &["capturedAgeMs", "source"],
    );
    takes(
        "js8Context: the source",
        &page["meta"]["source"],
        &["plan", "history"],
    );
    let plan = page["meta"]["source"]["plan"].as_array().unwrap();
    assert!(!plan.is_empty(), "scene guard: the band plan is sent");
    for channel in plan {
        takes("js8Context: a channel", channel, CHANNEL);
    }
}

/// The APRS roster (`aprs.ts`): a packet and a station share twelve keys and add five and six of
/// their own, and a weather report has nine.
#[test]
fn an_aprs_packet_a_station_and_a_weather_report_carry_only_the_keys_the_page_takes() {
    use tempo_app::engine::{AprsHeard, AprsSource, AprsWxDto};
    const COMMON: &[&str] = &[
        "lat",
        "lon",
        "symbolTable",
        "symbolCode",
        "kind",
        "text",
        "speedKnots",
        "courseDeg",
        "path",
        "raw",
        "sourceKind",
        "wx",
    ];
    let engine = engine();
    engine.lock().unwrap().push_aprs_heard(AprsHeard {
        source: "W1AW-13".into(),
        dest: "APNEXU".into(),
        path: vec!["WIDE1-1".into()],
        lat: Some(41.714),
        lon: Some(-72.728),
        symbol_table: '/',
        symbol_code: '_',
        kind: "weather",
        text: "Station weather".into(),
        speed_knots: Some(3),
        course_deg: Some(270),
        addressee: Some("K2ABC".into()),
        msg_id: Some("7".into()),
        at_unix: crate::now_unix(),
        source_kind: AprsSource::Rf,
        wx: Some(AprsWxDto {
            wind_dir_deg: Some(270),
            wind_mph: Some(5),
            gust_mph: Some(9),
            temp_f: Some(61),
            rain_1h_in100: Some(1),
            rain_24h_in100: Some(12),
            rain_midnight_in100: Some(8),
            humidity_pct: Some(77),
            pressure_tenth_hpa: Some(10_132),
        }),
        raw: "W1AW-13>APNEXU,WIDE1-1:_10090556c270s005g009t061r001p012P008h77b10132".into(),
    });
    let page = page(&engine, &sources(), "aprs", "");
    takes(
        "aprs: the meta",
        &page["meta"],
        &["source", "capturedAgeMs"],
    );
    takes(
        "aprs: the source",
        &page["meta"]["source"],
        &[
            "capturedAtMs",
            "ttlMin",
            "fadeAfterMin",
            "packets",
            "stations",
        ],
    );
    let rows = page["rows"].as_array().unwrap();
    let kinds: Vec<&str> = rows.iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        ["packet", "station"],
        "scene guard: the packet and the station it made are sent"
    );
    let own = |extra: &[&'static str]| -> Vec<&'static str> {
        COMMON.iter().chain(extra).copied().collect()
    };
    for (row, keys) in rows.iter().zip([
        own(&["source", "dest", "addressee", "msgId", "atUnix"]),
        own(&[
            "call",
            "lastHeardUnix",
            "lastRfUnix",
            "lastInetUnix",
            "packets",
            "firstHeardUnix",
        ]),
    ]) {
        let what = format!("aprs: a {}", row["kind"].as_str().unwrap());
        takes(&what, row, &["kind", "value"]);
        takes(&what, &row["value"], &keys);
        takes(
            &format!("{what}'s weather"),
            &row["value"]["wx"],
            &[
                "windDirDeg",
                "windMph",
                "gustMph",
                "tempF",
                "rain1hIn100",
                "rain24hIn100",
                "rainMidnightIn100",
                "humidityPct",
                "pressureTenthHpa",
            ],
        );
    }
}
