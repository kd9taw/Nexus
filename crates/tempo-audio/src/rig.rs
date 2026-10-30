//! PTT / CAT control via Hamlib's `rigctld` daemon over TCP.
//!
//! Using `rigctld` (rather than linking `libhamlib`) keeps Tempo free of a C
//! build dependency: the operator runs `rigctld -m <model> -r <port>` and Tempo
//! talks to it over a socket. The protocol is line-based — commands like `T 1`
//! (PTT on), `T 0` (PTT off), `F 14074000` (set freq), `M USB 0` (set mode); a
//! reply of `RPRT 0` means success.
//!
//! For rigs without CAT, [`PttMode::Vox`] performs no keying and relies on the
//! transceiver's VOX (audio-triggered TX).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tempo_app::remote_control::WritePermission;

pub mod remote;
pub(crate) mod tuning;

fn remote_permission_error(reason: tempo_app::remote_control::Reason) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("Remote CAT permission: {reason:?}"),
    )
}

#[cfg(test)]
#[path = "rig_remote_tests.rs"]
pub(crate) mod remote_tests;

/// Which serial control line keys the transmitter for [`PttMode::Serial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialLine {
    /// Request To Send.
    Rts,
    /// Data Terminal Ready.
    Dtr,
}

/// How transmit keying is performed — INDEPENDENT of CAT control. The WSJT-X model:
/// a rig can have full CAT freq/mode control while keying via VOX or a serial line,
/// so PTT and control are separate concerns (see [`Rig`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PttMode {
    /// Key via the CAT control channel (rigctld `T`). Requires a control channel;
    /// with none configured this no-ops (like VOX).
    Cat,
    /// [`PttMode::Cat`], keying the radio's DATA input — "Transmit audio source (CAT PTT):
    /// Rear/Data" (#381), WSJT-X's choice. A transmission whose audio Nexus plays keys with
    /// `T 3` ([`ptt_data_line`], Hamlib's `RIG_PTT_ON_DATA`: `TX1;` on a TS-590S, DATA SEND from
    /// ACC2/USB); anything keyed through [`Rig::ptt_plain`] still keys `T 1`, and every release
    /// is `T 0` exactly as for `Cat`.
    CatData,
    /// No CAT keying — rely on the rig's VOX (audio-triggered TX).
    #[default]
    Vox,
    /// Key directly by asserting a serial control line (RTS or DTR) on `port`.
    ///
    /// With the `serial` feature this drives the line via `serialport`; without
    /// it the keying is logged and otherwise a no-op (the build has no serial
    /// backend), so the engine still runs and can fall back to VOX.
    Serial { port: String, line: SerialLine },
}

/// Reply deadline (ms) for a PTT (`T`) command — a one-line RPRT that must answer fast. Kept off
/// the slow-transport 2.5 s read window so an un-key can never hang the single-threaded radio loop.
const PTT_DEADLINE_MS: u64 = 700;

/// Reply deadline (ms) for a KEY-DOWN on a network chain ([`Rig::set_network_chain`]): the same
/// window every other command there gets, because a rigctld reaching its radio over the network
/// can take that long to answer a key that works. The release keeps [`PTT_DEADLINE_MS`].
const NETWORK_KEY_DEADLINE_MS: u64 = 2_500;

/// How long a connection may go on owing late answers once a RELEASE has gone out behind the key
/// that owes them ([`Rig::command_lines`]) before it is given up and the next command opens a new
/// one.
///
/// Giving it up then is safe: the key and its release are both already queued on it, in order,
/// and rigctld carries out the lines a client sent before it closed (measured: the late `T 1` and
/// then its `T 0`, after the close). Before a release is queued it is never given up, since that is
/// the defect the connection is kept for. The bound only ends a connection one answer behind for
/// good (a peer that lost an answer, so every later answer is taken for the line before it): there
/// every release reads as unconfirmed and every refusal as no answer, until the release on a new
/// connection is confirmed. 30 s is past the longest stall measured behind a held rigctld (20 s, a
/// Hamlib timeout waited out under its client lock), so a stall that ends is still answered on its
/// own connection, and short enough that such a wedge is over in half a minute.
const OWED_ANSWER_BOUND: Duration = Duration::from_secs(30);

/// Whether `b` ends one answer of an ordered line ([`Rig::command_lines`]): rigctld ends a `T` or
/// `\stop_morse` answer with a newline, and a `W <cmd> 0` raw set with one terminator, a NUL on
/// Hamlib 4.7.1 and a newline on 4.5.5 ([`Rig::send_raw_set`]).
fn ends_an_answer(b: u8) -> bool {
    b == b'\n' || b == b'\0'
}

/// How long opening the CAT connection may take, per address the control address resolves to.
///
/// ⚠️ RADIO-LOOP LIVENESS. The radio loop opens (and, after any failed command, re-opens) this
/// connection itself, so an unbounded connect to a network rigctld that is firewalled or switched
/// off held the loop for the OS SYN-retry time (~127 s on Linux, ~21 s on Windows): no slot
/// boundary, no PTT deadline, no tune ceiling meanwhile. A rigctld that is there answers in
/// milliseconds (a local one at once), so this bounds only the connect nobody answers, which then
/// fails as "not reachable" and the radio reads as down. Three seconds, as for a Flex's own API.
const CAT_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Open the CAT connection to `addr`, trying each address it resolves to (as
/// `TcpStream::connect` does) but each within [`CAT_CONNECT_TIMEOUT`]. A connect nobody answers
/// is `TimedOut` with words saying so; every other failure (a refused port, an unresolvable name)
/// is returned as the OS gave it, so a local rigctld that is not running reads as it always has.
fn connect_cat(addr: &str) -> std::io::Result<TcpStream> {
    use std::net::ToSocketAddrs;
    let mut last = None;
    for at in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&at, CAT_CONNECT_TIMEOUT) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(connect_cat_failure(addr, last))
}

/// The error [`connect_cat`] returns after every address failed with `last` (`None`: the address
/// resolved to none). A connect nobody answered carries the [`NotReachable`] marker.
pub(crate) fn connect_cat_failure(addr: &str, last: Option<std::io::Error>) -> std::io::Error {
    match last {
        Some(e) if e.kind() == std::io::ErrorKind::TimedOut => std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            NotReachable(format!(
                "rigctld at {addr} is not reachable: nothing answered within {} s",
                CAT_CONNECT_TIMEOUT.as_secs()
            )),
        ),
        Some(e) => e,
        None => std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("rigctld address {addr} resolved to no address"),
        ),
    }
}

/// What [`connect_cat`]'s `TimedOut` carries, in its own words: the connection was never made.
#[derive(Debug)]
struct NotReachable(String);

impl std::fmt::Display for NotReachable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotReachable {}

/// `EHOSTDOWN` ("Host is down"), which std leaves uncategorised: macOS's answer for a LAN host
/// that does not answer ARP, where Linux answers `EHOSTUNREACH`.
pub(crate) const EHOSTDOWN: Option<i32> = if cfg!(any(target_os = "linux", target_os = "android")) {
    Some(112)
} else if cfg!(target_vendor = "apple") {
    Some(64)
} else if cfg!(windows) {
    Some(10064) // WSAEHOSTDOWN
} else {
    None
};

/// Whether `e` says the CAT host, or the network to it, cannot be reached — a network rigctld
/// whose machine is switched off or unplugged — rather than a radio behind a reached rigctld
/// that did not answer, or one that said no. The radio loop counts it as the link not answering
/// (never a refusal), and says "can't reach" rather than "no reply from the rig".
pub fn cat_unreachable(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(e.kind(), HostUnreachable | NetworkUnreachable | NetworkDown)
        || (e.raw_os_error().is_some() && e.raw_os_error() == EHOSTDOWN)
        || e.get_ref().is_some_and(|inner| inner.is::<NotReachable>())
}

/// rigctld command line for PTT.
pub fn ptt_line(on: bool) -> String {
    format!("T {}\n", on as u8)
}
/// rigctld command line to key the radio's DATA input: `T 3`, Hamlib's `RIG_PTT_ON_DATA` (#381).
/// Only a key-down: whatever keyed the radio, the release is [`ptt_line`]`(false)`.
pub fn ptt_data_line() -> String {
    "T 3\n".to_string()
}
/// rigctld command line to set the dial frequency (Hz).
pub fn freq_line(hz: u64) -> String {
    format!("F {hz}\n")
}
/// rigctld command line to set mode + passband (Hz). `-1` = `RIG_PASSBAND_NOCHANGE` (leave the
/// rig's current filter width untouched); `0` = `RIG_PASSBAND_NORMAL` (Hamlib actively commands
/// the rig's *own* default width for the mode).
pub fn mode_line(mode: &str, passband_hz: i32) -> String {
    format!("M {mode} {passband_hz}\n")
}
/// rigctld `R` — FM repeater shift: "plus"→`+`, "minus"→`-`, anything else→`None`.
pub fn rptr_shift_line(shift: &str) -> String {
    // Normalised by `tempo_app::settings::rptr_shift_sign`, NOT by a second copy of the match:
    // the licence gate asks the same function which way this shift moves the transmitter, and a
    // wire layer that disagreed with it would key somewhere the gate had not judged.
    let s = match tempo_app::settings::rptr_shift_sign(shift) {
        1 => "+",
        -1 => "-",
        _ => "None",
    };
    format!("R {s}\n")
}
/// rigctld `O` — FM repeater offset magnitude (Hz).
pub fn rptr_offset_line(hz: i64) -> String {
    format!("O {hz}\n")
}
/// rigctld `C` — CTCSS (PL) tone. Hamlib wants TENTHS of Hz (100.0 Hz → 1000); 0 = off.
pub fn ctcss_line(tone_hz: f32) -> String {
    format!("C {}\n", (tone_hz * 10.0).round().max(0.0) as u32)
}
/// rigctld `S` — split on/off + which VFO transmits (e.g. `S 1 VFOB`).
pub fn split_line(on: bool, tx_vfo: &str) -> String {
    format!("S {} {}\n", on as u8, tx_vfo)
}
/// rigctld `I` — the split (TX) frequency in Hz.
pub fn split_freq_line(hz: u64) -> String {
    format!("I {hz}\n")
}
/// rigctld `X` — the split (TX) VFO's MODE + passband. The plain `M` verb only
/// ever reaches the RX VFO, so this is the one way to say "transmit in LSB while
/// I listen in USB" — which is precisely what a linear INVERTING satellite
/// transponder needs (see `tempo_core::doppler::uplink_mode_for`).
pub fn split_mode_line(mode: &str, passband_hz: i32) -> String {
    format!("X {mode} {passband_hz}\n")
}
/// rigctld `V` — select the active VFO (e.g. `VFOA`, `VFOB`, `Main`, `Sub`).
pub fn vfo_line(vfo: &str) -> String {
    format!("V {vfo}\n")
}
/// What a `U FUNC <n>` attempt actually ESTABLISHED — three outcomes, not two.
///
/// ⭐⭐ R1 (transmit review, 2026-09-19). Collapsing this into `Result` is what made a
/// tune-up the radio really started read as one it refused: `Err` covered both "the rig
/// answered no" and "we stopped listening after 700 ms while Hamlib was still reading the
/// rig's reply". The ATU is a transmit action, and those two must never be the same thing.
///
/// The shape is `remote_control::Completion::refuse`'s: a failure AFTER an attempted write is
/// Unknown, never Rejected.
#[derive(Debug)]
pub enum FuncSet {
    /// `RPRT 0` — the rig took it.
    Ok,
    /// Nothing is acting on this command: either the rig ANSWERED with an error code, or the
    /// line never reached the wire at all. The radio's state is what it was.
    Refused(String),
    /// The line WENT OUT and no verdict came back — no reply inside our window, or the link
    /// dropped under it. The rig may be doing exactly what we asked, right now.
    Uncertain(String),
}

/// rigctld `U` — set a function (RIT/XIT must be enabled this way before `J`/`Z`).
///
/// Takes a VALUE, not a bool, because `RIG_FUNC_TUNER` is not boolean — see
/// [`ATU_START_TUNE`]. Every other function this project sends is on/off; pass `u8::from(bool)`.
pub fn func_line(func: &str, value: u8) -> String {
    format!("U {func} {value}\n")
}
/// The `U TUNER <n>` value that asks a rig to RUN ITS ATU TUNE-UP, as opposed to merely
/// switching the tuner in-line.
///
/// ⚠️ THIS IS WHY THE ATU BUTTON DID NOTHING (operator, 2026-08-15, FTDX10). Nexus sent
/// `U TUNER 1`, and on a Yaesu that is `AC001` — "ATU in line". If the tuner was already in
/// line, which it usually is, the rig does exactly nothing: no relays, no carrier, no change
/// the operator can see. Read out of Hamlib's own source rather than inferred from ours
/// (`rigs/yaesu/newcat.c`, `case RIG_FUNC_TUNER`):
///
/// ```text
/// SNPRINTF(priv->cmd_str, ..., "AC00%d%c", status == 0 ? 0 : status, cat_term);
/// ```
///
/// The status passes STRAIGHT THROUGH, so `2` produces `AC002` — Yaesu's "start tuning".
///
/// WHY SENDING 2 IS SAFE ON EVERY OTHER BACKEND, also read from source, because a class-wide
/// CAT change is exactly where this project has been burned before:
///  * **Icom** (`rigs/icom/icom.c::icom_set_func`) — `fctbuf[0] = status ? 0x01 : 0x00`, so 2
///    is clamped to the same 0x01 it sends today. No change, no regression. (Icom's own
///    "start tune" byte 0x02 is simply not reachable through `RIG_FUNC_TUNER`.)
///  * **Kenwood** (`rigs/kenwood/kenwood.c`) — `"AC1%c0"` with `(status == 0) ? '0' : '1'`,
///    so 2 is likewise identical to 1 (and its third digit, the tune-state, is hard-coded 0).
///
/// So this is a real fix on Yaesu and a no-op everywhere else — never a new behaviour on a
/// radio that was working.
///
/// ⭐ AND SINCE 1.14 IT IS NOT EVEN SENT THERE (operator ruling, 2026-09-19: "Say it can't,
/// add it natively"). A no-op the rig ACKs is worse than it looks — the ATU button switched
/// the tuner in line, tuned nothing, and the Digital section stood down for it (#322 / R4) —
/// so [`crate::rigmodels::hamlib_atu_start_tune_reaches`] answers the capability from the
/// model, the engine's ATU gate refuses the press, and the cockpit disables the button with a
/// reason. The native Icom (CI-V) path is where those rigs get a real tune-up.
///
/// ⚠️ AND NOT ON THE FT-710, whose `AC` P3 2 is unused and whose tuning start is 3 (Yaesu's
/// FT-710 CAT manual, page 8): the radio loop sends
/// [`crate::rigmodels::hamlib_atu_start_tune_value`], which is this value for every other model.
///
/// ⚠️ NEEDS BENCH (no rig on this machine). What is verified here is the BYTES: that Nexus
/// sends `U TUNER 2` and that Hamlib carries the 2 to the backend unclamped. What is NOT
/// verified is the FTDX10 actually starting a tune-up cycle in response to `AC002`, and
/// whether it does so when the ATU is currently switched out.
pub const ATU_START_TUNE: u8 = 2;

/// rigctld `J` — RIT offset in Hz (receive incremental tuning).
pub fn rit_line(hz: i32) -> String {
    format!("J {hz}\n")
}
/// rigctld `Z` — XIT offset in Hz (transmit incremental tuning).
pub fn xit_line(hz: i32) -> String {
    format!("Z {hz}\n")
}
/// rigctld `L` — set a level by name (e.g. `RFPOWER 0.5` 0..1, `KEYSPD 25` WPM).
pub fn level_line(name: &str, value: &str) -> String {
    format!("L {name} {value}\n")
}
/// rigctld `L` for a NAMED receiver — `L Sub AF 0.500`, Hamlib's VFO-mode spelling. Main is the
/// plain [`level_line`], byte for byte, so nothing sent to a single-receiver radio can change.
pub fn receiver_level_line(rx: crate::dualrx::ReceiverId, name: &str, value: &str) -> String {
    match rx {
        crate::dualrx::ReceiverId::Main => level_line(name, value),
        crate::dualrx::ReceiverId::Sub => format!("L Sub {name} {value}\n"),
    }
}
/// rigctld `b` — send_morse: the rig keys CW from this text (rest of the line).
///
/// ⚠️ A SINGLE-CHARACTER WORD IS PADDED TO TWO (issue #86, VE3FMQ, FT-991A: "CQ does not
/// send the K after the call sign", "? is not sent at all"). Hamlib's Yaesu backend
/// (`newcat_send_morse`, in the 4.7.1 DLL Nexus ships) interprets a ONE-character message as
/// a keyer-memory playback number — valid only '1'..'5', anything else returns -RIG_EINVAL
/// and keys NOTHING. Nexus queues CW word-at-a-time (the Stop-TX contract), so every 1-char
/// word — the closing K, a bare ?, R, E — was silently rejected, and a lone digit 1-5 would
/// have PLAYED the rig's stored memory instead of keying the digit. A trailing space makes
/// the message two characters, which routes down the working KM1/KY path; in Morse a
/// trailing space is only a word gap, and on the other backends (Kenwood pads its KY buffer
/// with spaces anyway; Icom sends the text) it is equally harmless.
pub fn morse_line(text: &str) -> String {
    if text.chars().count() == 1 {
        format!("b {text} \n")
    } else {
        format!("b {text}\n")
    }
}
/// ⛔ Why the radio's own CW keyer is never handed `text` ([`Rig::send_morse`]), in the CW screen's
/// words, or `None` when it may be. Hamlib puts the text inside a radio command (`KY <text>;` to a
/// Kenwood or an Elecraft, `KM1<text>;` to a newer Yaesu), and those radios end a command at the
/// first `;`, so a `;` in the text would end the keyer's command early and the radio would read
/// the rest as a command of its own; a line break would end rigctld's own command. Only what
/// [`tempo_core::cw::sendable`] keeps goes, with spaces between words: the rule a TCI program's
/// CW keeps too. Nexus's own CW is refused whole by the same rule before any of it is queued
/// (`Engine::send_cw`), so this is the backstop: a word reaches it only when the keyer was changed
/// to this one under a message already queued, or from a caller outside the engine.
pub fn cw_text_refusal(text: &str) -> Option<String> {
    let c = text
        .chars()
        .find(|c| *c != ' ' && !tempo_core::cw::sendable(*c))?;
    Some(tempo_core::cw::keyer_refusal(c))
}
/// Parse the S-meter reading (dB relative to S9) from a rigctld `l STRENGTH` reply.
/// Hamlib reports STRENGTH as a signed integer dB value where S9 = 0 dB (S1 ≈ -48 dB,
/// S9+20 = +20 dB) — NOT the 0.0–1.0 fraction `read_level` expects. Returns `None` when
/// the rig answered with no number (RPRT/empty) or an implausible out-of-range value.
pub fn parse_smeter_db(reply: &str) -> Option<i32> {
    reply
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("RPRT"))
        .find_map(|l| l.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.round() as i32)
        // Real STRENGTH spans ~S0 (-54 dB) to S9+60; allow margin for noise-floor reads,
        // reject only clearly-garbage magnitudes (e.g. an error sentinel that parsed).
        .filter(|db| (-80..=100).contains(db))
}

/// True if a rigctld reply indicates success (`RPRT 0`).
pub fn reply_ok(reply: &str) -> bool {
    reply.lines().any(|l| l.trim() == "RPRT 0")
}

/// The Hamlib result code a rigctld reply carries (`RPRT -5` → `Some(-5)`), if any.
pub fn rprt_code(reply: &str) -> Option<i32> {
    reply
        .lines()
        .find_map(|l| l.trim().strip_prefix("RPRT ")?.trim().parse::<i32>().ok())
}

/// Is this Hamlib code a LINK fault — the rig did not answer — rather than a refusal?
///
/// ⚠️ THIS DISTINCTION IS LOAD-BEARING (the overnight-radio review, 2026-09-02). Every
/// non-`RPRT 0` reply used to become `ErrorKind::Other`, which the radio loop reads as "the
/// rig refused it": a radio that was merely SWITCHED OFF answered `RPRT -5` (Hamlib's
/// ETIMEOUT), was reported as "does not cover that frequency", and had its band given up
/// on — a give-up that no recovery cleared. Hamlib's own codes, from `rig.h`:
/// -5 ETIMEOUT, -6 EIO, -13 BUSERROR, -14 BUSBUSY are the link not answering;
/// -1 EINVAL, -9 ERJCTED, -15 EARG, -17 EDOM (and the rest) are the rig, or Hamlib, saying no.
pub fn rprt_is_link_fault(code: i32) -> bool {
    matches!(code, -5 | -6 | -13 | -14)
}

/// Turn a non-OK rigctld reply into the error the radio loop can act on: a link fault is
/// `TimedOut` (so the loop's "no reply from the rig" wording and its circuit breaker apply);
/// anything else stays `Other` — a genuine refusal.
fn rprt_error(what: &str, reply: &str) -> std::io::Error {
    match rprt_code(reply) {
        Some(code) if rprt_is_link_fault(code) => std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("rigctld {what}: the rig did not answer (Hamlib RPRT {code})"),
        ),
        _ => std::io::Error::other(format!("rigctld {what} error: {reply:?}")),
    }
}

/// A frequency line of a rigctld reply, in Hz. Hamlib's own rigctld prints a whole number
/// (`14074000`); wfview's prints Hamlib's `%lf` form (`14074000.000000`), which Hamlib's NET
/// client reads as a double, so it is read as one here too and rounded to the Hz. The bounds are
/// the broker's for an `F` it is sent (finite, 0 to 1 THz).
fn hz_in_line(line: &str) -> Option<u64> {
    let hz = line.trim().parse::<f64>().ok()?;
    (hz.is_finite() && (0.0..=1e12).contains(&hz)).then(|| hz.round() as u64)
}

/// Parse the RECEIVE frequency ranges (Hz, inclusive) out of a rigctld `\dump_state` reply.
///
/// The format is Hamlib's own machine-readable capability dump — the one its NETRIGCTL backend
/// parses to reconstruct a remote rig's caps, so it is stable in a way the prose `dump_caps`
/// output is not. Grounded in Hamlib 4.7.1 `tests/rigctl_parse.c` (`declare_proto_rig(dump_state)`):
///
/// ```text
/// 1                                             protocol version (RIGCTLD_PROT_VER)
/// 1035                                          rig model
/// 0                                             ITU region (deprecated, always 0)
/// 30000.000000 60000000.000000 0x1ff -1 -1 0x3 0x3     RX range: start end modes lo_pwr hi_pwr vfo ant
/// …more RX ranges…
/// 0 0 0 0 0 0 0                                 END of the RX list
/// 1800000.000000 2000000.000000 0x1ff 5 100 0x3 0x3    TX ranges follow…
/// 0 0 0 0 0 0 0                                 END of the TX list
/// ```
///
/// Deliberately strict: `None` unless the reply really looks like a dump_state (leading protocol
/// version, then ≥1 well-formed 7-field range line, then the all-zero terminator). Everything
/// else — an `RPRT` error, an empty list (Nexus's own CI-V daemon, which does not know its radio's
/// range), a future format — reads as "unknown", and the caller must then fail OPEN rather than
/// assume nothing is covered.
pub fn parse_dump_state_rx_ranges(reply: &str) -> Option<Vec<(u64, u64)>> {
    let mut lines = reply.lines().map(str::trim).filter(|l| !l.is_empty());
    // Line 1 is the protocol version. Accept only versions whose range-list layout we know
    // (0 and 1 share it); anything else may have moved the lists and must read as unknown.
    if !matches!(lines.next()?.parse::<u32>().ok()?, 0 | 1) {
        return None;
    }
    lines.next()?.parse::<i64>().ok()?; // rig model
    lines.next()?.parse::<i64>().ok()?; // ITU region
    let mut ranges = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 7 {
            return None; // not the range-list shape we were promised — refuse to guess
        }
        // Frequencies print through FREQFMT (`freq_t` is a double), so "30000.000000".
        let start = f[0].parse::<f64>().ok()?;
        let end = f[1].parse::<f64>().ok()?;
        if start == 0.0 && end == 0.0 {
            // The all-zero terminator closes the RX list. TX ranges follow; we don't need them.
            return (!ranges.is_empty()).then_some(ranges);
        }
        if !(start.is_finite() && end.is_finite()) || start < 0.0 || end < start {
            return None;
        }
        ranges.push((start as u64, end as u64));
    }
    None // ran out of lines without ever seeing the terminator
}

/// The rig's ATTENUATOR pads and PREAMP steps, in dB, as its `\dump_state` declares them.
/// Both ascending, both WITHOUT the implicit `0` (off) every radio has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSteps {
    /// Preamp labels, in list order — the position of a label in here is what the rig is
    /// actually told to select on an Icom, so the ORDER is data, not presentation.
    pub preamp_db: Vec<u8>,
    /// Attenuator pads.
    pub attenuator_db: Vec<u8>,
}

