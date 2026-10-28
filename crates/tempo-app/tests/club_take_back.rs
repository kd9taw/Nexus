//! Taking a contact back at a club, through the whole stack over 127.0.0.1: real engines, the
//! REAL `fdbridge` impls, `fdsync::serve_until` and the real position pumps, on the Illinois QSO
//! Party (the club's next event). Loopback only, ephemeral ports.
//!
//! Each case is read by value off the host's club log, its club Cabrillo, the TV board's rows,
//! every position's mirror and chip: a take-back reaching the host before or after its contact,
//! twice, across a host restart, from a position that was away when it took back, and only ever
//! a laptop's own contact. Mismatched builds refuse each other in words.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_app::fdevent::SyncState;
use tempo_net::fdsync::{self, ClubBackend, PositionSync};

type Shared = Arc<Mutex<Engine>>;

/// 64 hex digits for `tag`, from a per-run seed: never a real key.
fn throwaway_hex(tag: &str) -> String {
    use std::hash::BuildHasher;
    static SEED: std::sync::OnceLock<std::hash::RandomState> = std::sync::OnceLock::new();
    let seed = SEED.get_or_init(std::hash::RandomState::new);
    (0..4u8)
        .map(|i| format!("{:016x}", seed.hash_one((tag, i))))
        .collect()
}

fn club_key(posid: &str) -> fdsync::PositionKey {
    fdsync::PositionKey::new(throwaway_hex(posid))
}

/// An Illinois QSO Party station in Kane County on the club's call, joining `join_addr`.
fn ilqp_engine(posid: &str, name: &str, join_addr: &str) -> Shared {
    let mut e = Engine::new("W9XYZ", "EN50", 0);
    let mut s = e.settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = posid.into();
    s.fd_position_name = name.into();
    s.fd_join_addr = join_addr.into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("the party runs");
    e.set_fd_position_key(club_key(posid));
    Arc::new(Mutex::new(e))
}

/// A listener with SO_REUSEADDR, so a restarted host binds the same port again.
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
    let backend: Arc<dyn PositionSync> = Arc::new(EnginePositionSync::new(eng.clone()));
    let (a, sd2) = (addr.to_string(), sd.clone());
    std::thread::spawn(move || fdsync::run_position_until(&a, backend, sd2));
    sd
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

fn exchange(county: &str) -> Vec<(String, String)> {
    vec![("RST".into(), "599".into()), ("QTH".into(), county.into())]
}

/// Log `call` from `counties` (two or more: a county line, one row per county).
fn party(eng: &Shared, call: &str, counties: &[&str]) {
    let rows: Vec<_> = counties.iter().map(|c| exchange(c)).collect();
    let logged = engine_lock(eng)
        .contest_log_manual_rows(call, &rows, "CW", None)
        .expect("the party runs");
    assert!(logged.iter().all(|&ok| ok), "{call} refused: fixture bug");
}

/// The newest contact as the strip names it.
fn newest(eng: &Shared) -> (String, u64) {
    let fd = engine_lock(eng)
        .snapshot()
        .field_day
        .expect("the party runs");
    let q = fd.log.last().expect("a contact");
    (q.call.clone(), q.when_unix)
}

/// Ctrl+D twice at `eng`: the answer's `club` (whether this laptop was with the host).
fn take_back_newest(eng: &Shared) -> (u64, Option<bool>) {
    let (call, when) = newest(eng);
    let removal = engine_lock(eng)
        .contest_remove_last(&call, when)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("{call} refused: {r:?}"));
    (removal.entry.id(), removal.club)
}

fn restore(eng: &Shared, id: u64) -> Option<bool> {
    engine_lock(eng)
        .contest_restore(id)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("restore refused: {r:?}"))
        .club
}

/// The calls in the host's club log, live rows, in merge order.
fn club_calls(host: &Shared) -> Vec<String> {
    engine_lock(host)
        .fd_club_log()
        .map(|c| c.rows().iter().map(|r| r.call.clone()).collect())
        .unwrap_or_default()
}

