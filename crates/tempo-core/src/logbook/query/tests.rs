//! The port held to the JavaScript it ports: the string semantics one by one, the UI's band table
//! against `ui/src/band.ts` itself, and every question C17b froze as a golden answered by
//! [`super::reference`] from the golden log.

use super::*;
use crate::logbook::{parse_adif, QslRcvd, QslSent, QslVia, RecordId, UploadOutcome, UploadStatus};
use serde_json::{json, Value};

// ── the string semantics ────────────────────────────────────────────────────────────────────────

#[test]
fn trim_is_javascripts_not_rusts() {
    // U+FEFF is JavaScript whitespace and not Unicode's White_Space; U+0085 the reverse.
    assert_eq!(js_trim("\u{FEFF} W1AW \u{85}"), "W1AW \u{85}");
    assert_eq!("\u{FEFF} W1AW \u{85}".trim(), "\u{FEFF} W1AW");
    assert_eq!(js_trim("\u{A0}\u{3000}\tK1ABC\u{2028}\n"), "K1ABC");
    assert_eq!(
        js_trim(" \u{200B}X"),
        "\u{200B}X",
        "a zero-width space is not a space"
    );
}

#[test]
fn case_folds_are_unicodes_full_mappings() {
    assert_eq!(js_upper("ſtraße"), "STRASSE");
    assert_eq!(js_upper("dſ1x"), "DS1X");
    assert_eq!(js_upper("ı5"), "I5");
    assert_eq!(js_upper("ﬁ"), "FI");
    assert_eq!(js_lower("ÅLAND"), "åland");
    // The final sigma, as JavaScript's toLowerCase applies it.
    assert_eq!(js_lower("ΟΔΟΣ"), "οδος");
    assert_eq!(js_lower("İ"), "i\u{307}");
}

#[test]
fn strings_compare_by_utf16_code_units() {
    // U+E000 is below U+10000 as a code point, but its unit is above the pair's high surrogate.
    assert_eq!("\u{E000}".cmp("\u{10000}"), Ordering::Less);
    assert_eq!(utf16_cmp("\u{E000}", "\u{10000}"), Ordering::Greater);
    assert_eq!(utf16_cmp("\u{10000}", "\u{10001}"), Ordering::Less);
    assert_eq!(utf16_cmp("AB", "ABC"), Ordering::Less);
    assert_eq!(utf16_cmp("B", "ABC"), Ordering::Greater);
    assert_eq!(utf16_cmp("Å", "Å"), Ordering::Equal);
}

#[test]
fn times_format_as_date_formats_them() {
    assert_eq!(fmt_utc(0), "1970-01-01 00:00Z");
    assert_eq!(fmt_utc(1_715_350_800), "2024-05-10 14:20Z");
    assert_eq!(fmt_utc(1_695_115_140), "2023-09-19 09:19Z");
    assert_eq!(fmt_utc(951_782_400), "2000-02-29 00:00Z", "a leap day");
    assert_eq!(
        fmt_utc(253_402_300_800),
        "10000-01-01 00:00Z",
        "the year unpadded"
    );
    assert_eq!(
        fmt_utc(8_640_000_000_000),
        "275760-09-13 00:00Z",
        "the last Date"
    );
    assert_eq!(
        fmt_utc(8_640_000_000_001),
        "NaN-NaN-NaN NaN:NaNZ",
        "past it"
    );
}

#[test]
fn modes_and_bands_key_as_the_ui_keys_them() {
    for (mode, key) in [
        ("usb", "SSB"),
        (" LSB ", "SSB"),
        ("bpsk31", "PSK31"),
        ("BPSK63", "PSK63"),
        ("FT8", "FT8"),
        ("PH", "PH"),
        ("", ""),
    ] {
        assert_eq!(mode_key(mode), key, "{mode:?}");
    }
    for (band, mhz, key) in [
        ("20m", 0.0, Some("20M")),
        (" 20M ", 0.0, Some("20M")),
        ("", 14.074, Some("20M")),
        ("-fm", 146.52, Some("2M")),
        ("junk", 0.0, None),
        ("", 0.0, None),
        ("", f64::NAN, None),
        ("", 13.9, None),
        ("70cm", 0.0, Some("70CM")),
        ("", 70.5, Some("4M")),
        ("1.25cm", 0.0, Some("1.25CM")),
    ] {
        assert_eq!(band_key(band, mhz).as_deref(), key, "{band:?} {mhz}");
    }
}

/// The mode a logged contact counts as for "new mode", from the table the needs engine
/// (`crates/propagation`) and the UI's `modeKey` read too: what the ADIF reader stores for each
/// MODE/SUBMODE pair, and the key the card's answer folds that to.
#[test]
fn a_logged_mode_keys_as_the_shared_table_says() {
    let table: Value = serde_json::from_str(include_str!(
        "../../../../../ui/src/features/__fixtures__/mode-keys.json"
    ))
    .expect("mode-keys.json");
    let field = |row: &Value, f: &str| -> String {
        row[f]
            .as_str()
            .unwrap_or_else(|| panic!("{f} in {row}"))
            .to_string()
    };
    for row in table["folds"].as_array().expect("folds") {
        let mode = field(row, "mode");
        let submode = row["submode"]
            .as_str()
            .map(|s| format!("<SUBMODE:{}>{s}", s.len()))
            .unwrap_or_default();
        let adif = format!(
            "<CALL:4>K1JT<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m<MODE:{}>{mode}{submode}<EOR>",
            mode.len()
        );
        let records = parse_adif(&adif);
        assert_eq!(records.len(), 1, "{adif}");
        assert_eq!(
            records[0].mode,
            field(row, "stored"),
            "stored mode of {row}"
        );
        assert_eq!(
            mode_key(&records[0].mode),
            field(row, "key"),
            "key of {row}"
        );
    }
    for row in table["spellings"].as_array().expect("spellings") {
        assert_eq!(mode_key(&field(row, "stored")), field(row, "key"), "{row}");
    }
}

