//! ⭐ **A serial contest must ISSUE a serial — through the door the operator actually
//! uses.**
//!
//! Five shipped rulesets declare a `FieldKind::Serial` slot (CQ WPX CW/SSB, ARRL
//! Sweepstakes CW/SSB, the California QSO Party). The number that goes in it is a
//! scoring-grade value: the other operator copies it and a checker matches it against
//! their log, so a row that carries the wrong one is a QSO line the sponsor can reject.
//!
//! [`ContestSession::compose_for`] has always known how to issue one. Nothing called it:
//! the live path is `contest_log_manual` -> `FieldDayLog::log_fields_at` ->
//! `ContestSession::tx_for_row`, which finds nothing in flight and copies the session
//! template — whose serial slot holds the placeholder `"0"` (`session::sent_value`).
//! Every row of every serial contest was logged, and exported, claiming serial zero.
//!
//! ⚠️ **The log half is only half.** A number the operator never had is a number they
//! never sent, so this file pins BOTH ends against each other: what the cockpit shows
//! and keys (`Engine::contest_sent_exchange`, which is `{EXCH}` for the CW keyer and
//! `sentExchange` for the RTTY macro dock) and what the row is stamped with. They are
//! one fact and a test that checked either alone would pass while the two disagreed.

use tempo_app::engine::Engine;
use tempo_core::contest::{install_call_resolver, CallLocation};

/// CQ WPX prices every contact by the relation between two stations, so a session
/// refuses to build without a country file. Every call this file works is a US one;
/// `install_call_resolver` is process-wide, so it goes in once per test binary.
fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(|_call| {
            Some(CallLocation {
                entity: "United States",
                continent: "NA",
                cq_zone: None,
            })
        })
        .expect("the first install in this test binary");
    });
}

/// An engine running CQ WPX CW — the smallest serial exchange that ships (RST + NR),
/// so nothing here is load-bearing except the serial.
fn wpx_engine() -> Engine {
    resolver();
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "cqwpx_cw".into();
    s.contest_category_power = "LOW".into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.contest_qth_state = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("a CQ WPX CW session builds");
    e
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn qso_lines(cab: &str) -> Vec<String> {
    cab.lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(|l| l.to_string())
        .collect()
}

/// ⭐ **The run: 1, 2, 3 — on the air and in the log, and the same number in both.**
#[test]
fn a_serial_contest_issues_a_run_instead_of_logging_zero() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    for (n, call) in [(1, "K1ABC"), (2, "W4XYZ"), (3, "N0DEF")] {
        // What the cockpit shows and the keyer sends BEFORE the contact is logged.
        // `{EXCH}` had dropped the serial slot entirely, so in CQ WPX — whose whole
        // exchange is RST plus the serial — it rendered the empty string and the
        // operator had no number to send at all.
        assert_eq!(
            e.contest_sent_exchange().as_deref(),
            Some(n.to_string().as_str()),
            "the exchange about to go on the air for {call}"
        );
        assert!(e
            .contest_log_manual(call, &fields(&[("RST", "599"), ("NR", "42")]), "CW", None)
            .expect("the contest session is running"));
    }

    let cab = e.export_log("cabrillo").expect("one entry");
    let lines = qso_lines(&cab);
    assert_eq!(lines.len(), 3, "{cab}");
    for (i, (n, call)) in [(1, "K1ABC"), (2, "W4XYZ"), (3, "N0DEF")]
        .iter()
        .enumerate()
    {
        assert!(
            lines[i].contains(&format!("W9XYZ 599 {n} {call} 599 42")),
            "row {i} did not send serial {n}:\n{}",
            lines[i]
        );
    }
}

