//! ⭐ **An operator's own contest through the app**: saved to `custom-contests.json`, listed,
//! started, logged and exported; locked against an exchange edit once a journal holds its rows;
//! imported over a collision; exported with nothing the operator typed; and, once gone, refused
//! by name instead of running as ARRL Field Day.
//!
//! Own process, because the user layer is process-global. Each test uses its own id and token.

use std::path::PathBuf;
use tempo_app::custom_contests::{self, OnCollision, Store};
use tempo_app::engine::Engine;
use tempo_core::fd_rules::custom::{self as core_custom, CustomDef, CustomField, CustomWindow};

/// 2026-11-14T00:00Z.
const NOW: u64 = 1_794_614_400;

/// A fresh folder under the system temp dir, for one test.
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nexus-custom-contests-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

fn def(id: &str, token: &str, fields: &[&str]) -> CustomDef {
    CustomDef {
        format: core_custom::FORMAT.into(),
        version: core_custom::VERSION,
        id: id.into(),
        name: format!("Sprint {id}"),
        short: String::new(),
        cabrillo_contest: token.into(),
        modes: vec!["CW".into()],
        window: CustomWindow::Weekend {
            month: 3,
            weekend: "nth_full".into(),
            n: 1,
            start_hour_utc: 14,
            duration_hours: 4,
        },
        exchange: fields
            .iter()
            .map(|f| CustomField {
                field: f.to_string(),
                label: String::new(),
            })
            .collect(),
        ft_cq: None,
        ft_roundup: false,
        dupe: "band".into(),
        points: Default::default(),
    }
}

fn store(d: &std::path::Path) -> Store {
    let (store, problems) = Store::load(d.join(custom_contests::FILE_NAME), NOW);
    assert!(problems.is_empty(), "{problems:?}");
    store
}

/// An engine on `event`, journaling into `d`, with an age typed for `event`.
fn engine(d: &std::path::Path, event: &str) -> Engine {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.set_fd_log_path(d.join("fieldday_backup_test.adi"));
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = event.into();
    s.contest_qth_state = "IL".into();
    s.contest_category_operator = "SINGLE-OP".into();
    s.contest_entered
        .insert(event.into(), [("AGE".to_string(), "42".to_string())].into());
    e.apply_settings(s);
    e
}

/// ⭐ Save → list → start → log → Cabrillo, end to end; then the exchange locks, and the name
/// does not.
#[test]
fn save_list_start_and_export_then_the_exchange_locks() {
    let d = dir("e2e");
    let mut st = store(&d);
    let mine = def("custom_app_e2e", "APP-E2E", &["rst", "serial"]);
    st.save_contest(&mine, &d, NOW).expect("saved");
    let on_disk = std::fs::read_to_string(d.join(custom_contests::FILE_NAME)).expect("written");
    assert!(on_disk.contains("\"custom_app_e2e\""), "{on_disk}");
    let listed = custom_contests::list(&d);
    let row = listed
        .iter()
        .find(|c| c.def.id == "custom_app_e2e")
        .expect("listed");
    assert!(!row.exchange_locked, "no contacts yet");

    let mut e = engine(&d, "custom_app_e2e");
    e.set_mode("fieldday-sp").expect("the contest starts");
    let fields = [
        ("RST".to_string(), "599".to_string()),
        ("NR".to_string(), "7".to_string()),
    ];
    assert_eq!(e.contest_log_manual("K1ABC", &fields, "CW", None), Ok(true));
    e.journal_mark().wait();
    let cbr = e.export_log("cabrillo").expect("exports");
    assert!(cbr.contains("CONTEST: APP-E2E"), "{cbr}");
    assert!(
        cbr.lines()
            .any(|l| l.starts_with("QSO:") && l.ends_with("K1ABC 599 7")),
        "{cbr}"
    );

    assert!(custom_contests::list(&d)
        .iter()
        .any(|c| c.def.id == "custom_app_e2e" && c.exchange_locked));
    let mut changed = mine.clone();
    changed.exchange.push(CustomField {
        field: "cq_zone".into(),
        label: String::new(),
    });
    let err = st
        .save_contest(&changed, &d, NOW)
        .expect_err("the exchange is locked");
    assert!(err.contains("has contacts logged"), "{err}");
    let mut retoken = mine.clone();
    retoken.cabrillo_contest = "APP-E2E-NEW".into();
    assert!(
        st.save_contest(&retoken, &d, NOW).is_err(),
        "so is the token its rows are found by"
    );
    let mut renamed = mine.clone();
    renamed.name = "A better name".into();
    renamed.points.cw = 3;
    st.save_contest(&renamed, &d, NOW)
        .expect("name and points stay editable");
}

