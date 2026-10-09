//! ★ WHAT THE HOSTED PAGE TAKES, KEY FOR KEY. The page reads each collection below with a parser
//! (`ui/src/remote-web/*.ts`) that refuses an object carrying a key it does not list, and the
//! station fills those objects from types it shares with the desktop. A field the desktop gains
//! therefore reaches every Remote browser unless the station keeps it off, and the browser then
//! refuses the whole read: the park list's `states`, added for the desktop's log form, emptied the
//! Remote log form's park search exactly that way.
//!
//! Each test reads what a browser reads, as the station sends it: a collection through the
//! station's own [`Publisher`] (a planning document page by page, its chunks joined), and an
//! application-lane sample through that lane's publisher, as the stream carries it. It holds every
//! object the page checks to the page's list, copied from its parser. The shared values the
//! station sends whole (a hunter-feed spot, a rare-DX alert, an APRS packet, a band-plan channel, a
//! programming project) are written here field by field, every optional field set: a field added to
//! one of those types does not compile here until it has a value, and then it is sent. A value the
//! engine computes (a confirmation report, a Field Day status, a JS8 row) is also written field by
//! field, because no test station fills every field. A key the page does not list stays at the
//! station. A key the page should show goes into its parser first, as optional, and is sent only
//! once that page is deployed.
use super::super::application::{self, Command};
use super::{Publisher, Request, Sources};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    read(
        &mut Publisher::default(),
        engine,
        sources,
        collection,
        search,
        None,
    )
    .unwrap_or_else(|e| panic!("{collection}: the station refused the read: {e}"))
}

/// One page of `collection` from `publisher`, at `cursor`, its envelope held to the keys
/// `application-query-protocol.ts` takes.
fn read(
    publisher: &mut Publisher,
    engine: &crate::SharedEngine,
    sources: &Sources,
    collection: &str,
    search: &str,
    cursor: Option<&str>,
) -> Result<Value, &'static str> {
    let request: Request =
        serde_json::from_value(json!({ "requestId": ID, "collection": collection,
        "cursor": cursor, "search": search, "unconfirmed": false, "after": null }))
        .unwrap();
    let page: Value =
        serde_json::from_str(&publisher.read(&request, engine, Some(sources), Instant::now())?)
            .unwrap();
    takes(
        &format!("{collection}: the page"),
        &page,
        &[
            "type",
            "requestId",
            "collection",
            "snapshotId",
            "offset",
            "total",
            "retained",
            "nextCursor",
            "ageMs",
            "rows",
            "meta",
        ],
    );
    Ok(page)
}

