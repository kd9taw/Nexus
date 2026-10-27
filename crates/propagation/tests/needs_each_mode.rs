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
    activation_alert, Band, DxpeditionPlan, DxpeditionTracker, LogNeeds, ModeClass, NeedKind,
    NeedTag, OtaSpot, PropAdvisor, SpaceWx,
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

/// The keys the table holds that name no one mode a contact is logged in: a family (a bare MFSK, a
/// bare PSK), N3FJP's generic phone token, and a beacon (FST4W, which nobody works). A contact may
/// be STORED as one, and then that key is what it worked; a station HEARD as one is judged by its
/// class (`a_station_heard_in_a_mode_no_contact_is_logged_in_is_judged_by_its_class`).
const HEARD_BY_CLASS: [&str; 4] = ["MFSK", "PSK", "PH", "FST4W"];

/// THE WORKED SIDE AND THE HEARD SIDE THROUGH ONE FOLD, over every spelling the table holds. A
/// contact logged in one mode answers a station heard in a mode with the SAME key as worked (the
/// chip goes out), and a station heard in any OTHER key as a new mode, even within one class:
/// FT4 does not cover FT8, nor SSB cover FM. A station heard as a key in [`HEARD_BY_CLASS`] is new
/// only when the contact is of another class.
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
                let want = if HEARD_BY_CLASS.contains(&text(hear, "key")) {
                    ModeClass::from_adif(text(logged, "key")) != ModeClass::from_adif(spelling)
                } else {
                    text(logged, "key") != text(hear, "key")
                };
                assert_eq!(new_mode, want, "logged {logged}, heard in {spelling:?}");
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

/// A STATION HEARD IN A MODE NO CONTACT IS LOGGED IN IS JUDGED BY ITS CLASS. PSK Reporter names a
/// station's mode in the words of the program that reported it; one capture of reports near EN52
/// (2026-10-12) named FT8, WSPR, FT4, CW, VARAC, JS8, FT2, SAFECOM, PSK31, JTTY, JT65 and FREEDV.
/// Nobody works a WSPR beacon, and no contact is logged as VARAC, FREEDV or SAFECOM, so judged as
/// modes of their own each one was a "new mode" no contact could ever clear: the 1.17.1 test
/// builds listed every WSPR beacon in the United States and Canada as one, against a log holding
/// hundreds of FT8 contacts with both. The radio's own WSPR and FST4W decodes did the same. Each
/// mode a contact IS logged in stays a mode of its own — JTTY among them since JTTY contacts are
/// logged (as `MODE=MFSK SUBMODE=JTTY`, and read from an older log's bare `MODE=JTTY`).
#[test]
fn a_station_heard_in_a_mode_no_contact_is_logged_in_is_judged_by_its_class() {
    let mut n = LogNeeds::new();
    // The United States on 20 m, in FT8 and on SSB, both confirmed; K1XYZ is heard in its zone.
    n.add("K1ABC", "20m", "FT8", None, None, true);
    n.add("W1AW", "20m", "SSB", None, None, true);
    for token in [
        "WSPR", "FST4W", "VARAC", "FREEDV", "SAFECOM", "OTHER", "MFSK", "PSK",
    ] {
        let a = heard(&n, "K1XYZ", "20m", token);
        assert!(
            !is_new_mode(&a),
            "{token:?}: a mode of its class is worked: {a:?}"
        );
    }
    for mode in [
        "JS8",
        "FT4",
        "FT2",
        "Q65",
        "FST4",
        "CW",
        "RTTY",
        "PSK31",
        "PSK125",
        "QPSK31",
        "MSK144",
        "JT65",
        "JTTY",
        "AM",
        "FM",
        "TempoFast",
    ] {
        let a = heard(&n, "K1XYZ", "20m", mode).unwrap_or_else(|| panic!("{mode}: no row"));
        assert_eq!(a.tags, vec![NeedTag::NewMode], "{mode}");
        let json = serde_json::to_value(&a).expect("serializes");
        assert_eq!(json["exactMode"], mode_key(mode), "{mode}: {json}");
    }

    // No digital contact at all: the beacon is the class need, and the row names the class.
    let mut voice_only = LogNeeds::new();
    voice_only.add("W1AW", "20m", "SSB", None, None, true);
    let a = heard(&voice_only, "K1XYZ", "20m", "WSPR").expect("no digital mode ever worked");
    assert_eq!(a.tags, vec![NeedTag::NewMode]);
    assert_eq!(a.headline, "New mode — Digital United States (any band)");
    let json = serde_json::to_value(&a).expect("serializes");
    assert!(json["exactMode"].is_null(), "WSPR names no mode: {json}");
}