/// ⭐ **THE MUST-FIX**: a picked contest that is not installed refuses to start, by name — it
/// never runs as ARRL Field Day.
#[test]
fn a_custom_contest_that_is_gone_refuses_instead_of_running_field_day() {
    let d = dir("gone");
    let mut e = engine(&d, "custom_gone");
    let err = e.set_mode("fieldday-sp").expect_err("refused");
    assert_eq!(
        err,
        "The custom contest 'custom_gone' is not installed (deleted, or its file could not be \
         read). Pick a contest."
    );
    assert!(e.snapshot().field_day.is_none(), "no Field Day session");
}

/// …and on FT8 the Operate strip's Call CQ and S&P, which open the picked contest's own Running
/// and S&P, are refused with the same words: nothing goes on the air as Field Day.
#[test]
fn ft8_call_cq_and_sp_in_a_custom_contest_that_is_gone_are_refused() {
    let d = dir("gone-ft8");
    let mut e = engine(&d, "custom_gone_ft8");
    let err = e.start_running_with_cq(None).expect_err("Call CQ refused");
    assert!(err.contains("is not installed"), "{err}");
    let err = e.start_search_and_pounce().expect_err("S&P refused");
    assert!(err.contains("is not installed"), "{err}");
    assert!(e.snapshot().field_day.is_none(), "no Field Day session");
    assert!(!e.tx_enabled(), "nothing armed");
}

/// Delete is refused while the contest is the picked one; after, its id is gone.
#[test]
fn delete_is_refused_while_picked() {
    let d = dir("delete");
    let mut st = store(&d);
    st.save_contest(&def("custom_app_delete", "APP-DELETE", &["rst"]), &d, NOW)
        .expect("saved");
    let err = st
        .delete("custom_app_delete", " custom_app_delete ")
        .expect_err("picked");
    assert!(err.contains("Pick another contest"), "{err}");
    st.delete("custom_app_delete", "arrlfd").expect("deleted");
    assert!(tempo_core::fd_rules::custom_def("custom_app_delete").is_none());
    assert!(!std::fs::read_to_string(d.join(custom_contests::FILE_NAME))
        .expect("written")
        .contains("custom_app_delete"));
}

/// An import whose id is taken asks; Keep both installs it beside, under a fresh id and token;
/// Replace replaces.
#[test]
fn an_import_collision_asks_then_keeps_both_or_replaces() {
    let d = dir("import");
    let mut st = store(&d);
    let mine = def("custom_app_import", "APP-IMPORT", &["rst"]);
    st.save_contest(&mine, &d, NOW).expect("saved");
    let mut theirs = mine.clone();
    theirs.points.cw = 5;
    let text = core_custom::to_file_text(&theirs);
    let err = st
        .import(&text, OnCollision::Ask, &d, NOW)
        .expect_err("asks");
    assert_eq!(
        err,
        format!(
            "{}Sprint custom_app_import",
            custom_contests::COLLISION_PREFIX
        )
    );
    let both = st
        .import(&text, OnCollision::KeepBoth, &d, NOW)
        .expect("kept both");
    assert_eq!(both.id, "custom_sprint_custom_app_import");
    assert_eq!(both.cabrillo_contest, "APP-IMPORT-2");
    assert_eq!(
        tempo_core::fd_rules::custom_def("custom_app_import")
            .expect("still there")
            .points
            .cw,
        1
    );
    st.import(&text, OnCollision::Replace, &d, NOW)
        .expect("replaced");
    assert_eq!(
        tempo_core::fd_rules::custom_def("custom_app_import")
            .expect("replaced")
            .points
            .cw,
        5
    );
}

/// ⭐ An exported definition carries nothing the operator typed (the maintainer's ruling on
/// sharing): the age lives in Settings.
#[test]
fn an_export_carries_no_typed_value() {
    let d = dir("export");
    let mut st = store(&d);
    st.save_contest(
        &def("custom_app_export", "APP-EXPORT", &["rst", "age"]),
        &d,
        NOW,
    )
    .expect("saved");
    let mut e = engine(&d, "custom_app_export");
    e.set_mode("fieldday-sp")
        .expect("starts with the typed age");
    let text = custom_contests::export_text("custom_app_export").expect("exports");
    assert!(!text.contains("42"), "{text}");
    assert!(!text.to_ascii_lowercase().contains("entered"), "{text}");
    assert!(
        text.contains("\"age\""),
        "the field is named, its value is not"
    );
}