/// A planning document (`navigation.ts`, `configuration.ts`) as a browser reads it: every page of
/// one sealed capture, each page's meta held to the page's keys, the chunks joined. The station
/// builds the capture off the reading thread and refuses the first reads as busy until it is done.
fn document(
    engine: &crate::SharedEngine,
    sources: &Sources,
    collection: &str,
    search: &str,
) -> Value {
    let mut publisher = Publisher::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    let (mut text, mut cursor) = (String::new(), None::<String>);
    loop {
        let page = match read(
            &mut publisher,
            engine,
            sources,
            collection,
            search,
            cursor.as_deref(),
        ) {
            Err("applicationBusy") if cursor.is_none() && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            other => {
                other.unwrap_or_else(|e| panic!("{collection}: the station refused the read: {e}"))
            }
        };
        takes(
            &format!("{collection}: the meta"),
            &page["meta"],
            &["source", "capturedAgeMs"],
        );
        takes(
            &format!("{collection}: the source"),
            &page["meta"]["source"],
            &[
                "encoding",
                "bytes",
                "chunks",
                "kind",
                "search",
                "contextId",
                "stationContextId",
                "capturedAtMs",
                "validForMs",
                "documentAgeMs",
            ],
        );
        for chunk in page["rows"].as_array().unwrap() {
            text.push_str(chunk.as_str().unwrap());
        }
        match page["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    serde_json::from_str(&text).unwrap()
}

/// The application lane's `command` sample as a browser's stream carries it, the reply held to the
/// keys `application-protocol.ts` takes: its `data`.
fn sample(
    engine: &crate::SharedEngine,
    samples: &mut application::Publisher,
    command: Command,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    let text = loop {
        match samples.read(engine, command, ID, None, Instant::now()) {
            Err("applicationBusy") if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            other => {
                break other
                    .unwrap_or_else(|e| panic!("{command:?}: the station refused the read: {e}"))
            }
        }
    };
    let reply: Value = serde_json::from_str(&text).unwrap();
    takes(
        &format!("{command:?}: the reply"),
        &reply,
        &[
            "type",
            "requestId",
            "command",
            "revision",
            "baseRevision",
            "ageMs",
            "data",
            "removed",
        ],
    );
    reply["data"].clone()
}

/// `value` as the page's parser checks it: every key it lists, and no other.
fn takes(what: &str, value: &Value, listed: &[&str]) {
    takes_optional(what, value, listed, &[]);
}

/// `value` as a parser that also admits `optional` keys checks it: every key in `listed`, and none
/// outside `listed` and `optional`.
fn takes_optional(what: &str, value: &Value, listed: &[&str], optional: &[&str]) {
    let Some(object) = value.as_object() else {
        panic!("{what}: the page expects an object, the station sent {value}");
    };
    let unlisted: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|k| !listed.contains(k) && !optional.contains(k))
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

/// The Awards view's confirmation report (`confirmations.ts`): six keys at its root, four a
/// diagnosis and a reason, an action's kind with the ten keys only some kinds fill, and three a
/// bucket and a one-away entity. The desktop's own report also names each contact (`id`, `call`,
/// `band`, `mode`, `whenUnix`, an action's `otherId`, a bucket's `qsoIds`); the station's report
/// is never named, and the page lists none of them.
#[test]
fn a_confirmation_report_carries_only_the_keys_the_page_takes() {
    use tempo_app::dto::{
        ActionBucketDto, ActionDto, DiagnosticsReportDto, OneAwayDto, QsoDiagnosisDto, ReasonDto,
    };
    let report = |what: &str, report: &Value| {
        for diagnosis in report["diagnoses"].as_array().unwrap() {
            takes(
                &format!("{what}: a diagnosis"),
                diagnosis,
                &["index", "award", "status", "reasons"],
            );
            for reason in diagnosis["reasons"].as_array().unwrap() {
                takes(
                    &format!("{what}: a reason"),
                    reason,
                    &["code", "confidence", "explanation", "action"],
                );
                takes_optional(
                    &format!("{what}: an action"),
                    &reason["action"],
                    &["kind"],
                    &[
                        "source",
                        "detail",
                        "field",
                        "found",
                        "expected",
                        "logged",
                        "suggested",
                        "call",
                        "otherIndex",
                        "untilUnix",
                    ],
                );
            }
        }
        for bucket in report["buckets"].as_array().unwrap() {
            takes(
                &format!("{what}: a bucket"),
                bucket,
                &["kind", "count", "qsoIndices"],
            );
        }
        for entity in report["oneAway"].as_array().unwrap() {
            takes(
                &format!("{what}: one away"),
                entity,
                &["entity", "bands", "newEntity"],
            );
        }
    };
    let engine = engine();
    engine.lock().unwrap().import_adif(
        "<CALL:5>K1ABC<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20250101<TIME_ON:6>010000<EQSL_QSL_RCVD:1>Y<EOR>\n\
         <CALL:6>JA1ABC<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20250102<TIME_ON:6>010000<EQSL_QSL_RCVD:1>Y<EOR>\n",
    );
    let page = page(&engine, &sources(), "confirmations", "");
    let source = &page["meta"]["source"];
    takes(
        "confirmations: the report",
        source,
        &[
            "diagnoses",
            "buckets",
            "oneAway",
            "waitingOnPartner",
            "pendingLag",
            "logCount",
        ],
    );
    assert!(
        source["diagnoses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| !d["reasons"].as_array().unwrap().is_empty())
            && !source["buckets"].as_array().unwrap().is_empty(),
        "scene guard: a contact is diagnosed with a reason, and bucketed: {source}"
    );
    report("confirmations", source);
    // Every key the station's report can carry, written out: no one kind of action fills all ten.
    let written = DiagnosticsReportDto {
        diagnoses: vec![QsoDiagnosisDto {
            index: 0,
            award: "DXCC".into(),
            status: "needsAction".into(),
            reasons: vec![ReasonDto {
                code: "r4a".into(),
                confidence: "confident".into(),
                explanation: "The band differs from the one the partner uploaded.".into(),
                action: ActionDto {
                    kind: "fixField".into(),
                    source: Some("lotw".into()),
                    detail: Some("rejected".into()),
                    field: Some("band".into()),
                    found: Some("20m".into()),
                    expected: Some("17m".into()),
                    logged: Some("K1ABC".into()),
                    suggested: Some("K1ABD".into()),
                    call: Some("K1ABC".into()),
                    other_index: Some(1),
                    until_unix: Some(1_760_000_000),
                    // Named on the desktop's report only.
                    other_id: None,
                },
            }],
            // Named on the desktop's report only.
            id: None,
            call: None,
            band: None,
            mode: None,
            when_unix: None,
        }],
        buckets: vec![ActionBucketDto {
            kind: "fixField".into(),
            count: 1,
            qso_indices: Vec::new(),
            // Named on the desktop's report only.
            qso_ids: None,
        }],
        one_away: vec![OneAwayDto {
            entity: "Japan".into(),
            bands: vec!["20m".into()],
            new_entity: true,
        }],
        waiting_on_partner: 1,
        pending_lag: 1,
    };
    report(
        "confirmations, every field",
        &serde_json::to_value(written).unwrap(),
    );
}

/// The Field Day view (`field-day.ts`): the capture's four keys, the operator settings' four, the
/// ruleset's six and three it may carry, and the running status: seventeen keys and eighteen it
/// may carry, with its contacts, club board (each row's measured clock one it may carry), dupe
/// rule and location warning. The ruleset the
/// desktop's Settings previews also carries a location warning; the station's never does.
#[test]
fn a_field_day_capture_carries_only_the_keys_the_page_takes() {
    use tempo_app::dto::{
        DupeRuleDto, FdBoardFullDto, FdClubBoardRow, FdClubDto, FieldDayQso, FieldDayStatus,
        LocationWarningDto,
    };
    let status = |what: &str, status: &Value| {
        takes_optional(
            what,
            status,
            &[
                "running",
                "state",
                "dxcall",
                "qsoCount",
                "sections",
                "workedSections",
                "points",
                "event",
                "poweredPoints",
                "bonusPoints",
                "totalScore",
                "eventStartUnix",
                "eventEndUnix",
                "rulesYear",
                "rulesGenerated",
                "assistanceOn",
                "log",
            ],
            &[
                "club",
                "multCount",
                "scoreNoteKey",
                "upload",
                "receives",
                "composing",
                "role",
                "boards",
                "myClass",
                "mySection",
                "sentExchange",
                "composingText",
                "bands",
                "locationWarning",
                "dupeModeGroups",
                "dupeRule",
                "objectiveMultiplier",
                "satelliteCredit",
            ],
        );
        for contact in status["log"].as_array().unwrap() {
            takes_optional(
                &format!("{what}: a contact"),
                contact,
                &[
                    "call", "class", "section", "band", "mode", "submode", "whenUnix",
                ],
                &["mex", "rcvd", "dkey", "dupe", "sat"],
            );
        }
        if let Some(rule) = status.get("dupeRule") {
            takes(
                &format!("{what}: the dupe rule"),
                rule,
                &[
                    "byCall",
                    "byBand",
                    "byModeClass",
                    "byFields",
                    "bySentFields",
                    "modeClassGroups",
                    "logDupes",
                ],
            );
        }
        if let Some(warning) = status.get("locationWarning") {
            takes(
                &format!("{what}: the location warning"),
                warning,
                &["typed", "hints"],
            );
        }
        if let Some(club) = status.get("club").filter(|club| !club.is_null()) {
            takes_optional(
                &format!("{what}: the club"),
                club,
                &[
                    "syncState",
                    "queued",
                    "offlineSinceUnix",
                    "hosting",
                    "event",
                    "hostCall",
                    "score",
                    "qsos",
                    "sections",
                    "skewSecs",
                    "dupes",
                    "board",
                ],
                &["lastError", "dkeys", "boardFull"],
            );
            if let Some(full) = club.get("boardFull") {
                takes(
                    &format!("{what}: the club's full board"),
                    full,
                    &["positions", "shown"],
                );
            }
            for row in club["board"].as_array().unwrap() {
                takes_optional(
                    &format!("{what}: a club board row"),
                    row,
                    &[
                        "posid",
                        "posName",
                        "band",
                        "mode",
                        "operator",
                        "qsos",
                        "rate",
                        "lastSeenSecs",
                    ],
                    &["clockMs"],
                );
            }
        }
    };
    let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
        tempo_app::settings::Settings {
            fd_active: true,
            fd_event: "arrlfd".into(),
            fd_class: "1D".into(),
            fd_section: "EMA".into(),
            fd_operator: "W1AW".into(),
            fd_power_mult: 2,
            fd_bonuses: vec!["emergency-power".into()],
            fd_bonuses_planned: vec!["natural-power".into()],
            ..Default::default()
        },
    )));
    {
        let mut e = engine.lock().unwrap();
        e.restore_field_day_if_enabled();
        assert!(
            e.fd_log_manual("K1ABC", "2A", "WI", "CW").unwrap(),
            "scene guard: the contact is logged"
        );
    }
    let page = page(&engine, &sources(), "fieldDay", "");
    let source = &page["meta"]["source"];
    takes(
        "fieldDay: the capture",
        source,
        &["active", "fieldDay", "settings", "ruleset"],
    );
    takes(
        "fieldDay: the settings",
        &source["settings"],
        &["fdOperator", "fdPowerMult", "fdBonuses", "fdBonusesPlanned"],
    );
    // Winter Field Day's capture adds the two objective lists, which the page takes as optional.
    let wfd = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
        tempo_app::settings::Settings {
            fd_active: true,
            fd_event: "wfd".into(),
            fd_class: "1O".into(),
            fd_section: "EMA".into(),
            fd_objectives: vec!["wfd-qrp".into()],
            ..Default::default()
        },
    )));
    wfd.lock().unwrap().restore_field_day_if_enabled();
    // `self::` — the capture above is bound to a local named `page`.
    let wfd_page = self::page(&wfd, &sources(), "fieldDay", "");
    let wfd_settings = &wfd_page["meta"]["source"]["settings"];
    assert_eq!(
        wfd_settings["fdObjectives"],
        serde_json::json!(["wfd-qrp"]),
        "scene guard: Winter Field Day's capture carries its objectives"
    );
    takes_optional(
        "fieldDay: Winter Field Day's settings",
        wfd_settings,
        &["fdOperator", "fdPowerMult", "fdBonuses", "fdBonusesPlanned"],
        &["fdObjectives", "fdObjectivesPlanned"],
    );
    // …and its ruleset says it allows spotting only over amateur RF, a key the page takes too.
    assert_eq!(
        wfd_page["meta"]["source"]["ruleset"]["spotsRfOnly"],
        serde_json::json!(true),
        "scene guard: Winter Field Day's ruleset carries its spotting rule"
    );
    takes_optional(
        "fieldDay: Winter Field Day's ruleset",
        &wfd_page["meta"]["source"]["ruleset"],
        &[
            "event",
            "rulesYear",
            "bannedModes",
            "spottingAllowed",
            "clusterAllowed",
            "enforcement",
        ],
        &["exchange", "problem", "role", "spotsRfOnly"],
    );
    takes_optional(
        "fieldDay: the ruleset",
        &source["ruleset"],
        &[
            "event",
            "rulesYear",
            "bannedModes",
            "spottingAllowed",
            "clusterAllowed",
            "enforcement",
        ],
        &["exchange", "problem", "role", "spotsRfOnly"],
    );
    assert_eq!(
        source["fieldDay"]["log"].as_array().map(Vec::len),
        Some(1),
        "scene guard: the running status is sent with its contact"
    );
    status("fieldDay: the status", &source["fieldDay"]);
    // Every key a status can carry, written out: a contest's multiplier count, bands and mode
    // groups, a club's sync and board, a location warning, a contact's received exchange and dupe
    // mark.
    let written = FieldDayStatus {
        running: true,
        state: "idle".into(),
        dxcall: Some("K1ABC".into()),
        qso_count: 1,
        sections: 1,
        worked_sections: vec!["WI".into()],
        points: 2,
        event: "arrlfd".into(),
        powered_points: 4,
        bonus_points: 100,
        total_score: 104,
        objective_multiplier: Some(7),
        satellite_credit: Some(false),
        event_start_unix: 1_782_500_400,
        event_end_unix: 1_782_597_600,
        mult_count: Some(1),
        score_note_key: "contest.scoreNote.bonuses".into(),
        rules_year: 2026,
        rules_generated: "2026-01-01".into(),
        assistance_on: vec!["Cluster".into()],
        log: vec![FieldDayQso {
            call: "K1ABC".into(),
            class: "2A".into(),
            section: "WI".into(),
            band: "20m".into(),
            mode: "CW".into(),
            submode: String::new(),
            when_unix: 1_782_510_000,
            sat: "AO-91".into(),
            mex: "1D EMA".into(),
            rcvd: vec!["2A".into(), "WI".into()],
            dkey: vec!["K1ABC".into(), "20m".into(), "CW".into()],
            dupe: true,
        }],
        club: Some(FdClubDto {
            sync_state: "synced".into(),
            queued: 1,
            offline_since_unix: 1_782_505_000,
            hosting: true,
            event: "arrlfd".into(),
            host_call: "W1AW".into(),
            score: 104,
            qsos: 1,
            sections: 1,
            skew_secs: -2,
            last_error: Some("The host is not answering.".into()),
            dupes: vec![("K1ABC".into(), "20m".into(), "CW".into())],
            dkeys: vec![vec!["K1ABC".into(), "20m".into(), "CW".into()]],
            board: vec![FdClubBoardRow {
                posid: "cw-1".into(),
                pos_name: "CW tent".into(),
                band: "20m".into(),
                mode: "CW".into(),
                operator: "W1AW".into(),
                qsos: 1,
                rate: 30,
                last_seen_secs: 5,
                clock_ms: Some(-3_000),
            }],
            board_full: Some(FdBoardFullDto {
                positions: 70,
                shown: 59,
            }),
        }),
        upload: Default::default(),
        receives: Vec::new(),
        composing: Vec::new(),
        sent_exchange: "1D EMA".into(),
        composing_text: "1D EMA".into(),
        bands: vec!["20m".into()],
        dupe_mode_groups: vec![vec!["CW".into(), "DIG".into()]],
        dupe_rule: DupeRuleDto {
            by_call: true,
            by_band: true,
            by_mode_class: true,
            by_fields: vec!["SECTION".into()],
            by_sent_fields: Vec::new(),
            mode_class_groups: vec![vec!["CW".into(), "DIG".into()]],
            log_dupes: true,
        },
        location_warning: Some(LocationWarningDto {
            typed: "XX".into(),
            hints: vec!["EMA".into()],
        }),
        role: String::new(),
        boards: Vec::new(),
    };
    status(
        "fieldDay: every field",
        &serde_json::to_value(written).unwrap(),
    );
}