/// `UI_BANDS` IS `ui/src/band.ts`'s `BAND_RANGES`: read off the TypeScript, so a band added
/// there and not here — or an edge moved — fails here rather than answering differently.
#[test]
fn the_band_table_is_the_uis() {
    let ts = include_str!("../../../../../ui/src/band.ts");
    let body = ts
        .split_once("const BAND_RANGES: BandRange[] = [")
        .expect("the table")
        .1
        .split_once("\n]")
        .expect("its end")
        .0;
    let field = |line: &str, name: &str| -> String {
        let rest = line.split_once(&format!("{name}: ")).expect(name).1;
        rest.split([',', ' ', '}'])
            .next()
            .expect("a value")
            .trim_matches('\'')
            .to_string()
    };
    let parsed: Vec<(f64, f64, String)> = body
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("{ lo:"))
        .map(|l| {
            (
                field(l, "lo").parse().expect("lo"),
                field(l, "hi").parse().expect("hi"),
                field(l, "label"),
            )
        })
        .collect();
    assert_eq!(parsed.len(), UI_BANDS.len(), "the same number of bands");
    for (ours, theirs) in UI_BANDS.iter().zip(&parsed) {
        assert_eq!(
            (ours.0, ours.1, ours.2),
            (theirs.0, theirs.1, theirs.2.as_str())
        );
    }
}

// ── the goldens ─────────────────────────────────────────────────────────────────────────────────

/// C17b's fixture log and its frozen answers (`ui/src/features/logAnswers.golden.test.ts`).
pub(crate) const GOLDEN_LOG: &str =
    include_str!("../../../../../ui/src/features/__fixtures__/log-query/log.json");
pub(crate) const GOLDEN_ANSWERS: &str =
    include_str!("../../../../../ui/src/features/__fixtures__/log-query/answers.json");

/// The statistics' COUNTS the engine answers with (`countLogStats`), over the golden log and over
/// a log built to tie (`ui/src/features/logStats.countFinish.test.ts`, which also pins that the
/// golden log's counts finish to the golden answer). The window orders them; the engine counts.
pub(crate) const STATISTICS: &str =
    include_str!("../../../../../ui/src/features/__fixtures__/log-query/statistics.json");

/// A log of the statistics fixture: its records, each call's entity, and its counts.
pub(crate) type CountedLog = (Vec<QsoRecord>, HashMap<String, Option<String>>, Value);

/// The fixture's statistics counts: the golden log's, and the tied log with its counts.
pub(crate) fn statistics_goldens() -> (Value, Vec<CountedLog>) {
    let all: Vec<Value> = serde_json::from_str(STATISTICS).expect("statistics.json");
    let golden = all
        .iter()
        .find(|x| x["name"] == "golden")
        .expect("the golden log's counts")["counts"]
        .clone();
    let logs = all
        .iter()
        .filter(|x| x["log"].is_array())
        .map(|x| {
            let mut entities = HashMap::new();
            let log = x["log"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    // `stNNN` read as a fixture id the loader knows (`fx1NNN`).
                    let mut v = v.clone();
                    let n: u64 = v["id"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("st")
                        .parse()
                        .unwrap();
                    v["id"] = json!(format!("fx{}", 1000 + n));
                    let r = golden_record(&v);
                    let entity = v["entity"].as_str().map(str::to_string);
                    let held = entities.insert(r.call.clone(), entity.clone());
                    assert!(
                        held.is_none_or(|e| e == entity),
                        "{}: one entity per call",
                        r.call
                    );
                    r
                })
                .collect();
            (log, entities, x["counts"].clone())
        })
        .collect();
    (golden, logs)
}

/// A golden row as a record, the way the engine holds a contact: every field the fixture sets,
/// and its id as a provisional id numbered like the fixture's (`fx07` → `~…07`).
pub(crate) fn golden_record(v: &Value) -> QsoRecord {
    let mut r = parse_adif("<CALL:1>X<QSO_DATE:8>20240101<TIME_ON:4>0000<EOR>").remove(0);
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let id = v["id"].as_str().expect("a golden row has an id");
    r.id = Some(golden_id(id));
    r.call = text("call").expect("call");
    r.grid = text("grid");
    r.country = text("country");
    r.state = text("state");
    r.band = text("band").expect("band");
    r.freq_mhz = v["freqMhz"].as_f64().expect("freqMhz");
    r.mode = text("mode").expect("mode");
    r.rst_sent = text("rstSent");
    r.rst_rcvd = text("rstRcvd");
    r.name = text("name");
    r.qth = text("qth");
    r.comment = text("comment");
    r.notes = text("notes");
    r.when_unix = v["whenUnix"].as_u64().expect("whenUnix");
    r.time_known = v["timeKnown"].as_bool().unwrap_or(true);
    let flag = |o: &Value, k: &str| o.get(k).and_then(Value::as_bool).unwrap_or(false);
    let rcvd = &v["qslRcvd"];
    r.qsl_rcvd = QslRcvd {
        card: flag(rcvd, "card"),
        lotw: flag(rcvd, "lotw"),
        eqsl: flag(rcvd, "eqsl"),
        qrz: flag(rcvd, "qrz"),
    };
    // The store derives both from the channels, so the fixture must agree with that rule.
    r.confirmed = r.qsl_rcvd.any();
    r.award_confirmed = r.qsl_rcvd.award();
    assert_eq!(
        (r.confirmed, r.award_confirmed),
        (flag(v, "confirmed"), flag(v, "awardConfirmed")),
        "{id}: the fixture's confirmation flags follow its channels"
    );
    let sent = &v["qslSent"];
    r.qsl_sent = QslSent {
        sent: flag(sent, "sent"),
        via: sent
            .get("via")
            .and_then(Value::as_str)
            .and_then(QslVia::from_code),
        date_unix: sent.get("dateUnix").and_then(Value::as_u64),
        cleared_unix: None,
    };
    let ota = &v["ota"];
    let ota_text = |k: &str| ota.get(k).and_then(Value::as_str).map(str::to_string);
    r.ota.my_program = ota_text("myProgram");
    r.ota.my_ref = ota_text("myRef");
    r.ota.their_program = ota_text("theirProgram");
    r.ota.their_ref = ota_text("theirRef");
    r.ota.iota = ota_text("iota");
    if let Some(lotw) = v["upload"].get("lotw") {
        r.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::from_code(lotw["outcome"].as_str().expect("outcome"))
                .expect("a known outcome"),
            when_unix: lotw["whenUnix"].as_i64().unwrap_or(0),
            detail: None,
        });
    }
    r.prop_mode = text("propMode");
    r.sat_name = text("satName");
    r
}

