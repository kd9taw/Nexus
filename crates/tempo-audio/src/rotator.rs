//! Antenna rotator control via Hamlib's `rotctld` daemon over TCP — the same
//! daemon-over-TCP pattern as the `rigctld` CAT path, so Nexus needs no C dependency.
//! The operator runs `rotctld -m <model> -r <port> -t <tcp>` (or points a rig with a
//! built-in rotor); Nexus connects and sends `P <az> <el>` to turn the antenna and `p`
//! to read where it is, and asks `\dump_state` what the rotator's backend accepts — whether it
//! has an elevation axis at all, and the elevation range it reaches ([`Limits`]).
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
///
/// ⚠️ **Elevation 0 is a COMMAND, not a blank.** On an az/el rotator this line lays the antenna
/// on the horizon. It is right for a rotator with no elevation axis, and for the satellite
/// track's fallback after a mount has REFUSED elevation; a manual move goes through
/// [`point_keeping`], which sends it only when there is no elevation to keep: no elevation axis
/// (or a daemon that declares nothing), or a rotator that cannot report where it is.
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
    exchange(addr, line, deadline_ms, |text| {
        let complete = text.lines().filter(|l| !l.trim().is_empty()).count();
        complete >= want_lines || text.lines().any(|l| l.trim_start().starts_with("RPRT"))
    })
}

/// [`ask`] with its own rule for when a reply is whole: `done` is asked of everything read so
/// far each time it ends with a newline. `\dump_state` needs this — its length depends on the
/// Hamlib that answers ([`dump_state_done`]).
fn exchange(
    addr: &str,
    line: &str,
    deadline_ms: u64,
    done: impl Fn(&str) -> bool,
) -> std::io::Result<String> {
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
                if text.ends_with('\n') && done(&text) {
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
///
/// That answer — `RPRT -11`, or `RPRT -4`, Hamlib's "not available" and "not implemented" — is
/// [`std::io::ErrorKind::Unsupported`], so the Rotor pane can tell a rotator that has no
/// position to give from one that did not answer at all (a controller that is off, a rotctld
/// that is not running).
pub fn read_position(addr: &str) -> std::io::Result<(f64, Option<f64>)> {
    let reply = ask(addr, "p\n", 2, POLL_DEADLINE_MS)?;
    let mut nums = reply.lines().map(str::trim).filter(|l| !l.is_empty());
    let az = nums
        .next()
        .and_then(|l| l.parse::<f64>().ok())
        .ok_or_else(|| {
            let kind = if matches!(reply.trim(), "RPRT -11" | "RPRT -4") {
                std::io::ErrorKind::Unsupported
            } else {
                std::io::ErrorKind::Other
            };
            std::io::Error::new(
                kind,
                format!(
                    "the rotator does not report its position: {:?}",
                    reply.trim()
                ),
            )
        })?;
    Ok((az, nums.next().and_then(|l| l.parse::<f64>().ok())))
}

/// Current azimuth, or `None` on any failure — the polling surfaces show "—" rather than
/// erroring twice a second.
pub fn read_azimuth(addr: &str) -> Option<f64> {
    read_position(addr).ok().map(|(az, _)| az)
}

/// What a rotctld says its backend accepts: Hamlib's own `\dump_state`, asked of the running
/// daemon. It is therefore the Hamlib the station actually runs — the bundled 4.7.1, a system
/// Hamlib, or an external rotctld on another machine — rather than a table kept in Nexus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The Hamlib rotator model the daemon runs.
    pub model: u32,
    pub min_az: f64,
    pub max_az: f64,
    pub min_el: f64,
    pub max_el: f64,
    /// Hamlib's `rot_type`, where the daemon states it (protocol 1: rotctld 4.5.5 and the bundled
    /// 4.7.1); `None` from a protocol-0 daemon (4.3.1), whose `\dump_state` has no such line.
    pub kind: Option<RotKind>,
}

/// Hamlib's `rot_type`, as `\dump_state` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotKind {
    Az,
    El,
    AzEl,
    Other,
}

