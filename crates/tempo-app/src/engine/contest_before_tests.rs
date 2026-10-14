//! The station as it was before the contest changed it, through the engine's own write paths — a
//! Settings save, the seat swap, the Unassisted switch, a restore — and through settings.json. Every
//! assertion is on the note's VALUE: when it is taken, what it holds, that the first one stands, and
//! when it is let go.
use super::*;
use crate::settings::ContestBefore;
use tempo_core::fd_rules::{ruleset_by_id, EventWindow};

const DAY: u64 = 86_400;

/// The Illinois QSO Party's 2026 running, off the rules data: Sunday 18 October, 1700Z to 0100Z.
fn ilqp_2026() -> EventWindow {
    ruleset_by_id("ilqp", 2026)
        .expect("ILQP ships")
        .event_window(2026)
}

/// ARRL Field Day's 2026 running, the contest the picker's blank default runs.
fn field_day_2026() -> EventWindow {
    ruleset_by_id("arrlfd", 2026)
        .expect("Field Day ships")
        .event_window(2026)
}

/// The Friday before the Illinois QSO Party, when a club sets up: inside its week, and in October,
/// far outside Field Day's.
fn ilqp_friday() -> u64 {
    ilqp_2026().start_unix - 2 * DAY
}

/// The note reads a contest's week at `unix`, on this test's thread.
fn at(unix: u64) {
    super::contest_before::NOW_FOR_TESTS.with(|c| c.set(Some(unix)));
}

/// A station on its everyday settings: its own call, nobody else at the key, no position, no
/// declarations, Unassisted off, the Illinois QSO Party picked but not running. On the Friday
/// before it, unless a test reads another time.
fn everyday() -> Engine {
    at(ilqp_friday());
    let mut e = Engine::new("K9ABC", "EN52", 0);
    let mut s = e.settings().clone();
    s.mycall = "K9ABC".into();
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "COOK".into();
    e.apply_settings(s);
    e
}

/// What [`everyday`] holds, as the note holds it.
fn everyday_values() -> ContestBefore {
    ContestBefore {
        mycall: "K9ABC".into(),
        ..ContestBefore::default()
    }
}

/// A Settings save of `edit` over what the engine holds now.
fn save(e: &mut Engine, edit: impl FnOnce(&mut Settings)) {
    let mut s = e.settings().clone();
    edit(&mut s);
    e.apply_settings(s);
}

#[test]
fn an_everyday_station_holds_no_note() {
    assert_eq!(everyday().settings().contest_before, None);
}

#[test]
fn a_club_setup_saved_before_the_switch_is_noted_as_the_station_was_before_it() {
    let mut e = everyday();
    // Friday at the club: the club's call, a tent, MULTI-OP. Nobody has turned the mode on.
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_position_name = "CW tent".into();
        s.contest_category_operator = "MULTI-OP".into();
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn turning_the_mode_on_takes_the_note_even_when_nothing_else_changed() {
    let mut e = everyday();
    save(&mut e, |s| s.fd_active = true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn the_first_note_stands_through_every_later_change_of_the_weekend() {
    let mut e = everyday();
    save(&mut e, |s| s.contest_category_power = "LOW".into());
    save(&mut e, |s| s.fd_active = true);
    save(&mut e, |s| s.contest_entry_class = "ILLINOIS FIXED".into());
    e.set_fd_operator("w9op".into());
    e.set_unassisted_mode(true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn a_seat_swap_or_the_unassisted_switch_takes_the_note_on_its_own() {
    let mut e = everyday();
    e.set_fd_operator("w9op".into());
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    let mut e = everyday();
    e.set_unassisted_mode(true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn a_save_that_moves_none_of_the_seven_takes_no_note() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.contest_email = "k9abc@example.org".into();
        s.contest_qth_county = "WILL".into();
    });
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn a_stale_or_forged_payload_neither_writes_the_note_nor_wipes_it() {
    let mut e = everyday();
    // A surface that loaded before the weekend: its copy has no note.
    let stale = e.settings().clone();
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    let mut wipe = stale;
    wipe.mycall = "W9CLB".into();
    e.apply_settings(wipe);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "a payload without one wipes nothing"
    );
    let mut forged = e.settings().clone();
    forged.contest_before = Some(ContestBefore {
        mycall: "N0PE".into(),
        ..ContestBefore::default()
    });
    e.apply_settings(forged);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "nor does a payload write one"
    );
}

#[test]
fn turning_the_mode_off_lets_the_note_go_with_the_contest() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_active = true;
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    // The checklist's save: the mode off, the call put back.
    save(&mut e, |s| {
        s.fd_active = false;
        s.mycall = "K9ABC".into();
    });
    assert_eq!(e.settings().contest_before, None);
    assert_eq!(e.settings().mycall, "K9ABC");
}

#[test]
fn turning_the_mode_off_after_the_contest_with_nothing_put_back_lets_it_go() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_active = true;
    });
    at(ilqp_2026().end_unix + 600);
    save(&mut e, |s| s.fd_active = false);
    assert_eq!(
        e.settings().contest_before,
        None,
        "declined at the end: dismissed"
    );
    assert_eq!(
        e.settings().mycall,
        "W9CLB",
        "what was not put back stays as it is"
    );
}