/// The fixture's id `fxNN` as a record id, and back.
pub(crate) fn golden_id(id: &str) -> RecordId {
    let n: u64 = id.trim_start_matches("fx").parse().expect("fxNN");
    RecordId::Provisional {
        hash: n,
        ordinal: 0,
    }
}
pub(crate) fn golden_name(id: &RecordId) -> String {
    match id {
        RecordId::Provisional { hash, .. } => format!("fx{hash:02}"),
        other => panic!("not a golden id: {other}"),
    }
}

/// The golden log: its records, and each record's resolved entity as the fixture gives it.
pub(crate) fn golden_log() -> (Vec<QsoRecord>, HashMap<RecordId, Option<String>>) {
    let rows: Vec<Value> = serde_json::from_str(GOLDEN_LOG).expect("log.json");
    let mut entities = HashMap::new();
    let log = rows
        .iter()
        .map(|v| {
            let r = golden_record(v);
            entities.insert(
                r.id.expect("an id"),
                v.get("entity").and_then(Value::as_str).map(str::to_string),
            );
            r
        })
        .collect();
    (log, entities)
}

/// Every golden `(question, answer)`.
pub(crate) fn golden_answers() -> Vec<(Value, Value)> {
    let all: Vec<Value> = serde_json::from_str(GOLDEN_ANSWERS).expect("answers.json");
    all.into_iter()
        .map(|x| (x["q"].clone(), x["a"].clone()))
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|s| s.as_str().expect("a string").to_string())
        .collect()
}

/// The reference's answer to golden question `q`, normalised as the golden file stores answers:
/// a row as its id, a page as its numbers and keys.
fn reference_answer(
    log: &[QsoRecord],
    entities: &HashMap<RecordId, Option<String>>,
    q: &Value,
) -> Value {
    let name = |r: &QsoRecord| golden_name(&r.id.expect("an id"));
    let query = || -> LogQuery { serde_json::from_value(q["query"].clone()).expect("a query") };
    // A row's entity, as the fixture gives it (`q.entity`).
    let entity = |r: &QsoRecord| entities[&r.id.expect("an id")].clone();
    match q["kind"].as_str().expect("kind") {
        "page" => {
            let order = reference::order(log, &query(), &entity);
            let (offset, limit) = (
                q["offset"].as_u64().unwrap() as usize,
                q["limit"].as_u64().unwrap() as usize,
            );
            let keys: Vec<String> = order
                .iter()
                .skip(offset)
                .take(limit)
                .map(|&i| name(&log[i]))
                .collect();
            json!({"total": order.len(), "logSize": log.len(), "offset": offset, "keys": keys})
        }
        "locate" => {
            let order = reference::order(log, &query(), &entity);
            let id = q["id"].as_str().unwrap();
            json!({"index": order.iter().position(|&i| name(&log[i]) == id)})
        }
        "callHistory" => {
            let h = reference::call_history(
                log,
                q["call"].as_str().unwrap(),
                q["band"].as_str().unwrap(),
                q["mode"].as_str().unwrap(),
                q["matchMode"].as_bool().unwrap(),
            );
            json!({
                "count": h.count,
                "workedBefore": h.worked_before,
                "dupeThisBand": h.dupe_this_band,
                "lastUnix": h.last_unix,
                "confirmedCount": h.confirmed_count,
                "bands": h.bands,
                "modes": h.modes,
                "qsos": h.qsos.iter().map(|&i| name(&log[i])).collect::<Vec<_>>(),
            })
        }
        "entity" => {
            let of = |r: &QsoRecord| entities[&r.id.expect("an id")].clone();
            serde_json::to_value(reference::entity(log, q["entity"].as_str().unwrap(), &of))
                .unwrap()
        }
        "callsSummary" => {
            let summary = calls_summary(log, &strings(&q["calls"]));
            Value::Object(
                summary
                    .into_iter()
                    .map(|(c, s)| (c, serde_json::to_value(s).unwrap()))
                    .collect(),
            )
        }
        "workedCalls" => json!(reference::worked_calls(log, &strings(&q["calls"]))),
        "rowsAt" => json!(q["indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i
                .as_i64()
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| log.get(i))
                .map(name))
            .collect::<Vec<_>>()),
        "row" => {
            let id = q["id"].as_str().unwrap();
            json!(log.iter().find(|r| name(r) == id).map(name))
        }
        "logSize" => json!(log.len()),
        "workedGrids" => json!(reference::fold(log, WorkedGrids::default())),
        "gridPoints" => json!(reference::fold(
            log,
            GridCounts::new(q["band"].as_str().unwrap())
        )),
        "bandsInLog" => json!(reference::fold(log, BandsInLog::default())),
        "lotwBacklog" => json!(reference::fold(log, LotwBacklog::default())),
        // The COUNTS, which the window orders (`finishLogStats`) into the golden answer.
        "statistics" => json!(reference::fold(log, LogStatCounter::new(&entity))),
        other => panic!("no reference answer for {other}"),
    }
}

/// ★ THE PORT ANSWERS WHAT THE UI ANSWERED. Every golden question, asked of the reference over
/// the golden log, gets exactly the frozen answer — the statistics as the counts the window
/// finishes into it — and the count of questions cannot shrink by one dropped quietly.
#[test]
fn the_reference_gives_every_golden_answer() {
    let (log, entities) = golden_log();
    assert_eq!(log.len(), 24, "the fixture's 24 rows");
    let (statistics, _) = statistics_goldens();
    let mut answered = 0;
    for (q, a) in golden_answers() {
        let want = if q["kind"] == "statistics" {
            statistics.clone()
        } else {
            a
        };
        assert_eq!(reference_answer(&log, &entities, &q), want, "{q}");
        answered += 1;
    }
    assert_eq!(answered, 78, "every question in the golden file");
}

