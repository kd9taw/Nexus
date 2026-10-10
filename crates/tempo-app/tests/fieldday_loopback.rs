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

/// 64 hex digits for `tag`, generated (a per-run seed through the operating system's hasher):
/// the same all through one run, and never a real key or hash.
fn throwaway_hex(tag: &str) -> String {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(std::hash::RandomState::new);
    (0..4u8)
        .map(|i| format!("{:016x}", seed.hash_one((tag, i))))
        .collect()
}

/// The club key a position `posid` sends this run: the same through its restarts, as a
/// laptop's own key is (the shell keeps it beside settings.json).
fn club_key(posid: &str) -> fdsync::PositionKey {
    fdsync::PositionKey::new(throwaway_hex(posid))
}

/// The host bridge's key hash for this run, where the shell gives it SHA-256: one way, so a
/// search of what the host keeps for a key can tell the two apart.
fn test_hash(secret: &str) -> String {
    throwaway_hex(&format!("hash of {secret}"))
}

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
    e.set_fd_position_key(club_key(posid));
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
    let backend: Arc<dyn ClubBackend> = Arc::new(EngineClubBackend(eng.clone(), test_hash));
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
        key: club_key(pos),
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
/// host's screen. A GOTA position's JOIN as an older Nexus writes it (1.17's bytes, no club
/// key) is turned away by name, and nothing it sends merges. CONTROL: the same GOTA position
/// on this build joins, and its contact goes into the club's file under its own call.
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
    let kept: Vec<String> = engine_lock(&host)
        .fd_club_log()
        .map(|c| c.kept_out().iter().map(|k| k.reason.clone()).collect())
        .unwrap_or_default();
    if !kept.iter().any(|r| r.contains("is not in the club's log")) {
        wrong.push(format!(
            "the host's kept-out list does not name the contact: {kept:?}"
        ));
    }
    if listed
        .iter()
        .any(|r| r.contains("is not in the club's log"))
    {
        wrong.push(format!("the turned-away list names a contact: {listed:?}"));
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
    let update = engine_lock(&host)
        .fd_club_log()
        .and_then(|c| c.version_refusal(2))
        .expect("a v2 JOIN is refused by its version");
    assert!(
        told(&got, &update),
        "an older Nexus is told to update: {got:?}"
    );
    assert!(
        !got.iter().any(|m| matches!(m, fdsync::Msg::Ack { .. })),
        "nothing it sent merged: {got:?}"
    );
    let got = raw_peer(
        &addr,
        &[
            join_line("cccc0003", "K9GOT"),
            row_line("cccc0003", 1, "K1ABC"),
        ],
        800,
    );
    assert!(
        got.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 1 })),
        "CONTROL: the GOTA position on this build joins and its contact merges: {got:?}"
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

