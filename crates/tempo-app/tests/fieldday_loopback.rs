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
    let key = club_key(posid);
    let hash = test_hash(key.secret());
    e.set_fd_position_key(key, hash);
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
    set_key(&copy, copy_key.clone());
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

/// This laptop's club key, handed to the engine as the shell hands it at startup: with its hash
/// as this run's host bridge makes one.
fn set_key(eng: &Shared, key: fdsync::PositionKey) {
    let hash = test_hash(key.secret());
    engine_lock(eng).set_fd_position_key(key, hash);
}

/// A station's club block as its own screen reads it: the snapshot's, as JSON.
fn club_json(eng: &Shared) -> serde_json::Value {
    serde_json::to_value(engine_lock(eng).snapshot().field_day.and_then(|f| f.club)).unwrap()
}

/// The host's turned-away entries as its screen shows them: all an entry carries but the handle
/// its button sends back.
fn shown_refused(eng: &Shared) -> Vec<serde_json::Value> {
    let club = club_json(eng);
    club["refused"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut v| {
            v.as_object_mut().unwrap().remove("handle");
            v
        })
        .collect()
}

/// The handle of the host's entry whose club code is `code`.
fn handle_with_code(eng: &Shared, code: &str) -> Option<u64> {
    club_json(eng)["refused"]
        .as_array()?
        .iter()
        .find(|v| v["clubCode"].as_str() == Some(code))
        .and_then(|v| v["handle"].as_u64())
}

/// The position names a board's rows carry, sorted.
fn board_names(board: &serde_json::Value, key: &str) -> Vec<String> {
    let mut names: Vec<String> = board
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| r[key].as_str().unwrap_or("").to_string())
        .collect();
    names.sort();
    names
}