/// Parse the PREAMP and ATTENUATOR step lists out of a rigctld `\dump_state` reply.
///
/// ⚠️ These are nine fields past where [`parse_dump_state_rx_ranges`] stops, which is why
/// nothing read them before: that function returns at the RX list's all-zero terminator, and
/// everything below is still in the reply, unexamined. Grounded in Hamlib 4.7.1
/// `tests/rigctl_parse.c` (`declare_proto_rig(dump_state)`) and checked against real replies
/// from Hamlib 4.5.5's own `rigctld` (`tests/fixtures/dump_state_ic7300.txt`, `…7610.txt`):
///
/// ```text
/// 1                     protocol version          ─┐
/// 3078                  rig model                  │ 3 scalars
/// 0                     ITU region                ─┘
/// <rx range rows…>      terminated by "0 0 0 0 0 0 0"
/// <tx range rows…>      terminated by "0 0 0 0 0 0 0"
/// <tuning steps…>       "<modes> <hz>" rows, terminated by "0 0"
/// <filters…>            "<modes> <hz>" rows, terminated by "0 0"
/// 9999                  max_rit                   ─┐
/// 9999                  max_xit                    │ 4 scalars
/// 0                     max_ifshift                │
/// 0                     announces                 ─┘
/// 12 20                 PREAMP list      ← this
/// 6 12 18               ATTENUATOR list  ← and this
/// 0x9c20c1013bfe        has_get_func, and five more masks
/// ```
///
/// **`None` means UNKNOWN and must fail the same way the range table does** — hide the
/// control, do not guess a ladder. An EMPTY list is a different, positive answer: this radio
/// has no pad. A caller that cannot tell those apart will eventually offer a made-up pad.
pub fn parse_dump_state_db_lists(reply: &str) -> Option<DbSteps> {
    let mut lines = reply.lines().map(str::trim).filter(|l| !l.is_empty());
    // Only the protocol versions whose field order is known — a later one may have moved
    // the lists, and reading the wrong line here yields a plausible-looking wrong ladder.
    if !matches!(lines.next()?.parse::<u32>().ok()?, 0 | 1) {
        return None;
    }
    lines.next()?.parse::<i64>().ok()?; // rig model
    lines.next()?.parse::<i64>().ok()?; // ITU region

    // Two range lists (7 fields each), then two "<modes> <hz>" lists (2 fields each). Each
    // ends at its own all-zero row, and a row of the wrong width means this is not the
    // layout we were promised — refuse rather than walk on and miscount.
    let mut skip_list = |width: usize| -> Option<()> {
        for line in lines.by_ref() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() != width {
                return None;
            }
            // The terminator is all-zero; `0` parses under every field's radix.
            if f.iter()
                .all(|v| v.trim_start_matches("0x").parse::<u64>() == Ok(0))
            {
                return Some(());
            }
        }
        None // ran out of lines without a terminator
    };
    skip_list(7)?; // RX ranges
    skip_list(7)?; // TX ranges
    skip_list(2)?; // tuning steps
    skip_list(2)?; // filters
    for _ in 0..4 {
        // max_rit, max_xit, max_ifshift, announces
        lines.next()?.parse::<i64>().ok()?;
    }

    // Hamlib prints each list as space-separated decibels and ends it with the newline —
    // there is no count and no terminator VALUE, so an absent list is a `0` or a bare line.
    let list = |line: &str| -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for tok in line.split_whitespace() {
            match tok.parse::<u8>().ok()? {
                0 => break, // RIG_DBLST_END — and what our own broker emits for "none"
                v => out.push(v),
            }
        }
        Some(out)
    };
    let preamp_db = list(lines.next()?)?;
    let attenuator_db = list(lines.next()?)?;
    Some(DbSteps {
        preamp_db,
        attenuator_db,
    })
}

/// Whether `hz` falls inside any of `ranges`. `ranges` empty is caller-checked, not handled here.
pub fn ranges_cover(ranges: &[(u64, u64)], hz: u64) -> bool {
    ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(&hz))
}

/// Parse a rigctld `u <FUNC>` (get-function) reply. In the default protocol a SUCCESSFUL get
/// returns the value ONLY — `0` or `1` on its own line, with NO `RPRT` — while an error returns
/// `RPRT <negative>` (e.g. `-11` ENAVAIL = the rig doesn't have this func). So an `RPRT` line
/// means unavailable/errored → `None`; otherwise the first `0`/`1` → `Some(off/on)`.
pub fn parse_func_reply(reply: &str) -> Option<bool> {
    for l in reply.lines() {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        if l.starts_with("RPRT") {
            return None; // error / not available — the caller keeps the last known state
        }
        match l {
            "0" => return Some(false),
            "1" => return Some(true),
            _ => {}
        }
    }
    None
}

/// Parse a rigctld `m` (get_mode) reply into (mode, passband_hz): the mode name on one line and
/// the RX passband width (Hz) on the next. Either may be absent on a given read (a networked
/// chain can split the two lines). Ignores `RPRT`/blank lines; a 0 width (rig's "default filter")
/// is treated as no-value.
/// Has a newline-terminated reply delivered all `expected_lines` of its payload?
///
/// `expected_lines` is the rigctld protocol's own answer length for the verb (1 for almost
/// everything, 2 for `m` and `s`), not a guess about the rig.
///
/// ⚠️ **An `RPRT` line ends the reply whatever the count says.** A refusal is rigctld's ENTIRE
/// answer — it carries none of the payload lines a success would — so waiting for a second line
/// after `RPRT -1` would burn the deadline on a reply that is already complete, and turn every
/// rejected `m` into a timeout that drops the connection.
fn reply_complete(out: &[u8], expected_lines: usize) -> bool {
    if !out.ends_with(b"\n") {
        return false;
    }
    // ⚠️ THE SINGLE-LINE PATH IS THE OLD CONDITION, EXACTLY. Every verb but `m` and `s` comes
    // through here, including the keying ones, so it must not acquire a new way to fail. Counting
    // NON-EMPTY lines would not be identical: a reply of just "\n" has none, and would go from
    // an immediate Ok to waiting out the deadline and dropping the connection.
    if expected_lines <= 1 {
        return true;
    }
    let text = String::from_utf8_lossy(out);
    if text.lines().any(|l| l.trim_start().starts_with("RPRT")) {
        return true;
    }
    text.lines().filter(|l| !l.trim().is_empty()).count() >= expected_lines
}

pub fn parse_mode_passband(reply: &str) -> (Option<String>, Option<u32>) {
    let mut mode = None;
    let mut passband = None;
    for l in reply
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("RPRT"))
    {
        if let Ok(hz) = l.parse::<u32>() {
            if passband.is_none() && hz > 0 {
                passband = Some(hz);
            }
        } else if mode.is_none() {
            mode = Some(l.to_string());
        }
    }
    (mode, passband)
}

/// A handle to the rig's keying + CAT tuning.
///
/// CAT control (freq/mode/split/RIT/power/morse) and PTT keying are SEPARATE concerns.
/// The `control` channel is present whenever a CAT rig is configured — independent of
/// how PTT is keyed — so a rig keyed by VOX or a serial line still receives freq/mode
/// commands (the WSJT-X model). `control == None` means no CAT (every CAT verb is a
/// quiet no-op); PTT then keys via [`PttMode`].
pub struct Rig {
    /// CAT control channel — rigctld `host:port` (e.g. `127.0.0.1:4532`). `Some` =
    /// a CAT rig is configured; drives all freq/mode/CAT verbs AND `PttMode::Cat`.
    control: Option<String>,
    /// Lazily-opened TCP connection to `control`.
    stream: Option<TcpStream>,
    /// How PTT is keyed (independent of `control`).
    ptt_mode: PttMode,
    /// Lazily-opened serial port for [`PttMode::Serial`] (feature `serial`).
    #[cfg(feature = "serial")]
    serial: Option<Box<dyn serialport::SerialPort>>,
    /// Last PTT state we commanded (also lets callers/tests observe keying).
    pub keyed: bool,
    /// True when the CAT link is a NETWORK transport (rigctld → SmartSDR / remote radio), where a
    /// reply can legitimately take seconds. Serial rigs answer in milliseconds, so a serial link
    /// uses a much shorter per-command deadline — a stalled serial read then can't hold the radio
    /// loop (and the fast dial poll) for 2.5 s. Default false (serial / local).
    slow_transport: bool,
    /// True when the CAT link reaches the radio over the network (a NET rigctl chain, a Flex): a
    /// key-down that waits for the link ([`Self::key_awaiting_answer`]) waits
    /// [`NETWORK_KEY_DEADLINE_MS`] for its answer instead of [`PTT_DEADLINE_MS`].
    network_chain: bool,
    /// Answers still owed on `stream` by ordered lines (a key, a release, a stop) whose deadline
    /// passed with the line sent ([`Self::command_lines`]): one terminator each
    /// ([`ends_an_answer`]). Zero whenever `stream` is `None`.
    owed_replies: usize,
    /// Since when `stream` has owed answers without a break, for [`OWED_ANSWER_BOUND`].
    owed_since: Option<std::time::Instant>,
    /// Whether the last key line written on `stream` was a key-DOWN: its release is not queued
    /// behind it yet, so the connection must not be given up ([`OWED_ANSWER_BOUND`]).
    owed_key_down: bool,
    /// [`OWED_ANSWER_BOUND`]; a field so a test can shorten it.
    owed_bound: Duration,
    /// The (verb, token) pairs never sent on this link, because the radio's Hamlib driver turns
    /// them into a different command than the radio's own manual defines — see
    /// [`crate::rigmodels::hamlib_never_send`]. Empty by default.
    never_send: &'static [(HamlibVerb, &'static str)],
    #[cfg(test)]
    before_remote_write: Option<Box<dyn FnOnce() + Send>>,
}

/// What a rigctld line asks: read or write a LEVEL, read or write a FUNC. The short and long
/// spellings (`l` and `\get_level`, …) are one verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HamlibVerb {
    GetLevel,
    SetLevel,
    GetFunc,
    SetFunc,
}

impl HamlibVerb {
    /// The verb and the token a command line carries, or `None` for any other line.
    pub fn of_line(line: &str) -> Option<(HamlibVerb, &str)> {
        let mut words = line.split_whitespace();
        let verb = match words.next()? {
            "l" | "\\get_level" => Self::GetLevel,
            "L" | "\\set_level" => Self::SetLevel,
            "u" | "\\get_func" => Self::GetFunc,
            "U" | "\\set_func" => Self::SetFunc,
            _ => return None,
        };
        Some((verb, words.next()?))
    }
}

/// Does this read error mean "nothing was read, try again" rather than "the stream is broken"?
///
/// `WouldBlock`/`TimedOut` are the per-read window expiring. **`Interrupted` is EINTR**: the
/// syscall was cut short by a signal before it waited at all, which says nothing whatever about
/// the peer. Treating it as a hard error surfaced as a spurious CAT failure — and it is not
/// theoretical, it took CI's own `slow_fragmented_reply_still_reads_whole_line` down on a loaded
/// runner (2026-08-21), which is the same thing that would happen to an operator whose machine
/// is busy mid-QSO. `std::io::Read::read` does not retry EINTR for you; `read_exact` does, which
/// is why this only bites the hand-rolled loops.
fn read_should_retry(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}

/// What a command's `TimedOut` error carries when its deadline passed before a whole reply came
/// back, in the error's own words. The line went out, so the radio may be acting on it right now
/// and answer late: no answer in time, which is not a refusal ([`no_answer_in_time`]). A PTT line
/// that rigctld answered with Hamlib's own "the rig did not answer" carries it too ([`ptt_reply`]),
/// marked as Hamlib's answer ([`deadline_passed`] tells the two apart).
#[derive(Debug)]
struct NoAnswerInTime {
    words: String,
    /// rigctld's answer: Hamlib's own "the rig did not answer", not Nexus's deadline passing.
    hamlib: bool,
}

impl std::fmt::Display for NoAnswerInTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

impl std::error::Error for NoAnswerInTime {}

/// Whether `e` is a command's deadline passing with its line sent and no whole reply back, or a PTT
/// line Hamlib could not get the radio to answer (`NoAnswerInTime`), rather than an answer that
/// said no or any other failure. For a key it is the difference between a radio that may be keying
/// and one that is not: a slow radio behind rigctld (a Xiegu, a vintage Kenwood, any rig at 19200
/// baud or less) keys, and answers after `PTT_DEADLINE_MS`, or after Hamlib's own timeout.
pub fn no_answer_in_time(e: &std::io::Error) -> bool {
    e.get_ref()
        .is_some_and(|inner| inner.is::<NoAnswerInTime>())
}

/// Whether `e` is Nexus's own deadline passing with the line sent and no whole reply back: of the
/// two cases [`no_answer_in_time`] counts, the one that is not rigctld's answer. WSJT-X sets no
/// deadline of its own on a key and waits for Hamlib's answer, so a slot over (FT8, FT4, JS8 and
/// the other timed-slot modes) still goes out on this one, while Hamlib's own "the rig did not
/// answer" is a Hamlib error, which WSJT-X halts on (`crate::slot::slot_key_failure`).
pub fn deadline_passed(e: &std::io::Error) -> bool {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<NoAnswerInTime>())
        .is_some_and(|mark| !mark.hamlib)
}

/// What rigctld's answer to a PTT line (`T 1`, `T 3`, `T 0`) says. `RPRT 0`, or an empty reply, is
/// done. Any other code is the rig, or Hamlib, refusing it, except Hamlib's own "the rig did not
/// answer" ([`rprt_is_link_fault`]): rigctld sent the line on and heard nothing back, so the radio
/// may be acting on it, as when the PTT deadline passes. That is no answer in time
/// ([`no_answer_in_time`]), and `TimedOut`, as [`rprt_error`] makes it for every other command;
/// for a key, the radio loop plays the over with its warning. Both keep a PTT error's words.
fn ptt_reply(reply: &str) -> std::io::Result<()> {
    if reply_ok(reply) || reply.is_empty() {
        return Ok(());
    }
    let words = format!("rigctld PTT error: {reply:?}");
    Err(match rprt_code(reply) {
        Some(code) if rprt_is_link_fault(code) => std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            NoAnswerInTime {
                words,
                hamlib: true,
            },
        ),
        _ => std::io::Error::other(words),
    })
}

impl Rig {
    /// General constructor: an optional CAT control channel + a PTT method. This is
    /// the seam that decouples control from keying — pass `Some(addr)` for a CAT rig
    /// regardless of whether `ptt_mode` is `Cat`, `Vox`, or `Serial`.
    pub fn with_control(control: Option<String>, ptt_mode: PttMode) -> Self {
        Self {
            control,
            stream: None,
            ptt_mode,
            #[cfg(feature = "serial")]
            serial: None,
            keyed: false,
            slow_transport: false,
            network_chain: false,
            owed_replies: 0,
            owed_since: None,
            owed_key_down: false,
            owed_bound: OWED_ANSWER_BOUND,
            never_send: &[],
            #[cfg(test)]
            before_remote_write: None,
        }
    }

