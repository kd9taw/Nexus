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
use propagation::{
    activation_alert, Band, DxpeditionPlan, DxpeditionTracker, LogNeeds, NeedKind, NeedTag,
    OtaSpot, PropAdvisor, SpaceWx,
};
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

/// The log of the report: The Gambia on 20 m SSB, confirmed, and on 40 m FT4.
fn the_gambia_on_ssb_and_ft4() -> LogNeeds {
    let mut n = LogNeeds::new();
    n.add("C56YK", "20m", "SSB", None, None, true);
    n.add("C5AAA", "40m", "FT4", None, None, false);
    n
}

/// C5R on the hunter feed at a park, on 20 m, in `mode` as the feed spells it.
fn activator(mode: &str) -> OtaSpot {
    OtaSpot {
        program: "POTA".into(),
        reference: "C5-0001".into(),
        name: String::new(),
        activator: "C5R".into(),
        freq_khz: 14_074.0,
        mode: mode.into(),
        spotter: None,
        comment: None,
        grid: None,
        lat: None,
        lon: None,
        spot_time_unix: Some(1_780_000_000),
        states: Vec::new(),
    }
}

/// The activator's own row on the Needed board (the hunter feed's, not a cluster spot's).
fn activation_row(needs: &LogNeeds, mode: &str) -> NeedAlert {
    activation_alert(&activator(mode), needs, &needs.slots(), false, true)
        .expect("an active park on a band is always a row")
}

/// A POTA or SOTA ACTIVATOR'S OWN ROW judges the mode its feed names, as every other row does. It
/// read the feed's mode as a class, so C5R activating in FT8 against a log with The Gambia on FT4
/// said nothing, while the same station decoded by the radio said "new mode". The row's mode label
/// stays the class: the board drops an activation row that repeats a cluster row by call, band and
/// that label.
#[test]
fn an_activators_own_row_judges_the_mode_its_feed_names() {
    let n = the_gambia_on_ssb_and_ft4();
    let row = activation_row(&n, "FT8");
    assert!(row.tags.contains(&NeedTag::NewMode), "{row:?}");
    assert_eq!(
        row.headline,
        "New mode — FT8 The Gambia (any band) · POTA C5-0001"
    );
    assert_eq!(row.mode, "Digital", "the row's label is still the class");
    let json = serde_json::to_value(&row).expect("serializes");
    assert_eq!(json["exactMode"], "FT8", "{json}");

    // FT4 is worked (on 40 m) and so is SSB, in either sideband's spelling.
    for worked in ["FT4", "SSB", "USB", "LSB"] {
        let row = activation_row(&n, worked);
        assert!(!row.tags.contains(&NeedTag::NewMode), "{worked}: {row:?}");
    }

    // Work C5R in FT8 and the row says nothing new.
    let mut n = the_gambia_on_ssb_and_ft4();
    n.add("C5R", "20m", "FT8", None, None, false);
    let row = activation_row(&n, "FT8");
    assert!(!row.tags.contains(&NeedTag::NewMode), "{row:?}");
}

/// A feed mode that names a class, a family or nothing keeps the class rule, like a cluster spot
/// placed by its frequency. Judged as a mode of its own, `DATA` or a bare `MFSK` could never be
/// worked, so the need would never clear.
#[test]
fn an_activator_whose_feed_names_only_a_class_is_judged_by_class() {
    let n = the_gambia_on_ssb_and_ft4();
    for token in [
        "", "DATA", "DIGI", "OTHER", "MFSK", "PSK", "PH", "PHONE", "DV",
    ] {
        let row = activation_row(&n, token);
        assert!(
            !row.tags.contains(&NeedTag::NewMode),
            "{token:?}: a mode of its class is worked: {row:?}"
        );
    }

    let mut voice_only = LogNeeds::new();
    voice_only.add("C56YK", "20m", "SSB", None, None, true);
    let row = activation_row(&voice_only, "DATA");
    assert!(row.tags.contains(&NeedTag::NewMode), "{row:?}");
    assert_eq!(
        row.headline,
        "New mode — Digital The Gambia (any band) · POTA C5-0001"
    );
    let json = serde_json::to_value(&row).expect("serializes");
    assert!(json["exactMode"].is_null(), "a class names no mode: {json}");
}