#[test]
fn unassisted_put_back_before_the_mode_goes_off_leaves_no_stray_note() {
    // The checklist's order: Unassisted through its own switch while the mode is still on, then the
    // save that turns the mode off. The other way round, the switch would find no note open and take
    // a new one holding the contest's "on" as "before".
    let mut e = everyday();
    e.set_unassisted_mode(true);
    save(&mut e, |s| s.fd_active = true);
    e.set_unassisted_mode(false);
    save(&mut e, |s| s.fd_active = false);
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn a_restore_takes_the_backups_note_whole() {
    let mut e = everyday();
    let mut bundle = e.settings().clone();
    let noted = ContestBefore {
        mycall: "K9OLD".into(),
        contest_category_power: "HIGH".into(),
        ..ContestBefore::default()
    };
    bundle.contest_before = Some(noted.clone());
    e.apply_restored_settings(bundle);
    assert_eq!(e.settings().contest_before, Some(noted));
}

#[test]
fn the_note_round_trips_through_settings_json_and_an_older_file_reads_as_none() {
    let s = Settings {
        contest_before: Some(ContestBefore {
            mycall: "K9ABC".into(),
            fd_operator: "K9ZZZ".into(),
            fd_position_name: "CW tent".into(),
            contest_category_operator: "MULTI-OP".into(),
            contest_entry_class: "ILLINOIS FIXED".into(),
            contest_category_power: "LOW".into(),
            contest_category_band: "20M".into(),
            unassisted_mode: true,
        }),
        ..Settings::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    // The wire keys `ui/src/types.ts` writes by hand.
    for key in [
        "\"contestBefore\":{\"mycall\":\"K9ABC\"",
        "\"fdOperator\":\"K9ZZZ\"",
        "\"fdPositionName\":\"CW tent\"",
        "\"contestCategoryOperator\":\"MULTI-OP\"",
        "\"contestEntryClass\":\"ILLINOIS FIXED\"",
        "\"contestCategoryPower\":\"LOW\"",
        "\"contestCategoryBand\":\"20M\"",
        "\"unassistedMode\":true}",
    ] {
        assert!(json.contains(key), "missing {key} in the settings file");
    }
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    let mut older: serde_json::Value = serde_json::from_str(&json).unwrap();
    older.as_object_mut().unwrap().remove("contestBefore");
    assert_eq!(
        serde_json::from_value::<Settings>(older)
            .unwrap()
            .contest_before,
        None
    );
}

/// A station that has never picked a contest: the picker's blank default, Contest mode off, in
/// October, far from Field Day. One save first, so the engine's own fill-ins (a fresh roster's
/// default radio) are in before what a test saves.
fn nothing_picked() -> Engine {
    at(ilqp_friday());
    let mut e = Engine::new("K9ABC", "EN52", 0);
    let s = e.settings().clone();
    e.apply_settings(s);
    assert_eq!(e.settings().fd_event, "", "premise: no contest picked");
    e
}

/// The settings.json keys whose values differ between `a` and `b`.
fn changed(a: &Settings, b: &Settings) -> Vec<String> {
    let (a, b) = (
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap(),
    );
    let (a, b) = (a.as_object().unwrap(), b.as_object().unwrap());
    let mut keys: Vec<String> = a
        .keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

#[test]
fn with_no_contest_picked_a_save_holds_exactly_what_the_form_sent() {
    let mut e = nothing_picked();
    let mut form = e.settings().clone();
    form.mycall = "W9CLB".into();
    e.apply_settings(form.clone());
    assert_eq!(changed(&form, e.settings()), Vec::<String>::new());
    // A pick of nothing but spaces is the same blank.
    let mut e = nothing_picked();
    save(&mut e, |s| s.fd_event = "  ".into());
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn with_no_contest_picked_the_unassisted_switch_changes_only_unassisted() {
    let mut e = nothing_picked();
    let was = e.settings().clone();
    e.set_unassisted_mode(true);
    assert_eq!(changed(&was, e.settings()), vec!["unassistedMode"]);
}

#[test]
fn with_no_contest_picked_the_operator_box_changes_only_the_operator() {
    let mut e = nothing_picked();
    let was = e.settings().clone();
    e.set_fd_operator("w9op".into());
    assert_eq!(changed(&was, e.settings()), vec!["fdOperator"]);
}

#[test]
fn with_no_contest_picked_the_mode_going_on_still_takes_the_note_and_going_off_lets_it_go() {
    // The blank pick runs ARRL Field Day when the mode goes on: that is a contest running.
    let mut e = nothing_picked();
    save(&mut e, |s| {
        s.fd_active = true;
        s.fd_class = "1D".into();
        s.fd_section = "IL".into();
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    save(&mut e, |s| s.mycall = "W9CLB".into());
    e.set_unassisted_mode(true);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "the first note stands while the contest runs"
    );
    e.set_unassisted_mode(false);
    save(&mut e, |s| {
        s.fd_active = false;
        s.mycall = "K9ABC".into();
    });
    assert_eq!(e.settings().contest_before, None);
}

#[test]
fn a_club_call_set_with_a_contest_picked_is_offered_back_when_the_contest_ends() {
    let mut e = nothing_picked();
    // Picking the contest moves none of the seven.
    save(&mut e, |s| {
        s.fd_event = "ilqp".into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "COOK".into();
    });
    assert_eq!(e.settings().contest_before, None);
    // Friday, the mode still off: the club's call.
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    // The weekend.
    at(ilqp_2026().start_unix + 3600);
    save(&mut e, |s| s.fd_active = true);
    e.set_fd_operator("w9op".into());
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "what the checklist offers as the mode goes off: the station's own call"
    );
    // The checklist's save once the contest is over: the call put back, the mode off.
    at(ilqp_2026().end_unix + 600);
    save(&mut e, |s| {
        s.mycall = "K9ABC".into();
        s.fd_active = false;
    });
    assert_eq!(e.settings().contest_before, None);
    assert_eq!(e.settings().mycall, "K9ABC");
}

#[test]
fn picking_the_contest_and_setting_the_club_call_in_one_save_takes_the_note() {
    let mut e = nothing_picked();
    save(&mut e, |s| {
        s.fd_event = "ilqp".into();
        s.mycall = "W9CLB".into();
    });
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
}

#[test]
fn at_launch_a_note_with_no_contest_picked_and_the_mode_off_is_let_go_and_any_other_is_kept() {
    at(ilqp_friday());
    let noted = ContestBefore {
        mycall: "K9OLD".into(),
        ..ContestBefore::default()
    };
    let launched = |event: &str, active: bool| {
        let stored = Settings {
            mycall: "K9ABC".into(),
            fd_event: event.into(),
            fd_active: active,
            fd_class: "1D".into(),
            fd_section: "IL".into(),
            contest_before: Some(noted.clone()),
            ..Settings::default()
        };
        Engine::with_settings(stored)
            .settings()
            .contest_before
            .clone()
    };
    assert_eq!(
        launched("", false),
        None,
        "an earlier build's everyday note"
    );
    assert_eq!(launched("  ", false), None, "spaces are the same blank");
    assert_eq!(
        launched("ilqp", false),
        Some(noted.clone()),
        "a contest picked"
    );
    assert_eq!(
        launched("", true),
        Some(noted.clone()),
        "a restart mid-contest"
    );
    assert_eq!(launched("ilqp", true), Some(noted.clone()), "both");
    // A month on, the party is over: its note goes with the mode off, and stays with it on.
    at(ilqp_2026().end_unix + 30 * DAY);
    assert_eq!(launched("ilqp", false), None, "a contest over");
    assert_eq!(
        launched("ilqp", true),
        Some(noted.clone()),
        "the mode still on"
    );
    // The blank default is Field Day: in its week, a note is kept.
    at(field_day_2026().start_unix - DAY);
    assert_eq!(launched("", false), Some(noted), "Field Day's week");
}

#[test]
fn a_pick_counts_from_a_week_before_its_contest_until_it_ends() {
    let ilqp = ilqp_2026();
    let club_call_at = |unix: u64| {
        let mut e = everyday();
        at(unix);
        save(&mut e, |s| s.mycall = "W9CLB".into());
        e.settings().contest_before.clone()
    };
    assert_eq!(
        club_call_at(ilqp.start_unix - 8 * DAY),
        None,
        "eight days before: everyday business"
    );
    assert_eq!(
        club_call_at(ilqp.start_unix - 6 * DAY),
        Some(everyday_values()),
        "six days before: the club's week"
    );
    assert_eq!(
        club_call_at(ilqp.start_unix + 3600),
        Some(everyday_values()),
        "while it runs"
    );
    assert_eq!(
        club_call_at(ilqp.end_unix + 3600),
        None,
        "once it has ended, though it stays picked"
    );
}

#[test]
fn the_blank_pick_is_field_day_and_counts_in_field_days_week() {
    let mut e = nothing_picked();
    at(field_day_2026().start_unix - 2 * DAY);
    save(&mut e, |s| s.mycall = "W9CLB".into());
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "the club call typed in the week before Field Day, with the lit default left as it was"
    );
}

#[test]
fn declined_before_the_contest_ends_the_note_stays_for_its_end() {
    // The rehearsal: the club call in on the Friday, the mode on and off again with nothing put
    // back, then the party.
    let mut e = everyday();
    save(&mut e, |s| s.mycall = "W9CLB".into());
    save(&mut e, |s| s.fd_active = true);
    save(&mut e, |s| s.fd_active = false);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "the rehearsal's Turn off, nothing ticked"
    );
    at(ilqp_2026().start_unix + 3600);
    save(&mut e, |s| s.fd_active = true);
    assert_eq!(e.settings().contest_before, Some(everyday_values()));
    // After the party, the checklist offers the station's own call; ticked, it is used.
    at(ilqp_2026().end_unix + 600);
    save(&mut e, |s| {
        s.mycall = "K9ABC".into();
        s.fd_active = false;
    });
    assert_eq!(e.settings().contest_before, None, "used");
    assert_eq!(e.settings().mycall, "K9ABC");
}

#[test]
fn a_declined_note_goes_once_its_contest_is_over() {
    let mut e = everyday();
    save(&mut e, |s| {
        s.mycall = "W9CLB".into();
        s.fd_active = true;
    });
    save(&mut e, |s| s.fd_active = false);
    assert_eq!(
        e.settings().contest_before,
        Some(everyday_values()),
        "premise: declined on the Friday, kept"
    );
    at(ilqp_2026().end_unix + 2 * DAY);
    save(&mut e, |s| s.contest_email = "w9clb@example.org".into());
    assert_eq!(e.settings().contest_before, None);
    assert_eq!(
        e.settings().mycall,
        "W9CLB",
        "what was not put back stays as it is"
    );
}

/// ⭐ The Band category is one of the contest's changes: a `20M` left from an earlier contest is
/// what the note holds when this contest's change replaces it, and putting it back uses the note.
/// Read through the wire key the checklist reads.
#[test]
fn the_band_category_a_contest_replaced_is_noted_and_putting_it_back_uses_the_note() {
    // A month before the party, outside its week: an everyday save leaves 20M declared.
    at(ilqp_2026().start_unix - 30 * DAY);
    let mut e = Engine::new("K9ABC", "EN52", 0);
    save(&mut e, |s| {
        s.mycall = "K9ABC".into();
        s.fd_event = "ilqp".into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "COOK".into();
        s.contest_category_band = "20M".into();
    });
    assert_eq!(e.settings().contest_before, None, "premise: no note yet");
    at(ilqp_friday());
    save(&mut e, |s| s.contest_category_band = "ALL".into());
    let band = e
        .settings()
        .contest_before
        .as_ref()
        .map(|n| serde_json::to_value(n).unwrap()["contestCategoryBand"].clone());
    assert_eq!(band, Some(serde_json::json!("20M")));
    save(&mut e, |s| s.fd_active = true);
    save(&mut e, |s| {
        s.contest_category_band = "20M".into();
        s.fd_active = false;
    });
    assert_eq!(
        e.settings().contest_before,
        None,
        "all of it put back: used"
    );
    assert_eq!(e.settings().contest_category_band, "20M");
}