/// ★ The statistics are COUNTED as the UI counts them, on a log built to tie and to break a port —
/// labels that differ only in case or accents, full Unicode case maps (`uſb` is a sideband, `ſc`
/// a state), an empty entity that does not fall back, date-only contacts, instants no `Date`
/// holds. The same counts `countLogStats` gives (the window finishes them).
#[test]
fn the_statistics_counts_are_the_uis() {
    let (_, logs) = statistics_goldens();
    assert!(!logs.is_empty(), "premise: the tied log is in the fixture");
    for (log, entities, counts) in logs {
        let entity = |call: &str| entities.get(call).cloned().flatten();
        let got = json!(reference::fold(
            &log,
            LogStatCounter::new(&|r: &QsoRecord| entity(&r.call))
        ));
        assert_eq!(got, counts, "{} rows", log.len());
    }
}

/// A row's entity is asked of the resolver once per call AND the record's DXCC numbers, never
/// once per call: one call is two entities when its records carry two numbers (K1N, Navassa in
/// 2015 and a US call since), or LoTW's beside the log's (K1B).
#[test]
fn the_statistics_ask_each_calls_entity_apart_for_each_records_numbers() {
    let rec = |call: &str, dxcc: Option<u32>, lotw: Option<&str>| {
        let mut r = parse_adif("<CALL:1>X<QSO_DATE:8>20240101<TIME_ON:4>1200<EOR>").remove(0);
        r.call = call.to_string();
        r.dxcc = dxcc;
        crate::logbook::set_extra(
            &mut r,
            crate::logbook::LOTW_DXCC_FIELD,
            lotw.map(str::to_string),
        );
        r
    };
    let log = [
        rec("K1N", Some(182), None),
        rec("K1N", None, None),
        rec("K1B", Some(291), Some("20")),
        rec("K1B", Some(291), None),
    ];
    // As the app names a row's entity: LoTW's number, else the log's, else the call's — all
    // three calls here are US calls today.
    let entity = |r: &QsoRecord| {
        let name = match crate::logbook::record_lotw_dxcc(r).or(r.dxcc) {
            Some(182) => "Navassa Island",
            Some(20) => "Baker & Howland Islands",
            _ => "United States",
        };
        Some(name.to_string())
    };
    let counts = reference::fold(&log, LogStatCounter::new(&entity));
    let entities: Vec<(&str, usize)> = counts
        .entities
        .iter()
        .map(|t| (t.label.as_str(), t.count))
        .collect();
    assert_eq!(
        entities,
        [
            ("Navassa Island", 1),
            ("United States", 2),
            ("Baker & Howland Islands", 1),
        ]
    );
}

/// The WAS gate is the UI's: its three US-family entities and fifty codes, read out of
/// `logStats.ts` itself.
#[test]
fn the_was_lists_are_the_uis() {
    let ts = include_str!("../../../../../ui/src/features/logStats.ts");
    let list = |name: &str| -> Vec<String> {
        let from = ts.find(&format!("const {name} = new Set([")).expect(name);
        let body = &ts[from..from + ts[from..].find("])").expect("its end")];
        body.split('\'')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    };
    assert_eq!(list("US_ENTITIES"), US_ENTITIES);
    assert_eq!(list("WAS_STATES"), WAS_STATES);
}

/// ★ BY STATE counts a contact by its DXCC entity, the United States, Alaska or Hawaii, however
/// its source spelled COUNTRY (LoTW's and DXKeeper's `UNITED STATES OF AMERICA`, or none at all),
/// and by the stored text only where its call resolves to no entity, as the entities are counted.
/// A US mailing country on a foreign entity's contact is not a state.
#[test]
fn by_state_counts_a_contact_by_its_dxcc_entity() {
    // (call, the entity its call resolves to, the stored COUNTRY, STATE)
    let rows: [(&str, Option<&str>, Option<&str>, &str); 7] = [
        (
            "W1AW",
            Some("United States"),
            Some("UNITED STATES OF AMERICA"),
            "CT",
        ),
        ("K1ABC", Some("United States"), None, "MA"),
        ("KL7AA", Some("Alaska"), Some("ALASKA"), "AK"),
        ("KH6AA", Some("Hawaii"), Some("HAWAII"), "HI"),
        ("KP4AA", Some("Puerto Rico"), Some("United States"), "FL"),
        // No entity: the stored text decides, as it always has.
        ("Q0ZZZ", None, Some("United States"), "WI"),
        ("Q1ZZZ", None, Some("UNITED STATES OF AMERICA"), "NY"),
    ];
    let log: Vec<QsoRecord> = rows
        .iter()
        .map(|&(call, _, country, state)| {
            let mut r = parse_adif("<CALL:1>X<QSO_DATE:8>20240101<TIME_ON:4>1200<EOR>").remove(0);
            r.call = call.to_string();
            r.country = country.map(str::to_string);
            r.state = Some(state.to_string());
            r
        })
        .collect();
    let entity = |call: &str| {
        rows.iter()
            .find(|r| r.0 == call)
            .and_then(|r| r.1)
            .map(str::to_string)
    };
    let counts = reference::fold(&log, LogStatCounter::new(&|r: &QsoRecord| entity(&r.call)));
    let by_state: Vec<(&str, usize)> = counts
        .by_state
        .iter()
        .map(|t| (t.label.as_str(), t.count))
        .collect();
    assert_eq!(
        by_state,
        [("CT", 1), ("MA", 1), ("AK", 1), ("HI", 1), ("WI", 1)]
    );
}

