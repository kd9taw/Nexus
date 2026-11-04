//! The Yaesu G-5500's own interface — a GS-232B, Hamlib model 603 — driven through the REAL
//! Hamlib backend, with no hardware. rotctld opens a pseudo-terminal as its serial port, and a
//! stand-in controller on the other end answers the one thing the backend reads back: `C2`, the
//! position, as `AZ=aaa  EL=eee` (the reply Hamlib's `gs232b_rot_get_position` parses). What
//! these tests assert is the SERIAL LINE: the bytes a G-5500's controller would receive for each
//! thing the Rotor pane does. The stand-in never moves; the mast's motion is not what is under
//! test here (`rotctld_manual.rs` watches motion on Hamlib's dummy).
//!
//! It is the closest thing to the operator's station this box has. It is not the station: a
//! real GS-232B's firmware, its timing and the G-5500's own limits are not in it (NEEDS-BENCH).
//!
//! Unix only (the pty), and only with the `serial` feature, whose `serialport` makes the pair.
//! Skips audibly without a `rotctld`, and never under CI (see `common::require_daemon`).
#![cfg(all(unix, feature = "serial"))]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serialport::{SerialPort, TTYPort};
use tempo_audio::rotator::{self, RotKind};

mod common;

/// One daemon at a time, as in the other rotctld suites: parallel spawn/kill flakes under churn.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A GS-232B at `az`/`el` on the far end of a pty, and the rotctld (model 603) that drives it.
struct Gs232b {
    daemon: Child,
    addr: String,
    wire: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    // Held so the pty stays open for the daemon: its slave end, and the reader's thread.
    _slave: TTYPort,
    reader: Option<std::thread::JoinHandle<()>>,
}

/// How the stand-in controller answers `C2`.
#[derive(Clone, Copy)]
enum Answer {
    /// At once, with this position.
    At(u32, u32),
    /// After this long, as a controller across a network would — and with an azimuth that
    /// counts the `C2`s it has answered (1, 2, 3 …), so a read can tell its own answer from the
    /// answer to an earlier one.
    LateCounting(Duration),
}

impl Gs232b {
    fn start(bin: &str, az: u32, el: u32) -> Gs232b {
        Gs232b::start_on(bin, Answer::At(az, el), |path, port| {
            [
                "-m",
                "603",
                "-r",
                path,
                "-s",
                "9600",
                "-T",
                "127.0.0.1",
                "-t",
                port,
            ]
            .map(String::from)
            .to_vec()
        })
    }

    /// A GS-232B answering as `answer` says, driven by a rotctld started with `args(pty, port)`.
    fn start_on(bin: &str, answer: Answer, args: impl Fn(&str, &str) -> Vec<String>) -> Gs232b {
        let (mut master, slave) = TTYPort::pair().expect("a pseudo-terminal pair");
        let path = slave.name().expect("the pty's path");
        let wire = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, halt) = (wire.clone(), stop.clone());
        master
            .set_timeout(Duration::from_millis(50))
            .expect("a read timeout");
        // The late answers go out from a thread of their own, each at its due time.
        let (late, due) = std::sync::mpsc::channel::<(Instant, u32)>();
        let mut writer = master
            .try_clone_native()
            .expect("a second handle on the pty");
        std::thread::spawn(move || {
            for (at, n) in due {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                let _ = writer.write_all(format!("AZ={n:03}  EL=045\r\n").as_bytes());
            }
        });
        let reader = std::thread::spawn(move || {
            let mut answered = 0u32;
            let mut pending = Vec::new();
            let mut buf = [0u8; 256];
            while !halt.load(Ordering::SeqCst) {
                let n = match master.read(&mut buf) {
                    Ok(n) => n,
                    Err(_) => continue, // a timeout, or nobody on the other end yet
                };
                pending.extend_from_slice(&buf[..n]);
                while let Some(end) = pending.iter().position(|&b| b == b'\r') {
                    let cmd = String::from_utf8_lossy(&pending[..end]).trim().to_string();
                    pending.drain(..=end);
                    if cmd.is_empty() {
                        continue; // Hamlib writes a second CR after each set command
                    }
                    if cmd == "C2" {
                        answered += 1;
                        match answer {
                            Answer::At(az, el) => {
                                let _ = master
                                    .write_all(format!("AZ={az:03}  EL={el:03}\r\n").as_bytes());
                            }
                            // Every answer is late by the same round trip, and in order.
                            Answer::LateCounting(rtt) => {
                                let _ = late.send((Instant::now() + rtt, answered));
                            }
                        }
                    }
                    log.lock().expect("the wire log").push(cmd);
                }
            }
        });
        // A port read off a `:0` bind is free only until someone else takes it, and rotctld exits
        // when it cannot listen: so a daemon that dies before it answers is tried on a new port.
        let (daemon, addr) = (0..5)
            .find_map(|_| {
                let port = std::net::TcpListener::bind("127.0.0.1:0")
                    .expect("a free port")
                    .local_addr()
                    .expect("its number")
                    .port();
                let mut daemon = Command::new(bin)
                    .args(args(&path, &port.to_string()))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn rotctld");
                let addr = format!("127.0.0.1:{port}");
                // Ready = alive and answering `\dump_state` (which touches no serial port).
                let deadline = Instant::now() + Duration::from_secs(10);
                while Instant::now() < deadline {
                    if daemon.try_wait().expect("try_wait").is_some() {
                        return None;
                    }
                    if TcpStream::connect(&addr).is_ok() && rotator::read_limits(&addr).is_ok() {
                        return Some((daemon, addr));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = daemon.kill();
                let _ = daemon.wait();
                None
            })
            .expect("rotctld never answered as a GS-232B on the pty, on five ports");
        let mut rig = Gs232b {
            daemon,
            addr,
            wire,
            stop,
            _slave: slave,
            reader: Some(reader),
        };
        rig.take();
        rig
    }

    /// The commands that reached the serial line since the last take, once the line has been
    /// quiet for a little longer than Hamlib's GS-232B post-write delay (50 ms).
    fn take(&mut self) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = usize::MAX;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let now = self.wire.lock().expect("the wire log").len();
            if now == seen || Instant::now() >= deadline {
                break;
            }
            seen = now;
        }
        std::mem::take(&mut *self.wire.lock().expect("the wire log"))
    }
}