/// The radio-programming document (`configuration.ts`): four keys at its root, seven a project and
/// five its origin. The station reads its own `radioprog.json`; this reads one written here
/// through the same reader, never saved and then saved.
#[test]
fn a_programming_project_carries_only_the_keys_the_page_takes() {
    const ROOT: &[&str] = &["mygrid", "projects", "revision", "saved"];
    let dir =
        std::env::temp_dir().join(format!("nexus-page-keys-{}", super::snapshot_id().unwrap()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("radioprog.json");
    let never_saved = super::configuration::programming_at(&path, "FN31");
    let file = crate::RadioProgFile {
        version: 1,
        projects: vec![crate::RadioProgProject {
            id: "working".into(),
            name: "My channels".into(),
            created_utc: 1_760_000_000,
            updated_utc: 1_760_000_600,
            origin: crate::ProgOrigin {
                kind: "grid".into(),
                grid: "FN31".into(),
                label: "Newington".into(),
                lat: 41.7,
                lon: -72.7,
            },
            radius_km: 50.0,
            channels: Vec::new(),
        }],
    };
    std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
    let saved = super::configuration::programming_at(&path, "FN31");
    std::fs::remove_dir_all(&dir).unwrap();
    takes("programming: never saved", &never_saved.unwrap(), ROOT);
    let saved = saved.unwrap();
    takes("programming: the document", &saved, ROOT);
    let projects = saved["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1, "scene guard: the project is sent");
    takes(
        "programming: a project",
        &projects[0],
        &[
            "id",
            "name",
            "createdUtc",
            "updatedUtc",
            "origin",
            "radiusKm",
            "channels",
        ],
    );
    takes(
        "programming: its origin",
        &projects[0]["origin"],
        &["kind", "grid", "label", "lat", "lon"],
    );
}

/// The settings document (`configuration.ts`): four keys at its root, and the station's list of
/// withheld per-radio keys beside them. The settings themselves are read open.
#[test]
fn the_settings_document_carries_only_the_keys_the_page_takes() {
    takes_optional(
        "settings: the document",
        &document(&engine(), &sources(), "settings", ""),
        &["settings", "withheld", "revision", "platform"],
        &["radioWithheld"],
    );
}

/// A station with a call, a grid and a fresh propagation snapshot, and every feed the Connect board
/// can carry fresh, so that each one is sent.
fn planning() -> (crate::SharedEngine, Sources) {
    let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
        tempo_app::settings::Settings {
            mycall: "W1AW".into(),
            mygrid: "FN31RX09".into(),
            prop_engine: "heuristic".into(),
            ..Default::default()
        },
    )));
    let log = engine.lock().unwrap().log_revision();
    let mut prop = propagation::offline(crate::now_unix() - 1, "W1AW", "FN31RX09");
    prop.source = "live".into();
    let context = crate::PropContext {
        call: "W1AW".into(),
        grid: "FN31RX09".into(),
        log,
    };
    let sources = Sources {
        propagation: Arc::new(Mutex::new(Some((Instant::now(), prop, context)))),
        navigation: super::navigation::Source::new(
            Arc::new(Mutex::new(Some((Instant::now(), Vec::new())))),
            Arc::new(Mutex::new(Some((Instant::now(), Vec::new())))),
            Arc::new(Mutex::new(Some((
                Instant::now(),
                propagation::live::protons::ProtonFlux {
                    j1: 1.0,
                    j5: 0.5,
                    j10: 0.2,
                },
            )))),
            Arc::new(Mutex::new(Some((Instant::now(), Default::default())))),
        ),
        ..sources()
    };
    (engine, sources)
}