/// The bands in the log are its own spellings, first seen first: nothing trimmed or folded, and
/// only the empty string left out (`if (r.band) seen.add(r.band)`).
#[test]
fn the_bands_in_the_log_are_its_own_spellings() {
    let (golden, _) = golden_log();
    let log: Vec<QsoRecord> = ["20m", "  ", "", "20M", "20m", " 40m"]
        .iter()
        .map(|band| {
            let mut r = golden[0].clone();
            r.band = band.to_string();
            r
        })
        .collect();
    assert_eq!(
        reference::fold(&log, BandsInLog::default()),
        ["20m", "  ", "20M", " 40m"]
    );
}

/// A square is the grid trimmed, upper-cased as JavaScript does and cut to four UTF-16 units.
#[test]
fn a_grid_square_is_cut_as_javascript_cuts_it() {
    assert_eq!(grid_square(Some(" fn31pr ")).as_deref(), Some("FN31"));
    assert_eq!(grid_square(Some("fn3")), None);
    assert_eq!(grid_square(None), None);
    // ß upper-cases to SS, so two of them make a four-unit square.
    assert_eq!(grid_square(Some("ßß")).as_deref(), Some("SSSS"));
    // A character above U+FFFF is two units: the cut through it leaves half, here U+FFFD.
    assert_eq!(
        grid_square(Some("FN3\u{1F600}")).as_deref(),
        Some("FN3\u{FFFD}")
    );
    assert_eq!(
        grid_square(Some("F\u{1F600}N3")).as_deref(),
        Some("F\u{1F600}N")
    );
}

/// The entity index — the engine's kept answer, grown row by row — answers every golden entity
/// question as the reference does, built in one go and built by appending.
#[test]
fn the_entity_index_is_the_reference() {
    let (log, entities) = golden_log();
    let mut index = EntityIndex::default();
    for r in &log {
        index.add(entities[&r.id.unwrap()].as_deref(), r);
    }
    let of = |r: &QsoRecord| entities[&r.id.expect("an id")].clone();
    for (q, a) in golden_answers() {
        if q["kind"] != "entity" {
            continue;
        }
        let entity = q["entity"].as_str().unwrap();
        assert_eq!(
            index.answer(entity),
            reference::entity(&log, entity, &of),
            "{entity}"
        );
        assert_eq!(
            serde_json::to_value(index.answer(entity)).unwrap(),
            a,
            "{entity}"
        );
    }
}

/// A contact whose band cannot be placed marks its entity's band unknown — whether it came as a
/// label no band has or as a frequency outside every band — and one with neither does not
/// (`entitySlots`: `(q.band ?? '').trim() || (q.freqMhz ?? 0) > 0`). The golden log has no
/// unplaceable frequency.
#[test]
fn a_band_that_cannot_be_placed_is_unknown_to_its_entity() {
    let (log, _) = golden_log();
    for (band, freq_mhz, unknown) in [
        ("", 30.5, true), // between 10 m and 6 m
        ("junk", 0.0, true),
        ("  ", 0.0, false),
        ("", 0.0, false),
        ("", f64::NAN, false), // JSON's null, which the UI reads as 0
    ] {
        let mut r = log[0].clone();
        r.band = band.to_string();
        r.freq_mhz = freq_mhz;
        let mut index = EntityIndex::default();
        index.add(Some("Somewhere"), &r);
        let answer = index.answer("somewhere");
        assert_eq!(
            (answer.slots.band_unknown, answer.slots.bands_worked.len()),
            (unknown, 0),
            "{band:?} at {freq_mhz} MHz"
        );
        let of = |_: &QsoRecord| Some("Somewhere".to_string());
        assert_eq!(
            answer,
            reference::entity(std::slice::from_ref(&r), "somewhere", &of)
        );
    }
}

/// The fixture is not vacuous where the port is subtle: "z" is in every formatted time, "ssb"
/// reaches the sidebands only through the mode fold, the twins keep log order both ways, ß folds
/// to SS, and the band map's key is untrimmed.
#[test]
fn the_golden_log_exercises_what_it_claims() {
    let (log, _) = golden_log();
    let q = |search: &str| LogQuery {
        sort: SortKey::Time,
        asc: false,
        search: search.to_string(),
        needs_confirm_only: false,
        then: Vec::new(),
        filter: Default::default(),
    };
    let none = |_: &QsoRecord| None;
    assert_eq!(reference::order(&log, &q("z"), &none).len(), 24);
    let ssb: Vec<String> = reference::order(&log, &q("ssb"), &none)
        .into_iter()
        .map(|i| golden_name(&log[i].id.unwrap()))
        .collect();
    assert!(ssb.contains(&"fx03".to_string()) && ssb.contains(&"fx04".to_string()));
    for asc in [true, false] {
        let by_call = reference::order(
            &log,
            &LogQuery {
                sort: SortKey::Call,
                asc,
                search: String::new(),
                needs_confirm_only: false,
                then: Vec::new(),
                filter: Default::default(),
            },
            &none,
        );
        let at = |n: &str| {
            by_call
                .iter()
                .position(|&i| golden_name(&log[i].id.unwrap()) == n)
                .unwrap()
        };
        assert!(
            at("fx17") < at("fx18"),
            "the twins keep log order, asc={asc}"
        );
    }
    assert_eq!(
        reference::worked_calls(&log, &["W1AW".into(), "W1AW ".into()]),
        ["W1AW".to_string()]
    );
}