    /// Mark this CAT link as a slow NETWORK transport (a longer per-command deadline). Call after
    /// constructing a `Rig` for a network-CAT rig (e.g. a Flex reached over TCP) so a legitimately
    /// slow reply isn't cut off; leave default (false) for serial rigs so a stalled read is bounded.
    pub fn set_slow_transport(&mut self, slow: bool) {
        self.slow_transport = slow;
    }
    /// Mark this CAT link as reaching the radio over the network, so a key-down that waits for
    /// the link ([`Self::key_awaiting_answer`]) waits [`NETWORK_KEY_DEADLINE_MS`] for its answer.
    /// Every release keeps [`PTT_DEADLINE_MS`].
    pub fn set_network_chain(&mut self, network: bool) {
        self.network_chain = network;
    }
    /// Name the (verb, token) pairs this link must never send (#381, #385). Everything else is
    /// sent as before, so a power or a switch the radio's driver maps correctly still goes out.
    pub fn set_never_send(&mut self, refused: &'static [(HamlibVerb, &'static str)]) {
        self.never_send = refused;
    }
    /// Why `line` must not be sent on this link, or `None` when it may. Asked where EVERY
    /// single-line command passes ([`Self::command_lines`]) — the heavy poll, the tune level,
    /// the operator's own sets and the Remote checks alike — so no reader or writer can form one
    /// around it.
    fn refused_line(&self, line: &str) -> Option<String> {
        let (verb, token) = HamlibVerb::of_line(line)?;
        self.never_send
            .iter()
            .any(|&(v, t)| v == verb && t == token)
            .then(|| {
                format!(
                    "`{}` is never sent to this radio: its Hamlib driver turns it into a different \
                     command",
                    line.trim()
                )
            })
    }
    /// Change how this rig is keyed WITHOUT touching its (already-open) CAT control channel. Used by
    /// the dual-radio handoff: a monitor rig is opened read-only (`PttMode::Vox`), so when it's adopted
    /// as the ACTIVE radio it must be switched to the profile's real PTT mode (`Cat`/`Serial`) or
    /// `ptt()` would silently no-op (the "TX dead after switching to the FTDX10" bug).
    pub fn set_ptt_mode(&mut self, mode: PttMode) {
        self.ptt_mode = mode;
    }
    /// Close the serial port this rig opened to key [`PttMode::Serial`], if it holds one. For a
    /// rig the dual-radio handoff demotes to a read-only monitor, once it has been unkeyed: a
    /// monitor never keys, and another radio's keying line can live on the same port (an SO2R
    /// controller, RTS for one radio and DTR for the other). Keying again reopens it through
    /// `open_control_line_port`, which idles both lines, exactly as the first open did.
    pub fn release_ptt_port(&mut self) {
        #[cfg(feature = "serial")]
        {
            self.serial = None;
        }
    }
    /// How this rig is currently keyed (for the handoff to verify an adopted rig can key).
    pub fn ptt_mode(&self) -> &PttMode {
        &self.ptt_mode
    }
    /// No CAT control, no keying — rely on the rig's VOX.
    pub fn vox() -> Self {
        Self::with_control(None, PttMode::Vox)
    }
    /// A CAT rig keyed via CAT: control + PTT both over rigctld at `addr`.
    pub fn rigctld(addr: &str) -> Self {
        Self::with_control(Some(addr.to_string()), PttMode::Cat)
    }
    /// Serial-line PTT (RTS/DTR) with NO CAT control. For serial PTT alongside CAT,
    /// use [`with_control`](Self::with_control) and pass a control address.
    pub fn serial(port: &str, line: SerialLine) -> Self {
        Self::with_control(
            None,
            PttMode::Serial {
                port: port.to_string(),
                line,
            },
        )
    }

    fn ensure_connected(&mut self) -> std::io::Result<&mut TcpStream> {
        if self.stream.is_none() {
            self.owed_replies = 0; // owed on a connection that is gone
            self.owed_since = None;
            if let Some(addr) = &self.control {
                let s = connect_cat(addr)?;
                s.set_read_timeout(Some(Duration::from_millis(500)))?;
                s.set_write_timeout(Some(Duration::from_millis(500)))?;
                self.stream = Some(s);
            }
        }
        self.stream
            .as_mut()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "no rig stream"))
    }

    /// Send one command line and read its COMPLETE (newline-terminated) reply.
    ///
    /// On ANY failure — incomplete reply by the deadline, a closed connection,
    /// or a hard I/O error — the stream is dropped so the next command
    /// reconnects from a clean protocol state. This is a TX-safety invariant:
    /// if a slow reply were left in the socket buffer, the *next* command would
    /// read it as its own answer and every command after that would be judged on
    /// the previous one's reply (a keyed rig read as "PTT ok", a rejected T read
    /// as success). A dropped stream can never desync; a stale byte can.
    fn command(&mut self, line: &str) -> std::io::Result<String> {
        self.command_with_deadline(line, None)
    }

    /// Like [`command`] but with an explicit reply deadline (ms), overriding the
    /// transport-aware default. Used for PTT: a keying SET returns a one-line RPRT
    /// and must NOT inherit the slow-transport 2.5 s read window (native CI-V /
    /// networked chains) — a slow-to-answer rig would otherwise hold the un-key on
    /// the single-threaded radio loop for up to ~2 s (the "Tune won't turn off"
    /// report). The transport-aware window stays for the slow READ-BACK polls it
    /// was added for (the K4 CAT-stability work).
    fn command_with_deadline(
        &mut self,
        line: &str,
        deadline_ms: Option<u64>,
    ) -> std::io::Result<String> {
        self.command_permitted(line, deadline_ms, None)
    }

    /// Like [`command`] for a verb whose reply the rigctld protocol defines as MORE THAN ONE
    /// line — today `m` (mode + passband) and `s` (split flag + VFO).
    ///
    /// ⚠️ **This exists because the drain above cannot do the job alone (#CI flake, proven
    /// 2026-09-21).** [`command_inner`] returns at the first newline, so when the two lines of
    /// an `m` reply arrive in separate reads — the networked-chain case the drain's own comment
    /// describes — the second line stays in the socket. The pre-command drain is NON-BLOCKING,
    /// so it discards only what has ALREADY arrived: a straggler still in flight survives it and
    /// is read as the NEXT command's answer. Measured: `l RFPOWER` issued right after a split
    /// `m` returned the passband width, `raw=Some(2400.0)`, which the 0..=1 power range rejects
    /// as `ReadingUnavailable`.
    ///
    /// Reading the reply the protocol's own length fixes it at the source, so nothing is left to
    /// drain. The drain stays as the backstop it was always meant to be.
    fn command_expecting(&mut self, line: &str, expected_lines: usize) -> std::io::Result<String> {
        self.command_lines(line, None, None, expected_lines, false, false)
    }

    fn command_permitted(
        &mut self,
        line: &str,
        deadline_ms: Option<u64>,
        permission: Option<&WritePermission>,
    ) -> std::io::Result<String> {
        self.command_lines(line, deadline_ms, permission, 1, false, false)
    }

    /// [`Self::command_with_deadline`] for a read that must END by its deadline: each socket read
    /// waits only for what is left of it. Elsewhere the deadline is looked at between reads, each
    /// of which may wait the whole 500 ms read window, so a deadline shorter than the window could
    /// be overrun by up to the window. The tune-time meter reads need the strict form: a read that
    /// outlasts the tune carrier's lead gaps the carrier. PTT keeps its own deadline as it was.
    fn command_ending_by(&mut self, line: &str, deadline_ms: u64) -> std::io::Result<String> {
        self.command_lines(line, Some(deadline_ms), None, 1, true, false)
    }

    /// ⛔ A LATE KEY KEEPS ITS CONNECTION (`ordered`). Every other failure drops the connection,
    /// as [`Self::command`] says. A key line (`T …`) whose deadline passes is still live inside
    /// rigctld, which can hold it for seconds (a mode write ahead of it waiting out Hamlib's
    /// timeout under rigctld's one client lock). Dropping the connection then sent the release on
    /// a NEW connection, and rigctld's lock does not serve its connections in arrival order, so
    /// the release could be carried out BEFORE the key and leave the radio keyed after the
    /// operator let go. So the connection stays, and its answer is counted as owed
    /// (`owed_replies`): the next key line is written behind it at once and reads the owed
    /// answers before its own (rigctld serves one connection strictly in order, so the release
    /// always follows the key), and any other command first waits for them within its own
    /// deadline and, until they are in, sends nothing and fails as no answer in time. The
    /// desync this module guards against stays impossible: an owed answer is counted, never
    /// read as another command's. Only a hard error (the connection closed or broken) drops it,
    /// or answers still owed [`OWED_ANSWER_BOUND`] after a release went out behind the key.
    ///
    /// Ordered, and so written behind owed answers at once, are the key lines and the stops a
    /// Stop TX sends with the unkey (`\stop_morse`, a radio's own CW playback stop): a stop must
    /// never wait out an owed answer and then not be sent. Each answers with one terminator.
    fn command_lines(
        &mut self,
        line: &str,
        deadline_ms: Option<u64>,
        permission: Option<&WritePermission>,
        expected_lines: usize,
        end_by_deadline: bool,
        ordered: bool,
    ) -> std::io::Result<String> {
        // Before the stream is touched: a refused line never reached the wire, so it is neither a
        // link failure nor a reason to drop the connection below.
        if let Some(why) = self.refused_line(line) {
            return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, why));
        }
        self.give_up_a_stale_debt();
        if !ordered {
            let window = deadline_ms.unwrap_or(self.transport_deadline_ms());
            self.settle_owed(window)?;
        }
        // Only a key line moves `owed_key_down`: a stop keys nothing.
        let key_down = match line.trim() {
            "T 0" => Some(false),
            l if l.starts_with("T ") => Some(true),
            _ => None,
        };
        let mut owed = std::mem::take(&mut self.owed_replies);
        let owed_before = owed;
        match self.command_inner(
            line,
            deadline_ms,
            permission,
            expected_lines,
            end_by_deadline,
            &mut owed,
        ) {
            Ok(reply) => {
                self.owed_since = None;
                Ok(reply)
            }
            Err(e) if ordered && deadline_passed(&e) && self.stream.is_some() => {
                if crate::civ::diag::is_enabled() {
                    crate::civ::diag::note(&format!(
                        "Rig: rigctld cmd {:?} not answered in time ({e}) → connection kept, {} answer(s) owed (keyed={})",
                        line.trim(),
                        owed + 1,
                        self.keyed
                    ));
                }
                // Owing without a break since the earlier miss, if one was still owed when this
                // line went out.
                if self.owed_since.is_none() || owed_before == 0 {
                    self.owed_since = Some(std::time::Instant::now());
                }
                if let Some(down) = key_down {
                    self.owed_key_down = down;
                }
                self.owed_replies = owed + 1;
                Err(e)
            }
            Err(e) => {
                // Diagnostic: dropping the rigctld connection is what triggers the daemon's
                // disconnect fail-safe unkey. Log WHICH command failed and why (keyed = mid-TX,
                // where the drop steals our own transmit) so the flicker's true trigger is named.
                if crate::civ::diag::is_enabled() {
                    crate::civ::diag::note(&format!(
                        "Rig: rigctld cmd {:?} FAILED ({e}) → dropping connection (keyed={})",
                        line.trim(),
                        self.keyed
                    ));
                }
                self.stream = None; // force a clean reconnect on the next call
                self.owed_replies = 0;
                self.owed_since = None;
                Err(e)
            }
        }
    }

    /// Give up a connection that has owed answers for [`OWED_ANSWER_BOUND`] with a release queued
    /// behind its last key ([`OWED_ANSWER_BOUND`] says why that is safe and when it is needed).
    /// The next command opens a new connection; `keyed` is left as it is, so it is cleared only by
    /// a release that connection confirms.
    fn give_up_a_stale_debt(&mut self) {
        let stale = self
            .owed_since
            .is_some_and(|since| since.elapsed() >= self.owed_bound);
        if self.owed_replies == 0 || self.owed_key_down || !stale {
            return;
        }
        if crate::civ::diag::is_enabled() {
            crate::civ::diag::note(&format!(
                "Rig: {} answer(s) still owed after {:?} with the release queued → connection given up (keyed={})",
                self.owed_replies, self.owed_bound, self.keyed
            ));
        }
        // Read what has arrived first, so the close is an orderly one and not a reset that could
        // cost rigctld the lines still queued on it.
        if let Some(stream) = self.stream.as_mut() {
            let mut scratch = [0u8; 256];
            if stream.set_nonblocking(true).is_ok() {
                while matches!(stream.read(&mut scratch), Ok(n) if n > 0) {}
            }
        }
        self.stream = None;
        self.owed_replies = 0;
        self.owed_since = None;
    }

    /// A command's reply deadline (ms) when it names none: the long window on a slow transport.
    fn transport_deadline_ms(&self) -> u64 {
        if self.slow_transport {
            2_500
        } else {
            700
        }
    }

    /// Read the answers still owed by late key lines ([`Self::command_lines`]), writing nothing,
    /// for at most `deadline_ms`. Still owed by then: no answer in time, the connection kept. A
    /// closed or broken connection is dropped, and with it what it owed.
    fn settle_owed(&mut self, deadline_ms: u64) -> std::io::Result<()> {
        if self.owed_replies == 0 {
            return Ok(());
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(deadline_ms);
        let mut buf = [0u8; 256];
        while self.owed_replies > 0 {
            let Some(stream) = self.stream.as_mut() else {
                self.owed_replies = 0;
                self.owed_since = None;
                return Ok(());
            };
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    NoAnswerInTime {
                        words: format!(
                            "rig still owes the answer to an earlier key after {deadline_ms} ms; nothing sent"
                        ),
                        hamlib: false,
                    },
                ));
            }
            let read = stream
                .set_read_timeout(Some(left.min(Duration::from_millis(500))))
                .and_then(|()| stream.read(&mut buf));
            let failed = match read {
                Ok(0) => std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "rigctld closed the connection",
                ),
                // Nothing but owed answers is in flight on this connection, so every terminator
                // read here ends one of them.
                Ok(n) => {
                    let answers = buf[..n].iter().filter(|&&b| ends_an_answer(b)).count();
                    self.owed_replies = self.owed_replies.saturating_sub(answers);
                    if self.owed_replies == 0 {
                        self.owed_since = None;
                    }
                    continue;
                }
                Err(e) if read_should_retry(e.kind()) => continue,
                Err(e) => e,
            };
            self.stream = None;
            self.owed_replies = 0;
            self.owed_since = None;
            return Err(failed);
        }
        Ok(())
    }

    fn command_inner(
        &mut self,
        line: &str,
        deadline_override: Option<u64>,
        permission: Option<&WritePermission>,
        expected_lines: usize,
        end_by_deadline: bool,
        owed: &mut usize,
    ) -> std::io::Result<String> {
        if let Some(permission) = permission {
            permission
                .check(std::time::Instant::now())
                .map_err(remote_permission_error)?;
        }
        // Read the transport class before the mutable stream borrow below (they'd otherwise alias).
        let fallback_deadline_ms = self.transport_deadline_ms();
        #[cfg(test)]
        let before_write = if permission.is_some() {
            self.before_remote_write.take()
        } else {
            None
        };
        if self.stream.is_none() {
            *owed = 0; // a connection opened now owes nothing
        }
        let stream = self.ensure_connected()?;
        // Discard any STALE bytes left in the socket by a prior MULTI-LINE reply — `m` (get_mode)
        // returns the mode line AND a passband line, and on a networked chain the 2nd line can
        // arrive AFTER we already returned the 1st. A lingering byte would be read as THIS
        // command's answer and desync every command after it (the exact hazard the drop-on-failure
        // guards, but a successful multi-line reply slips past that). Non-blocking → free when clean.
        // Not while late keys' answers are owed: those bytes are counted below, never discarded.
        if *owed == 0 {
            stream.set_nonblocking(true)?;
            let mut scratch = [0u8; 256];
            while let Ok(n) = stream.read(&mut scratch) {
                if n == 0 {
                    break; // peer closed — the real read below surfaces it
                }
            }
            stream.set_nonblocking(false)?;
        }
        stream.set_read_timeout(Some(Duration::from_millis(500)))?; // restore blocking-with-timeout
        #[cfg(test)]
        if let Some(before_write) = before_write {
            before_write();
        }
        // Connecting and draining a stale response may have outlived the
        // operation. Admission before that I/O is insufficient: check again at
        // the actual write. Local CAT/PTT calls carry no Remote permission.
        if let Some(permission) = permission {
            permission
                .begin_write(std::time::Instant::now())
                .map_err(remote_permission_error)?;
        }
        stream.write_all(line.as_bytes())?;
        // Read until a COMPLETE reply (newline-terminated), not one 500 ms
        // gulp: a networked chain (rigctld → SmartSDR CAT → radio) can take
        // longer than one read window and can split a reply across reads.
        // The per-read timeout (500 ms) bounds each wait; an overall deadline
        // bounds the whole reply. An incomplete reply is an ERROR (not a
        // silently-truncated "") so callers never treat a partial or timed-out
        // answer as success — and `command` drops the stream. The deadline is
        // transport-aware: a serial rig answers in ms so a stall is bounded
        // tightly (it can't hold the radio loop / fast dial poll), while a
        // network chain keeps the long 2.5 s window for legitimately slow replies.
        let deadline_ms = deadline_override.unwrap_or(fallback_deadline_ms);
        let deadline = std::time::Instant::now() + Duration::from_millis(deadline_ms);
        // For a multi-line verb: once the FIRST line is in but the rest is not, how long the
        // straggler gets. Short because it is measuring one more segment on a link that has
        // already answered, not the rig's thinking time — and because the fallback below is
        // exactly the old behaviour, so this bounds the cost of a daemon that answers a
        // multi-line verb with fewer lines than the protocol defines (none known: Hamlib's
        // rigctld and both of Nexus's own impersonators send both lines).
        const MULTILINE_TAIL_MS: u64 = 100;
        let mut tail_deadline: Option<std::time::Instant> = None;
        let mut out = Vec::with_capacity(64);
        let mut buf = [0u8; 256];
        let too_late = |out: &[u8]| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                NoAnswerInTime {
                    words: format!(
                        "rig reply incomplete after {deadline_ms} ms (got {:?})",
                        String::from_utf8_lossy(out)
                    ),
                    hamlib: false,
                },
            )
        };
        let partial = loop {
            // A read that must end by its deadline waits only for what is left of it.
            if end_by_deadline {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Err(too_late(&out));
                }
                let window = if tail_deadline.is_some() {
                    MULTILINE_TAIL_MS
                } else {
                    500
                };
                stream.set_read_timeout(Some(left.min(Duration::from_millis(window))))?;
            }
            match stream.read(&mut buf) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "rigctld closed the connection",
                    ));
                }
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    // The owed answers of late keys come first (rigctld answers one connection in
                    // order): each whole answer of theirs is set aside, never taken for this one.
                    while *owed > 0 {
                        let Some(end) = out.iter().position(|&b| ends_an_answer(b)) else {
                            break;
                        };
                        out.drain(..=end);
                        *owed -= 1;
                    }
                    // Short of the expected line count: give the rest a brief window, and
                    // shorten the per-read wait to match so the window can actually expire.
                    if expected_lines > 1
                        && tail_deadline.is_none()
                        && out.ends_with(b"\n")
                        && !reply_complete(&out, expected_lines)
                    {
                        tail_deadline = Some(
                            std::time::Instant::now() + Duration::from_millis(MULTILINE_TAIL_MS),
                        );
                        stream.set_read_timeout(Some(Duration::from_millis(MULTILINE_TAIL_MS)))?;
                    }
                    // TWO TERMINATORS, and only one of them is a newline. rigctld's own answers end
                    // in `\n` (`f` → "14074000\n"), but `w` (send_cmd) hands back the RIG's string
                    // terminated by a NUL and no newline at all — measured against Hamlib 4.7.0 on
                    // an FT-710: `w MD0;` → "MD02;\0", `w SS05;` → "SS0570000;\0". Waiting for a
                    // newline there burns the whole deadline, errors, and DROPS THE CONNECTION, so
                    // every raw-CAT read failed silently and forced a reconnect. Accepting either
                    // terminator is version-agnostic: a daemon that does append a newline still
                    // matches the first arm, and no ordinary reply contains a NUL.
                    if out.ends_with(b"\0") || reply_complete(&out, expected_lines) {
                        return Ok(String::from_utf8_lossy(&out).to_string());
                    }
                }
                Err(ref e) if read_should_retry(e.kind()) => {} // nothing read — wait out the deadline
                Err(e) => return Err(e), // hard error — caller drops the stream
            }
            // The straggler never came. Return the lines that DID arrive: that is precisely what
            // this function did before it counted lines at all, so a short multi-line reply can
            // never become an error it was not already.
            if tail_deadline.is_some_and(|t| std::time::Instant::now() >= t) {
                break String::from_utf8_lossy(&out).to_string();
            }
            if std::time::Instant::now() >= deadline {
                return Err(too_late(&out));
            }
        };
        // ⚠️ THE REPLY IS SALVAGEABLE; THE CONNECTION IS NOT. The line we stopped waiting for is
        // still in flight, and the pre-command drain above is NON-BLOCKING — it discards only
        // what has already arrived, so it catches the straggler by luck, not by guarantee. Left
        // alone it becomes the NEXT command's answer, which is the desync this function's own
        // header calls a TX-safety invariant ("a keyed rig read as PTT ok"): a `m` width of `0`,
        // rigctld's answer for the rig's default filter, is byte-identical to `t`'s answer for
        // NOT KEYED. So take the `Ok` path's reply and the `Err` path's cleanup — one reconnect,
        // on a reply that was already anomalous. See
        // `a_straggler_past_the_tail_window_never_answers_the_next_command`.
        self.stream = None;
        Ok(partial)
    }

    /// Send a command whose reply is MANY lines with no length or terminator we can predict
    /// (`\dump_state`), reading until the peer goes quiet for one read window.
    ///
    /// Kept separate from [`Rig::command`] on purpose: that one returns at the first newline,
    /// which for a multi-line reply would leave the rest in the socket to be misread as the NEXT
    /// command's answer — the exact desync the drop-on-failure guard exists for. This drains to
    /// quiet and, like `command`, drops the stream on any failure so nothing is left behind.
    fn command_multiline(&mut self, line: &str) -> std::io::Result<String> {
        // A late key's owed answer is read first, as for every other command (`command_lines`).
        self.settle_owed(2_000)?;
        match self.command_multiline_inner(line) {
            Ok(reply) => Ok(reply),
            Err(e) => {
                self.stream = None; // force a clean reconnect on the next call
                Err(e)
            }
        }
    }

    fn command_multiline_inner(&mut self, line: &str) -> std::io::Result<String> {
        let stream = self.ensure_connected()?;
        stream.set_nonblocking(false)?;
        // A short per-read window is the "peer went quiet" signal; the overall deadline bounds a
        // chatty or stalled daemon. Both are generous enough for a local rigctld's ~40-line dump.
        stream.set_read_timeout(Some(Duration::from_millis(300)))?;
        stream.write_all(line.as_bytes())?;
        let deadline = std::time::Instant::now() + Duration::from_millis(2_000);
        let mut out = Vec::with_capacity(2048);
        let mut buf = [0u8; 1024];
        let mut still_talking = false;
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break, // peer closed — parse what we have
                Ok(n) => out.extend_from_slice(&buf[..n]),
                // A per-read window that expired with nothing in it means the peer has gone
                // quiet, so the reply is complete. EINTR is NOT that: the syscall was cut
                // short by a signal without waiting, so breaking here would truncate a reply
                // that is still arriving. Retry it instead; the deadline below still bounds us.
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(ref e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break; // quiet for a full window: the reply is complete
                }
                Err(e) => return Err(e),
            }
            if std::time::Instant::now() >= deadline {
                still_talking = true;
                break;
            }
        }
        if out.is_empty() {
            return Err(std::io::Error::other("no reply to a multi-line command"));
        }
        // ⚠️ The SAME hazard as the multi-line tail window in `command_inner`, and the same
        // answer. The quiet-window break above means the daemon has finished; THIS break means
        // it is still mid-dump when our patience ran out, so the rest of the dump lands in the
        // socket and becomes the next command's answer — the desync `command`'s header calls a
        // TX-safety invariant. Keep what we parsed, drop the connection.
        if still_talking {
            self.stream = None;
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    /// Key (true) or unkey (false) the transmitter. No-op under VOX (and under CAT
    /// keying when no control channel is configured — degrades to VOX).
    ///
    /// FAIL-SAFE `keyed` semantics (the stuck-TX-light root cause): a key-down ATTEMPT
    /// marks the rig keyed immediately (even if the command then fails, the radio may
    /// have keyed), but an unkey clears the flag ONLY when the command succeeded. A
    /// failed unkey therefore leaves `keyed == true`, so every unkey-first teardown
    /// gate keeps firing and the idle self-heal in the radio loop retries until the
    /// radio actually releases — one transient CAT failure can no longer latch PTT
    /// until the radio is rebooted.
    ///
    /// This is the key for a transmission whose audio Nexus plays, so on a Rear/Data radio
    /// ([`PttMode::CatData`]) its key-down is `T 3`, the DATA input. [`Self::ptt_plain`] is the
    /// key for one whose audio it does not play.
    #[track_caller]
    pub fn ptt(&mut self, on: bool) -> std::io::Result<()> {
        self.key(on, true, false)
    }

    /// [`Self::ptt`] for a transmission whose audio Nexus does NOT play: the operator's own voice
    /// at the radio's microphone (the Phone cockpit's PTT, and the CAT broker's key, which rides
    /// it) and an FSK keyline, whose tones are the keyed line rather than audio. It keys exactly as
    /// `ptt` does except on a Rear/Data radio ([`PttMode::CatData`]), where `ptt` keys the DATA
    /// input and this keys with `T 1`, as every release has: the Rear/Data choice (#381) is about
    /// where Nexus's own audio enters the radio, so it leaves these as they were. Release: `T 0`.
    #[track_caller]
    pub fn ptt_plain(&mut self, on: bool) -> std::io::Result<()> {
        self.key(on, false, false)
    }

    /// A key-down ([`Self::ptt`], or with `plain` [`Self::ptt_plain`]) that waits for the radio's
    /// answer as long as the link can take to give it: on a network chain
    /// ([`Self::set_network_chain`]) [`NETWORK_KEY_DEADLINE_MS`], elsewhere the PTT deadline. For
    /// the radio loop's own overs and the operator's PTT, whose warning, and for the tune whether
    /// anything plays at all, turn on the key being confirmed. The slot overs (FT8, FT4, JS8 and
    /// the rest) key with `ptt`, whose deadline is part of their timing.
    #[track_caller]
    pub fn key_awaiting_answer(&mut self, plain: bool) -> std::io::Result<()> {
        self.key(true, !plain, true)
    }

    #[track_caller]
    fn key(&mut self, on: bool, nexus_audio: bool, link_window: bool) -> std::io::Result<()> {
        // Diagnostic: name the exact caller of every PTT change. A CI-V capture that shows an
        // unexplained unkey mid-TX can then be traced to the precise line that dropped it —
        // this is how the IC-9700 flicker's true source gets pinned instead of inferred. Cheap
        // when the diagnostic log is off (a single atomic load; the format! is skipped).
        if crate::civ::diag::is_enabled() {
            crate::civ::diag::note(&format!(
                "Rig::ptt({on}) called from {}",
                std::panic::Location::caller()
            ));
        }
        // The DEBUG tier's keying trace. Guarded on the tier being ON before the format!, since
        // this runs twice per over — the atomic load is the cheap part, the string is not.
        if tempo_core::applog::debug_enabled() {
            tempo_core::applog::debug(
                "ptt",
                &format!(
                    "{} via {}",
                    if on { "KEY" } else { "unkey" },
                    match &self.ptt_mode {
                        PttMode::Vox => "VOX".to_string(),
                        PttMode::Serial { .. } => "serial line".to_string(),
                        PttMode::Cat => "CAT".to_string(),
                        PttMode::CatData if nexus_audio => "CAT (data)".to_string(),
                        PttMode::CatData => "CAT".to_string(),
                    }
                ),
            );
        }
        if on {
            self.keyed = true;
        }
        let result = match &self.ptt_mode {
            PttMode::Vox => Ok(()),
            PttMode::Serial { .. } => self.serial_ptt(on),
            PttMode::Cat | PttMode::CatData => {
                if self.control.is_none() {
                    Ok(()) // CAT keying chosen but no CAT channel → VOX fallback
                } else {
                    // The one difference Rear/Data makes: the KEY-DOWN of a transmission whose
                    // audio Nexus plays goes to the DATA input. Every release is the same `T 0`.
                    let line = if on && nexus_audio && self.ptt_mode == PttMode::CatData {
                        ptt_data_line()
                    } else {
                        ptt_line(on)
                    };
                    // PTT is time-critical: a fixed 700 ms deadline (not the slow-transport
                    // 2.5 s read window) so the un-key can't hang the radio loop. Missing it is
                    // not a refusal ([`no_answer_in_time`]): a slow radio keys and answers late.
                    // Nor is Hamlib's own "the rig did not answer" ([`ptt_reply`]). A key-down
                    // asked to wait for the link ([`Self::key_awaiting_answer`]) waits a network
                    // chain's window; a release never does. A missed deadline keeps the
                    // connection, so the release goes out behind the key
                    // ([`Self::command_lines`]).
                    let deadline = if on && link_window && self.network_chain {
                        NETWORK_KEY_DEADLINE_MS
                    } else {
                        PTT_DEADLINE_MS
                    };
                    self.command_lines(&line, Some(deadline), None, 1, false, true)
                        .and_then(|reply| ptt_reply(&reply))
                }
            }
        };
        if !on && result.is_ok() {
            self.keyed = false;
        }
        result
    }

    /// Assert/deassert the configured serial PTT line.
    ///
    /// With the `serial` feature this lazily opens the port and drives RTS/DTR;
    /// without it, keying is logged and treated as a no-op so the engine can
    /// still run (effectively VOX) on a build with no serial backend.
    ///
    /// Only the KEYED line is driven from here. The other one is deasserted once, at open, by
    /// [`crate::control_line::idle_both_lines`] — without that this function was the third
    /// instance of the stuck-PTT defect its own headline names: on a conventional interface
    /// (DTR = key, RTS = PTT) opening the port raised both, and nothing here ever lowered the
    /// one we do not key.
    #[cfg(feature = "serial")]
    fn serial_ptt(&mut self, on: bool) -> std::io::Result<()> {
        let (port, line) = match &self.ptt_mode {
            PttMode::Serial { port, line } => (port.clone(), *line),
            _ => return Ok(()),
        };
        if self.serial.is_none() {
            // We only toggle control lines, never data, so the rate is meaningless —
            // but some rigs refuse a given rate at open, so let `control_line` find one
            // that works instead of hardcoding a rate the port may reject.
            self.serial = Some(crate::control_line::open_control_line_port(&port)?);
        }
        let sp = self.serial.as_mut().unwrap();
        match line {
            SerialLine::Rts => sp.write_request_to_send(on)?,
            SerialLine::Dtr => sp.write_data_terminal_ready(on)?,
        }
        Ok(())
    }

    /// Serial PTT no-op fallback when built without the `serial` feature.
    #[cfg(not(feature = "serial"))]
    fn serial_ptt(&mut self, on: bool) -> std::io::Result<()> {
        if let PttMode::Serial { port, line } = &self.ptt_mode {
            eprintln!(
                "tempo-audio: serial PTT requested ({line:?} on {port}, key={on}) but the \
                 `serial` feature is not enabled — treating as VOX (no-op)."
            );
        }
        Ok(())
    }

    /// Set the dial frequency (Hz). No-op unless a CAT control channel is configured.
    ///
    /// Surfaces a rig REJECTION (`RPRT <negative>` — e.g. a frequency outside the radio's range,
    /// the HF-only-rig-asked-for-2 m report) as an `Err`, exactly like [`Rig::set_mode`]. This
    /// used to `map(|_| ())` the reply away, which made a refusal indistinguishable from success:
    /// the caller advanced its `last_dial` to a frequency the radio was never on, suppressed the
    /// read-back that would have corrected it, and reported "CAT confirmed — rig accepted a
    /// command" off the back of a refusal. A command's outcome is in its reply; throwing the
    /// reply away is throwing the outcome away.
    pub fn set_freq(&mut self, hz: u64) -> std::io::Result<()> {
        self.set_freq_permitted(hz, None)
    }

    /// One Remote CAT frequency write. An acknowledgement is not readback;
    /// the owning radio worker must obtain a later measurement to complete it.
    pub fn remote_set_freq(
        &mut self,
        hz: u64,
        permission: &WritePermission,
    ) -> std::io::Result<()> {
        self.set_freq_permitted(hz, Some(permission))
    }

    fn set_freq_permitted(
        &mut self,
        hz: u64,
        permission: Option<&WritePermission>,
    ) -> std::io::Result<()> {
        if self.control.is_none() {
            return if permission.is_some() {
                Err(std::io::ErrorKind::NotConnected.into())
            } else {
                Ok(())
            };
        }
        let reply = self.command_permitted(&freq_line(hz), None, permission)?;
        if reply_ok(&reply) || (permission.is_none() && reply.is_empty()) {
            Ok(())
        } else {
            Err(rprt_error("freq", &reply))
        }
    }

    /// The radio's RECEIVE frequency ranges (Hz, inclusive), straight from Hamlib's backend
    /// capability table via rigctld's `\dump_state`.
    ///
    /// This is how Nexus can know a radio cannot reach 2 m BEFORE commanding it there. RX (not TX)
    /// ranges are the right list: monitoring APRS needs only receive, and RX coverage is a superset
    /// of TX coverage on every rig, so a frequency outside the RX list is unreachable either way.
    ///
    /// `None` on any doubt — no CAT, an unparseable or unexpected reply, an empty list, or a
    /// protocol version we don't recognise. **Unknown must always FAIL OPEN** at the call site
    /// (command it and let the rig answer): a capability probe that guessed "not covered" would
    /// block legitimate QSYs, which is far worse than the refusal it was trying to avoid.
    pub fn read_rx_ranges(&mut self) -> Option<Vec<(u64, u64)>> {
        self.control.as_ref()?;
        // `\dump_state` is a long, multi-line reply; the normal per-command read stops at the
        // first newline. Read to the deadline instead and parse whatever arrived.
        let reply = self.command_multiline("\\dump_state\n").ok()?;
        parse_dump_state_rx_ranges(&reply)
    }

    /// Ask the rig ONCE whether its split can be read without disturbing it.
    ///
    /// Uses `\dump_caps` — the PROSE capability dump, which is the only one carrying the split
    /// flags (`\dump_state`, the machine-readable one, does not). Verified against Hamlib
    /// 4.7.1 `tests/rigctl_parse.c`, where `dump_caps` is a real protocol command
    /// (`{ '1', "dump_caps", … }`).
    ///
    /// Cache the answer per connection: it is a long reply and it cannot change while the rig
    /// is the same rig. `None` = we could not ask, which the caller must treat as
    /// [`SplitDetect::Absent`] — silence is not permission.
    pub fn read_split_capability(&mut self) -> Option<crate::baud_ladder::SplitDetect> {
        self.control.as_ref()?;
        let reply = self.command_multiline("\\dump_caps\n").ok()?;
        Some(crate::baud_ladder::parse_caps(&reply).split_detect)
    }

    /// Set the operating mode (e.g. "USB") + passband. A BLANK mode is a no-op —
    /// the caller is choosing to OBEY the radio's current mode (max compatibility),
    /// so Nexus sends no `M` command. Also a no-op unless a CAT control channel is
    /// configured (works even when PTT is keyed by VOX/serial — control is separate).
    /// Surfaces a rig REJECTION (`RPRT -1`, e.g. a rig with no DATA/PKT submode) as
    /// an `Err`, so the radio loop's bounded retry can give up instead of looping.
    pub fn set_mode(&mut self, mode: &str, passband_hz: i32) -> std::io::Result<()> {
        self.set_mode_permitted(mode, passband_hz, None)
    }

    /// One Remote CAT mode write, with the same native codec and reply parser.
    pub fn remote_set_mode(
        &mut self,
        mode: &str,
        passband_hz: i32,
        permission: &WritePermission,
    ) -> std::io::Result<()> {
        if mode.is_empty()
            || !mode
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        self.set_mode_permitted(mode, passband_hz, Some(permission))
    }

    fn set_mode_permitted(
        &mut self,
        mode: &str,
        passband_hz: i32,
        permission: Option<&WritePermission>,
    ) -> std::io::Result<()> {
        if mode.trim().is_empty() {
            return if permission.is_some() {
                Err(std::io::ErrorKind::InvalidInput.into())
            } else {
                Ok(())
            };
        }
        if self.control.is_none() {
            return if permission.is_some() {
                Err(std::io::ErrorKind::NotConnected.into())
            } else {
                Ok(())
            };
        }
        let reply = self.command_permitted(&mode_line(mode, passband_hz), None, permission)?;
        if reply_ok(&reply) || (permission.is_none() && reply.is_empty()) {
            Ok(())
        } else {
            Err(rprt_error("mode", &reply))
        }
    }

    /// Apply FM repeater settings: shift direction (`R`), offset magnitude (`O`), and
    /// CTCSS tone (`C`). Every command is still ATTEMPTED whatever the one before it
    /// answered — a rig that supports shift but not CTCSS must still get its shift — but
    /// the outcome is now REPORTED: `true` only when every command sent was accepted.
    /// No-op (and `true`) without a CAT control channel; `tone_hz` 0 disables CTCSS.
    /// Call after a successful FM `set_mode` (the connection is live).
    ///
    /// ⚠️ IT USED TO SWALLOW ALL THREE RESULTS AND ALWAYS ANSWER `Ok(())`, which is what
    /// made #319 ("TONE stopped working on the VHF memories, a restart cleared it")
    /// possible: the radio loop records the config as applied the instant it is sent, so a
    /// tone the rig refused — or never answered — was booked as delivered and never sent
    /// again for the rest of the session. A restart was the only thing that cleared the
    /// belief, which is exactly what the reporter found. The caller needs to be able to
    /// tell "the machine's settings are current" from "we said the words".
    pub fn set_fm_repeater(&mut self, shift: &str, offset_hz: i64, tone_hz: f32) -> bool {
        if self.control.is_none() {
            return true;
        }
        // Bound to locals FIRST so every one is sent: a `&&` chain would stop at the first
        // refusal and leave the tone unsaid on a rig that merely has no repeater shift.
        let shift_ok = self.cat(&rptr_shift_line(shift)).is_ok();
        let offset_ok = offset_hz <= 0 || self.cat(&rptr_offset_line(offset_hz)).is_ok();
        let tone_ok = self.cat(&ctcss_line(tone_hz)).is_ok();
        shift_ok && offset_ok && tone_ok
    }

    /// Whether a CAT control channel is configured (so the freq/mode/CAT verbs are
    /// live). Lets callers distinguish "no CAT" from "CAT present but the rig is mute".
    pub fn has_control(&self) -> bool {
        self.control.is_some()
    }

    /// The CAT control channel's `host:port`, `None` without one.
    pub fn control_addr(&self) -> Option<&str> {
        self.control.as_deref()
    }

    /// Probe the rig by reading its current dial frequency (Hz) over CAT — the
    /// basis of a WSJT-X-style "Test CAT". Connects to rigctld and sends `f`,
    /// which replies with the frequency on its own line. Returns a descriptive
    /// error when rigctld is unreachable (connection refused) or the rig itself
    /// doesn't answer (bad baud / serial port / CAT disabled → no numeric reply).
    /// Only valid with a CAT control channel.
    /// Read a rig LEVEL (e.g. "RFPOWER" → 0.0–1.0) via rigctld `l NAME`.
    /// CAT-only; errors on FakeIt/none like `read_freq`.
    ///
    /// ⚠️ A level this link must never read errors without a byte on the wire — see
    /// [`Self::set_never_send`].
    pub fn read_level(&mut self, name: &str) -> std::io::Result<f32> {
        if self.control.is_none() {
            return Err(std::io::Error::other("not a CAT rig"));
        }
        let reply = self.command(&format!("l {name}\n"))?;
        reply
            .lines()
            .find_map(|l| l.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            .ok_or_else(|| std::io::Error::other("no level in reply"))
    }

    /// Read the rig's S-meter (dB relative to S9) via rigctld `l STRENGTH`. CAT-only;
    /// `None` on VOX/serial or no numeric reply. Unlike [`Rig::read_level`], STRENGTH is a
    /// signed dB value, not a 0.0–1.0 fraction (parsing/bounds live in [`parse_smeter_db`]).
    pub fn read_smeter_db(&mut self) -> Option<i32> {
        self.control.as_ref()?;
        let reply = self.command("l STRENGTH\n").ok()?;
        parse_smeter_db(&reply)
    }

    /// Read the rig's OWN PTT state via rigctld `t` — is the transmitter keyed, by anyone?
    /// This is how radio-side keying (mic PTT, a straight key) becomes visible at all: every
    /// other TX indication in the app reports what NEXUS keyed (#57 — an FTdx10 keyed from
    /// the mic showed RX and no meters, because nothing ever asked the rig). CAT-only;
    /// `None` on VOX/serial, a link hiccup, or a non-numeric reply — the caller keeps the
    /// last known state rather than flapping the indicator on one lost poll. Read-only:
    /// this path never writes `T`, so it cannot key or unkey anything.
    pub fn read_ptt(&mut self) -> Option<bool> {
        self.control.as_ref()?;
        let reply = self.command("t\n").ok()?;
        reply
            .lines()
            .find_map(|l| l.trim().parse::<u8>().ok())
            .map(|v| v != 0)
    }

    /// Read a transmit METER as a raw float via rigctld `l NAME` — unlike [`Rig::read_level`]
    /// this does NOT clamp to 0..1, so it works for SWR (1.0–6.0), Po watts, and COMP dB.
    /// CAT-only; `None` on VOX/serial or no finite numeric reply (e.g. the rig ignores the
    /// read while receiving). Used for SWR/ALC/RFPOWER_METER/COMP_METER.
    pub fn read_meter_f32(&mut self, name: &str) -> Option<f32> {
        self.read_meter_f32_within(name, None)
    }

    /// [`read_meter_f32`] with a caller-chosen deadline (ms). The tune-time meter poll uses a
    /// short one: a read that outlasts the tune carrier's audio lead would gap the carrier, so
    /// it is abandoned (the stream drops and reconnects on the next command) rather than
    /// waited for. `None` = the transport's normal deadline.
    pub fn read_meter_f32_within(&mut self, name: &str, deadline_ms: Option<u64>) -> Option<f32> {
        self.control.as_ref()?;
        let line = format!("l {name}\n");
        let reply = match deadline_ms {
            Some(ms) => self.command_ending_by(&line, ms),
            None => self.command(&line),
        }
        .ok()?;
        reply
            .lines()
            .find_map(|l| l.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
    }

    /// Read a rig CAT function state (e.g. "NB", "NR", "ANF", "COMP", "VOX") via rigctld
    /// `u FUNC`. CAT-only; `None` on VOX/serial, an unsupported func, or a link hiccup — the
    /// caller keeps the last known state rather than flickering the toggle.
    pub fn read_func(&mut self, token: &str) -> Option<bool> {
        self.control.as_ref()?;
        let reply = self.command(&format!("u {token}\n")).ok()?;
        parse_func_reply(&reply)
    }

    /// Enable/disable a rig CAT function via rigctld `U FUNC <0|1>`. CAT-only; `Ok(())` on
    /// `RPRT 0`, else an error (unsupported func or link failure).
    ///
    /// The receive-side funcs this serves do not care WHY it failed. The one that does — the
    /// ATU, which keys the transmitter — calls [`Self::set_func_value`] and reads the
    /// [`FuncSet`] verdict itself.
    pub fn set_func(&mut self, token: &str, on: bool) -> std::io::Result<()> {
        match self.set_func_value(token, u8::from(on)) {
            FuncSet::Ok => Ok(()),
            FuncSet::Refused(why) | FuncSet::Uncertain(why) => Err(std::io::Error::other(why)),
        }
    }

    /// `U FUNC <n>` with an arbitrary value — because `RIG_FUNC_TUNER` is NOT a boolean.
    ///
    /// Hamlib carries this value through UNCHANGED to the backend, verified in its source rather
    /// than assumed: `tests/rigctl_parse.c` reads it with `sscanf(arg2, "%d", &func_stat)`, and
    /// `src/settings.c::rig_set_func` hands it straight to `caps->set_func(rig, vfo, func,
    /// status)` with no clamping on the way. What each BACKEND then does with it differs, which
    /// is the whole reason this entry point exists — see [`ATU_START_TUNE`].
    pub fn set_func_value(&mut self, token: &str, value: u8) -> FuncSet {
        if self.control.is_none() {
            // Nothing was written, so nothing is acting on it — a refusal, not uncertainty.
            return FuncSet::Refused("not a CAT rig".into());
        }
        let line = func_line(token, value);
        // Refused before the wire, so nothing is acting on it: a refusal, like the one above, and
        // never the `Uncertain` below — which would stand an FT QSO down for a tune-up that never
        // started (the FTX-1's `U TUNER` is a menu write, #385).
        if let Some(why) = self.refused_line(&line) {
            return FuncSet::Refused(why);
        }
        match self.command(&line) {
            Ok(reply) if reply_ok(&reply) => FuncSet::Ok,
            // The rig ANSWERED, and said no. That is a fact about the radio.
            Ok(reply) => FuncSet::Refused(match rprt_code(&reply) {
                Some(code) => format!("the rig answered RPRT {code} to {token} {value}"),
                None => format!("the rig answered {reply:?} to {token} {value}"),
            }),
            // ⚠️ EVERY OTHER OUTCOME IS UNCERTAIN, and deliberately so: `command` writes the
            // line BEFORE it waits, so a deadline, a dropped stream or a half-open link all
            // leave a command on the wire that the radio may be acting on this second. The
            // caller decides what to do with that; what it must not do is call it a refusal.
            Err(e) => FuncSet::Uncertain(format!("no verdict on {token} {value}: {e}")),
        }
    }

    pub fn read_freq(&mut self) -> std::io::Result<u64> {
        if self.control.is_none() {
            return Err(std::io::Error::other("not a CAT rig"));
        }
        let reply = self.command("f\n")?;
        reply
            .lines()
            .find_map(hz_in_line)
            .filter(|hz| *hz > 0)
            .ok_or_else(|| match rprt_code(&reply) {
                Some(code) if rprt_is_link_fault(code) => std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("the rig did not answer the frequency read (Hamlib RPRT {code})"),
                ),
                _ => std::io::Error::other(format!(
                    "rig did not return a frequency (reply {reply:?}) — check the serial port, \
                     baud rate, and that CAT/CI-V is enabled on the rig"
                )),
            })
    }

    /// Read the rig's current mode (e.g. "USB"/"CW"). `None` if not a CAT rig or the
    /// rig didn't answer. The `m` reply is the mode on one line, passband on the next.
    pub fn read_mode(&mut self) -> Option<String> {
        self.read_mode_passband().0
    }

    /// Read the rig's mode + RX passband (Hz) from ONE `m` reply. CAT-only.
    ///
    /// ⚠️ **Both lines are read here, and that is load-bearing rather than tidy.** The previous
    /// wording called the passband "opportunistic — a networked chain that splits the two lines
    /// just surfaces the width on a later poll (the pre-command drain flushes the stray line so
    /// it never poisons the next command)". The parenthetical was false, and CI proved it: the
    /// drain is non-blocking, so a second line still in flight survives it and is read as the
    /// NEXT command's answer. See [`command_expecting`], which carries the measurement. Leaving
    /// nothing behind is also what the [`command`] header calls a TX-safety invariant — the
    /// desync it names is "a keyed rig read as PTT ok".
    pub fn read_mode_passband(&mut self) -> (Option<String>, Option<u32>) {
        if self.control.is_none() {
            return (None, None);
        }
        match self.command_expecting("m\n", 2) {
            Ok(reply) => parse_mode_passband(&reply),
            Err(_) => (None, None),
        }
    }

    /// Set the RX passband width (Hz) by re-issuing the current mode with the new width — Hamlib
    /// carries filter width as the 2nd arg of set_mode (there is no portable bandwidth level).
    /// CAT-only. The caller passes the current mode (the loop tracks it).
    pub fn set_passband(&mut self, mode: &str, hz: u32) -> std::io::Result<()> {
        self.set_mode(mode, hz as i32)
    }

    /// Send a RAW CAT command string straight to the rig via rigctld's `w` (send_cmd) and
    /// return the rig's reply. This BYPASSES Hamlib's mode abstraction AND its mode cache —
    /// `read_mode` (the `m` command) can return the mode Hamlib *thinks* it set even when the
    /// rig never moved, whereas e.g. raw Yaesu `MD0;` returns the rig's TRUE current mode code
    /// off the wire. Diagnostic-only; `None` if not a CAT rig or no reply.
    /// Send a raw CAT string that the rig does NOT answer — a SET.
    ///
    /// ⛔ **NEVER THROUGH `w`.** rigctld's `w` (send_cmd) writes the string and then waits for
    /// the radio's reply for the radio's whole read timeout, holding the port, and a Yaesu never
    /// answers a set (Hamlib 4.7.1 `tests/rigctl_parse.c` `send_cmd`: `read_string(rp, buf,
    /// rxbytes, …)` runs for a set too; `.timeout` is 1000 ms on the FT-710, 2000 ms on the
    /// FTDX101, FTDX10, FT-991, FT-891 and FTX-1). Every command after it, the next `T 0`
    /// included, waited that long: Stop TX on an FT-710 reached the radio 2.06 s late behind the
    /// two `KY` playback stops, 4.18 s on an FTX-1, against a real rigctld (4.7.1 and 4.5.5).
    ///
    /// `W <cmd> 0` (send_cmd_rx) is the same write with a reply length of zero, and Hamlib
    /// reads nothing for it (`read_string_generic` returns at once for `rxmax < 1`, iofunc.c).
    /// rigctld then answers with one terminator, a NUL on 4.7.1 and a newline on 4.5.5, which
    /// `command_inner` takes as a whole reply, so nothing is left in the socket either. The
    /// deadline is the PTT one: like a key, this waits for rigctld and never for the radio.
    ///
    /// Returns whether rigctld took the line — NOT whether the radio honoured it, which nothing
    /// on this path can know. The caller confirms by reading the value back.
    pub fn send_raw_set(&mut self, raw: &str) -> bool {
        self.raw_set(raw, false)
    }

    /// [`Self::send_raw_set`] for a STOP (a radio's own CW playback stop): written behind any
    /// answer a late key still owes, never held back by it ([`Self::command_lines`]).
    pub fn send_raw_stop(&mut self, raw: &str) -> bool {
        self.raw_set(raw, true)
    }

    fn raw_set(&mut self, raw: &str, ordered: bool) -> bool {
        if self.control.is_none() {
            return false;
        }
        self.command_lines(
            &format!("W {raw} 0\n"),
            Some(PTT_DEADLINE_MS),
            None,
            1,
            false,
            ordered,
        )
        .is_ok_and(|reply| !reply.trim_start().starts_with("RPRT -"))
    }

    pub fn send_raw(&mut self, raw: &str) -> Option<String> {
        self.send_raw_within(raw, None)
    }

    /// [`Self::send_raw`] with a caller-chosen deadline (ms), for a read made while a tune carrier
    /// plays: the bound [`Self::read_meter_f32_within`] gives the tune-time meter poll. `None` =
    /// the transport's normal deadline.
    pub fn send_raw_within(&mut self, raw: &str, deadline_ms: Option<u64>) -> Option<String> {
        self.control.as_ref()?;
        let line = format!("w {raw}\n");
        let reply = match deadline_ms {
            Some(ms) => self.command_ending_by(&line, ms),
            None => self.command(&line),
        }
        .ok()?;
        // `str::trim` does NOT remove a NUL — it is not whitespace — so trim it explicitly or every
        // caller gets "MD02;\0" and has to know that. The rig's own terminator (`;`) is left alone:
        // callers parse the rig's string, not a cleaned-up version of it.
        let trimmed = reply.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    // --- the all-mode (phone/CW) control surface. All CAT-only (no-op otherwise). ---

    /// Set split on/off and which VFO transmits (DX pileups). `tx_vfo` e.g. "VFOB".
    pub fn set_split(&mut self, on: bool, tx_vfo: &str) -> std::io::Result<()> {
        self.cat(&split_line(on, tx_vfo))
    }
    /// Set the split (TX) frequency in Hz.
    pub fn set_split_freq(&mut self, hz: u64) -> std::io::Result<()> {
        self.cat(&split_freq_line(hz))
    }
    /// Read the rig's CURRENT split state via rigctld `s` (get_split_vfo) — is split on, and
    /// which VFO transmits. The reply is two lines: `0|1` then the TX VFO name.
    ///
    /// ⚠️ THIS EXISTS SO SPLIT CAN BE RESTORED RATHER THAN FORCED OFF. `Rig` had no split getter
    /// at all, so the loop's teardown wrote an absolute `S 0 VFOA` — which on a multi-client
    /// radio (a Flex with SmartSDR beside us, or any rig where the operator set split at the
    /// front panel) silently cancelled a split Nexus never turned on. 2026-08-17 Flex audit,
    /// wave-2 #22.
    ///
    /// CAT-only; `None` on VOX/serial, a link hiccup, or a reply we cannot parse — and a `None`
    /// means the caller must keep today's behaviour rather than guess at a state it never read.
    pub fn read_split(&mut self) -> Option<(bool, String)> {
        self.control.as_ref()?;
        // Two lines, for the same reason `m` reads two: a split reply would otherwise leave the
        // VFO name in the socket for the next command to read as its own answer.
        let reply = self.command_expecting("s\n", 2).ok()?;
        let mut lines = reply.lines().map(str::trim).filter(|l| !l.is_empty());
        let on = lines.next()?.parse::<u8>().ok()? != 0;
        // A rig that answers the flag but not the VFO name is still worth believing about the
        // flag; VFO B is the near-universal TX VFO and what `apply_tx_dial_shift` itself uses.
        let vfo = lines.next().unwrap_or("VFOB").to_string();
        Some((on, vfo))
    }
    /// Read the split (TX) frequency in Hz via rigctld `i` (get_split_freq) — the other half of
    /// what a restore needs: `set_split_freq` overwrites the TX VFO's frequency with no snapshot,
    /// so even restoring split=ON would leave the operator's TX frequency clobbered.
    /// CAT-only; `None` on VOX/serial or a non-numeric reply.
    pub fn read_split_freq(&mut self) -> Option<u64> {
        self.control.as_ref()?;
        let reply = self.command("i\n").ok()?;
        reply.lines().find_map(hz_in_line).filter(|hz| *hz > 0)
    }
    /// Set the split (TX) VFO's mode + passband — the transmit-side twin of
    /// [`Self::set_mode`], which only ever reaches the RX VFO. A BLANK mode is a
    /// no-op for the same reason it is there: the caller is choosing to obey the
    /// radio. Rig rejections surface as `Err` (many backends do not implement
    /// `X` at all), so the caller can say so rather than assume it landed.
    pub fn set_split_mode(&mut self, mode: &str, passband_hz: i32) -> std::io::Result<()> {
        if mode.trim().is_empty() {
            return Ok(());
        }
        self.cat(&split_mode_line(mode, passband_hz))
    }
    /// Select the active VFO (e.g. "VFOA"/"VFOB").
    pub fn set_vfo(&mut self, vfo: &str) -> std::io::Result<()> {
        self.cat(&vfo_line(vfo))
    }
    /// Read WHICH VFO the rig is on via rigctld `v` (get_vfo) — `Some(true)` = B.
    ///
    /// ⚠️ THIS EXISTS BECAUSE A/B SELECTION WAS WRITE-ONLY. Nexus told the rig which VFO to use
    /// and never asked, so an A/B press on the radio's own front panel left the indicator
    /// reporting the last command — which by then named the wrong VFO.
    ///
    /// `None` for everything that is not plainly A or B, and that is the whole safety of it:
    /// no CAT, a rejection, a link hiccup, `MEM`, `currVFO`, or a Main/Sub name this step does
    /// not model. A `None` must leave the indicator exactly as it was — "absent" is not "A".
    ///
    /// The caller must only ever ask a rig whose capability dump said `Can get VFO: Y`
    /// ([`crate::baud_ladder::RigCaps::vfo_read_native`]); an emulated answer is Hamlib's cache
    /// of our own last `set_vfo` and can only ever agree with us.
    pub fn read_vfo(&mut self) -> Option<bool> {
        self.control.as_ref()?;
        let reply = self.command("v\n").ok()?;
        // `rig_strvfo` prints "VFOA"/"VFOB"; the bare letters are the spelling
        // `civ::commands::select_vfo` already accepts, so both are read here.
        match reply.lines().map(str::trim).find(|l| !l.is_empty())? {
            "VFOA" | "A" => Some(false),
            "VFOB" | "B" => Some(true),
            // "Main"/"Sub"/"MEM"/"currVFO" and anything else: NOT translated to A or B. A
            // satellite rig's Main/Sub is a different question and belongs to the dual-receiver
            // work, not to this indicator.
            _ => None,
        }
    }
    /// Ask the rig ONCE whether it can report its own VFO selection — the `v` twin of
    /// [`Self::read_split_capability`], off the same `\dump_caps` reply.
    ///
    /// `None` = we could not ask, which the caller must treat as "cannot", because silence is
    /// not permission here either.
    /// The rig's OWN lowest settable RF power, in thousandths of full scale, off the same
    /// `\dump_caps` reply as [`Self::read_split_capability`].
    ///
    /// ⚠️ ASK THIS ONLY WHEN A READING ALREADY LOOKS LIKE ZERO, and cache the answer. The dump is
    /// a long multi-line reply, and issuing it inside the heavy poll is what once starved the
    /// readers after it — the regression recorded in [`crate::amppoll`]. A healthy radio never
    /// reaches the caller that asks, so a working station pays nothing for this.
    ///
    /// `None` = could not ask, or a rig that publishes no RFPOWER at all. Neither is a floor of
    /// zero, and the caller must leave such a reading exactly as it found it.
    pub fn read_rfpower_floor_milli(&mut self) -> Option<u16> {
        self.control.as_ref()?;
        let reply = self.command_multiline("\\dump_caps\n").ok()?;
        crate::baud_ladder::parse_caps(&reply).rfpower_floor_milli
    }
    pub fn read_vfo_capability(&mut self) -> Option<bool> {
        self.control.as_ref()?;
        let reply = self.command_multiline("\\dump_caps\n").ok()?;
        Some(crate::baud_ladder::parse_caps(&reply).vfo_read_native)
    }
    /// Set RIT (receive incremental tuning) offset in Hz; enabling RIT first (0 = off).
    pub fn set_rit(&mut self, hz: i32) -> std::io::Result<()> {
        self.cat(&func_line("RIT", u8::from(hz != 0)))?;
        self.cat(&rit_line(hz))
    }
    /// Set XIT (transmit incremental tuning) offset in Hz (0 = off).
    pub fn set_xit(&mut self, hz: i32) -> std::io::Result<()> {
        self.cat(&func_line("XIT", u8::from(hz != 0)))?;
        self.cat(&xit_line(hz))
    }
    /// Set RF output power as a 0.0–1.0 fraction (Hamlib `RFPOWER`).
    pub fn set_power(&mut self, frac: f32) -> std::io::Result<()> {
        self.cat(&level_line(
            "RFPOWER",
            &format!("{:.3}", frac.clamp(0.0, 1.0)),
        ))
    }
    /// Set mic gain as a 0.0–1.0 fraction (Hamlib `MICGAIN`).
    pub fn set_mic_gain(&mut self, frac: f32) -> std::io::Result<()> {
        self.cat(&level_line(
            "MICGAIN",
            &format!("{:.3}", frac.clamp(0.0, 1.0)),
        ))
    }
    /// Set an RX DSP level (0.0–1.0) by Hamlib name, e.g. "NR" or "NB".
    pub fn set_rx_level(&mut self, name: &str, frac: f32) -> std::io::Result<()> {
        self.cat(&level_line(name, &format!("{:.3}", frac.clamp(0.0, 1.0))))
    }
    /// Set a receive level (0.0–1.0) on a NAMED receiver — [`receiver_level_line`]. Main is
    /// exactly [`Rig::set_rx_level`]. The Sub is accepted only by a backend that can address it
    /// (Nexus's own CI-V daemon on a dual-receiver Icom); the radio loop sends it nowhere else.
    pub fn set_rx_level_on(
        &mut self,
        rx: crate::dualrx::ReceiverId,
        name: &str,
        frac: f32,
    ) -> std::io::Result<()> {
        self.cat(&receiver_level_line(
            rx,
            name,
            &format!("{:.3}", frac.clamp(0.0, 1.0)),
        ))
    }
    /// Set the MANUAL-NOTCH FREQUENCY in Hz (Hamlib `NOTCHF`). Not a 0.0–1.0 level: this one
    /// is an absolute frequency in the audio passband, which is why it does not go through
    /// [`Rig::set_rx_level`] — that clamps to a fraction and would command a notch at 1 Hz.
    pub fn set_notch_freq_hz(&mut self, hz: f32) -> std::io::Result<()> {
        self.cat(&level_line(
            "NOTCHF",
            &format!("{}", hz.max(0.0).round() as i32),
        ))
    }
    /// Set the TRANSMIT-MONITOR GAIN (Hamlib `MONITOR_GAIN`) as a 0.0–1.0 fraction — how
    /// loud the rig plays your own audio back while you are talking. Its on/off half is the
    /// `MON` func, not a level.
    pub fn set_monitor_gain(&mut self, frac: f32) -> std::io::Result<()> {
        self.set_rx_level("MONITOR_GAIN", frac)
    }

    /// Set the ATTENUATOR in whole dB (Hamlib `ATT`; `0` = off).
    ///
    /// ⚠️ NOT [`Rig::set_rx_level`], for the same reason [`Rig::set_notch_freq_hz`] is not:
    /// that one clamps to 0..1 and prints `{:.3}`, so a 12 dB pad would leave here as
    /// `L ATT 1.000` — well-formed, accepted, and the wrong amount of attenuation.
    ///
    /// The caller must offer only pads the rig declares ([`Rig::read_db_steps`]); a value
    /// the rig does not have is refused or rounded by the backend, and neither is a thing
    /// to discover on the air.
    pub fn set_att_db(&mut self, db: u8) -> std::io::Result<()> {
        self.cat(&level_line("ATT", &db.to_string()))
    }
    /// Set the PREAMP by its dB LABEL (Hamlib `PREAMP`; `0` = off). Same integer path and
    /// the same reason as [`Rig::set_att_db`]. On an Icom the label is translated to a list
    /// POSITION at the CI-V boundary — see `civ::commands::preamp_index_for_db`.
    pub fn set_preamp_db(&mut self, db: u8) -> std::io::Result<()> {
        self.cat(&level_line("PREAMP", &db.to_string()))
    }
    /// Set an Icom's SSB transmit-bandwidth step (`16 58`: 0 WIDE, 1 MID, 2 NAR) through Nexus's
    /// own CI-V daemon, the only link that answers this token — see `crate::txextras`.
    pub fn set_ssb_tx_bw(&mut self, idx: u8) -> std::io::Result<()> {
        self.cat(&level_line(
            crate::txextras::ICOM_TBW_TOKEN,
            &idx.to_string(),
        ))
    }
    /// Read the attenuator in dB. Through the RAW reader, because [`Rig::read_level`]
    /// filters to 0.0..=1.0 and would REJECT every reading except "off" — which fails
    /// asymmetrically: the control looks healthy until the operator switches a pad in.
    pub fn read_att_db(&mut self) -> Option<u8> {
        self.read_meter_f32("ATT")
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
    }
    /// Read the preamp's dB label. Same raw-reader reason as [`Rig::read_att_db`].
    pub fn read_preamp_db(&mut self) -> Option<u8> {
        self.read_meter_f32("PREAMP")
            .map(|v| v.round().clamp(0.0, 255.0) as u8)
    }

    /// The radio's own ATTENUATOR pads and PREAMP steps, off one `\dump_state`.
    ///
    /// An attenuator is a LIST, not a slider — the handful of pads this particular radio
    /// has — so the control cannot be built without asking. `None` = we could not ask or
    /// could not walk the reply, and the caller must then offer no control at all rather
    /// than a guessed ladder (the same rule as [`Rig::read_rx_ranges`], for the same
    /// reason: a made-up pad is commanded at the radio, not merely displayed).
    pub fn read_db_steps(&mut self) -> Option<DbSteps> {
        self.control.as_ref()?;
        // Long multi-line reply, like the range table — read to the deadline, not to the
        // first newline.
        let reply = self.command_multiline("\\dump_state\n").ok()?;
        parse_dump_state_db_lists(&reply)
    }

    /// Set the AGC time constant by Hamlib enum int (FAST=2, MEDIUM=5, SLOW=3, OFF=0).
    pub fn set_agc(&mut self, hamlib_val: u8) -> std::io::Result<()> {
        self.cat(&level_line("AGC", &hamlib_val.to_string()))
    }
    /// Read the AGC time constant (Hamlib `AGC`) as its enum int; `None` if unsupported/no reply.
    pub fn read_agc(&mut self) -> Option<u8> {
        self.read_meter_f32("AGC").map(|v| v.round() as u8)
    }
    /// Set the rig's internal CW keyer speed in WPM (Hamlib `KEYSPD`).
    pub fn set_keyspd(&mut self, wpm: u32) -> std::io::Result<()> {
        self.cat(&level_line("KEYSPD", &wpm.to_string()))
    }
    /// Key CW from text via the rig's own keyer (Hamlib `send_morse`). Set the speed
    /// first with [`set_keyspd`](Self::set_keyspd). Best for canned/keyboard macros;
    /// CAT latency makes it poor for live paddle feel (use WinKeyer for that).
    ///
    /// ⛔ Text the radio's keyer may not be handed ([`cw_text_refusal`]) is refused before anything
    /// is sent (`InvalidInput`, with the CW screen's words), whoever asks.
    pub fn send_morse(&mut self, text: &str) -> std::io::Result<()> {
        if let Some(why) = cw_text_refusal(text) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, why));
        }
        self.cat(&morse_line(text))
    }
    /// Abort CW in progress. Newer Hamlib exposes `\stop_morse`; older builds vary by
    /// manufacturer (the WinKeyer path has a reliable Clear-Buffer abort instead).
    /// A stop: written behind any answer a late key still owes, never held back by it
    /// ([`Self::command_lines`]).
    pub fn stop_morse(&mut self) -> std::io::Result<()> {
        if self.control.is_none() {
            return Ok(());
        }
        let line = "\\stop_morse\n";
        let reply = self.command_lines(line, None, None, 1, false, true)?;
        Self::cat_reply(line, &reply)
    }
    /// Play the rig's voice memory `ch` — Hamlib's `\send_voice_mem`, the exact spelling
    /// its own NET client uses (on a Yaesu it becomes `PB0<ch>;`). ⚠️ The RIG transmits the
    /// message itself; this only relays the ask.
    pub fn send_voice_mem(&mut self, ch: u32) -> std::io::Result<()> {
        self.cat(&format!("\\send_voice_mem {ch}\n"))
    }
    /// Abort a voice-memory playback in progress.
    pub fn stop_voice_mem(&mut self) -> std::io::Result<()> {
        self.cat("\\stop_voice_mem\n")
    }

    /// Send a rigctld command, succeeding on `RPRT 0` (or an empty reply); no-op when
    /// no CAT control channel is configured. Shared by the all-mode control verbs above.
    fn cat(&mut self, line: &str) -> std::io::Result<()> {
        if self.control.is_none() {
            return Ok(());
        }
        let reply = self.command(line)?;
        Self::cat_reply(line, &reply)
    }

    /// What rigctld's `reply` to the set `line` says ([`Self::cat`]).
    fn cat_reply(line: &str, reply: &str) -> std::io::Result<()> {
        if reply_ok(reply) || reply.is_empty() {
            Ok(())
        } else if rprt_code(reply).is_some_and(rprt_is_link_fault) {
            // The rig did not answer: the link, as for a dial or a mode, never a refusal.
            Err(rprt_error(line.trim(), reply))
        } else {
            Err(std::io::Error::other(format!(
                "rigctld error for {line:?}: {reply:?}"
            )))
        }
    }
}

