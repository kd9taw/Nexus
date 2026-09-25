//! SPEC-2 v3 C19 (Part A): a contest session's dupe sweep is built by the command that opens the
//! session, from the logbook's rows read before the command takes the Engine lock and installed
//! in the same hold as the switch. So no snapshot ever sees the session without it, and none
//! sweeps the log to make it (the operator's pick: the command waits until the sweep is ready).
//! With no log in memory left to sweep (the C19 cut), a command that cannot read the rows is
//! refused after its tries: retry briefly, then refuse. `set_mode` itself is untouched: every
//! switch here goes through it, wrapped or not.

use super::*;
use crate::logstore::tests::{engine_on_store, flush, no_resolve, qso, Dir};
use crate::logstore::SESSION_READS;
use std::sync::Mutex;
use tempo_core::logbook::sqlite::WriteHold;
use tempo_core::logbook::{adif_header, adif_record, LOG_SWEEPS};

/// An engine on a store of its own, ready to enter Field Day (class and section filled) with
/// the master OFF, so the switch is the test's to make.
fn ready(d: &Dir) -> Engine {
    let mut e = engine_on_store(d);
    let mut s = e.settings().clone();
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = "a1b2c3d4".into();
    e.apply_settings(s);
    e
}

/// Switch to `spec` through the wrap, as the `set_mode` command does. Whether the session's
/// sweep was installed from the rows read before the switch — or, when they could not be read,
/// the refusal, and no switch.
fn switch(m: &Mutex<Engine>, spec: &str) -> Result<bool, String> {
    with_session_rows(
        m,
        |e| e.mode_opens_session(spec),
        |e, rows| {
            e.set_mode(spec).expect("the switch");
            rows.is_some_and(|r| e.open_session_from(r))
        },
    )
}

/// Turn the Field Day master on while Field Day runs: the session is kept, and the snapshot
/// starts asking for it.
fn master_on(e: &mut Engine) {
    let mut s = e.settings().clone();
    s.fd_active = true;
    e.apply_settings(s);
}

/// A Field Day journal holding `rows` — `(call, when)` — as the journal carries them.
fn journal(d: &Dir, rows: &[(&str, u64)]) {
    let mut text = adif_header();
    for (call, when) in rows {
        let row = adif_record(&qso(call, *when));
        let at = row.rfind("<EOR>").expect("a record ends");
        text.push_str(&format!(
            "{}<CLASS:2>2A<ARRL_SECT:3>EMA{}",
            &row[..at],
            &row[at..]
        ));
    }
    std::fs::write(d.0.join("fd.adi"), text).expect("the journal");
}

/// ★ A Field Day session opened through the wrap has its sweep BEFORE its first snapshot: the
/// snapshot sweeps nothing, and the session holds this session's general-log contacts and not
/// the ones before it. The CONTROL is the same switch made directly, without the rows: no
/// session's sweep is open at all — nothing sweeps a log to make one any more (SPEC-2 v3 C19) —
/// which is why every command that opens a session goes through the wrap.
#[test]
fn a_field_day_opened_through_the_wrap_has_its_sweep_before_its_first_snapshot() {
    for wrapped in [true, false] {
        let d = Dir::new("session-open");
        let mut e = ready(&d);
        e.set_fd_log_path(d.0.join("fd.adi"));
        let now = now_unix_secs();
        // A contest restarted mid-event: the journal restores a contact from an hour ago, so the
        // session starts then, and the general log's contacts since are the session's.
        journal(&d, &[("K1ABC", now - 3_600)]);
        e.log_qso(qso("W1AW", now - 1_800));
        e.log_qso(qso("N0OLD", now - 86_400));
        flush(&e);
        let m = Mutex::new(e);
        if wrapped {
            assert_eq!(switch(&m, "fieldday-sp"), Ok(true), "opened with its sweep");
        } else {
            engine_lock(&m).set_mode("fieldday-sp").expect("the switch");
        }
        let mut e = m.into_inner().expect("not poisoned");
        master_on(&mut e);
        if !wrapped {
            let Mode::FieldDay { station, .. } = &e.mode else {
                panic!("in Field Day");
            };
            let (start, rule) = (station.log.session.start_unix, station.log.dupe_rule());
            assert!(
                e.station.worked_since(start, &rule).is_none(),
                "CONTROL: a session opened directly, without its rows, has no sweep"
            );
            continue;
        }
        LOG_SWEEPS.with(|c| c.set(0));
        let _ = e.snapshot();
        let b4 = e.session_b4().expect("a session is open");
        assert!(
            b4.worked_this_session("W1AW") && !b4.worked_this_session("N0OLD"),
            "this session's contact, and not the one before it"
        );
        assert_eq!(
            LOG_SWEEPS.with(|c| c.get()),
            0,
            "the first snapshot sweeps nothing: the sweep was ready"
        );
    }
}

