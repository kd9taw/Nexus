//! Every laptop of a club lists the club's contacts as the host does, through the whole stack over
//! 127.0.0.1: real engines, the REAL `fdbridge` impls, `fdsync::serve_until` and the real
//! position pumps, on the Illinois QSO Party. Loopback only, ephemeral ports.
//!
//! Each case compares a laptop's list (`Engine::fd_club_log_newest`, what its contest screen
//! reads) with the host's own, by value, row for row: after live logging, after a laptop was away
//! and came back, after a take-back and its restore, and after the host removed a position. An
//! older Nexus is turned away in words.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::dto::FdClubLogDto;
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
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
    e.set_fd_position_key(fdsync::PositionKey::new(throwaway_hex(posid)));
    Arc::new(Mutex::new(e))
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

/// Log `call` from `county` at `eng`; `false` when the engine would not log it.
fn log(eng: &Shared, call: &str, county: &str) -> bool {
    let rows = vec![vec![
        ("RST".to_string(), "599".to_string()),
        ("QTH".to_string(), county.to_string()),
    ]];
    engine_lock(eng)
        .contest_log_manual_rows(call, &rows, "CW", None)
        .expect("the party runs")
        .iter()
        .all(|&ok| ok)
}

fn logs(eng: &Shared, calls: &[&str]) {
    for call in calls {
        assert!(log(eng, call, "COOK"), "{call} refused: fixture bug");
    }
}

/// The club list as `eng`'s contest screen reads it.
fn list(eng: &Shared) -> Option<FdClubLogDto> {
    engine_lock(eng).fd_club_log_newest()
}

fn calls(l: &Option<FdClubLogDto>) -> Vec<String> {
    l.as_ref()
        .map(|l| l.rows.iter().map(|r| r.call.clone()).collect())
        .unwrap_or_default()
}

/// Wait until the host's list is `ready` and every laptop lists exactly what the host lists, and
/// return that list.
fn wait_same(
    host: &Shared,
    laptops: &[&Shared],
    what: &str,
    ready: impl Fn(&FdClubLogDto) -> bool,
) -> FdClubLogDto {
    let mut last = (None, Vec::new());
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let h = list(host);
        let l: Vec<_> = laptops.iter().map(|p| list(p)).collect();
        if h.as_ref().is_some_and(&ready) && l.iter().all(|x| *x == h) {
            return h.unwrap();
        }
        last = (h, l);
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "{what}: the laptops never listed what the host lists:\nhost {:?}\nlaptops {:?}",
        last.0, last.1
    );
}