/// LAST-DITCH TEARDOWN UNKEY — the backstop for "the operator must always be able to stop a
/// transmission".
///
/// `run_radio` drops PTT on its error RETURN, but only that exit path reaches the line. A panic
/// unwinding past it, the thread being torn down, or any later restructuring of the loop skips it
/// entirely, and the `catch_unwind` in src-tauri cannot see the rig. Every other resource this
/// crate owns already had a destructor (`RigctldProc`, `FskKeyer`, `WinKeyer`, `SerialKeyer`,
/// `FlexDax`, `FlexSpectrum`, `DecodeWorker`) — the one that is RADIATING did not, so a radio-thread
/// panic mid-over left the carrier up until the operator reached the rig.
///
/// GATED ON `keyed`, deliberately. `keyed` is fail-safe (a key-down ATTEMPT sets it; a failed unkey
/// never clears it), so it is exactly "we believe this radio may be transmitting". An unconditional
/// teardown unkey would instead make every idle and every read-only monitor rig send a keying
/// command on drop, and when the stream has already been dropped by a failed command
/// `ensure_connected` would open a fresh TCP connection just to send it.
///
/// It cannot fight the normal path: every teardown that unkeys first leaves `keyed == false`, so
/// this is a no-op there, and a second unkey at the radio is idempotent anyway.
///
/// IT CAN BLOCK, and that is the accepted trade. A CAT unkey over a live stream is bounded by the
/// 700 ms `PTT_DEADLINE_MS`; if a prior failure dropped the stream, `ensure_connected` reconnects
/// first, bounded by `CAT_CONNECT_TIMEOUT` (immediate against a refused localhost rigctld — the
/// usual case — up to that bound for a network rig that has gone away). A teardown that waits is
/// better than a radio that keeps transmitting.
impl Drop for Rig {
    fn drop(&mut self) {
        if !self.keyed {
            return;
        }
        // Best-effort, result discarded: a Drop must never panic. (`ptt` itself is panic-free —
        // the whole path propagates I/O errors with `?`, and the diag sink swallows a poisoned
        // lock rather than unwrapping it.)
        let _ = self.ptt(false);
    }
}