/// ADIF fields as one record, then `<EOR>`.
#[cfg(feature = "live")]
fn adif_record(fields: &[(&str, &str)]) -> String {
    let mut r: String = fields
        .iter()
        .map(|(name, value)| format!("<{name}:{}>{value}", value.len()))
        .collect();
    r.push_str("<EOR>\n");
    r
}

/// The calls of the report's log: `n` United States calls (K, W, N, and AA to AK; AH is mostly
/// Pacific islands and AL is Alaska, entities of their own) and `n / 2` Canadian ones (VE, VA),
/// across every call area.
#[cfg(feature = "live")]
fn us_and_canada(n: usize) -> (Vec<String>, Vec<String>) {
    let letters = |i: usize| -> String {
        [i / 676 % 26, i / 26 % 26, i % 26]
            .iter()
            .map(|&k| (b'A' + k as u8) as char)
            .collect()
    };
    let us_prefixes = [
        "K", "W", "N", "AA", "AB", "AC", "AD", "AE", "AF", "AG", "AI", "AJ", "AK",
    ];
    let us = (0..n)
        .map(|i| format!("{}{}{}", us_prefixes[i % 13], i % 10, letters(i)))
        .collect();
    let ve = (0..n / 2)
        .map(|i| format!("{}{}{}", ["VE", "VA"][i % 2], 1 + i % 7, letters(i)))
        .collect();
    (us, ve)
}

/// THE REPORT'S LOG, as the Logbook holds it: hundreds of FT8 contacts with the United States and
/// Canada as Nexus writes them, FT4 as Nexus writes it (MODE=MFSK, SUBMODE=FT4), the same two
/// countries on SSB, and again as imports bring them: a LoTW report, a QRZ logbook download, a
/// Club Log export, and another program's ADIF that spells a digital contact DATA, DIGITAL or a
/// bare MFSK. Germany is worked on CW alone and Japan in FT8: the disagreeing data.
#[cfg(feature = "live")]
fn the_reports_log() -> String {
    let bands = ["80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m"];
    let (us, ve) = us_and_canada(260);
    let mut adif = String::new();
    for (i, call) in us.iter().chain(&ve).enumerate() {
        let date = format!("2026{:02}{:02}", 1 + i % 9, 1 + i % 28);
        let band = bands[i % bands.len()];
        // Nexus writes FT8 as itself, and FT4 as ADIF's MFSK submode; LoTW confirms most of it.
        let mode: &[(&str, &str)] = if i % 9 == 4 {
            &[("MODE", "MFSK"), ("SUBMODE", "FT4")]
        } else {
            &[("MODE", "FT8")]
        };
        let mut fields = vec![("CALL", call.as_str()), ("BAND", band)];
        fields.extend_from_slice(mode);
        fields.extend([
            ("QSO_DATE", date.as_str()),
            ("TIME_ON", "120000"),
            ("LOTW_QSL_RCVD", "Y"),
            ("PROGRAMID", "Nexus"),
        ]);
        adif.push_str(&adif_record(&fields));
    }
    // Phone, as the Phone cockpit logs it.
    for call in ["W1AW", "VE3SSB"] {
        adif.push_str(&adif_record(&[
            ("CALL", call),
            ("BAND", "20m"),
            ("MODE", "SSB"),
            ("SUBMODE", "USB"),
            ("QSO_DATE", "20260410"),
            ("TIME_ON", "150000"),
        ]));
    }
    // A LoTW report row, a QRZ logbook download (lower-case tags), a Club Log export.
    adif.push_str(&adif_record(&[
        ("APP_LoTW_OWNCALL", "KD9TAW"),
        ("STATION_CALLSIGN", "KD9TAW"),
        ("CALL", "W8LTW"),
        ("BAND", "20M"),
        ("FREQ", "14.07600"),
        ("MODE", "FT8"),
        ("APP_LoTW_MODEGROUP", "DATA"),
        ("QSO_DATE", "20260301"),
        ("TIME_ON", "120000"),
        ("QSL_RCVD", "Y"),
        ("QSLRDATE", "20260302"),
    ]));
    adif.push_str(&adif_record(&[
        ("call", "VE3QRZ"),
        ("band", "40m"),
        ("mode", "FT8"),
        ("qso_date", "20260302"),
        ("time_on", "1300"),
        ("app_qrzlog_status", "C"),
    ]));
    adif.push_str(&adif_record(&[
        ("CALL", "VA3CLB"),
        ("QSO_DATE", "20260303"),
        ("TIME_ON", "140000"),
        ("BAND", "30m"),
        ("MODE", "FT8"),
        ("RST_SENT", "-10"),
    ]));
    // Another program's ADIF: a digital contact as DATA, DIGITAL, or a bare MFSK.
    for (call, mode) in [("N4DAT", "DATA"), ("K5DIG", "DIGITAL"), ("VE7MFS", "MFSK")] {
        adif.push_str(&adif_record(&[
            ("CALL", call),
            ("BAND", "20m"),
            ("MODE", mode),
            ("QSO_DATE", "20260304"),
            ("TIME_ON", "160000"),
        ]));
    }
    // Germany on CW alone, Japan in FT8, both confirmed on 20 m.
    for (call, mode) in [("DL1CW", "CW"), ("DL2CW", "CW"), ("JA1FT", "FT8")] {
        adif.push_str(&adif_record(&[
            ("CALL", call),
            ("BAND", "20m"),
            ("MODE", mode),
            ("QSO_DATE", "20260305"),
            ("TIME_ON", "170000"),
            ("LOTW_QSL_RCVD", "Y"),
        ]));
    }
    adif
}

