//! The activation export read from the logbook store, held to the code before SPEC-2 v3 C18 moved
//! it: that code VERBATIM (`old_respond`), over the log in memory, beside the store that log
//! mirrors — and on the 1.13 path, where the log in memory is all there is. Byte for byte: the
//! list, every chunk of every file, and every refusal.
use super::*;
use crate::remote_service::query::log_tests::{
    edit_at, launch, memory, random_change, record_at, settle, Dir, Gen, CALLS,
};
use tempo_app::engine::Engine;
use tempo_core::logbook::{Logbook, LoggedActivation};

// ── the code before C18, VERBATIM ────────────────────────────────────────────────────────────

/// The two Engine reads the code below called, as they stood: `StationCore`'s
/// `self.logbook.activations()` and `self.logbook.adif_for_activation(..)`, over the log in
/// memory. `get_log` is that log's records in log order and `Logbook::from_store` holds them as
/// given, so each is the same function of the same records.
trait Before {
    fn log_activations(&self) -> Vec<LoggedActivation>;
    fn export_logbook_for_activation(
        &self,
        reference: &str,
        day_start_unix: u64,
        callsign: Option<&str>,
    ) -> String;
}

impl Before for Engine {
    fn log_activations(&self) -> Vec<LoggedActivation> {
        Logbook::from_store(self.get_log()).activations()
    }
    fn export_logbook_for_activation(
        &self,
        reference: &str,
        day_start_unix: u64,
        callsign: Option<&str>,
    ) -> String {
        Logbook::from_store(self.get_log()).adif_for_activation(reference, day_start_unix, callsign)
    }
}

/// The reply to one read: the list (no selection, chunk 0 only), one chunk of one listed
/// activation's file, or a refusal that names why nothing was sent.
fn old_respond(
    engine: &Engine,
    selection: Option<&Selection>,
    index: u32,
) -> Result<Value, &'static str> {
    if selection.is_some_and(|s| !s.valid()) || (selection.is_none() && index != 0) {
        return Err("invalidRequest");
    }
    // Only activations a browser can name back. A reference imported with other bytes is left out
    // rather than offered and then refused.
    let listed: Vec<LoggedActivationDto> = engine
        .log_activations()
        .into_iter()
        .map(LoggedActivationDto::from)
        .filter(|a| {
            Selection {
                reference: a.reference.clone(),
                day_start_unix: a.day_start_unix,
                callsign: a.callsign.clone(),
            }
            .valid()
        })
        .collect();
    let Some(selection) = selection else {
        return Ok(
            json!({"operation":"activationExport","activations":&listed[..listed.len().min(LISTED)]}),
        );
    };
    if !listed.iter().any(|a| selection.names(a)) {
        return Ok(json!({"operation":"activationExport","refused":"notFound"}));
    }
    let text = engine.export_logbook_for_activation(
        &selection.reference,
        selection.day_start_unix,
        selection.callsign.as_deref(),
    );
    chunked("activationExport", &text, index)
}

// ── logs made of activations ─────────────────────────────────────────────────────────────────

/// 2026-09-09 00:00:00 UTC, the first of the three days the contacts below are made on.
const DAY: u64 = 1_788_912_000;

