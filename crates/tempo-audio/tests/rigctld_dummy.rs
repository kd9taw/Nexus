//! CAT-layer integration tests against a REAL Hamlib `rigctld -m 1` (dummy rig).
//!
//! The unit tests in `rig.rs` drive mock TCP servers, which encode OUR beliefs
//! about the rigctld protocol. Every historical CAT bug in this codebase lived
//! in the gap between those beliefs and real Hamlib behavior (reply framing,
//! RPRT formats, multi-line `m` replies, mode-token tables). These tests close
//! that gap: same `Rig` code, real daemon on the other end.
//!
//! Skips (loudly, passing) when no `rigctld` is available so a contributor
//! without Hamlib stays green; CI installs `libhamlib-utils` so the suite is
//! real there. Override the binary with `NEXUS_RIGCTLD=/path/to/rigctld`
//! (inherits the environment, so `LD_LIBRARY_PATH` works for extracted debs).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tempo_audio::rig::Rig;

/// Locate a usable rigctld, or None (→ tests skip).
fn rigctld_bin() -> Option<String> {
    if let Ok(p) = std::env::var("NEXUS_RIGCTLD") {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    let ok = Command::new("rigctld")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then(|| "rigctld".to_string())
}

/// Tests each own a daemon, but their LIFECYCLES must not overlap: parallel
/// spawn/kill of several rigctld instances flaked ~1-in-6 runs (connection
/// resets under churn). Every test takes this lock first. (`.unwrap_or_else`
/// because a poisoned lock from one failed test must not cascade.)
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A live `rigctld -m 1` on an ephemeral port, killed on drop.
struct DummyRig {
    child: Child,
    addr: String,
    /// Persistent observer connection (see [`DummyRig::observe`]).
    obs: Option<TcpStream>,
}

/// Per-process unique ports. A bind-:0-then-release reservation raced under
/// parallel tests: two daemons could land on one port, so two test bodies
/// silently shared one rig and corrupted each other's mode/PTT state (seen as
/// flaky cross-talk failures). An atomic counter over a pid-salted base makes
/// each spawn's port unique within the run without any reserve/release window.
static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

fn unique_port() -> u16 {
    let base = 20_000 + (std::process::id() % 20_000) as u16;
    base + NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

impl DummyRig {
    fn spawn(bin: &str) -> DummyRig {
        DummyRig::spawn_with(bin, &["-P", "RIG"])
    }

    /// Spawn with extra rigctld args prepended to the standard dummy set.
    /// -P RIG matters: the dummy's caps default to PTT_NONE, so without it
    /// every `T` returns RPRT -1 and `t` returns -11 (measured, Hamlib 4.5.5).
    fn spawn_with(bin: &str, extra: &[&str]) -> DummyRig {
        // A port from the counter can still collide with an unrelated service
        // (measured once in 20 runs: rigctld exits 1 on a taken port). The
        // liveness check below detects exactly that, and the counter hands a
        // fresh port on every attempt — so collisions self-heal by respawn.
        'attempt: for attempt in 0..5 {
            let port = unique_port();
            let mut child = Command::new(bin)
                .args(["-m", "1", "-T", "127.0.0.1", "-t", &port.to_string()])
                .args(extra)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn rigctld");
            let addr = format!("127.0.0.1:{port}");
            // Ready = OUR child is still alive AND it answers a complete `f`
            // reply — merely accepting TCP is not ready, and a dead child
            // means the port was taken (respawn rather than talk to a stranger).
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().expect("try_wait") {
                    eprintln!("rigctld exited during startup ({status}) — port {port} taken, retrying (attempt {attempt})");
                    continue 'attempt;
                }
                if let Ok(mut s) = TcpStream::connect(&addr) {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                    if s.write_all(b"f\n").is_ok() {
                        let mut line = String::new();
                        if BufReader::new(&s).read_line(&mut line).is_ok()
                            && line.trim().parse::<u64>().is_ok()
                        {
                            return DummyRig {
                                child,
                                addr,
                                obs: None,
                            };
                        }
                    }
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("rigctld not ready on {addr} — retrying (attempt {attempt})");
                    continue 'attempt;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        panic!("rigctld failed to start after 5 attempts");
    }

    /// Independent observation channel: ask the daemon directly, bypassing the
    /// code under test, so an assertion can't be fooled by a client-side bug.
    ///
    /// Holds ONE persistent connection (like every real rigctld client does) —
    /// the first version opened a fresh socket per query, and that one-shot
    /// churn is exactly what real clients never do; rigctld answered it with
    /// occasional RSTs and empty replies. Failures drop the socket and retry
    /// on a fresh one, mirroring the production client's own recovery.
    fn observe(&mut self, cmd: &str) -> String {
        for _attempt in 0..3 {
            if self.obs.is_none() {
                if let Ok(s) = TcpStream::connect(&self.addr) {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
                    let _ = s.set_write_timeout(Some(Duration::from_millis(1000)));
                    self.obs = Some(s);
                } else {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            }
            let sock = self.obs.as_mut().unwrap();
            let ok = sock.write_all(format!("{cmd}\n").as_bytes()).is_ok();
            if ok {
                let mut line = String::new();
                if BufReader::new(&*sock).read_line(&mut line).is_ok() && !line.trim().is_empty() {
                    return line.trim().to_string();
                }
            }
            self.obs = None; // dead/odd socket — reconnect and retry
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("observer got no usable reply to {cmd:?}");
    }
}

/// Construct a `Rig` and absorb the daemon's startup window: a freshly
/// spawned rigctld can RST the first client connection (seen ~1-in-10 under
/// WSL2). The PRODUCTION radio loop tolerates exactly this — it re-issues CAT
/// commands every iteration and `command()` drops+reconnects on failure — so
/// the harness models that with a bounded first-contact retry, then hands the
/// test a rig whose next command is expected to behave strictly.
fn connect_settled(addr: &str) -> Rig {
    let mut rig = Rig::rigctld(addr);
    for attempt in 0..10 {
        match rig.read_freq() {
            Ok(_) => return rig,
            Err(e) if attempt == 9 => panic!("rig never settled on {addr}: {e}"),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    unreachable!()
}

impl Drop for DummyRig {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every test body runs behind this guard so a rigctld-less box skips loudly.
macro_rules! require_rigctld {
    () => {
        match rigctld_bin() {
            Some(bin) => bin,
            None => {
                eprintln!("SKIP: no rigctld found (install libhamlib-utils or set NEXUS_RIGCTLD)");
                return;
            }
        }
    };
}

#[test]
fn freq_set_read_roundtrips_against_real_rigctld() {
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let mut d = DummyRig::spawn(&bin);
    let mut rig = connect_settled(&d.addr);
    rig.set_freq(14_074_000).expect("set_freq");
    assert_eq!(rig.read_freq().expect("read_freq"), 14_074_000);
    // Retune (the dial-retune loop depends on repeat F commands being honored).
    rig.set_freq(7_074_000).expect("retune");
    assert_eq!(rig.read_freq().unwrap(), 7_074_000);
    assert_eq!(
        d.observe("f"),
        "7074000",
        "wire truth matches the client view"
    );
}

#[test]
fn pktusb_the_digital_mode_token_is_accepted_by_real_hamlib() {
    // Mode arming forces PKTUSB on digital entry (TX-safety: never TX on a
    // wrong sideband). That only works if the token is one Hamlib accepts —
    // a mock can't prove that; the real daemon's mode table can.
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let d = DummyRig::spawn(&bin);
    let mut rig = connect_settled(&d.addr);
    rig.set_mode("PKTUSB", 3000)
        .expect("PKTUSB must be a real Hamlib token");
    assert_eq!(rig.read_mode().as_deref(), Some("PKTUSB"));
    // And the classic voice sides used by Phone.
    for m in ["USB", "LSB", "CW"] {
        rig.set_mode(m, 0)
            .unwrap_or_else(|e| panic!("set_mode {m}: {e}"));
        assert_eq!(rig.read_mode().as_deref(), Some(m));
    }
}

#[test]
fn ptt_reaches_the_wire_and_keyed_tracks_it() {
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let mut d = DummyRig::spawn(&bin);
    let mut rig = connect_settled(&d.addr);
    assert!(
        !rig.keyed,
        "must construct unkeyed (Enable-TX latch analog)"
    );
    rig.ptt(true).expect("key");
    assert!(rig.keyed);
    assert_eq!(d.observe("t"), "1", "the daemon really sees PTT high");
    rig.ptt(false).expect("unkey");
    assert!(!rig.keyed);
    assert_eq!(d.observe("t"), "0", "the daemon really sees PTT released");
}

#[test]
fn failed_unkey_leaves_keyed_latched_for_the_retry_loop() {
    // THE stuck-TX-light invariant: an unkey that FAILS must leave `keyed`
    // true so the radio loop's idle self-heal keeps retrying — one transient
    // CAT failure must never silently latch the rig in transmit. Here the
    // failure is real: the daemon dies mid-QSO.
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let d = DummyRig::spawn(&bin);
    let mut rig = connect_settled(&d.addr);
    rig.ptt(true).expect("key while the daemon is alive");
    assert!(rig.keyed);
    drop(d); // rigctld gone — the unkey command has nowhere to land
    let err = rig.ptt(false);
    assert!(
        err.is_err(),
        "unkey against a dead daemon must surface an Err"
    );
    assert!(
        rig.keyed,
        "failed unkey must LEAVE keyed=true (drives the self-heal retry); \
         clearing it here would report an un-keyed rig that may still be transmitting"
    );
}

#[test]
fn real_rprt_rejection_framing_parses_as_err() {
    // A rig rejection must surface as Err so the radio loop's bounded retry can give up
    // instead of falsely advancing its idea of the rig's state. Mock coverage cannot prove
    // the FRAMING — that Rig actually parses a negative `RPRT n` off the wire — so this needs
    // a real daemon that really rejects something.
    //
    // Finding one is the whole difficulty, because the dummy backend accepts almost
    // everything. Measured against Hamlib 4.6.5 and 4.7.0~rc on 2026-08-13:
    //
    //   T 1 (no -P RIG)   → RPRT 0    t → 1, RPRT 0     M NOSUCHMODE 0 → RPRT 0
    //   T 1 (-P NONE)     → RPRT 0    F 99999999999999  → RPRT 0
    //   V NOSUCHVFO       → RPRT -1   G NOSUCHOP → RPRT -1   L NOSUCHLEVEL 1 → RPRT -11
    //
    // This test used to key PTT against a daemon spawned without `-P RIG`, on the belief that
    // a PTT-less dummy rejects it. It does not (and the comment's own cited measurement was of
    // `t`, not `T 1`) — so the test asserted `is_err()` on a command the daemon answers
    // `RPRT 0`, and failed on any machine with a real rigctld rather than testing the framing.
    // An unknown VFO is rejected for real, so drive the framing through that instead.
    //
    // The "a failed key-down still marks keyed" invariant that used to ride along here is not
    // lost: it is a Rig state rule, not a wire-framing one, and is covered by the unit tests in
    // `rig.rs` plus `failed_unkey_leaves_keyed_latched_for_the_retry_loop` above.
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let d = DummyRig::spawn_with(&bin, &[]);
    let mut rig = connect_settled(&d.addr);
    assert!(
        rig.set_vfo("NOSUCHVFO").is_err(),
        "a real negative RPRT (`V NOSUCHVFO` → RPRT -1) must surface as Err — if this passes as \
         Ok the loop would treat a rejected command as applied"
    );
    // The connection must remain usable afterwards (drop-and-reconnect hygiene).
    assert_eq!(rig.read_freq().expect("post-error command"), 145_000_000);
}

#[test]
fn fm_repeater_lines_are_real_hamlib_protocol_not_just_our_belief() {
    // set_fm_repeater sends R/O/C best-effort and SWALLOWS per-command errors —
    // so a wrong line format would never surface in production. Verify against
    // the real daemon that each command actually lands: shift, offset, tone.
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let mut d = DummyRig::spawn(&bin);
    let mut rig = connect_settled(&d.addr);
    rig.set_mode("FM", 0).expect("FM first (live connection)");
    rig.set_fm_repeater("-", 600_000, 103.5)
        .expect("repeater setup");
    assert_eq!(d.observe("r"), "-", "shift direction reached the rig");
    assert_eq!(d.observe("o"), "600000", "offset magnitude reached the rig");
    assert_eq!(
        d.observe("c"),
        "1035",
        "CTCSS reached the rig (tenths of Hz)"
    );
}

#[test]
fn multiline_mode_reply_cannot_desync_the_next_command() {
    // `m` answers TWO lines (mode + passband). The stale-byte drain exists so
    // a lingering second line is never read as the NEXT command's reply —
    // the desync failure mode where a keyed rig gets judged by the previous
    // answer. Exercise it against real two-line framing, repeatedly.
    let bin = require_rigctld!();
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner());
    let d = DummyRig::spawn(&bin);
    let mut rig = connect_settled(&d.addr);
    rig.set_freq(14_074_000).unwrap();
    for _ in 0..5 {
        let (mode, _pb) = rig.read_mode_passband();
        assert!(mode.is_some(), "mode read must succeed");
        // If the passband line lingered, this freq read would parse garbage.
        assert_eq!(
            rig.read_freq().expect("freq after multi-line reply"),
            14_074_000,
            "a stale passband byte desynced the following command"
        );
    }
}