/// Hamlib model 405, the Green Heron RT-21 ([`Limits::elevation`]).
const RT21: u32 = 405;

impl Limits {
    /// The elevation range this backend accepts, or `None` when it has no elevation axis: one
    /// declared azimuth-only (`rot_type=Az`), or one with an empty declared range — how the
    /// Rotor-EZ, DCU-1 and ERC family say it (`rot_type=Other`, elevation 0–0), and how every
    /// azimuth-only backend says it to a daemon too old to print `rot_type`.
    ///
    /// `Other` counts when it declares a range: EasyComm II and III (the SatNOGS rotators'
    /// protocol) forward the elevation they are given, and Hamlib's frontend refuses one outside
    /// the range — the same test the satellite track already relies on.
    ///
    /// ⚠️ **Except the Green Heron RT-21.** Its backend declares 0–90° because an RT-21 can drive
    /// elevation through a SECOND controller on a second serial port (rotctld's `-R`); without one
    /// it drops the elevation it is sent and reports 0 (Hamlib's `rotorez.c`). Nexus's own daemon
    /// is never given `-R`, and a daemon cannot say whether it was (it is a command-line option,
    /// not a setting rotctld reports), so an RT-21 is azimuth-only here: sent exactly the line it
    /// always was, and shown no elevation it cannot move.
    pub fn elevation(&self) -> Option<(f64, f64)> {
        (self.max_el > self.min_el && self.kind != Some(RotKind::Az) && self.model != RT21)
            .then_some((self.min_el, self.max_el))
    }
}

/// Parse a `\dump_state` reply, in either shape Hamlib prints: protocol `1` (rotctld 4.5.5 and
/// the bundled 4.7.1: the model, then `min_az=…` lines, `rot_type=…` and `done`) or protocol `0`
/// (4.3.1: seven bare lines — protocol, model, min_az, max_az, min_el, max_el, south_zero).
/// `None` for anything else.
pub fn parse_limits(reply: &str) -> Option<Limits> {
    let lines: Vec<&str> = reply
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let protocol: u32 = lines.first()?.parse().ok()?;
    let model: u32 = lines.get(1)?.parse().ok()?;
    if protocol == 0 {
        let at = |i: usize| lines.get(i)?.parse::<f64>().ok();
        return Some(Limits {
            model,
            min_az: at(2)?,
            max_az: at(3)?,
            min_el: at(4)?,
            max_el: at(5)?,
            kind: None,
        });
    }
    let field = |key: &str| {
        lines
            .iter()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
    };
    let num = |key: &str| field(key)?.parse::<f64>().ok();
    let kind = field("rot_type").map(|t| match t {
        "AzEl" => RotKind::AzEl,
        "Az" => RotKind::Az,
        "El" => RotKind::El,
        _ => RotKind::Other,
    });
    Some(Limits {
        model,
        min_az: num("min_az")?,
        max_az: num("max_az")?,
        min_el: num("min_el")?,
        max_el: num("max_el")?,
        kind,
    })
}

/// A `\dump_state` reply is whole at its `done` line (protocol 1), at seven lines when it began
/// with protocol `0`, or at an `RPRT` refusal.
fn dump_state_done(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.iter().any(|l| *l == "done" || l.starts_with("RPRT"))
        || (lines.first() == Some(&"0") && lines.len() >= 7)
}

