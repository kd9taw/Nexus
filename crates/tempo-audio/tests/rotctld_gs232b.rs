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

impl Gs232b {
    fn start(bin: &str, az: u32, el: u32) -> Gs232b {
        let (mut master, slave) = TTYPort::pair().expect("a pseudo-terminal pair");
        let path = slave.name().expect("the pty's path");
        let wire = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, halt) = (wire.clone(), stop.clone());
        master
            .set_timeout(Duration::from_millis(50))
            .expect("a read timeout");
        let reader = std::thread::spawn(move || {
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
                        let _ = master.write_all(format!("AZ={az:03}  EL={el:03}\r\n").as_bytes());
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
                    .args([
                        "-m",
                        "603",
                        "-r",
                        &path,
                        "-s",
                        "9600",
                        "-T",
                        "127.0.0.1",
                        "-t",
                        &port.to_string(),
                    ])
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
