//! CAT over a VIRTUAL serial port: a pseudo-terminal, the kind wfview makes on a Mac
//! (`~/Downloads/rig-ptyN`) and socat makes on Linux, with a stand-in Icom IC-7300 on its master
//! end answering CI-V the way the radio does.
//!
//! Two routes, each through the real production entry:
//! - **Hamlib.** `spawn_rigctld` with the hold-low default, against the real `rigctld`. Before the
//!   virtual-port rule this failed at open, `ser_set_rts: Cannot change RTS - Inappropriate ioctl
//!   for device`: a pseudo-terminal has no RTS to hold, and Hamlib aborts the open when the hold
//!   fails. Run it against a second Hamlib with `NEXUS_RIGCTLD=/path/to/rigctld`.
//! - **Native Icom CI-V.** `CivDaemon::start` on the same kind of port reads the dial and takes a
//!   scope sweep, and never turns the radio's scope output off, since the program behind a
//!   virtual port may be drawing a waterfall from it.
//!
//! Linux only: that is where a pseudo-terminal can be made in a test. The rigctld half skips
//! (audibly, and never on GitHub Actions) where no Hamlib is installed; see `common`.
#![cfg(all(target_os = "linux", feature = "serial"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempo_audio::civ::broker::CivDaemon;
use tempo_audio::civ::commands::IcomModel;
use tempo_audio::rigctld_proc::{spawn_rigctld, ControlLines, KeyingFacts};

mod common;

/// The two tests share the process environment (one sets `PATH`), so they run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Each test's whole budget. Every wait below has its own bound too; this is the backstop for one
/// inside the code under test, so a regression FAILS here with a message instead of hanging CI.
/// A Hamlib whose `-L` dump never exits (4.5.5) costs the dump's own 10 s of it.
const TEST_BUDGET: Duration = Duration::from_secs(35);

/// Ends the whole test binary, with a message, if the test that armed it is still running at
/// [`TEST_BUDGET`]. Disarmed on drop.
struct Watchdog(Option<std::sync::mpsc::Sender<()>>);

impl Watchdog {
    fn arm(test: &'static str) -> Watchdog {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(TEST_BUDGET) {
                eprintln!(
                    "pty_cat: {test} was still running after {} s; failing it instead of hanging",
                    TEST_BUDGET.as_secs()
                );
                std::process::exit(101);
            }
        });
        Watchdog(Some(tx))
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

/// A loopback port nothing is listening on right now. The window before the daemon binds it is
/// tiny, and the test fails, rather than hangs, if something takes it.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free loopback port")
}

const RIG: u8 = 0x94; // IC-7300's CI-V default
const DIAL_HZ: u64 = 14_074_000;

/// A pseudo-terminal: its master end, and the slave's path.
fn open_pty() -> (std::fs::File, String) {
    // SAFETY: plain libc calls on a descriptor this function owns; `ptsname_r` writes at most
    // `buf.len()` bytes, NUL-terminated.
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(m >= 0, "posix_openpt");
        assert_eq!(libc::grantpt(m), 0);
        assert_eq!(libc::unlockpt(m), 0);
        let mut buf = [0 as libc::c_char; 128];
        assert_eq!(libc::ptsname_r(m, buf.as_mut_ptr(), buf.len()), 0);
        let name = std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned();
        (std::fs::File::from_raw_fd(m), name)
    }
}