/// Ask the rotctld at `addr` what its backend accepts ([`Limits`]). It never touches the serial
/// line: rotctld answers from what it already holds.
///
/// `Ok(None)` when the daemon ANSWERED and declared nothing — it refused the command, answered
/// something else, or hung up on it, as a rotctld-compatible server that is not Hamlib may.
///
/// ⚠️ **An unanswered `\dump_state` is an `Err`, never `Ok(None)`.** rotctld runs one command at a
/// time for all of its clients, so a busy one (another program's command waiting on a slow
/// controller) answers late, and reading "no answer" as "no elevation axis" would send a G-5500
/// the `P <az> 0` this module stopped sending. An unreachable daemon is an `Err` too.
pub fn read_limits(addr: &str) -> std::io::Result<Option<Limits>> {
    match exchange(addr, "\\dump_state\n", POLL_DEADLINE_MS, dump_state_done) {
        Ok(reply) => Ok(parse_limits(&reply)),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "rotctld did not say within {POLL_DEADLINE_MS} ms whether this rotator has an \
                 elevation axis, so Nexus did not move it. rotctld answers that without asking \
                 the rotator, so it is busy with another program's command, or stuck: try again"
            ),
        )),
        Err(e) => Err(e),
    }
}

/// A reported angle as the wire carries it: to the tenth of a degree, inside `lo..=hi`.
fn kept(deg: f64, lo: f64, hi: f64) -> String {
    format!("{:.1}", ((deg * 10.0).round() / 10.0).clamp(lo, hi))
}

/// The `P` line a manual move sends: the axes asked for (`az`, `el`), and any axis left out kept
/// where the rotator reports it. `limits` is what the daemon declared (`None`: it declared
/// nothing); `at` is the position the rotator reported, `None` when it has none to give. Pure, so
/// every rule below is a test.
///
/// - No elevation axis, or nothing declared: [`point_line`], exactly the line an azimuth move
///   always sent; an elevation asked for is refused, since it would reach no axis.
/// - An elevation axis: an elevation asked for must lie inside the declared range (a typed 200°
///   is refused, never clamped onto the stop); a kept one goes back as reported. An azimuth
///   asked for is wrapped into 0–360 as it always was; a kept one goes back RAW, because a
///   G-5500 is a 450° rotator and a reading of 400° sent as 40° would swing the mast a whole turn
///   the other way.
/// - A rotator that does not report the elevation (a backend with no read-back, such as
///   EasyComm I): nothing can be kept, and an azimuth move sends the line it always did. One
///   that does not report the azimuth refuses an elevation move, which would otherwise have to
///   send the mast to a bearing nobody chose.
pub fn manual_line(
    az: Option<f64>,
    el: Option<f64>,
    limits: Option<&Limits>,
    at: Option<(f64, Option<f64>)>,
) -> Result<String, String> {
    if az.is_none() && el.is_none() {
        return Err("nothing to point".to_string());
    }
    if az.is_some_and(|a| !a.is_finite()) || el.is_some_and(|e| !e.is_finite()) {
        return Err("a bearing or an elevation that is not a number".to_string());
    }
    let (Some(l), Some((lo, hi))) = (limits, limits.and_then(Limits::elevation)) else {
        return match (az, limits) {
            (Some(a), _) if el.is_none() => Ok(point_line(a)),
            (_, Some(_)) => Err("this rotator has no elevation axis".to_string()),
            (_, None) => Err(
                "this rotctld does not say whether the rotator has an elevation axis, so Nexus \
                 sends it none"
                    .to_string(),
            ),
        };
    };
    let el = match (el, at.and_then(|(_, e)| e)) {
        (Some(e), _) if (lo..=hi).contains(&e) => format!("{e:.1}"),
        (Some(e), _) => {
            return Err(format!(
                "{e}° is outside the {lo}–{hi}° this rotator's elevation reaches"
            ))
        }
        (None, Some(reported)) => kept(reported, lo, hi),
        // No elevation reported, so none to keep (`az` is set: both-`None` returned above).
        (None, None) => {
            return az
                .map(point_line)
                .ok_or_else(|| "nothing to point".to_string())
        }
    };
    let az = match (az, at) {
        (Some(a), _) => wire_az(a),
        (None, Some((reported, _))) => kept(reported, l.min_az, l.max_az),
        (None, None) => {
            return Err(
                "the rotator does not report its azimuth, so an elevation move would have to \
                 guess one — turn it to a bearing first"
                    .to_string(),
            )
        }
    };
    Ok(format!("P {az} {el}\n"))
}

