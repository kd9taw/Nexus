//! Antenna rotator control via Hamlib's `rotctld` daemon over TCP — the same
//! daemon-over-TCP pattern as the `rigctld` CAT path, so Nexus needs no C dependency.
//! The operator runs `rotctld -m <model> -r <port> -t <tcp>` (or points a rig with a
//! built-in rotor); Nexus connects and sends `P <az> <el>` to turn the antenna and `p`
//! to read where it is.
//!
//! ⭐ **A COMMAND THAT GOT NO ANSWER IS A FAILURE.** This module used to say the opposite,
//! and it is the worst defect the 2026-08-18 rotor review found. `point`/`point_azel` read
//! one 64-byte gulp with `.unwrap_or(0)` and then accepted an EMPTY reply as an ack — so a
//! read timeout, a reset and a peer that closed all collapsed into `Ok(())`. rotctld answers
//! every set command with an `RPRT <n>` line in non-extended mode, so an empty reply could
//! only ever mean failure; it was being read as success. The blast radius was the whole
//! pointing story: the manual slew and the ↗ point-at-call returned success with the antenna
//! motionless, and in a satellite pass `send_rot_step` turned the phantom ack into
//! `RotOutcome::AzElOk`, which reset the miss counter — so `MISS_LIMIT`, `gave_up()` and the
//! "the rotator stopped answering, point it yourself" alert were UNREACHABLE for a daemon that
//! went silent. For a mast the operator cannot see from the shack, a pointing command that
//! silently did nothing is the worst possible failure mode.
//!
//! Everything here now requires a positive `RPRT 0`, reads until a complete line against an
//! overall deadline (a split reply is not a foreign one), and reports what the daemon said.
//! The shape is deliberately the CAT client's — see `rig.rs`'s `command`, whose comment
//! ("An incomplete reply is an ERROR … so callers never treat a partial or timed-out answer as
//! success") was the rule this module was missing.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Per-read window. Bounds one wait, never the whole reply — the deadlines below do that.
const READ_WINDOW: Duration = Duration::from_millis(500);

/// How long to wait for a rotctld that has accepted the connection and gone quiet, for a
/// command that MOVES something.
///
/// ⚠️ **Reconciled with Hamlib's own budget, measured rather than guessed.** rotctld does not
/// answer a `P` until its backend has finished timing out and retrying, and that budget is
/// per-model: `rotctl -m <n> -L` on the bundled 4.7.1 gives `timeout × (retry + 1) +
/// post_write_delay` = 800 ms for EasyComm II/III and GS-232 generic, 1 650 ms for the
/// GS-232A/B family, 1 900 ms for all three SPIDs, 4 000 ms for the M2 RC2800, 5 000 ms for the
/// rotorez family (Rotor-EZ, DCU, ERC, RT-21, YRC-1) and 12 000 ms for the Prosistels. The old
/// value was 800 ms — shorter than Hamlib's own budget for every curated model but two, so a
/// slow-but-working rotator read as done as well.
///
/// 3 500 ms covers ONE FULL ATTEMPT of every curated backend (the longest single `timeout` is
/// the Prosistel's 3 000 ms) and the entire retry budget of the GS-232, SPID and EasyComm
/// families. It deliberately does NOT cover the rotorez family's or the Prosistel's last
/// retries: a 12 s block would stall the satellite track's 3 s tick — and hence its Doppler —
/// for a rotator that is almost certainly dead. What absorbs the difference is
/// `TrackDriver::MISS_LIMIT`: five consecutive silences before the mast is given up, and one
/// success forgives them all, so an intermittent controller that answers on Hamlib's third
/// retry costs a miss, not the pass.
const CMD_DEADLINE_MS: u64 = 3_500;

/// The same for a position READ. Shorter on purpose: a lost poll costs one "—" on screen and
/// is retried two seconds later, while a long block piles background reads up behind each other
/// (the strip and the pane both poll at 2 s).
const POLL_DEADLINE_MS: u64 = 1_500;

