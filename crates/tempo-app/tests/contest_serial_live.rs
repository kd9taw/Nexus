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

/// The number is a property of the CONTACT, not of the moment it is read.
///
/// A serial is issued once, to one station, and every later read of it — a repeat of
/// the exchange, the cockpit re-rendering, a second look at the same peer — must give
/// back that number rather than the next one. The failure this pins is the one
/// `ContestSession::compose_for`'s own header names: sending your exchange twice and
/// sending two different numbers, where the second is the one they copy.
#[test]
fn a_repeat_to_the_same_station_shows_the_number_already_issued() {
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

    // The shell's startup order, on a machine that has just come back up.
    let mut back = Engine::new("W9XYZ", "EN61", 0);
    let mut s = back.settings().clone();
    s.fd_active = true;
    s.fd_event = "cqwpx_cw".into();
    s.contest_category_power = "LOW".into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.contest_qth_state = "WI".into();
    back.apply_settings(s);
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