/// One contact: on one of three UTC days, a fifth of them ON a midnight or the second before one;
/// worked under a station callsign, an operator, both (the station's wins), a callsign a browser
/// cannot name back, or none; from one park of many, a two-fer written with either separator, a
/// summit, a WWFF reference, a reference a browser cannot name back, or from home. Notes outside
/// ASCII, a long comment that makes a file span chunks, and a passthrough tag only a whole record
/// carries.
fn contact(g: &mut Gen) -> String {
    let mut f = String::new();
    let mut tag = |name: &str, val: &str| f.push_str(&format!("<{name}:{}>{val}", val.len()));
    tag("CALL", g.pick(CALLS));
    let when = match g.below(5) {
        0 => DAY + 86_400 * g.below(3) as u64,
        1 => DAY + 86_400 * (1 + g.below(2) as u64) - 1,
        _ => DAY + g.below(3 * 86_400) as u64,
    };
    let (y, m, d, hh, mm, ss) = tempo_core::logbook::datetime_utc(when);
    tag("QSO_DATE", &format!("{y:04}{m:02}{d:02}"));
    tag("TIME_ON", &format!("{hh:02}{mm:02}{ss:02}"));
    tag("BAND", g.pick(&["20m", "40m", "2m"]));
    tag("MODE", g.pick(&["SSB", "CW", "FT8"]));
    match g.below(6) {
        0 => tag("STATION_CALLSIGN", "KD9TAW"),
        1 => tag("OPERATOR", "W1ABC"),
        2 => {
            tag("STATION_CALLSIGN", "N0CLUB");
            tag("OPERATOR", "W1ABC");
        }
        3 => tag("STATION_CALLSIGN", "KD9TAW-1"),
        4 => {}
        _ => tag("STATION_CALLSIGN", "KD9TAW/P"),
    }
    match g.below(10) {
        0 | 1 => {
            tag("MY_SIG", "POTA");
            tag("MY_SIG_INFO", "US-1234");
        }
        2 => {
            tag("MY_SIG", "POTA");
            tag("MY_SIG_INFO", "US-1234,US-5678");
        }
        3 => tag("MY_POTA_REF", "US-5678; US-1234"),
        4 => tag("MY_SOTA_REF", "W9/WI-001"),
        5 => {
            tag("MY_SIG", "WWFF");
            tag("MY_SIG_INFO", "KFF-1234");
        }
        6 => {
            tag("MY_SIG", "POTA");
            tag("MY_SIG_INFO", g.pick(&["US_1234", "US-1234-ÖÖ"]));
        }
        7 => {
            let park = format!("US-{:04}", g.below(60));
            tag("MY_SIG", "POTA");
            tag("MY_SIG_INFO", &park);
        }
        _ => {}
    }
    if g.chance(3) {
        tag("NOTES", "Größe: 5 W, dipole");
    }
    if g.chance(8) {
        tag("COMMENT", &"long comment ".repeat(200));
    }
    if g.chance(5) {
        tag("APP_OTHER_LOGGER", "xyz");
    }
    f.push_str("<EOR>\n");
    f
}

/// `n` contacts of [`contact`]'s kinds, seeded.
fn activation_log(n: usize, seed: u64) -> String {
    let mut g = Gen(seed | 1);
    let mut s = tempo_core::logbook::adif_header();
    for _ in 0..n {
        s.push_str(&contact(&mut g));
    }
    s
}

/// One change to what an activation is made of, written as an edit may leave it rather than as
/// an import cleans it: the reference in lower case, padded, or a two-fer with an empty part; the
/// callsign in lower case and padded; the program; the time, onto a midnight or the second
/// before one.
fn activation_change(e: &crate::SharedEngine, g: &mut Gen) {
    let mut eng = e.lock().unwrap();
    let len = eng.log_records().len();
    if len == 0 {
        return;
    }
    let at = g.below(len);
    let mut r = record_at(&eng, at);
    match g.below(5) {
        0 => {
            let reference = g.pick(&[
                " us-1234 ",
                "us-1234,US-5678",
                "US-5678;;",
                " ; ",
                "w9/wi-001",
            ]);
            r.ota.my_ref = Some(reference.into());
        }
        1 => r.station_callsign = Some(g.pick(&[" kd9taw ", "n0club", "KD9TAW"]).into()),
        2 => r.operator = Some(g.pick(&["w1abc ", "K9OP", "  "]).into()),
        3 => r.ota.my_program = Some(g.pick(&["pota", "SOTA", "", " wwff "]).into()),
        _ => r.when_unix = DAY + 86_400 * (1 + g.below(2) as u64) - g.below(2) as u64,
    }
    edit_at(&mut eng, at, r);
}

// ── every answer, the old code's and the new code's ──────────────────────────────────────────

/// The old code's answer, over the log in memory under the Engine lock, as it ran.
fn old(
    e: &crate::SharedEngine,
    selection: Option<&Selection>,
    index: u32,
) -> Result<String, &'static str> {
    old_respond(&e.lock().unwrap(), selection, index).map(|v| v.to_string())
}

/// The new code's answer: the log's rows under the Engine lock, the read after it is released.
fn new(
    e: &crate::SharedEngine,
    selection: Option<&Selection>,
    index: u32,
) -> Result<String, &'static str> {
    let rows = e.lock().unwrap().log_rows();
    respond(&rows, selection, index).map(|v| v.to_string())
}