impl Drop for Gs232b {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.reader.take() {
            let _ = t.join();
        }
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
fn a_g5500_on_a_gs232b_keeps_its_elevation_when_the_beam_is_turned() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut g5500 = Gs232b::start(&bin, 123, 45);

    // What the backend declares — how Nexus knows this rotator has an elevation axis at all,
    // and that it reaches 0–180° (the G-5500's travel).
    let limits = rotator::read_limits(&g5500.addr)
        .expect("rotctld answers")
        .expect("a Hamlib daemon declares its limits");
    assert_eq!(limits.kind, Some(RotKind::AzEl));
    assert_eq!(limits.elevation(), Some((0.0, 180.0)));
    assert!(
        g5500.take().is_empty(),
        "\\dump_state never touches the serial line"
    );

    // THE BUG, on the controller's own line: the old azimuth move commands elevation 0.
    rotator::point(&g5500.addr, 200.0).expect("RPRT 0");
    assert_eq!(g5500.take(), ["W200 000"]);

    // The azimuth move now: read where the mast is, then send its elevation back.
    rotator::point_keeping(&g5500.addr, Some(200.0), None).expect("RPRT 0");
    assert_eq!(g5500.take(), ["C2", "W200 045"]);

    // An elevation move keeps the azimuth the same way.
    rotator::point_keeping(&g5500.addr, None, Some(60.0)).expect("RPRT 0");
    assert_eq!(g5500.take(), ["C2", "W123 060"]);

    // Both at once need no reading.
    rotator::point_keeping(&g5500.addr, Some(10.0), Some(170.0)).expect("RPRT 0");
    assert_eq!(g5500.take(), ["W010 170"]);
}

#[test]
fn stop_on_a_gs232b_is_the_all_stop() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut g5500 = Gs232b::start(&bin, 123, 45);
    rotator::stop(&g5500.addr).expect("RPRT 0");
    // `S` is the GS-232B's "All Stop": azimuth and elevation both (Hamlib's gs232a backend names
    // it so; the single-axis stops are `A` and `E`, and nothing Nexus sends uses them).
    assert_eq!(g5500.take(), ["S"]);
}

#[test]
fn the_pane_reads_both_axes_from_a_gs232b() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut g5500 = Gs232b::start(&bin, 123, 45);
    assert_eq!(
        rotator::read_position(&g5500.addr).expect("a position"),
        (123.0, Some(45.0))
    );
    assert_eq!(g5500.take(), ["C2"]);
}

/// ⭐ A GS-232B across a network (2026-11-03, a tester's rotators "losing connection" over the
/// internet): every answer arrives 700 ms after its `C2`. Hamlib's own 400 ms timeout gives up on
/// each read, flushes, writes `C2` again, and then reads the answer to the EARLIER `C2` — so a
/// read returns an old position and the controller is sent more commands than were asked for.
/// Under the integrated daemon's own arguments (`rotctld_args`, with its `-C`), each read gets
/// its own answer.
#[test]
fn a_gs232b_across_a_slow_link_answers_each_read_with_its_own_position() {
    let bin = require_rotctld!();
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rtt = Duration::from_millis(700);
    let reads = |g: &mut Gs232b| -> (Vec<f64>, usize) {
        let az: Vec<f64> = (0..4)
            .map(|_| {
                let (az, _) = rotator::read_position(&g.addr).expect("a position, late or not");
                // A poll's pace, so a stale answer has time to land where it would on a pane.
                std::thread::sleep(Duration::from_millis(300));
                az
            })
            .collect();
        std::thread::sleep(Duration::from_millis(1500));
        let c2 = g.take().iter().filter(|c| *c == "C2").count();
        (az, c2)
    };

    // THE DEFECT, as Hamlib's defaults run it: more `C2`s than reads, and reads answered by an
    // earlier `C2`.
    let mut plain = Gs232b::start_on(&bin, Answer::LateCounting(rtt), |path, port| {
        [
            "-m",
            "603",
            "-r",
            path,
            "-s",
            "9600",
            "-T",
            "127.0.0.1",
            "-t",
            port,
        ]
        .map(String::from)
        .to_vec()
    });
    let (az, c2) = reads(&mut plain);
    assert!(
        c2 > 4,
        "Hamlib's 400 ms timeout re-sends C2 on a 700 ms link ({c2} for 4 reads, {az:?})"
    );
    drop(plain);

    // The integrated daemon's arguments: one `C2` per read, each answered in turn.
    let mut nexus = Gs232b::start_on(&bin, Answer::LateCounting(rtt), |path, port| {
        tempo_audio::rigctld_proc::rotctld_args(603, path, 9600, port.parse().expect("a port"))
    });
    let (az, c2) = reads(&mut nexus);
    assert_eq!(c2, 4, "one C2 per read: {az:?}");
    assert_eq!(
        az,
        [1.0, 2.0, 3.0, 4.0],
        "each read is answered by its own C2"
    );
}