/// ★ The country column sorts on the country the Logbook shows: the DXCC entity, so a QRZ
/// `Germany` and a `Fed. Rep. of Germany` sort together, and the stored text only where the call
/// resolves to no entity.
#[test]
fn the_country_column_sorts_on_the_entity_the_logbook_shows() {
    // (call, the entity its call resolves to, the stored COUNTRY)
    let rows: [(&str, Option<&str>, Option<&str>); 5] = [
        ("DL1ABC", Some("Fed. Rep. of Germany"), Some("Germany")),
        ("F5ABC", Some("France"), Some("France")),
        (
            "DL2XYZ",
            Some("Fed. Rep. of Germany"),
            Some("Fed. Rep. of Germany"),
        ),
        ("G4ABC", Some("England"), Some("England")),
        // No entity: the stored text decides.
        ("Q0ZZZ", None, Some("Freedonia")),
    ];
    let log: Vec<QsoRecord> = rows
        .iter()
        .enumerate()
        .map(|(i, &(call, _, country))| {
            let mut r = parse_adif("<CALL:1>X<QSO_DATE:8>20240101<TIME_ON:4>1200<EOR>").remove(0);
            r.call = call.to_string();
            r.country = country.map(str::to_string);
            r.when_unix += i as u64 * 60;
            r
        })
        .collect();
    let entity = |q: &QsoRecord| {
        rows.iter()
            .find(|r| r.0 == q.call)
            .and_then(|r| r.1)
            .map(str::to_string)
    };
    let sorted = |asc: bool| -> Vec<&str> {
        let query = LogQuery {
            sort: SortKey::Country,
            asc,
            search: String::new(),
            needs_confirm_only: false,
            then: Vec::new(),
            filter: Default::default(),
        };
        reference::order(&log, &query, &entity)
            .into_iter()
            .map(|i| log[i].call.as_str())
            .collect()
    };
    assert_eq!(
        sorted(true),
        ["G4ABC", "DL1ABC", "DL2XYZ", "F5ABC", "Q0ZZZ"]
    );
    assert_eq!(
        sorted(false),
        ["Q0ZZZ", "F5ABC", "DL2XYZ", "DL1ABC", "G4ABC"]
    );
}

/// The country column sorts on each ROW's entity, which its record's DXCC number can decide: two
/// contacts with one call (K1N, in two years) sort apart when one record names another entity —
/// the entity is asked per call AND number, never once per call.
#[test]
fn the_country_column_sorts_each_row_on_its_own_entity() {
    let row = |when: u64, dxcc: Option<u32>| {
        let mut r = parse_adif("<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:4>1200<EOR>").remove(0);
        r.when_unix += when;
        r.dxcc = dxcc;
        r
    };
    // A US special event first in the log, the 2015 Navassa operation after it, and Mexico.
    let mut xe = row(120, None);
    xe.call = "XE1ABC".into();
    let log = [row(0, None), row(60, Some(182)), xe];
    let entity = |q: &QsoRecord| {
        let name = match (q.call.as_str(), q.dxcc) {
            ("K1N", Some(182)) => "Navassa Island",
            ("K1N", _) => "United States",
            _ => "Mexico",
        };
        Some(name.to_string())
    };
    let query = LogQuery {
        sort: SortKey::Country,
        asc: true,
        search: String::new(),
        needs_confirm_only: false,
        then: Vec::new(),
        filter: Default::default(),
    };
    let sorted: Vec<(&str, Option<u32>)> = reference::order(&log, &query, &entity)
        .into_iter()
        .map(|i| (log[i].call.as_str(), log[i].dxcc))
        .collect();
    assert_eq!(
        sorted,
        [("XE1ABC", None), ("K1N", Some(182)), ("K1N", None)],
        "Mexico, Navassa Island, United States"
    );
}

// ── the timeline ────────────────────────────────────────────────────────────────────────────────

/// A contact of `mode` at `when_unix`.
fn worked(mode: &str, when_unix: u64) -> QsoRecord {
    let mut r = parse_adif("<CALL:4>W1AW<QSO_DATE:8>20240101<TIME_ON:4>1200<EOR>").remove(0);
    r.mode = mode.to_string();
    r.when_unix = when_unix;
    r
}

/// The timeline of `log` at `now`: (bucket, count) for each bucket with a contact, by mode.
fn timeline_of(
    log: &[QsoRecord],
    range: TimelineRange,
    now: u64,
) -> Vec<(String, Vec<(usize, usize)>)> {
    reference::fold(log, TimelineCounter::new(range, now))
        .modes
        .into_iter()
        .map(|m| {
            let hit = m.counts.iter().copied().enumerate().filter(|&(_, c)| c > 0);
            (m.label, hit.collect())
        })
        .collect()
}

/// 2026-10-14 15:37:12Z.
const OCT_14_1537: u64 = 1_791_992_232;

/// Each window ends with the current UTC hour, day or month and holds the 24, 30 or 12 before it,
/// edge by edge — also asked at the first instant of a month, and across a new year.
#[test]
fn the_timeline_windows_end_with_the_current_utc_hour_day_and_month() {
    let day = TimelineRange::PastDay.edges(OCT_14_1537);
    assert_eq!(day.len(), 25);
    assert_eq!(day[0], 1_791_907_200, "2026-10-13 16:00Z");
    assert_eq!(day[23], 1_791_990_000, "the current hour, 15:00Z");
    assert_eq!(day[24], 1_791_993_600, "its end, 16:00Z");
    assert!(day.windows(2).all(|w| w[1] - w[0] == 3_600));

    let days = TimelineRange::Past30Days.edges(OCT_14_1537);
    assert_eq!(days.len(), 31);
    assert_eq!(days[0], 1_789_430_400, "2026-09-15");
    assert_eq!(days[29], 1_791_936_000, "today, 2026-10-14");
    assert_eq!(days[30], 1_792_022_400, "its end, 2026-10-15");
    assert!(days.windows(2).all(|w| w[1] - w[0] == 86_400));

    // The first of each month from 2025-11 to 2026-11.
    assert_eq!(
        TimelineRange::PastYear.edges(OCT_14_1537),
        [
            1_761_955_200,
            1_764_547_200,
            1_767_225_600,
            1_769_904_000,
            1_772_323_200,
            1_775_001_600,
            1_777_593_600,
            1_780_272_000,
            1_782_864_000,
            1_785_542_400,
            1_788_220_800,
            1_790_812_800,
            1_793_491_200,
        ]
    );
    // At 2026-11-01 00:00:00Z the new month is the current one.
    let nov_1 = 1_793_491_200;
    assert_eq!(TimelineRange::PastDay.edges(nov_1)[23], nov_1);
    assert_eq!(TimelineRange::Past30Days.edges(nov_1)[29], nov_1);
    let year = TimelineRange::PastYear.edges(nov_1);
    assert_eq!(
        (year[0], year[11], year[12]),
        (1_764_547_200, nov_1, 1_796_083_200),
        "2025-12 to 2026-11, ending 2026-12-01"
    );
    // 2027-01-15 08:00Z: 2026-02 to 2027-01.
    let year = TimelineRange::PastYear.edges(1_800_000_000);
    assert_eq!(
        (year[0], year[11], year[12]),
        (1_769_904_000, 1_798_761_600, 1_801_440_000)
    );
}