/// ⭐ **A LAN peer cannot join as another laptop's position** — over the real bridge and
/// sockets, at an ARRL Field Day club. A position id rides every board line, so any peer can
/// read one off the board; with another key than the one that position first joined with, its
/// JOIN is turned away by name before it can send a contact as that position, and the host's
/// screen names it. The position's own laptop keeps syncing, and rejoins after a host restart,
/// when the peer is turned away still. Nothing the host keeps or shows holds a key: its journal
/// keeps the hash, and the snapshot every screen and Remote read, the TV's board and Settings
/// hold none. CONTROL: the JOIN line itself carries the key, so the searches find it there.
#[test]
fn a_lan_peer_cannot_join_as_another_laptops_position() {
    let dir = std::env::temp_dir().join(format!("fd-held-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_held.jsonl");
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let tent = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    let tent_sd = start_pump(&tent, &addr);
    log_fd(&tent, "K1ABC", "EMA", "CW");
    wait_until("the tent's contact merged", 10, || club_rows(&host) == 1);

    let peer_key = club_key("a peer that read bbbb0002 off the board");
    let peer_join = fdsync::encode_line(&fdsync::Msg::Join {
        v: fdsync::PROTO_VERSION,
        pos: "bbbb0002".into(),
        name: "CW tent".into(),
        call: "W9XYZ".into(),
        max_seq: 0,
        contest: "arrlfd".into(),
        role: String::new(),
        key: peer_key.clone(),
    });
    let got = raw_peer(
        &addr,
        &[peer_join.clone(), row_line("bbbb0002", 2, "W1FAK")],
        800,
    );
    let mut wrong = Vec::new();
    if !told(&got, tempo_app::fdevent::POSITION_HELD) {
        wrong.push(format!("the peer was not told why: {got:?}"));
    }
    if got.iter().any(|m| matches!(m, fdsync::Msg::Ack { .. })) {
        wrong.push(format!("the peer's contact was acked: {got:?}"));
    }
    log_fd(&tent, "N0XYZ", "MN", "CW");
    wait_until("the tent's second contact merged", 10, || {
        club_rows(&host) == 2
    });
    let calls = club_file_calls(&host);
    if calls
        != [
            ("W9XYZ".to_string(), "K1ABC".to_string()),
            ("W9XYZ".to_string(), "N0XYZ".to_string()),
        ]
    {
        wrong.push(format!("the club's file: {calls:?}"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let listed: Vec<String> = engine_lock(&host)
        .fd_club_log()
        .map(|c| c.refused(now).iter().map(|r| r.reason.clone()).collect())
        .unwrap_or_default();
    if listed != [tempo_app::fdevent::POSITION_HELD] {
        wrong.push(format!("the host's list: {listed:?}"));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // The host restarts on the same event's journal: the same pins.
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    engine_lock(&host).fd_host_stop();
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, reusable_listener(port));
    let host_pump_sd = start_pump(&host, &addr);
    log_fd(&tent, "W5DEF", "STX", "CW");
    wait_until("the tent rejoined the restarted host", 30, || {
        club_rows(&host) == 3
    });
    let got = raw_peer(&addr, std::slice::from_ref(&peer_join), 800);
    assert!(
        told(&got, tempo_app::fdevent::POSITION_HELD),
        "the peer is still turned away after the restart: {got:?}"
    );

    // Where the keys are, and where they are not.
    let keys = [club_key("aaaa0001"), club_key("bbbb0002"), peer_key];
    assert!(
        peer_join.contains(keys[2].secret()),
        "CONTROL: the JOIN line carries it"
    );
    let kept = std::fs::read_to_string(&journal).unwrap();
    assert!(
        kept.contains(&test_hash(keys[1].secret())),
        "the journal pins the tent by its key's hash"
    );
    let shown = {
        let h = engine_lock(&host);
        let t = engine_lock(&tent);
        let board = h.fd_board_snapshot().unwrap();
        vec![
            ("the host's journal", kept),
            (
                "the host's snapshot",
                serde_json::to_string(&h.snapshot()).unwrap(),
            ),
            (
                "the tent's snapshot",
                serde_json::to_string(&t.snapshot()).unwrap(),
            ),
            (
                "the host's Settings",
                serde_json::to_string(h.settings()).unwrap(),
            ),
            (
                "the tent's Settings",
                serde_json::to_string(t.settings()).unwrap(),
            ),
            (
                "the TV's board",
                tempo_app::fd_scoreboard::build_data_core(&board, now)
                    + &tempo_app::fd_scoreboard::build_meta(&board, now),
            ),
        ]
    };
    for (what, text) in &shown {
        for key in &keys {
            assert!(!text.contains(key.secret()), "{what} holds a club key");
        }
    }
    for sd in [host_sd, host_pump_sd, tent_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The position's own club line: the sentence its host last sent it, if any.
fn last_error(eng: &Shared) -> Option<String> {
    engine_lock(eng).fd_mirror_mut().last_error.clone()
}

/// The worked calls of the host's club file, sorted.
fn worked(eng: &Shared) -> Vec<String> {
    club_file_calls(eng).into_iter().map(|(_, w)| w).collect()
}

/// The handles the host's own screen shows a Give button for, from the snapshot it reads.
fn give_handles(eng: &Shared) -> Vec<u64> {
    engine_lock(eng)
        .snapshot()
        .field_day
        .and_then(|f| f.club)
        .map(|c| c.refused.iter().filter_map(|r| r.handle).collect())
        .unwrap_or_default()
}

/// [`wait_until`], naming what it saw when the wait runs out: `show` reads the state.
fn wait_showing(what: &str, secs: u64, mut cond: impl FnMut() -> bool, show: impl Fn() -> String) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}\nsaw: {}", show());
}

/// ⭐ **The host gives a club position to the laptop it belongs to, and gives it back the same
/// way** — over the real bridge, sockets, pumps and engines, at an ARRL Field Day club.
///
/// A laptop whose settings came from the CW tent's (the tent's position id, a club key of its
/// own) joins first and sends two contacts. The tent, with two of its own numbered 1 and 2 as
/// the copy's are, is turned away by name on its screen and the host's, and its entry there
/// carries a Give button. The host presses it: the copy is closed while it sends nothing and
/// turned away by name; a contact it logs afterwards is never acked; the tent is served on its
/// next try and both its contacts are in the club's file, the copy's out of it. The copy's entry
/// then carries the button, and a press gives the position back with all three of the copy's
/// contacts; a third press gives it to the tent again. A restarted host replays the three gives
/// in order: the tent holds the position, with its contacts, and the copy is turned away still.
/// No club key or key hash is in any snapshot, board, TV payload, Remote's source, Settings or the
/// diagnostic log; the journal holds the hashes. CONTROLS: the JOIN line carries a key, the
/// journal the hashes, and the diagnostic log a line written to it, so each search can find one.
#[test]
fn the_host_gives_a_held_position_to_the_laptop_it_belongs_to() {
    let dir = std::env::temp_dir().join(format!("fd-give-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    tempo_core::applog::init(dir.join("nexus-diag.log"));
    let journal = dir.join("fd_event_give.jsonl");
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let mut host_sd = start_host(&host, listener);
    let mut host_pump_sd = start_pump(&host, &addr);
    let held = Some(tempo_app::fdevent::POSITION_HELD.to_string());
    let press = |handles: Vec<u64>| {
        assert_eq!(
            handles.len(),
            1,
            "one laptop held out of its position: {handles:?}"
        );
        engine_lock(&host)
            .fd_club_give_position(handles[0])
            .expect("the host gives the position");
    };

    let tent_key = club_key("bbbb0002");
    let short = |e: Option<String>| match e.as_deref() {
        None => "none".to_string(),
        Some(t) if Some(t) == held.as_deref() => "POSITION_HELD".to_string(),
        Some(t) => t.chars().take(60).collect(),
    };
    let copy = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    let copy_key = club_key("a laptop with the CW tent's settings");
    engine_lock(&copy).set_fd_position_key(copy_key.clone());
    let copy_sd = start_pump(&copy, &addr);
    log_fd(&copy, "K1ABC", "EMA", "CW");
    log_fd(&copy, "N0XYZ", "MN", "CW");
    wait_until("the copy's two contacts merged", 10, || {
        club_rows(&host) == 2
    });
    let tent = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    log_fd(&tent, "W5DEF", "STX", "CW");
    log_fd(&tent, "K7GHI", "OR", "CW");
    let tent_sd = start_pump(&tent, &addr);
    let show = || {
        let holder = engine_lock(&host)
            .fd_club_log()
            .and_then(|c| c.pinned("bbbb0002").map(str::to_string));
        format!(
            "club file {:?}; holder {}; tent told {}; copy told {}; copy acked {}; buttons {:?}",
            worked(&host),
            match holder {
                Some(h) if h == test_hash(tent_key.secret()) => "the tent",
                Some(h) if h == test_hash(copy_key.secret()) => "the copy",
                Some(_) => "another",
                None => "none",
            },
            short(last_error(&tent)),
            short(last_error(&copy)),
            engine_lock(&copy).fd_mirror_mut().acked,
            give_handles(&host),
        )
    };
    wait_showing(
        "the tent turned away by name",
        10,
        || last_error(&tent) == held,
        show,
    );
    wait_showing(
        "the host's list gives the tent a button",
        10,
        || give_handles(&host).len() == 1,
        show,
    );

    press(give_handles(&host));
    wait_showing(
        "the copy closed and turned away by name",
        10,
        || last_error(&copy) == held,
        show,
    );
    log_fd(&copy, "W1FAK", "CT", "CW");
    wait_showing(
        "the tent's contacts are the club's, and only those",
        30,
        || worked(&host) == ["K7GHI", "W5DEF"],
        show,
    );
    wait_showing(
        "the host's list gives the copy a button",
        30,
        || give_handles(&host).len() == 1,
        show,
    );
    let copy_acked = engine_lock(&copy).fd_mirror_mut().acked;
    assert_eq!(
        copy_acked, 2,
        "nothing the copy sent after the press was acked"
    );
    assert_eq!(last_error(&tent), None, "the tent is served");

    press(give_handles(&host));
    wait_showing(
        "the tent closed and turned away by name",
        10,
        || last_error(&tent) == held,
        show,
    );
    wait_showing(
        "the copy's three contacts are the club's, and only those",
        30,
        || worked(&host) == ["K1ABC", "N0XYZ", "W1FAK"],
        show,
    );
    wait_showing(
        "the host's list gives the tent a button again",
        30,
        || give_handles(&host).len() == 1,
        show,
    );
    press(give_handles(&host));
    wait_showing(
        "the copy turned away again",
        10,
        || last_error(&copy) == held,
        show,
    );
    wait_showing(
        "the tent's contacts are the club's again",
        30,
        || worked(&host) == ["K7GHI", "W5DEF"],
        show,
    );

    // The host restarts on the same event's journal.
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    engine_lock(&host).fd_host_stop();
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    assert_eq!(
        worked(&host),
        ["K7GHI", "W5DEF"],
        "the replayed club log, before anyone rejoins"
    );
    host_sd = start_host(&host, reusable_listener(port));
    host_pump_sd = start_pump(&host, &addr);
    log_fd(&tent, "W0JKL", "CO", "CW");
    wait_showing(
        "the tent rejoined the restarted host as its position",
        30,
        || worked(&host) == ["K7GHI", "W0JKL", "W5DEF"],
        show,
    );
    wait_showing(
        "the copy turned away by the restarted host",
        30,
        || give_handles(&host).len() == 1,
        show,
    );

    // Where the keys and their hashes are, and where they are not.
    let keys = [club_key("aaaa0001"), club_key("bbbb0002"), copy_key];
    let hashes: Vec<String> = keys.iter().map(|k| test_hash(k.secret())).collect();
    let join_line = fdsync::encode_line(&fdsync::Msg::Join {
        v: fdsync::PROTO_VERSION,
        pos: "bbbb0002".into(),
        name: "CW tent".into(),
        call: "W9XYZ".into(),
        max_seq: 0,
        contest: "arrlfd".into(),
        role: String::new(),
        key: keys[2].clone(),
    });
    assert!(
        join_line.contains(keys[2].secret()),
        "CONTROL: the JOIN line carries the key"
    );
    let kept = std::fs::read_to_string(&journal).unwrap();
    assert!(
        kept.contains(&hashes[1]) && kept.contains(&hashes[2]),
        "CONTROL: the journal pins and gives by the keys' hashes"
    );
    let marker = format!("fd-give-{}", std::process::id());
    tempo_core::applog::info("fd-give-test", &marker);
    tempo_core::applog::flush();
    let diag = tempo_core::applog::path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    assert!(
        diag.contains(&marker),
        "CONTROL: the diagnostic log holds a line written to it"
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut shown = vec![("the diagnostic log".to_string(), diag)];
    for (who, eng) in [("host", &host), ("tent", &tent), ("copy", &copy)] {
        let e = engine_lock(eng);
        shown.push((
            format!("the {who}'s snapshot"),
            serde_json::to_string(&e.snapshot()).unwrap(),
        ));
        shown.push((
            format!("the {who}'s Settings"),
            serde_json::to_string(e.settings()).unwrap(),
        ));
        shown.push((
            format!("the {who}'s Field Day status Remote reads"),
            serde_json::to_string(&e.bounded_field_day_status().unwrap()).unwrap(),
        ));
    }
    {
        let board = engine_lock(&host).fd_board_snapshot().unwrap();
        shown.push((
            "the TV's board".to_string(),
            tempo_app::fd_scoreboard::build_data_core(&board, now)
                + &tempo_app::fd_scoreboard::build_meta(&board, now),
        ));
    }
    for (what, text) in &shown {
        for (key, hash) in keys.iter().zip(&hashes) {
            assert!(!text.contains(key.secret()), "{what} holds a club key");
            assert!(!text.contains(hash.as_str()), "{what} holds a key's hash");
        }
    }
    assert!(
        !kept.contains(keys[1].secret()),
        "the journal holds a club key"
    );
    for sd in [host_sd, host_pump_sd, tent_sd, copy_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
