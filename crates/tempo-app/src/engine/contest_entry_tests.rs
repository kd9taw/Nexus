//! The contest strip's contact in progress, shared with the contest logger window, through the
//! engine: there only while the window is open, the same for both windows, logged once when
//! both press Enter at the same moment, and never a change to the transmitter.

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier, Mutex};

use super::contest_entry::{
    EntryClaim, ENTRY_CHANGED, ENTRY_CLOSED, ENTRY_LOGGED, ENTRY_TOO_LARGE,
};
use super::*;

fn boxes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A contest engine for `event`, with `edit` applied to its settings, in search and pounce.
fn contest(event: &str, edit: impl FnOnce(&mut Settings)) -> Engine {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    let mut s = e.settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = event.into();
    s.fd_position_id = "aaaa0001".into();
    edit(&mut s);
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("the contest runs");
    let _ = e.snapshot();
    e
}

/// The New York QSO Party from Albany County, which LOGS a duplicate contact (marked, worth
/// nothing) rather than refusing it — so a second Enter on the same contact would be a second
/// row, not a refusal.
fn nyqp() -> Engine {
    contest("nyqp", |s| {
        s.contest_qth_state = "NY".into();
        s.contest_qth_county = "ALB".into();
    })
}

/// The Illinois QSO Party from Kane County, which refuses a duplicate contact.
fn ilqp() -> Engine {
    contest("ilqp", |s| {
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "KANE".into();
    })
}

/// The contest log's calls, oldest first.
fn calls(e: &Engine) -> Vec<String> {
    e.snapshot()
        .field_day
        .expect("the contest runs")
        .log
        .iter()
        .map(|q| q.call.clone())
        .collect()
}

/// What the strip in a window logs on Enter: the claim on the entry it saw, then its own command.
fn enter(
    e: &mut Engine,
    claim: &EntryClaim,
    fields: &[(&str, &str)],
    mode: &str,
) -> Result<bool, String> {
    let fields: Vec<(String, String)> = fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let call = claim.call.clone();
    e.contest_log_entry(
        Some(claim),
        |e| e.contest_log_manual(&call, &fields, mode, None),
        |logged| *logged,
    )
}

fn claim(rev: u64, call: &str, fields: &[(&str, &str)]) -> EntryClaim {
    EntryClaim {
        rev,
        call: call.into(),
        fields: boxes(fields),
    }
}

/// What says whether anything transmits, or could on its own.
fn transmit_state(e: &Engine) -> (bool, Option<TxOwner>, bool, bool, bool) {
    (
        e.tx_enabled(),
        e.tx_owner(),
        e.manual_ptt(),
        e.tuning(),
        e.slot_tx_abort,
    )
}

#[test]
fn there_is_a_shared_entry_only_while_the_logger_window_is_open() {
    let mut e = nyqp();
    assert_eq!(
        e.snapshot().contest_entry,
        None,
        "no logger window, no shared entry"
    );
    assert_eq!(
        e.contest_entry_put("K1ABC", boxes(&[]), serde_json::Value::Null),
        Err(ENTRY_CLOSED.to_string()),
        "nothing to put to while the window is closed"
    );

    e.contest_entry_share(true);
    let opened = e.snapshot().contest_entry.expect("the window is open");
    assert_eq!(
        (opened.call.as_str(), opened.fields.len()),
        ("", 0),
        "a new sharing starts blank"
    );

    let rev = e
        .contest_entry_put(
            "K1ABC",
            boxes(&[("QTH", "BRX")]),
            serde_json::json!({ "x": 1 }),
        )
        .unwrap();
    // Opened again while open (a click on its button focuses it): nothing is reset.
    e.contest_entry_share(true);
    let shown = e.snapshot().contest_entry.unwrap();
    assert_eq!(shown.rev, rev);
    assert_eq!(shown.call, "K1ABC");
    assert_eq!(shown.fields, boxes(&[("QTH", "BRX")]));
    assert_eq!(shown.marks, serde_json::json!({ "x": 1 }));

    e.contest_entry_share(false);
    assert_eq!(
        e.snapshot().contest_entry,
        None,
        "closing the window ends the sharing"
    );
}

#[test]
fn every_put_moves_the_rev_and_the_last_one_is_what_both_windows_show() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let r1 = e
        .contest_entry_put("K1A", boxes(&[]), serde_json::Value::Null)
        .unwrap();
    let r2 = e
        .contest_entry_put("K1AB", boxes(&[("RST", "57")]), serde_json::Value::Null)
        .unwrap();
    assert!(r2 > r1);
    let shown = e.snapshot().contest_entry.unwrap();
    assert_eq!((shown.rev, shown.call.as_str()), (r2, "K1AB"));
    assert_eq!(shown.fields, boxes(&[("RST", "57")]));
}