/// A contact counts in the bucket its UTC time falls in: the second before midnight in the old
/// hour and day, midnight itself in the new ones.
#[test]
fn a_contact_is_counted_in_the_utc_hour_and_day_it_was_logged() {
    let now = 1_791_955_800; // 2026-10-14 05:30Z
    let log = [
        worked("FT8", 1_791_935_999), // 2026-10-13 23:59:59Z
        worked("FT8", 1_791_936_000), // 2026-10-14 00:00:00Z
        worked("FT8", 1_791_936_001),
    ];
    // The hour window starts at 2026-10-13 06:00Z: 23:00Z is its 18th bucket, 00:00Z its 19th.
    assert_eq!(
        timeline_of(&log, TimelineRange::PastDay, now),
        [("FT8".to_string(), vec![(17, 1), (18, 2)])]
    );
    // The day window starts at 2026-09-15: the 13th is its 29th day, the 14th (today) its 30th.
    assert_eq!(
        timeline_of(&log, TimelineRange::Past30Days, now),
        [("FT8".to_string(), vec![(28, 1), (29, 2)])]
    );
}

/// Across the end of a month: September 30's last second and October 1's first are a day apart in
/// the 30-day window and a month apart in the year.
#[test]
fn a_contact_is_counted_in_the_utc_month_it_was_logged() {
    let now = 1_791_979_200; // 2026-10-14 12:00Z
    let log = [
        worked("CW", 1_790_812_799), // 2026-09-30 23:59:59Z
        worked("CW", 1_790_812_800), // 2026-10-01 00:00:00Z
    ];
    assert_eq!(
        timeline_of(&log, TimelineRange::Past30Days, now),
        [("CW".to_string(), vec![(15, 1), (16, 1)])]
    );
    // 2025-11 is the first month: September is the 11th, October the 12th.
    assert_eq!(
        timeline_of(&log, TimelineRange::PastYear, now),
        [("CW".to_string(), vec![(10, 1), (11, 1)])]
    );
}

/// February 29 is a whole day of its own, and a leap year's February a month of 29 days.
#[test]
fn a_leap_day_is_a_day_and_its_february_a_month() {
    let now = 1_835_863_200; // 2028-03-05 10:00Z
    let log = [
        worked("SSB", 1_835_395_199), // 2028-02-28 23:59:59Z
        worked("SSB", 1_835_395_200), // 2028-02-29 00:00:00Z
        worked("SSB", 1_835_481_599), // 2028-02-29 23:59:59Z
        worked("SSB", 1_835_481_600), // 2028-03-01 00:00:00Z
    ];
    let days = TimelineRange::Past30Days.edges(now);
    assert_eq!(
        (days[0], days[30]),
        (1_833_321_600, 1_835_913_600),
        "2028-02-05 to 2028-03-06"
    );
    assert_eq!(
        (days[24], days[25]),
        (1_835_395_200, 1_835_481_600),
        "February 29, all day"
    );
    assert_eq!(
        timeline_of(&log, TimelineRange::Past30Days, now),
        [("SSB".to_string(), vec![(23, 1), (24, 2), (25, 1)])]
    );
    let year = TimelineRange::PastYear.edges(now);
    assert_eq!(year[11] - year[10], 29 * 86_400, "February 2028");
    assert_eq!(
        timeline_of(&log, TimelineRange::PastYear, now),
        [("SSB".to_string(), vec![(10, 3), (11, 1)])]
    );
}

/// Each window holds a contact at its first instant and the last second before its end — and not
/// the second before it starts, nor its end itself (a clock running ahead).
#[test]
fn a_window_holds_its_first_instant_and_not_the_second_before_it_or_its_end() {
    for range in [
        TimelineRange::PastDay,
        TimelineRange::Past30Days,
        TimelineRange::PastYear,
    ] {
        let edges = range.edges(OCT_14_1537);
        let (first, end, last) = (edges[0], edges[edges.len() - 1], edges.len() - 2);
        let log = [
            worked("FT8", first - 1),
            worked("FT8", first),
            worked("FT4", end - 1),
            worked("FT4", end),
        ];
        assert_eq!(
            timeline_of(&log, range, OCT_14_1537),
            [
                ("FT8".to_string(), vec![(0, 1)]),
                ("FT4".to_string(), vec![(last, 1)])
            ],
            "{range:?}"
        );
        assert_eq!(TimelineCounter::new(range, OCT_14_1537).since(), first);
    }
}

/// The lines are the "By mode" card's modes: the sidebands fold into SSB, nothing else folds
/// (case included), and a blank mode is on no line. First seen first.
#[test]
fn the_timeline_counts_modes_as_the_by_mode_card_does() {
    let at = OCT_14_1537 - 60;
    let log: Vec<QsoRecord> = ["FT8", "usb", " LSB ", "CW", "ft8", "", "  ", "SSB", "FT8"]
        .iter()
        .map(|m| worked(m, at))
        .collect();
    let timeline = reference::fold(
        &log,
        TimelineCounter::new(TimelineRange::PastDay, OCT_14_1537),
    );
    let lines: Vec<(&str, usize)> = timeline
        .modes
        .iter()
        .map(|m| (m.label.as_str(), m.counts.iter().sum()))
        .collect();
    assert_eq!(lines, [("FT8", 2), ("SSB", 3), ("CW", 1), ("ft8", 1)]);
    let by_mode: Vec<(String, usize)> =
        reference::fold(&log, LogStatCounter::new(&|_: &QsoRecord| None))
            .by_mode
            .into_iter()
            .map(|t| (t.label, t.count))
            .collect();
    let lines: Vec<(String, usize)> = lines.into_iter().map(|(m, c)| (m.to_string(), c)).collect();
    assert_eq!(lines, by_mode, "the By mode card's tally");
}