/// ⭐ **A serial keyed to one station must not be keyed to another.**
///
/// The failure, in search-and-pounce with the run at 1: you call K1ABC and key your
/// exchange, so K1ABC copies 1. He is busy. You spin on, find W4XYZ, key the exchange —
/// **still 1, because nothing has issued** — and W4XYZ copies 1 too. W4XYZ comes back
/// first, so he is logged and takes 1. K1ABC answers later and his row is stamped 2, a
/// number he never copied. Two stations hold one serial, and the K1ABC row disagrees
/// with K1ABC's own log: both contacts are rejected at check-in, and nothing on screen
/// ever said so.
///
/// ⚠️ **WHY THE SEQUENCE IS commit → wipe → commit → log → return, and why it must not
/// be simplified back.** This repro went red TWICE. The first red was the defect. The
/// second came after the engine fix was already in, because the test was still driving
/// `contest_log_manual` alone and never announcing a peer — so it exercised nothing the
/// fix had changed. The cure was not to adjust the assertion but to make the test walk
/// the entry line the way an operator does: commit `K1ABC`, wipe without logging, commit
/// `W4XYZ`, log him, and only then let `K1ABC` come back.
///
/// **A repro that survives the fix being applied HALFWAY is a repro; one that greens the
/// moment anything changes was only ever a shape.** Collapse these steps — drop the
/// wipe, or log straight from `contest_log_manual` without committing a call — and this
/// passes against a build with no peer binding at all, which is precisely the state it
/// exists to catch.
#[test]
fn a_serial_keyed_to_one_station_is_not_re_issued_to_another() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    // The operator commits K1ABC on the entry line and keys the exchange. THIS is the
    // number he copies off the air.
    e.contest_working("K1ABC").expect("a running session");
    let keyed_to_k1abc = e
        .contest_sent_exchange()
        .expect("a running contest session");

    // No answer. The entry line is wiped and W4XYZ is worked and logged instead —
    // K1ABC is never logged at this point.
    e.contest_entry_reset();
    e.contest_working("W4XYZ").expect("a running session");
    let keyed_to_w4xyz = e
        .contest_sent_exchange()
        .expect("a running contest session");
    assert_ne!(
        keyed_to_w4xyz, keyed_to_k1abc,
        "W4XYZ was shown the number K1ABC had already copied"
    );
    assert!(e
        .contest_log_manual("W4XYZ", &fields(&[("RST", "599"), ("NR", "7")]), "CW", None)
        .unwrap());

    // …then K1ABC comes back. He must be given the number he already copied.
    e.contest_working("K1ABC").expect("a running session");
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some(keyed_to_k1abc.as_str()),
        "K1ABC came back and was shown a different number"
    );
    assert!(e
        .contest_log_manual("K1ABC", &fields(&[("RST", "599"), ("NR", "9")]), "CW", None)
        .unwrap());

    let cab = e.export_log("cabrillo").expect("one entry");
    let lines = qso_lines(&cab);
    assert_eq!(lines.len(), 2, "{cab}");
    let k1abc = lines
        .iter()
        .find(|l| l.contains("K1ABC"))
        .expect("K1ABC was logged");
    let w4xyz = lines
        .iter()
        .find(|l| l.contains("W4XYZ"))
        .expect("W4XYZ was logged");

    assert!(
        k1abc.contains(&format!("W9XYZ 599 {keyed_to_k1abc} K1ABC")),
        "K1ABC's row does not carry the serial he copied ({keyed_to_k1abc}):\n{k1abc}"
    );
    assert!(
        !w4xyz.contains(&format!("W9XYZ 599 {keyed_to_k1abc} W4XYZ")),
        "W4XYZ took the serial already keyed to K1ABC ({keyed_to_k1abc}):\n{w4xyz}"
    );
}