/// Every answer a browser can be given, compared byte for byte: the list, and the list asked for
/// a chunk; for every activation the log holds, listed or not, every chunk of its file and one
/// past the last; and with `near`, for every fifth a near miss — the next day, the other
/// callsign, the reference in lower case — and a park no contact names. How many of the answers
/// were a chunk of a file, and how many a refusal.
fn assert_answers_are_the_old_answers(
    e: &crate::SharedEngine,
    near: bool,
    what: &str,
) -> (usize, usize) {
    let (mut files, mut refusals) = (0, 0);
    for index in [0, 1] {
        assert_eq!(
            new(e, None, index),
            old(e, None, index),
            "{what}: the list, chunk {index}"
        );
    }
    let held = e.lock().unwrap().log_activations();
    let mut asks = Vec::new();
    for (i, a) in held.iter().enumerate() {
        asks.push(
            json!({"reference":a.reference,"dayStartUnix":a.day_start_unix,"callsign":a.callsign}),
        );
        if near && i % 5 == 0 {
            let other = if a.callsign.is_some() {
                Value::Null
            } else {
                json!("KD9TAW")
            };
            asks.push(json!({"reference":a.reference,"dayStartUnix":a.day_start_unix + 86_400,"callsign":a.callsign}));
            asks.push(
                json!({"reference":a.reference,"dayStartUnix":a.day_start_unix,"callsign":other}),
            );
            asks.push(json!({"reference":a.reference.to_lowercase(),"dayStartUnix":a.day_start_unix,"callsign":a.callsign}));
        }
    }
    if near {
        asks.push(json!({"reference":"US-9999","dayStartUnix":DAY,"callsign":"KD9TAW"}));
    }
    for ask in asks {
        let selection: Selection = serde_json::from_value(ask.clone()).unwrap();
        let first = old(e, Some(&selection), 0);
        let chunks = first
            .as_ref()
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|v| v["file"]["chunks"].as_u64())
            .unwrap_or(0);
        for index in 0..=chunks as u32 {
            let answer = new(e, Some(&selection), index);
            let before = match index {
                0 => first.clone(),
                _ => old(e, Some(&selection), index),
            };
            assert!(
                answer == before,
                "{what}: {ask}, chunk {index}\nnew: {answer:.300?}\nold: {before:.300?}"
            );
            match answer {
                Ok(a) if a.contains("\"base64\"") => files += 1,
                Ok(a) if !a.contains("\"refused\"") => panic!("{what}: {ask}: {a:.300}"),
                _ => refusals += 1,
            }
        }
    }
    (files, refusals)
}

/// ★ PARITY: over 3,000 contacts, on the store and on the 1.13 path, every answer is byte for byte
/// the old code's — the list past its bound, every file and every refusal. Premises first: the log
/// really holds what the answers must get right.
#[test]
fn every_answer_read_from_the_store_is_the_old_answer() {
    let text = activation_log(3_000, 0x0C18_A2E1);
    let d = Dir::new("activation-parity");
    std::fs::write(d.log(), &text).unwrap();
    let store = launch(&d);
    let held = store.lock().unwrap().log_activations();
    fn named(a: &LoggedActivation) -> bool {
        Selection {
            reference: a.reference.clone(),
            day_start_unix: a.day_start_unix,
            callsign: a.callsign.clone(),
        }
        .valid()
    }
    let listed = held.iter().filter(|a| named(a)).count();
    assert!(
        listed > LISTED,
        "premise: the list is cut at its bound: {listed}"
    );
    assert!(
        listed < held.len(),
        "premise: some activations cannot be named back"
    );
    for callsign in [None, Some("W1ABC"), Some("N0CLUB"), Some("KD9TAW/P")] {
        assert!(
            held.iter().any(|a| a.callsign.as_deref() == callsign),
            "premise: an activation worked under {callsign:?}"
        );
    }
    for (arm, e) in [("the store", &store), ("the 1.13 path", &memory(&d, &text))] {
        let (files, refusals) = assert_answers_are_the_old_answers(e, true, arm);
        assert!(
            files > 100 && refusals > 100,
            "{arm}: premise: files and refusals compared: {files}, {refusals}"
        );
    }
    let spans = |e: &crate::SharedEngine| {
        held.iter().filter(|a| named(a)).any(|a| {
            let s = Selection {
                reference: a.reference.clone(),
                day_start_unix: a.day_start_unix,
                callsign: a.callsign.clone(),
            };
            new(e, Some(&s), 1).is_ok()
        })
    };
    assert!(spans(&store), "premise: a file spans chunks");
    settle(&store);
}