/// Only a switch that opens a contest session pays for the read. A switch between ordinary
/// modes reads nothing, and neither does a switch between Field Day modes, which keeps the
/// session open, nor leaving Field Day. The POSITIVE CONTROL: entering Field Day reads once.
#[test]
fn a_switch_that_opens_no_session_reads_nothing() {
    let d = Dir::new("session-none");
    let m = Mutex::new(ready(&d));
    let reads = || SESSION_READS.with(|c| c.get());
    SESSION_READS.with(|c| c.set(0));
    for spec in ["qso-monitor", "chat", "qso-monitor"] {
        switch(&m, spec).expect("switched");
    }
    assert_eq!(reads(), 0, "a switch between ordinary modes reads nothing");
    switch(&m, "fieldday-sp").expect("switched");
    assert_eq!(reads(), 1, "entering Field Day opens a session: one read");
    for spec in ["fieldday-run", "fieldday-sp", "chat"] {
        switch(&m, spec).expect("switched");
    }
    assert_eq!(
        reads(),
        1,
        "a switch between Field Day modes keeps the session, and leaving it opens none"
    );
}

/// The TX gate answers alike right after a switch made through the wrap and the same switch
/// made directly — `tx_allowed_as` for every operating mode, `tx_allowed`, the TX-enable latch
/// and the CQ run — a Field Day open included. The wrap reads and installs; the switch is
/// `set_mode`'s, unchanged.
#[test]
fn the_tx_gate_answers_alike_after_a_switch_through_the_wrap() {
    use crate::settings::OperatingMode::{Cw, Digital, Keyboard, Phone, Rtty};
    for (from, to) in [
        ("chat", "fieldday-sp"),
        ("chat", "fieldday-run"),
        ("qso-monitor", "fieldday-run"),
        ("chat", "qso-monitor"),
        ("chat", "qso-run"),
        ("fieldday-sp", "fieldday-run"),
        ("fieldday-run", "chat"),
    ] {
        let (d1, d2) = (Dir::new("tx-direct"), Dir::new("tx-wrapped"));
        let mut direct = ready(&d1);
        let wrapped = Mutex::new(ready(&d2));
        direct.set_mode(from).expect("from");
        engine_lock(&wrapped).set_mode(from).expect("from");
        direct.set_mode(to).expect("to");
        switch(&wrapped, to).expect("switched");
        let w = engine_lock(&wrapped);
        for om in [Digital, Phone, Cw, Rtty, Keyboard] {
            assert_eq!(
                direct.tx_allowed_as(om),
                w.tx_allowed_as(om),
                "{from} → {to}: the gate as {om:?}"
            );
        }
        assert_eq!(
            direct.tx_allowed(),
            w.tx_allowed(),
            "{from} → {to}: the gate"
        );
        assert_eq!(
            direct.tx_enabled(),
            w.tx_enabled(),
            "{from} → {to}: the TX-enable latch"
        );
        assert_eq!(direct.cq_running, w.cq_running, "{from} → {to}: the CQ run");
    }
}

/// ⛔ The read before a session opens reaches back `SESSION_READ_WINDOW`, and that is enough
/// only while no session can start earlier. A new session starts no earlier than its oldest
/// contact restored from the Field Day journal, and `set_mode` restores only the last four days.
/// This pins the two together: a contact just inside four days is restored and starts the
/// session, and one past the read's reach is not. Should the journal ever keep more, the older
/// contact starts the session before the rows read for it reach, and this fails.
#[test]
fn a_session_starts_no_earlier_than_the_rows_read_for_it_reach() {
    let d = Dir::new("session-window");
    let mut e = ready(&d);
    e.set_fd_log_path(d.0.join("fd.adi"));
    let now = now_unix_secs();
    let inside = now - 4 * 86_400 + 600;
    let past = now - SESSION_READ_WINDOW - 600;
    journal(&d, &[("K1ABC", inside), ("K2ABC", past)]);
    let m = Mutex::new(e);
    assert_eq!(switch(&m, "fieldday-sp"), Ok(true), "opened with its sweep");
    let e = engine_lock(&m);
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("in Field Day");
    };
    let start = station.log.session.start_unix;
    assert_eq!(
        start, inside,
        "the contact four days back starts the session"
    );
    assert!(
        start >= now - SESSION_READ_WINDOW,
        "the session starts inside the rows read for it"
    );
    assert!(
        !station.log.qsos().iter().any(|q| q.call == "K2ABC"),
        "the contact past the read's reach is not restored"
    );
}

