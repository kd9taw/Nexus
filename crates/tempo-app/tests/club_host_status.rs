//! Where each club position's link stands on the host's board, and the host's Remove, through the
//! whole stack over 127.0.0.1: real engines, the REAL `fdbridge` impls, `fdsync::serve_until` and
//! the real position pumps, on the Illinois QSO Party (the club's next event). Loopback only,
//! ephemeral ports.
//!
//! Each position joins through a relay that passes every byte and keeps the host's lines to it
//! (the wire tap), and that can go quiet both ways the way a dead Wi-Fi link does (no FIN, no
//! RST) or be pointed at a restarted host. Everything is read by value off the snapshots the
//! screens read: the host's board and hosting line, each laptop's club link, the club's files.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempo_app::dto::{ClubLinkDto, FdClubDto};
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::fdbridge::{EngineClubBackend, EnginePositionSync};
use tempo_app::fdevent::{RemoveRefusal, POSITION_REMOVED};
use tempo_net::fdsync::{self, ClubBackend, PositionSync};

type Shared = Arc<Mutex<Engine>>;
/// The host's lines on each connection a position made through a [`Relay`], in order.
type Tapped = Arc<Mutex<Vec<Vec<String>>>>;

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

/// A host on a port of its own, its club journal at `journal`, joined by its own pump: the
/// address it serves at, and the shutdown flags of both.
fn start_host(host: &Shared, journal: std::path::PathBuf) -> (String, Vec<Arc<AtomicBool>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let addr = format!("127.0.0.1:{port}");
    {
        let mut e = engine_lock(host);
        let mut s = e.settings().clone();
        s.fd_host_enable = true;
        s.fd_host_port = port;
        e.apply_settings(s);
        e.fd_host_start(journal).expect("the club starts");
    }
    let sd = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn ClubBackend> = Arc::new(EngineClubBackend(host.clone()));
    let sd2 = sd.clone();
    std::thread::spawn(move || fdsync::serve_until(listener, backend, sd2));
    (addr.clone(), vec![sd, start_pump(host, &addr)])
}

fn start_pump(eng: &Shared, addr: &str) -> Arc<AtomicBool> {
    let sd = Arc::new(AtomicBool::new(false));
    let backend: Arc<dyn PositionSync> = Arc::new(EnginePositionSync::new(eng.clone()));
    let (a, sd2) = (addr.to_string(), sd.clone());
    std::thread::spawn(move || fdsync::run_position_until(&a, backend, sd2));
    sd
}

/// ⭐ **A relay between one position and its host**: every byte passes through, line by line;
/// the host's lines to the position are kept per connection (the wire tap); it can go QUIET both
/// ways, holding every line and every close the way a dead Wi-Fi link goes silent without a FIN
/// or an RST, and pass them on when it comes back; and it can be pointed at a restarted host on
/// a port of its own, so no port is ever let go and bound again.
struct Relay {
    addr: String,
    to: Arc<Mutex<String>>,
    quiet: Arc<AtomicBool>,
    /// The host's lines on each connection the position made, in order.
    tapped: Tapped,
}

impl Relay {
    fn start(to: &str) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let relay = Relay {
            addr: format!("127.0.0.1:{}", listener.local_addr().unwrap().port()),
            to: Arc::new(Mutex::new(to.to_string())),
            quiet: Arc::new(AtomicBool::new(false)),
            tapped: Arc::new(Mutex::new(Vec::new())),
        };
        let (to, quiet, tapped) = (relay.to.clone(), relay.quiet.clone(), relay.tapped.clone());
        std::thread::spawn(move || {
            for position in listener.incoming().flatten() {
                let target = to.lock().unwrap().clone();
                let Ok(host) = TcpStream::connect(&target) else {
                    continue;
                };
                let n = {
                    let mut t = tapped.lock().unwrap();
                    t.push(Vec::new());
                    t.len() - 1
                };
                pass(
                    position.try_clone().unwrap(),
                    host.try_clone().unwrap(),
                    &quiet,
                    None,
                );
                pass(host, position, &quiet, Some((tapped.clone(), n)));
            }
        });
        relay
    }

    /// How many connections the position has made through the relay.
    fn connections(&self) -> usize {
        self.tapped.lock().unwrap().len()
    }

    /// The host's lines on connection `n`, decoded.
    fn heard(&self, n: usize) -> Vec<fdsync::Msg> {
        self.tapped.lock().unwrap()[n]
            .iter()
            .filter_map(|l| fdsync::decode_line(l))
            .collect()
    }
}