/// A contest file that cannot be read is reported, left as it is, and never written over.
#[test]
fn an_unreadable_file_is_left_alone_and_never_overwritten() {
    let d = dir("unreadable");
    let path = d.join(custom_contests::FILE_NAME);
    std::fs::write(&path, "{ not json").expect("written");
    let (mut st, problems) = Store::load(path.clone(), NOW);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let err = st
        .save_contest(&def("custom_app_locked", "APP-LOCKED", &["rst"]), &d, NOW)
        .expect_err("refused");
    assert!(err.contains("Move it aside first"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).expect("kept"), "{ not json");
}

/// Club sync refuses an own contest: every position would need the same definition.
#[test]
fn club_sync_refuses_a_custom_contest() {
    let d = dir("club");
    store(&d)
        .save_contest(&def("custom_app_club", "APP-CLUB", &["rst"]), &d, NOW)
        .expect("saved");
    let rs = tempo_core::fd_rules::ruleset_by_id(
        "custom_app_club",
        tempo_core::fd_rules::CURRENT_RULES_YEAR,
    )
    .expect("installed");
    assert_eq!(
        tempo_app::fdevent::club_refusal(rs),
        Some(tempo_app::fdevent::ClubRefusal::Custom)
    );
    assert_eq!(
        tempo_app::fdevent::picker_name("custom_app_club"),
        "Sprint custom_app_club"
    );
}

/// The builder's preview: what this station sends, a real Cabrillo line, the FT8 match — and a
/// definition refusal verbatim.
#[test]
fn the_preview_renders_from_the_real_compiler_session_and_exporter() {
    let mut settings = Engine::new("W9XYZ", "EN52", 0).settings().clone();
    settings.contest_qth_state = "IL".into();
    let mut d = def("custom_app_preview", "APP-PREVIEW", &["rst", "serial"]);
    d.modes = vec!["CW".into(), "FT8".into()];
    let p = custom_contests::preview(&d, &settings, NOW);
    assert_eq!(p.refusal, "");
    assert_eq!(p.you_send, "599 1");
    assert!(p.sample_qso_line.starts_with("QSO: 14025 CW "), "{p:?}");
    assert!(p.sample_qso_line.ends_with("K1ABC 599 1"), "{p:?}");
    assert_eq!(p.ft_shape, Some(core_custom::FtActivity::RttyRoundup));
    assert_eq!(p.ft_activity, None, "not asked for");
    assert_eq!(p.ft_messages, "standard");
    let mut bad = d.clone();
    bad.cabrillo_contest = "CQ-WPX-CW".into();
    assert!(custom_contests::preview(&bad, &settings, NOW)
        .refusal
        .contains("use the built-in one"));
    let mut aged = d.clone();
    aged.exchange.push(CustomField {
        field: "age".into(),
        label: String::new(),
    });
    let p = custom_contests::preview(&aged, &settings, NOW);
    assert!(p.start_problem.contains("Your AGE is empty"), "{p:?}");
    assert_eq!(p.entered_slots, ["AGE"]);
}

/// ⭐ The builder's FT8/FT4 line says what this build sends for each exchange shape: the activity's
/// own CQ and exchange where it sends that activity's messages, nothing where it sends the standard
/// ones. Rendered here, from the sequencer's own choice and builders.
#[test]
fn the_preview_names_the_ft_messages_this_build_sends() {
    let mut settings = Engine::new("W9XYZ", "EN52ab", 0).settings().clone();
    settings.fd_class = "2B".into();
    settings.fd_section = "WI".into();
    settings.contest_qth_state = "IL".into();
    let cases: &[(&str, &[&str], Option<&str>, &str, &str, &str)] = &[
        (
            "custom_pv_fd",
            &["fd_class", "arrl_section"],
            None,
            "field_day",
            "CQ FD W9XYZ EN52",
            "2B WI",
        ),
        (
            "custom_pv_navhf",
            &["grid4"],
            Some("TEST"),
            "na_vhf",
            "CQ TEST W9XYZ EN52",
            "EN52",
        ),
        (
            "custom_pv_none",
            &["rst", "cq_zone"],
            None,
            "standard",
            "",
            "",
        ),
    ];
    for (i, (id, fields, cq, messages, ft_cq, sends)) in cases.iter().enumerate() {
        let mut d = def(id, &format!("APP-PV-{i}"), fields);
        d.modes = vec!["FT8".into()];
        d.ft_cq = cq.map(str::to_string);
        let p = custom_contests::preview(&d, &settings, NOW);
        assert_eq!(p.refusal, "", "{id}");
        assert_eq!(
            (
                p.ft_messages.as_str(),
                p.ft_cq.as_str(),
                p.ft_sends.as_str()
            ),
            (*messages, *ft_cq, *sends),
            "{id}"
        );
    }
}

