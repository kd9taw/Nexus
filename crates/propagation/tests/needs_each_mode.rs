//! "New mode" counts each mode separately (operator ruling, 2026-10-07): FT8, FT4, RTTY, PSK31
//! and the rest are their own modes on the callsign card, the roster, Band Activity, the Needed
//! board and the alerts. The DXCC award totals still count CW / Phone / Digital.
//!
//! The report behind it: C5R (The Gambia) heard calling on 20 m FT8, against a log holding The
//! Gambia on 20 m SSB and 40 m FT4. The card said "New mode-slot", because it compares exact
//! modes; the roster said nothing, because the engine compared mode classes and FT4 had already
//! "worked Digital". Two answers to one question, on one screen.
//!
//! Every assertion here reads `ui/src/features/__fixtures__/mode-keys.json`, the table the UI's
//! `modeKey`, the ADIF reader and the card's engine answer are held to as well. So the engine
//! cannot drift from the card without one of the four going red.

use propagation::model::mode_key;
use propagation::needalert::{score_slots, NeedAlert};
use propagation::{LogNeeds, NeedTag};
use serde_json::Value;

const TABLE: &str = include_str!("../../../ui/src/features/__fixtures__/mode-keys.json");

fn table() -> Value {
    serde_json::from_str(TABLE).expect("mode-keys.json parses")
}

fn rows<'a>(t: &'a Value, list: &str) -> &'a Vec<Value> {
    t[list]
        .as_array()
        .unwrap_or_else(|| panic!("{list} is a list"))
}

fn text<'a>(row: &'a Value, field: &str) -> &'a str {
    row[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} in {row}"))
}

/// The engine's answer for a station heard in `mode`, exactly as the Needed board scores one.
fn heard(needs: &LogNeeds, call: &str, band: &str, mode: &str) -> Option<NeedAlert> {
    score_slots(call, band, mode, None, None, needs, &needs.slots())
}

fn is_new_mode(alert: &Option<NeedAlert>) -> bool {
    alert
        .as_ref()
        .is_some_and(|a| a.tags.contains(&NeedTag::NewMode))
}

/// The fold itself, row by row: a stored mode's key, and a key folds to itself (a station heard
/// in a mode named by its key lands on the same key).
#[test]
fn the_fold_is_the_tables() {
    let t = table();
    for row in rows(&t, "folds").iter().chain(rows(&t, "spellings")) {
        let (stored, key) = (text(row, "stored"), text(row, "key"));
        assert_eq!(mode_key(stored), key, "{row}");
        assert_eq!(mode_key(key), key, "a key folds to itself: {row}");
    }
}

/// THE WORKED SIDE AND THE HEARD SIDE THROUGH ONE FOLD, over every spelling the table holds. A
/// contact logged in one mode answers a station heard in a mode with the SAME key as worked (the
/// chip goes out), and a station heard in any OTHER key as a new mode, even within one class:
/// FT4 does not cover FT8, nor SSB cover FM.
#[test]
fn each_mode_is_worked_by_its_own_contacts_and_by_no_other() {
    let t = table();
    let folds = rows(&t, "folds");
    for logged in folds {
        let mut n = LogNeeds::new();
        // Confirmed and on the band heard, so a mode need is the only thing left to say.
        n.add("C56YK", "20m", text(logged, "stored"), None, None, true);
        for hear in folds {
            for spelling in [text(hear, "stored"), text(hear, "key")] {
                let new_mode = is_new_mode(&heard(&n, "C5R", "20m", spelling));
                assert_eq!(
                    new_mode,
                    text(logged, "key") != text(hear, "key"),
                    "logged {logged}, heard in {spelling:?}",
                );
            }
        }
    }
}

/// The scenarios the card is held to by the UI's own test of this table: the engine's row for the
/// heard station, field by field, or no row at all.
#[test]
fn each_scenario_is_answered_as_the_table_says() {
    let t = table();
    for s in rows(&t, "scenarios") {
        let name = text(s, "name");
        let mut n = LogNeeds::new();
        for q in s["log"].as_array().expect("log") {
            let confirmed = q["confirmed"].as_bool().expect("confirmed");
            n.add(
                text(q, "call"),
                text(q, "band"),
                text(q, "mode"),
                None,
                None,
                confirmed,
            );
            assert_eq!(
                propagation::dxcc::resolve(text(q, "call")).map(|i| i.entity),
                Some(text(s, "entity")),
                "{name}: the table's entity is the one cty.dat resolves",
            );
        }
        let h = &s["heard"];
        let alert = heard(&n, text(h, "call"), text(h, "band"), text(h, "mode"));
        match s["alert"].as_object() {
            None => assert!(alert.is_none(), "{name}: expected no row, got {alert:?}"),
            Some(want) => {
                let row = alert
                    .as_ref()
                    .unwrap_or_else(|| panic!("{name}: expected a row, got none"));
                let got = serde_json::to_value(row).expect("serializes");
                for (field, value) in want {
                    assert_eq!(&got[field], value, "{name}: {field} of {got}");
                }
            }
        }
        // The card's verdict and the engine's are the same verdict.
        assert_eq!(
            is_new_mode(&alert),
            s["cardNewMode"].as_bool().expect("cardNewMode"),
            "{name}"
        );
    }
}

/// Work it, and the chip goes out: the C5R case, then the FT8 contact logged the way the FT8 tier
/// logs it ("FT8") against the station heard the way the radio's own decodes name it ("FT8").
#[test]
fn logging_the_mode_clears_the_need() {
    let mut n = LogNeeds::new();
    n.add("C56YK", "20m", "SSB", None, None, true);
    n.add("C5AAA", "40m", "FT4", None, None, false);
    let before = heard(&n, "C5R", "20m", "FT8");
    assert!(is_new_mode(&before), "FT8 never worked: {before:?}");
    n.add("C5R", "20m", "FT8", None, None, false);
    let after = heard(&n, "C5R", "20m", "FT8");
    assert!(
        after.is_none(),
        "worked on FT8 now, nothing left: {after:?}"
    );
}

/// A station placed only in a CLASS — a cluster spot by its frequency, "Digital" or "Phone" — keeps
/// the class rule: the log can only say it is new when no mode of that class was ever worked.
#[test]
fn a_station_heard_only_by_class_is_judged_by_class() {
    let mut n = LogNeeds::new();
    n.add("C56YK", "20m", "SSB", None, None, true);
    n.add("C5AAA", "40m", "FT4", None, None, false);
    for class in ["Digital", "Phone", ""] {
        let a = heard(&n, "C5R", "20m", class);
        assert!(
            !is_new_mode(&a),
            "{class:?}: a mode of that class is worked: {a:?}"
        );
    }

    let mut voice_only = LogNeeds::new();
    voice_only.add("C56YK", "20m", "SSB", None, None, true);
    let a = heard(&voice_only, "C5R", "20m", "Digital").expect("no digital mode ever worked");
    assert_eq!(a.tags, vec![NeedTag::NewMode]);
    assert_eq!(a.headline, "New mode — Digital The Gambia (any band)");
    let json = serde_json::to_value(&a).expect("serializes");
    assert!(
        json["exactMode"].is_null(),
        "a class names no exact mode: {json}"
    );
}