/// The Connect board (`navigation.ts`): fourteen keys at its root, four the band outlook, three the
/// coverage and the getting-out reports, and three each feed it carries. The X-ray reading is not
/// sent here: the station keeps it process-wide, and setting it would reach every other test.
#[test]
fn the_connect_board_carries_only_the_keys_the_page_takes() {
    let (engine, sources) = planning();
    let board = document(&engine, &sources, "connect", "");
    takes(
        "connect: the board",
        &board,
        &[
            "mycall",
            "mygrid",
            "prop",
            "sourceAgeMs",
            "sourceValidForMs",
            "bandOutlook",
            "gettingOut",
            "scales",
            "muf",
            "aurora",
            "pca",
            "declination",
            "coverage",
            "xray",
        ],
    );
    takes(
        "connect: the band outlook",
        &board["bandOutlook"],
        &["engine", "bands", "mufNow", "mufHourly"],
    );
    takes(
        "connect: the coverage",
        &board["coverage"],
        &["grids", "zones", "logCount"],
    );
    takes(
        "connect: getting out",
        &board["gettingOut"],
        &["count", "maxKm", "reports"],
    );
    for feed in ["scales", "muf", "aurora", "pca"] {
        takes(
            &format!("connect: the {feed} feed"),
            &board[feed],
            &["value", "ageMs", "validForMs"],
        );
    }
}