/// [`club_calls`] sorted: two laptops' pumps merge in either order.
fn club_calls_sorted(host: &Shared) -> Vec<String> {
    let mut calls = club_calls(host);
    calls.sort();
    calls
}

/// The host's club Cabrillo, as it would be sent in.
fn club_cabrillo(host: &Shared) -> String {
    engine_lock(host)
        .fd_club_export(true)
        .expect("the club file")
}

/// The calls on the TV board's rows.
fn board_calls(host: &Shared) -> Vec<String> {
    engine_lock(host)
        .fd_board_snapshot()
        .and_then(|b| b.contest)
        .map(|c| c.rows.iter().map(|r| r.call.clone()).collect())
        .unwrap_or_default()
}

/// Does `eng`'s club mirror hold a dupe key for `call`?
fn mirror_has(eng: &Shared, call: &str) -> bool {
    engine_lock(eng)
        .fd_mirror_mut()
        .dkeys
        .iter()
        .any(|k| k.first().is_some_and(|c| c == call))
}

fn state(eng: &Shared) -> SyncState {
    engine_lock(eng).fd_sync_state()
}

fn taken_back(host: &Shared, pos: &str) -> Vec<u64> {
    engine_lock(host)
        .fd_club_log()
        .map(|c| c.taken_back(pos))
        .unwrap_or_default()
}