/// One PSK Reporter report on 20 m: `tx` heard in `mode` by `rx` at `rx_grid`, at the board's time.
#[cfg(feature = "live")]
fn pskr_report(tx: &str, mode: &str, rx: &str, rx_grid: &str) -> propagation::PathSpot {
    propagation::PathSpot {
        time: NOW,
        tx_call: tx.into(),
        tx_grid: None,
        rx_call: rx.into(),
        rx_grid: Some(rx_grid.into()),
        band: Band::B20,
        mode: Some(mode.into()),
        snr: None,
        freq_mhz: None,
    }
}

/// THE BOARD OF THE REPORT, by value: the report's log read through the real ADIF reader, the
/// stations PSK Reporter's receivers near EN52 heard on 20 m in each mode that capture named, one
/// the operator's own signal reaches (the getting-out arm), and the radio's own FT8 decodes,
/// scored and ranked as the Needed board ranks them.
#[cfg(feature = "live")]
fn the_reports_board() -> (LogNeeds, Vec<NeedAlert>) {
    use propagation::needalert::{heard_near_me, rank, workable_by_getting_out, Heard};
    let mut needs = LogNeeds::new();
    for q in tempo_core::logbook::parse_adif(&the_reports_log()) {
        let sat = q.prop_mode.as_deref() == Some("SAT");
        needs.add_qso(
            &q.call,
            &q.band,
            &q.mode,
            q.grid.as_deref(),
            q.state.as_deref(),
            q.award_confirmed,
            sat,
            // The report's log carries no DXCC number or CQ zone: each contact counts by its call.
            propagation::RecordFields::default(),
        );
    }
    let mut reports: Vec<propagation::PathSpot> = [
        ("K1ABC", "FT8"),
        ("VE3ABC", "FT8"),
        ("W9WSP", "WSPR"),
        ("VE3WSP", "WSPR"),
        ("N8VAR", "VARAC"),
        ("VA3VAR", "VARAC"),
        ("K4JTY", "JTTY"),
        ("W5FDV", "FREEDV"),
        ("AB1SAF", "SAFECOM"),
        ("K1JSE", "JS8"),
        ("DL1ABC", "FT8"),
        ("JA1ABC", "FT8"),
    ]
    .iter()
    .map(|(tx, mode)| pskr_report(tx, mode, "K9RX", "EN52"))
    .collect();
    // The operator heard in Ohio, and a WSPR beacon there heard only from England.
    reports.push(pskr_report("KD9TAW", "FT8", "W8RCV", "EN81"));
    reports.push(pskr_report("W8WSP", "WSPR", "G4RCV", "IO91"));
    let me = propagation::geo::maidenhead_to_latlon("EN52").expect("EN52");
    let mut heard = heard_near_me(&reports, me);
    heard.extend(workable_by_getting_out(&reports, "KD9TAW"));
    for call in ["K1ABC", "VE3ABC"] {
        heard.push(Heard {
            call: call.into(),
            band: "20m".into(),
            mode: "FT8".into(),
            freq_mhz: None,
            admitted_at: None,
            evidence: Some("decoded by YOUR radio on this band".into()),
            grid: None,
            us_state: None,
        });
    }
    let board = rank(&heard, &needs, &needs.slots());
    (needs, board)
}