/// Cap on the TCP connect itself. Without one, an external rotctld on an unreachable host
/// stalls for the OS SYN timeout (~75 s on Linux, ~21 s on Windows) inside a 2 s poll.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// rotctld `P` — point to `az_deg` (normalized to [0,360)), elevation 0.
pub fn point_line(az_deg: f64) -> String {
    format!("P {} 0\n", wire_az(az_deg))
}

/// rotctld `P` — point to `az_deg` (normalized to [0,360)) and `el_deg` (clamped to the
/// [0,180] a flip-capable mount can reach; the mount's real ceiling is Hamlib's to enforce,
/// and it now says so out loud instead of being clamped away here). The elevation-capable
/// form used to track a satellite pass.
pub fn point_line_azel(az_deg: f64, el_deg: f64) -> String {
    format!("P {} {:.1}\n", wire_az(az_deg), el_deg.clamp(0.0, 180.0))
}

/// One azimuth, as the wire carries it: rounded to the tenth of a degree the `P` command is
/// written in, and THEN normalized into [0,360).
///
/// ⚠️ **The order is the fix.** Normalising first and formatting after put `360.0` on the wire
/// for anything from 359.95° up, because `{:.1}` rounds. A Green Heron RT-21 declares
/// `max_az 359.9` (bundled Hamlib caps, model 405), so rotctld refuses those bearings outright
/// — the top 0.15° of the compass was unreachable on that controller, and before this module
/// told the truth about replies, the refusal was reported as success. Rounding first makes
/// 359.96° land on `0.0`, which is the same bearing and is inside every backend's range.
fn wire_az(az_deg: f64) -> String {
    format!("{:.1}", ((az_deg * 10.0).round() / 10.0).rem_euclid(360.0))
}

fn connect(addr: &str) -> std::io::Result<TcpStream> {
    // Resolve first so a bare host name is a NAMED error rather than a bad address: the
    // external-rotctld field takes `host:port` and `192.168.1.50` alone is not one.
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("rotctld address {addr:?} is not host:port ({e})"),
            )
        })?
        .next()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("rotctld address {addr:?} resolved to nothing"),
            )
        })?;
    let s = TcpStream::connect_timeout(&sock, CONNECT_TIMEOUT)?;
    s.set_read_timeout(Some(READ_WINDOW))?;
    s.set_write_timeout(Some(READ_WINDOW))?;
    Ok(s)
}

/// Send `line` and read the daemon's answer to it, reading until the reply is COMPLETE rather
/// than taking one gulp: rotctld can split a line across reads, and half of `RPRT 0` is not an
/// error reply, it is an unfinished one.
///
/// `want_lines` is how many lines a good answer has (1 for a set command's `RPRT`, 2 for a
/// position's `az`/`el`); an `RPRT` line always ends the read, because an error reply is one
/// line whatever was asked. Nothing at all by the deadline is an ERROR — never an empty string
/// a caller might read as an ack.
fn ask(addr: &str, line: &str, want_lines: usize, deadline_ms: u64) -> std::io::Result<String> {
    let mut s = connect(addr)?;
    s.write_all(line.as_bytes())?;
    let deadline = Instant::now() + Duration::from_millis(deadline_ms);
    let mut out = Vec::with_capacity(64);
    let mut buf = [0u8; 256];
    loop {
        match s.read(&mut buf) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "rotctld closed the connection without answering {:?}",
                        line.trim()
                    ),
                ));
            }
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&out);
                let complete = text.lines().filter(|l| !l.trim().is_empty()).count();
                if text.ends_with('\n')
                    && (complete >= want_lines
                        || text.lines().any(|l| l.trim_start().starts_with("RPRT")))
                {
                    return Ok(text.to_string());
                }
            }
            // A per-read window expiring is not the answer; the deadline below is.
            Err(ref e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "the rotator did not answer {:?} within {deadline_ms} ms (got {:?}) — \
                     check that the controller is powered on, on the right port, and at the \
                     baud rate its model needs",
                    line.trim(),
                    String::from_utf8_lossy(&out)
                ),
            ));
        }
    }
}