/// Rows read before the log changed are not its rows after: a contact logged between the read
/// and the switch makes them stale, and a stale read is not installed — the wrap reads again
/// (below), and never opens a session from rows that are not the log's.
#[test]
fn rows_read_before_the_log_changed_are_not_its_rows_after() {
    let d = Dir::new("session-stale");
    let mut e = ready(&d);
    let rows = e.session_read().expect("a store").read();
    assert!(
        e.session_rows_current(&rows),
        "nothing changed since: current"
    );
    e.log_qso(qso("W1AW", now_unix_secs() + 60));
    assert!(
        !e.session_rows_current(&rows),
        "a contact logged since: not the log's rows any more"
    );
    e.set_mode("fieldday-sp").expect("the switch");
    assert!(!e.open_session_from(rows), "and not installed");
}

/// Rows that saw ANOTHER window's contact — committed to the shared store, not yet taken in
/// here — are not this window's log, and are not installed. Through the wrap, the window takes
/// the contact in and reads again, and the session holds it.
#[test]
fn rows_that_saw_another_windows_contact_wait_for_it_to_be_taken_in() {
    let d = Dir::new("session-foreign");
    let a = ready(&d);
    let mut b = engine_on_store(&d);
    // After the session will start (now), so it is the session's.
    b.log_qso(qso("W9FOR", now_unix_secs() + 60));
    flush(&b);
    let rows = a.session_read().expect("a store").read();
    assert!(
        !a.session_rows_current(&rows),
        "a read that saw another window's contact is not this window's log"
    );
    let m = Mutex::new(a);
    assert_eq!(
        switch(&m, "fieldday-sp"),
        Ok(true),
        "taken in, read again, and opened"
    );
    let mut a = m.into_inner().expect("not poisoned");
    master_on(&mut a);
    let b4 = a.session_b4().expect("a session is open");
    assert!(
        b4.worked_this_session("W9FOR"),
        "the other window's contact is this session's"
    );
}

/// ★ The launch: the open sets aside the rows a session can hold, the attach hands them back,
/// and a Field Day session restored just after is swept from them — its first snapshot sweeps
/// nothing.
#[test]
fn a_session_restored_at_launch_is_swept_from_the_rows_the_open_set_aside() {
    let d = Dir::new("session-launch");
    let now = now_unix_secs();
    {
        let mut e = engine_on_store(&d);
        e.log_qso(qso("W1AW", now - 1_800));
        e.log_qso(qso("N0OLD", now - 86_400));
        flush(&e);
    }
    journal(&d, &[("K1ABC", now - 3_600)]);
    let opened = crate::logstore::open_reporting(
        &d.log(),
        no_resolve(),
        None,
        Some(crate::logstore::HotBuild::keyed_by(None)),
        &mut |_| {},
    )
    .expect("the store opens");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let rows = e
        .attach_log_store(opened)
        .expect("the rows the open set aside");
    e.set_fd_log_path(d.0.join("fd.adi"));
    let mut s = e.settings().clone();
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = "a1b2c3d4".into();
    s.fd_active = true;
    // The master was left on: the save re-enters Field Day, as the launch's restore does.
    e.apply_settings(s);
    assert!(
        e.open_session_from(rows),
        "swept from the rows the open set aside"
    );
    LOG_SWEEPS.with(|c| c.set(0));
    let _ = e.snapshot();
    assert_eq!(
        LOG_SWEEPS.with(|c| c.get()),
        0,
        "the first snapshot sweeps nothing"
    );
    let b4 = e.session_b4().expect("a session is open");
    assert!(b4.worked_this_session("W1AW") && !b4.worked_this_session("N0OLD"));
}

/// Rows that start after the session does — a clock stepped back further than the read's
/// margin — cannot hold all of it, and are not installed; the CONTROL is the same rows with the
/// session inside them.
#[test]
fn rows_that_start_after_the_session_are_not_installed() {
    let d = Dir::new("session-bound");
    let m = Mutex::new(ready(&d));
    engine_lock(&m).set_mode("fieldday-sp").expect("the switch");
    let mut e = m.into_inner().expect("not poisoned");
    let start = {
        let Mode::FieldDay { station, .. } = &e.mode else {
            panic!("in Field Day");
        };
        station.log.session.start_unix
    };
    let read = |bound: u64, e: &Engine| crate::logstore::SessionRows {
        rows: Vec::new(),
        bound,
        marks: e.station.marks(),
        exact: true,
    };
    assert!(
        !e.open_session_from(read(start + 1, &e)),
        "rows from after the session's start"
    );
    assert!(
        e.open_session_from(read(start, &e)),
        "CONTROL: rows from its start"
    );
}