/// The slave behind a `rig-pty1` symlink in a fresh directory, the shape wfview hands out.
fn wfview_style_link(slave: &str, tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("nexus-pty-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let link = dir.join("rig-pty1");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(slave, &link).expect("symlink");
    link.to_string_lossy().into_owned()
}

/// Five-byte little-endian BCD, CI-V's frequency encoding.
fn bcd5(mut hz: u64) -> [u8; 5] {
    let mut out = [0u8; 5];
    for b in &mut out {
        let lo = (hz % 10) as u8;
        hz /= 10;
        let hi = (hz % 10) as u8;
        hz /= 10;
        *b = (hi << 4) | lo;
    }
    out
}

fn frame(cmd: u8, data: &[u8]) -> Vec<u8> {
    let mut f = vec![0xFE, 0xFE, 0xE0, RIG, cmd];
    f.extend_from_slice(data);
    f.push(0xFD);
    f
}

/// One whole scope sweep in one frame (`27 00`, division 1 of 1), the shape a virtual port
/// carries: center mode, 14.074 MHz ± 25 kHz, in range, then 475 points on the IC-7300's
/// 0–160 scale.
fn sweep_frame() -> Vec<u8> {
    let mut d = vec![0x00, 0x00, 0x01, 0x01, 0x00];
    d.extend_from_slice(&bcd5(DIAL_HZ));
    d.extend_from_slice(&bcd5(25_000));
    d.push(0x00);
    d.extend((0..475usize).map(|i| (i * 160 / 474) as u8));
    frame(0x27, &d)
}

/// A stand-in IC-7300 on the pty's master end. It answers the dial reads, acknowledges the
/// scope switches, NAKs the rest, and once its waveform output is switched on sends one sweep.
/// Every frame it is sent is kept for the asserts.
struct FakeIcom {
    heard: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeIcom {
    fn start(master: std::fs::File) -> FakeIcom {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        // Non-blocking, so the thread can notice `stop`.
        // SAFETY: fcntl on a descriptor `master` owns.
        unsafe {
            use std::os::unix::io::AsRawFd;
            let fd = master.as_raw_fd();
            let fl = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        }
        let thread = {
            let (heard, stop) = (heard.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut m = master;
                let mut acc: Vec<u8> = Vec::new();
                let mut buf = [0u8; 512];
                while !stop.load(Ordering::Relaxed) {
                    match m.read(&mut buf) {
                        Ok(n) if n > 0 => acc.extend_from_slice(&buf[..n]),
                        // EAGAIN, or EIO while no slave is open: wait for one.
                        _ => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                    }
                    while let Some(end) = acc.iter().position(|&b| b == 0xFD) {
                        let f: Vec<u8> = acc.drain(..=end).collect();
                        let Some(start) = f.windows(2).position(|w| w == [0xFE, 0xFE]) else {
                            continue;
                        };
                        let f = f[start..].to_vec();
                        if f.len() < 6 || f[2] != RIG {
                            continue;
                        }
                        heard.lock().unwrap().push(f.clone());
                        let (cmd, data) = (f[4], &f[5..f.len() - 1]);
                        let mut reply = match (cmd, data) {
                            (0x03, _) => frame(0x03, &bcd5(DIAL_HZ)),
                            (0x25, [0x00]) => {
                                let mut d = vec![0x00];
                                d.extend_from_slice(&bcd5(DIAL_HZ));
                                frame(0x25, &d)
                            }
                            (0x27, [0x10..=0x12, ..]) => frame(0xFB, &[]),
                            _ => frame(0xFA, &[]),
                        };
                        if cmd == 0x27 && data == [0x11, 0x01] {
                            reply.extend(sweep_frame());
                        }
                        let _ = m.write_all(&reply);
                    }
                }
            })
        };
        FakeIcom {
            heard,
            stop,
            thread: Some(thread),
        }
    }

    fn heard(&self, cmd: u8, data: &[u8]) -> usize {
        self.heard
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f[4] == cmd && &f[5..f.len() - 1] == data)
            .count()
    }
}