/// Write a contest file holding `entries`, as a raw JSON list.
fn write_file(path: &std::path::Path, entries: Vec<serde_json::Value>) {
    let file = serde_json::json!({"version": 1, "contests": entries});
    std::fs::write(path, serde_json::to_vec_pretty(&file).expect("json")).expect("written");
}

/// ⭐ **An entry this version cannot use stays on disk through the next save** — one a later
/// version wrote (a downgrade), one with a key this version does not know, and one a newer rules
/// download now refuses (its token is a shipped contest's). Each is listed as unusable, the good
/// one installs, and saving another contest writes all three back untouched.
#[test]
fn entries_this_version_cannot_use_survive_the_next_save() {
    let d = dir("skipped");
    let path = d.join(custom_contests::FILE_NAME);
    let good = serde_json::to_value(def("custom_app_good", "APP-GOOD", &["rst"])).expect("json");
    let mut later =
        serde_json::to_value(def("custom_app_later", "APP-LATER", &["rst"])).expect("json");
    later["version"] = serde_json::json!(2);
    let mut unknown_key =
        serde_json::to_value(def("custom_app_newkey", "APP-NEWKEY", &["rst"])).expect("json");
    unknown_key["multipliers"] = serde_json::json!(["dxcc"]);
    let shipped_token =
        serde_json::to_value(def("custom_app_owned", "CQ-WPX-CW", &["rst"])).expect("json");
    write_file(
        &path,
        vec![
            good,
            later.clone(),
            unknown_key.clone(),
            shipped_token.clone(),
        ],
    );
    let (mut st, problems) = Store::load(path.clone(), NOW);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(tempo_core::fd_rules::custom_def("custom_app_good").is_some());
    let unusable: Vec<String> = st.unusable().into_iter().map(|u| u.id).collect();
    assert_eq!(
        unusable,
        ["custom_app_later", "custom_app_newkey", "custom_app_owned"]
    );
    assert!(
        st.unusable()[0].why.contains("version 2"),
        "{:?}",
        st.unusable()
    );

    st.save_contest(&def("custom_app_after", "APP-AFTER", &["rst"]), &d, NOW)
        .expect("saved");
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("kept")).expect("json");
    let entries = back["contests"].as_array().expect("a list");
    for kept in [&later, &unknown_key, &shipped_token] {
        assert!(
            entries.contains(kept),
            "{kept} was not written back untouched"
        );
    }
    assert!(entries.iter().any(|e| e["id"] == "custom_app_after"));
    assert!(entries.iter().any(|e| e["id"] == "custom_app_good"));
}

/// A new contest may not take the id of an entry this version cannot use (saving it would drop
/// that entry); deleting the unusable entry removes it from the file.
#[test]
fn an_unusable_entry_keeps_its_id_until_it_is_deleted() {
    let d = dir("unusable-id");
    let path = d.join(custom_contests::FILE_NAME);
    let mut later =
        serde_json::to_value(def("custom_app_held", "APP-HELD", &["rst"])).expect("json");
    later["version"] = serde_json::json!(2);
    write_file(&path, vec![later]);
    let (mut st, _) = Store::load(path.clone(), NOW);
    let err = st
        .save_contest(&def("custom_app_held", "APP-HELD-NEW", &["rst"]), &d, NOW)
        .expect_err("the id is held");
    assert!(err.contains("can't use"), "{err}");
    st.delete("custom_app_held", "arrlfd").expect("deleted");
    assert!(st.unusable().is_empty());
    assert!(!std::fs::read_to_string(&path)
        .expect("written")
        .contains("custom_app_held"));
}