fn journal(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("club-list-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (dir.clone(), dir.join("fd_event_test.jsonl"))
}

/// A host on an ephemeral port that joins itself, as the shell runs one.
struct Host {
    eng: Shared,
    addr: String,
    sds: Vec<Arc<AtomicBool>>,
}

impl Host {
    fn start(journal: &std::path::Path) -> Host {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let eng = ilqp_engine("aaaa0001", "HQ", &addr);
        engine_lock(&eng)
            .fd_host_start(journal.to_path_buf())
            .expect("the club starts");
        let sd = Arc::new(AtomicBool::new(false));
        let backend: Arc<dyn ClubBackend> = Arc::new(EngineClubBackend(eng.clone()));
        let sd2 = sd.clone();
        std::thread::spawn(move || fdsync::serve_until(listener, backend, sd2));
        let sds = vec![sd, start_pump(&eng, &addr)];
        Host { eng, addr, sds }
    }

    fn stop(&self) {
        for sd in &self.sds {
            sd.store(true, Ordering::Relaxed);
        }
        std::thread::sleep(Duration::from_millis(600));
    }
}

/// ⭐ **Every laptop lists the club's contacts as the host does, live**: the host's own and two
/// positions' contacts, newest first, each with the position that logged it, row for row and the
/// total, on both laptops.
#[test]
fn every_laptop_lists_the_hosts_club_log_after_live_logging() {
    let (dir, path) = journal("live");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p3 = ilqp_engine("cccc0003", "Phone tent", &host.addr);
    let sds = [start_pump(&p2, &host.addr), start_pump(&p3, &host.addr)];
    wait_same(&host.eng, &[&p2, &p3], "an empty club", |l| l.total == 0);

    logs(&p2, &["K9AAA", "K9AAB"]);
    logs(&host.eng, &["W9BBB"]);
    logs(&p3, &["N9CCC"]);
    let l = wait_same(&host.eng, &[&p2, &p3], "four contacts", |l| l.total == 4);
    assert_eq!(l.total, 4);
    let mut got = calls(&Some(l.clone()));
    got.sort();
    assert_eq!(got, ["K9AAA", "K9AAB", "N9CCC", "W9BBB"]);
    let by = |call: &str| {
        l.rows
            .iter()
            .find(|r| r.call == call)
            .unwrap()
            .pos_name
            .clone()
    };
    assert_eq!(
        (by("K9AAA"), by("W9BBB"), by("N9CCC")),
        ("CW tent".into(), "HQ".into(), "Phone tent".into()),
        "each with the position that logged it"
    );

    for sd in &sds {
        sd.store(true, Ordering::Relaxed);
    }
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A laptop away from the host catches the list up when it is back**: what the club logged
/// meanwhile, and what it logged itself while away, including a contact another position had
/// already made, which every list marks a duplicate as the host's does; and a contact another
/// position took back while it was away, which its list held, is gone from it.
#[test]
fn a_laptop_back_from_an_offline_period_lists_what_the_host_lists() {
    let (dir, path) = journal("offline");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p3 = ilqp_engine("cccc0003", "Phone tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);
    let p3_sd = start_pump(&p3, &host.addr);
    logs(&p2, &["K9AAA"]);
    logs(&p3, &["N9CCZ"]);
    wait_same(&host.eng, &[&p2, &p3], "two contacts", |l| l.total == 2);

    // p2 goes away; the club logs on, p3 takes N9CCZ back, and p2 logs on too.
    p2_sd.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(600));
    logs(&host.eng, &["W9BBB"]);
    let when = {
        let fd = engine_lock(&p3)
            .snapshot()
            .field_day
            .expect("the party runs");
        let q = fd.log.last().expect("a contact");
        assert_eq!(q.call, "N9CCZ");
        q.when_unix
    };
    engine_lock(&p3)
        .contest_remove_last("N9CCZ", when)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("refused: {r:?}"));
    logs(&p3, &["N9CCC", "N9CCD"]);
    std::thread::sleep(Duration::from_millis(1100));
    // A contact the club already holds from the host: p2, away, cannot know.
    assert!(log(&p2, "W9BBB", "COOK"), "p2 logs it, away from the club");
    logs(&p2, &["K9AAB"]);
    wait_until("the host has the club's four", 10, || {
        list(&host.eng).is_some_and(|l| l.total == 4)
    });
    let mut stale = calls(&list(&p2));
    stale.sort();
    assert_eq!(stale, ["K9AAA", "N9CCZ"], "p2's list is what it last heard");

    let p2_sd = start_pump(&p2, &host.addr);
    let l = wait_same(&host.eng, &[&p2, &p3], "p2 back", |l| l.total == 6);
    assert_eq!(l.total, 6);
    assert!(
        !calls(&Some(l.clone())).contains(&"N9CCZ".to_string()),
        "the take-back made while p2 was away is off its list: {l:?}"
    );
    let dupes: Vec<_> = l
        .rows
        .iter()
        .filter(|r| r.dupe)
        .map(|r| (&r.call[..], &r.posid[..]))
        .collect();
    assert_eq!(
        dupes,
        [("W9BBB", "bbbb0002")],
        "the later W9BBB is the club's duplicate"
    );

    for sd in [p2_sd, p3_sd] {
        sd.store(true, Ordering::Relaxed);
    }
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A take-back leaves every laptop's list, and its restore brings it back**, as on the host;
/// and **a position the host removed keeps its contacts on every list**, as on the host.
#[test]
fn a_take_back_and_a_removed_position_list_on_every_laptop_as_on_the_host() {
    let (dir, path) = journal("take-back");
    let host = Host::start(&path);
    let p2 = ilqp_engine("bbbb0002", "CW tent", &host.addr);
    let p3 = ilqp_engine("cccc0003", "Phone tent", &host.addr);
    let p2_sd = start_pump(&p2, &host.addr);
    let p3_sd = start_pump(&p3, &host.addr);
    logs(&p2, &["K9AAA"]);
    logs(&p3, &["N9CCC", "N9CCD"]);
    wait_same(&host.eng, &[&p2, &p3], "three contacts", |l| l.total == 3);

    // p3 takes its newest back (Ctrl+D twice).
    let (call, when) = {
        let fd = engine_lock(&p3)
            .snapshot()
            .field_day
            .expect("the party runs");
        let q = fd.log.last().expect("a contact");
        (q.call.clone(), q.when_unix)
    };
    assert_eq!(call, "N9CCD");
    let removal = engine_lock(&p3)
        .contest_remove_last(&call, when)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("refused: {r:?}"));
    let l = wait_same(&host.eng, &[&p2, &p3], "the take-back", |l| l.total == 2);
    assert!(
        !calls(&Some(l.clone())).contains(&"N9CCD".to_string()),
        "{l:?}"
    );
    assert_eq!(l.total, 2);

    engine_lock(&p3)
        .contest_restore(removal.entry.id())
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("restore refused: {r:?}"));
    let l = wait_same(&host.eng, &[&p2, &p3], "the restore", |l| l.total == 3);
    assert!(
        calls(&Some(l.clone())).contains(&"N9CCD".to_string()),
        "{l:?}"
    );

    // The host removes p3: its contacts stay in the club's log, and on p2's list.
    engine_lock(&host.eng)
        .fd_club_remove_position("cccc0003")
        .expect("removed");
    logs(&p2, &["K9AAB"]);
    let l = wait_same(&host.eng, &[&p2], "after the removal", |l| l.total == 4);
    assert_eq!(l.total, 4);
    assert!(l.rows.iter().any(|r| r.posid == "cccc0003"), "{l:?}");

    for sd in [p2_sd, p3_sd] {
        sd.store(true, Ordering::Relaxed);
    }
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

fn join_line(v: u32, pos: &str) -> String {
    serde_json::json!({
        "t": "join", "v": v, "pos": pos, "name": "Raw tent",
        "call": "W9XYZ", "max_seq": 0, "contest": "ilqp", "role": "in_state",
        "key": throwaway_hex(pos),
    })
    .to_string()
}

/// ⭐ **An older Nexus is turned away when it joins, and told to update**: a laptop that cannot
/// list the club's contacts would leave its operator looking at a part of the club's log. The
/// line is the bytes the previous release writes (v4 with its key).
#[test]
fn an_older_nexus_is_turned_away_and_told_to_update() {
    let (dir, path) = journal("older");
    let host = Host::start(&path);
    let got = raw_peer(&host.addr, &[join_line(4, "ffff0006")], 800);
    let said = got
        .iter()
        .find_map(|m| match m {
            fdsync::Msg::Error { msg } => Some(msg.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no words: {got:?}"));
    assert!(
        said.contains("needs club sync v5 on every laptop")
            && said.contains("every laptop lists the club's contacts")
            && said.contains("this Nexus speaks v4")
            && said.contains("Update Nexus on this laptop"),
        "{said}"
    );
    assert!(
        !got.iter().any(|m| matches!(m, fdsync::Msg::Welcome { .. })),
        "never welcomed: {got:?}"
    );
    // CONTROL: a current JOIN from the same kind of peer is welcomed.
    let ok = raw_peer(
        &host.addr,
        &[join_line(fdsync::PROTO_VERSION, "ffff0008")],
        600,
    );
    assert!(
        ok.iter()
            .any(|m| matches!(m, fdsync::Msg::Welcome { v: 5, .. })),
        "{ok:?}"
    );
    host.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **THE TRAFFIC THE LIST ADDS, MEASURED**: five positions log their contacts, their
/// rows merged by the real host engine one at a time, and after EVERY contact each of the five
/// connections is sent what changed in the club list, as `fdsync`'s own writer writes it for the
/// wire (the worst case: a real connection sends at most one change a tick, so a burst costs less).
/// Each laptop's list, built only from those lines, is the host's own after every contact, and
/// every line is inside the cap. The bytes are the bound: a few hundred a contact for each laptop,
/// whatever the club's size, and a laptop joining late is sent the newest hundred, not the log.
#[test]
fn the_club_list_costs_each_laptop_a_bounded_few_hundred_bytes_a_contact() {
    let (dir, path) = journal("traffic");
    let host = ilqp_engine("aaaa0001", "HQ", "");
    engine_lock(&host)
        .fd_host_start(path.clone())
        .expect("the club starts");
    let positions = ["aaaa0001", "bbbb0002", "cccc0003", "dddd0004", "eeee0005"];
    let names = [
        "HQ",
        "CW tent",
        "Phone tent",
        "Digital tent",
        "Satellite tent",
    ];
    let holds: Vec<u64> = positions
        .iter()
        .zip(names)
        .map(|(pos, name)| {
            engine_lock(&host)
                .fd_club_join(
                    fdsync::PROTO_VERSION,
                    pos,
                    name,
                    "W9XYZ",
                    "ilqp",
                    "in_state",
                    &tempo_app::fdevent::sha256_hex(&throwaway_hex(pos)),
                )
                .expect("served")
                .hold
        })
        .collect();
    let counties = [
        "COOK", "DUPG", "LAKE", "WILL", "KANE", "MCHE", "WINN", "PEOR",
    ];
    let mut sent: Vec<fdsync::ListSent> = (0..5).map(|_| Default::default()).collect();
    let mut laptops: Vec<fdsync::ClubList> = vec![Default::default(); 5];
    let (mut bytes, mut lines) = (0usize, 0usize);
    let feed = |w: &[u8], laptop: &mut fdsync::ClubList| -> usize {
        let text = std::str::from_utf8(w).unwrap();
        for l in text.lines() {
            assert!(l.len() < fdsync::MAX_LINE_BYTES, "a line past the cap");
            match fdsync::decode_line(l) {
                Some(fdsync::Msg::Rows(r)) => laptop.apply(&r, 100),
                other => panic!("{other:?}"),
            }
        }
        text.lines().count()
    };
    for (s, l) in sent.iter_mut().zip(laptops.iter_mut()) {
        let mut w = Vec::new();
        s.write(&mut w, &engine_lock(&host).fd_club_list(), true)
            .unwrap();
        feed(&w, l);
    }
    // 400 a position here, so the suite stays quick (the host rebuilds its list after every
    // contact, which is quadratic in debug). `CLUB_TRAFFIC_PER_POSITION=2000` runs the five-by-two-
    // thousand Field Day: about 200 bytes a contact a laptop, as here.
    let per_position: u64 = std::env::var("CLUB_TRAFFIC_PER_POSITION")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let t0 = 1_792_000_000u64;
    for i in 0..per_position * 5 {
        let p = (i % 5) as usize;
        let seq = i / 5 + 1;
        let row = fdsync::WireQso {
            pos: positions[p].into(),
            seq,
            // Every fifth station is worked again by another position: a duplicate.
            call: format!("K{}{}", i % 1_700, ["AB", "CD", "EF"][(i % 3) as usize]),
            band: "20m".into(),
            mode: "CW".into(),
            when: t0 + i / 2,
            op: "W9XYZ".into(),
            ex: vec![
                fdsync::WireField {
                    k: "RST".into(),
                    d: String::new(),
                    r: "599".into(),
                },
                fdsync::WireField {
                    k: "QTH".into(),
                    d: "il_counties".into(),
                    r: counties[(i % 8) as usize].into(),
                },
            ],
            mex: vec![
                fdsync::WireField {
                    k: "RST".into(),
                    d: String::new(),
                    r: "599".into(),
                },
                fdsync::WireField {
                    k: "QTH".into(),
                    d: "il_counties".into(),
                    r: "KANE".into(),
                },
            ],
            ..Default::default()
        };
        engine_lock(&host)
            .fd_club_merge(holds[p], &row)
            .expect("merged");
        let view = engine_lock(&host).fd_club_list();
        for (s, l) in sent.iter_mut().zip(laptops.iter_mut()) {
            let mut w = Vec::new();
            s.write(&mut w, &view, false).unwrap();
            bytes += w.len();
            lines += feed(&w, l);
        }
        if i % 1_000 == 999 {
            let want = list(&host).unwrap();
            for l in &laptops {
                assert_eq!(l.total, want.total);
                assert_eq!(
                    l.rows
                        .iter()
                        .map(|r| (&r.pos[..], r.seq, r.dupe, &r.name[..]))
                        .collect::<Vec<_>>(),
                    want.rows
                        .iter()
                        .map(|r| (&r.posid[..], r.seq, r.dupe, &r.pos_name[..]))
                        .collect::<Vec<_>>(),
                    "after {} contacts",
                    i + 1
                );
            }
        }
    }
    let contacts = per_position * 5;
    let per_laptop = bytes / 5;
    let per_contact = per_laptop as u64 / contacts;
    // A late joiner's catch-up: the whole list, once.
    let mut join = Vec::new();
    fdsync::ListSent::default()
        .write(&mut join, &engine_lock(&host).fd_club_list(), true)
        .unwrap();
    eprintln!(
        "club list traffic, 5 positions x {per_position} contacts: {bytes} bytes in {lines} lines to five laptops, \
         {per_laptop} bytes a laptop, {per_contact} bytes a contact a laptop; a joining laptop's catch-up {} bytes",
        join.len()
    );
    assert!(per_contact < 600, "{per_contact} bytes a contact a laptop");
    assert!(join.len() < 64 * 1024, "a catch-up of {} bytes", join.len());
    assert_eq!(list(&host).unwrap().total, contacts);
    let _ = std::fs::remove_dir_all(&dir);
}