/// ⭐ THE RACE: both windows press Enter on the same contact at the same moment, on two threads
/// that meet at a barrier and then take the one engine lock. In a contest that logs a duplicate
/// rather than refusing it, a second log would be a second row; one contact must be one row.
#[test]
fn two_enters_at_the_same_moment_log_one_contact() {
    let engine = Arc::new(Mutex::new(nyqp()));
    engine_lock(&engine).contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let rounds = 60;
    for n in 0..rounds {
        let call = format!("K1A{n:02}");
        let rev = engine_lock(&engine)
            .contest_entry_put(&call, boxes(&fields), serde_json::Value::Null)
            .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let presses: Vec<_> = (0..2)
            .map(|_| {
                let engine = Arc::clone(&engine);
                let barrier = Arc::clone(&barrier);
                let seen = claim(rev, &call, &fields);
                std::thread::spawn(move || {
                    barrier.wait();
                    enter(&mut engine_lock(&engine), &seen, &fields, "PH")
                })
            })
            .collect();
        let mut answers: Vec<Result<bool, String>> =
            presses.into_iter().map(|p| p.join().unwrap()).collect();
        answers.sort();
        assert_eq!(
            answers,
            vec![Ok(true), Err(ENTRY_LOGGED.to_string())],
            "round {n}: one Enter logs it, the other is told it was logged"
        );
    }
    let log = calls(&engine_lock(&engine));
    assert_eq!(log.len(), rounds, "one row per contact: {log:?}");
}