const NOW: i64 = 1_780_000_000;

/// The need on C5R's 20 m work-now card, for an operation that announced `modes`, or `None` when
/// the card is not shown (nothing is needed).
fn card_need(needs: &LogNeeds, modes: &[&str]) -> Option<NeedKind> {
    let plan = DxpeditionPlan {
        call: "C5R".into(),
        entity: "The Gambia".into(),
        grid: Some("IK13".into()),
        start_unix: NOW - 100,
        end_unix: NOW + 100,
        bands: vec![Band::B20],
        modes: modes.iter().map(|m| m.to_string()).collect(),
        ft8_mode: None,
        most_wanted_rank: None,
        website: None,
    };
    let wx = SpaceWx::default();
    let advisory = PropAdvisor::new("KD9TAW", "EN52").advise(NOW, &[], &wx);
    let dash = DxpeditionTracker::new("EN52").dashboard(NOW, &[plan], needs, &advisory, &wx);
    assert_eq!(dash.active, ["C5R"], "control: the operation is on the air");
    dash.workable_now.first().map(|c| c.need)
}

/// A DXPEDITION'S WORK-NOW CARD judges each mode the operation announced. It judged "Digital"
/// whatever the operation announced, so C5R on FT8 against The Gambia on FT4 showed no card.
#[test]
fn a_dxpedition_card_judges_each_mode_it_announced() {
    let n = the_gambia_on_ssb_and_ft4();
    assert_eq!(card_need(&n, &["FT8"]), Some(NeedKind::NewMode));
    // Two of three announced modes are new: the card is a new mode until each one is worked.
    assert_eq!(
        card_need(&n, &["CW", "SSB", "FT8"]),
        Some(NeedKind::NewMode)
    );
    // Every announced mode worked, and 20 m confirmed: nothing to show.
    assert_eq!(card_need(&n, &["SSB"]), None);
    assert_eq!(card_need(&n, &["FT4"]), None);
    assert_eq!(card_need(&n, &["SSB", "FT4"]), None);

    let mut n = the_gambia_on_ssb_and_ft4();
    n.add("C5R", "20m", "FT8", None, None, false);
    assert_eq!(card_need(&n, &["FT8"]), None, "worked on FT8 now");
}

/// An operation that announced no mode, or only "Digital" or the PSK family, keeps the class
/// rule: its card is a new mode only when no digital mode was ever worked with the entity.
#[test]
fn a_dxpedition_that_names_no_mode_is_judged_by_class() {
    let n = the_gambia_on_ssb_and_ft4();
    for modes in [&[][..], &["Digital"], &["PSK"]] {
        assert_eq!(card_need(&n, modes), None, "{modes:?}: FT4 is worked");
    }
    let mut voice_only = LogNeeds::new();
    voice_only.add("C56YK", "20m", "SSB", None, None, true);
    assert_eq!(card_need(&voice_only, &[]), Some(NeedKind::NewMode));
}

/// A PSK31 STATION HEARD AFTER AN FLDIGI PSK31 IMPORT IS NOT A NEW MODE. fldigi logs ADIF 3's
/// `MODE=PSK SUBMODE=PSK31`, and the reader kept only the PSK, so the board read every PSK31
/// station as a new mode against a log full of PSK31 contacts. Read through the real reader, as
/// the log is, so the import and the board cannot disagree.
#[cfg(feature = "live")]
#[test]
fn a_psk31_station_heard_after_an_fldigi_psk31_import_is_not_a_new_mode() {
    let adif = "<CALL:5>C56YK<QSO_DATE:8>20260701<TIME_ON:6>012345<BAND:3>20m\
                <MODE:3>PSK<SUBMODE:5>PSK31<EOR>";
    let mut n = LogNeeds::new();
    for q in tempo_core::logbook::parse_adif(adif) {
        n.add(&q.call, &q.band, &q.mode, None, None, true);
    }
    let a = heard(&n, "C5R", "20m", "PSK31");
    assert!(!is_new_mode(&a), "worked on PSK31: {a:?}");
    let other = heard(&n, "C5R", "20m", "PSK63");
    assert!(
        is_new_mode(&other),
        "control: PSK63 is still new: {other:?}"
    );
}
