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

/// The questions C17a answers in its second part, once C14's folds are in: the golden file
/// holds them, and the phase-1 engine does not answer them yet.
pub(crate) const LATER: [&str; 5] = [
    "workedGrids",
    "gridPoints",
    "bandsInLog",
    "statistics",
    "lotwBacklog",
];

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
    match q["kind"].as_str().expect("kind") {
        "page" => {
            let order = reference::order(log, &query());
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
            let order = reference::order(log, &query());
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
        other => panic!("no reference answer for {other}"),
    }
}

/// ★ THE PORT ANSWERS WHAT THE UI ANSWERED. Every golden question C17a answers now, asked of the
/// reference over the golden log, gets exactly the frozen answer — and the questions it does not
/// answer yet are exactly C14's folds, so the count cannot shrink by a question dropped quietly.
#[test]
fn the_reference_gives_every_golden_answer() {
    let (log, entities) = golden_log();
    assert_eq!(log.len(), 24, "the fixture's 24 rows");
    let mut answered = 0;
    let mut later = std::collections::BTreeMap::<String, usize>::new();
    for (q, a) in golden_answers() {
        let kind = q["kind"].as_str().expect("kind").to_string();
        if LATER.contains(&kind.as_str()) {
            *later.entry(kind).or_default() += 1;
            continue;
        }
        assert_eq!(reference_answer(&log, &entities, &q), a, "{q}");
        answered += 1;
    }
    assert_eq!(answered, 71, "every phase-1 question in the golden file");
    assert_eq!(
        later.into_iter().collect::<Vec<_>>(),
        [
            ("bandsInLog".to_string(), 1),
            ("gridPoints".to_string(), 3),
            ("lotwBacklog".to_string(), 1),
            ("statistics".to_string(), 1),
            ("workedGrids".to_string(), 1),
        ]
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
    };
    assert_eq!(reference::order(&log, &q("z")).len(), 24);
    let ssb: Vec<String> = reference::order(&log, &q("ssb"))
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
            },
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