/// Send a command that MOVES something and insist on `RPRT 0`.
///
/// Anything else is an error carrying what the daemon actually said, because that string is the
/// diagnosis: `RPRT -1` is an out-of-range bearing (a Green Heron refuses past 359.9°),
/// `RPRT -11` a function the backend does not implement, `RPRT -6` an I/O error on the serial
/// link to the controller.
fn command(addr: &str, line: &str) -> std::io::Result<()> {
    let reply = ask(addr, line, 1, CMD_DEADLINE_MS)?;
    let rprt = reply
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("RPRT"))
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "rotctld answered {:?} with {:?}, which is not an RPRT reply",
                line.trim(),
                reply.trim()
            ))
        })?;
    if rprt == "RPRT 0" {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "the rotator refused {:?}: {rprt}",
        line.trim()
    )))
}

/// Point the rotator at `az_deg` via rotctld at `addr` (host:port). `Ok` only on `RPRT 0`.
pub fn point(addr: &str, az_deg: f64) -> std::io::Result<()> {
    command(addr, &point_line(az_deg))
}

/// Point the rotator at `az_deg`/`el_deg` via rotctld at `addr` (host:port). `Ok` only on
/// `RPRT 0`. The az/el twin of [`point`] for satellite tracking on an elevation-capable rotor.
pub fn point_azel(addr: &str, az_deg: f64, el_deg: f64) -> std::io::Result<()> {
    command(addr, &point_line_azel(az_deg, el_deg))
}

/// Stop rotation immediately (rotctld `S`). Checked like any other command: a backend that
/// refuses `S` used to report a successful STOP, which is the one answer a stop must never
/// give — the operator walks away believing the mast is halted.
pub fn stop(addr: &str) -> std::io::Result<()> {
    command(addr, "S\n")
}

/// Read the rotator's position: `(azimuth, elevation)` in degrees, elevation `None` when the
/// backend answers with an azimuth alone.
///
/// An error here means the rotator did not report a position — which is NOT the same as a
/// rotator that cannot be pointed. Model 403 (Hy-Gain DCU-1/DCU-1X) has no `get_position` at
/// all in the bundled Hamlib and answers `p` with `RPRT -11` while taking `P` perfectly, so a
/// caller must not turn this failure into "no rotator".
pub fn read_position(addr: &str) -> std::io::Result<(f64, Option<f64>)> {
    let reply = ask(addr, "p\n", 2, POLL_DEADLINE_MS)?;
    let mut nums = reply.lines().map(str::trim).filter(|l| !l.is_empty());
    let az = nums
        .next()
        .and_then(|l| l.parse::<f64>().ok())
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "the rotator does not report its position: {:?}",
                reply.trim()
            ))
        })?;
    Ok((az, nums.next().and_then(|l| l.parse::<f64>().ok())))
}