/// ⭐ **Correcting a busted call carries the number over; it does not mint a second
/// one.**
///
/// The hazard is not symmetric between the two ends, which is why it needs its own
/// test. At the LOG end a wrong key means no binding is found and a fresh number is
/// issued — visible, and the run stays dense. At the BINDING end a wrong key would mint
/// a second number when the right call is identified, while the station only ever
/// copied the first. So a correction must re-key the same number onto the corrected
/// call, and it does so without a reset between: the corrected call inherits the
/// exchange in flight because it holds no number of its own.
///
/// ⚠️ **The wrong call keeps a binding of its own** — the one thing here that changed
/// when the keep-both ruling landed (2026-09-20), and it is why this test's positive
/// control is about the RUN and not about the map. A stale entry on a call that will
/// never come back is harmless and is what buys the far worse case — a station who
/// really did copy a number being forgotten — in
/// `moving_to_another_station_without_a_reset_keeps_the_first_stations_number` above.
#[test]
fn correcting_a_busted_call_keeps_the_number_that_station_copied() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    // Keyed to what the operator THOUGHT the call was.
    e.contest_working("K1ABC").expect("a running session");
    let keyed = e.contest_sent_exchange().expect("a running session");

    // "K1ABD, not K1ABC" — the box is corrected on the SAME entry, no reset between.
    e.contest_working("K1ABD").expect("a running session");
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some(keyed.as_str()),
        "the correction minted a second number; the station copied the first"
    );

    assert!(e
        .contest_log_manual("K1ABD", &fields(&[("RST", "599"), ("NR", "3")]), "CW", None)
        .unwrap());

    // POSITIVE CONTROL — the wrong call stranded nothing: the next station takes the
    // very next number, not the one after a burnt one.
    e.contest_working("W4XYZ").expect("a running session");
    let next: u32 = e
        .contest_sent_exchange()
        .expect("a running session")
        .parse()
        .expect("the exchange is the serial");
    assert_eq!(
        next,
        keyed.parse::<u32>().expect("a serial") + 1,
        "the busted call stranded a serial"
    );

    let cab = e.export_log("cabrillo").expect("one entry");
    let line = qso_lines(&cab).remove(0);
    assert!(
        line.contains(&format!("W9XYZ 599 {keyed} K1ABD")),
        "the corrected row does not carry the number that went out:\n{line}"
    );
}

/// ⭐ **Typing the next station over the entry box must not FORGET the one before it.**
///
/// The operator calls K1ABC and keys the exchange — he copies 1. No answer, so the next
/// call is typed straight over the box without touching the wipe. **That gesture is
/// ambiguous by construction**: the same keystrokes are "K1ABD, not K1ABC" and "no
/// answer, I've moved on", and nothing in the session can tell them apart. It was read
/// as a correction ALWAYS, which `remove`d K1ABC's binding and handed it to the new
/// call — so the station with a number written on his pad came back to a different one,
/// through the gesture an operator makes most.
///
/// **The ruling (operator, 2026-09-20): keep BOTH bindings.** The outgoing call keeps
/// the number it copied; if the call really was busted, the stale entry costs a
/// `String` and a `u32` and that callsign will never call in, which is exactly the
/// trade [`issued`](tempo_core::contest::ContestSession::issued)'s own doc makes when
/// it says nothing is ever evicted.
///
/// The accepted consequence is asserted below rather than left implicit: two stations
/// hold one serial and both rows carry it. That is correct — both partners copied that
/// number, so both match at check-in.
#[test]
fn moving_to_another_station_without_a_reset_keeps_the_first_stations_number() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    e.contest_working("K1ABC").expect("a running session");
    let keyed = e
        .contest_sent_exchange()
        .expect("a running contest session");

    // ⚠️ NO `contest_entry_reset` here, and that is the whole test: every other repro in
    // this file wipes between stations, which is the one gesture that was never broken.
    e.contest_working("W4XYZ").expect("a running session");
    assert!(e
        .contest_log_manual("W4XYZ", &fields(&[("RST", "599"), ("NR", "7")]), "CW", None)
        .unwrap());

    // K1ABC answers after all. He must be given the number he already copied.
    e.contest_working("K1ABC").expect("a running session");
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some(keyed.as_str()),
        "K1ABC was forgotten the moment the next call was typed over the box"
    );
    assert!(e
        .contest_log_manual("K1ABC", &fields(&[("RST", "599"), ("NR", "9")]), "CW", None)
        .unwrap());

    let cab = e.export_log("cabrillo").expect("one entry");
    let lines = qso_lines(&cab);
    assert_eq!(lines.len(), 2, "{cab}");
    for call in ["K1ABC", "W4XYZ"] {
        let line = lines
            .iter()
            .find(|l| l.contains(call))
            .unwrap_or_else(|| panic!("{call} was logged:\n{cab}"));
        assert!(
            line.contains(&format!("W9XYZ 599 {keyed} {call}")),
            "{call}'s row does not carry the number that went out ({keyed}):\n{line}"
        );
    }

    // POSITIVE CONTROL — keeping both bindings mints nothing: one number went out, one
    // number was spent, and the next station takes the next one rather than the one
    // after a burnt one.
    e.contest_working("N0DEF").expect("a running session");
    let next = (keyed.parse::<u32>().expect("the exchange is the serial") + 1).to_string();
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some(next.as_str()),
        "the ambiguous type-over minted a second number"
    );
}