/// A window with no contact answers its edges and no mode; the answer crosses IPC as the UI
/// types it (`LogTimeline`), and the range is asked for by its name there.
#[test]
fn the_timeline_answers_in_the_uis_shape() {
    let empty = reference::fold(
        &[worked("FT8", 1_000_000_000)],
        TimelineCounter::new(TimelineRange::PastYear, OCT_14_1537),
    );
    assert_eq!(empty.edges.len(), 13);
    assert!(empty.modes.is_empty());

    let one = reference::fold(
        &[worked("FT8", OCT_14_1537)],
        TimelineCounter::new(TimelineRange::PastDay, OCT_14_1537),
    );
    let mut counts = vec![0; 24];
    counts[23] = 1;
    assert_eq!(
        serde_json::to_value(&one).unwrap(),
        json!({ "edges": one.edges, "modes": [{ "label": "FT8", "counts": counts }] })
    );
    for (name, range) in [
        ("pastDay", TimelineRange::PastDay),
        ("past30Days", TimelineRange::Past30Days),
        ("pastYear", TimelineRange::PastYear),
    ] {
        assert_eq!(
            serde_json::from_value::<TimelineRange>(json!(name)).unwrap(),
            range
        );
    }
}

/// ★ The search finds a contact by the country the Logbook shows, its DXCC entity, as well as
/// by its stored text: a QRZ `Germany` row is found by "fed. rep", and still by "germany".
#[test]
fn a_search_finds_a_contact_by_the_country_the_logbook_shows() {
    // (call, the entity its call resolves to, the stored COUNTRY)
    let rows: [(&str, Option<&str>, Option<&str>); 4] = [
        ("DL1ABC", Some("Fed. Rep. of Germany"), Some("Germany")),
        ("TA1XX", Some("Turkey"), Some("Türkiye")),
        ("G4ABC", Some("England"), Some("England")),
        ("Q0ZZZ", None, Some("Freedonia")),
    ];
    let log: Vec<QsoRecord> = rows
        .iter()
        .enumerate()
        .map(|(i, &(call, _, country))| {
            let mut r = parse_adif("<CALL:1>X<QSO_DATE:8>20240101<TIME_ON:4>1200<EOR>").remove(0);
            r.call = call.to_string();
            r.country = country.map(str::to_string);
            r.when_unix += i as u64 * 60;
            r
        })
        .collect();
    let entity = |q: &QsoRecord| {
        rows.iter()
            .find(|r| r.0 == q.call)
            .and_then(|r| r.1)
            .map(str::to_string)
    };
    let found = |search: &str, sort: SortKey| -> Vec<&str> {
        let query = LogQuery {
            sort,
            asc: true,
            search: search.to_string(),
            needs_confirm_only: false,
            then: Vec::new(),
            filter: Default::default(),
        };
        reference::order(&log, &query, &entity)
            .into_iter()
            .map(|i| log[i].call.as_str())
            .collect()
    };
    for sort in [SortKey::Call, SortKey::Country, SortKey::Time] {
        assert_eq!(found("fed. rep", sort), ["DL1ABC"], "{sort:?}");
        assert_eq!(found("TURKEY", sort), ["TA1XX"], "{sort:?}");
        // The stored text still finds its contact.
        assert_eq!(found("germany", sort), ["DL1ABC"], "{sort:?}");
        assert_eq!(found("türkiye", sort), ["TA1XX"], "{sort:?}");
        assert_eq!(found("freedonia", sort), ["Q0ZZZ"], "{sort:?}");
        assert!(found("nowhere", sort).is_empty(), "{sort:?}");
    }
}

/// The search finds a contact by the entity its ROW shows, which its record's DXCC number can
/// decide: the 2015 K1N contact set to Navassa is found by "navassa", and a K1N contact without
/// a number is not — the entity is asked per call AND number.
#[test]
fn a_search_finds_a_contact_by_the_entity_its_record_names() {
    let row = |when: u64, dxcc: Option<u32>| {
        let mut r = parse_adif("<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:4>1200<EOR>").remove(0);
        r.when_unix += when;
        r.dxcc = dxcc;
        r
    };
    let log = [row(0, None), row(60, Some(182))];
    let entity = |q: &QsoRecord| {
        let name = match q.dxcc {
            Some(182) => "Navassa Island",
            _ => "United States",
        };
        Some(name.to_string())
    };
    let query = LogQuery {
        sort: SortKey::Time,
        asc: true,
        search: "navassa".to_string(),
        needs_confirm_only: false,
        then: Vec::new(),
        filter: Default::default(),
    };
    let found: Vec<Option<u32>> = reference::order(&log, &query, &entity)
        .into_iter()
        .map(|i| log[i].dxcc)
        .collect();
    assert_eq!(found, [Some(182)]);
}

/// ★ The statistics' states count a merged contest contact under the state its exchange carried
/// — the one its upload carries and WAS counts — not the one looked up from its call beside it.
#[test]
fn the_statistics_count_a_merged_contacts_exchange_state() {
    let mut merged =
        parse_adif("<CALL:5>K2ABC<QSO_DATE:8>20240101<TIME_ON:4>1200<BAND:3>20m<MODE:2>CW<EOR>")
            .remove(0);
    merged.state = Some("NJ".into());
    merged.contest = Some(Box::new(crate::logbook::ContestFields {
        contest_id: "NAQP-CW".into(),
        adif: vec![("STATE".into(), "NY".into())],
        ..Default::default()
    }));
    let entity = |_: &QsoRecord| Some("United States".to_string());
    let counts = reference::fold(&[merged], LogStatCounter::new(&entity));
    let states: Vec<(&str, usize)> = counts
        .by_state
        .iter()
        .map(|t| (t.label.as_str(), t.count))
        .collect();
    assert_eq!(states, [("NY", 1)]);
}
