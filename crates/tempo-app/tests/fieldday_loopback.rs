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

fn start_host(eng: &Shared, port: u16) -> Arc<AtomicBool> {
    let listener = reusable_listener(port);
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

    // An OS-assigned free port, reused across the host restart.
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let addr = format!("127.0.0.1:{port}");

    // The host engine — ALSO position #1, joined over its own loopback
    // listener (zero special cases for the host's own contacts).
    let host = fd_engine("W9ABC", "aaaa0001", "HQ", &addr);
    engine_lock(&host).fd_host_start(journal.clone()).unwrap();
    let host_sd = start_host(&host, port);
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
    let host2_sd = start_host(&host2, port);
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