/// ⭐ **Two laptops that name one position alike are told apart on the host's screen, and a give
/// the host was tricked into is undone by value** — over the real bridge, sockets, pumps and
/// engines, at an ARRL Field Day club.
///
/// The CW tent sends two contacts, then loses its key file and is turned away by the host that
/// pinned the old key. A LAN peer names the same position with the tent's name and call and a key
/// of its own, and is turned away too. The host's list then has two entries that read the same in
/// everything the laptops chose, each with a Give button. Each laptop's own club line shows its
/// club code, and the host's entry for it shows the same code, so the host can tell which entry
/// is the tent. A press on the peer's entry (the host tricked) takes the tent's contacts out of
/// the club's file; a press on the tent's gives it back, and the club's file holds the tent's
/// contacts again. No club code is on the TV, in the journal or in Settings. CONTROL: the host's
/// snapshot carries both codes, so the search can find one.
#[test]
fn two_laptops_naming_one_position_alike_are_told_apart_and_a_wrong_give_is_undone() {
    let dir = std::env::temp_dir().join(format!("fd-codes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_codes.jsonl");
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let tent = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    let first_sd = start_pump(&tent, &addr);
    log_fd(&tent, "K1ABC", "EMA", "CW");
    log_fd(&tent, "N0XYZ", "MN", "CW");
    wait_until("the tent's two contacts merged", 10, || {
        worked(&host) == ["K1ABC", "N0XYZ"]
    });

    // The tent loses its key file, and starts again with a key of its own.
    first_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    set_key(&tent, club_key("the CW tent, after it lost its key file"));
    let tent_sd = start_pump(&tent, &addr);
    // A LAN peer names the tent's position, with its name and call, and a key of its own.
    let peer = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    set_key(&peer, club_key("a laptop that read bbbb0002 off the board"));
    log_fd(&peer, "W1FAK", "CT", "CW");
    let peer_sd = start_pump(&peer, &addr);
    let held = Some(tempo_app::fdevent::POSITION_HELD.to_string());
    let show = || {
        format!(
            "club file {:?}; tent told {:?}; peer told {:?}; host's entries {:?}",
            worked(&host),
            last_error(&tent).map(|e| e.chars().take(40).collect::<String>()),
            last_error(&peer).map(|e| e.chars().take(40).collect::<String>()),
            shown_refused(&host),
        )
    };
    wait_showing(
        "both laptops turned away, each with a button on the host's list",
        20,
        || last_error(&tent) == held && last_error(&peer) == held && give_handles(&host).len() == 2,
        show,
    );
    let entries = shown_refused(&host);
    assert_ne!(
        entries[0], entries[1],
        "the host's screen tells the two laptops apart"
    );
    // What the host's own screen read while both were on its list.
    let host_snapshot = serde_json::to_string(&engine_lock(&host).snapshot()).unwrap();
    let code = |eng: &Shared| {
        club_json(eng)["clubCode"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let (tent_code, peer_code) = (code(&tent), code(&peer));
    assert!(
        !tent_code.is_empty() && tent_code != peer_code,
        "each laptop's own club line shows its own code: {tent_code:?}, {peer_code:?}"
    );
    let mut codes: Vec<String> = entries
        .iter()
        .map(|e| e["clubCode"].as_str().unwrap_or("").to_string())
        .collect();
    codes.sort();
    let mut want = vec![tent_code.clone(), peer_code.clone()];
    want.sort();
    assert_eq!(
        codes, want,
        "the host's entries show the codes the laptops show"
    );

    // The host is tricked: it presses the peer's entry.
    let peers = handle_with_code(&host, &peer_code).unwrap();
    engine_lock(&host).fd_club_give_position(peers).unwrap();
    wait_showing(
        "the tent's contacts are out of the club's file, the peer's in",
        30,
        || worked(&host) == ["W1FAK"] && last_error(&peer).is_none(),
        show,
    );
    // …and undoes it on the tent's entry, the one with the tent's own code.
    wait_showing(
        "the tent's entry still has its button",
        20,
        || handle_with_code(&host, &tent_code).is_some(),
        show,
    );
    let tents = handle_with_code(&host, &tent_code).unwrap();
    engine_lock(&host).fd_club_give_position(tents).unwrap();
    wait_showing(
        "the club's file holds the tent's contacts again, and only those",
        30,
        || worked(&host) == ["K1ABC", "N0XYZ"] && last_error(&tent).is_none(),
        show,
    );
    wait_showing(
        "the peer is turned away again",
        15,
        || last_error(&peer) == held,
        show,
    );

    // Where the codes are, and where they are not.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let board = engine_lock(&host).fd_board_snapshot().unwrap();
    let elsewhere = [
        (
            "the TV's board",
            tempo_app::fd_scoreboard::build_data_core(&board, now)
                + &tempo_app::fd_scoreboard::build_meta(&board, now),
        ),
        (
            "the host's journal",
            std::fs::read_to_string(&journal).unwrap(),
        ),
        (
            "the host's Settings",
            serde_json::to_string(engine_lock(&host).settings()).unwrap(),
        ),
        (
            "the tent's Settings",
            serde_json::to_string(engine_lock(&tent).settings()).unwrap(),
        ),
    ];
    for code in [&tent_code, &peer_code] {
        assert!(
            host_snapshot.contains(code.as_str()),
            "CONTROL: the host's own screen shows {code}"
        );
        for (what, text) in &elsewhere {
            assert!(!text.contains(code.as_str()), "{what} holds a club code");
        }
    }
    for sd in [host_sd, host_pump_sd, tent_sd, peer_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A peer cannot pass as another position by name on any screen the host's board reaches**
/// — over the real bridge, sockets and pumps, at an ARRL Field Day club. The CW tent joins first.
/// A LAN peer joins as a position of its own named "CW tent" too, and sends one contact the club
/// takes and one it keeps out; a second peer names itself what the first one's line reads. The
/// host's board, the tent's board (the lines every position is sent), the TV's board, Remote's
/// source and the host's kept-out list each tell every one of them apart, and the tent, which the
/// host heard first, keeps its own name.
#[test]
fn a_peer_cannot_pass_as_another_position_by_name_on_any_board() {
    let dir = std::env::temp_dir().join(format!("fd-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_names.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let tent = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    let tent_sd = start_pump(&tent, &addr);
    log_fd(&tent, "K1ABC", "EMA", "CW");
    wait_until("the tent's contact merged", 10, || {
        worked(&host) == ["K1ABC"]
    });
    let join_as = |pos: &str, name: &str| {
        fdsync::encode_line(&fdsync::Msg::Join {
            v: fdsync::PROTO_VERSION,
            pos: pos.into(),
            name: name.into(),
            call: "W9XYZ".into(),
            max_seq: 0,
            contest: "arrlfd".into(),
            role: String::new(),
            key: club_key(pos),
        })
    };
    let got = raw_peer(
        &addr,
        &[
            join_as("cccc0003", "CW tent"),
            row_line("cccc0003", 1, "W1FAK"),
            row_line("cccc0003", 2, "K1A\u{200B}BC"),
        ],
        800,
    );
    assert!(
        got.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 1 })),
        "CONTROL: the peer is a position of its own, and its contact merged: {got:?}"
    );
    raw_peer(&addr, &[join_as("dddd0004", "CW tent (2)")], 500);
    let want = ["CW tent", "CW tent (2)", "CW tent (2) (2)", "HQ"];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let boards = || {
        let tv: serde_json::Value = {
            let board = engine_lock(&host).fd_board_snapshot().unwrap();
            serde_json::from_str(&tempo_app::fd_scoreboard::build_data_core(&board, now)).unwrap()
        };
        let remote =
            serde_json::to_value(engine_lock(&host).bounded_field_day_status().unwrap()).unwrap();
        vec![
            (
                "the host's board",
                board_names(&club_json(&host)["board"], "posName"),
            ),
            (
                "the tent's board",
                board_names(&club_json(&tent)["board"], "posName"),
            ),
            ("the TV's board", board_names(&tv["positions"], "label")),
            (
                "Remote's source",
                board_names(&remote["club"]["board"], "posName"),
            ),
        ]
    };
    wait_showing(
        "every board names the four positions apart",
        15,
        || boards().iter().all(|(_, names)| names == &want),
        || format!("{:?}", boards()),
    );
    let kept: Vec<String> = club_json(&host)["keptOut"]["latest"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|k| k["posName"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(
        kept,
        ["CW tent (2)"],
        "the host's kept-out list names the peer as its board does"
    );
    for sd in [host_sd, host_pump_sd, tent_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A LAN peer cannot take the host's own club position before the host's laptop joins its
/// own club** — over the real bridge and sockets, at an ARRL Field Day club. The host's position id
/// rides every board line of every event it has hosted, and its own position joins its club over
/// loopback only once hosting has started: a peer that JOINs as it first, with a key of its own, is
/// turned away by name and nothing it sends is acked. The host's own laptop then joins, and its
/// contact is the club's, before and after a host restart. CONTROL: the peer's own position joins.
#[test]
fn a_lan_peer_cannot_take_the_hosts_own_position_before_the_host_joins_its_club() {
    let dir = std::env::temp_dir().join(format!("fd-own-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_own.jsonl");
    let listener = reusable_listener(0);
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, listener);
    let peer_join = fdsync::encode_line(&fdsync::Msg::Join {
        v: fdsync::PROTO_VERSION,
        pos: "aaaa0001".into(),
        name: "HQ".into(),
        call: "W9XYZ".into(),
        max_seq: 0,
        contest: "arrlfd".into(),
        role: String::new(),
        key: club_key("a peer that read the host's position id off a board"),
    });
    let got = raw_peer(
        &addr,
        &[peer_join.clone(), row_line("aaaa0001", 1, "W1FAK")],
        800,
    );
    let mut wrong = Vec::new();
    if !told(&got, tempo_app::fdevent::POSITION_HELD) {
        wrong.push(format!("the peer was not turned away by name: {got:?}"));
    }
    if got.iter().any(|m| matches!(m, fdsync::Msg::Ack { .. })) {
        wrong.push(format!("the peer's contact was acked: {got:?}"));
    }
    let host_pump_sd = start_pump(&host, &addr);
    log_fd(&host, "K1ABC", "EMA", "CW");
    let mut joined = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if worked(&host) == ["K1ABC"] {
            joined = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !joined {
        wrong.push(format!(
            "the host's own contact is not the club's: club file {:?}, host told {:?}",
            worked(&host),
            last_error(&host).map(|e| e.chars().take(60).collect::<String>())
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let own = raw_peer(
        &addr,
        &[
            join_line("eeee0005", "W9XYZ"),
            row_line("eeee0005", 1, "N0XYZ"),
        ],
        800,
    );
    assert!(
        own.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 1 })),
        "CONTROL: the peer's own position joins: {own:?}"
    );

    // The host restarts on the same event's journal: the peer is turned away still.
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    engine_lock(&host).fd_host_stop();
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, reusable_listener(port));
    let got = raw_peer(&addr, std::slice::from_ref(&peer_join), 800);
    assert!(
        told(&got, tempo_app::fdevent::POSITION_HELD),
        "after a restart: {got:?}"
    );
    let host_pump_sd = start_pump(&host, &addr);
    log_fd(&host, "W5DEF", "STX", "CW");
    wait_until("the host's own contacts are the club's", 30, || {
        worked(&host) == ["K1ABC", "N0XYZ", "W5DEF"]
    });
    for sd in [host_sd, host_pump_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A laptop whose whole settings folder was copied from a connected one is turned away by
/// name, and neither laptop's contacts are lost** — over the real bridge, sockets, pumps and
/// engines, at an ARRL Field Day club. The copy carries the laptop's position id and its club key,
/// so the host cannot tell the two apart by either: they would be one position, each counting its
/// own contacts from 1, so one laptop's would never be sent (the host's count for the position is
/// past them) and the other's would be dropped as repeats, with both chips reading Synced. The host
/// turns the copy away while the laptop it came from is connected, by name, and its entry on the
/// host's list shows the club code both laptops show. CONTROL: a laptop with a position and a key
/// of its own joins and its contact is the club's.
#[test]
fn a_copied_settings_folder_is_turned_away_by_name_while_its_laptop_is_connected() {
    let dir = std::env::temp_dir().join(format!("fd-copied-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_copied.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let host_pump_sd = start_pump(&host, &addr);
    let tent = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    let tent_sd = start_pump(&tent, &addr);
    log_fd(&tent, "K1ABC", "EMA", "CW");
    log_fd(&tent, "N0XYZ", "MN", "CW");
    wait_until("the tent's two contacts merged", 10, || {
        worked(&host) == ["K1ABC", "N0XYZ"]
    });
    // The copy: the tent's whole settings folder, so its position id and its club key.
    let copy = fd_engine("W9XYZ", "bbbb0002", "CW tent", &addr);
    log_fd(&copy, "W5DEF", "STX", "CW");
    log_fd(&copy, "K7GHI", "OR", "CW");
    let copy_sd = start_pump(&copy, &addr);
    std::thread::sleep(Duration::from_millis(2_500));
    log_fd(&copy, "W0JKL", "CO", "CW");
    std::thread::sleep(Duration::from_millis(1_500));
    log_fd(&tent, "W1FAK", "CT", "CW");
    let show = || {
        let state = |e: &Shared| {
            let c = club_json(e);
            format!(
                "{} acked {}",
                c["syncState"].as_str().unwrap_or("-"),
                engine_lock(e).fd_mirror_mut().acked
            )
        };
        format!(
            "club file {:?}; tent {}; copy {}; copy told {:?}",
            worked(&host),
            state(&tent),
            state(&copy),
            last_error(&copy).map(|e| e.chars().take(60).collect::<String>())
        )
    };
    wait_showing(
        "three contacts in the club's file",
        15,
        || worked(&host).len() == 3,
        show,
    );
    std::thread::sleep(Duration::from_millis(1_000));
    assert_eq!(
        worked(&host),
        ["K1ABC", "N0XYZ", "W1FAK"],
        "every contact of the tent's is the club's: {}",
        show()
    );
    assert_eq!(
        last_error(&copy).as_deref(),
        Some(tempo_app::fdevent::POSITION_IN_USE),
        "the copy is turned away by name"
    );
    let code = |eng: &Shared| {
        club_json(eng)["clubCode"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let entries = shown_refused(&host);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["reason"], tempo_app::fdevent::POSITION_IN_USE);
    assert!(
        !code(&tent).is_empty()
            && code(&tent) == code(&copy)
            && entries[0]["clubCode"].as_str() == Some(code(&tent).as_str()),
        "the copy's entry shows the club code both laptops show: {entries:?}"
    );
    assert!(
        club_json(&host)["refused"][0]["handle"].is_null(),
        "and no Give button: the position is already that key's"
    );
    let own = fd_engine("W9XYZ", "cccc0003", "SSB tent", &addr);
    let own_sd = start_pump(&own, &addr);
    log_fd(&own, "W9AAA", "IL", "CW");
    wait_until("CONTROL: a laptop of its own joins", 10, || {
        worked(&host).contains(&"W9AAA".to_string())
    });
    for sd in [host_sd, host_pump_sd, tent_sd, copy_sd, own_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **No JOIN gets past the key check, whatever its key field holds** — over the real bridge and
/// sockets, at an ARRL Field Day club whose CW tent is pinned and connected. A JOIN with no key
/// field, an empty key, a key one digit short or long, the tent's key in capitals or with a space
/// after it, and the pinned HASH sent as the key are each turned away by name, and nothing any of
/// them sends is acked; a key that is not a string is no JOIN at all. The same after a host
/// restart. CONTROL: the tent's own key is let in once its laptop has gone.
#[test]
fn no_join_gets_past_the_key_check_whatever_its_key_holds() {
    let dir = std::env::temp_dir().join(format!("fd-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_keys.jsonl");
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
    wait_until("the tent's contact merged", 10, || {
        worked(&host) == ["K1ABC"]
    });
    let real = club_key("bbbb0002").secret().to_string();
    let join_with = |key: &str| {
        format!(
            "{{\"t\":\"join\",\"v\":{},\"pos\":\"bbbb0002\",\"name\":\"CW tent\",\"call\":\"W9XYZ\",\
             \"max_seq\":0,\"contest\":\"arrlfd\"{key}}}\n",
            fdsync::PROTO_VERSION
        )
    };
    let no_key = tempo_app::fdevent::NO_POSITION_KEY;
    let held = tempo_app::fdevent::POSITION_HELD;
    let cases: Vec<(&str, String, Option<&str>)> = vec![
        ("no key field", join_with(""), Some(no_key)),
        ("an empty key", join_with(",\"key\":\"\""), Some(no_key)),
        (
            "63 digits",
            join_with(&format!(",\"key\":\"{}\"", &real[..63])),
            Some(no_key),
        ),
        (
            "65 digits",
            join_with(&format!(",\"key\":\"{real}0\"")),
            Some(no_key),
        ),
        (
            "the key in capitals",
            join_with(&format!(",\"key\":\"{}\"", real.to_ascii_uppercase())),
            Some(no_key),
        ),
        (
            "a space after it",
            join_with(&format!(",\"key\":\"{real} \"")),
            Some(no_key),
        ),
        (
            "the pinned hash as the key",
            join_with(&format!(",\"key\":\"{}\"", test_hash(&real))),
            Some(held),
        ),
        ("a number", join_with(",\"key\":7"), None),
        ("null", join_with(",\"key\":null"), None),
    ];
    let mut wrong = Vec::new();
    let mut check = |when: &str| {
        for (what, join, want) in &cases {
            let got = raw_peer(
                &addr,
                &[join.clone(), row_line("bbbb0002", 9, "W1FAK")],
                600,
            );
            let ok = match want {
                Some(sentence) => told(&got, sentence),
                None => got.is_empty(),
            };
            if !ok || got.iter().any(|m| matches!(m, fdsync::Msg::Ack { .. })) {
                wrong.push(format!("{when}, {what}: {got:?}"));
            }
        }
    };
    check("live");
    host_sd.store(true, Ordering::Relaxed);
    host_pump_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    engine_lock(&host).fd_host_stop();
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, reusable_listener(port));
    check("after a restart");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    tent_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    let got = raw_peer(
        &addr,
        &[
            join_with(&format!(",\"key\":\"{real}\"")),
            row_line("bbbb0002", 2, "N0XYZ"),
        ],
        800,
    );
    assert!(
        got.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 2 })),
        "CONTROL: the tent's own key: {got:?}"
    );
    host_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link a test can drop as a Wi-Fi link drops: a relay between a laptop and the host that
/// forwards both ways until [`Relay::cut`]. A cut connection forwards nothing more and its
/// laptop's side is closed at once (the laptop's network went down under it), while its host's
/// side stays open, silent, until the host closes it, which the relay notes. Every connection
/// the laptop opens after the cut is relayed afresh.
struct Relay {
    addr: String,
    live: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    host_closed: Arc<Mutex<Option<Instant>>>,
}

impl Relay {
    fn start(host: &str) -> Relay {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let live: Arc<Mutex<Vec<Arc<AtomicBool>>>> = Arc::default();
        let host_closed: Arc<Mutex<Option<Instant>>> = Arc::default();
        let (host, live2, closed2) = (host.to_string(), live.clone(), host_closed.clone());
        std::thread::spawn(move || {
            for laptop in listener.incoming().flatten() {
                let Ok(to_host) = std::net::TcpStream::connect(&host) else {
                    continue;
                };
                let dark = Arc::new(AtomicBool::new(false));
                live2.lock().unwrap().push(dark.clone());
                for s in [&laptop, &to_host] {
                    s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                }
                let pump =
                    |mut from: std::net::TcpStream,
                     mut to: std::net::TcpStream,
                     dark: Arc<AtomicBool>,
                     closed: Option<Arc<Mutex<Option<Instant>>>>| {
                        use std::io::{Read, Write};
                        let mut buf = [0u8; 8192];
                        loop {
                            match from.read(&mut buf) {
                                Ok(0) => {
                                    if dark.load(Ordering::SeqCst) {
                                        if let Some(c) = &closed {
                                            *c.lock().unwrap() = Some(Instant::now());
                                        }
                                    }
                                    let _ = to.shutdown(std::net::Shutdown::Both);
                                    return;
                                }
                                Ok(n) if !dark.load(Ordering::SeqCst) => {
                                    if to.write_all(&buf[..n]).is_err() {
                                        return;
                                    }
                                }
                                Ok(_) => {}
                                Err(e)
                                    if e.kind() == std::io::ErrorKind::WouldBlock
                                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                                Err(_) => return,
                            }
                            // Cut: the laptop's side goes down at once; the host's stays open.
                            if dark.load(Ordering::SeqCst) && closed.is_none() {
                                let _ = from.shutdown(std::net::Shutdown::Both);
                                return;
                            }
                        }
                    };
                let (l2, h2) = (laptop.try_clone().unwrap(), to_host.try_clone().unwrap());
                let (d1, d2, c) = (dark.clone(), dark.clone(), closed2.clone());
                std::thread::spawn(move || pump(laptop, h2, d1, None));
                std::thread::spawn(move || pump(to_host, l2, d2, Some(c)));
            }
        });
        Relay {
            addr,
            live,
            host_closed,
        }
    }

    /// Drop every connection relayed so far, as a dropped link drops it.
    fn cut(&self) -> Instant {
        for dark in self.live.lock().unwrap().drain(..) {
            dark.store(true, Ordering::SeqCst);
        }
        Instant::now()
    }

    /// When the host closed its side of a connection that was cut: when it noticed the link
    /// was gone.
    fn host_noticed(&self) -> Option<Instant> {
        *self.host_closed.lock().unwrap()
    }
}

/// ⭐ **A laptop back from a dropped link is turned away only until the host notices its old
/// link is gone, and then let in by itself** — over the real bridge, sockets and pumps, through a
/// relay that drops the CW tent's link the way a Wi-Fi link drops: the laptop's side goes down at
/// once, so it is back within a second, while the host's side of the old connection stays open
/// and silent until the host's dead-man closes it. Until then the laptop's JOIN meets a laptop
/// with its key connected as its position, and is turned away by name; after it, the next JOIN
/// is served, and the contact it logged meanwhile reaches the club's file. The measured times are
/// printed. CONTROL: the race happened — the laptop was turned away at least once.
#[test]
fn a_laptop_back_from_a_dropped_link_is_turned_away_only_until_the_host_notices() {
    let dir = std::env::temp_dir().join(format!("fd-dropped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let listener = reusable_listener(0);
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let host = fd_engine("W9XYZ", "aaaa0001", "HQ", &addr);
    engine_lock(&host)
        .fd_host_start(dir.join("fd_event_dropped.jsonl"))
        .unwrap();
    let host_sd = start_host(&host, listener);
    let relay = Relay::start(&addr);
    let tent = fd_engine("W9XYZ", "bbbb0002", "CW tent", &relay.addr);
    let tent_sd = start_pump(&tent, &relay.addr);
    log_fd(&tent, "K1ABC", "EMA", "CW");
    wait_until("the tent's contact merged through the relay", 10, || {
        worked(&host) == ["K1ABC"]
    });
    let wall = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    };
    let (cut, cut_wall) = (relay.cut(), wall());
    log_fd(&tent, "K9EEE", "IL", "CW");
    let in_use = tempo_app::fdevent::POSITION_IN_USE;
    // The host's list keeps the clock of each laptop's latest refused JOIN, in whole seconds.
    let refused_at = || {
        let now = wall() as u64;
        engine_lock(&host).fd_club_log().and_then(|c| {
            c.refused(now)
                .iter()
                .filter(|r| r.reason == in_use)
                .map(|r| r.at_unix)
                .max()
        })
    };
    let (mut first_refused, mut last_refused, mut served) = (None, None::<u64>, None);
    let deadline = cut + Duration::from_secs(60);
    while Instant::now() < deadline {
        if let Some(at) = refused_at() {
            first_refused.get_or_insert(Instant::now());
            last_refused = Some(last_refused.map_or(at, |l| l.max(at)));
        } else if first_refused.is_some() && last_error(&tent).is_none() && worked(&host).len() == 2
        {
            served = Some(Instant::now());
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let noticed = relay.host_noticed();
    let after = |t: Option<Instant>| t.map(|t| t.duration_since(cut).as_secs_f64());
    let last_after = last_refused.map(|at| at as f64 - cut_wall);
    eprintln!(
        "dropped link, seconds after the cut: first refused {:?}, last refused JOIN {:?} (whole \
         seconds), host noticed {:?}, served {:?}",
        after(first_refused),
        last_after,
        after(noticed),
        after(served)
    );
    let (Some(last_after), Some(noticed), Some(served)) =
        (last_after, after(noticed), after(served))
    else {
        panic!(
            "CONTROL or outcome missing: refused {:?}, noticed {:?}, served {:?}; club file {:?}",
            after(first_refused),
            after(noticed),
            after(served),
            worked(&host)
        );
    };
    let dead = fdsync::DEAD_SECS as f64;
    assert!(
        noticed <= dead + 1.5,
        "the host noticed the dead link within its dead-man: {noticed} s"
    );
    assert!(
        last_after <= noticed + 1.0,
        "turned away no longer than the host took to notice: {last_after} s, noticed {noticed} s"
    );
    assert!(
        served - noticed <= 17.0,
        "served on its next try after that: {served} s, noticed {noticed} s"
    );
    assert_eq!(
        worked(&host),
        ["K1ABC", "K9EEE"],
        "the contact logged meanwhile"
    );
    for sd in [host_sd, tent_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