/// Current azimuth, or `None` on any failure — the polling surfaces show "—" rather than
/// erroring twice a second.
pub fn read_azimuth(addr: &str) -> Option<f64> {
    read_position(addr).ok().map(|(az, _)| az)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// What a staged rotctld does with the one command it is given.
    enum Behave {
        /// Answer with this text (bytes exactly as written).
        Say(&'static str),
        /// Answer in two writes with a pause between — a reply split across reads.
        Split(&'static str, &'static str),
        /// Accept the connection and never say anything. THE case that shipped as success.
        Silent,
        /// Accept the connection and close it without a word.
        Hangup,
    }

    /// A rotctld on loopback that plays one script, and hands back the line it was sent.
    ///
    /// The suite had none of this: `rotator.rs`'s tests were two string-formatting assertions,
    /// and the integration suite's own header claimed "mock servers that encode OUR beliefs
    /// about the rotctld protocol" while staging only a real daemon that always answers. The
    /// silent daemon — the shipped bug — was never staged anywhere.
    fn stage(b: Behave) -> (String, mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let addr = l.local_addr().expect("its address").to_string();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = l.accept() else { return };
            let mut line = String::new();
            let mut reader = std::io::BufReader::new(s.try_clone().expect("clone"));
            let _ = reader.read_line(&mut line);
            let _ = tx.send(line);
            match b {
                Behave::Say(text) => {
                    let _ = s.write_all(text.as_bytes());
                }
                Behave::Split(a, z) => {
                    let _ = s.write_all(a.as_bytes());
                    let _ = s.flush();
                    std::thread::sleep(Duration::from_millis(120));
                    let _ = s.write_all(z.as_bytes());
                }
                // Hold the socket open, saying nothing, until the client gives up.
                Behave::Silent => std::thread::sleep(Duration::from_secs(6)),
                Behave::Hangup => {}
            }
            let _ = s.flush();
            std::thread::sleep(Duration::from_millis(50));
        });
        (addr, rx)
    }

    #[test]
    fn point_line_formats_and_normalizes_azimuth() {
        assert_eq!(point_line(90.0), "P 90.0 0\n");
        assert_eq!(point_line(0.0), "P 0.0 0\n");
        assert_eq!(point_line(359.4), "P 359.4 0\n");
        // wrap negatives and ≥360 into [0,360)
        assert_eq!(point_line(-90.0), "P 270.0 0\n");
        assert_eq!(point_line(450.0), "P 90.0 0\n");
    }

    #[test]
    fn the_top_tenth_of_a_degree_never_reaches_the_wire_as_360() {
        // A Green Heron RT-21 declares max_az 359.9 (bundled Hamlib caps, model 405), so
        // `P 360.0` is refused outright — the compass's last 0.15° was unusable on it.
        // Rounding BEFORE the wrap puts those bearings on 0.0, the same bearing, in range.
        for az in [359.95, 359.96, 359.99, 359.999, 360.0] {
            assert_eq!(point_line(az), "P 0.0 0\n", "az {az} must not become 360.0");
            assert_eq!(point_line_azel(az, 10.0), "P 0.0 10.0\n");
        }
        // …and the bearing just below it is still itself.
        assert_eq!(point_line(359.94), "P 359.9 0\n");
        assert_eq!(point_line(359.9), "P 359.9 0\n");
    }

    #[test]
    fn point_line_azel_formats_normalizes_and_clamps() {
        assert_eq!(point_line_azel(90.0, 45.0), "P 90.0 45.0\n");
        assert_eq!(point_line_azel(0.0, 0.0), "P 0.0 0.0\n");
        // azimuth wraps into [0,360)
        assert_eq!(point_line_azel(-90.0, 30.0), "P 270.0 30.0\n");
        assert_eq!(point_line_azel(450.0, 10.0), "P 90.0 10.0\n");
        // elevation clamps into [0,180] — the flipped frame's range, not 90. A mount that
        // cannot reach it refuses the command, and that refusal is now visible.
        assert_eq!(point_line_azel(180.0, -5.0), "P 180.0 0.0\n");
        assert_eq!(point_line_azel(180.0, 120.0), "P 180.0 120.0\n");
        assert_eq!(point_line_azel(180.0, 200.0), "P 180.0 180.0\n");
    }

    #[test]
    fn an_accepted_command_is_a_success_and_sends_what_it_promised() {
        let (addr, rx) = stage(Behave::Say("RPRT 0\n"));
        assert!(point_azel(&addr, 123.4, 45.0).is_ok());
        assert_eq!(rx.recv().expect("the daemon saw a line"), "P 123.4 45.0\n");
    }

    #[test]
    fn a_refused_command_is_an_error_that_carries_the_daemons_own_code() {
        // RPRT -1 is what a Green Heron gives for a bearing past its 359.9° ceiling, and
        // RPRT -11 what a backend gives for a function it does not implement. Both used to
        // reach the operator as "rotctld error: …"; what matters is that neither is Ok.
        for (code, needle) in [("RPRT -1\n", "RPRT -1"), ("RPRT -11\n", "RPRT -11")] {
            let (addr, _rx) = stage(Behave::Say(code));
            let e = point(&addr, 180.0).expect_err("a refusal is not a success");
            assert!(e.to_string().contains(needle), "{e}");
        }
    }

    #[test]
    fn a_daemon_that_says_nothing_is_a_failure_not_an_ack() {
        // ⭐ THE SHIPPED BUG, staged. This returned Ok(()) — a slew that never happened,
        // reported as done, which in a pass also reset the miss counter so the rotator could
        // never be given up on. The short deadline keeps the test quick; the mechanism (a
        // timed-out read is an Err, not an empty ack) is the same one CMD_DEADLINE_MS uses.
        let (addr, _rx) = stage(Behave::Silent);
        let e = ask(&addr, "P 10.0 0\n", 1, 300).expect_err("silence is not an ack");
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            e.to_string().contains("baud"),
            "the error names the likeliest cause: {e}"
        );
    }

    #[test]
    fn a_daemon_that_hangs_up_without_answering_is_a_failure_too() {
        let (addr, _rx) = stage(Behave::Hangup);
        let e = point(&addr, 10.0).expect_err("a closed socket is not an ack");
        assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn garbage_is_not_an_ack_either() {
        let (addr, _rx) = stage(Behave::Say("hello there\n"));
        let e = point(&addr, 10.0).expect_err("a non-RPRT reply is not a success");
        assert!(e.to_string().contains("not an RPRT reply"), "{e}");
    }

    #[test]
    fn a_reply_split_across_reads_is_still_one_reply() {
        // Half of "RPRT 0" is an unfinished answer, not a foreign one. Reading one gulp made
        // this a spurious failure — the mirror image of the bug above.
        let (addr, _rx) = stage(Behave::Split("RPR", "T 0\n"));
        assert!(point(&addr, 10.0).is_ok());
    }

    #[test]
    fn a_refused_connection_is_an_error() {
        // Nothing listening at all — the one failure the old code could report.
        let l = TcpListener::bind("127.0.0.1:0").expect("a port");
        let addr = l.local_addr().expect("its address").to_string();
        drop(l);
        assert!(point(&addr, 10.0).is_err());
    }

    #[test]
    fn an_address_with_no_port_is_named_rather_than_swallowed() {
        let e = point("192.168.1.50", 10.0).expect_err("a bare host is not an address");
        assert!(e.to_string().contains("host:port"), "{e}");
    }

    #[test]
    fn stop_reports_a_backend_that_refuses_it() {
        let (addr, rx) = stage(Behave::Say("RPRT 0\n"));
        assert!(stop(&addr).is_ok());
        assert_eq!(rx.recv().expect("a line"), "S\n");

        let (addr, _rx) = stage(Behave::Say("RPRT -11\n"));
        assert!(
            stop(&addr).is_err(),
            "a stop that did not stop must not report success"
        );
    }

    #[test]
    fn a_position_reply_gives_azimuth_and_elevation() {
        let (addr, rx) = stage(Behave::Say("123.4\n45.6\n"));
        assert_eq!(
            read_position(&addr).expect("a position"),
            (123.4, Some(45.6))
        );
        assert_eq!(rx.recv().expect("a line"), "p\n");
    }

    #[test]
    fn a_rotator_that_cannot_report_is_distinguishable_from_one_that_is_not_there() {
        // Model 403 (Hy-Gain DCU-1/DCU-1X) has no get_position in the bundled Hamlib: it
        // answers `p` with RPRT -11 and takes `P` perfectly. The reply reaches the caller as
        // an error that says so, instead of being indistinguishable from a dead daemon.
        let (addr, _rx) = stage(Behave::Say("RPRT -11\n"));
        let e = read_position(&addr).expect_err("RPRT is not a position");
        assert!(
            e.to_string().contains("does not report its position"),
            "{e}"
        );
        // …and the convenience wrapper still degrades to None for the 2 s polls.
        let (addr, _rx) = stage(Behave::Say("RPRT -11\n"));
        assert_eq!(read_azimuth(&addr), None);
    }
}