/// A path prediction (`navigation.ts`): four keys at its root and four the prediction.
#[test]
fn a_path_prediction_carries_only_the_keys_the_page_takes() {
    let (engine, sources) = planning();
    let path = document(&engine, &sources, "path", "PM95");
    takes(
        "path: the document",
        &path,
        &["mygrid", "grid", "prediction", "sourceAgeMs"],
    );
    takes(
        "path: the prediction",
        &path["prediction"],
        &["engine", "bands", "mufNow", "mufHourly"],
    );
}

/// The satellites board's view (`navigation.ts`): nine keys, as the station's own view builder
/// writes them. The roots of the satellites and satellite documents are not read here: the station
/// builds them from a process-wide element set, or from the operator's own files when that is
/// empty.
#[test]
fn a_satellite_view_carries_only_the_keys_the_page_takes() {
    let iss = super::navigation::test_fresh_catalog()
        .elements
        .into_iter()
        .find(|t| t.name == "ISS (ZARYA)")
        .expect("scene guard: the bundled elements carry the ISS");
    let catalog = std::collections::HashMap::from([(
        25544,
        propagation::live::tle::SatCatalogEntry {
            norad: 25544,
            name: iss.name.clone(),
            status: "alive".into(),
            amateur: true,
            decayed: false,
            src: None,
            classes: Some(vec!["fm".into()]),
        },
    )]);
    // A grid no other test asks for: the station keeps one pass list per grid.
    let view = tauri::async_runtime::block_on(crate::satellite_view_from_inputs(
        "DM79".into(),
        vec![iss],
        crate::now_unix(),
        "mirror".into(),
        catalog,
    ))
    .unwrap();
    takes(
        "satellites: the view",
        &super::navigation::value(&view).unwrap(),
        &[
            "tleAgeDays",
            "usableCount",
            "agingCount",
            "heldBackCount",
            "tleFetchedAt",
            "tleSource",
            "birds",
            "passes",
            "excluded",
        ],
    );
}

