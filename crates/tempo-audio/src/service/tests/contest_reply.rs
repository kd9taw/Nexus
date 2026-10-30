//! A logger's WSJT-X UDP Reply (GridTracker, JTAlert) to a CQ while a contest runs whose own
//! FT8/FT4 messages apply hands the station to the contest's S&P, its messages and its log, as the
//! operator's own double-click on that CQ does: WSJT-X's Reply is its double-click (`replyToCQ`,
//! `widgets/mainwindow.cpp` 13279, sets `m_bDoubleClicked` and calls `processMessage`).
//!
//! From FT8 QSO mode that opens the contest's session, whose rows are read off the Engine lock
//! (SPEC-2 v3 C19) that the radio loop's tick holds, so the loop makes that answer on a thread of
//! its own; in the contest's S&P, and outside a contest, the tick makes it as before. Here the
//! session reaches back an hour, to the contest's journal, past K9NEW, worked outside the contest
//! half an hour ago: a session opened without its rows stops a debug build at its first look.
//!
//! The loop is the real `RadioLoop::step` on a VOX rig and a mock sound card, the CQ decoded from a
//! real FT8 capture; the Reply lands late in the slot, too late for the first over to fit.

use super::decode_in_flight::{ft8_frame, reply_datagram};
use super::*;
use std::time::Instant;

/// A store-backed station on FT8 in `mode`, ARRL Field Day picked and Contest mode `on`, whose
/// contest journal holds a contact an hour old and whose log holds K9NEW, worked half an hour ago.
/// W1AW's CQ FD has been decoded. "Double-click on call sets Tx enable" is off: a logger's Reply
/// arms TX whatever it says, as WSJT-X's does.
fn station(tag: &str, on: bool, mode: &str) -> (std::path::PathBuf, Arc<Mutex<Engine>>) {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "nexus-contest-reply-{tag}-{nanos}-{}",
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("scratch");
    std::fs::write(dir.join("log.adi"), tempo_core::logbook::adif_header()).expect("log");
    let store = tempo_app::logstore::open(
        &dir.join("log.adi"),
        Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
        None,
    )
    .expect("the store opens");
    let now = (now_unix_ms() / 1000.0) as u64;
    let rec = |call: &str, when: u64| {
        let mut log = tempo_core::logbook::Logbook::new();
        log.import_adif(&format!(
            "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260910<TIME_ON:6>120000<EOR>",
            call.len()
        ));
        let mut r = tempo_core::logbook::QsoRecord::clone(&log.records()[0]);
        r.id = None;
        r.when_unix = when;
        r
    };
    let row = tempo_core::logbook::adif_record(&rec("K1JNL", now - 3_600));
    let at = row.rfind("<EOR>").expect("a record ends");
    let journal = format!(
        "{}{}<CLASS:2>2A<ARRL_SECT:3>EMA{}",
        tempo_core::logbook::adif_header(),
        &row[..at],
        &row[at..]
    );
    std::fs::write(dir.join("fd.adi"), journal).expect("the journal");
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.attach_log_store(store);
    e.set_fd_log_path(dir.join("fd.adi"));
    e.log_qso(rec("K9NEW", now - 1_800));
    e.flush_log_store(Duration::from_secs(30))
        .expect("the log is written");
    let mut s = e.settings().clone();
    s.fd_event = "arrlfd".into();
    s.fd_class = "1D".into();
    s.fd_section = "WI".into();
    s.fd_active = on;
    s.double_click_sets_tx = false;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e.set_mode("qso-monitor").expect("FT8 QSO mode");
    assert!(
        e.ingest(&ft8_frame("CQ FD W1AW FN31"), 3) >= 1,
        "premise: W1AW's CQ is decoded"
    );
    let engine = Arc::new(Mutex::new(e));
    if mode != "qso-monitor" {
        // Into the contest's S&P as its command enters it, with the session's rows.
        tempo_app::engine::act_opening_session(
            &engine,
            |e| e.mode_opens_session(mode),
            |e| e.set_mode(mode),
            |_| (),
        )
        .expect("the contest's S&P");
    }
    (dir, engine)
}

/// The station once a Reply is answered: the contest it is in, if any (Running, and the station it
/// works), FT8 QSO mode's station otherwise, and whether TX is armed.
type Answered = (Option<(bool, Option<String>)>, Option<String>, bool);

/// The station once a logger's Reply to W1AW's CQ FD has been answered.
fn after_a_reply(engine: &Arc<Mutex<Engine>>) -> Answered {
    let logger = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let server =
        WsjtxServer::new("127.0.0.1:0".parse().unwrap(), logger.local_addr().unwrap()).unwrap();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
    };
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state_for(engine);
    let (mut ra, mut reopen) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let slot = ((now_unix_ms() + 30_000.0) / 15_000.0).ceil() * 15_000.0;
    let mut tick = |now: f64| {
        state
            .step(
                engine,
                &mut backend,
                &mut rig,
                &sinks,
                now,
                &mut ra,
                &mut reopen,
                &mut station,
            )
            .unwrap();
    };
    tick(slot + 13_900.0);
    logger
        .send_to(
            &reply_datagram("CQ FD W1AW FN31"),
            server.local_addr().unwrap(),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    tick(slot + 13_920.0);
    // Answered once TX is armed: in the tick, or on the thread the tick handed it to.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !engine.lock().unwrap().tx_enabled() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let e = engine.lock().unwrap();
    let contest = e.snapshot().field_day.map(|fd| (fd.running, fd.dxcall));
    (contest, e.qso_dxcall().map(str::to_string), e.tx_enabled())
}

/// ⭐ A logger's Reply to a CQ, in a contest whose own messages apply, is the contest's S&P working
/// that station with TX armed, from the contest's S&P and from FT8 QSO mode alike, as the
/// operator's double-click is; with Contest mode off it is FT8 QSO mode's contact, as before.
#[test]
fn a_loggers_reply_to_a_cq_is_the_contests_sp() {
    let contest = (Some((false, Some("W1AW".to_string()))), None, true);
    let plain = (None, Some("W1AW".to_string()), true);
    for (tag, on, mode, want) in [
        ("sp", true, "fieldday-sp", contest.clone()),
        ("qso", true, "qso-monitor", contest),
        ("off", false, "qso-monitor", plain),
    ] {
        let (dir, engine) = station(tag, on, mode);
        assert_eq!(
            after_a_reply(&engine),
            want,
            "Contest mode {on}, from {mode}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