#[cfg(test)]
mod tests {
    /// A powered-off rig answers `RPRT -5`; the radio loop must see a LINK fault, not a
    /// refusal — that misread is what latched the band give-up on a radio that was only off.
    #[test]
    fn a_hamlib_timeout_is_a_link_fault_and_a_refusal_stays_a_refusal() {
        assert_eq!(super::rprt_code("RPRT -5\n"), Some(-5));
        assert_eq!(super::rprt_code("14074000\n"), None);
        assert_eq!(super::rprt_code("garbage\nRPRT -1\n"), Some(-1));
        for code in [-5, -6, -13, -14] {
            assert!(super::rprt_is_link_fault(code), "RPRT {code} is the link");
        }
        for code in [-1, -9, -15, -17, -4] {
            assert!(!super::rprt_is_link_fault(code), "RPRT {code} is a refusal");
        }
        assert_eq!(
            super::rprt_error("freq", "RPRT -5\n").kind(),
            std::io::ErrorKind::TimedOut
        );
        assert_eq!(
            super::rprt_error("freq", "RPRT -1\n").kind(),
            std::io::ErrorKind::Other
        );
        assert_eq!(
            super::rprt_error("freq", "nonsense\n").kind(),
            std::io::ErrorKind::Other,
            "an unparseable reply is still a refusal-class error, never a silent success"
        );
    }

    /// ⭐ AN EXTENDED WRITE THE RIG DID NOT ANSWER IS THE LINK, AS A DIAL IS. A level and a split
    /// each answered `RPRT -5` read as no reply (`TimedOut`), which the radio loop words as the
    /// link; `RPRT -1` stays a refusal (`Other`) in the words it always had, and `RPRT 0` is done.
    /// The silence read as a refusal.
    #[test]
    fn an_extended_write_the_rig_did_not_answer_is_the_link_and_a_refusal_stays_a_refusal() {
        let (addr, _log) = mock_rigctld(|line| {
            match line {
                "L RFPOWER 0.500" | "S 1 VFOB" => "RPRT -5\n",
                "L RFPOWER 0.250" | "S 0 VFOA" => "RPRT -1\n",
                _ => "RPRT 0\n",
            }
            .to_string()
        });
        let mut rig = Rig::rigctld(&addr);
        let kinds = [
            rig.set_power(0.5),
            rig.set_power(0.25),
            rig.set_power(0.75),
            rig.set_split(true, "VFOB"),
            rig.set_split(false, "VFOA"),
        ]
        .map(|r| r.map_err(|e| e.kind()));
        let refusal = rig.set_power(0.25).unwrap_err().to_string();
        assert_eq!(
            (kinds, refusal.as_str()),
            (
                [
                    Err(std::io::ErrorKind::TimedOut),
                    Err(std::io::ErrorKind::Other),
                    Ok(()),
                    Err(std::io::ErrorKind::TimedOut),
                    Err(std::io::ErrorKind::Other),
                ],
                "rigctld error for \"L RFPOWER 0.250\\n\": \"RPRT -1\\n\""
            )
        );
    }

    use super::*;

    #[test]
    fn command_lines_match_rigctld_protocol() {
        assert_eq!(ptt_line(true), "T 1\n");
        assert_eq!(ptt_line(false), "T 0\n");
        assert_eq!(freq_line(14_074_000), "F 14074000\n");
        assert_eq!(mode_line("USB", 0), "M USB 0\n");
        assert_eq!(mode_line("FM", 0), "M FM 0\n");
        // -1 = RIG_PASSBAND_NOCHANGE: leave the rig's own filter width alone (no Width-OSD pop).
        assert_eq!(mode_line("USB", -1), "M USB -1\n");
        // The mode-set resilience ladder's wire forms (service.rs): the full DATA
        // passband, then the filter-agnostic retry, then the plain-sideband fallback.
        assert_eq!(mode_line("PKTUSB", 3000), "M PKTUSB 3000\n");
        assert_eq!(mode_line("PKTUSB", 0), "M PKTUSB 0\n");
        assert!(reply_ok("RPRT 0\n"));
        assert!(!reply_ok("RPRT -1\n"));
    }

    /// ISSUE #86 — the closing K of every CW macro was silently never keyed.
    ///
    /// Hamlib's Yaesu `newcat_send_morse` reads a ONE-character message as a keyer-memory
    /// number ('1'..'5' only) and refuses anything else with -RIG_EINVAL — so word-at-a-time
    /// keying dropped every single-character word. The pad routes them down the normal path.
    #[test]
    fn a_single_character_cw_word_is_padded_past_hamlibs_memory_number_trap() {
        assert_eq!(morse_line("K"), "b K \n", "the closing K must actually key");
        assert_eq!(morse_line("?"), "b ? \n");
        assert_eq!(
            morse_line("5"),
            "b 5 \n",
            "a lone digit must KEY '5', not play the rig's stored memory 5"
        );
        // Multi-character words are untouched — the pad is not blanket rewriting.
        assert_eq!(morse_line("CQ"), "b CQ\n");
        assert_eq!(morse_line("VE3FMQ"), "b VE3FMQ\n");
    }