/// ★ A store that does not answer in time — a big write holding its lock — is asked again,
/// briefly, and then the switch is REFUSED (SPEC-2 v3 C19, the operator's "retry briefly, then
/// refuse"): nothing was switched, and the command says the logbook was busy. The CONTROL: the
/// same switch once the store answers opens the session with its sweep.
#[test]
fn a_store_that_does_not_answer_in_time_refuses_the_switch_after_its_tries() {
    let d = Dir::new("session-busy");
    let mut e = ready(&d);
    let hold = WriteHold::take(&d.db()).expect("hold the write lock");
    // A contact the store cannot take while the lock is held: every read waits for it, and
    // gives up.
    e.log_qso(qso("W1AW", now_unix_secs() + 60));
    let m = Mutex::new(e);
    SESSION_READS.with(|c| c.set(0));
    assert_eq!(
        switch(&m, "fieldday-sp"),
        Err(SESSION_BUSY.to_string()),
        "refused: the rows could not be read"
    );
    // Counted, not timed: each read waits `SESSION_READ_WAIT` twice (1.0 s measured alone), and a
    // wall-clock bound on that fails under the full suite's load while nothing is wrong.
    assert_eq!(
        SESSION_READS.with(|c| c.get()),
        3,
        "read three times, then refused"
    );
    assert!(!engine_lock(&m).in_field_day(), "nothing was switched");
    drop(hold);
    assert_eq!(
        switch(&m, "fieldday-sp"),
        Ok(true),
        "CONTROL: once the store answers, the session opens with its sweep"
    );
    let mut e = m.into_inner().expect("not poisoned");
    master_on(&mut e);
    let b4 = e.session_b4().expect("a session is open");
    assert!(b4.worked_this_session("W1AW"));
}

/// ★ D4-A with a session open: another window's contact, logged after the session started,
/// reaches this window on its freshness poll — the hot index built again from the store, off the
/// lock, and the contest session's sweep opened again from whole rows (a build from the store
/// reads no contest exchange) — so the DUPE badge knows it.
#[test]
fn another_windows_contact_during_a_session_reaches_the_session_on_the_poll() {
    let d = Dir::new("session-d4a");
    let a = ready(&d);
    let mut b = engine_on_store(&d);
    let m = Mutex::new(a);
    assert_eq!(switch(&m, "fieldday-sp"), Ok(true), "opened with its sweep");
    master_on(&mut engine_lock(&m));
    b.log_qso(qso("W9DUR", now_unix_secs() + 60));
    flush(&b);
    assert!(crate::logstore::tests::eventually(
        || engine_lock(&m).log_store_foreign_pending()
    ));
    assert!(
        sync_shared_log(&m),
        "the other window's contact is taken in"
    );
    let e = engine_lock(&m);
    let b4 = e.session_b4().expect("a session is open");
    assert!(b4.worked_this_session("W9DUR"), "and it is this session's");
}

/// ★ The launch's restore of a session left running is bounded like the switch (the
/// coordinator's ruling): with the store's write lock held elsewhere and a contact stuck behind
/// it, so no read of the session's rows can be current, the launch reads three times — about
/// three seconds at worst — and answers `SESSION_NOT_RESUMED`: Field Day is not entered, and
/// the radio loop's start is held no longer. The CONTROL: once the store answers, the same
/// restore enters Field Day with its sweep.
#[test]
fn the_launch_gives_a_busy_log_three_tries_then_starts_without_the_session() {
    let d = Dir::new("session-launch-busy");
    let mut s = ready(&d).settings().clone();
    // The master left on, as the launch finds it: the setting, and Field Day not entered yet.
    s.fd_active = true;
    let mut e = Engine::with_settings(s);
    e.attach_log_store(crate::logstore::tests::open_fast(&d));
    assert!(
        e.restore_opens_session(),
        "premise: the launch would restore it"
    );
    let hold = WriteHold::take(&d.db()).expect("hold the write lock");
    e.log_qso(qso("W1AW", now_unix_secs() + 60));
    let m = Mutex::new(e);
    let restore = |m: &Mutex<Engine>| {
        with_session_rows_at_launch(
            m,
            |e| e.restore_opens_session(),
            |e, rows| {
                e.restore_field_day_if_enabled();
                rows.is_some_and(|r| e.open_session_from(r))
            },
        )
    };
    SESSION_READS.with(|c| c.set(0));
    let started = std::time::Instant::now();
    assert_eq!(
        restore(&m),
        Err(SESSION_NOT_RESUMED.to_string()),
        "the launch starts without the session, and says so"
    );
    // Counted, and bounded loosely: each read waits `SESSION_READ_WAIT` twice (1.0 s alone). The
    // shape it replaced could hold the start for ten tries of five seconds.
    assert_eq!(
        SESSION_READS.with(|c| c.get()),
        3,
        "three reads, as the switch makes"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "a moment, not a wait: {:?}",
        started.elapsed()
    );
    assert!(!engine_lock(&m).in_field_day(), "Field Day was not entered");
    drop(hold);
    assert_eq!(
        restore(&m),
        Ok(true),
        "CONTROL: once the store answers, the restore enters Field Day with its sweep"
    );
}