impl Drop for FakeIcom {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Ask a rigctld-protocol server for the dial, retrying while it comes up. Its answer line, or
/// the last error.
fn ask_dial(addr: &str, within: Duration) -> Result<String, String> {
    let until = Instant::now() + within;
    let mut last = String::from("never connected");
    while Instant::now() < until {
        let sock: std::net::SocketAddr = addr.parse().expect("a socket address");
        match TcpStream::connect_timeout(&sock, Duration::from_secs(1)) {
            Ok(mut s) => {
                s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                s.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                if s.write_all(b"f\n").is_ok() {
                    let mut line = String::new();
                    if BufReader::new(&s).read_line(&mut line).is_ok() {
                        let line = line.trim().to_string();
                        if line == DIAL_HZ.to_string() {
                            return Ok(line);
                        }
                        last = line;
                    }
                }
            }
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(last)
}

/// ⭐ HAMLIB OPENS A VIRTUAL PORT. The hold-low default asked rigctld to hold RTS and DTR low,
/// a pseudo-terminal has neither, and Hamlib aborted the open on the failed hold; now a port
/// with no modem lines is opened without the holds, and the rig answers.
#[test]
fn rigctld_opens_a_virtual_port_with_the_hold_low_default_and_reads_the_dial() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let _watchdog = Watchdog::arm("rigctld_opens_a_virtual_port…");
    let Some(bin) = common::require_daemon("rigctld", "NEXUS_RIGCTLD") else {
        return;
    };
    // `spawn_rigctld` finds its daemon on PATH (a test binary has no bundle beside it), so a
    // binary named by NEXUS_RIGCTLD goes first on it.
    if let Some(dir) = std::path::Path::new(&bin)
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        let path = format!(
            "{}:{}",
            dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        std::env::set_var("PATH", path);
    }
    // The copy `spawn_rigctld` will find, by the same PATH lookup, named in every message.
    let version = std::process::Command::new("rigctld")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    eprintln!("pty_cat: {version}");
    let (master, slave) = open_pty();
    let radio = FakeIcom::start(master);
    let port_name = wfview_style_link(&slave, "rigctld");
    let tcp = free_port();
    let daemon = spawn_rigctld(
        3073, // IC-7300
        &port_name,
        115_200,
        tcp,
        false,
        KeyingFacts::default(),
        ControlLines::hold_low(),
    )
    .expect("spawn rigctld");
    // The daemon opens the pty node it was classified as, not the repointable link.
    let served = tempo_audio::rigctld_proc::daemon_serving_port(tcp).and_then(|d| d.device);
    assert_eq!(
        served.map(std::path::PathBuf::from),
        Some(std::fs::canonicalize(&slave).expect("slave")),
        "rigctld -r"
    );
    let answer = ask_dial(&format!("127.0.0.1:{tcp}"), Duration::from_secs(12));
    let said = daemon.said().join("\n");
    assert!(
        !said.contains("Cannot change RTS") && !said.contains("Cannot change DTR"),
        "the open tried to hold a line the port does not have ({version}):\n{said}"
    );
    assert_eq!(answer, Ok(DIAL_HZ.to_string()), "{version}:\n{said}");
    assert!(
        radio.heard(0x03, &[]) > 0,
        "the dial read reached the radio"
    );
}

/// ⭐ NATIVE ICOM CI-V ON A VIRTUAL PORT: the dial reads back, a whole-sweep scope frame becomes
/// a scope row, and the scope output is never switched off, at start or on disable.
#[test]
fn native_civ_on_a_virtual_port_reads_the_dial_takes_a_sweep_and_never_turns_the_scope_off() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let _watchdog = Watchdog::arm("native_civ_on_a_virtual_port…");
    let (master, slave) = open_pty();
    let radio = FakeIcom::start(master);
    let port_name = wfview_style_link(&slave, "native");
    let daemon = CivDaemon::start(&port_name, 115_200, RIG, 0, 1, Some(IcomModel::Ic7300))
        .expect("open the virtual port");
    let addr = daemon.local_addr().to_string();
    assert_eq!(
        ask_dial(&addr, Duration::from_secs(10)),
        Ok(DIAL_HZ.to_string())
    );

    daemon.set_scope_enabled(true);
    let until = Instant::now() + Duration::from_secs(5);
    let mut rows = Vec::new();
    while rows.is_empty() && Instant::now() < until {
        rows = daemon.take_scope_rows();
        std::thread::sleep(Duration::from_millis(20));
    }
    let row = rows.first().expect("the sweep became a scope row");
    assert_eq!(row.row.len(), 475);
    assert_eq!(row.lo_hz, (DIAL_HZ - 25_000) as f64);
    assert_eq!(row.hi_hz, (DIAL_HZ + 25_000) as f64);
    assert_eq!(
        radio.heard(0x27, &[0x11, 0x01]),
        1,
        "the scope was switched on"
    );

    daemon.set_scope_enabled(false);
    // The disable has had its chance to go out: the dial poll after it has been answered.
    assert_eq!(
        ask_dial(&addr, Duration::from_secs(5)),
        Ok(DIAL_HZ.to_string())
    );
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        radio.heard(0x27, &[0x11, 0x00]),
        0,
        "the scope output of a shared radio was switched off"
    );
    assert!(daemon.is_alive());
}