/// The station's channel memories (`memories.ts`, `memoryBank.ts`): the read's two keys, the bank's
/// three, a group's three, a memory's eight and the seventeen it may carry, and a net schedule's
/// four and three. The bank goes in through the station's own reader, every key set.
#[test]
fn a_memory_bank_carries_only_the_keys_the_page_takes() {
    const MEMORY: &[&str] = &[
        "offsetDir",
        "offsetMhz",
        "txMhz",
        "toneMode",
        "ctcssEncHz",
        "ctcssDecHz",
        "dtcsCode",
        "dtcsRxCode",
        "dtcsPol",
        "notes",
        "callsign",
        "grid",
        "lat",
        "lon",
        "skip",
        "lastUsedUtc",
        "net",
    ];
    const NET: &[&str] = &["netControl", "description", "netloggerName"];
    let bank = json!({"version": 2, "groups": [{"id": "local", "name": "Local", "order": 0}],
        "memories": [{"id": "w1aw-rptr", "name": "W1AW repeater", "kind": "repeater", "rxMhz": 146.94,
        "mode": "FM", "groups": ["local"], "favorite": true, "source": "manual",
        "offsetDir": "minus", "offsetMhz": 0.6, "txMhz": 146.34, "toneMode": "tone",
        "ctcssEncHz": 100.0, "ctcssDecHz": 100.0, "dtcsCode": 23, "dtcsRxCode": 23, "dtcsPol": "NN",
        "notes": "Club net", "callsign": "W1AW", "grid": "FN31", "lat": 41.7, "lon": -72.7,
        "skip": false, "lastUsedUtc": 1_760_000_000,
        "net": {"days": [1, 3], "utcTime": "1:00", "alertEnabled": true, "alertLeadMin": 10,
            "netControl": "W1AW", "description": "Weekly net", "netloggerName": "W1AW Net"}}]});
    let sources = sources();
    *sources.memories.lock().unwrap() = Some((
        Instant::now(),
        Arc::new(
            super::memories::parse(&bank.to_string())
                .expect("scene guard: the station takes the bank in"),
        ),
    ));
    let page = page(&engine(), &sources, "memories", "");
    let source = &page["meta"]["source"];
    takes("memories: the read", source, &["bank", "sourceAgeMs"]);
    takes(
        "memories: the bank",
        &source["bank"],
        &["version", "memories", "groups"],
    );
    takes(
        "memories: a group",
        &source["bank"]["groups"][0],
        &["id", "name", "order"],
    );
    let memory = &source["bank"]["memories"][0];
    assert!(
        MEMORY.iter().all(|k| memory.get(k).is_some())
            && NET.iter().all(|k| memory["net"].get(k).is_some()),
        "scene guard: every key a memory may carry is sent: {memory}"
    );
    takes_optional(
        "memories: a memory",
        memory,
        &[
            "id", "name", "kind", "rxMhz", "mode", "groups", "favorite", "source",
        ],
        MEMORY,
    );
    takes_optional(
        "memories: its net",
        &memory["net"],
        &["days", "utcTime", "alertEnabled", "alertLeadMin"],
        NET,
    );
}

/// Where the rotator points (`rotator.ts`): two keys, with no rotator configured and with one that
/// answers.
#[test]
fn a_rotator_reading_carries_only_the_keys_the_page_takes() {
    const READING: &[&str] = &["configured", "azimuthDeg"];
    let none = page(&engine(), &sources(), "rotator", "");
    takes("rotator: none configured", &none["meta"]["source"], READING);
    // An address of this test's own: the stand-in rotators' answers are process-wide.
    let addr = "127.0.0.1:14571";
    super::rotator::test_rotor::install(addr, Some(212.5));
    let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
        tempo_app::settings::Settings {
            rotator_host: addr.into(),
            ..Default::default()
        },
    )));
    let page = page(&engine, &sources(), "rotator", "");
    let source = &page["meta"]["source"];
    assert_eq!(
        source["azimuthDeg"],
        json!(212.5),
        "scene guard: the rotator answers"
    );
    takes("rotator: one that answers", source, READING);
}

/// The JS8 roster's history (`js8.ts`): five keys each heard call, worked or never worked.
#[test]
fn a_js8_history_carries_only_the_keys_the_page_takes() {
    let engine = engine();
    {
        let mut e = engine.lock().unwrap();
        let heard = |call: &str| {
            json!({"call": call, "grid": null, "snrDb": -8, "freqHz": 1500.0, "speed": "normal",
                "lastMs": 1, "lastHb": true, "lastCq": false, "storedMsgs": 0})
        };
        e.js8_load_journal(
            &json!({"inbox": [], "heard": [heard("W1AW"), heard("K2ABC")], "allcallReplied": [],
                "nextInboxId": 1})
            .to_string(),
        );
        e.import_adif("<CALL:4>W1AW<BAND:3>40m<MODE:3>SSB<QSO_DATE:8>20260908<TIME_ON:6>010000<GRIDSQUARE:4>FN31<NAME:3>HAM<COMMENT:4>NICE<EOR>");
    }
    let page = page(&engine, &sources(), "js8Context", "");
    let history = page["meta"]["source"]["history"].as_object().unwrap();
    assert_eq!(
        history.len(),
        2,
        "scene guard: a worked call and a call never worked have a history"
    );
    for (call, entry) in history {
        takes(
            &format!("js8Context: {call}'s history"),
            entry,
            &["count", "lastUnix", "grid", "name", "comment"],
        );
    }
}

