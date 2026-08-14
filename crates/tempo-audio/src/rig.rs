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

/// rigctld command line for PTT.
pub fn ptt_line(on: bool) -> String {
    format!("T {}\n", on as u8)
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
    let s = match shift.trim().to_ascii_lowercase().as_str() {
        "plus" | "+" => "+",
        "minus" | "-" => "-",
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
/// rigctld `U` — toggle a function (RIT/XIT must be enabled this way before `J`/`Z`).
pub fn func_line(func: &str, on: bool) -> String {
    format!("U {} {}\n", func, on as u8)
}
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
/// rigctld `b` — send_morse: the rig keys CW from this text (rest of the line).
pub fn morse_line(text: &str) -> String {
    format!("b {text}\n")
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
/// else — an `RPRT` error, our own broker's shorter answer, a future format — reads as "unknown",
/// and the caller must then fail OPEN rather than assume nothing is covered.
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
        }
    }

    /// Mark this CAT link as a slow NETWORK transport (a longer per-command deadline). Call after
    /// constructing a `Rig` for a network-CAT rig (e.g. a Flex reached over TCP) so a legitimately
    /// slow reply isn't cut off; leave default (false) for serial rigs so a stalled read is bounded.
    pub fn set_slow_transport(&mut self, slow: bool) {
        self.slow_transport = slow;
    }
    /// Change how this rig is keyed WITHOUT touching its (already-open) CAT control channel. Used by
    /// the dual-radio handoff: a monitor rig is opened read-only (`PttMode::Vox`), so when it's adopted
    /// as the ACTIVE radio it must be switched to the profile's real PTT mode (`Cat`/`Serial`) or
    /// `ptt()` would silently no-op (the "TX dead after switching to the FTDX10" bug).
    pub fn set_ptt_mode(&mut self, mode: PttMode) {
        self.ptt_mode = mode;
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
            if let Some(addr) = &self.control {
                let s = TcpStream::connect(addr)?;
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
        match self.command_inner(line, deadline_ms) {
            Ok(reply) => Ok(reply),
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
                Err(e)
            }
        }
    }

    fn command_inner(
        &mut self,
        line: &str,
        deadline_override: Option<u64>,
    ) -> std::io::Result<String> {
        // Read the transport class before the mutable stream borrow below (they'd otherwise alias).
        let slow = self.slow_transport;
        let stream = self.ensure_connected()?;
        // Discard any STALE bytes left in the socket by a prior MULTI-LINE reply — `m` (get_mode)
        // returns the mode line AND a passband line, and on a networked chain the 2nd line can
        // arrive AFTER we already returned the 1st. A lingering byte would be read as THIS
        // command's answer and desync every command after it (the exact hazard the drop-on-failure
        // guards, but a successful multi-line reply slips past that). Non-blocking → free when clean.
        stream.set_nonblocking(true)?;
        let mut scratch = [0u8; 256];
        while let Ok(n) = stream.read(&mut scratch) {
            if n == 0 {
                break; // peer closed — the real read below surfaces it
            }
        }
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?; // restore blocking-with-timeout
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
        let deadline_ms = deadline_override.unwrap_or(if slow { 2_500 } else { 700 });
        let deadline = std::time::Instant::now() + Duration::from_millis(deadline_ms);
        let mut out = Vec::with_capacity(64);
        let mut buf = [0u8; 256];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "rigctld closed the connection",
                    ));
                }
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    if out.ends_with(b"\n") {
                        return Ok(String::from_utf8_lossy(&out).to_string());
                    }
                }
                Err(ref e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {} // per-read timeout tick — keep waiting to the deadline
                Err(e) => return Err(e), // hard error — caller drops the stream
            }
            if std::time::Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "rig reply incomplete after {deadline_ms} ms (got {:?})",
                        String::from_utf8_lossy(&out)
                    ),
                ));
            }
        }
    }

    /// Send a command whose reply is MANY lines with no length or terminator we can predict
    /// (`\dump_state`), reading until the peer goes quiet for one read window.
    ///
    /// Kept separate from [`Rig::command`] on purpose: that one returns at the first newline,
    /// which for a multi-line reply would leave the rest in the socket to be misread as the NEXT
    /// command's answer — the exact desync the drop-on-failure guard exists for. This drains to
    /// quiet and, like `command`, drops the stream on any failure so nothing is left behind.
    fn command_multiline(&mut self, line: &str) -> std::io::Result<String> {
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
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break, // peer closed — parse what we have
                Ok(n) => out.extend_from_slice(&buf[..n]),
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
                break;
            }
        }
        if out.is_empty() {
            return Err(std::io::Error::other("no reply to a multi-line command"));
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
    #[track_caller]
    pub fn ptt(&mut self, on: bool) -> std::io::Result<()> {
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
        if on {
            self.keyed = true;
        }
        let result = match &self.ptt_mode {
            PttMode::Vox => Ok(()),
            PttMode::Serial { .. } => self.serial_ptt(on),
            PttMode::Cat => {
                if self.control.is_none() {
                    Ok(()) // CAT keying chosen but no CAT channel → VOX fallback
                } else {
                    // PTT is time-critical: a fixed 700 ms deadline (not the slow-transport
                    // 2.5 s read window) so the un-key can't hang the radio loop.
                    match self.command_with_deadline(&ptt_line(on), Some(PTT_DEADLINE_MS)) {
                        Ok(reply) if reply_ok(&reply) || reply.is_empty() => Ok(()),
                        Ok(reply) => Err(std::io::Error::other(format!(
                            "rigctld PTT error: {reply:?}"
                        ))),
                        Err(e) => Err(e),
                    }
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
        if self.control.is_none() {
            return Ok(());
        }
        let reply = self.command(&freq_line(hz))?;
        if reply_ok(&reply) || reply.is_empty() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "rigctld freq error: {reply:?}"
            )))
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

    /// Set the operating mode (e.g. "USB") + passband. A BLANK mode is a no-op —
    /// the caller is choosing to OBEY the radio's current mode (max compatibility),
    /// so Nexus sends no `M` command. Also a no-op unless a CAT control channel is
    /// configured (works even when PTT is keyed by VOX/serial — control is separate).
    /// Surfaces a rig REJECTION (`RPRT -1`, e.g. a rig with no DATA/PKT submode) as
    /// an `Err`, so the radio loop's bounded retry can give up instead of looping.
    pub fn set_mode(&mut self, mode: &str, passband_hz: i32) -> std::io::Result<()> {
        if mode.trim().is_empty() {
            return Ok(());
        }
        if self.control.is_none() {
            return Ok(());
        }
        let reply = self.command(&mode_line(mode, passband_hz))?;
        if reply_ok(&reply) || reply.is_empty() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "rigctld mode error: {reply:?}"
            )))
        }
    }

    /// Apply FM repeater settings: shift direction (`R`), offset magnitude (`O`), and
    /// CTCSS tone (`C`). Best-effort — a rig that supports shift but not CTCSS (or has no
    /// repeater support) rejects individual commands harmlessly, so each is sent and its
    /// per-command error swallowed. No-op without a CAT control channel; `tone_hz` 0
    /// disables CTCSS. Call after a successful FM `set_mode` (the connection is live).
    pub fn set_fm_repeater(
        &mut self,
        shift: &str,
        offset_hz: i64,
        tone_hz: f32,
    ) -> std::io::Result<()> {
        if self.control.is_none() {
            return Ok(());
        }
        let _ = self.command(&rptr_shift_line(shift));
        if offset_hz > 0 {
            let _ = self.command(&rptr_offset_line(offset_hz));
        }
        let _ = self.command(&ctcss_line(tone_hz));
        Ok(())
    }

    /// Whether a CAT control channel is configured (so the freq/mode/CAT verbs are
    /// live). Lets callers distinguish "no CAT" from "CAT present but the rig is mute".
    pub fn has_control(&self) -> bool {
        self.control.is_some()
    }

    /// Probe the rig by reading its current dial frequency (Hz) over CAT — the
    /// basis of a WSJT-X-style "Test CAT". Connects to rigctld and sends `f`,
    /// which replies with the frequency on its own line. Returns a descriptive
    /// error when rigctld is unreachable (connection refused) or the rig itself
    /// doesn't answer (bad baud / serial port / CAT disabled → no numeric reply).
    /// Only valid with a CAT control channel.
    /// Read a rig LEVEL (e.g. "RFPOWER" → 0.0–1.0) via rigctld `l NAME`.
    /// CAT-only; errors on FakeIt/none like `read_freq`.
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
        self.control.as_ref()?;
        let reply = self.command(&format!("l {name}\n")).ok()?;
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
    pub fn set_func(&mut self, token: &str, on: bool) -> std::io::Result<()> {
        if self.control.is_none() {
            return Err(std::io::Error::other("not a CAT rig"));
        }
        let reply = self.command(&format!("U {token} {}\n", u8::from(on)))?;
        if reply_ok(&reply) {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "set_func {token} rejected: {reply:?}"
            )))
        }
    }

    pub fn read_freq(&mut self) -> std::io::Result<u64> {
        if self.control.is_none() {
            return Err(std::io::Error::other("not a CAT rig"));
        }
        let reply = self.command("f\n")?;
        reply
            .lines()
            .find_map(|l| l.trim().parse::<u64>().ok())
            .filter(|hz| *hz > 0)
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "rig did not return a frequency (reply {reply:?}) — check the serial port, \
                     baud rate, and that CAT/CI-V is enabled on the rig"
                ))
            })
    }

    /// Read the rig's current mode (e.g. "USB"/"CW"). `None` if not a CAT rig or the
    /// rig didn't answer. The `m` reply is the mode on one line, passband on the next.
    pub fn read_mode(&mut self) -> Option<String> {
        self.read_mode_passband().0
    }

    /// Read the rig's mode + RX passband (Hz) from ONE `m` reply. CAT-only. The passband is
    /// opportunistic: present when both reply lines arrive in one read (the common path); a
    /// networked chain that splits them just surfaces the width on a later poll (the pre-command
    /// drain flushes the stray line so it never poisons the next command).
    pub fn read_mode_passband(&mut self) -> (Option<String>, Option<u32>) {
        if self.control.is_none() {
            return (None, None);
        }
        match self.command("m\n") {
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
    pub fn send_raw(&mut self, raw: &str) -> Option<String> {
        self.control.as_ref()?;
        let reply = self.command(&format!("w {raw}\n")).ok()?;
        let trimmed = reply.trim();
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
    /// Set RIT (receive incremental tuning) offset in Hz; enabling RIT first (0 = off).
    pub fn set_rit(&mut self, hz: i32) -> std::io::Result<()> {
        self.cat(&func_line("RIT", hz != 0))?;
        self.cat(&rit_line(hz))
    }
    /// Set XIT (transmit incremental tuning) offset in Hz (0 = off).
    pub fn set_xit(&mut self, hz: i32) -> std::io::Result<()> {
        self.cat(&func_line("XIT", hz != 0))?;
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
    pub fn send_morse(&mut self, text: &str) -> std::io::Result<()> {
        self.cat(&morse_line(text))
    }
    /// Abort CW in progress. Newer Hamlib exposes `\stop_morse`; older builds vary by
    /// manufacturer (the WinKeyer path has a reliable Clear-Buffer abort instead).
    pub fn stop_morse(&mut self) -> std::io::Result<()> {
        self.cat("\\stop_morse\n")
    }

    /// Send a rigctld command, succeeding on `RPRT 0` (or an empty reply); no-op when
    /// no CAT control channel is configured. Shared by the all-mode control verbs above.
    fn cat(&mut self, line: &str) -> std::io::Result<()> {
        if self.control.is_none() {
            return Ok(());
        }
        let reply = self.command(line)?;
        if reply_ok(&reply) || reply.is_empty() {
            Ok(())
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
/// first and `TcpStream::connect` is bounded only by the OS (immediate against a refused localhost
/// rigctld — the usual case — but longer for a network rig that has gone away). A teardown that
/// waits is better than a radio that keeps transmitting.
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

    #[test]
    fn fm_repeater_lines_match_rigctld_protocol() {
        assert_eq!(rptr_shift_line("plus"), "R +\n");
        assert_eq!(rptr_shift_line("minus"), "R -\n");
        assert_eq!(rptr_shift_line("simplex"), "R None\n");
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
        assert_eq!(func_line("RIT", true), "U RIT 1\n");
        assert_eq!(func_line("XIT", false), "U XIT 0\n");
        assert_eq!(rit_line(-200), "J -200\n");
        assert_eq!(xit_line(500), "Z 500\n");
        assert_eq!(level_line("RFPOWER", "0.500"), "L RFPOWER 0.500\n");
        assert_eq!(level_line("KEYSPD", "25"), "L KEYSPD 25\n");
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
    fn timed_out_reply_errors_and_drops_stream_so_the_next_command_cannot_desync() {
        // The C3 desync: a reply that lands after the 2.5 s deadline must NOT be
        // left in the socket for the next command to read as its own answer.
        // Command 1's reply arrives late (past the deadline) → the command must
        // error AND drop the stream; command 2 reconnects and reads its OWN
        // fresh reply, never the stale one.
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

    #[test]
    fn read_freq_parses_the_dial_over_tcp() {
        let (addr, log) = mock_rigctld(ok_reply(14_074_000));
        let mut rig = Rig::rigctld(&addr);
        assert_eq!(rig.read_freq().unwrap(), 14_074_000);
        assert_eq!(log.lock().unwrap().as_slice(), &["f".to_string()]);
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