    /// ⛔ THE RADIO'S OWN KEYER IS NEVER HANDED A `;`, A LINE BREAK, ANOTHER CONTROL CHARACTER OR A
    /// CHARACTER WITH NO MORSE. Hamlib puts `send_morse`'s text inside a radio command, and a
    /// Kenwood, Yaesu or Elecraft radio ends a command at the first `;`: the TS-2000 backend wrote
    /// `KY TEST;` and then `FA00007000000      ;` for `b TEST;FA00007000000` (rigctl 4.5.5), a
    /// retune. A line break would end rigctld's own command and start another. Each such text is
    /// refused, says which character, and NOTHING reaches rigctld. CONTROLS, by value: ordinary
    /// text, a padded one-character word, several words, lower case and every punctuation mark in
    /// Nexus's table go out exactly as before.
    #[test]
    fn the_cat_keyer_hands_rigctld_no_semicolon_line_break_or_character_without_morse() {
        let (addr, log) = mock_rigctld(ok_reply(7_030_000));
        let mut rig = Rig::rigctld(&addr);
        // Each refused text, and the character the refusal names, as the CW screen shows it.
        for (bad, named) in [
            ("TEST;FA00007000000", r#"";""#),
            ("K1ABC;", r#"";""#),
            (";", r#"";""#),
            ("TEST\nF 7000000", r#""\n""#),
            ("TEST\r", r#""\r""#),
            ("TEST\tDE", r#""\t""#),
            ("\u{1b}[2J", r#""\u{1b}""#),
            ("TEST\u{7f}", r#""\u{7f}""#),
            ("A\0B", r#""\u{0}""#),
            ("73!", r#""!""#),
            ("QRL'", r#""'""#),
            ("CAFÉ", r#""É""#),
        ] {
            let res = rig.send_morse(bad);
            assert_eq!(
                *log.lock().unwrap(),
                Vec::<String>::new(),
                "{bad:?} reached rigctld"
            );
            let Err(err) = res else {
                panic!("{bad:?} must be refused")
            };
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{bad:?}");
            assert!(
                err.to_string().contains(named),
                "{bad:?}: the refusal names {named}: {err}"
            );
        }
        for (text, wire) in [
            ("TEST", "b TEST"),
            ("K", "b K "),
            ("?", "b ? "),
            ("CQ CQ DE W9XYZ", "b CQ CQ DE W9XYZ"),
            ("cq de w9xyz", "b cq de w9xyz"),
            ("5NN/?=+-().,:@", "b 5NN/?=+-().,:@"),
        ] {
            let from = log.lock().unwrap().len();
            rig.send_morse(text)
                .unwrap_or_else(|e| panic!("control: {text:?} keys: {e}"));
            assert_eq!(
                log.lock().unwrap()[from..],
                [wire.to_string()],
                "control: {text:?} goes out exactly as before"
            );
        }
    }

    /// THE ATU BUTTON DID NOTHING — the wire form, pinned (operator 2026-08-15, FTDX10).
    ///
    /// `U TUNER 1` is Yaesu `AC001` ("ATU in line"), and on a rig whose tuner is already in
    /// line that is a no-op the operator sees as a dead button. `U TUNER 2` is `AC002`,
    /// "start tuning". This test is the byte-level half of that fix; the rig-level half needs
    /// a bench, and `ATU_START_TUNE` says so.
    #[test]
    fn the_atu_press_asks_the_rig_to_tune_up_not_merely_to_switch_the_tuner_in() {
        assert_eq!(func_line("TUNER", ATU_START_TUNE), "U TUNER 2\n");
        assert_ne!(
            func_line("TUNER", ATU_START_TUNE),
            "U TUNER 1\n",
            "1 only switches the tuner in-line on Yaesu — it does not start a tune-up, which \
             is exactly what made the button look dead"
        );
        // The ordinary boolean funcs must be UNTOUCHED by that entry point — NB/NR/ANF/COMP/VOX
        // are genuine on/off DSP toggles and a stray 2 there would be a different command.
        assert_eq!(func_line("NB", u8::from(true)), "U NB 1\n");
        assert_eq!(func_line("NR", u8::from(false)), "U NR 0\n");
    }

    #[test]
    fn fm_repeater_lines_match_rigctld_protocol() {
        assert_eq!(rptr_shift_line("plus"), "R +\n");
        assert_eq!(rptr_shift_line("minus"), "R -\n");
        assert_eq!(rptr_shift_line("simplex"), "R None\n");
        // The short forms are what `Settings` stores on the remote-recall path, and they route
        // through the same `rptr_shift_sign` the licence gate asks — so these three cases are
        // also the cross-crate agreement check: if the wire ever disagreed with the gate about
        // which way a shift moves the transmitter, the gate would judge a frequency the rig was
        // never going to key.
        assert_eq!(rptr_shift_line("+"), "R +\n");
        assert_eq!(rptr_shift_line("-"), "R -\n");
        assert_eq!(
            rptr_shift_line("MINUS"),
            "R -\n",
            "case-insensitive, as it always was"
        );
        assert_eq!(
            rptr_shift_line("sideways"),
            "R None\n",
            "unrecognised → no shift"
        );
        assert_eq!(rptr_offset_line(600_000), "O 600000\n");
        assert_eq!(ctcss_line(100.0), "C 1000\n"); // Hamlib wants tenths of Hz
        assert_eq!(ctcss_line(0.0), "C 0\n"); // off
        assert_eq!(ctcss_line(88.5), "C 885\n");
    }

    #[test]
    fn smeter_strength_parses_db_relative_to_s9() {
        assert_eq!(parse_smeter_db("-54\n"), Some(-54)); // ~S0, no signal
        assert_eq!(parse_smeter_db("-36\n"), Some(-36)); // ~S3
        assert_eq!(parse_smeter_db("0\n"), Some(0)); // S9
        assert_eq!(parse_smeter_db("20\n"), Some(20)); // S9+20
        assert_eq!(parse_smeter_db("-5.0\n"), Some(-5)); // float form rounds to int dB
        assert_eq!(parse_smeter_db("RPRT -1\n"), None); // error reply is not a reading
        assert_eq!(parse_smeter_db(""), None); // rig didn't answer
        assert_eq!(parse_smeter_db("9999\n"), None); // garbage magnitude → rejected
    }

    #[test]
    fn func_get_reply_branches_on_value_vs_rprt() {
        // Default protocol: a successful get is value-only (no RPRT); an error is RPRT<negative>.
        assert_eq!(parse_func_reply("1\n"), Some(true));
        assert_eq!(parse_func_reply("0\n"), Some(false));
        assert_eq!(parse_func_reply("RPRT -11\n"), None); // ENAVAIL — rig lacks the func
        assert_eq!(parse_func_reply("RPRT -5\n"), None); // transient — caller keeps last state
        assert_eq!(parse_func_reply(""), None); // no answer
    }

    #[test]
    fn mode_passband_parse_splits_the_m_reply() {
        assert_eq!(
            parse_mode_passband("USB\n2400\n"),
            (Some("USB".into()), Some(2400))
        );
        assert_eq!(
            parse_mode_passband("CW\n500\n"),
            (Some("CW".into()), Some(500))
        );
        assert_eq!(parse_mode_passband("USB\n"), (Some("USB".into()), None)); // split → width later
        assert_eq!(parse_mode_passband("USB\n0\n"), (Some("USB".into()), None)); // 0 = rig default
        assert_eq!(parse_mode_passband("RPRT -1\n"), (None, None));
    }

    #[test]
    fn all_mode_control_lines_match_rigctld_protocol() {
        assert_eq!(split_line(true, "VFOB"), "S 1 VFOB\n");
        assert_eq!(split_line(false, "VFOA"), "S 0 VFOA\n");
        assert_eq!(split_freq_line(14_205_000), "I 14205000\n");
        // `X` is the ONLY verb that reaches the TX VFO's mode — `M` is RX-side.
        // An inverting satellite transponder is exactly the case that needs it:
        // listen USB, transmit LSB, on one radio.
        assert_eq!(split_mode_line("LSB", -1), "X LSB -1\n");
        assert_eq!(vfo_line("VFOA"), "V VFOA\n");
        assert_eq!(func_line("RIT", u8::from(true)), "U RIT 1\n");
        assert_eq!(func_line("XIT", u8::from(false)), "U XIT 0\n");
        assert_eq!(rit_line(-200), "J -200\n");
        assert_eq!(xit_line(500), "Z 500\n");
        assert_eq!(level_line("RFPOWER", "0.500"), "L RFPOWER 0.500\n");
        assert_eq!(level_line("KEYSPD", "25"), "L KEYSPD 25\n");
        // A receive level for a NAMED receiver: the Sub by Hamlib's VFO-mode spelling, and Main
        // byte for byte the plain verb — so nothing a single-receiver radio is sent can change.
        assert_eq!(
            receiver_level_line(crate::dualrx::ReceiverId::Sub, "AF", "0.500"),
            "L Sub AF 0.500\n"
        );
        assert_eq!(
            receiver_level_line(crate::dualrx::ReceiverId::Main, "AF", "0.500"),
            level_line("AF", "0.500")
        );
        // send_morse takes the rest of the line as the CW text (spaces preserved).
        assert_eq!(morse_line("CQ CQ DE W9XYZ"), "b CQ CQ DE W9XYZ\n");
    }

    #[test]
    fn all_mode_control_is_a_no_op_under_vox() {
        // Every new verb is CAT-only — under VOX they must not attempt a connection.
        let mut rig = Rig::vox();
        rig.set_split(true, "VFOB").unwrap();
        rig.set_split_freq(14_205_000).unwrap();
        rig.set_vfo("VFOA").unwrap();
        rig.set_rit(-200).unwrap();
        rig.set_xit(0).unwrap();
        rig.set_power(0.5).unwrap();
        rig.set_keyspd(25).unwrap();
        rig.send_morse("TEST").unwrap();
        rig.stop_morse().unwrap();
        assert_eq!(rig.read_mode(), None);
    }

    #[test]
    fn vox_mode_keys_without_a_socket() {
        let mut rig = Rig::vox();
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        // freq/mode are also no-ops under VOX (no connection attempted).
        rig.set_freq(14_074_000).unwrap();
        rig.set_mode("USB", 0).unwrap();
    }

    // Without the `serial` feature, Serial PTT must fall back to a no-op (like
    // VOX) so the engine can run with no serial backend and no real port.
    #[cfg(not(feature = "serial"))]
    #[test]
    fn serial_mode_falls_back_to_vox_without_a_port() {
        let mut rig = Rig::serial("COM_DOES_NOT_EXIST", SerialLine::Rts);
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        // freq/mode are no-ops outside rigctld CAT — no connection attempted.
        rig.set_freq(14_074_000).unwrap();
        rig.set_mode("USB", 0).unwrap();
    }

    #[test]
    fn serial_constructor_sets_mode() {
        let rig = Rig::serial("COM5", SerialLine::Dtr);
        assert!(matches!(
            rig.ptt_mode,
            PttMode::Serial { ref port, line: SerialLine::Dtr } if port == "COM5"
        ));
        assert!(rig.control.is_none(), "serial PTT alone has no CAT control");
        assert!(!rig.keyed);
    }

    // ---- Mock-rigctld round-trip harness (no hardware, runs in CI) ----------
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A throwaway mock rigctld: binds an ephemeral port, accepts one connection,
    /// replies to each command line via `reply`, and records every command for
    /// assertions. Models the rigctl line protocol (`f`→freq, `F`/`M`/`T`→RPRT).
    fn mock_rigctld(
        reply: impl Fn(&str) -> String + Send + 'static,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let log_w = log.clone();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 256];
                loop {
                    let n = match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    for line in text.lines() {
                        log_w.lock().unwrap().push(line.to_string());
                        if stream.write_all(reply(line).as_bytes()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        (addr, log)
    }

    /// Healthy rig: `f`→`freq`, everything else→`RPRT 0`.
    fn ok_reply(freq: u64) -> impl Fn(&str) -> String + Send + 'static {
        move |line: &str| {
            if line.starts_with('f') {
                format!("{freq}\n")
            } else {
                "RPRT 0\n".to_string()
            }
        }
    }

    /// [`mock_rigctld`], but delivering each reply ONE LINE AT A TIME with `gap_ms` between
    /// them — the networked chain (rigctld → SmartSDR CAT → radio) that `command_inner`'s
    /// pre-command drain comment describes, made deterministic. A gap past
    /// `MULTILINE_TAIL_MS` is the case where the straggler misses its window entirely.
    /// Serves reconnections too, because dropping the stream is what an incomplete reply
    /// must do.
    fn mock_rigctld_split(
        gap_ms: u64,
        reply: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let log_w = log.clone();
        let reply = Arc::new(reply);
        std::thread::spawn(move || {
            // ⚠️ ONE THREAD PER CONNECTION, and it is not tidiness. A test here drops the stream
            // mid-reply on purpose, so the old connection is still mid-`gap_ms` sleep when the
            // client reconnects. Serving connections in sequence would make accepting that
            // reconnect wait out the sleep, and under a loaded test binary that wait is what
            // would turn this case into the very kind of flake it exists to kill.
            while let Ok((mut stream, _)) = listener.accept() {
                let (log_w, reply) = (log_w.clone(), reply.clone());
                std::thread::spawn(move || {
                    let mut buf = [0u8; 256];
                    loop {
                        let n = match stream.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        for line in text.lines() {
                            log_w.lock().unwrap().push(line.to_string());
                            for (i, part) in reply(line).split_inclusive('\n').enumerate() {
                                if i > 0 {
                                    std::thread::sleep(Duration::from_millis(gap_ms));
                                }
                                if stream.write_all(part.as_bytes()).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        });
        (addr, log)
    }

    /// **A SPLIT TWO-LINE REPLY MUST NOT POISON THE NEXT COMMAND** (CI flake, diagnosed
    /// 2026-09-21).
    ///
    /// [`Rig::command`]'s own header calls this a TX-safety invariant — "if a slow reply were
    /// left in the socket buffer, the next command would read it as its own answer … a keyed rig
    /// read as PTT ok". The guard installed for it, the pre-command drain, is NON-BLOCKING, so
    /// it discards only what has already arrived and a straggler still in flight walks straight
    /// past it. `read_mode_passband`'s own comment asserted the opposite ("the pre-command drain
    /// flushes the stray line so it never poisons the next command") and was wrong.
    ///
    /// Measured before the fix: `remote_read_level_value(Power)` got `raw=Some(2400.0)` — the
    /// passband of the preceding `m` — which the 0..=1 power range rejects as
    /// `ReadingUnavailable`. That is how it surfaced, as an FM-receiver test failing roughly one
    /// run in twenty.
    ///
    /// Both halves are asserted because either alone passes for the wrong reason: `m` must now
    /// GET its second line, and the next command must get ITS OWN answer.
    #[test]
    fn a_split_mode_reply_does_not_leak_its_passband_into_the_next_command() {
        let (addr, log) = mock_rigctld_split(5, |line| match line {
            "m" => "USB\n2400\n".to_string(),
            "l RFPOWER" => "0.500\n".to_string(),
            _ => "RPRT 0\n".to_string(),
        });
        let mut rig = Rig::with_control(Some(addr), PttMode::Cat);
        assert_eq!(
            rig.read_mode_passband(),
            (Some("USB".to_string()), Some(2400)),
            "both lines of a split reply belong to the `m` that asked for them"
        );
        assert_eq!(
            rig.read_meter_f32("RFPOWER"),
            Some(0.5),
            "the next command must read its OWN answer, not the stray passband"
        );
        assert_eq!(*log.lock().unwrap(), ["m", "l RFPOWER"]);
    }

    /// #381: on a link whose Hamlib driver SETS the power to answer a read of it, neither reader
    /// sends `l RFPOWER` — the heavy poll and the tune level read through `read_level`, the Remote
    /// level checks through `read_meter_f32` — while every other level still reads and a power
    /// the operator sets still goes out.
    #[test]
    fn a_refused_rf_power_read_sends_nothing_and_leaves_the_rest_of_the_radio_alone() {
        let (addr, log) = mock_rigctld(|line| match line {
            "l RFPOWER" | "l MICGAIN" => "0.500\n".to_string(),
            _ => "RPRT 0\n".to_string(),
        });
        let mut rig = Rig::with_control(Some(addr), PttMode::Vox);
        rig.set_never_send(&[(HamlibVerb::GetLevel, "RFPOWER")]);
        assert!(rig.read_level("RFPOWER").is_err(), "read_level refuses");
        assert_eq!(
            rig.read_meter_f32("RFPOWER"),
            None,
            "the raw reader refuses"
        );
        assert_eq!(
            rig.read_level("MICGAIN").ok(),
            Some(0.5),
            "other levels read"
        );
        rig.set_power(0.8).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["l MICGAIN", "L RFPOWER 0.800"],
            "no power read on the wire, and the operator's power write went out"
        );
    }

    /// The refusal matches the VERB and the whole TOKEN of a line, in either spelling — and
    /// nothing else. `l RF` is RF gain, not `l RFPOWER`; a read refusal does not block the write;
    /// a Sub-receiver line names `Sub`, not the level.
    #[test]
    fn a_refusal_matches_the_verb_and_the_whole_token_only() {
        use HamlibVerb::*;
        assert_eq!(
            HamlibVerb::of_line("l RFPOWER\n"),
            Some((GetLevel, "RFPOWER"))
        );
        assert_eq!(
            HamlibVerb::of_line("\\set_func MON 1\n"),
            Some((SetFunc, "MON"))
        );
        assert_eq!(
            HamlibVerb::of_line("L Sub RF 0.5\n"),
            Some((SetLevel, "Sub"))
        );
        assert_eq!(HamlibVerb::of_line("f\n"), None);
        assert_eq!(HamlibVerb::of_line("T 1\n"), None);
        let mut rig = Rig::with_control(Some("127.0.0.1:1".into()), PttMode::Vox);
        rig.set_never_send(&[(GetLevel, "RFPOWER"), (SetFunc, "MON")]);
        assert!(rig.refused_line("l RFPOWER\n").is_some());
        assert!(rig.refused_line("\\get_level RFPOWER\n").is_some());
        assert!(rig.refused_line("U MON 1\n").is_some());
        assert!(
            rig.refused_line("l RF\n").is_none(),
            "RF gain is not RF power"
        );
        assert!(
            rig.refused_line("L RFPOWER 0.8\n").is_none(),
            "the write is not refused"
        );
        assert!(
            rig.refused_line("u MON\n").is_none(),
            "nor the read of a refused write"
        );
    }

    /// A refused func write is a REFUSAL: nothing went out, so nothing is acting on it. The ATU
    /// path stands an FT QSO down on `Ok` and on `Uncertain`, never on `Refused` — so calling a
    /// line that never left `Uncertain` would stand a QSO down for a tune-up that never started.
    #[test]
    fn a_refused_func_write_is_a_refusal_not_an_uncertain_write() {
        let (addr, log) = mock_rigctld(|_| "RPRT 0\n".to_string());
        let mut rig = Rig::with_control(Some(addr), PttMode::Vox);
        rig.set_never_send(&[(HamlibVerb::SetFunc, "TUNER")]);
        assert!(matches!(
            rig.set_func_value("TUNER", ATU_START_TUNE),
            FuncSet::Refused(_)
        ));
        assert!(matches!(rig.set_func_value("VOX", 1), FuncSet::Ok));
        assert_eq!(
            *log.lock().unwrap(),
            ["U VOX 1"],
            "only the allowed line went out"
        );
    }

    /// **AND WHEN THE STRAGGLER MISSES ITS WINDOW, THE SOCKET IS DIRTY** (CI flake, diagnosed
    /// 2026-09-22 — the one the fix above did not close).
    ///
    /// The tail window bounds how long a multi-line verb waits for its last line, and then
    /// returns the lines that DID arrive. That fallback was correct about the reply and wrong
    /// about the CONNECTION: it returned `Ok`, so it skipped the drop that
    /// [`Rig::command`]'s header makes a TX-safety invariant ("if a slow reply were left in
    /// the socket buffer, the next command would read it as its own answer … a keyed rig read
    /// as PTT ok"), and the straggler arrived into a socket nobody was going to clean. The
    /// pre-command drain is non-blocking, so it catches the straggler only if it has already
    /// landed — a race, not a guard.
    ///
    /// The width here is `0`, which is not a contrivance: it is rigctld's own answer for the
    /// rig's default filter, and [`parse_mode_passband`] documents treating it as no-value.
    /// That makes the leaked line `0`, and `0` is exactly what `t` (get_ptt) answers for NOT
    /// KEYED — so the stale line does not merely fail to parse, it parses as the wrong
    /// transmit state on a rig that is keying. That is the named invariant, not an analogy.
    ///
    /// In the wild this reached an operator as `Reason::ReadingUnavailable` on an FM receiver
    /// adjustment: the leaked width landed on the next verb and `read_ptt`/`read_split`
    /// answered `None`, which the Remote radio owner refuses with.
    #[test]
    fn a_straggler_past_the_tail_window_never_answers_the_next_command() {
        let (addr, log) = mock_rigctld_split(250, |line| match line {
            // Two lines, the second far past MULTILINE_TAIL_MS.
            "m" => "USB\n0\n".to_string(),
            // The rig IS keying. Reading `0` here instead would be the desync.
            "t" => "1\n".to_string(),
            _ => "RPRT 0\n".to_string(),
        });
        let mut rig = Rig::with_control(Some(addr), PttMode::Cat);
        assert_eq!(
            rig.read_mode_passband(),
            (Some("USB".to_string()), None),
            "the fallback still returns the lines that DID arrive"
        );
        assert_eq!(
            rig.read_ptt(),
            Some(true),
            "the next command must read its OWN answer, never the stale width"
        );
        assert_eq!(
            *log.lock().unwrap(),
            ["m", "t"],
            "and it must ask the rig once, not paper over the leak with a retry"
        );
    }

    /// The same rule for the OTHER multi-line reader. `command_multiline` ends a `\dump_state`
    /// when the daemon goes quiet for a read window — that break means "finished" and leaves
    /// nothing behind. Its 2 s deadline is a different break with the opposite meaning: the
    /// daemon is STILL SENDING, and the remainder of a ~40-line dump would otherwise answer
    /// whatever we ask next. A dump line is a bare integer, so it parses as a frequency, a PTT
    /// state or a level just as readily as the real answer would.
    #[test]
    fn a_dump_that_outlasts_its_deadline_does_not_answer_the_next_command() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            // One thread per connection, for the reason `mock_rigctld_split` spells out: the
            // abandoned dump is still running when the client reconnects, and that reconnect
            // must not have to wait for it.
            while let Ok((mut stream, _)) = listener.accept() {
                std::thread::spawn(move || {
                    let mut buf = [0u8; 256];
                    loop {
                        let n = match stream.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        for line in String::from_utf8_lossy(&buf[..n]).to_string().lines() {
                            if line.starts_with("\\dump_state") {
                                // Never goes quiet and never ends, so the 2 s deadline is what
                                // stops the read — with the rest of the dump still coming.
                                for _ in 0..200 {
                                    if stream.write_all(b"1\n").is_err() {
                                        return;
                                    }
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                            } else if stream.write_all(b"14074000\n").is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });
        let mut rig = Rig::with_control(Some(addr), PttMode::Cat);
        // The dump is unparseable, which is fine and not what this asserts.
        let _ = rig.read_rx_ranges();
        assert_eq!(
            rig.read_freq().ok(),
            Some(14_074_000),
            "the next command must read its OWN answer, not the tail of an abandoned dump"
        );
    }

    /// The other direction, and it is what keeps the fix from costing a deadline on every
    /// refusal: an `RPRT` line is rigctld's WHOLE answer, carrying none of the payload lines a
    /// success would. A two-line verb must return on it at once rather than wait out the tail
    /// window for a line that is never coming.
    #[test]
    fn a_refused_two_line_verb_returns_at_once_rather_than_waiting_for_a_second_line() {
        let (addr, _log) = mock_rigctld(|line| match line {
            "m" => "RPRT -1\n".to_string(),
            _ => "RPRT 0\n".to_string(),
        });
        let mut rig = Rig::with_control(Some(addr), PttMode::Cat);
        let started = std::time::Instant::now();
        assert_eq!(rig.read_mode_passband(), (None, None));
        assert!(
            started.elapsed() < Duration::from_millis(80),
            "a refusal must not wait out the multi-line tail window: {:?}",
            started.elapsed()
        );
    }

    /// The single-line path must stay EXACTLY the pre-change condition — "ends with a newline"
    /// and nothing more. Every verb but `m` and `s` comes through it, keying included, so a new
    /// way for it to say "not yet" would be a new way for CAT to fail. The bare-newline row is
    /// the one that a non-empty-line count would silently change.
    #[test]
    fn counting_lines_never_changes_the_answer_for_a_single_line_verb() {
        for body in ["RPRT 0\n", "14074000\n", "\n", "0.500\n", "USB\n2400\n"] {
            assert_eq!(
                reply_complete(body.as_bytes(), 1),
                body.ends_with('\n'),
                "single-line completion must be the old newline test: {body:?}"
            );
        }
        assert!(!reply_complete(b"14074000", 1), "no newline, not complete");
        // Two-line verbs are the only ones that count, and an RPRT ends them early.
        assert!(!reply_complete(b"USB\n", 2), "one of two lines is not done");
        assert!(reply_complete(b"USB\n2400\n", 2), "both lines present");
        assert!(
            reply_complete(b"RPRT -1\n", 2),
            "a refusal is the whole reply"
        );
    }

    /// EINTR IS NOT "THE PEER WENT QUIET". A signal can cut a blocking read short before it
    /// has waited at all, and both reply loops used to treat that as a hard error: the
    /// single-line one returned it (and the caller dropped the stream), the multi-line one
    /// would have ended the reply early. On a busy machine that is a CAT failure with no cause
    /// an operator could ever find. CI hit exactly this on 2026-08-21.
    ///
    /// Both directions, because a classifier proven one way is half a test: the transient kinds
    /// must retry, and the genuinely broken ones must NOT — retrying a reset connection forever
    /// is the opposite failure and it hangs the radio loop instead of erroring.
    #[test]
    fn eintr_is_retried_but_a_broken_stream_is_not() {
        use std::io::ErrorKind::*;
        for k in [WouldBlock, TimedOut, Interrupted] {
            assert!(
                read_should_retry(k),
                "{k:?} means nothing was read — retry it"
            );
        }
        for k in [
            ConnectionReset,
            ConnectionAborted,
            BrokenPipe,
            UnexpectedEof,
            NotConnected,
            PermissionDenied,
        ] {
            assert!(
                !read_should_retry(k),
                "{k:?} is a broken stream — must NOT retry"
            );
        }
    }

    #[test]
    fn slow_fragmented_reply_still_reads_whole_line() {
        // SmartSDR CAT chains answer slower than a local rigctld and TCP can
        // fragment: 700 ms in, byte-at-a-time. The old single-500ms-read
        // returned "" here (operator report: green connect, dead control).
        // This is the NETWORK transport, so the long 2.5 s command deadline
        // applies (a serial rig would use the short one) — mark it slow.
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            use std::io::Read as _;
            let _ = sock.read(&mut buf); // consume the "f\n"
            std::thread::sleep(std::time::Duration::from_millis(700));
            for b in b"14074000\n" {
                let _ = sock.write_all(&[*b]);
                let _ = sock.flush();
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        });
        let mut rig = Rig::rigctld(&addr.to_string());
        rig.set_slow_transport(true); // network chain → long deadline
        assert_eq!(rig.read_freq().expect("whole reply assembled"), 14_074_000);
    }

    #[test]
    fn a_nul_terminated_raw_cat_reply_is_read_instead_of_timing_out() {
        // THE BUG THIS PINS, and it was silent in three places at once. rigctld's `w` (send_cmd)
        // returns the RIG's own reply, terminated by a NUL and no newline — measured against
        // Hamlib 4.7.0 driving an FT-710: `w MD0;` → "MD02;\0". `command_inner` returned only on
        // `\n`, so a raw read burned its whole deadline, errored, and dropped the connection.
        //
        // Consequences, all invisible: `raw_mode_query` (13 Yaesu models — the mode ground truth
        // that Hamlib's cached `m` cannot give) never once succeeded, and the FT-710 RF scope could
        // never learn its span or sweep mode, so it published nothing and reported "the radio is
        // not sending a spectrum" while the radio was sending one (station, 2026-08-19).
        //
        // The fake daemon here answers EXACTLY as the real one does — no newline anywhere — so the
        // test fails on the old code by timing out rather than by comparing strings.
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = sock.read(&mut buf); // consume "w MD0;\n"
            let _ = sock.write_all(b"MD02;\0"); // NUL-terminated, NO newline — the real shape
        });
        let mut rig = Rig::rigctld(&addr.to_string());
        let started = std::time::Instant::now();
        let reply = rig.send_raw("MD0;");
        // The NUL must be stripped, and the rig's own `;` must NOT be — callers parse the rig's
        // string. `MD02;` is USB on a Yaesu.
        assert_eq!(reply.as_deref(), Some("MD02;"));
        // And it must return promptly rather than after the deadline: the old code "worked" only in
        // the sense that it eventually gave up, and each give-up dropped the CAT connection. 700 ms
        // is the serial deadline; anything near it means we are still waiting for a newline.
        assert!(
            started.elapsed() < std::time::Duration::from_millis(400),
            "returned after {:?} — that is a timeout, not a read",
            started.elapsed()
        );
    }

    /// A rigctld that answers each line `delay` after it arrives, or never (`None`), with
    /// `reply(line)`. One connection after another, since a reply missed by its deadline drops
    /// the connection.
    fn mock_rigctld_late(
        delay: Option<Duration>,
        reply: impl Fn(&str) -> String + Send + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 256];
                'conn: loop {
                    let n = match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    for line in String::from_utf8_lossy(&buf[..n]).lines() {
                        let Some(delay) = delay else { continue };
                        std::thread::sleep(delay);
                        if stream.write_all(reply(line).as_bytes()).is_err() {
                            break 'conn;
                        }
                    }
                }
            }
        });
        addr
    }

    /// The tune-time meter read ENDS at its deadline. A tune carrier plays from a 250 ms lead, so
    /// the meter read made under it gets 120 ms and is abandoned past that. Each socket read used
    /// to wait its whole 500 ms window before the deadline was looked at: a rig that never
    /// answered held the loop 500 ms, and a reply at 300 ms was taken.
    #[test]
    fn a_tune_time_meter_read_ends_at_its_deadline() {
        let meters = |line: &str| {
            if line.starts_with("w ") {
                "RM4128;\0".to_string()
            } else {
                "1.5\n".to_string()
            }
        };
        let late = || Some(Duration::from_millis(300));
        let timed = |f: &mut dyn FnMut() -> Option<String>| {
            let started = std::time::Instant::now();
            (f(), started.elapsed())
        };
        let mut silent = Rig::rigctld(&mock_rigctld_late(None, meters));
        let (silent_value, silent_took) = timed(&mut || {
            silent
                .read_meter_f32_within("SWR", Some(120))
                .map(|v| v.to_string())
        });
        let mut slow = Rig::rigctld(&mock_rigctld_late(late(), meters));
        let (late_value, late_took) = timed(&mut || {
            slow.read_meter_f32_within("SWR", Some(120))
                .map(|v| v.to_string())
        });
        let mut slow_raw = Rig::rigctld(&mock_rigctld_late(late(), meters));
        let (raw_value, raw_took) = timed(&mut || slow_raw.send_raw_within("RM4;", Some(120)));
        assert_eq!(
            (silent_value, late_value, raw_value),
            (None, None, None),
            "a meter answered after the 120 ms deadline is a miss, never a reading"
        );
        for (what, took) in [
            ("no answer", silent_took),
            ("answer at 300 ms", late_took),
            ("raw answer at 300 ms", raw_took),
        ] {
            assert!(
                took < Duration::from_millis(400),
                "{what}: the 120 ms read held the loop {took:?}"
            );
        }
        // Controls: the same 300 ms answers are read under the transport's own deadline.
        let mut normal = Rig::rigctld(&mock_rigctld_late(late(), meters));
        assert_eq!(normal.read_meter_f32("SWR"), Some(1.5));
        let mut normal_raw = Rig::rigctld(&mock_rigctld_late(late(), meters));
        assert_eq!(normal_raw.send_raw("RM4;").as_deref(), Some("RM4128;"));
    }

    /// PTT keeps its deadline exactly as it was: looked at between reads, so a `T 1` answered at
    /// 800 ms (past the 700 ms PTT deadline, inside the second read) is still taken. Only the
    /// tune-time meter reads end at their deadline.
    #[test]
    fn a_key_answered_after_the_ptt_deadline_is_still_taken_as_before() {
        let mut rig = Rig::rigctld(&mock_rigctld_late(Some(Duration::from_millis(800)), |_| {
            "RPRT 0\n".to_string()
        }));
        assert_eq!(rig.ptt(true).map_err(|e| e.to_string()), Ok(()));
    }

    #[test]
    fn a_newline_terminated_reply_still_works_and_keeps_its_own_shape() {
        // The other half of the guard: accepting NUL must not have broken the ordinary path, and a
        // reply containing NO nul must still be read on its newline. Without this, the fix above
        // could have been written as "return on NUL" and passed its own test.
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 64];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(b"14074000\n");
        });
        let mut rig = Rig::rigctld(&addr.to_string());
        assert_eq!(
            rig.read_freq().expect("newline reply still read"),
            14_074_000
        );
    }

    #[test]
    fn timed_out_reply_errors_and_drops_stream_so_the_next_command_cannot_desync() {
        // The C3 desync: a reply that lands after the 2.5 s deadline must NOT be
        // left in the socket for the next command to read as its own answer.
        // Command 1's reply arrives late (past the deadline) → the command must
        // error AND drop the stream; command 2 reconnects and reads its OWN
        // fresh reply, never the stale one.
        //
        // Every command but a KEY line still drops the connection on a timeout, which is what
        // this pins. A key line (`T …`) no longer does: dropping it left the late key live inside
        // rigctld while its release went out on a new connection and could be carried out first,
        // leaving the radio keyed. A late key's answer is counted as owed and read before the
        // next command's, so the same desync stays impossible on a connection kept open — see
        // `a_late_key_and_its_release_arrive_on_one_connection_in_order` and
        // `a_late_key_reply_is_consumed_never_read_as_the_next_answer`.
        use std::io::{Read as _, Write as _};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let n = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let mut sock = match conn {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let idx = n.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut buf = [0u8; 64];
                    let _ = sock.read(&mut buf); // consume the command line
                    if idx == 0 {
                        // Reply LATE — past the 2.5 s deadline. The client has
                        // already abandoned this command; this write is stale.
                        std::thread::sleep(std::time::Duration::from_millis(2_800));
                        let _ = sock.write_all(b"RPRT 0\n");
                    } else {
                        let _ = sock.write_all(b"14074000\n"); // fresh, immediate
                    }
                });
            }
        });
        let mut rig = Rig::rigctld(&addr.to_string());
        // Slow reply must be an ERROR, never a silent success.
        assert!(
            rig.set_mode("USB", 0).is_err(),
            "reply past the 2.5 s deadline must error, not succeed"
        );
        // Stream was dropped; the next command reconnects and reads its OWN reply.
        assert_eq!(
            rig.read_freq().expect("reconnect + fresh reply"),
            14_074_000,
            "next command must read its own reply, not the stale RPRT 0"
        );
    }

    /// What [`mock_rigctld_paced`] carried out: `(connection, line)`, in order.
    type CarriedOut = Vec<(usize, String)>;

    /// A rigctld that serves each connection on its own thread and, as rigctld does, ONE LINE AT A
    /// TIME in the order it arrived: `answer(line)` says how long that line takes and what it
    /// answers, or `None` for a line it never answers. The log is `(connection, line)` in the order
    /// the lines were CARRIED OUT, which is the order the radio sees them.
    fn mock_rigctld_paced(
        answer: impl Fn(&str) -> Option<(Duration, String)> + Send + Sync + 'static,
    ) -> (String, Arc<Mutex<CarriedOut>>) {
        use std::io::BufRead;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = Arc::new(Mutex::new(Vec::<(usize, String)>::new()));
        let log_w = log.clone();
        let answer = Arc::new(answer);
        std::thread::spawn(move || {
            for (conn, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { break };
                let (log_w, answer) = (log_w.clone(), answer.clone());
                std::thread::spawn(move || {
                    let Ok(reader) = stream.try_clone() else {
                        return;
                    };
                    for line in std::io::BufReader::new(reader).lines() {
                        let Ok(line) = line else { return };
                        let Some((delay, reply)) = answer(&line) else {
                            log_w.lock().unwrap().push((conn, line));
                            continue;
                        };
                        std::thread::sleep(delay);
                        log_w.lock().unwrap().push((conn, line));
                        if stream.write_all(reply.as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (addr, log)
    }

    /// Wait (bounded) for the mock's log to reach `n` lines carried out.
    fn wait_for_lines(log: &Mutex<CarriedOut>, n: usize) -> CarriedOut {
        let until = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let seen = log.lock().unwrap().clone();
            if seen.len() >= n || std::time::Instant::now() >= until {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// ⛔ A LATE KEY IS NEVER OVERTAKEN BY ITS RELEASE. rigctld can hold a `T 1` for seconds (a
    /// mode write in front of it waiting out Hamlib's timeout, with rigctld's one client lock held).
    /// The key used to miss its 700 ms deadline and DROP the connection with the `T 1` still live
    /// inside rigctld; the release then went out on a NEW connection, whose thread rigctld's lock
    /// does not serve in arrival order, so the release could run first and leave the radio keyed
    /// after the operator let go. The release must go out behind the key on the SAME connection,
    /// which rigctld carries out in order, whatever its lock does.
    #[test]
    fn a_late_key_and_its_release_arrive_on_one_connection_in_order() {
        let (addr, log) = mock_rigctld_paced(|line| {
            let delay = if line == "T 1" { 1_500 } else { 0 };
            Some((Duration::from_millis(delay), "RPRT 0\n".to_string()))
        });
        let mut rig = Rig::rigctld(&addr);
        let key = rig.ptt(true);
        assert!(
            key.as_ref().is_err_and(deadline_passed),
            "a key answered at 1.5 s misses the 700 ms deadline: {key:?}"
        );
        assert!(rig.keyed, "a key-down attempt is keyed until released");
        // The release queues behind the key, so its answer comes once the key's has.
        let release = rig.ptt(false);
        assert_eq!(release.map_err(|e| e.to_string()), Ok(()));
        assert!(!rig.keyed, "a confirmed release clears keyed");
        assert_eq!(
            wait_for_lines(&log, 2),
            vec![(0, "T 1".to_string()), (0, "T 0".to_string())],
            "the key and its release on one connection, the key carried out first"
        );
    }

    /// A late key's answer is the key's, never the next command's: a read made while it is still
    /// owed sends nothing and says it got no answer in time (so the radio loop still counts a
    /// silent link), and once the key's answer is in, the next command reads its own. The late
    /// answer here is a refusal, so a release that took it for its own would fail.
    #[test]
    fn a_late_key_reply_is_consumed_never_read_as_the_next_answer() {
        let (addr, log) = mock_rigctld_paced(|line| {
            Some(match line {
                "T 1" => (Duration::from_millis(2_500), "RPRT -1\n".to_string()),
                "f" => (Duration::ZERO, "14074000\n".to_string()),
                _ => (Duration::ZERO, "RPRT 0\n".to_string()),
            })
        });
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.ptt(true).is_err());
        let early = rig.read_freq();
        assert!(
            early.as_ref().is_err_and(no_answer_in_time),
            "a read while the key's answer is owed is no answer in time: {early:?}"
        );
        wait_for_lines(&log, 1);
        assert_eq!(rig.read_freq().map_err(|e| e.to_string()), Ok(14_074_000));
        assert_eq!(rig.ptt(false).map_err(|e| e.to_string()), Ok(()));
        assert!(!rig.keyed);
        assert_eq!(
            wait_for_lines(&log, 3),
            vec![
                (0, "T 1".to_string()),
                (0, "f".to_string()),
                (0, "T 0".to_string())
            ],
            "the early read never went out; everything on the key's connection"
        );
    }

    /// Negative control: keeping the connection after a late key does not keep a DEAD one. A
    /// rigctld that closes the connection under the owed key fails the release, which leaves the
    /// rig keyed (so every unkey path keeps trying), and a later release reconnects and is
    /// confirmed.
    #[test]
    fn a_dead_socket_after_a_late_key_still_unkeys_until_confirmed() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let conns = Arc::new(AtomicUsize::new(0));
        let conns_w = conns.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let idx = conns_w.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut buf = [0u8; 64];
                    if idx == 0 {
                        // Take the key, never answer it, and go away.
                        let _ = stream.read(&mut buf);
                        std::thread::sleep(Duration::from_millis(900));
                        return;
                    }
                    while let Ok(n) = stream.read(&mut buf) {
                        if n == 0 || stream.write_all(b"RPRT 0\n").is_err() {
                            return;
                        }
                    }
                });
            }
        });
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.ptt(true).is_err());
        std::thread::sleep(Duration::from_millis(300)); // the key's connection is gone by now
        let mut releases = Vec::new();
        for _ in 0..3 {
            let r = rig.ptt(false);
            releases.push(r.is_ok());
            assert_eq!(rig.keyed, r.is_err(), "keyed until a release is confirmed");
            if r.is_ok() {
                break;
            }
        }
        assert_eq!(
            releases.last(),
            Some(&true),
            "a release is confirmed: {releases:?}"
        );
        assert!(conns.load(Ordering::SeqCst) >= 2, "it reconnected");
    }

    /// Negative control: a rigctld that never answers anything is still a silent link. Every
    /// command fails with no answer in time, each by its own deadline, a release nobody confirmed
    /// leaves the rig keyed, and dropping the rig still sends the release, on the key's
    /// connection.
    #[test]
    fn a_rigctld_that_never_answers_is_still_a_silent_link_and_still_unkeyed_on_drop() {
        let (addr, log) = mock_rigctld_paced(|_| None);
        let mut rig = Rig::rigctld(&addr);
        let started = std::time::Instant::now();
        assert!(rig.ptt(true).is_err_and(|e| deadline_passed(&e)));
        for _ in 0..2 {
            let read = rig.read_freq();
            assert!(read.is_err_and(|e| no_answer_in_time(&e)));
        }
        assert!(rig.ptt(false).is_err());
        assert!(rig.keyed, "an unconfirmed release leaves the rig keyed");
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "every command ended by its deadline: {:?}",
            started.elapsed()
        );
        drop(rig);
        assert_eq!(
            wait_for_lines(&log, 3),
            vec![
                (0, "T 1".to_string()),
                (0, "T 0".to_string()),
                (0, "T 0".to_string())
            ],
            "the reads never went out; the release and the drop's release follow the key"
        );
    }

    /// ⛔ A CONNECTION ONE ANSWER BEHIND FOR GOOD IS GIVEN UP, BUT ONLY ONCE THE RELEASE IS QUEUED. A
    /// peer that loses the answer to one key leaves Nexus one answer behind for good: each release
    /// takes its own `RPRT 0` for the lost one and reads as unconfirmed, the rig reads as keyed while
    /// the radio is not, and every read waits on an answer that never comes. Once a release has gone
    /// out behind the key and the bound has passed, the connection is given up and the next release,
    /// on a new one, is confirmed. Never before the release is queued: a read past the bound with
    /// only the key queued still sends nothing and opens nothing.
    #[test]
    fn a_lost_key_answer_is_given_up_once_its_release_is_queued() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let first = Arc::new(AtomicBool::new(true));
        let lose = first.clone();
        let (addr, log) = mock_rigctld_paced(move |line| {
            if line == "T 1" && lose.swap(false, Ordering::SeqCst) {
                return None; // the radio keyed; its answer is lost
            }
            Some(match line {
                "f" => (Duration::ZERO, "14074000\n".to_string()),
                _ => (Duration::ZERO, "RPRT 0\n".to_string()),
            })
        });
        let mut rig = Rig::rigctld(&addr);
        // Longer than one release's own wait and shorter than two, so only answers owed without a
        // break across several releases reach it.
        rig.owed_bound = Duration::from_millis(1_500);
        assert!(rig.ptt(true).is_err());
        std::thread::sleep(Duration::from_millis(1_600));
        assert!(
            rig.read_freq().is_err_and(|e| no_answer_in_time(&e)),
            "past the bound with no release queued, nothing is given up"
        );
        assert_eq!(
            log.lock().unwrap().clone(),
            vec![(0, "T 1".to_string())],
            "the read went out, or a new connection opened, before the release was queued"
        );
        // The release goes out behind the lost answer and takes its own answer for it.
        assert!(rig.ptt(false).is_err(), "premise: one answer behind");
        assert!(rig.keyed);
        let mut tries = 0;
        while rig.keyed && tries < 5 {
            let _ = rig.ptt(false);
            tries += 1;
        }
        assert!(
            !rig.keyed,
            "still keyed after {tries} releases past the bound"
        );
        assert_eq!(rig.read_freq().map_err(|e| e.to_string()), Ok(14_074_000));
        let log = wait_for_lines(&log, 4);
        assert_eq!(
            log[..2],
            [(0, "T 1".to_string()), (0, "T 0".to_string())],
            "the key's own connection carried the release behind it: {log:?}"
        );
        assert!(
            log[2..].iter().all(|(conn, _)| *conn > 0),
            "everything after the give-up went out on a new connection: {log:?}"
        );
    }

    /// A release behind an owed key is judged on its OWN answer: the key's late `RPRT 0` is set
    /// aside, so a release the rig refuses is a refusal and leaves the rig keyed. Taking the key's
    /// answer for the release's is the "rejected T read as success" desync.
    #[test]
    fn a_release_behind_an_owed_key_is_judged_on_its_own_answer() {
        let (addr, log) = mock_rigctld_paced(|line| {
            Some(match line {
                "T 1" => (Duration::from_millis(1_300), "RPRT 0\n".to_string()),
                _ => (Duration::ZERO, "RPRT -9\n".to_string()),
            })
        });
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.ptt(true).is_err());
        let release = rig.ptt(false);
        assert!(
            release.as_ref().is_err_and(|e| !no_answer_in_time(e)),
            "the key's RPRT 0 was read as the release's answer: {release:?}"
        );
        assert!(rig.keyed, "a refused release cleared keyed");
        assert_eq!(
            wait_for_lines(&log, 2),
            vec![(0, "T 1".to_string()), (0, "T 0".to_string())]
        );
    }

    /// ⛔ A STOP IS NEVER HELD BACK BY AN OWED KEY. Stop TX sends the CAT keyer's stop and a
    /// radio's own CW playback stop with the unkey; with a late key's answer owed they waited it out
    /// and were then not sent at all. They go out behind the key at once, as the release does, and
    /// a raw stop's NUL-terminated answer is counted like a newline one, so the release after them
    /// still reads its own answer.
    #[test]
    fn a_stop_goes_out_behind_an_owed_key_and_is_never_held_back() {
        let (addr, log) = mock_rigctld_paced(|line| {
            Some(match line {
                "T 1" => (Duration::from_millis(4_000), "RPRT 0\n".to_string()),
                l if l.starts_with("W ") => (Duration::ZERO, "\0".to_string()),
                _ => (Duration::ZERO, "RPRT 0\n".to_string()),
            })
        });
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.ptt(true).is_err());
        // Both miss their deadlines behind the 4 s key, so both answers are owed too.
        let _ = rig.stop_morse();
        let _ = rig.send_raw_stop("KY00;");
        assert_eq!(
            wait_for_lines(&log, 3),
            vec![
                (0, "T 1".to_string()),
                (0, "\\stop_morse".to_string()),
                (0, "W KY00; 0".to_string())
            ],
            "a stop was held back or went out on another connection"
        );
        assert_eq!(rig.ptt(false).map_err(|e| e.to_string()), Ok(()));
        assert!(!rig.keyed);
    }

    /// On a network chain a key-down that waits for the link waits the network window: a `T 1`
    /// answered at 1.2 s is a key, not "no answer in time". The release keeps its short deadline,
    /// which is why it has one. Controls: the same answer on a local link still misses the key
    /// deadline, and so does a slot over's key (`ptt`), whose deadline is part of its timing.
    #[test]
    fn a_network_chain_key_waits_the_network_window() {
        let late =
            || mock_rigctld_paced(|_| Some((Duration::from_millis(1_200), "RPRT 0\n".to_string())));
        let (net_addr, _) = late();
        let mut net = Rig::rigctld(&net_addr);
        net.set_network_chain(true);
        assert_eq!(
            net.key_awaiting_answer(false).map_err(|e| e.to_string()),
            Ok(())
        );
        assert!(
            net.ptt(false).is_err_and(|e| deadline_passed(&e)),
            "the release keeps the short deadline on a network chain"
        );
        let (local_addr, _) = late();
        let mut local = Rig::rigctld(&local_addr);
        assert!(local
            .key_awaiting_answer(false)
            .is_err_and(|e| deadline_passed(&e)));
        let (slot_addr, _) = late();
        let mut slot = Rig::rigctld(&slot_addr);
        slot.set_network_chain(true);
        assert!(slot.ptt(true).is_err_and(|e| deadline_passed(&e)));
    }

    #[test]
    fn read_freq_parses_the_dial_over_tcp() {
        let (addr, log) = mock_rigctld(ok_reply(14_074_000));
        let mut rig = Rig::rigctld(&addr);
        assert_eq!(rig.read_freq().unwrap(), 14_074_000);
        assert_eq!(log.lock().unwrap().as_slice(), &["f".to_string()]);
    }

    /// ⭐ A FREQUENCY IN HAMLIB'S `%lf` FORM IS A FREQUENCY. wfview's rigctld answers `f` and `i`
    /// with `14074000.000000` (its `src/rigctld.cpp`, 2.23; those are the bytes below), which
    /// Hamlib's own NET client reads as a double. Nexus read it as no frequency at all, "rig did
    /// not return a frequency", so the open's first read failed and sharing wfview's rigctld never
    /// connected. The control: `0.000000`, like a whole `0`, is still no frequency.
    #[test]
    fn a_frequency_in_hamlibs_lf_form_is_read() {
        let (addr, _) = mock_rigctld(|line| match line.trim() {
            "f" => "14074000.000000\n".into(),
            "i" => "14076000.000000\n".into(),
            _ => "RPRT 0\n".into(),
        });
        let mut rig = Rig::rigctld(&addr);
        let (zero, _) = mock_rigctld(|_| "0.000000\n".into());
        assert_eq!(
            (
                rig.read_freq().ok(),
                rig.read_split_freq(),
                Rig::rigctld(&zero).read_freq().is_err(),
            ),
            (Some(14_074_000), Some(14_076_000), true)
        );
    }

    /// ⭐ THE READ THAT MAKES A/B SELECTION HONEST. `v` (get_vfo) is the one question Nexus
    /// never asked, which is why an A/B press on the rig's front panel was invisible to it.
    ///
    /// The reply shapes are verbatim from Hamlib 4.5.5 `rigctld -m 1` on this box: `v` answers
    /// a single bare VFO-name line.
    #[test]
    fn read_vfo_asks_the_rig_which_vfo_it_is_on() {
        let (addr, log) = mock_rigctld(|_| "VFOB\n".to_string());
        let mut rig = Rig::rigctld(&addr);
        assert_eq!(rig.read_vfo(), Some(true), "VFOB is B");
        assert_eq!(
            log.lock().unwrap().as_slice(),
            &["v".to_string()],
            "one `v`, and nothing that could move the radio"
        );

        let (addr, _) = mock_rigctld(|_| "VFOA\n".to_string());
        assert_eq!(Rig::rigctld(&addr).read_vfo(), Some(false), "VFOA is A");
    }

    /// ABSENT IS NOT "A". Every answer that is not plainly one of the two VFOs this indicator
    /// models must come back `None`, so the caller keeps the commanded value instead of
    /// inventing a selection.
    ///
    /// ⚠️ `Main`/`Sub` IS A REAL REPLY, not a hypothetical: Hamlib 4.5.5's own dummy backend,
    /// asked here on this box, answers `Sub` to `v` after being sent `V VFOB`. Translating it
    /// to B would be modelling a second receiver, which this change deliberately does not do —
    /// so such a rig degrades to exactly the old behaviour rather than to a wrong one.
    #[test]
    fn a_vfo_reply_that_is_not_plainly_a_or_b_changes_nothing() {
        for reply in ["Main\n", "Sub\n", "MEM\n", "currVFO\n", "RPRT -11\n", "\n"] {
            let (addr, _) = mock_rigctld(move |_| reply.to_string());
            assert_eq!(
                Rig::rigctld(&addr).read_vfo(),
                None,
                "{reply:?} must leave the indicator alone, not resolve to a VFO"
            );
        }
        // …and a rig with no CAT control channel is never asked at all.
        assert_eq!(Rig::vox().read_vfo(), None);
    }

    #[test]
    fn set_freq_mode_ptt_send_correct_lines() {
        let (addr, log) = mock_rigctld(ok_reply(7_074_000));
        let mut rig = Rig::rigctld(&addr);
        rig.set_freq(7_074_000).unwrap();
        rig.set_mode("USB", 0).unwrap();
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        assert_eq!(
            *log.lock().unwrap(),
            vec!["F 7074000", "M USB 0", "T 1", "T 0"]
        );
    }

    #[test]
    fn the_split_vfo_gets_its_own_mode_so_an_inverting_bird_can_transmit_lsb() {
        // `M` only ever reaches the RX VFO. A linear INVERTING satellite
        // transponder needs USB down and LSB up ON ONE RADIO, so the TX VFO's
        // mode has to be commanded separately — rigctld `X`.
        let (addr, log) = mock_rigctld(ok_reply(435_640_000));
        let mut rig = Rig::rigctld(&addr);
        rig.set_split(true, "VFOB").unwrap();
        rig.set_split_freq(145_965_000).unwrap();
        rig.set_split_mode("LSB", -1).unwrap();
        // A BLANK mode is the caller choosing to obey the radio — no command.
        rig.set_split_mode("  ", -1).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            vec!["S 1 VFOB", "I 145965000", "X LSB -1"]
        );
    }

    #[test]
    fn a_backend_without_split_mode_support_reports_it_instead_of_faking_success() {
        // Plenty of Hamlib backends answer `X` with RPRT -11 (not implemented).
        // Swallowing that would leave the uplink in the wrong sideband while the
        // app believed it had set it — on the air, indistinguishable from nobody
        // answering. The caller must be able to SAY so.
        let (addr, _log) = mock_rigctld(|line: &str| {
            if line.starts_with('X') {
                "RPRT -11\n".to_string()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.set_split(true, "VFOB").is_ok());
        assert!(
            rig.set_split_mode("LSB", -1).is_err(),
            "a refused split-mode must surface, never a silent success"
        );
    }

    #[test]
    fn cat_control_works_with_vox_ptt() {
        // The keystone decoupling (WSJT-X model): a CAT rig keyed by VOX still receives
        // freq + mode commands over the control channel — but PTT no-ops (VOX keys it).
        // This is exactly the case that was silently broken: CAT rig + non-CAT PTT meant
        // the rig never got an M/F command, so the mode never switched per section.
        let (addr, log) = mock_rigctld(ok_reply(14_074_000));
        let mut rig = Rig::with_control(Some(addr), PttMode::Vox);
        rig.set_freq(14_074_000).unwrap();
        rig.set_mode("PKTUSB", 0).unwrap();
        rig.ptt(true).unwrap(); // VOX → no T command sent, but state is tracked
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
        assert_eq!(*log.lock().unwrap(), vec!["F 14074000", "M PKTUSB 0"]);
    }

    #[test]
    fn cat_ptt_without_a_control_channel_degrades_to_vox() {
        // PttMode::Cat but no control configured must not panic or attempt a connection —
        // it degrades to VOX (no-op keying) so the engine still runs.
        let mut rig = Rig::with_control(None, PttMode::Cat);
        rig.ptt(true).unwrap();
        assert!(rig.keyed);
        rig.ptt(false).unwrap();
        assert!(!rig.keyed);
    }

    #[test]
    fn set_freq_errors_when_the_rig_refuses_the_frequency() {
        // ⭐ THE FTdx10 BUG, at its narrowest. `F` sent to a radio that cannot reach the frequency
        // (2 m on an HF-only rig) answers `RPRT -1`. set_freq used to `map(|_| ())` that away, so a
        // refusal was indistinguishable from success — and the whole wedge followed from there: the
        // radio loop advanced `last_dial` to a frequency the radio was never on, suppressed the
        // read-back that would have corrected it, and told the operator "CAT confirmed — rig
        // accepted a command".
        let (addr, _log) = mock_rigctld(|l| {
            if l.starts_with('F') {
                "RPRT -1\n".to_string()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let mut rig = Rig::rigctld(&addr);
        let err = rig.set_freq(144_390_000).unwrap_err();
        // Same kind contract as set_mode: `Other` = the RIG actively refused, as opposed to a
        // timeout / unreachable link. The loop's note wording depends on telling those apart.
        assert_eq!(err.kind(), std::io::ErrorKind::Other, "{err}");

        // An accepted frequency still returns Ok — the guard must not break ordinary QSYs.
        let (addr2, _l2) = mock_rigctld(ok_reply(14_074_000));
        let mut rig2 = Rig::rigctld(&addr2);
        assert!(rig2.set_freq(14_074_000).is_ok());

        // No CAT configured: still a silent no-op success (VOX rigs have no dial to set).
        assert!(Rig::vox().set_freq(144_390_000).is_ok());
    }

    #[test]
    fn dump_state_rx_ranges_parse_a_real_hamlib_capability_dump() {
        // Byte layout taken from Hamlib 4.7.1 `tests/rigctl_parse.c` dump_state: protocol version,
        // rig model, ITU region, then 7-field RX range rows terminated by an all-zero row, then the
        // TX rows. Frequencies print through FREQFMT (freq_t is a double).
        let ftdx10 = "1\n1035\n0\n\
                      30000.000000 60000000.000000 0x1ff -1 -1 0x3 0x3\n\
                      0 0 0 0 0 0 0\n\
                      1800000.000000 2000000.000000 0x1ff 5 100 0x3 0x3\n\
                      0 0 0 0 0 0 0\n\
                      0xffffffff 1\n0 0\n";
        let r = parse_dump_state_rx_ranges(ftdx10).expect("a real dump parses");
        assert_eq!(r, vec![(30_000, 60_000_000)]);
        // The whole point: 2 m is NOT covered, and 20 m is — without commanding the radio anywhere.
        assert!(
            !ranges_cover(&r, 144_390_000),
            "an HF/6 m rig cannot do 2 m"
        );
        assert!(ranges_cover(&r, 14_074_000));
        // TX ranges must not leak into the RX list (parsing must stop at the RX terminator).
        assert!(
            ranges_cover(&r, 30_000_000),
            "the RX list, not the ham-band TX list"
        );

        // A multi-range VHF/UHF rig (IC-9700 shape) — every band it really covers.
        let ic9700 = "0\n1035\n1\n\
                      144000000.000000 148000000.000000 0x1ff -1 -1 0x3 0x3\n\
                      430000000.000000 450000000.000000 0x1ff -1 -1 0x3 0x3\n\
                      0 0 0 0 0 0 0\n\
                      0 0 0 0 0 0 0\n";
        let r2 = parse_dump_state_rx_ranges(ic9700).unwrap();
        assert_eq!(r2.len(), 2);
        assert!(
            ranges_cover(&r2, 144_390_000),
            "the APRS channel is covered"
        );
        assert!(!ranges_cover(&r2, 14_074_000), "…but 20 m is not");
    }

    /// ⭐ THE ATTENUATOR AND PREAMP STEP LISTS COME OFF THE SAME `\dump_state` REPLY, and
    /// until this parser existed nothing read them: [`parse_dump_state_rx_ranges`] returns at
    /// the RX list's all-zero terminator, and the two db lists sit nine fields further down,
    /// past the TX ranges, the tuning steps, the filters and four scalars.
    ///
    /// ⚠️ THIS IS WHY A STEP LIST CANNOT BE A CONSTANT. An attenuator is not a slider; it is
    /// the handful of pads a PARTICULAR radio has, and the two rigs below disagree — 20 dB on
    /// one, 6/12/18 dB on the other. Offering a value the rig does not own gets it NAKed or
    /// silently rounded to a neighbour, which attenuates by an amount nobody asked for.
    ///
    /// The fixtures are REAL: `\dump_state` captured over TCP from Hamlib 4.5.5's own
    /// `rigctld -m 3073` / `-m 3078` driven against a pty, saved verbatim. Nothing in them is
    /// transcribed, which matters because the field ORDER is the whole parse.
    #[test]
    fn dump_state_carries_the_rigs_own_attenuator_and_preamp_steps() {
        let ic7300 = include_str!("../tests/fixtures/dump_state_ic7300.txt");
        let ic7610 = include_str!("../tests/fixtures/dump_state_ic7610.txt");

        let a = parse_dump_state_db_lists(ic7300).expect("a real dump parses");
        assert_eq!(a.attenuator_db, vec![20], "the IC-7300 has one 20 dB pad");
        assert_eq!(a.preamp_db, vec![1, 2], "…and Icom's two P.AMP selectors");

        let b = parse_dump_state_db_lists(ic7610).expect("a real dump parses");
        assert_eq!(b.attenuator_db, vec![6, 12, 18], "the IC-7610's three pads");
        assert_eq!(b.preamp_db, vec![12, 20], "…and its two preamps, in dB");

        // ⭐ THE LISTS DIFFER BETWEEN RIGS. Asserted as its own statement because a parser
        // that returned a fixed ladder, or that read the wrong line and happened to find a
        // plausible number, would satisfy either rig's expectations taken alone.
        assert_ne!(a.attenuator_db, b.attenuator_db);
        assert_ne!(a.preamp_db, b.preamp_db);

        // ⚠️ AND THE TWO LISTS ARE NOT INTERCHANGEABLE. They are adjacent lines, so an
        // off-by-one in the field walk reads one as the other — and on the IC-7300 both are
        // short, which is exactly where that would go unnoticed.
        assert_ne!(
            a.preamp_db, a.attenuator_db,
            "the preamp list is the line BEFORE the attenuator list"
        );
        assert_eq!(
            parse_dump_state_rx_ranges(ic7610).map(|r| r.len()),
            Some(1),
            "the RX ranges still parse off the same reply, unchanged"
        );

        // A rig that declares neither reads as EMPTY, which is a positive answer ("this radio
        // has no pad") and different from the `None` an unparseable reply gives.
        let bare = "1\n1035\n0\n\
                    30000.000000 60000000.000000 0x1ff -1 -1 0x3 0x3\n\
                    0 0 0 0 0 0 0\n\
                    0 0 0 0 0 0 0\n\
                    0 0\n0 0\n0\n0\n0\n0\n0\n0\n0x0\n0x0\n0x0\n0x0\n0x0\n0x0\n";
        let n = parse_dump_state_db_lists(bare).expect("a well-formed dump with no pads");
        assert!(n.attenuator_db.is_empty() && n.preamp_db.is_empty());

        // ⚠️ UNKNOWN IS NOT EMPTY. A reply we cannot positively walk must read `None` — the
        // same fail-open rule the range table has. An empty list HIDES the control; `None`
        // must too, and for the caller to make that choice it has to be able to tell them
        // apart. A truncated dump is the realistic case: the reply arrived short.
        assert_eq!(parse_dump_state_db_lists(""), None);
        assert_eq!(parse_dump_state_db_lists("RPRT -11\n"), None);
        assert_eq!(
            parse_dump_state_db_lists("1\n1035\n0\n0 0 0 0 0 0 0\n"),
            None,
            "the reply stopped before the lists"
        );
        // A protocol version whose layout we do not know could have moved the lists.
        let moved = ic7610.replacen("1\n", "9\n", 1);
        assert_eq!(parse_dump_state_db_lists(&moved), None);
    }

    /// ⚠️ THE TRAP, MADE EXECUTABLE: `ATT` AND `PREAMP` MUST NOT GO THROUGH THE ORDINARY
    /// LEVEL PATH. Every other level on this surface is a 0..1 fraction, so both the reader
    /// and the setter are built for one — and these two are INTEGER DECIBELS.
    ///
    /// [`Rig::read_level`] filters its reading to `0.0..=1.0`, so a 12 dB pad is not clamped
    /// to 1.0, it is REJECTED: the read returns an error, three of those latch the level
    /// unsupported, and the control vanishes. And it fails ASYMMETRICALLY, which is what
    /// makes it nasty — `0` (the pad switched off) is inside the range and reads back fine,
    /// so the control works right up until the operator uses it.
    ///
    /// [`Rig::set_rx_level`] is the mirror image: it clamps to 0..1 and prints `{:.3}`, so
    /// `set_rx_level("ATT", 12.0)` puts `L ATT 1.000` on the wire — a pad no rig has.
    #[test]
    fn the_decibel_levels_use_the_raw_reader_and_an_integer_setter() {
        // A rigctld answering like a real one: `l ATT` -> "12", `l PREAMP` -> "20".
        let (addr, log) = mock_rigctld(|line: &str| {
            if line.starts_with("l ATT") {
                "12\n".to_string()
            } else if line.starts_with("l PREAMP") {
                "20\n".to_string()
            } else if line.starts_with('f') {
                "14074000\n".to_string()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let mut rig = Rig::rigctld(&addr);

        // ⭐ THE DISCRIMINATOR. The ordinary reader REFUSES the very reading these controls
        // exist to carry; the raw reader takes it. Both directions asserted, because a test
        // that only showed the raw reader working would pass just as well if someone quietly
        // widened `read_level` and broke every fractional level in the app.
        assert!(
            rig.read_level("ATT").is_err(),
            "read_level must keep rejecting a decibel reading — it is the 0..1 reader"
        );
        assert_eq!(rig.read_att_db(), Some(12));
        assert_eq!(rig.read_preamp_db(), Some(20));

        // The setters put WHOLE DECIBELS on the wire, not a fraction.
        rig.set_att_db(12).unwrap();
        rig.set_preamp_db(20).unwrap();
        rig.set_att_db(0).unwrap();
        // …and the monitor gain, which really IS a 0..1 fraction, still goes out as one.
        rig.set_monitor_gain(0.5).unwrap();
        let sent = log.lock().unwrap().clone();
        assert!(sent.contains(&"L ATT 12".to_string()), "{sent:?}");
        assert!(sent.contains(&"L PREAMP 20".to_string()), "{sent:?}");
        assert!(sent.contains(&"L ATT 0".to_string()), "{sent:?}");
        assert!(
            sent.contains(&"L MONITOR_GAIN 0.500".to_string()),
            "{sent:?}"
        );
        // ⚠️ AND NOT THE FRACTION THE GENERIC SETTER WOULD HAVE SENT. Named explicitly:
        // `L ATT 1.000` is well-formed, is accepted by rigctld, and pads by the wrong amount.
        assert!(
            !sent.iter().any(|l| l.starts_with("L ATT 1.0")),
            "the clamped-fraction form must never reach the wire: {sent:?}"
        );
        assert!(
            !sent.iter().any(|l| l.starts_with("L PREAMP 1.0")),
            "{sent:?}"
        );
    }

    /// The step lists come off ONE `\dump_state`, at the same once-per-CAT-confirmation
    /// probe that already reads the frequency ranges — not a second round trip, and not a
    /// constant. `None` when the rig did not answer with something we can walk, which the
    /// caller must treat exactly as it treats unknown ranges: offer nothing.
    #[test]
    fn the_rig_reads_its_own_step_lists_off_dump_state() {
        let dump = include_str!("../tests/fixtures/dump_state_ic7610.txt").to_string();
        let (addr, log) = mock_rigctld(move |line: &str| {
            if line.starts_with("\\dump_state") {
                dump.clone()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let mut rig = Rig::rigctld(&addr);
        let steps = rig.read_db_steps().expect("a real dump parses");
        assert_eq!(steps.attenuator_db, vec![6, 12, 18]);
        assert_eq!(steps.preamp_db, vec![12, 20]);
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|l| l.contains("dump_state"))
                .count(),
            1,
            "one round trip, not one per list"
        );

        // A rig that will not answer reads as UNKNOWN, never as an empty ladder.
        let (addr2, _) = mock_rigctld(|_: &str| "RPRT -11\n".to_string());
        assert_eq!(Rig::rigctld(&addr2).read_db_steps(), None);
        assert_eq!(Rig::vox().read_db_steps(), None, "no CAT, nothing to ask");
    }

    #[test]
    fn an_unrecognised_dump_state_reads_as_unknown_so_callers_fail_open() {
        // ⚠️ THE SAFETY PROPERTY. Anything we cannot positively parse must read as UNKNOWN, never
        // as "covers nothing" — a capability probe that wrongly answered "no" would block
        // legitimate QSYs, which is worse than the refused command it exists to avoid.
        for reply in [
            "RPRT -1\n",                                            // the verb is not implemented
            "RPRT -11\n",                                           // not available
            "",                                                     // nothing came back
            "1\n1035\n0\n0 0 0 0 0 0 0\n", // well-formed but an EMPTY rx list
            "1\n1035\n0\n30000 60000000\n", // wrong field count
            "9\n1035\n0\n30000 7500 0 0 0 0 0\n0 0 0 0 0 0 0\n", // unknown protocol version
            "hello\nworld\n",              // not a dump_state at all
            "1\n1035\n0\n30000.0 60000000.0 0x1ff -1 -1 0x3 0x3\n", // no terminator
        ] {
            assert_eq!(
                parse_dump_state_rx_ranges(reply),
                None,
                "must read as unknown, not as a range list: {reply:?}"
            );
        }
    }

    #[test]
    fn the_range_probe_leaves_nothing_in_the_socket_for_the_next_command() {
        // ⚠️ THE DESYNC HAZARD, and the reason `\dump_state` gets its own reader. It replies with
        // ~40 lines; the ordinary `command` returns at the FIRST newline, which would leave the rest
        // in the socket to be read as the next command's answer — after which every command is
        // judged on a previous one's reply. This is the failure mode the drop-on-error guard exists
        // for, and a successful multi-line reply slips straight past that guard.
        let dump = "0\n1035\n1\n\
                    30000.000000 60000000.000000 0xffffffff -1 -1 0x3 0x0\n\
                    0 0 0 0 0 0 0\n\
                    1800000.000000 2000000.000000 0xffffffff 5 100 0x3 0x0\n\
                    0 0 0 0 0 0 0\n\
                    0xffffffff 1\n0 0\n0xffffffff 2700\n0 0\n0\n0\n0\n0\n0\n0\n";
        let (addr, log) = mock_rigctld(move |l| {
            if l.starts_with("\\dump_state") {
                dump.to_string()
            } else if l.trim() == "f" {
                "14074000\n".to_string()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let mut rig = Rig::rigctld(&addr);
        assert_eq!(
            rig.read_rx_ranges(),
            Some(vec![(30_000, 60_000_000)]),
            "the probe read the whole dump"
        );
        // THE ASSERTION THAT MATTERS: the very next command gets its OWN answer.
        assert_eq!(
            rig.read_freq().expect("a clean read after the dump"),
            14_074_000,
            "a leftover dump_state line was read as the dial"
        );
        assert!(rig.set_mode("USB", 0).is_ok(), "and commands still succeed");
        let sent = log.lock().unwrap().clone();
        assert!(
            sent.iter().any(|l| l.starts_with("\\dump_state")),
            "the probe really went on the wire: {sent:?}"
        );

        // A rig/daemon that doesn't implement the verb: unknown, and nothing is disturbed.
        let (addr2, _l2) = mock_rigctld(|l| {
            if l.trim() == "f" {
                "7074000\n".to_string()
            } else {
                "RPRT -11\n".to_string()
            }
        });
        let mut rig2 = Rig::rigctld(&addr2);
        assert_eq!(rig2.read_rx_ranges(), None, "unrecognised → unknown");
        assert_eq!(rig2.read_freq().unwrap(), 7_074_000, "link still clean");

        // No CAT: no probe, no error, no socket.
        assert_eq!(Rig::vox().read_rx_ranges(), None);
    }

    #[test]
    fn set_mode_errors_when_rig_rejects_the_mode() {
        // A rig with no DATA/PKT submode replies RPRT -1 to `M PKTUSB` — set_mode must
        // surface that as Err so the radio loop's bounded retry can give up (not loop
        // an `M` command every tick). A mode the rig accepts still returns Ok.
        let (addr, _log) = mock_rigctld(|l| {
            if l.starts_with('M') {
                "RPRT -1\n".to_string()
            } else {
                "RPRT 0\n".to_string()
            }
        });
        let mut rig = Rig::rigctld(&addr);
        let err = rig.set_mode("PKTUSB", 0).unwrap_err();
        // The KIND is load-bearing: `Other` is the radio loop's "active rig rejection"
        // signal (mode_saw_reject), the one case where the give-up may blame the rig
        // and try the plain-sideband fallback — a timeout/refused link must never
        // masquerade as it (that misdiagnosis produced "rig has no PKTUSB mode" on an
        // IC-7610 whose CAT link was merely too slow).
        assert_eq!(err.kind(), std::io::ErrorKind::Other, "{err}");

        let (addr2, _l2) = mock_rigctld(ok_reply(14_074_000));
        let mut rig2 = Rig::rigctld(&addr2);
        assert!(rig2.set_mode("USB", 0).is_ok());
    }

    #[test]
    fn ptt_errors_when_rig_reports_failure() {
        // rigctld answers RPRT -1 (e.g. CAT not ready) → ptt must surface an error.
        let (addr, _log) = mock_rigctld(|_l| "RPRT -1\n".to_string());
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.ptt(true).is_err());
        // TX-SAFETY INVARIANT, and the reason `ptt` sets this BEFORE the attempt: a key-down
        // that ERRORED may still have keyed the rig, so we must believe we are keyed and let
        // the unkey paths run. Believing the failure means never sending the unkey. This was
        // pinned by an integration test that keyed a PTT-less rigctld dummy; that premise
        // stopped holding at Hamlib 4.6.5 (the dummy now accepts PTT), so the assertion moved
        // here, where it needs no daemon and cannot rot with a Hamlib release.
        assert!(rig.keyed, "a FAILED key-down must still leave keyed=true");
    }

    #[test]
    fn a_key_with_no_answer_in_time_is_told_apart_from_a_refusal() {
        // ⭐ A KEY CAN FAIL TWO WAYS, AND THE RADIO LOOP MUST TELL THEM APART. A slow radio
        // behind rigctld keys and answers after the PTT deadline; a refusal is an answer that
        // says no. The loop plays the first over, as it always has, and drops the second
        // (`service::KeyUp`), so the error must say which it was.
        let (addr, _log) = mock_rigctld(|_l| {
            std::thread::sleep(std::time::Duration::from_millis(1_500));
            "RPRT 0\n".to_string()
        });
        let mut rig = Rig::rigctld(&addr);
        let late = rig.ptt(true).expect_err("premise: the deadline passed");
        assert!(super::no_answer_in_time(&late), "{late}");
        // In the deadline's own words, as before.
        assert!(
            late.to_string()
                .starts_with("rig reply incomplete after 700 ms"),
            "{late}"
        );
        assert!(rig.keyed, "a key with no answer may have keyed the rig");

        let (addr, _log) = mock_rigctld(|_l| "RPRT -1\n".to_string());
        let refused = Rig::rigctld(&addr).ptt(true).expect_err("premise: refused");
        assert!(!super::no_answer_in_time(&refused), "{refused}");

        // Nothing listening: the key never went out.
        let unsent = Rig::rigctld("127.0.0.1:1")
            .ptt(true)
            .expect_err("premise: no rigctld");
        assert!(!super::no_answer_in_time(&unsent), "{unsent}");
    }

    #[test]
    fn hamlibs_no_answer_to_a_key_is_no_answer_in_time_not_a_refusal() {
        // ⭐ rigctld answers a key it sent on and heard nothing back about with Hamlib's own "the
        // rig did not answer" (`rprt_is_link_fault`). That is the deadline's case one hop further
        // down the line: the radio may be keying, so the radio loop plays the over, with its
        // warning (`service::KeyUp::NoAnswer`). An answer that says no is a refusal, and the loop
        // drops the over.
        for code in [-5, -6, -13, -14] {
            let e = super::ptt_reply(&format!("RPRT {code}\n")).expect_err("premise: not keyed");
            assert!(super::no_answer_in_time(&e), "RPRT {code}: {e}");
        }
        for code in [-9, -1] {
            let e = super::ptt_reply(&format!("RPRT {code}\n")).expect_err("premise: not keyed");
            assert!(
                !super::no_answer_in_time(&e),
                "RPRT {code} is a refusal: {e}"
            );
        }
        assert!(super::ptt_reply("RPRT 0\n").is_ok(), "RPRT 0 keyed it");
    }

    #[test]
    fn nexuss_own_deadline_is_told_apart_from_hamlibs_no_answer() {
        // Both are no answer in time to the other modes. A slot over goes out on the first and
        // halts on the second, as WSJT-X, which sets no deadline of its own on a key, halts on
        // every Hamlib error (`crate::slot::slot_key_failure`).
        let (addr, _log) = mock_rigctld(|_l| {
            std::thread::sleep(std::time::Duration::from_millis(1_500));
            "RPRT 0\n".to_string()
        });
        let late = Rig::rigctld(&addr)
            .ptt(true)
            .expect_err("premise: the deadline passed");
        assert!(
            super::no_answer_in_time(&late) && super::deadline_passed(&late),
            "{late}"
        );
        for code in [-5, -6, -13, -14] {
            let e = super::ptt_reply(&format!("RPRT {code}\n")).expect_err("premise: not keyed");
            assert!(super::no_answer_in_time(&e), "RPRT {code}: {e}");
            assert!(
                !super::deadline_passed(&e),
                "RPRT {code} is Hamlib's answer, not the deadline: {e}"
            );
        }
        for code in [-9, -1] {
            let e = super::ptt_reply(&format!("RPRT {code}\n")).expect_err("premise: not keyed");
            assert!(!super::deadline_passed(&e), "RPRT {code}: {e}");
        }
        let unsent = Rig::rigctld("127.0.0.1:1")
            .ptt(true)
            .expect_err("premise: no rigctld");
        assert!(!super::deadline_passed(&unsent), "{unsent}");
    }

    #[test]
    fn a_panicking_thread_still_drops_ptt() {
        // THE UN-DROPPED RIG. `run_radio` unkeys only on its error RETURN (service.rs) — a panic
        // unwinding past it, or any other teardown of the radio thread, never reaches that line,
        // and the catch_unwind in src-tauri cannot see the rig. Every other owned resource here
        // has a Drop (RigctldProc, FskKeyer, WinKeyer, SerialKeyer, FlexDax, FlexSpectrum,
        // DecodeWorker); the rig — the one that is RADIATING — did not, so the carrier stayed up
        // until the operator walked to the radio. The rig's own destructor is the only backstop
        // that survives EVERY exit path.
        //
        // (The panic below prints a "thread panicked" line to stderr. That is the test working.)
        let (addr, log) = mock_rigctld(ok_reply(14_074_000));
        let h = std::thread::spawn(move || {
            let mut rig = Rig::rigctld(&addr);
            rig.ptt(true).expect("mock rig keys");
            panic!("radio thread died mid-over");
        });
        assert!(h.join().is_err(), "the thread really panicked");
        // The unwind drops the Rig BEFORE join returns, and the mock logs each line before it
        // answers, so `ptt(false)` cannot return until "T 0" is already in the log — no race.
        let sent = log.lock().unwrap().clone();
        assert!(sent.iter().any(|l| l.trim() == "T 1"), "keyed: {sent:?}");
        assert!(
            sent.iter().any(|l| l.trim() == "T 0"),
            "PTT was NEVER dropped when the thread panicked — the radio is still \
             transmitting: {sent:?}"
        );
    }

    #[test]
    fn dropping_a_rig_that_was_never_keyed_sends_nothing() {
        // The teardown unkey is gated on `keyed` deliberately. An UNCONDITIONAL one would make
        // every Rig drop — every read-only monitor rig in the dual-radio pool, every teardown of
        // an idle radio — send a keying command, and when the stream is already closed
        // `ensure_connected` would sit on a `TcpStream::connect` to a rig that may be gone. Only
        // a rig we believe is radiating is worth that.
        let (addr, log) = mock_rigctld(ok_reply(14_074_000));
        {
            let mut rig = Rig::rigctld(&addr);
            assert_eq!(rig.read_freq().unwrap(), 14_074_000);
        }
        let sent = log.lock().unwrap().clone();
        assert!(
            !sent.iter().any(|l| l.trim().starts_with('T')),
            "an idle rig sent a keying command at teardown: {sent:?}"
        );

        // And a VOX rig — no CAT channel at all — tears down quietly whatever its keyed state.
        let mut vox = Rig::vox();
        vox.ptt(true).unwrap();
        drop(vox);
    }

    /// A listening port that never completes another connection: its accept queue is shrunk to
    /// nothing and filled, so the kernel drops every further SYN and a connect to it waits out the
    /// OS SYN retries (~127 s), exactly as one to a firewalled or switched-off network rigctld
    /// does. Unlike a non-routable address it black-holes the same way in CI and in a private
    /// network namespace with no route out. Linux only: `listen(fd, 0)` on a socket that is
    /// already listening only changes its backlog there.
    #[cfg(target_os = "linux")]
    fn black_hole() -> (TcpListener, Vec<TcpStream>, String) {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn listen(fd: i32, backlog: i32) -> i32;
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        // SAFETY: a valid listening socket for the call's whole duration.
        assert_eq!(
            unsafe { listen(listener.as_raw_fd(), 0) },
            0,
            "listen(fd, 0)"
        );
        let at = listener.local_addr().unwrap();
        let mut held = Vec::new();
        for _ in 0..8 {
            match TcpStream::connect_timeout(&at, Duration::from_millis(300)) {
                Ok(s) => held.push(s),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    return (listener, held, at.to_string());
                }
                Err(e) => panic!("filling the accept queue: {e}"),
            }
        }
        panic!("the accept queue never filled");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_cat_link_nobody_answers_gives_up_in_seconds_as_not_reachable() {
        // A network rigctld that is firewalled or switched off answers no SYN at all. An
        // unbounded connect held the radio loop for the OS retry time on every reconnect.
        let (_listener, _held, addr) = black_hole();
        let mut rig = Rig::rigctld(&addr);
        let t0 = std::time::Instant::now();
        let e = rig
            .read_freq()
            .expect_err("nothing answered the connection");
        let took = t0.elapsed();
        assert!(
            took < CAT_CONNECT_TIMEOUT + Duration::from_secs(2),
            "the connect held the caller {took:?}"
        );
        // A slow connect is never success, and giving up early is not the fix either.
        assert!(took >= CAT_CONNECT_TIMEOUT, "gave up after only {took:?}");
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
        assert!(e.to_string().contains("not reachable"), "{e}");
        assert!(e.to_string().contains(&addr), "names the address: {e}");
        // Nothing reached the wire, so this is not "the radio may be acting on it".
        assert!(!no_answer_in_time(&e), "{e}");

        // Control: a rigctld that is there connects and answers as before, at once.
        let (live, _log) = mock_rigctld(ok_reply(14_074_000));
        let mut rig = Rig::rigctld(&live);
        let t0 = std::time::Instant::now();
        assert_eq!(rig.read_freq().unwrap(), 14_074_000);
        assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_key_over_a_cat_link_nobody_answers_fails_and_halts_the_slot_over() {
        // Keying over CAT to a radio that cannot be reached must fail, never read as keyed-ok,
        // and must not be the "deadline passed, the radio may be keying" case a slot over plays
        // through: the over halts.
        let (_listener, _held, addr) = black_hole();
        let mut rig = Rig::rigctld(&addr);
        let t0 = std::time::Instant::now();
        let key = rig.ptt(true);
        assert!(key.is_err(), "keyed with no CAT link");
        assert!(t0.elapsed() < CAT_CONNECT_TIMEOUT + Duration::from_secs(2));
        assert!(
            crate::slot::slot_key_failure(&key, false).is_some(),
            "a key that never reached the radio must halt the over: {key:?}"
        );
        // The teardown unkey (the rig still counts itself keyed) is bounded too.
        let t0 = std::time::Instant::now();
        drop(rig);
        assert!(
            t0.elapsed() < CAT_CONNECT_TIMEOUT + Duration::from_secs(2),
            "teardown held {:?}",
            t0.elapsed()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_cat_host_nobody_answers_is_unreachable_not_a_silent_rig() {
        // The connection was never made, so this is the CAT host out of reach, not a radio behind
        // a reached rigctld that did not reply: the two are worded differently to the operator.
        let (_listener, _held, addr) = black_hole();
        let e = Rig::rigctld(&addr)
            .read_freq()
            .expect_err("nothing answered the connection");
        assert!(cat_unreachable(&e), "{e:?}");

        // Controls: a rig that did not answer (`RPRT -5`), a refusal (`RPRT -1`), and a local
        // rigctld that is not running are not "the host cannot be reached".
        let (addr, _log) = mock_rigctld(|_l| "RPRT -5\n".to_string());
        let e = Rig::rigctld(&addr).set_mode("USB", -1).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
        assert!(!cat_unreachable(&e), "{e:?}");
        let (addr, _log) = mock_rigctld(|_l| "RPRT -1\n".to_string());
        let e = Rig::rigctld(&addr).set_mode("USB", -1).unwrap_err();
        assert!(!cat_unreachable(&e), "{e:?}");
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        drop(l);
        let e = Rig::rigctld(&addr).read_freq().unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionRefused, "{e}");
        assert!(!cat_unreachable(&e), "{e:?}");
    }

    #[test]
    fn read_freq_errors_on_non_numeric_reply() {
        let (addr, _log) = mock_rigctld(|_l| "RPRT -1\n".to_string());
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.read_freq().is_err());
    }

    #[test]
    fn read_freq_errors_when_rigctld_unreachable() {
        // Grab then drop a port so nothing is listening → connection refused.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        drop(l);
        let mut rig = Rig::rigctld(&addr);
        assert!(rig.read_freq().is_err());
    }
}
