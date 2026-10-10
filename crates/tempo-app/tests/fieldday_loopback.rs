//! Field Day club-sync integration — the whole stack over 127.0.0.1: real
//! engines, the REAL `fdbridge` impls, `fdsync::serve_until` and the real
//! position pumps. Loopback only, ephemeral ports, no real network.
//!
//! The scenario the design promises: a host + three positions (the host
//! itself is position #1, connected over its own loopback listener — "a host
//! is just another position"); a position dies mid-event, logs offline and
//! reconnects; the host restarts and replays its journal. At the end the
//! club log must equal the union of every position's contacts, and every
//! position's club-dupe set must have converged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_net::fdsync::{self, ClubBackend, PositionSync};

type Shared = Arc<Mutex<Engine>>;

/// A Field-Day-ready engine: master on, exchange set, S&P mode, posid set.
fn fd_engine(call: &str, posid: &str, name: &str, join_addr: &str) -> Shared {
    let mut e = Engine::new(call, "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = posid.into();
    s.fd_position_name = name.into();
    s.fd_join_addr = join_addr.into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("enter FD");
    Arc::new(Mutex::new(e))
}

/// Bind a loopback listener with SO_REUSEADDR (the host "restart" rebinds the
/// same port while accepted sockets may sit in TIME_WAIT).
fn reusable_listener(port: u16) -> std::net::TcpListener {
    let raw = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap();
    raw.set_reuse_address(true).unwrap();
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    raw.bind(&addr.into()).unwrap();
    raw.listen(16).unwrap();
    raw.into()
}

fn start_host(eng: &Shared, listener: std::net::TcpListener) -> Arc<AtomicBool> {
    let sd = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn ClubBackend> = Arc::new(EngineClubBackend(eng.clone()));
    let sd2 = sd.clone();
    std::thread::spawn(move || fdsync::serve_until(listener, backend, sd2));
    sd
}

fn start_pump(eng: &Shared, addr: &str) -> Arc<AtomicBool> {
    let sd = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn PositionSync> = Arc::new(EnginePositionSync(eng.clone()));
    let (a, sd2) = (addr.to_string(), sd.clone());
    std::thread::spawn(move || fdsync::run_position_until(&a, backend, sd2));
    sd
}

fn log_fd(eng: &Shared, call: &str, section: &str, mode: &str) {
    assert!(
        engine_lock(eng)
            .fd_log_manual(call, "2A", section, mode)
            .expect("FD mode active"),
        "{call} refused as an own-log dupe — fixture bug"
    );
}

fn wait_until(what: &str, secs: u64, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}");
}

fn club_rows(eng: &Shared) -> usize {
    engine_lock(eng)
        .fd_board_snapshot()
        .map(|b| b.rows.len())
        .unwrap_or(0)
}

fn mirror_dupes(eng: &Shared) -> std::collections::HashSet<(String, String, String)> {
    engine_lock(eng).fd_mirror_mut().dupes.clone()
}