/// In a contest that refuses a duplicate, the second Enter was already safe from a second row —
/// but it was told "dupe", which is not what happened. It is told the contact was logged.
#[test]
fn where_the_contest_refuses_a_dupe_the_second_enter_is_told_it_was_logged() {
    let mut e = ilqp();
    e.contest_entry_share(true);
    let fields = [("RST", "599"), ("QTH", "COOK")];
    let rev = e
        .contest_entry_put("K9ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    let seen = claim(rev, "K9ABC", &fields);
    assert_eq!(enter(&mut e, &seen, &fields, "CW"), Ok(true));
    assert_eq!(
        enter(&mut e, &seen, &fields, "CW"),
        Err(ENTRY_LOGGED.to_string())
    );
    assert_eq!(calls(&e), ["K9ABC"]);
}

#[test]
fn an_enter_on_a_contact_the_other_window_has_changed_logs_nothing() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let rev = e
        .contest_entry_put("K1ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    // The other window corrects the call before this one's Enter arrives.
    e.contest_entry_put("K1ABD", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    assert_eq!(
        enter(&mut e, &claim(rev, "K1ABC", &fields), &fields, "PH"),
        Err(ENTRY_CHANGED.to_string()),
        "K1ABC is not on the strip any more"
    );
    assert!(calls(&e).is_empty(), "nothing logged");
    // …and the contact as it now stands logs.
    let now = e.snapshot().contest_entry.unwrap().rev;
    assert_eq!(
        enter(&mut e, &claim(now, "K1ABD", &fields), &fields, "PH"),
        Ok(true)
    );
    assert_eq!(calls(&e), ["K1ABD"]);
}

#[test]
fn a_change_that_left_the_contact_as_shown_does_not_refuse_it() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let rev = e
        .contest_entry_put("K1ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    // The other window wrote again — a mark of its own, the same call and boxes.
    e.contest_entry_put(
        "k1abc ",
        boxes(&fields),
        serde_json::json!({ "take": null }),
    )
    .unwrap();
    assert_eq!(
        enter(&mut e, &claim(rev, "K1ABC", &fields), &fields, "PH"),
        Ok(true)
    );
    assert_eq!(calls(&e), ["K1ABC"]);
}

#[test]
fn a_contact_logged_leaves_the_entry_for_the_next_one() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let rev = e
        .contest_entry_put("K1ABC", boxes(&fields), serde_json::json!({ "fill": 1 }))
        .unwrap();
    assert_eq!(
        enter(&mut e, &claim(rev, "K1ABC", &fields), &fields, "PH"),
        Ok(true)
    );
    let next = e.snapshot().contest_entry.unwrap();
    assert!(next.rev > rev, "the log moved the entry on");
    assert_eq!(next.call, "", "the call goes with its contact");
    assert_eq!(next.marks, serde_json::Value::Null, "and so do its marks");
    assert_eq!(
        next.fields,
        boxes(&fields),
        "the boxes stay, as a strip's do"
    );
    // A window that has seen the log claims the next contact normally.
    let rev = e
        .contest_entry_put("K1ABD", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    assert_eq!(
        enter(&mut e, &claim(rev, "K1ABD", &fields), &fields, "PH"),
        Ok(true)
    );
    assert_eq!(calls(&e), ["K1ABC", "K1ABD"]);
}

/// A window that had not seen the log is refused even if the next contact typed since looks the
/// same as the one it shows: its Enter was for the contact already in the log.
#[test]
fn an_enter_older_than_the_last_log_is_refused_whatever_it_shows() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let rev = e
        .contest_entry_put("K1ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    let stale = claim(rev, "K1ABC", &fields);
    assert_eq!(enter(&mut e, &stale, &fields, "PH"), Ok(true));
    e.contest_entry_put("K1ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    assert_eq!(
        enter(&mut e, &stale, &fields, "PH"),
        Err(ENTRY_LOGGED.to_string())
    );
    assert_eq!(calls(&e), ["K1ABC"]);
}

#[test]
fn a_dupe_the_contest_refuses_leaves_the_entry_where_it_was() {
    let mut e = ilqp();
    let fields = [("RST", "599"), ("QTH", "COOK")];
    // Worked before the window opened, from the strip alone.
    assert!(e
        .contest_log_manual(
            "K9ABC",
            &fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<Vec<_>>(),
            "CW",
            None
        )
        .unwrap());
    e.contest_entry_share(true);
    let rev = e
        .contest_entry_put("K9ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    assert_eq!(
        enter(&mut e, &claim(rev, "K9ABC", &fields), &fields, "CW"),
        Ok(false)
    );
    let entry = e.snapshot().contest_entry.unwrap();
    assert_eq!(
        (entry.rev, entry.call.as_str()),
        (rev, "K9ABC"),
        "a refused dupe is not a contact"
    );
}

/// Closing the logger window can never stop the main window logging: with no shared entry a
/// claim checks nothing, whatever rev it names.
#[test]
fn with_the_window_closed_a_claim_checks_nothing() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    e.contest_entry_put("K1ABD", boxes(&[]), serde_json::Value::Null)
        .unwrap();
    e.contest_entry_share(false);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    assert_eq!(
        enter(&mut e, &claim(0, "K1ABC", &fields), &fields, "PH"),
        Ok(true)
    );
    assert_eq!(calls(&e), ["K1ABC"]);
}

/// A window holding a rev from an earlier sharing can never pass for current in the next one.
#[test]
fn a_new_sharing_starts_past_every_rev_the_last_one_handed_out() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let old = e
        .contest_entry_put("K1ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    e.contest_entry_share(false);
    e.contest_entry_share(true);
    let fresh = e.snapshot().contest_entry.unwrap();
    assert!(fresh.rev > old, "{} > {old}", fresh.rev);
    assert_eq!(
        enter(&mut e, &claim(old, "K1ABC", &fields), &fields, "PH"),
        Err(ENTRY_CHANGED.to_string()),
        "the new sharing's entry is blank, not K1ABC"
    );
}

#[test]
fn a_put_larger_than_any_strip_writes_is_refused() {
    let mut e = nyqp();
    e.contest_entry_share(true);
    let before = e.snapshot().contest_entry.unwrap();
    let long = "X".repeat(4096);
    for (call, fields, marks) in [
        (long.as_str(), boxes(&[]), serde_json::Value::Null),
        (
            "K1ABC",
            boxes(&[("QTH", long.as_str())]),
            serde_json::Value::Null,
        ),
        (
            "K1ABC",
            boxes(&[]),
            serde_json::json!({ "x": "y".repeat(9000) }),
        ),
    ] {
        assert_eq!(
            e.contest_entry_put(call, fields, marks),
            Err(ENTRY_TOO_LARGE.to_string())
        );
    }
    assert_eq!(
        e.snapshot().contest_entry.unwrap(),
        before,
        "nothing stored"
    );
}

/// Nothing about sharing the entry, or logging from it, keys, unkeys or arms anything.
#[test]
fn nothing_about_the_shared_entry_touches_the_transmitter() {
    let mut e = nyqp();
    let before = transmit_state(&e);
    e.contest_entry_share(true);
    let fields = [("RST", "59"), ("QTH", "BRX")];
    let rev = e
        .contest_entry_put("K1ABC", boxes(&fields), serde_json::Value::Null)
        .unwrap();
    assert_eq!(
        enter(&mut e, &claim(rev, "K1ABC", &fields), &fields, "PH"),
        Ok(true)
    );
    let _ = enter(&mut e, &claim(rev, "K1ABC", &fields), &fields, "PH");
    e.contest_entry_share(false);
    assert_eq!(transmit_state(&e), before);
}