/// ★ A file of many chunks, and one over the bound, read from the store: each the old answer. The
/// premise is checked on the old code, so the bound is really crossed.
#[test]
fn a_file_of_many_chunks_and_one_over_the_bound_are_the_old_answers() {
    let d = Dir::new("activation-big");
    let mut text = tempo_core::logbook::adif_header();
    let note = "x".repeat(1_000);
    for (day, n) in [(DAY, 1_100), (DAY + 86_400, 150)] {
        for i in 0..n {
            let (y, m, dd, hh, mm, ss) = tempo_core::logbook::datetime_utc(day + i as u64 * 60);
            text.push_str(&format!(
                "<CALL:6>K{:05}<QSO_DATE:8>{y:04}{m:02}{dd:02}<TIME_ON:6>{hh:02}{mm:02}{ss:02}\
                 <BAND:3>20m<MODE:3>SSB<STATION_CALLSIGN:6>KD9TAW<MY_SIG:4>POTA\
                 <MY_SIG_INFO:7>US-1234<COMMENT:1000>{note}<EOR>\n",
                i
            ));
        }
    }
    std::fs::write(d.log(), &text).unwrap();
    let store = launch(&d);
    let ask = |day: u64| -> Selection {
        serde_json::from_value(
            json!({"reference":"US-1234","dayStartUnix":day,"callsign":"KD9TAW"}),
        )
        .unwrap()
    };
    assert_eq!(
        old(&store, Some(&ask(DAY)), 0),
        Ok(json!({"operation":"activationExport","refused":"tooLarge"}).to_string()),
        "premise: the first day's file is over the bound"
    );
    let chunks = serde_json::from_str::<Value>(&old(&store, Some(&ask(DAY + 86_400)), 0).unwrap())
        .unwrap()["file"]["chunks"]
        .as_u64()
        .unwrap();
    assert!(
        chunks >= 4,
        "premise: the second day's file spans chunks: {chunks}"
    );
    let (files, _) = assert_answers_are_the_old_answers(&store, false, "big");
    assert_eq!(
        files as u64, chunks,
        "every chunk of the second day's file was compared"
    );
    settle(&store);
}

/// ★ THE PROPERTY: after every one of 24 changes to each of six seeded logs — the kinds of change
/// the app makes, and activation fields as an edit may leave them — every answer read from the
/// store is the old answer. Premise: answers were compared while the log held a reference and a
/// callsign as an edit left them, and an activation no contact names a program for.
#[test]
fn after_every_change_every_answer_is_the_old_answer() {
    let raw = |m: Option<&str>| m.is_some_and(|m| m != m.trim().to_ascii_uppercase());
    let (mut raw_refs, mut raw_calls, mut no_program) = (0, 0, 0);
    for seed in 1..=6u64 {
        let d = Dir::new(&format!("activation-prop-{seed}"));
        std::fs::write(d.log(), activation_log(150, seed * 7_919)).unwrap();
        let e = launch(&d);
        let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for step in 0..24u64 {
            if g.chance(2) {
                random_change(&e, &mut g, step);
            } else {
                activation_change(&e, &mut g);
            }
            assert_answers_are_the_old_answers(&e, false, &format!("seed {seed}, step {step}"));
            let (log, held) = {
                let eng = e.lock().unwrap();
                (eng.get_log(), eng.log_activations())
            };
            raw_refs += usize::from(log.iter().any(|r| raw(r.ota.my_ref.as_deref())));
            raw_calls += usize::from(log.iter().any(|r| raw(r.station_callsign.as_deref())));
            no_program += usize::from(held.iter().any(|a| a.program.is_none()));
        }
        settle(&e);
    }
    assert!(
        raw_refs > 0 && raw_calls > 0 && no_program > 0,
        "premise: {raw_refs}, {raw_calls}, {no_program}"
    );
}

/// Every word this read can refuse with is one a page accepts in an operation reply: the page's
/// list is closed (`OPERATION_ERRORS`), and a word outside it fails the whole reply. Both
/// directions: the picture's own words are NOT on it, so the check can say no.
#[test]
fn a_refused_read_answers_in_words_the_page_accepts() {
    let page = include_str!("../../../../ui/src/remote-web/operation-protocol.ts");
    let list = page
        .split("export const OPERATION_ERRORS = [")
        .nth(1)
        .and_then(|rest| rest.split("] as const").next())
        .expect("the page's operation errors");
    let accepted = |word: &str| list.contains(&format!("'{word}'"));
    for (refused, word) in [
        ("applicationBusy", "stationBusy"),
        ("applicationUnavailable", "stationUnavailable"),
    ] {
        assert!(
            !accepted(refused),
            "control: {refused} is not a word the page accepts"
        );
        assert_eq!(in_operation_words(refused), word);
        assert!(accepted(word), "{word} is a word the page accepts");
    }
    assert!(accepted("invalidRequest"), "and the read's own refusal");
}