/// A received SSTV picture as the gallery fetches it (`sstv.ts`): the read's two keys, the
/// picture's six, and two each chunk of it.
#[test]
fn an_sstv_picture_carries_only_the_keys_the_page_takes() {
    let gallery = super::super::sstv::fixture::Gallery::new();
    let engine = engine();
    gallery.seed(&mut engine.lock().unwrap());
    // The sample hands out a picture's id and the read takes it back, so the two share the ids.
    let images = super::super::sstv::SharedImages::default();
    let mut samples = application::Publisher::default();
    samples.sstv_images = images.clone();
    let id = sample(&engine, &mut samples, Command::Sstv)["state"]["gallery"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut publisher = Publisher {
        sstv_images: images,
        ..Default::default()
    };
    let sources = Sources {
        sstv: gallery.source(),
        ..sources()
    };
    let page = read(&mut publisher, &engine, &sources, "sstvImage", &id, None)
        .unwrap_or_else(|e| panic!("sstvImage: the station refused the read: {e}"));
    takes(
        "sstvImage: the meta",
        &page["meta"],
        &["source", "capturedAgeMs"],
    );
    takes(
        "sstvImage: the picture",
        &page["meta"]["source"],
        &["imageId", "mime", "byteLength", "sha256", "width", "height"],
    );
    let chunks = page["rows"].as_array().unwrap();
    assert!(
        !chunks.is_empty(),
        "scene guard: the picture's bytes are sent"
    );
    for chunk in chunks {
        takes("sstvImage: a chunk", chunk, &["index", "base64"]);
    }
}

/// The SSTV cockpit's sample (`sstv.ts`): three keys at its root, fifteen the receiver's state,
/// eleven its health, five a received picture with its FSK ID beside them when one was read, and
/// seven each band-plan channel.
#[test]
fn the_sstv_sample_carries_only_the_keys_the_page_takes() {
    let gallery = super::super::sstv::fixture::Gallery::new();
    let engine = engine();
    gallery.seed(&mut engine.lock().unwrap());
    let data = sample(
        &engine,
        &mut application::Publisher::default(),
        Command::Sstv,
    );
    takes(
        "sstv: the sample",
        &data,
        &["state", "capturedAtMs", "plan"],
    );
    takes(
        "sstv: the state",
        &data["state"],
        &[
            "armed",
            "mode",
            "linesDone",
            "linesTotal",
            "previewRgbBase64",
            "previewWidth",
            "previewHeight",
            "hedrShiftHz",
            "gallery",
            "health",
            "sending",
            "txMode",
            "txProgress",
            "txElapsedSecs",
            "txTotalSecs",
        ],
    );
    takes(
        "sstv: its health",
        &data["state"]["health"],
        &[
            "armed",
            "audioPeak",
            "lastAudioUnix",
            "drains",
            "visSeen",
            "lastVisUnix",
            "unknownVis",
            "lastUnknownVisCode",
            "lastUnknownVisUnix",
            "images",
            "lastImageUnix",
        ],
    );
    let pictures = data["state"]["gallery"].as_array().unwrap();
    assert!(
        !pictures.is_empty() && pictures.iter().all(|p| p["fskId"].is_string()),
        "scene guard: each picture is sent with its FSK ID"
    );
    for picture in pictures {
        takes_optional(
            "sstv: a picture",
            picture,
            &["path", "mode", "finishedUtc", "freqMhz", "lines"],
            &["fskId"],
        );
    }
    let plan = data["plan"].as_array().unwrap();
    assert!(!plan.is_empty(), "scene guard: the band plan is sent");
    for channel in plan {
        takes(
            "sstv: a channel",
            channel,
            &["band", "group", "dialMhz", "mode", "label", "note", "tx"],
        );
    }
}

/// The APRS cockpit's live sample (`aprs.ts`): four keys at its root, thirteen the receiver's
/// health, nine the APRS-IS link's, and nine the settings it shows.
#[test]
fn the_aprs_sample_carries_only_the_keys_the_page_takes() {
    let data = sample(
        &engine(),
        &mut application::Publisher::default(),
        Command::Aprs,
    );
    takes(
        "aprs: the sample",
        &data,
        &["health", "isStatus", "settings", "capturedAtMs"],
    );
    takes(
        "aprs: the receiver's health",
        &data["health"],
        &[
            "arm",
            "audioPeak",
            "lastAudioUnix",
            "drains",
            "framesSeen",
            "framesDecoded",
            "lastDecodeUnix",
            "lastFrameSeenUnix",
            "framePeak",
            "maxFramePeak",
            "frameClippedSamples",
            "radioName",
            "bandRadioCount",
        ],
    );
    takes(
        "aprs: the APRS-IS link",
        &data["isStatus"],
        &[
            "enabled",
            "connected",
            "verified",
            "packets",
            "lastPacketUnix",
            "uplinkEnabled",
            "uploaded",
            "gateRejected",
            "lastReject",
        ],
    );
    takes(
        "aprs: the settings",
        &data["settings"],
        &[
            "mygrid",
            "aprsChannelMhz",
            "aprsComment",
            "aprsPath",
            "aprsSymbolTable",
            "aprsSymbolCode",
            "aprsIsEnabled",
            "aprsIsRadiusKm",
            "aprsIsWatchCalls",
        ],
    );
}

/// The satellite cockpit's live sample (`navigation.ts`): four keys at its root and five the
/// settings it shows. The pass being tracked and the transponder held are read open.
#[test]
fn the_satellite_sample_carries_only_the_keys_the_page_takes() {
    let data = sample(
        &engine(),
        &mut application::Publisher::default(),
        Command::Satellite,
    );
    takes(
        "satellite: the sample",
        &data,
        &["capturedAtMs", "settings", "track", "held"],
    );
    takes(
        "satellite: the settings",
        &data["settings"],
        &[
            "mygrid",
            "rotatorConfigured",
            "satDopplerOff",
            "satVfoMap",
            "radioPegged",
        ],
    );
}

/// The JS8 cockpit's sample (`js8.ts`): two keys at its root, twenty-three the state, five what is
/// armed, nine each station heard and each inbox message, eleven a decoded row, and four a queued
/// row and a pending reply. The station and the message come from the station's own journal; a
/// decoded row, a queued one and a pending reply are written field by field.
#[test]
fn the_js8_sample_carries_only_the_keys_the_page_takes() {
    const DECODED: &[&str] = &[
        "atMs",
        "speed",
        "freqHz",
        "snrDb",
        "dtS",
        "from",
        "text",
        "directedToMe",
        "mine",
        "complete",
        "lowConf",
    ];
    const QUEUED: &[&str] = &["origin", "display", "first", "last"];
    const PENDING: &[&str] = &["origin", "to", "display", "firesAtMs"];
    let engine = engine();
    engine.lock().unwrap().js8_load_journal(
        &json!({"heard": [{"call": "W1AW", "grid": "FN31", "snrDb": -8, "freqHz": 1500.0,
            "speed": "normal", "lastMs": 1, "lastHb": true, "lastCq": false, "storedMsgs": 1}],
        "inbox": [{"id": 1, "from": "W1AW", "to": "K2ABC", "text": "HELLO", "path": ["W1AW"],
            "state": "unread", "atMs": 1, "freqHz": 1500.0, "snrDb": -8}],
        "allcallReplied": [], "nextInboxId": 2})
        .to_string(),
    );
    let data = sample(
        &engine,
        &mut application::Publisher::default(),
        Command::Js8,
    );
    takes("js8: the sample", &data, &["state", "capturedAtMs"]);
    let state = &data["state"];
    takes(
        "js8: the state",
        state,
        &[
            "speed",
            "rxSpeeds",
            "txEnabled",
            "sending",
            "hbOn",
            "hbNextAtMs",
            "hbIntervalMin",
            "cqOn",
            "cqNextAtMs",
            "cqIntervalMin",
            "autoreply",
            "relay",
            "hbAck",
            "armed",
            "idleMinutes",
            "idleLimitMin",
            "idleTripped",
            "activity",
            "stations",
            "inbox",
            "queue",
            "pendingReply",
            "lastError",
        ],
    );
    takes(
        "js8: what is armed",
        &state["armed"],
        &["autoreply", "relay", "hbAck", "hb", "cq"],
    );
    let (stations, inbox) = (
        state["stations"].as_array().unwrap(),
        state["inbox"].as_array().unwrap(),
    );
    assert!(
        !stations.is_empty() && !inbox.is_empty(),
        "scene guard: the journal's station and message are sent"
    );
    for station in stations {
        takes(
            "js8: a station",
            station,
            &[
                "call",
                "grid",
                "snrDb",
                "freqHz",
                "speed",
                "lastMs",
                "lastHb",
                "lastCq",
                "storedMsgs",
            ],
        );
    }
    for message in inbox {
        takes(
            "js8: a message",
            message,
            &[
                "id", "from", "to", "text", "path", "state", "atMs", "freqHz", "snrDb",
            ],
        );
    }
    for row in state["activity"].as_array().unwrap() {
        takes("js8: a decoded row", row, DECODED);
    }
    for row in state["queue"].as_array().unwrap() {
        takes("js8: a queued row", row, QUEUED);
    }
    if !state["pendingReply"].is_null() {
        takes("js8: the pending reply", &state["pendingReply"], PENDING);
    }
    // The rows a journal does not hold, written out.
    let decoded = tempo_app::dto::Js8ActivityRow {
        at_ms: 1,
        speed: serde_json::from_value(json!("normal")).unwrap(),
        freq_hz: 1500.0,
        snr_db: -8,
        dt_s: 0.2,
        from: "W1AW".into(),
        text: "W1AW: K2ABC HELLO".into(),
        directed_to_me: true,
        mine: false,
        complete: true,
        low_conf: false,
    };
    takes(
        "js8: a decoded row, every field",
        &serde_json::to_value(decoded).unwrap(),
        DECODED,
    );
    let queued = tempo_app::dto::Js8QueueRow {
        origin: serde_json::from_value(json!("operator")).unwrap(),
        display: "K2ABC HELLO".into(),
        first: true,
        last: true,
    };
    takes(
        "js8: a queued row, every field",
        &serde_json::to_value(queued).unwrap(),
        QUEUED,
    );
    let pending = tempo_app::dto::Js8PendingReply {
        origin: serde_json::from_value(json!("autoReply")).unwrap(),
        to: "W1AW".into(),
        display: "W1AW SNR -08".into(),
        fires_at_ms: 1,
    };
    takes(
        "js8: a pending reply, every field",
        &serde_json::to_value(pending).unwrap(),
        PENDING,
    );
}

/// The band buttons' choices, which the settings sample carries (`band-choices.ts`): two lists,
/// and seven keys each channel, as the desktop's band plan writes them for the station's licence.
#[test]
fn a_band_choice_carries_only_the_keys_the_page_takes() {
    let engine = engine();
    engine.lock().unwrap().set_license_class("extra");
    let data = sample(
        &engine,
        &mut application::Publisher::default(),
        Command::Settings,
    );
    takes(
        "settings: the band choices",
        &data["bandChoices"],
        &["cw", "phone"],
    );
    for mode in ["cw", "phone"] {
        let choices = data["bandChoices"][mode].as_array().unwrap();
        assert!(
            !choices.is_empty(),
            "scene guard: an Extra has {mode} bands"
        );
        for channel in choices {
            takes(
                &format!("settings: a {mode} band choice"),
                channel,
                &["band", "group", "dialMhz", "mode", "label", "note", "tx"],
            );
        }
    }
}