/// One direction of a relayed connection, line by line, holding each line and the close while
/// the relay is quiet.
fn pass(
    from: TcpStream,
    mut into: TcpStream,
    quiet: &Arc<AtomicBool>,
    tap: Option<(Tapped, usize)>,
) {
    let quiet = quiet.clone();
    std::thread::spawn(move || {
        let mut r = BufReader::new(from.try_clone().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            let closed = !matches!(r.read_line(&mut line), Ok(n) if n > 0);
            while quiet.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(20));
            }
            if closed {
                break;
            }
            if let Some((tapped, n)) = &tap {
                tapped.lock().unwrap()[*n].push(line.trim_end().to_string());
            }
            if into.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
        let _ = into.shutdown(Shutdown::Both);
        let _ = from.shutdown(Shutdown::Both);
    });
}

/// Wait up to `secs` for `cond`; past it, panic naming what `show` reads.
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

/// Log `call` from `county` on `eng`.
fn party(eng: &Shared, call: &str, county: &str) {
    let rows = vec![vec![
        ("RST".to_string(), "599".to_string()),
        ("QTH".to_string(), county.to_string()),
    ]];
    let logged = engine_lock(eng)
        .contest_log_manual_rows(call, &rows, "CW", None)
        .expect("the party runs");
    assert!(logged.iter().all(|&ok| ok), "{call} refused: fixture bug");
}

/// The host's screen, as one snapshot reads it: its club block and its club link.
fn host_screen(host: &Shared) -> (FdClubDto, ClubLinkDto) {
    let s = engine_lock(host).snapshot();
    (
        s.field_day.and_then(|f| f.club).expect("the club block"),
        s.club_link.expect("the club link"),
    )
}

/// The link word on the host's board for `posid`, and its heard seconds.
fn board_link(club: &FdClubDto, posid: &str) -> (Option<String>, Option<u64>) {
    club.board
        .iter()
        .find(|r| r.posid == posid)
        .map_or((None, None), |r| (r.link.clone(), r.heard_secs))
}

/// The board's Connected rows and the hosting line's count, from the one snapshot.
fn counts(club: &FdClubDto, link: &ClubLinkDto) -> (usize, u32) {
    (
        club.board
            .iter()
            .filter(|r| r.link.as_deref() == Some("connected"))
            .count(),
        link.hosting.as_ref().expect("the hosting line").connected,
    )
}

/// A laptop's own club link: its state, its status word and its heard seconds, and what the host
/// told it.
fn own_link(eng: &Shared) -> (String, Option<String>, Option<u64>, Option<String>) {
    let l = engine_lock(eng)
        .snapshot()
        .club_link
        .expect("the club link");
    (l.state, l.status, l.heard_secs, l.told)
}

