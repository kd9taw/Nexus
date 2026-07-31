//! A whole satellite pass, driven against a REAL Hamlib `rotctld -m 1`.
//!
//! The unit tests either side of this cover halves that cannot see each other:
//! `tempo_core::rotator` knows the tracking policy but has never touched a
//! socket, and `tempo_audio::rotator`'s tests drive mock servers that encode
//! OUR beliefs about the rotctld protocol. The bugs live in between — in
//! whether the sequence of commands a real pass produces is one a real daemon
//! accepts, and in whether what comes back is interpreted correctly.
//!
//! So these run the actual pass: real orbital elements, a real observer, real
//! look angles from the propagator, through the same `TrackDriver` the
//! production loop uses, onto a real daemon. The only thing simulated is the
//! clock — the loop sleeps 3 s between ticks, and a 10-minute pass is not a
//! unit test.
//!
//! Skips (loudly, passing) when no `rotctld` is available so a contributor
//! without Hamlib stays green; CI installs `libhamlib-utils`, which ships both
//! `rigctld` and `rotctld`, so the suite is real there. Override the binary
//! with `NEXUS_ROTCTLD=/path/to/rotctld`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tempo_core::rotator::{RotOutcome, RotStep, RotatorConfig, TrackDriver};

/// Locate a usable rotctld, or None (→ tests skip).
fn rotctld_bin() -> Option<String> {
    if let Ok(p) = std::env::var("NEXUS_ROTCTLD") {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    Command::new("rotctld")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        .then(|| "rotctld".to_string())
}

/// Same discipline as the rigctld suite: parallel spawn/kill of several daemons
/// flakes under connection churn, so lifecycles never overlap.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
static NEXT_PORT: AtomicU16 = AtomicU16::new(0);

/// Per-process unique ports, offset from the rigctld suite's range so the two
/// integration suites cannot collide when the whole crate runs at once.
fn unique_port() -> u16 {
    let base = 41_000 + (std::process::id() % 20_000) as u16;
    base + NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

/// A live `rotctld -m 1` (dummy rotator) on an ephemeral port, killed on drop.
struct DummyRot {
    child: Child,
    addr: String,
}

impl DummyRot {
    fn spawn(bin: &str) -> DummyRot {
        'attempt: for attempt in 0..5 {
            let port = unique_port();
            let mut child = Command::new(bin)
                .args(["-m", "1", "-T", "127.0.0.1", "-t", &port.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn rotctld");
            let addr = format!("127.0.0.1:{port}");
            // Ready = our child alive AND answering a complete `p` (get
            // position) reply. Accepting TCP is not ready, and a dead child
            // means the port was taken — respawn rather than talk to a stranger.
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.try_wait().expect("try_wait") {
                    eprintln!("rotctld exited during startup ({status}) — port {port} taken, retrying (attempt {attempt})");
                    continue 'attempt;
                }
                if let Ok(mut s) = TcpStream::connect(&addr) {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                    if s.write_all(b"p\n").is_ok() {
                        let mut line = String::new();
                        if BufReader::new(&s).read_line(&mut line).is_ok()
                            && line.trim().parse::<f64>().is_ok()
                        {
                            return DummyRot { child, addr };
                        }
                    }
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("rotctld not ready on {addr} — retrying (attempt {attempt})");
                    continue 'attempt;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        panic!("rotctld failed to start after 5 attempts");
    }

    /// Ask the daemon where it thinks it is pointing, bypassing the code under
    /// test — so an assertion cannot be satisfied by a client-side bug.
    fn position(&self) -> (f64, f64) {
        for _ in 0..3 {
            if let Ok(mut s) = TcpStream::connect(&self.addr) {
                let _ = s.set_read_timeout(Some(Duration::from_millis(1000)));
                let _ = s.set_write_timeout(Some(Duration::from_millis(1000)));
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
        panic!("no usable position reply from the dummy rotator");
    }
}

impl Drop for DummyRot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

macro_rules! require_rotctld {
    () => {
        match rotctld_bin() {
            Some(bin) => bin,
            None => {
                eprintln!("SKIP: no rotctld found (install libhamlib-utils or set NEXUS_ROTCTLD)");
                return;
            }
        }
    };
}

/// The same element set the propagation tests use, and an observer in the
/// operator's own grid.
const ISS_L1: &str = "1 25544U 98067A   08264.51782528 -.00002182  00000-0 -11606-4 0  2927";
const ISS_L2: &str = "2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537";
const OBSERVER: (f64, f64) = (41.9, -88.0);

fn iss() -> propagation::sat::Tle {
    propagation::sat::Tle {
        name: "ISS (ZARYA)".to_string(),
        line1: ISS_L1.to_string(),
        line2: ISS_L2.to_string(),
    }
}

/// Find a real pass with enough elevation to be worth tracking.
fn a_real_pass() -> propagation::sat::Pass {
    // Search forward from the element-set epoch so the test is deterministic
    // and never depends on today's date.
    let epoch = 1_221_913_539;
    propagation::sat::passes(&iss(), OBSERVER, epoch, 24)
        .into_iter()
        .find(|p| p.max_el_deg >= 25.0)
        .expect("the ISS makes a workable pass over EN52 within a day of this epoch")
}

/// Drive a whole pass at the production 3 s cadence — in simulated time.
/// Returns every step the driver produced, paired with the outcome the real
/// daemon gave back.
fn fly_pass(
    addr: &str,
    driver: &mut TrackDriver,
    pass: &propagation::sat::Pass,
) -> Vec<(RotStep, RotOutcome)> {
    let mut log = Vec::new();
    let mut t = pass.aos_unix;
    while t <= pass.los_unix {
        if let Some((az, el)) = propagation::sat::look_at(&iss(), OBSERVER, t) {
            let step = driver.step(az, el.max(0.0));
            let outcome = if step == RotStep::Hold {
                RotOutcome::AzOk
            } else {
                send(addr, step)
            };
            if step != RotStep::Hold {
                driver.record(step, outcome, az, el.max(0.0));
            }
            log.push((step, outcome));
        }
        t += 3;
    }
    log
}

/// The production wire helper, mirrored: this is the one piece `src-tauri`
/// owns that this crate cannot import.
fn send(addr: &str, step: RotStep) -> RotOutcome {
    match step {
        RotStep::Hold => RotOutcome::AzOk,
        RotStep::PointAzEl { az, el } => match tempo_audio::rotator::point_azel(addr, az, el) {
            Ok(()) => RotOutcome::AzElOk,
            Err(_) => {
                if tempo_audio::rotator::point(addr, az).is_ok() {
                    RotOutcome::AzOnly
                } else {
                    RotOutcome::Failed
                }
            }
        },
        RotStep::PointAz { az } => {
            if tempo_audio::rotator::point(addr, az).is_ok() {
                RotOutcome::AzOk
            } else {
                RotOutcome::Failed
            }
        }
    }
}

#[test]
fn a_whole_pass_is_accepted_by_a_real_rotctld() {
    let bin = require_rotctld!();
    let _lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = DummyRot::spawn(&bin);

    let pass = a_real_pass();
    let mut driver = TrackDriver::new(RotatorConfig::default());
    let log = fly_pass(&rot.addr, &mut driver, &pass);

    assert!(
        log.len() > 60,
        "a pass at 3 s ticks should be dozens of steps, got {}",
        log.len()
    );
    // EVERY command a real pass produced was accepted by a real daemon. This is
    // the assertion the mock-server tests cannot make: it is the wire format,
    // the value ranges and the reply framing all at once.
    let failed = log.iter().filter(|(_, o)| *o == RotOutcome::Failed).count();
    assert_eq!(failed, 0, "{failed} of {} commands were refused", log.len());
    assert!(
        !driver.gave_up(),
        "the driver should never give up on a rotator that answered everything"
    );

    // Independent confirmation that the commands landed on a real target
    // rather than being accepted into a void: the daemon is actively slewing.
    //
    // Deliberately NOT an equality check against the last command. The whole
    // pass is issued in a few milliseconds of wall time while the daemon moves
    // at ~7.8°/s, so it is still near its home position when the last command
    // lands — and a real rotator is slower still. See the transit test below.
    let before = rot.position();
    std::thread::sleep(Duration::from_millis(400));
    let after = rot.position();
    assert!(
        after != before,
        "the daemon is not moving ({before:?} → {after:?}), so our commands set \
         no target — they were accepted and discarded"
    );
}

#[test]
fn a_real_rotator_is_in_transit_so_read_back_is_not_where_we_pointed() {
    // Measured against Hamlib 4.5.5's dummy: it slews at ~7.8°/s and reports
    // where the boom IS, not where it was told to go. A real rotator is worse.
    //
    // This is why the tracking badge reports the COMMANDED position and says so
    // — a display that read the rotator back would spend most of a pass showing
    // a position lagging the satellite by tens of degrees, which looks exactly
    // like a tracking fault. It is also why the pass test above does not assert
    // read-back equality: that assertion would only pass on a rotator that
    // teleports, and no such rotator exists.
    let bin = require_rotctld!();
    let _lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = DummyRot::spawn(&bin);

    let (target_az, target_el) = (20.0, 20.0);
    tempo_audio::rotator::point_azel(&rot.addr, target_az, target_el)
        .expect("the daemon accepts a normal az/el command");

    // Immediately after the command the antenna is nowhere near the target.
    let (az, el) = rot.position();
    assert!(
        (az - target_az).abs() > 5.0 || (el - target_el).abs() > 5.0,
        "expected the rotator to still be in transit, but it read back at \
         ({az}, {el}) for a target of ({target_az}, {target_el}) — if a rotator \
         really did teleport, the comment above needs revisiting"
    );

    // Given time, it arrives — so the command was a real target, not ignored.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (az, el) = rot.position();
        if (az - target_az).abs() < 1.0 && (el - target_el).abs() < 1.0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "rotator never reached ({target_az}, {target_el}); stuck at ({az}, {el})"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn the_deadband_actually_saves_commands_over_a_real_pass() {
    // The deadband exists because a rotator commanded every 3 s for ten minutes
    // chatters its relays continuously. That it *works* is a unit test; that it
    // makes a real difference on a real pass is this one, and the number is
    // worth knowing rather than assuming.
    let bin = require_rotctld!();
    let _lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = DummyRot::spawn(&bin);
    let pass = a_real_pass();

    let mut driver = TrackDriver::new(RotatorConfig::default());
    let log = fly_pass(&rot.addr, &mut driver, &pass);
    let held = log.iter().filter(|(s, _)| *s == RotStep::Hold).count();
    let sent = log.len() - held;

    eprintln!(
        "pass of {} ticks: {sent} commands sent, {held} suppressed by the 2° deadband",
        log.len()
    );
    assert!(sent > 0, "a tracked pass must actually move the rotator");
    assert!(
        held > 0,
        "a 2° deadband over a real pass should suppress SOMETHING — if it never \
         does, the rotator is being commanded on every tick and the deadband is \
         not doing its job"
    );
}

#[test]
fn an_az_only_rotator_still_tracks_the_whole_pass() {
    // Hamlib's dummy has an elevation axis, so a real daemon cannot produce the
    // az-only refusal. Pointing at a CLOSED port makes `point_azel` fail the
    // same way an az-only rotator's refusal does — which is the honest limit of
    // this test: it proves our fallback and recovery logic behaves, not that
    // Hamlib rejects elevation in the manner we assume.
    let bin = require_rotctld!();
    let _lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let rot = DummyRot::spawn(&bin);
    let pass = a_real_pass();

    let mut driver = TrackDriver::new(RotatorConfig::default());
    // Teach the driver this rotator refuses elevation, then fly the real pass.
    let first = driver.step(0.0, 0.0);
    driver.record(first, RotOutcome::AzOnly, 0.0, 0.0);
    assert!(!driver.azel_ok());

    let log = fly_pass(&rot.addr, &mut driver, &pass);
    let az_only = log
        .iter()
        .filter(|(s, _)| matches!(s, RotStep::PointAz { .. }))
        .count();
    let probes = log
        .iter()
        .filter(|(s, _)| matches!(s, RotStep::PointAzEl { .. }))
        .count();

    assert!(az_only > 0, "an az-only rotator must still be pointed");
    assert_eq!(
        log.iter().filter(|(_, o)| *o == RotOutcome::Failed).count(),
        0,
        "azimuth-only commands are still valid rotctld and must all be accepted"
    );
    // The recovery probe re-offers elevation periodically. Against this daemon
    // the probe SUCCEEDS (it has an elevation axis), so the driver recovers —
    // which is the desired behaviour when the original refusal was transient.
    assert!(
        probes > 0,
        "the recovery probe must re-offer elevation rather than tracking flat forever"
    );
    assert!(
        driver.azel_ok(),
        "a probe that succeeded should have restored az/el tracking"
    );
}

#[test]
fn a_dead_rotator_is_given_up_on_rather_than_hammered_forever() {
    // No daemon at all: every command fails. The driver must stop after a
    // bounded number of misses rather than spending the whole pass timing out.
    let _lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // A port nothing is listening on. Connection is refused immediately, so
    // this does not spend the client's 800 ms timeout per attempt.
    let addr = format!("127.0.0.1:{}", unique_port());
    let pass = a_real_pass();
    let mut driver = TrackDriver::new(RotatorConfig::default());

    let mut ticks = 0;
    let mut t = pass.aos_unix;
    while t <= pass.los_unix && !driver.gave_up() {
        if let Some((az, el)) = propagation::sat::look_at(&iss(), OBSERVER, t) {
            let step = driver.step(az, el.max(0.0));
            if step != RotStep::Hold {
                let outcome = send(&addr, step);
                assert_eq!(outcome, RotOutcome::Failed, "nothing is listening");
                driver.record(step, outcome, az, el.max(0.0));
                ticks += 1;
            }
        }
        t += 3;
    }
    assert!(driver.gave_up(), "a dead rotator must be given up on");
    assert_eq!(
        ticks,
        tempo_core::rotator::MISS_LIMIT as usize,
        "gave up after the wrong number of attempts"
    );
    assert_eq!(
        driver.last_aim(),
        None,
        "nothing ever reached the rotator, so there is no position to report"
    );
}
