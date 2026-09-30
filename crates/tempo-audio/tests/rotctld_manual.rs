//! Moving an az/el rotator by hand, against a REAL Hamlib `rotctld -m 1` — the dummy rotator,
//! which slews both axes over time and reports where it is, so what a manual move does to the
//! axis it was NOT given can be watched rather than assumed.
//!
//! ⭐ The defect this file exists for: an azimuth move sent `P <az> 0` on every rotator, and on
//! an az/el mount that commands the elevation to 0 — every turn of the beam laid the antenna on
//! the horizon (a G-5500 on a GS-232B gets `W<az> 000`; `rotctld_gs232b.rs` shows that on the
//! serial line). `point_keeping` now keeps the elevation where the rotator reports it.
//!
//! Also here, because it is the same mast: STOP (`S`) halts BOTH axes — the one Stop button in
//! the Rotor pane is the elevation's stop too.
//!
//! Skips audibly without a `rotctld`, and never under CI (see `common::require_daemon`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tempo_audio::rotator::{self, RotKind};

mod common;

/// One daemon at a time, as in the pass suite: parallel spawn/kill flakes under churn.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

/// Per-process ports, clear of the rigctld suite's and the pass suite's (41 000 up).
fn unique_port() -> u16 {
    let base = 45_000 + (std::process::id() % 10_000) as u16;
    base + NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

/// A live `rotctld -m 1` on its own port, killed on drop.
struct Dummy {
    child: Child,
    addr: String,
}

impl Dummy {
    fn spawn(bin: &str) -> Dummy {
        for attempt in 0..5 {
            let port = unique_port();
            let mut child = Command::new(bin)
                .args(["-m", "1", "-T", "127.0.0.1", "-t", &port.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn rotctld");
            let addr = format!("127.0.0.1:{port}");
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if child.try_wait().expect("try_wait").is_some() {
                    break; // the port was taken: try another
                }
                if let Ok(mut s) = TcpStream::connect(&addr) {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                    let mut line = String::new();
                    if s.write_all(b"p\n").is_ok()
                        && BufReader::new(&s).read_line(&mut line).is_ok()
                        && line.trim().parse::<f64>().is_ok()
                    {
                        return Dummy { child, addr };
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("rotctld did not come up on {addr} (attempt {attempt})");
        }
        panic!("rotctld failed to start after 5 attempts");
    }

    /// Where the daemon says it is, read around the code under test.
    fn at(&self) -> (f64, f64) {
        for _ in 0..3 {
            if let Ok(mut s) = TcpStream::connect(&self.addr) {
                let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
                if s.write_all(b"p\n").is_ok() {
                    let mut r = BufReader::new(&s);
                    let (mut az, mut el) = (String::new(), String::new());
                    if r.read_line(&mut az).is_ok() && r.read_line(&mut el).is_ok() {
                        if let (Ok(a), Ok(e)) = (az.trim().parse(), el.trim().parse()) {
                            return (a, e);
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("no position from the dummy rotator");
    }

    /// Wait until the mast is within 1° of `(az, el)` on the axes given.
    fn arrive(&self, az: Option<f64>, el: Option<f64>) -> (f64, f64) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (a, e) = self.at();
            let near = |got: f64, want: Option<f64>| want.is_none_or(|w| (got - w).abs() < 1.0);
            if near(a, az) && near(e, el) {
                return (a, e);
            }
            assert!(
                Instant::now() < deadline,
                "never reached {az:?}/{el:?}: stuck at {a}/{e}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl Drop for Dummy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

macro_rules! require_rotctld {
    () => {
        match common::require_daemon("rotctld", "NEXUS_ROTCTLD") {
            Some(bin) => bin,
            None => return,
        }
    };
}

#[test]
fn an_azimuth_move_keeps_the_elevation_where_it_is() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = Dummy::spawn(&bin);

    // The antenna is raised (with the pass's own az/el line, which this change leaves alone),
    // then the operator turns it.
    rotator::point_azel(&rot.addr, 0.0, 20.0).expect("the elevation is accepted");
    rot.arrive(None, Some(20.0));
    rotator::point_keeping(&rot.addr, Some(40.0), None).expect("the azimuth is accepted");
    let (_, el) = rot.arrive(Some(40.0), None);
    assert!(
        (el - 20.0).abs() < 1.0,
        "the azimuth move dropped the elevation to {el}° — it was 20°"
    );
}

#[test]
fn stop_halts_both_axes() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = Dummy::spawn(&bin);

    let start = rot.at();
    rotator::point_azel(&rot.addr, 180.0, 80.0).expect("a far target on both axes");
    std::thread::sleep(Duration::from_millis(700));
    let moving = rot.at();
    // The positive control: both axes really are on their way, or "stopped" proves nothing.
    assert!(
        moving.0 != start.0 && moving.1 != start.1,
        "both axes should be moving: {start:?} → {moving:?}"
    );
    rotator::stop(&rot.addr).expect("rotctld accepts S");
    let stopped = rot.at();
    std::thread::sleep(Duration::from_millis(800));
    let later = rot.at();
    assert_eq!(later, stopped, "an axis kept moving after STOP");
    assert!(
        later.0 < 170.0 && later.1 < 75.0,
        "it stopped short of the target: {later:?}"
    );
}

#[test]
fn the_daemon_declares_the_range_a_typed_elevation_must_fall_in() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = Dummy::spawn(&bin);

    let limits = rotator::read_limits(&rot.addr)
        .expect("the daemon answers")
        .expect("a Hamlib rotctld declares its limits");
    assert_eq!(limits.kind, Some(RotKind::AzEl));
    assert_eq!(limits.elevation(), Some((0.0, 90.0)), "the dummy's 0–90°");
    // 95° is past the dummy's stop: refused by Nexus, and nothing moves.
    let before = rot.at();
    let e = rotator::point_keeping(&rot.addr, None, Some(95.0)).expect_err("past the stop");
    assert!(e.to_string().contains("0–90°"), "{e}");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rot.at(), before, "a refused move moved the mast");
}