/// ⭐ **A station who already holds a number is given THEIR number — not the one in
/// flight — and the exchange in flight is re-rendered to it.**
///
/// ⚠️ **The two sources have to DISAGREE or this says nothing.** With W4XYZ in flight on
/// 2 the operator types K1ABC, who copied 1 an hour ago, over the box: the binding and
/// the live exchange now name different numbers and exactly one of them is what K1ABC
/// has on his pad. Before the fix the in-flight exchange was merely re-labelled — K1ABC
/// was handed W4XYZ's 2, the strip went on showing 2, the keyer sent 2 and the row was
/// stamped 2, a number K1ABC never copied — and W4XYZ's own binding was dropped on the
/// way through, so he had been forgotten too.
#[test]
fn a_station_who_already_holds_a_number_is_given_their_own_not_the_one_in_flight() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    // K1ABC copies 1 and is left unlogged.
    e.contest_working("K1ABC").expect("a running session");
    assert_eq!(e.contest_sent_exchange().as_deref(), Some("1"));
    e.contest_entry_reset();

    // W4XYZ is called and copies 2 — the disagreeing value.
    e.contest_working("W4XYZ").expect("a running session");
    assert_eq!(e.contest_sent_exchange().as_deref(), Some("2"));

    // K1ABC comes back and is typed over the box with W4XYZ still in flight.
    e.contest_working("K1ABC").expect("a running session");
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some("1"),
        "K1ABC was handed the number in flight instead of the one he copied"
    );
    // The PHONE leg reads the exchange off the screen and speaks it — three of the five
    // serial rulesets have one — so the in-flight EXCHANGE, not merely the counter, has
    // to carry the number the binding names.
    assert_eq!(
        e.contest_composing_text().as_deref(),
        Some("599 1"),
        "the strip still reads aloud the number that was in flight"
    );

    // …and the row is stamped with it, which is the half a checker sees.
    assert!(e
        .contest_log_manual("K1ABC", &fields(&[("RST", "599"), ("NR", "4")]), "CW", None)
        .unwrap());
    let cab = e.export_log("cabrillo").expect("one entry");
    let line = qso_lines(&cab).remove(0);
    assert!(
        line.contains("W9XYZ 599 1 K1ABC"),
        "K1ABC's row does not carry the number he copied:\n{line}"
    );

    // W4XYZ was not forgotten on the way through.
    e.contest_working("W4XYZ").expect("a running session");
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some("2"),
        "W4XYZ's binding was dropped when K1ABC was typed over the box"
    );
}