#[test]
fn host_three_positions_outage_and_host_restart_converge_on_the_union() {
    let dir = std::env::temp_dir().join(format!("fd-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_test.jsonl");

    // An OS-assigned port, held from here on and bound again by the host restart.
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");

    // The host engine — ALSO position #1, joined over its own loopback
    // listener (zero special cases for the host's own contacts).
    let host = fd_engine("W9ABC", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);

    // Two more positions.
    let p2 = fd_engine("W9ABC", "bbbb0002", "CW tent", &addr);
    let p3 = fd_engine("W9ABC", "cccc0003", "SSB tent", &addr);
    let p2_pump_sd = start_pump(&p2, &addr);
    let p3_pump_sd = start_pump(&p3, &addr);

    // Contacts: 4 distinct keys + ONE cross-position dupe (p3 re-works the
    // host's W1AW on the same band+mode — merged raw, scored once).
    log_fd(&host, "W1AW", "CT", "CW");
    log_fd(&host, "K1ABC", "EMA", "PH");
    log_fd(&p2, "N0XYZ", "MN", "CW");
    log_fd(&p3, "W1AW", "CT", "CW"); // the cross-position dupe
    log_fd(&p3, "K5DEF", "STX", "DIG");

    wait_until("all 5 rows merged at the host", 10, || {
        club_rows(&host) == 5
    });
    {
        let eng = engine_lock(&host);
        let board = eng.fd_board_snapshot().unwrap();
        assert_eq!(board.rows.len(), 5, "raw union: every row kept");
        assert_eq!(board.positions.len(), 3, "three positions known");
        assert!(board.positions.iter().any(|p| p.label == "CW tent"));
    }
    // Every position's club dupe set converged to the same 4 unique keys.
    let expect_keys = 4;
    for (label, eng) in [("host", &host), ("p2", &p2), ("p3", &p3)] {
        wait_until(&format!("{label} mirror converged"), 10, || {
            mirror_dupes(eng).len() == expect_keys
        });
    }
    let host_keys = mirror_dupes(&host);
    assert_eq!(mirror_dupes(&p2), host_keys, "p2 sees the same club keys");
    assert_eq!(mirror_dupes(&p3), host_keys, "p3 sees the same club keys");
    // The chip is honest everywhere: everything acked → synced.
    for eng in [&host, &p2, &p3] {
        wait_until("synced", 10, || {
            engine_lock(eng).fd_sync_state() == tempo_app::fdevent::SyncState::Synced
        });
    }

    // --- kill p2, log offline, reconnect: the outbox re-streams the gap ---
    p2_pump_sd.store(true, Ordering::Relaxed);
    wait_until("p2 link down", 10, || {
        !engine_lock(&p2).fd_mirror_mut().connected
    });
    log_fd(&p2, "K9GHI", "IL", "PH"); // logged while offline — journal only
    {
        let eng = engine_lock(&p2);
        match eng.fd_sync_state() {
            tempo_app::fdevent::SyncState::Offline { queued, .. } => {
                assert_eq!(queued, 1, "the offline contact queues honestly")
            }
            other => panic!("expected Offline, got {other:?}"),
        }
    }
    let p2_pump_sd = start_pump(&p2, &addr);
    wait_until("p2's offline row reached the host", 10, || {
        club_rows(&host) == 6
    });

    // --- host restart: replay the journal, positions re-push for free ------
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600)); // accept loop notices ≤200 ms + conn teardown
    let host2 = fd_engine("W9ABC", "aaaa0001", "HQ", &addr);
    engine_lock(&host2).fd_host_start(journal.clone()).unwrap();
    assert_eq!(
        club_rows(&host2),
        6,
        "the journal replay alone rebuilds the whole club log"
    );
    let host2_sd = start_host(&host2, reusable_listener(port));
    let host2_pump_sd = start_pump(&host2, &addr);

    // The surviving pumps reconnect on backoff; one more contact proves the
    // reborn host is live end-to-end.
    log_fd(&p3, "N2JKL", "ENY", "CW");
    wait_until("the reborn host converges on the union", 20, || {
        club_rows(&host2) == 7
    });
    {
        let eng = engine_lock(&host2);
        let board = eng.fd_board_snapshot().unwrap();
        // The union: every (posid, seq) exactly once.
        let mut ids: Vec<(String, u64)> = board
            .rows
            .iter()
            .map(|r| (r.posid.clone(), r.seq))
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 7, "no id duplicated, none lost");
        assert_eq!(
            ids.iter().filter(|(p, _)| p == "aaaa0001").count(),
            2,
            "the old host's own contacts came back with the journal"
        );
    }
    // Dupe sets converge again across the restart (p2 + p3 against host2).
    wait_until("post-restart convergence", 20, || {
        let h = engine_lock(&host2).fd_club_counts().0;
        mirror_dupes(&p2).len() == h && mirror_dupes(&p3).len() == h
    });

    for sd in [host2_sd, host2_pump_sd, p2_pump_sd, p3_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The two callsign columns of each QSO line of the host's club Cabrillo — `(call sent,
/// call worked)` — sorted by the call worked (two pumps merge in either order).
fn club_file_calls(eng: &Shared) -> Vec<(String, String)> {
    let cab = engine_lock(eng)
        .fd_club_export(true)
        .expect("the host exports its club file");
    let mut calls: Vec<(String, String)> = cab
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(|l| {
            // QSO: freq mo date time SENT class section WORKED class section
            let cols: Vec<&str> = l.split_whitespace().collect();
            (cols[5].to_string(), cols[8].to_string())
        })
        .collect();
    calls.sort_by(|a, b| a.1.cmp(&b.1));
    calls
}

/// ⭐ **A GOTA position's contacts reach the club's Cabrillo under the GOTA station's own
/// call** — over the real bridge and sockets, and again after a host restart that replays
/// the journal once the GOTA position has gone. ARRL Field Day rule 4.1.1.1: the GOTA
/// station "must use a different callsign from the primary Field Day station". CONTROL: the
/// host's own contact keeps the club's call.
#[test]
fn a_gota_positions_contacts_keep_its_own_call_in_the_club_file_across_a_host_restart() {
    let dir = std::env::temp_dir().join(format!("fd-gota-loopback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_gota.jsonl");
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let gota = fd_engine("K9GOT", "cccc0003", "GOTA", &addr);
    let gota_sd = start_pump(&gota, &addr);
    log_fd(&host, "W1AW", "CT", "CW");
    log_fd(&gota, "K1ABC", "EMA", "PH");
    wait_until("both contacts merged at the host", 10, || {
        club_rows(&host) == 2
    });
    let expected = vec![
        ("K9GOT".to_string(), "K1ABC".to_string()),
        ("W9XYZ".to_string(), "W1AW".to_string()),
    ];
    assert_eq!(
        club_file_calls(&host),
        expected,
        "the GOTA tent's contact under its own call, the host's under the club's"
    );

    // The GOTA position packs up, then the host restarts: nothing joins it again.
    for sd in [gota_sd, host_sd, host_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(600));
    let host2 = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host2).fd_host_start(journal.clone()).unwrap();
    assert_eq!(
        club_rows(&host2),
        2,
        "the journal replay rebuilt the club log"
    );
    assert_eq!(
        club_file_calls(&host2),
        expected,
        "the GOTA call came back with the journal"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The name a position shows on the club board, as the HOST holds it.
fn board_label(eng: &Shared, posid: &str) -> String {
    engine_lock(eng)
        .fd_board_snapshot()
        .and_then(|b| b.positions.iter().find(|p| p.id == posid).cloned())
        .map(|p| p.label)
        .unwrap_or_default()
}

#[test]
fn renaming_a_position_reaches_the_club_board_on_the_live_connection() {
    // THE OPERATOR'S BUG (club Field Day, 2026-08-30): "I started with no
    // name, then added it, but it's still displaying the old name/number id."
    // The name travelled in the JOIN line and nowhere else, so a rename only
    // took effect when the connection was next rebuilt — and nothing said so.
    // End-to-end here (real engines, real bridge, real sockets) because the
    // fix spans the wire, the club log and the engine's identity seam.
    let dir = std::env::temp_dir().join(format!("fd-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");

    let host = fd_engine("W9ABC", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_rename.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let p2 = fd_engine("W9ABC", "bbbb0002", "CW tent", &addr);
    let p2_sd = start_pump(&p2, &addr);

    wait_until("the board knows the position by its name", 10, || {
        board_label(&host, "bbbb0002") == "CW tent"
    });
    // The operator renames it in Settings, mid-event. Nothing touches the
    // connection — that is the whole point.
    {
        let mut eng = engine_lock(&p2);
        let mut s = eng.settings().clone();
        s.fd_position_name = "GOTA tent".into();
        eng.apply_settings(s);
    }
    wait_until("the rename reached the board", 10, || {
        board_label(&host, "bbbb0002") == "GOTA tent"
    });
    assert!(
        engine_lock(&p2).fd_mirror_mut().connected,
        "on the SAME connection — a reconnect always carried the new name, \
         which is exactly the workaround the operator did not have"
    );

    for sd in [host_sd, p2_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A scripted host `ahead_ms` ahead of this PC, for one position. Its welcome's clock is IN
/// STEP with this PC's, so only the round trips can move the position's number; it answers
/// every timed ping with its stamps `ahead_ms` ahead, and keeps each presence line it hears.
fn scripted_host(
    listener: std::net::TcpListener,
    ahead_ms: u64,
    sd: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<String>> {
    use std::io::Write;
    std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = std::io::BufReader::new(s);
        let mut heard = Vec::new();
        while !sd.load(Ordering::Relaxed) {
            let line = match fdsync::read_capped_line(&mut r) {
                Ok(Some(l)) => l,
                Ok(None) => break,
                Err(_) => continue,
            };
            let reply = match fdsync::decode_line(&line) {
                Some(fdsync::Msg::Join { .. }) => fdsync::Msg::Welcome {
                    v: fdsync::PROTO_VERSION,
                    event: "TEST FD".into(),
                    host_call: "W9ABC".into(),
                    acked: 0,
                    now_unix: now_ms() / 1000,
                    contest: "arrlfd".into(),
                },
                Some(fdsync::Msg::Ping { t0 }) if t0 > 0 => {
                    let t1 = now_ms() + ahead_ms;
                    fdsync::Msg::Pong { t0, t1, t2: t1 }
                }
                Some(fdsync::Msg::Pos { .. }) => {
                    heard.push(line.trim_end().to_string());
                    continue;
                }
                _ => continue,
            };
            if w.write_all(fdsync::encode_line(&reply).as_bytes()).is_err() {
                break;
            }
        }
        heard
    })
}

/// ⭐ **A position measures its clock against the host's through the real pump and bridge**,
/// by value: the scripted host is 45 s ahead and its welcome says nothing of it, and the
/// position's own club block says 45 s behind as soon as a round trip closes, and its next
/// report tells the host so, in ms.
#[test]
fn a_position_measures_its_clock_against_the_host_through_the_real_pump() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let host_sd = Arc::new(AtomicBool::new(false));
    let host = scripted_host(listener, 45_000, host_sd.clone());
    let pos = fd_engine("W9ABC", "bbbb0002", "CW tent", &addr);
    let pump_sd = start_pump(&pos, &addr);
    let skew = || {
        engine_lock(&pos)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .map(|c| c.skew_secs)
    };
    wait_until("the position measured the host's clock", 5, || {
        skew() == Some(-45)
    });
    // The report that follows the measurement carries it.
    std::thread::sleep(Duration::from_millis(500));
    pump_sd.store(true, Ordering::Relaxed);
    host_sd.store(true, Ordering::Relaxed);
    let heard = host.join().unwrap();
    let reported: Vec<i64> = heard
        .iter()
        .filter_map(|l| match fdsync::decode_line(l) {
            Some(fdsync::Msg::Pos { clock_ms, .. }) => clock_ms,
            _ => None,
        })
        .collect();
    assert!(
        !reported.is_empty() && reported.iter().all(|ms| (ms + 45_000).abs() < 1_000),
        "the host heard this position is 45 s behind it: {heard:?}"
    );
}

/// The host's own board, through real engines and sockets: every position's row carries
/// the clock it measured. Here both run on one PC, so each is within a whisker of 0 (the
/// host's own position included: it joins itself over loopback). A position's board shows
/// the same rows with no clock: the column is the host's, and no board line carries it.
#[test]
fn the_hosts_board_carries_each_positions_measured_clock() {
    let dir = std::env::temp_dir().join(format!("fd-clock-board-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9ABC", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_test.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let p2 = fd_engine("W9ABC", "bbbb0002", "CW tent", &addr);
    let p2_sd = start_pump(&p2, &addr);
    let clocks = || -> Vec<(String, Option<i64>)> {
        engine_lock(&host)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .map(|c| c.board.into_iter().map(|r| (r.posid, r.clock_ms)).collect())
            .unwrap_or_default()
    };
    wait_until("both rows carry a measured clock", 10, || {
        let c = clocks();
        c.len() == 2 && c.iter().all(|(_, ms)| ms.is_some())
    });
    for (posid, ms) in clocks() {
        let ms = ms.unwrap();
        assert!(ms.abs() < 2_000, "{posid} runs on this PC's clock: {ms} ms");
    }
    let p2_board = || -> Vec<Option<i64>> {
        engine_lock(&p2)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .map(|c| c.board.into_iter().map(|r| r.clock_ms).collect())
            .unwrap_or_default()
    };
    wait_until("the position holds both rows", 10, || p2_board().len() == 2);
    assert_eq!(
        p2_board(),
        [None, None],
        "a position's board has no clock column"
    );
    for sd in [host_sd, host_pump_sd, p2_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A peer that speaks the wire by hand: send `lines`, then read what the host answers for
/// `ms`. Nexus never writes these lines; a LAN peer can.
fn raw_peer(addr: &str, lines: &[String], ms: u64) -> Vec<fdsync::Msg> {
    use std::io::Write;
    let s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = std::io::BufReader::new(s);
    for l in lines {
        w.write_all(l.as_bytes()).unwrap();
    }
    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        match fdsync::read_capped_line(&mut r) {
            Ok(Some(line)) => got.extend(fdsync::decode_line(&line)),
            Ok(None) => break,
            Err(_) => {}
        }
    }
    got
}

fn join_line(pos: &str, call: &str) -> String {
    fdsync::encode_line(&fdsync::Msg::Join {
        v: fdsync::PROTO_VERSION,
        pos: pos.into(),
        name: "TENT".into(),
        call: call.into(),
        max_seq: 0,
        contest: "arrlfd".into(),
        role: String::new(),
    })
}

fn row_line(pos: &str, seq: u64, call: &str) -> String {
    fdsync::encode_line(&fdsync::Msg::Qso(fdsync::WireQso {
        pos: pos.into(),
        seq,
        call: call.into(),
        class: "2A".into(),
        sect: "EMA".into(),
        band: "40m".into(),
        mode: "CW".into(),
        when: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        ..Default::default()
    }))
}

fn told(got: &[fdsync::Msg], sentence: &str) -> bool {
    got.iter()
        .any(|m| matches!(m, fdsync::Msg::Error { msg } if msg.as_str() == sentence))
}

/// ⭐ **A LAN peer let in as one position cannot send contacts as another** — over the real
/// bridge and sockets, at an ARRL Field Day club. The host's gates decide on the JOIN, and the
/// merge used to go by the row's own position id: a peer let in on its own id that sent a row
/// as the HOST's own position took that position's next sequence number, so the host's next
/// real contact merged as a repeat and never reached the club's file, with nothing on screen.
/// Refused by name now. CONTROL: the same peer's own contact merges.
#[test]
fn a_lan_peer_cannot_send_contacts_as_another_position() {
    let dir = std::env::temp_dir().join(format!("fd-another-pos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_another.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    log_fd(&host, "W1AW", "CT", "CW");
    wait_until("the host's own contact merged", 10, || {
        club_rows(&host) == 1
    });

    let got = raw_peer(
        &addr,
        &[
            join_line("dddd0004", "K9GOT"),
            row_line("aaaa0001", 2, "K1ABC"),
        ],
        800,
    );
    log_fd(&host, "N0XYZ", "MN", "CW");
    let peer_own = raw_peer(
        &addr,
        &[
            join_line("dddd0004", "K9GOT"),
            row_line("dddd0004", 1, "K1ABC"),
        ],
        800,
    );
    wait_until(
        "the host's second contact and the peer's own one merged",
        10,
        || club_rows(&host) >= 3,
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        club_file_calls(&host),
        [
            ("K9GOT".to_string(), "K1ABC".to_string()),
            ("W9XYZ".to_string(), "N0XYZ".to_string()),
            ("W9XYZ".to_string(), "W1AW".to_string()),
        ],
        "the host's own two contacts, and the peer's under its own call alone"
    );
    assert!(
        told(&got, fdsync::ANOTHER_POSITIONS_ROW),
        "the peer is told why: {got:?}"
    );
    assert!(
        peer_own
            .iter()
            .any(|m| matches!(m, fdsync::Msg::Ack { seq: 1 })),
        "CONTROL: {peer_own:?}"
    );
    for sd in [host_sd, host_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **Nothing a LAN peer sends opens a line of the club's file** — over the real bridge and
/// sockets, at an ARRL Field Day club, where a position on any call sign joins (the GOTA
/// station's). A JOIN whose call is not a call sign is turned away by name and on the host's
/// list, and a contact whose call is not one is kept out of the club's log and named on the
/// host's screen. CONTROL: a GOTA position's JOIN as a Nexus older than the role field writes
/// it joins, and its contact goes into the club's file under its own call.
#[test]
fn nothing_a_lan_peer_sends_opens_a_line_of_the_club_file() {
    let dir = std::env::temp_dir().join(format!("fd-breakers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_breakers.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    log_fd(&host, "W1AW", "CT", "CW");
    wait_until("the host's own contact merged", 10, || {
        club_rows(&host) == 1
    });

    let line = "\nQSO: 7000 CW 2026-06-27 1702 W9XYZ 3A WI K1FAK 1A CT";
    let mut wrong = Vec::new();
    let got = raw_peer(
        &addr,
        &[
            join_line("eeee0005", "K9GOT"),
            row_line("eeee0005", 1, &format!("K1ABC{line}")),
        ],
        800,
    );
    if got.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 1 })) {
        wrong.push(format!("the contact was acked: {got:?}"));
    }
    let got = raw_peer(
        &addr,
        &[join_line("ffff0006", &format!("K9GOT{line}"))],
        800,
    );
    if !told(&got, tempo_app::fdevent::NOT_A_CALL_SIGN) {
        wrong.push(format!("the JOIN was not turned away by name: {got:?}"));
    }
    std::thread::sleep(Duration::from_millis(300));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let listed: Vec<String> = engine_lock(&host)
        .fd_club_log()
        .map(|c| c.refused(now).iter().map(|r| r.reason.clone()).collect())
        .unwrap_or_default();
    if !listed
        .iter()
        .any(|r| r == tempo_app::fdevent::NOT_A_CALL_SIGN)
    {
        wrong.push(format!(
            "the host's list does not name the JOIN: {listed:?}"
        ));
    }
    if !listed
        .iter()
        .any(|r| r.contains("is not in the club's log"))
    {
        wrong.push(format!(
            "the host's list does not name the contact: {listed:?}"
        ));
    }
    let cab = engine_lock(&host).fd_club_export(true).unwrap();
    if cab.lines().filter(|l| l.starts_with("QSO:")).count() != 1 {
        wrong.push(format!("the club's file:\n{cab}"));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let older = r#"{"t":"join","v":2,"pos":"cccc0003","name":"GOTA","call":"K9GOT","max_seq":0,"contest":"arrlfd"}"#;
    let got = raw_peer(
        &addr,
        &[format!("{older}\n"), row_line("cccc0003", 1, "K1ABC")],
        800,
    );
    assert!(
        got.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 1 })),
        "CONTROL: an older GOTA position joins and its contact merges: {got:?}"
    );
    assert_eq!(
        club_file_calls(&host),
        [
            ("K9GOT".to_string(), "K1ABC".to_string()),
            ("W9XYZ".to_string(), "W1AW".to_string()),
        ]
    );
    for sd in [host_sd, host_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