/// A temp journal for one test.
fn journal(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("club-take-back-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (dir.clone(), dir.join("fd_event_test.jsonl"))
}

/// A host on an ephemeral port that joins itself, as the shell runs one.
struct Host {
    eng: Shared,
    addr: String,
    port: u16,
    sds: Vec<Arc<AtomicBool>>,
}

impl Host {
    fn start(journal: &std::path::Path) -> Host {
        let listener = reusable_listener(0);
        let port = listener.local_addr().unwrap().port();
        Host::start_on(journal, listener, port)
    }

    fn start_on(journal: &std::path::Path, listener: std::net::TcpListener, port: u16) -> Host {
        let addr = format!("127.0.0.1:{port}");
        let eng = ilqp_engine("aaaa0001", "HQ", &addr);
        engine_lock(&eng)
            .fd_host_start(journal.to_path_buf())
            .expect("the club starts");
        let sds = vec![start_host(&eng, listener), start_pump(&eng, &addr)];
        Host {
            eng,
            addr,
            port,
            sds,
        }
    }

    fn stop(&self) {
        for sd in &self.sds {
            sd.store(true, Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_millis(600));
    }
}

/// ⭐ **A position takes back its own contact, a county line with both its rows, and every
/// laptop sees it go: the host's club log, its club Cabrillo, the TV board, and every
/// position's club dupe keys; Restore brings it back the same way, and the club's file is the
/// bytes it was.** Both chips settle on Synced each time.
#[test]
fn a_take_back_leaves_every_laptop_and_its_restore_brings_back_the_same_file() {
    let (dir, path) = journal("every-laptop");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);

    party(&p2, "K9AAA", &["COOK"]);
    party(&p2, "K9NR", &["COOK", "DUPG"]); // one contact, two rows
    party(&host.eng, "W9BBB", &["LAKE"]);
    wait_until("four rows at the host", 10, || {
        club_calls(&host.eng).len() == 4
    });
    wait_until("both mirrors know K9NR", 10, || {
        mirror_has(&host.eng, "K9NR") && mirror_has(&p2, "K9NR")
    });
    let before = club_cabrillo(&host.eng);
    assert_eq!(
        before.matches("K9NR").count(),
        2,
        "premise: the county line is two lines"
    );

    let (id, club) = take_back_newest(&p2);
    assert_eq!(club, Some(true), "the answer: with the host now");
    wait_until("the host takes both rows back", 10, || {
        club_calls_sorted(&host.eng) == ["K9AAA", "W9BBB"]
    });
    assert_eq!(taken_back(&host.eng, "bbbb0002"), [2, 3]);
    assert!(
        !club_cabrillo(&host.eng).contains("K9NR"),
        "out of the club file"
    );
    let mut tv = board_calls(&host.eng);
    tv.sort();
    assert_eq!(tv, ["K9AAA", "W9BBB"], "out of the TV board");
    wait_until("every mirror drops K9NR", 10, || {
        !mirror_has(&host.eng, "K9NR") && !mirror_has(&p2, "K9NR")
    });
    assert!(
        mirror_has(&p2, "K9AAA") && mirror_has(&p2, "W9BBB"),
        "…and keeps the rest"
    );
    for eng in [&host.eng, &p2] {
        wait_until("synced", 10, || state(eng) == SyncState::Synced);
    }

    assert_eq!(restore(&p2, id), Some(true));
    wait_until("the host has it back", 10, || {
        club_calls(&host.eng).len() == 4
    });
    assert_eq!(
        club_cabrillo(&host.eng),
        before,
        "the club file is the bytes it was"
    );
    assert!(taken_back(&host.eng, "bbbb0002").is_empty());
    wait_until("every mirror has K9NR again", 10, || {
        mirror_has(&host.eng, "K9NR") && mirror_has(&p2, "K9NR")
    });
    for eng in [&host.eng, &p2] {
        wait_until("synced again", 10, || state(eng) == SyncState::Synced);
    }

    p2_sd.store(true, Ordering::Relaxed);
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

fn raw_peer(addr: &str, lines: &[String], ms: u64) -> Vec<fdsync::Msg> {
    use std::io::{BufReader, Write};
    let s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    for l in lines {
        w.write_all(format!("{l}\n").as_bytes()).unwrap();
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

/// The JOIN a current Nexus sends, as `pos`, with that position's key.
fn join_line(pos: &str) -> String {
    serde_json::json!({
        "t": "join", "v": fdsync::PROTO_VERSION, "pos": pos, "name": "Raw tent",
        "call": "W9XYZ", "max_seq": 0, "contest": "ilqp", "role": "in_state",
        "key": throwaway_hex(pos),
    })
    .to_string()
}

fn row_line(pos: &str, seq: u64, call: &str) -> String {
    serde_json::json!({
        "t": "qso", "pos": pos, "seq": seq, "call": call, "band": "20m", "mode": "CW",
        "when": 1_792_000_000u64 + seq, "op": "W9XYZ",
        "ex": [{"k": "RST", "r": "599"}, {"k": "QTH", "d": "il_counties", "r": "COOK"}],
        "mex": [{"k": "RST", "r": "599"}, {"k": "QTH", "d": "il_counties", "r": "KANE"}],
    })
    .to_string()
}

fn mark_line(t: &str, pos: &str, seq: u64) -> String {
    serde_json::json!({ "t": t, "pos": pos, "seq": seq }).to_string()
}

/// ⭐ **A laptop takes back only its own contacts.** A laptop JOINed as one position naming
/// another's row is refused by name and closed, and the other's contact stays in the club's
/// file; the same laptop's take-back of its OWN row goes through (the control), and so does
/// the host's of its own, which leaves every other position's contact alone.
#[test]
fn a_laptop_takes_back_only_its_own_contacts() {
    let (dir, path) = journal("own-only");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);
    party(&p2, "K9AAA", &["COOK"]);
    party(&host.eng, "W9BBB", &["LAKE"]);
    wait_until("two rows at the host", 10, || {
        club_calls(&host.eng).len() == 2
    });

    let got = raw_peer(
        &host.addr,
        &[
            join_line("cccc0003"),
            row_line("cccc0003", 1, "N9CCC"),
            mark_line("retract", "bbbb0002", 1),
        ],
        800,
    );
    assert!(
        got.iter().any(|m| matches!(m, fdsync::Msg::Error { msg }
            if msg == fdsync::ANOTHER_POSITIONS_ROW)),
        "refused by name: {got:?}"
    );
    assert!(club_cabrillo(&host.eng).contains("K9AAA"), "K9AAA stays");
    assert!(taken_back(&host.eng, "bbbb0002").is_empty());

    // CONTROL: its own row, the same way, goes.
    let got = raw_peer(
        &host.addr,
        &[join_line("cccc0003"), mark_line("retract", "cccc0003", 1)],
        800,
    );
    assert!(
        got.iter()
            .any(|m| matches!(m, fdsync::Msg::Retracted { seq: 1 })),
        "{got:?}"
    );
    wait_until("N9CCC leaves the club log", 10, || {
        !club_calls(&host.eng).contains(&"N9CCC".to_string())
    });

    // The host's own Ctrl+D takes back the host's own contact and nobody else's.
    take_back_newest(&host.eng);
    wait_until("W9BBB leaves the club log", 10, || {
        club_calls(&host.eng) == ["K9AAA"]
    });
    assert_eq!(taken_back(&host.eng, "aaaa0001"), [1]);

    p2_sd.store(true, Ordering::Relaxed);
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A take-back that reaches the host BEFORE its contact keeps the contact out**, over the
/// wire and through the real pump: a position away from the host logs K9AAA, takes it back,
/// and logs W9BBB; back with the host, the club gets W9BBB and the tombstone, and never
/// K9AAA. Restored, K9AAA reaches the club after all: the row goes up with the restore,
/// though the host's ack is already past it.
#[test]
fn a_take_back_that_reaches_the_host_before_its_contact_keeps_it_out() {
    let (dir, path) = journal("before");
    let host = Host::start(&path);

    // Over the wire, in that order: the take-back, then its row, then the restore.
    let got = raw_peer(
        &host.addr,
        &[
            join_line("dddd0004"),
            mark_line("retract", "dddd0004", 5),
            row_line("dddd0004", 5, "N9DDD"),
        ],
        800,
    );
    assert!(
        got.iter().any(|m| matches!(m, fdsync::Msg::Ack { seq: 5 })),
        "the row is accounted for: {got:?}"
    );
    assert!(!club_calls(&host.eng).contains(&"N9DDD".to_string()));
    assert!(!club_cabrillo(&host.eng).contains("N9DDD"));
    // The host notices that connection is gone within a read tick; a JOIN before then is
    // turned away as a position in use.
    std::thread::sleep(Duration::from_millis(600));
    raw_peer(
        &host.addr,
        &[join_line("dddd0004"), mark_line("restore", "dddd0004", 5)],
        800,
    );
    assert!(club_cabrillo(&host.eng).contains("N9DDD"), "restored");

    // Through the real pump: away, take back, log on; then back with the host.
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    party(&p2, "K9AAA", &["COOK"]);
    let (id, club) = take_back_newest(&p2);
    assert_eq!(
        club,
        Some(false),
        "not with the host: it goes when the link is back"
    );
    party(&p2, "W9BBB", &["LAKE"]);
    let p2_sd = start_pump(&p2, &host.addr);
    wait_until("W9BBB at the host", 10, || {
        club_calls(&host.eng).contains(&"W9BBB".to_string())
    });
    wait_until("the tombstone at the host", 10, || {
        taken_back(&host.eng, "bbbb0002") == [1]
    });
    assert!(
        !club_cabrillo(&host.eng).contains("K9AAA"),
        "never in the club"
    );
    wait_until("p2 synced", 10, || state(&p2) == SyncState::Synced);

    restore(&p2, id);
    wait_until("K9AAA reaches the club after all", 10, || {
        club_cabrillo(&host.eng).contains("K9AAA")
    });
    wait_until("p2 synced again", 10, || state(&p2) == SyncState::Synced);

    p2_sd.store(true, Ordering::Relaxed);
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The host journal's take-back lines for `pos`, as `(kind, seq)`.
fn journal_marks(path: &std::path::Path, pos: &str) -> Vec<(String, u64)> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            let (kind, body) = v.as_object()?.iter().next()?;
            (body.get("pos")?.as_str()? == pos)
                .then(|| Some((kind.clone(), body.get("seq")?.as_u64()?)))
                .flatten()
        })
        .filter(|(k, _)| k == "take_back" || k == "put_back")
        .collect()
}

/// ⭐ **A take-back twice is one tombstone and one journal line**, over the wire, and
/// answered both times; through the pump, remove, restore and remove again end taken back,
/// each change journaled once.
#[test]
fn a_take_back_twice_is_one_tombstone_and_one_journal_line() {
    let (dir, path) = journal("twice");
    let host = Host::start(&path);
    let got = raw_peer(
        &host.addr,
        &[
            join_line("eeee0005"),
            row_line("eeee0005", 1, "N9EEE"),
            mark_line("retract", "eeee0005", 1),
            mark_line("retract", "eeee0005", 1),
        ],
        800,
    );
    assert_eq!(
        got.iter()
            .filter(|m| matches!(m, fdsync::Msg::Retracted { seq: 1 }))
            .count(),
        2,
        "answered both times: {got:?}"
    );
    assert_eq!(
        journal_marks(&path, "eeee0005"),
        [("take_back".to_string(), 1)]
    );
    assert!(!club_calls(&host.eng).contains(&"N9EEE".to_string()));

    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);
    party(&p2, "K9AAA", &["COOK"]);
    wait_until("K9AAA at the host", 10, || {
        club_calls(&host.eng).contains(&"K9AAA".to_string())
    });
    let (id, _) = take_back_newest(&p2);
    wait_until("taken back", 10, || {
        taken_back(&host.eng, "bbbb0002") == [1]
    });
    restore(&p2, id);
    wait_until("back", 10, || taken_back(&host.eng, "bbbb0002").is_empty());
    take_back_newest(&p2);
    wait_until("taken back again", 10, || {
        taken_back(&host.eng, "bbbb0002") == [1]
    });
    assert!(!club_calls(&host.eng).contains(&"K9AAA".to_string()));
    assert_eq!(
        journal_marks(&path, "bbbb0002"),
        [
            ("take_back".to_string(), 1),
            ("put_back".to_string(), 1),
            ("take_back".to_string(), 1)
        ]
    );

    p2_sd.store(true, Ordering::Relaxed);
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A host restart between a take-back and its undo loses neither**: the restarted host's
/// journal alone still holds the contact taken back, and a restore made while the host was
/// down reaches it when the position joins again — the host lists what it holds taken back
/// after the welcome, and the position answers with the row and the restore.
#[test]
fn a_host_restart_between_a_take_back_and_its_undo_loses_neither() {
    let (dir, path) = journal("restart");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);
    party(&p2, "K9AAA", &["COOK"]);
    party(&p2, "W9BBB", &["LAKE"]);
    wait_until("both at the host", 10, || club_calls(&host.eng).len() == 2);
    let (id, _) = take_back_newest(&p2);
    wait_until("taken back", 10, || club_calls(&host.eng) == ["K9AAA"]);
    // As above: the take-back's ack must reach the position before the host goes, or it
    // still counts as queued beside the restore below.
    wait_until("p2 synced", 10, || state(&p2) == SyncState::Synced);
    host.stop();
    wait_until("p2 sees the host gone", 20, || {
        !engine_lock(&p2).fd_mirror_mut().connected
    });

    // Put back while the host is down: owed, and said so.
    assert_eq!(restore(&p2, id), Some(false));
    assert!(matches!(state(&p2), SyncState::Offline { queued: 1, .. }));

    // The restarted host, from its journal alone: still taken back.
    let port = host.port;
    let reborn = Host::start_on(&path, reusable_listener(port), port);
    assert_eq!(taken_back(&reborn.eng, "bbbb0002"), [2]);
    assert!(!club_cabrillo(&reborn.eng).contains("W9BBB"));
    wait_until("the restore reaches the reborn host", 30, || {
        club_calls(&reborn.eng) == ["K9AAA", "W9BBB"]
    });
    assert!(club_cabrillo(&reborn.eng).contains("W9BBB"));
    wait_until("p2 synced", 10, || state(&p2) == SyncState::Synced);

    p2_sd.store(true, Ordering::Relaxed);
    reborn.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A position away from the host when it takes back catches the club up when it is back**:
/// its chip says the take-back is queued here, the club still holds the contact meanwhile,
/// and once the link is back the club drops it and the chip settles.
#[test]
fn a_position_away_when_it_takes_back_catches_the_club_up_when_it_returns() {
    let (dir, path) = journal("away");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);
    party(&p2, "K9AAA", &["COOK"]);
    party(&p2, "W9BBB", &["LAKE"]);
    wait_until("both at the host", 10, || club_calls(&host.eng).len() == 2);
    // The host holding both is not the position hearing it does: its queue counts until the
    // host's ack arrives, so wait for that before the link goes, or the second contact still
    // counts as queued below.
    wait_until("p2 synced", 10, || state(&p2) == SyncState::Synced);
    p2_sd.store(true, Ordering::Relaxed);
    wait_until("p2 link down", 10, || {
        !engine_lock(&p2).fd_mirror_mut().connected
    });

    let (_, club) = take_back_newest(&p2);
    assert_eq!(club, Some(false));
    assert!(
        matches!(state(&p2), SyncState::Offline { queued: 1, .. }),
        "{:?}",
        state(&p2)
    );
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        club_calls(&host.eng),
        ["K9AAA", "W9BBB"],
        "the club still has it"
    );

    let p2_sd = start_pump(&p2, &host.addr);
    wait_until("the club drops it", 10, || {
        club_calls(&host.eng) == ["K9AAA"]
    });
    wait_until("p2 synced", 10, || state(&p2) == SyncState::Synced);

    p2_sd.store(true, Ordering::Relaxed);
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **An older Nexus is turned away in words** by a current host, and its contacts never reach the
/// club. The lines are the bytes those builds write: a v3 test build's JOIN (it proves its
/// position with a key, and cannot take a contact back) and 1.17.0's (v2: no contest, no
/// role, no key). The other direction — a current position at an older host — is
/// `fdsync::tests::a_v5_position_joining_an_older_host_gets_that_hosts_refusal_verbatim`.
#[test]
fn an_older_nexus_is_turned_away_in_words_and_sends_nothing_into_the_club() {
    let (dir, path) = journal("older");
    let host = Host::start(&path);
    // The v3 build's encoder's own field order and values, as the one-off fixture that ran that
    // build's `fdsync.rs` printed them; only the key is this run's throwaway one.
    let v3 = format!(
        r#"{{"t":"join","v":3,"pos":"ffff0006","name":"Old tent","call":"W9XYZ","max_seq":0,"contest":"ilqp","role":"in_state","key":"{}"}}"#,
        throwaway_hex("ffff0006")
    );
    let v2 = r#"{"t":"join","v":2,"pos":"ffff0007","name":"Old tent","call":"W9XYZ","max_seq":0}"#;
    for (v, join, pos) in [(3, v3.as_str(), "ffff0006"), (2, v2, "ffff0007")] {
        let got = raw_peer(
            &host.addr,
            &[join.to_string(), row_line(pos, 1, "N9OLD")],
            800,
        );
        let said = got.iter().find_map(|m| match m {
            fdsync::Msg::Error { msg } => Some(msg.clone()),
            _ => None,
        });
        let said = said.unwrap_or_else(|| panic!("v{v}: no words: {got:?}"));
        assert!(
            said.contains(&format!(
                "needs club sync v{} on every laptop",
                fdsync::PROTO_VERSION
            )) && said.contains(&format!("this Nexus speaks v{v}"))
                && said.contains("Update Nexus on this laptop"),
            "v{v}: {said}"
        );
        assert!(
            !got.iter()
                .any(|m| matches!(m, fdsync::Msg::Welcome { .. } | fdsync::Msg::Ack { .. })),
            "v{v}: never welcomed, nothing acked: {got:?}"
        );
    }
    assert!(!club_cabrillo(&host.eng).contains("N9OLD"));
    // CONTROL: a current JOIN from the same kind of peer is welcomed.
    let ok = raw_peer(&host.addr, &[join_line("ffff0008")], 600);
    assert!(
        ok.iter()
            .any(|m| matches!(m, fdsync::Msg::Welcome { v, .. } if *v == fdsync::PROTO_VERSION)),
        "{ok:?}"
    );
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