/// A read does not advance the run — and that is ALL this pins.
///
/// ⚠️ It says nothing about the number being bound to a station; the test that did claim
/// that was vacuous, because reading twice with nothing in between passes whether or not
/// anything is bound. `a_serial_keyed_to_one_station_is_not_re_issued_to_another` above
/// is the one that makes the condition. This one exists because
/// `Engine::contest_sent_exchange` runs on every snapshot tick and every macro preview,
/// so a read that advanced the counter would burn a serial per frame.
#[test]
fn reading_the_exchange_does_not_advance_the_run() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    assert!(e
        .contest_log_manual("K1ABC", &fields(&[("RST", "599"), ("NR", "1")]), "CW", None)
        .unwrap());
    // Second contact, read three times before it is logged: the same number each time.
    for _ in 0..3 {
        assert_eq!(e.contest_sent_exchange().as_deref(), Some("2"));
    }
    assert!(e
        .contest_log_manual("W4XYZ", &fields(&[("RST", "599"), ("NR", "2")]), "CW", None)
        .unwrap());
    assert_eq!(
        e.contest_sent_exchange().as_deref(),
        Some("3"),
        "the logged contact's number was not spent"
    );
}

/// ⭐ **A restart mid-contest continues the run — through the app's OWN restart.**
///
/// The session-level half of this landed on 2026-09-20 (`FieldDayLog::restore_row`
/// re-derives the run from the restored rows) and was inert on arrival: with nothing
/// issuing, every journaled row carried the `"0"` placeholder and there was no run to
/// re-derive. This is the chain it was waiting for, walked the way the shell walks it —
/// `set_fd_log_path`, then `restore_field_day_if_enabled`, which builds a FRESH session
/// and merges the journal back over it.
#[test]
fn a_restart_continues_the_run_from_the_journal() {
    // ⚠️ Unique per RUN, not just per process: sibling worktrees run this binary
    // concurrently and a pid can repeat across them, so a pid-only name is a shared
    // path that merely looks private.
    let dir = std::env::temp_dir().join(format!(
        "contest-serial-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let journal = dir.join("fieldday_backup_test.adi");

    let mut e = wpx_engine();
    e.set_fd_log_path(journal.clone());
    e.set_frequency(14.025, "20m", "CW");
    for call in ["K1ABC", "W4XYZ", "N0DEF"] {
        assert!(e
            .contest_log_manual(call, &fields(&[("RST", "599"), ("NR", "42")]), "CW", None)
            .unwrap());
    }
    assert_eq!(e.contest_sent_exchange().as_deref(), Some("4"));
    drop(e);

    // ⚠️ The shell's startup order, and it has to be THIS order — `Engine::with_settings`
    // (which launches in `Mode::Chat`), then the journal path, then the restore. Building
    // it with `apply_settings` instead tests nothing: that call reconciles the mode with
    // the master switch and enters Field Day on the spot, with no journal path set yet, so
    // the `restore_field_day_if_enabled` below is a silent no-op — it refuses to rebuild a
    // live FD log. `src-tauri`'s launch does it in the order copied here.
    let mut s = Engine::new("W9XYZ", "EN61", 0).settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "cqwpx_cw".into();
    s.contest_category_power = "LOW".into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.contest_qth_state = "WI".into();
    let mut back = Engine::with_settings(s);
    back.set_fd_log_path(journal.clone());
    back.restore_field_day_if_enabled();

    assert_eq!(
        back.contest_sent_exchange().as_deref(),
        Some("4"),
        "the restart re-issued a number the log had already sent"
    );
    back.set_frequency(14.025, "20m", "CW");
    assert!(back
        .contest_log_manual(
            "VE3GHI",
            &fields(&[("RST", "599"), ("NR", "42")]),
            "CW",
            None
        )
        .unwrap());
    let cab = back.export_log("cabrillo").expect("one entry");
    let lines = qso_lines(&cab);
    assert_eq!(lines.len(), 4, "the journal did not come back:\n{cab}");
    assert!(
        lines[3].contains("W9XYZ 599 4 VE3GHI 599 42"),
        "the contact after the restart:\n{}",
        lines[3]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// POSITIVE CONTROL — a contest with no serial slot is untouched.
///
/// Field Day is the exchange every byte-pinned ADIF and Cabrillo golden in the tree
/// rides on, and the serial wiring runs through the same two functions it uses. If
/// `contest_sent_exchange` started inventing a slot, or `contest_log_manual` started
/// issuing into one, this is where it would show.
#[test]
fn an_exchange_with_no_serial_slot_is_unchanged() {
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "arrlfd".into();
    s.fd_class = "2A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("a Field Day session builds");
    e.set_frequency(14.025, "20m", "CW");

    // Class and section, and nothing joining them — no slot to issue into means no
    // number, on every contact.
    assert_eq!(e.contest_sent_exchange().as_deref(), Some("2A WI"));
    assert!(e
        .contest_log_manual(
            "K1ABC",
            &fields(&[("CLASS", "3A"), ("SECTION", "IL")]),
            "CW",
            None
        )
        .unwrap());
    assert_eq!(e.contest_sent_exchange().as_deref(), Some("2A WI"));

    let cab = e.export_log("cabrillo").expect("one entry");
    let lines = qso_lines(&cab);
    assert_eq!(lines.len(), 1, "{cab}");
    assert!(
        lines[0].contains("W9XYZ 2A WI K1ABC 3A IL"),
        "Field Day's exchange moved:\n{}",
        lines[0]
    );
}

/// ⭐ **The strip the operator READS shows the issued serial, not the placeholder.**
///
/// On phone there is no macro: the operator reads the exchange off the screen and speaks
/// it. The log strip's "Sent:" line and the phone cockpit both render the composing
/// VECTOR, whose serial slot holds `session::sent_value`'s `"0"` for ever — so with the
/// run at 1 the screen said `599 0` while the row was logged as 1. A log that records
/// serials the station never sent fails check-in while looking correct, and three of the
/// five serial rulesets have a phone leg.
///
/// The report stays: `contest_sent_exchange` drops it because `{RST}` is its own macro
/// token, but nothing keys this string, and dropping `599` here would be a regression for
/// CQ WW RTTY, where it is part of what the operator says.
#[test]
fn the_exchange_the_operator_reads_aloud_carries_the_issued_serial() {
    let mut e = wpx_engine();
    e.set_frequency(14.025, "20m", "CW");

    e.contest_working("K1ABC").expect("a running session");
    assert_eq!(
        e.contest_composing_text().as_deref(),
        Some("599 1"),
        "the strip showed the placeholder, not the number this station was given"
    );

    assert!(e
        .contest_log_manual("K1ABC", &fields(&[("RST", "599"), ("NR", "5")]), "CW", None)
        .unwrap());
    e.contest_working("W4XYZ").expect("a running session");
    assert_eq!(e.contest_composing_text().as_deref(), Some("599 2"));

    // …and it follows the BINDING, not the counter: a station worked and left unlogged
    // is read back at the number they copied.
    e.contest_entry_reset();
    e.contest_working("N0DEF").expect("a running session");
    assert_eq!(e.contest_composing_text().as_deref(), Some("599 3"));
    e.contest_entry_reset();
    e.contest_working("W4XYZ").expect("a running session");
    assert_eq!(
        e.contest_composing_text().as_deref(),
        Some("599 2"),
        "W4XYZ came back and the strip read a different number aloud"
    );
}

/// POSITIVE CONTROL — Field Day, which has no serial slot, reads exactly as before, and
/// the report-bearing CQ WW RTTY exchange keeps its `599`.
#[test]
fn an_exchange_with_no_serial_reads_aloud_unchanged() {
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "arrlfd".into();
    s.fd_class = "2A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("a Field Day session builds");
    assert_eq!(e.contest_composing_text().as_deref(), Some("2A WI"));
}