/// THE REPORT, BY VALUE: "seeing a ton of ft8 neededs when "mode" is turned on, but I have already
/// gotten those countries on the ft8 mode. Countries like usa and canada where I have hundreds of
/// contacts already". "Mode" is the board's New mode chip, which lists the rows tagged NewMode. On
/// this log it lists three: Germany in FT8 (worked on CW alone), and the United States in JS8 and in
/// JTTY (never worked in either, and contacts are logged in both), and no station of the United
/// States or Canada in FT8, WSPR, VarAC, FreeDV or SAFECOM. The radio's own FT8 decodes of both
/// countries, and its WSPR decodes, are no need.
#[cfg(feature = "live")]
#[test]
fn the_new_mode_rows_are_the_modes_the_log_does_not_hold() {
    let (us, ve) = us_and_canada(260);
    for call in &us {
        let entity = propagation::dxcc::resolve(call).map(|i| i.entity);
        assert_eq!(entity, Some("United States"), "premise: {call}");
    }
    for call in &ve {
        let entity = propagation::dxcc::resolve(call).map(|i| i.entity);
        assert_eq!(entity, Some("Canada"), "premise: {call}");
    }
    let (needs, board) = the_reports_board();
    let new_mode: Vec<String> = board
        .iter()
        .filter(|a| a.tags.contains(&NeedTag::NewMode))
        .map(|a| format!("{} {} {}", a.call, a.band, a.headline))
        .collect();
    assert_eq!(
        new_mode,
        [
            "DL1ABC 20m New mode — FT8 Fed. Rep. of Germany (any band)",
            "K1JSE 20m New mode — JS8 United States (any band)",
            "K4JTY 20m New mode — JTTY United States (any band)",
        ],
        "the rows the New mode chip lists"
    );
    for call in ["K1ABC", "VE3ABC", "JA1ABC"] {
        assert!(
            board.iter().all(|a| a.call != call),
            "{call}: worked in FT8 and confirmed, so no row: {board:?}"
        );
    }
    // The radio's own decodes, on the FT8 tier and on the WSPR tier.
    for (call, mode) in [("K1ABC", "FT8"), ("VE3ABC", "FT8"), ("W9WSP", "WSPR")] {
        let a = heard(&needs, call, "20m", mode);
        assert!(a.is_none(), "{call} decoded in {mode}: {a:?}");
    }
}

/// THE DESKTOP'S WINDOWS ARE TESTED AGAINST THIS BOARD, NOT A COPY OF IT. The fixture is the
/// report's board, and `ui/src/neededNewMode.agreement.test.tsx` shows it in each window that shows
/// a need. When the board changes this fails and prints the rows: put them in the file, and the
/// desktop's test then says what the windows make of them.
#[cfg(feature = "live")]
#[test]
fn the_reports_board_is_the_one_the_desktop_windows_are_tested_against() {
    let got = serde_json::to_value(the_reports_board().1).expect("serializes");
    let want: Value = serde_json::from_str(include_str!(
        "../../../ui/src/features/__fixtures__/needed-new-mode-board.json"
    ))
    .expect("the fixture parses");
    assert!(
        got == want,
        "the board now reads:\n{}",
        serde_json::to_string_pretty(&got).expect("prints")
    );
}
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