/// Point the rotator, keeping any axis the caller leaves out where the rotator reports it — the
/// manual slew, the ↗ point-at-call and the Rotor pane's elevation. `Ok` only on `RPRT 0`.
///
/// ⭐ **An azimuth move used to be `P <az> 0` on every rotator** ([`point_line`]). On an az/el
/// mount that commands the ELEVATION to zero — a G-5500 on a GS-232B gets `W200 000` on its
/// serial line (measured through Hamlib's own GS-232B backend) — so every turn of the beam laid
/// the antenna on the horizon. This asks the daemon whether the rotator has an elevation axis
/// ([`read_limits`]) and, when it does, where it is ([`read_position`]), and sends that back
/// ([`manual_line`]). A rotator with no elevation axis (declared azimuth-only, the RT-21, or a
/// rotctld that declares nothing) is sent exactly the line it always was, and is not asked for
/// its position first. A daemon that does not answer the question is sent nothing.
pub fn point_keeping(addr: &str, az: Option<f64>, el: Option<f64>) -> std::io::Result<()> {
    let line = keeping_line(read_limits(addr), || read_position(addr), az, el)?;
    command(addr, &line)
}

/// [`point_keeping`]'s decision without its transport: the line a manual move sends, given what
/// the daemon answered to `\dump_state` (`limits`, as [`read_limits`] returns it) and a way to ask
/// where the rotator is (`position`, as [`read_position`], asked only when an axis must be kept).
/// Every other caller that moves a mast by hand — Nexus Remote's host, whose tests drive it with
/// an in-process mast instead of a socket — goes through this same rule.
pub fn keeping_line(
    limits: std::io::Result<Option<Limits>>,
    position: impl FnOnce() -> std::io::Result<(f64, Option<f64>)>,
    az: Option<f64>,
    el: Option<f64>,
) -> std::io::Result<String> {
    let limits = limits?;
    let keeps =
        limits.as_ref().and_then(Limits::elevation).is_some() && (az.is_none() || el.is_none());
    let at = if keeps {
        match position() {
            Ok(at) => Some(at),
            // An answer that there is no position to give (RPRT -11/-4): nothing to keep.
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => None,
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    manual_line(az, el, limits.as_ref(), at).map_err(std::io::Error::other)
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
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::Unsupported,
            "a backend with none to give"
        );
        // …while a read the controller did not answer is not that: Hamlib's timeout, and a
        // daemon that is not there at all.
        let (addr, _rx) = stage(Behave::Say("RPRT -5\n"));
        let e = read_position(&addr).expect_err("a timeout is not a position");
        assert_ne!(e.kind(), std::io::ErrorKind::Unsupported, "{e}");
        let gone = TcpListener::bind("127.0.0.1:0").expect("a port");
        let addr = gone.local_addr().expect("its address").to_string();
        drop(gone);
        let e = read_position(&addr).expect_err("nothing is listening");
        assert_ne!(e.kind(), std::io::ErrorKind::Unsupported, "{e}");
        // …and the convenience wrapper still degrades to None for the 2 s polls.
        let (addr, _rx) = stage(Behave::Say("RPRT -11\n"));
        assert_eq!(read_azimuth(&addr), None);
    }

    /// A rotctld on loopback that answers each of up to `n` connections (one command each, as
    /// this client always sends) with `answer(command)`, handing back each line it was sent —
    /// recorded BEFORE the answer goes out, so a caller that has its reply finds its line here.
    /// An empty answer is silence: that socket stays open, unanswered, past the client's deadline.
    fn script(
        n: usize,
        answer: impl Fn(&str) -> String + Send + 'static,
    ) -> (String, mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let addr = l.local_addr().expect("its address").to_string();
        l.set_nonblocking(true).expect("non-blocking accept");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(8);
            let mut held = Vec::new();
            while held.len() < n && Instant::now() < until {
                let Ok((mut s, _)) = l.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                let _ = s.set_nonblocking(false);
                let mut line = String::new();
                let mut reader = std::io::BufReader::new(s.try_clone().expect("clone"));
                let _ = reader.read_line(&mut line);
                let reply = answer(line.trim_end());
                let _ = tx.send(line);
                let _ = s.write_all(reply.as_bytes());
                held.push(s);
            }
            // Keep every connection open a while: a complete reply needs no close, and a silent
            // one must meet the client's deadline, not a hang-up.
            std::thread::sleep(Duration::from_secs(3));
        });
        (addr, rx)
    }

    /// What real Hamlib backends answer `\dump_state` with, captured from rotctld 4.5.5 driving
    /// each over a pseudo-terminal (the bundled 4.7.1 prints the same shape). The GS-232B (603)
    /// is the G-5500's interface.
    const GS232B_STATE: &str = "1\n603\nmin_az=-180.000000\nmax_az=450.000000\nmin_el=0.000000\n\
                                max_el=180.000000\nsouth_zero=0\nrot_type=AzEl\ndone\n";
    /// The Idiom Press Rotor-EZ (401): azimuth only, said the family's way — `Other`, 0–0.
    const ROTOR_EZ_STATE: &str = "1\n401\nmin_az=0.000000\nmax_az=360.000000\nmin_el=0.000000\n\
                                  max_el=0.000000\nsouth_zero=0\nrot_type=Other\ndone\n";
    /// The Green Heron RT-21 (405): `Other` with 0–90°, for a second controller Nexus never has.
    const RT21_STATE: &str = "1\n405\nmin_az=0.000000\nmax_az=359.899994\nmin_el=0.000000\n\
                              max_el=90.000000\nsouth_zero=0\nrot_type=Other\ndone\n";
    /// EasyComm II (202): `Other` with 0–180°, an az/el protocol.
    const EASYCOMM2_STATE: &str = "1\n202\nmin_az=0.000000\nmax_az=360.000000\nmin_el=0.000000\n\
                                   max_el=180.000000\nsouth_zero=0\nrot_type=Other\ndone\n";
    /// The F1TE tracker (604): az/el, and no position read-back at all in Hamlib.
    const F1TE_STATE: &str = "1\n604\nmin_az=-180.000000\nmax_az=360.000000\nmin_el=0.000000\n\
                              max_el=180.000000\nsouth_zero=0\nrot_type=AzEl\ndone\n";

    /// A G-5500 on a GS-232B at 123° / 45°, as a scripted rotctld.
    fn g5500(cmd: &str) -> String {
        match cmd {
            "\\dump_state" => GS232B_STATE.to_string(),
            "p" => "123.000000\n45.000000\n".to_string(),
            _ => "RPRT 0\n".to_string(),
        }
    }

    fn lines(rx: &mpsc::Receiver<String>) -> Vec<String> {
        rx.try_iter().map(|l| l.trim_end().to_string()).collect()
    }

    #[test]
    fn dump_state_is_read_in_both_shapes_hamlib_has_used() {
        let l = parse_limits(GS232B_STATE).expect("protocol 1");
        assert_eq!(
            l,
            Limits {
                model: 603,
                min_az: -180.0,
                max_az: 450.0,
                min_el: 0.0,
                max_el: 180.0,
                kind: Some(RotKind::AzEl)
            }
        );
        assert_eq!(l.elevation(), Some((0.0, 180.0)), "the G-5500's 0–180°");
        // Hamlib 4.3.1: protocol 0, seven bare lines and no rot_type.
        let old = parse_limits("0\n603\n-180.000000\n450.000000\n0.000000\n180.000000\n0\n")
            .expect("protocol 0");
        assert_eq!(
            (old.model, old.kind, old.elevation()),
            (603, None, Some((0.0, 180.0)))
        );
        // Neither shape: a refusal, garbage, an empty answer, a protocol-1 reply cut short.
        for junk in ["RPRT -4\n", "hello\n", "", "1\n603\ndone\n", "1\n"] {
            assert_eq!(parse_limits(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn an_elevation_axis_is_what_the_backend_declares() {
        let declared = |state: &str| parse_limits(state).expect("a declaration").elevation();
        assert_eq!(declared(GS232B_STATE), Some((0.0, 180.0)));
        // EasyComm, the SatNOGS rotators' protocol, is `Other` with a range: it has one.
        assert_eq!(declared(EASYCOMM2_STATE), Some((0.0, 180.0)));
        // The Rotor-EZ family says "none" as `Other` with 0–0.
        assert_eq!(declared(ROTOR_EZ_STATE), None);
        // The RT-21 declares 0–90° for a second controller that Nexus's daemon never has.
        assert_eq!(declared(RT21_STATE), None);
        // M2's RC2800_EARLY_AZ (1002) declares Az WITH an elevation range: rot_type says none.
        assert_eq!(
            declared(
                "1\n1002\nmin_az=0.000000\nmax_az=360.000000\nmin_el=0.000000\n\
                 max_el=180.000000\nsouth_zero=0\nrot_type=Az\ndone\n"
            ),
            None
        );
        // A protocol-0 daemon's azimuth-only backend: an empty range.
        assert_eq!(
            declared("0\n401\n0.000000\n360.000000\n0.000000\n0.000000\n0\n"),
            None
        );
    }

    #[test]
    fn a_manual_move_keeps_the_axis_it_was_not_given() {
        let gs232b = parse_limits(GS232B_STATE).unwrap();
        let at = Some((123.0, Some(45.0)));
        // ⭐ THE BUG, as a line: an azimuth move keeps the elevation instead of `P 200.0 0`.
        assert_eq!(
            manual_line(Some(200.0), None, Some(&gs232b), at),
            Ok("P 200.0 45.0\n".into())
        );
        // An elevation move keeps the azimuth — RAW: 400° on a 450° G-5500 stays 400°, where
        // 40° would swing the mast a whole turn back.
        assert_eq!(
            manual_line(None, Some(60.0), Some(&gs232b), Some((400.0, Some(45.0)))),
            Ok("P 400.0 60.0\n".into())
        );
        assert_eq!(
            manual_line(Some(10.0), Some(170.0), Some(&gs232b), None),
            Ok("P 10.0 170.0\n".into())
        );
        // What was typed beyond the mount is refused, never clamped onto the stop.
        let e = manual_line(None, Some(181.0), Some(&gs232b), at).unwrap_err();
        assert!(e.contains("0–180°"), "{e}");
        assert!(manual_line(None, Some(-1.0), Some(&gs232b), at).is_err());
        // A kept reading a hair past a stop goes back as the stop.
        assert_eq!(
            manual_line(Some(90.0), None, Some(&gs232b), Some((1.0, Some(180.4)))),
            Ok("P 90.0 180.0\n".into())
        );
        assert!(manual_line(Some(f64::NAN), None, Some(&gs232b), at).is_err());
        assert!(manual_line(None, None, Some(&gs232b), at).is_err());
    }

    #[test]
    fn a_rotator_that_reports_no_position_keeps_nothing_and_still_points() {
        // The F1TE tracker is az/el with no read-back: there is no elevation to keep, so an
        // azimuth move is the line it always was — not a refusal of a move that works today.
        let f1te = parse_limits(F1TE_STATE).unwrap();
        assert_eq!(
            manual_line(Some(200.0), None, Some(&f1te), None),
            Ok(point_line(200.0))
        );
        // An elevation move has no azimuth to keep, and never sends the mast to 0° instead.
        let e = manual_line(None, Some(30.0), Some(&f1te), None).unwrap_err();
        assert!(e.contains("does not report its azimuth"), "{e}");
        // Both axes given need no reading.
        assert_eq!(
            manual_line(Some(200.0), Some(30.0), Some(&f1te), None),
            Ok("P 200.0 30.0\n".into())
        );
    }

    #[test]
    fn an_azimuth_only_rotator_is_sent_exactly_the_line_it_always_was() {
        let rotor_ez = parse_limits(ROTOR_EZ_STATE).unwrap();
        let rt21 = parse_limits(RT21_STATE).unwrap();
        for limits in [Some(&rotor_ez), Some(&rt21), None] {
            assert_eq!(
                manual_line(Some(200.0), None, limits, None),
                Ok(point_line(200.0))
            );
            assert!(
                manual_line(None, Some(30.0), limits, None).is_err(),
                "an elevation reaches no axis"
            );
        }
    }

    #[test]
    fn point_keeping_asks_the_daemon_then_sends_the_elevation_back() {
        let (addr, rx) = script(3, g5500);
        point_keeping(&addr, Some(200.0), None).expect("RPRT 0");
        assert_eq!(lines(&rx), ["\\dump_state", "p", "P 200.0 45.0"]);

        // Azimuth-only rotators, the RT-21 among them: no position read, and today's line.
        for state in [ROTOR_EZ_STATE, RT21_STATE] {
            let (addr, rx) = script(2, move |cmd| match cmd {
                "\\dump_state" => state.to_string(),
                _ => "RPRT 0\n".to_string(),
            });
            point_keeping(&addr, Some(200.0), None).expect("RPRT 0");
            assert_eq!(
                lines(&rx),
                [
                    "\\dump_state".to_string(),
                    point_line(200.0).trim_end().to_string()
                ]
            );
        }

        // A rotctld that is not Hamlib, and refuses `\dump_state`: today's line again.
        let (addr, rx) = script(2, |cmd| {
            if cmd == "\\dump_state" {
                "RPRT -4\n"
            } else {
                "RPRT 0\n"
            }
            .to_string()
        });
        point_keeping(&addr, Some(200.0), None).expect("RPRT 0");
        assert_eq!(lines(&rx)[1], point_line(200.0).trim_end());

        // An az/el rotator with no read-back: the position is asked, refused, and the azimuth
        // still goes out as it always did.
        let (addr, rx) = script(3, |cmd| match cmd {
            "\\dump_state" => F1TE_STATE.to_string(),
            "p" => "RPRT -11\n".to_string(),
            _ => "RPRT 0\n".to_string(),
        });
        point_keeping(&addr, Some(200.0), None).expect("RPRT 0");
        assert_eq!(
            lines(&rx),
            ["\\dump_state", "p", point_line(200.0).trim_end()]
        );

        // An elevation outside the mount's range reaches no command at all.
        let (addr, rx) = script(3, g5500);
        assert!(point_keeping(&addr, None, Some(200.0)).is_err());
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !lines(&rx).iter().any(|l| l.starts_with('P')),
            "refused before the wire"
        );
    }

    #[test]
    fn an_unanswered_dump_state_is_not_an_azimuth_only_rotator() {
        // rotctld runs one command at a time for every client, so a busy one leaves the question
        // unanswered. Reading that as "no elevation axis" would send a G-5500 `P <az> 0`, so
        // nothing is sent at all, and the error says why.
        let (addr, rx) = script(2, |cmd| {
            if cmd == "\\dump_state" {
                String::new()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let e = point_keeping(&addr, Some(200.0), None).expect_err("cannot tell");
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
        assert!(e.to_string().contains("elevation axis"), "{e}");
        assert_eq!(lines(&rx), ["\\dump_state"], "nothing reached the rotator");
        // …while one that answers by hanging up is a server that declares nothing.
        let (addr, _rx) = stage(Behave::Hangup);
        assert_eq!(read_limits(&addr).expect("an answer of sorts"), None);
    }
}