/// ⭐ **A position's link reads Connected, then Reconnecting, then Lost, then Connected again, on
/// the host's board and on the position's own club line, and the host's count of connected
/// positions agrees with its board in every snapshot.** The relay between the CW tent and the host
/// goes quiet both ways, the way a dead Wi-Fi link does: the host's board says Reconnecting once
/// two heartbeats are missed and Lost past the dead timeout; the CW tent says the same of the
/// host; the host closes the link on its own clock, no later than 2 s after both ends say Lost;
/// when the link comes back the tent rejoins on its own and both say Connected. CONTROLS: the
/// Phone tent, on its own link, reads Connected throughout, and the host's own line reads "host".
#[test]
fn a_positions_link_reads_connected_reconnecting_lost_and_connected_again_at_both_ends() {
    let dir = std::env::temp_dir().join(format!("club-status-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let host = ilqp_engine("aaaa0001", "HQ", "");
    let (addr, mut sds) = start_host(&host, dir.join("fd_event_status.jsonl"));
    let relay = Relay::start(&addr);
    let cw = ilqp_engine("bbbb0002", "CW tent", &relay.addr);
    let phone = ilqp_engine("cccc0003", "Phone tent", &addr);
    sds.push(start_pump(&cw, &relay.addr));
    sds.push(start_pump(&phone, &addr));

    // Every snapshot read below: the board's Connected rows against the hosting line's count, and,
    // once all three are in, the CONTROLS.
    let seen = Mutex::new(Vec::<(Option<String>, Option<String>)>::new());
    let all_in = AtomicBool::new(false);
    let sample = || {
        let (club, link) = host_screen(&host);
        let (rows, line) = counts(&club, &link);
        assert_eq!(
            rows as u32, line,
            "the hosting line disagrees with the board: {:?}",
            club.board
        );
        let (word, _) = board_link(&club, "bbbb0002");
        if all_in.load(Ordering::Relaxed) {
            assert_eq!(
                board_link(&club, "cccc0003").0.as_deref(),
                Some("connected"),
                "CONTROL"
            );
            assert_eq!(
                board_link(&club, "aaaa0001").0.as_deref(),
                Some("host"),
                "CONTROL"
            );
        }
        let mine = own_link(&cw).1;
        let mut s = seen.lock().unwrap();
        if s.last() != Some(&(word.clone(), mine.clone())) {
            s.push((word.clone(), mine.clone()));
        }
        (word, mine, line)
    };
    let show = || format!("{:?}", seen.lock().unwrap());
    // The host's own row is on its board only once its own loopback JOIN has been served. The
    // serve loop polls accept every 200 ms and serves each connection on a thread of its own, so
    // the three JOINs can be served in any order, and the row can come in after both tents: it is
    // waited for with them, before the CONTROLS read it.
    wait_showing(
        "both tents connected, on the host's board and the CW tent's own line, and the host's row",
        10,
        || {
            let (word, mine, line) = sample();
            word.as_deref() == Some("connected")
                && mine.as_deref() == Some("connected")
                && line == 2
                && board_link(&host_screen(&host).0, "aaaa0001").0.as_deref() == Some("host")
        },
        show,
    );
    let (state, _, heard, _) = own_link(&cw);
    assert_eq!(state, "joined");
    assert!(heard.is_some_and(|h| h <= 10), "heard {heard:?}");
    all_in.store(true, Ordering::Relaxed);
    sample();

    relay.quiet.store(true, Ordering::Relaxed);
    let quiet = Instant::now();
    wait_showing(
        "the host's board says Reconnecting, and its count drops",
        14,
        || {
            let (word, _, line) = sample();
            word.as_deref() == Some("reconnecting") && line == 1
        },
        show,
    );
    let (club, _) = host_screen(&host);
    let heard = board_link(&club, "bbbb0002").1.expect("heard before");
    assert!(
        (11..=15).contains(&heard),
        "Reconnecting is heard 11 to 15 s ago: {heard}"
    );
    wait_showing(
        "both ends say Lost past the dead timeout",
        14,
        || {
            let (word, mine, _) = sample();
            word.as_deref() == Some("lost") && mine.as_deref() == Some("lost")
        },
        show,
    );
    assert!(
        quiet.elapsed() >= Duration::from_secs(fdsync::DEAD_SECS - fdsync::PING_SECS),
        "Lost no sooner than the dead timeout after the last heartbeat: {:?}",
        quiet.elapsed()
    );
    let (club, _) = host_screen(&host);
    assert!(board_link(&club, "bbbb0002")
        .1
        .is_some_and(|h| h > fdsync::DEAD_SECS));
    // Lost is the heard age in whole seconds (`PositionLink::of`); the host closes the link on
    // its own clock, once its serve loop's 250 ms read tick finds the socket silent past the dead
    // timeout, and then takes the engine lock. So the close can trail both ends' Lost by a tick
    // or so, more on a loaded box: it is waited for, up to 2 s (eight ticks).
    wait_showing(
        "the host closed the link, no later than 2 s after both ends said Lost",
        2,
        || {
            !engine_lock(&host)
                .fd_club_log()
                .unwrap()
                .connected("bbbb0002")
        },
        show,
    );
    // The tent's end closes on the same rule: its pump's 250 ms read tick finds the host silent
    // for `DEAD_SECS` (a monotonic clock), while its own line reads Lost off the whole seconds
    // since it last heard the host, which can turn up to a tick earlier. A link restored inside
    // that tick delivers the host's held lines and then its close to a session still open: the
    // tent hears the host and loses the link in one breath, and truthfully reads Reconnecting
    // before it rejoins. The link comes back only once both ends have closed it, as on the dead
    // Wi-Fi link this stands in for.
    wait_showing(
        "the CW tent closed its end too, no later than 2 s after both ends said Lost",
        2,
        || own_link(&cw).0 != "joined",
        show,
    );

    relay.quiet.store(false, Ordering::Relaxed);
    wait_showing(
        "the CW tent rejoins on its own, and both ends say Connected",
        20,
        || {
            let (word, mine, line) = sample();
            word.as_deref() == Some("connected")
                && mine.as_deref() == Some("connected")
                && line == 2
        },
        show,
    );
    // Each end went through every word in order, and no other.
    let host_words: Vec<String> = {
        let mut w: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(h, _)| h.clone())
            .collect();
        w.dedup();
        w
    };
    let own_words: Vec<String> = {
        let mut w: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, m)| m.clone())
            .collect();
        w.dedup();
        w
    };
    let want = ["connected", "reconnecting", "lost", "connected"]
        .map(String::from)
        .to_vec();
    assert_eq!(host_words, want, "the host's board: {}", show());
    assert_eq!(own_words, want, "the CW tent's own line: {}", show());

    for sd in sds {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The club's contacts as the screens and the files read them.
#[derive(Debug, PartialEq)]
struct ClubContacts {
    /// The club's rows, as the journal writes them.
    rows: String,
    /// The dupe keys every position is sent.
    keys: Vec<Vec<String>>,
    /// The score and QSO count every position is sent.
    score: (u32, u64),
    cabrillo: String,
    adif: String,
    /// Each position's QSO count on the board.
    board: Vec<(String, u64)>,
}

fn club_contacts(host: &Shared) -> ClubContacts {
    let mut e = engine_lock(host);
    let club = e.fd_club_log().expect("hosting");
    let rows = serde_json::to_string(club.rows()).unwrap();
    let keys = club.dkeys().to_vec();
    let mut board: Vec<(String, u64)> = club
        .board_rows(0)
        .into_iter()
        .map(|r| (r.pos, r.qsos))
        .collect();
    board.sort();
    let state = e.fd_club_state(0, 0, "");
    let (cabrillo, adif) = (
        e.fd_club_export(true).expect("the club Cabrillo"),
        e.fd_club_export(false).expect("the club ADIF"),
    );
    ClubContacts {
        rows,
        keys,
        score: (state.score, state.qsos),
        cabrillo,
        adif,
        board,
    }
}

/// Ctrl+D twice at `eng`: the newest contact taken back.
fn take_back_newest(eng: &Shared) {
    let fd = engine_lock(eng)
        .snapshot()
        .field_day
        .expect("the party runs");
    let q = fd.log.last().expect("a contact");
    let (call, when) = (q.call.clone(), q.when_unix);
    engine_lock(eng)
        .contest_remove_last(&call, when)
        .expect("the party runs")
        .unwrap_or_else(|r| panic!("{call} refused: {r:?}"));
}

/// ⭐ **The host removes a position and lets it back, and the contacts it sent never move.** The
/// host presses Remove on the CW tent: within a tick its link is closed with the removal sentence,
/// which its own club line shows as the host's words; each time it tries again it is turned away
/// with the same sentence and nothing else, while the host's board says Removed and its count
/// leaves it out. Its contacts stay in the club's rows, keys, score, board and both files, the same
/// bytes; one it logs while removed stays in its own log. Allow back lets it in on its next try,
/// and it sends that contact up as an offline laptop does. CONTROLS: the host's own position cannot
/// be removed; the Phone tent stays Connected throughout; and the CW tent's take-back still works
/// once it is back, putting the club's file back to the bytes it was before it rejoined.
#[test]
fn the_host_removes_a_position_and_lets_it_back_and_its_contacts_never_move() {
    let dir = std::env::temp_dir().join(format!("club-remove-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let host = ilqp_engine("aaaa0001", "HQ", "");
    let (addr, mut sds) = start_host(&host, dir.join("fd_event_remove.jsonl"));
    let relay = Relay::start(&addr);
    let cw = ilqp_engine("bbbb0002", "CW tent", &relay.addr);
    let phone = ilqp_engine("cccc0003", "Phone tent", &addr);
    sds.push(start_pump(&cw, &relay.addr));
    sds.push(start_pump(&phone, &addr));
    party(&host, "W9HHH", "LAKE");
    party(&cw, "K1ABC", "COOK");
    party(&cw, "W5XYZ", "WILL");
    party(&phone, "N0XX", "DUPG");
    let rows = |h: &Shared| engine_lock(h).fd_club_log().unwrap().rows().len();
    wait_showing(
        "four contacts at the host and both tents Connected",
        10,
        || {
            let (club, link) = host_screen(&host);
            rows(&host) == 4 && counts(&club, &link) == (2, 2)
        },
        || format!("{} rows, {:?}", rows(&host), host_screen(&host).0.board),
    );
    let before = club_contacts(&host);
    assert!(
        before.cabrillo.contains("K1ABC") && before.adif.contains("W5XYZ"),
        "premise: the files hold them"
    );

    assert_eq!(
        engine_lock(&host).fd_club_remove_position("aaaa0001"),
        Err(RemoveRefusal::Own),
        "CONTROL: the host's own position"
    );
    let removed_at = relay.connections();
    assert_eq!(
        engine_lock(&host).fd_club_remove_position("bbbb0002"),
        Ok(())
    );
    let removed = Some(POSITION_REMOVED.to_string());
    wait_showing(
        "the CW tent told why, and its link closed, within a tick",
        2,
        || {
            own_link(&cw).3 == removed
                && !engine_lock(&host)
                    .fd_club_log()
                    .unwrap()
                    .connected("bbbb0002")
        },
        || format!("{:?}", own_link(&cw)),
    );
    let (state, status, _, told) = own_link(&cw);
    assert_eq!(
        (state.as_str(), status, told),
        ("turnedAway", None, removed.clone())
    );
    assert_eq!(
        engine_lock(&cw)
            .snapshot()
            .field_day
            .and_then(|f| f.club)
            .and_then(|c| c.last_error),
        removed,
        "the Contest screen's club block quotes the host"
    );
    let last = relay.heard(removed_at - 1);
    assert!(
        matches!(last.last(), Some(fdsync::Msg::Error { msg }) if msg == POSITION_REMOVED),
        "the live connection's last line is the sentence: {last:?}"
    );

    // It tries again, twice, and each time is told the same and nothing else.
    party(&cw, "K9NEW", "JODA");
    let turned_away = |n: usize| matches!(relay.heard(n).as_slice(), [fdsync::Msg::Error { msg }] if msg == POSITION_REMOVED);
    wait_showing(
        "two more tries, each answered with the sentence and nothing else",
        12,
        || relay.connections() >= removed_at + 2 && (removed_at..removed_at + 2).all(turned_away),
        || {
            (removed_at..relay.connections())
                .map(|n| format!("try {n}: {:?}", relay.heard(n)))
                .collect::<Vec<_>>()
                .join("\n")
        },
    );
    let (club, link) = host_screen(&host);
    assert_eq!(board_link(&club, "bbbb0002").0.as_deref(), Some("removed"));
    assert_eq!(counts(&club, &link), (1, 1), "the count leaves it out");
    assert_eq!(
        board_link(&club, "cccc0003").0.as_deref(),
        Some("connected"),
        "CONTROL"
    );
    assert!(
        club.refused.is_empty(),
        "a removed position is not listed as turned away: {:?}",
        club.refused
    );
    assert_eq!(
        club_contacts(&host),
        before,
        "its contacts, while it is removed"
    );
    let own: Vec<String> = engine_lock(&cw)
        .snapshot()
        .field_day
        .unwrap()
        .log
        .iter()
        .map(|q| q.call.clone())
        .collect();
    assert_eq!(
        own,
        ["K1ABC", "W5XYZ", "K9NEW"],
        "its own log keeps all three"
    );

    assert_eq!(engine_lock(&host).fd_club_allow_back("bbbb0002"), Ok(()));
    wait_showing(
        "the CW tent rejoins on its next try and sends K9NEW up",
        20,
        || {
            let (club, link) = host_screen(&host);
            rows(&host) == 5
                && counts(&club, &link) == (2, 2)
                && own_link(&cw).1.as_deref() == Some("connected")
        },
        || {
            format!(
                "{} rows, {:?}, {:?}",
                rows(&host),
                own_link(&cw),
                host_screen(&host).0.board
            )
        },
    );
    assert!(club_contacts(&host).cabrillo.contains("K9NEW"));
    assert_eq!(own_link(&cw).3, None, "nothing told now");

    // CONTROL: its take-back works as before, and the club's file is the bytes it was.
    take_back_newest(&cw);
    wait_showing(
        "K9NEW taken back at the club",
        10,
        || !club_contacts(&host).cabrillo.contains("K9NEW"),
        || club_contacts(&host).cabrillo,
    );
    assert_eq!(
        club_contacts(&host),
        before,
        "rows, keys, score, both files and the board"
    );

    for sd in sds {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ **A removed position is still turned away after the host restarts, until the restarted host
/// lets it back.** The host removes the CW tent and restarts on its journal (on a new port, the
/// relay pointed at it); the tent, still trying, is turned away by the restarted host with the
/// same sentence, and the board says Removed; Allow back there lets it in. CONTROL: the restarted
/// host has its contacts.
#[test]
fn a_removed_position_is_still_turned_away_after_the_host_restarts() {
    let dir = std::env::temp_dir().join(format!("club-remove-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("fd_event_rr.jsonl");
    let host = ilqp_engine("aaaa0001", "HQ", "");
    let (addr, host_sds) = start_host(&host, journal.clone());
    let relay = Relay::start(&addr);
    let cw = ilqp_engine("bbbb0002", "CW tent", &relay.addr);
    let cw_sd = start_pump(&cw, &relay.addr);
    party(&cw, "K1ABC", "COOK");
    let rows = |h: &Shared| engine_lock(h).fd_club_log().map_or(0, |c| c.rows().len());
    wait_showing(
        "K1ABC at the host",
        10,
        || rows(&host) == 1,
        || rows(&host).to_string(),
    );
    engine_lock(&host)
        .fd_club_remove_position("bbbb0002")
        .unwrap();
    for sd in host_sds {
        sd.store(true, Ordering::Relaxed);
    }

    let restarted = ilqp_engine("aaaa0001", "HQ", "");
    let (addr2, sds) = start_host(&restarted, journal);
    *relay.to.lock().unwrap() = addr2;
    assert_eq!(rows(&restarted), 1, "CONTROL: its contact replayed");
    let tried = relay.connections();
    wait_showing(
        "the CW tent turned away by the restarted host",
        20,
        || {
            relay.connections() > tried
                && matches!(relay.heard(relay.connections() - 1).as_slice(),
                    [fdsync::Msg::Error { msg }] if msg == POSITION_REMOVED)
        },
        || format!("{} connections, {:?}", relay.connections(), own_link(&cw)),
    );
    let (club, _) = host_screen(&restarted);
    assert_eq!(board_link(&club, "bbbb0002").0.as_deref(), Some("removed"));
    engine_lock(&restarted)
        .fd_club_allow_back("bbbb0002")
        .unwrap();
    wait_showing(
        "let back in by the restarted host",
        20,
        || own_link(&cw).1.as_deref() == Some("connected"),
        || format!("{:?}", own_link(&cw)),
    );

    cw_sd.store(true, Ordering::Relaxed);
    for sd in sds {
        sd.store(true, Ordering::Relaxed);
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_dir_all(&dir);
}
