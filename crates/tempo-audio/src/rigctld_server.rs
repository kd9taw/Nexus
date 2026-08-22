//! A rigctld-compatible TCP **server** — the "broker, not port hog" half of CAT.
//!
//! Nexus owns one connection to the radio; this server lets WSJT-X / N1MM / Log4OM
//! (any Hamlib NET rigctl client) **share that radio through Nexus** by speaking the
//! same text protocol Hamlib's `rigctld` does, on :4532. Every command is relayed to
//! a [`RigBackend`] (Nexus's live rig state), so a logger setting the dial retunes
//! Nexus too.
//!
//! Pure protocol handling ([`handle_command`]) is unit-tested; the std-only TCP loop
//! ([`serve`]) is covered by a localhost integration test. The one piece that needs a
//! real WSJT-X to validate is the exact `\dump_state` byte layout — emitted here in
//! the classic **protocol-0** form (so the client skips the protocol-1 key=value
//! block), grounded in Hamlib's `netrigctl` parser.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

/// The rig state the broker serves + the setters it relays. Implemented by Nexus's
/// live rig bridge (real radio), the native CI-V daemon, and by a mock in tests.
///
/// The extended verbs below default to `None` = **not implemented** (`RPRT -11`), so an
/// implementation that only fills in the core set behaves byte-identically to the
/// pre-extension broker. `Some(true)` → `RPRT 0`, `Some(false)` → `RPRT -1`.
pub trait RigBackend: Send + Sync {
    fn freq_hz(&self) -> u64;
    fn mode(&self) -> (String, u32); // (mode, passband Hz)
    fn ptt(&self) -> bool;
    /// True when Nexus ITSELF currently intends to transmit (a slot over, a tune carrier, or
    /// manual phone PTT). The disconnect fail-safe unkey (`serve_connection`) consults this: it
    /// must NOT unkey when Nexus is the one transmitting, because Nexus's own `Rig` is a client
    /// of this same broker and a transient reconnect of that connection would otherwise steal the
    /// active transmit (the IC-9700 native-CI-V flicker). Defaults false so the fail-safe still
    /// protects against an external client (WSJT-X/N1MM) that keyed then crashed.
    fn owner_transmitting(&self) -> bool {
        false
    }
    fn vfo(&self) -> String {
        "VFOA".to_string()
    }
    fn split(&self) -> bool {
        false
    }
    /// Setters return true on success (→ `RPRT 0`), false → `RPRT -1`.
    fn set_freq(&self, hz: u64) -> bool;
    fn set_mode(&self, mode: &str, passband_hz: u32) -> bool;
    fn set_ptt(&self, on: bool) -> bool;
    fn set_vfo(&self, _vfo: &str) -> bool {
        true
    }
    /// The radio's RECEIVE frequency ranges (Hz, inclusive) for `\dump_state`. `None` = don't
    /// answer the verb (`RPRT -1`), which every client must read as "capabilities unknown" and
    /// therefore fail OPEN. Implement it to let a client know what this radio can and cannot reach
    /// without having to command it there and see what happens.
    fn rx_ranges(&self) -> Option<Vec<(u64, u64)>> {
        None
    }
    // ---- extended verbs (the full surface Nexus's own `Rig` client uses) ----
    /// Read a level (`l NAME`). Return the reply VALUE line(s) without trailing newline
    /// (e.g. `"-12"` for STRENGTH dB, `"0.50"` for RFPOWER 0..1).
    fn level(&self, _name: &str) -> Option<String> {
        None
    }
    /// Set a level (`L NAME VALUE`, e.g. `RFPOWER 0.5`, `KEYSPD 25`).
    fn set_level(&self, _name: &str, _value: &str) -> Option<bool> {
        None
    }
    /// Read a function state (`u TOKEN`) — `Some(on)` or `None` = unimplemented.
    fn func(&self, _token: &str) -> Option<bool> {
        None
    }
    /// Set a function (`U TOKEN 0|1`, e.g. RIT/XIT enable).
    fn set_func(&self, _token: &str, _on: bool) -> Option<bool> {
        None
    }
    /// Key CW from text (`b TEXT`).
    fn send_morse(&self, _text: &str) -> Option<bool> {
        None
    }
    /// Abort CW in progress (`\stop_morse`).
    fn stop_morse(&self) -> Option<bool> {
        None
    }
    /// Split on/off + TX VFO (`S 0|1 VFOB`).
    fn set_split(&self, _on: bool, _tx_vfo: &str) -> Option<bool> {
        None
    }
    /// Split TX frequency (`I <hz>`).
    fn set_split_freq(&self, _hz: u64) -> Option<bool> {
        None
    }
    /// Split TX VFO's mode + passband (`X <mode> <pb>`). The plain `M` verb only
    /// ever reaches the RX VFO, so this is the one way to command "transmit LSB
    /// while I listen USB" — the inverting-transponder uplink sideband.
    fn set_split_mode(&self, _mode: &str, _passband_hz: i32) -> Option<bool> {
        None
    }
    /// RIT offset in Hz (`J <hz>`).
    fn set_rit(&self, _hz: i32) -> Option<bool> {
        None
    }
    /// XIT / ΔTX offset in Hz (`Z <hz>`).
    fn set_xit(&self, _hz: i32) -> Option<bool> {
        None
    }
    /// FM repeater shift (`R +|-|None`).
    fn set_rptr_shift(&self, _shift: &str) -> Option<bool> {
        None
    }
    /// FM repeater offset magnitude in Hz (`O <hz>`).
    fn set_rptr_offset(&self, _hz: i64) -> Option<bool> {
        None
    }
    /// CTCSS tone in tenths of Hz (`C 1000` = 100.0 Hz; 0 = off).
    fn set_ctcss(&self, _tenths: u32) -> Option<bool> {
        None
    }
    /// A client connected (`true`) / disconnected (`false`), with its peer address —
    /// the broker's connection-log hook (#53: with the broker as the advertised share
    /// endpoint, "is VarAC actually connected?" must be answerable from the app's own
    /// connection log rather than by guesswork). Default: no-op.
    fn client_event(&self, _peer: &str, _connected: bool) {}
}

/// The classic protocol-0 `\dump_state` capability dump. Wide HF–UHF ranges, all
/// modes, so a NET-rigctl client (WSJT-X) accepts the rig and lets you set any
/// freq/mode. First line `0` = protocol version 0 → the client does NOT read the
/// protocol-1 `key=value … done` trailer. NOTE: the one part needing live WSJT-X
/// validation; built to match Hamlib's `netrigctl` field-by-field reader.
const DUMP_STATE: &str = concat!(
    "0\n",                                                // protocol version (classic)
    "2\n",                                                // rig model (NET rigctl)
    "1\n",                                                // ITU region
    "135700 1300000000 0xffffffff -1 -1 0x3 0x0\n",       // rx range (all modes)
    "0 0 0 0 0 0 0\n",                                    // rx range terminator
    "135700 1300000000 0xffffffff 5000 100000 0x3 0x0\n", // tx range
    "0 0 0 0 0 0 0\n",                                    // tx range terminator
    "0xffffffff 1\n",                                     // tuning step: all modes, 1 Hz
    "0 0\n",                                              // tuning-step terminator
    "0xffffffff 2700\n",                                  // filter: all modes, 2700 Hz
    "0xffffffff 500\n",                                   // filter: all modes, 500 Hz
    "0 0\n",                                              // filter terminator
    "0\n",                                                // max_rit
    "0\n",                                                // max_xit
    "0\n",                                                // max_ifshift
    "0\n",                                                // announces
    "0\n",                                                // preamp list (empty)
    "0\n",                                                // attenuator list (empty)
    "0x0\n",                                              // has_get_func
    "0x0\n",                                              // has_set_func
    "0x0\n",                                              // has_get_level
    "0x0\n",                                              // has_set_level
    "0x0\n",                                              // has_get_parm
    "0x0\n",                                              // has_set_parm
);

/// The `\dump_state` reply for `backend`.
///
/// Uses [`DUMP_STATE`] verbatim unless the backend declares real RX ranges, in which case its RX
/// list is substituted. The wide default is deliberate for a broker fronting an arbitrary radio: a
/// NET-rigctl client (WSJT-X) must be allowed to set any freq/mode, and a client reading these
/// ranges as capability must therefore see "covers everything" rather than a guess.
fn dump_state(backend: &dyn RigBackend) -> String {
    let Some(ranges) = backend.rx_ranges().filter(|r| !r.is_empty()) else {
        return DUMP_STATE.to_string();
    };
    let mut out = String::with_capacity(DUMP_STATE.len());
    let mut lines = DUMP_STATE.lines();
    for _ in 0..3 {
        // protocol version, rig model, ITU region — unchanged
        out.push_str(lines.next().unwrap_or("0"));
        out.push('\n');
    }
    for (lo, hi) in ranges {
        // Same 7 fields Hamlib emits: start end modes low_power high_power vfo ant.
        out.push_str(&format!("{lo} {hi} 0xffffffff -1 -1 0x3 0x0\n"));
    }
    // Skip the default RX rows (up to and including their all-zero terminator), keep the rest.
    let mut in_rx = true;
    for line in lines {
        if in_rx {
            if line.trim() == "0 0 0 0 0 0 0" {
                in_rx = false;
                out.push_str("0 0 0 0 0 0 0\n");
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn rprt(ok: bool) -> String {
    if ok {
        "RPRT 0\n".into()
    } else {
        "RPRT -1\n".into()
    }
}

/// Map an extended-verb outcome: `None` = the backend doesn't implement it → Hamlib's
/// `RPRT -11` (not implemented), otherwise RPRT 0/-1.
fn rprt_ext(r: Option<bool>) -> String {
    match r {
        None => "RPRT -11\n".into(),
        Some(ok) => rprt(ok),
    }
}

/// Outcome of handling one request line.
pub enum Handled {
    /// Write this back to the client.
    Reply(String),
    /// Client asked to quit — write nothing and close.
    Close,
}

/// Canonicalise a request line: rewrite a leading `\long_name` verb to its short letter, so
/// ONE dispatch table serves both spellings. Real rigctld accepts every verb both ways
/// (`rigctl(1)`: "Commands can be sent … as a single char, or as a long command name") and
/// real clients mix them — VarAC sends long forms (#53). Before this existed the dispatcher
/// knew only the letters while [`parse_ptt_set`] ALSO matched `\set_ptt`, so a long-form key
/// request answered "not implemented" yet still recorded `asserted_ptt` — and the disconnect
/// fail-safe then acted on a key-down that never happened. Both callers now canonicalise
/// first, so the two can never disagree again.
///
/// Long names not in this table pass through untouched (→ `RPRT -11`, unknown, exactly as a
/// real daemon refuses a verb it lacks). `\dump_state`, `\chk_vfo`, `\get_powerstat` and
/// `\stop_morse` have no single-letter form and keep their spelled dispatch.
fn canonical_line(line: &str) -> std::borrow::Cow<'_, str> {
    let Some(rest) = line.strip_prefix('\\') else {
        return std::borrow::Cow::Borrowed(line);
    };
    let (verb, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    // The Hamlib 4.7.x short↔long table for every verb this server dispatches.
    let short = match verb {
        "get_freq" => "f",
        "set_freq" => "F",
        "get_mode" => "m",
        "set_mode" => "M",
        "get_vfo" => "v",
        "set_vfo" => "V",
        "get_ptt" => "t",
        "set_ptt" => "T",
        "get_split_vfo" => "s",
        "set_split_vfo" => "S",
        "set_split_freq" => "I",
        "set_split_mode" => "X",
        "set_rit" => "J",
        "set_xit" => "Z",
        "get_level" => "l",
        "set_level" => "L",
        "get_func" => "u",
        "set_func" => "U",
        "set_rptr_shift" => "R",
        "set_rptr_offs" => "O",
        "set_ctcss_tone" => "C",
        "send_morse" => "b",
        _ => return std::borrow::Cow::Borrowed(line),
    };
    if tail.is_empty() {
        std::borrow::Cow::Owned(short.to_string())
    } else {
        std::borrow::Cow::Owned(format!("{short} {tail}"))
    }
}

/// Handle one rigctld request line against `backend`. Pure (apart from the backend
/// calls) so the protocol is unit-testable. Implements the subset a NET-rigctl
/// client (WSJT-X) uses — get/set freq (`f`/`F`), mode (`m`/`M`), PTT (`t`/`T`),
/// VFO (`v`/`V`), split (`s`), `\dump_state`, `\chk_vfo`, `\get_powerstat`, `q` —
/// each in both its short and `\long_name` spelling ([`canonical_line`]) — plus the
/// extended verbs Nexus's own `Rig` client sends (levels `l`/`L`, funcs
/// `u`/`U`, morse `b`/`\stop_morse`, split `S`/`I`/`X`, RIT/XIT `J`/`Z`, FM
/// repeater `R`/`O`/`C`), which answer `RPRT -11` unless the backend implements them.
pub fn handle_command(line: &str, backend: &dyn RigBackend) -> Handled {
    let line = canonical_line(line.trim());
    let line: &str = &line;
    // `b` (send_morse) takes the REST OF THE LINE as text — CW messages contain spaces, so
    // dispatch it before the whitespace tokenizer below would split them.
    if let Some(text) = line.strip_prefix("b ") {
        return Handled::Reply(rprt_ext(backend.send_morse(text.trim())));
    }
    match line {
        "" => Handled::Reply(String::new()),
        "\\dump_state" => Handled::Reply(dump_state(backend)),
        // No VFO mode → the client sends commands without an explicit VFO argument.
        "\\chk_vfo" => Handled::Reply("CHKVFO 0\n".into()),
        "\\get_powerstat" => Handled::Reply("1\n".into()), // powered on
        "\\stop_morse" => Handled::Reply(rprt_ext(backend.stop_morse())),
        "q" | "Q" => Handled::Close,
        _ => {
            let mut p = line.split_whitespace();
            let reply = match p.next() {
                Some("f") => format!("{}\n", backend.freq_hz()),
                // Hamlib sends freq as printf %lf ("F 14074000.000000"), so parse
                // as f64 and round to Hz — a u64 parse rejects every real client.
                Some("F") => rprt(
                    p.next()
                        .and_then(|s| s.parse::<f64>().ok())
                        // Reject NaN/±inf and absurd magnitudes: `f.round() as u64`
                        // saturates inf/huge to u64::MAX (a garbage dial with a
                        // false RPRT 0). Cap at 1 THz — far above any ham band.
                        .filter(|f| f.is_finite() && (0.0..=1e12).contains(f))
                        .map(|f| backend.set_freq(f.round() as u64))
                        .unwrap_or(false),
                ),
                Some("m") => {
                    let (mode, pbw) = backend.mode();
                    format!("{mode}\n{pbw}\n")
                }
                Some("M") => {
                    let mode = p.next().unwrap_or("");
                    let pbw = p.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
                    rprt(!mode.is_empty() && backend.set_mode(mode, pbw))
                }
                Some("v") => format!("{}\n", backend.vfo()),
                Some("V") => rprt(p.next().map(|v| backend.set_vfo(v)).unwrap_or(false)),
                Some("t") => format!("{}\n", backend.ptt() as u8),
                // RIG_PTT_ON=1, RIG_PTT_ON_MIC=2, RIG_PTT_ON_DATA=3 are all key-down;
                // only 0 (RIG_PTT_OFF) is key-up. WSJT-X with a Rear/Data audio source
                // sends `T 3`, so keying on `s == "1"` would silently never TX.
                Some("T") => rprt(
                    p.next()
                        .and_then(|s| s.parse::<i32>().ok())
                        .map(|v| backend.set_ptt(v != 0))
                        .unwrap_or(false),
                ),
                Some("s") => format!("{}\n{}\n", backend.split() as u8, backend.vfo()),
                // ---- extended verbs (RPRT -11 when the backend doesn't implement them,
                // exactly like the pre-extension broker) ----
                Some("l") => match p.next().and_then(|n| backend.level(n)) {
                    Some(v) => format!("{v}\n"),
                    None => "RPRT -11\n".into(),
                },
                Some("L") => {
                    let name = p.next().unwrap_or("");
                    let value = p.next().unwrap_or("");
                    if name.is_empty() || value.is_empty() {
                        rprt(false)
                    } else {
                        rprt_ext(backend.set_level(name, value))
                    }
                }
                Some("u") => match p.next().and_then(|t| backend.func(t)) {
                    Some(on) => format!("{}\n", on as u8),
                    None => "RPRT -11\n".into(),
                },
                Some("U") => {
                    let token = p.next().unwrap_or("");
                    match (token.is_empty(), p.next()) {
                        (false, Some(v)) => rprt_ext(backend.set_func(token, v != "0")),
                        _ => rprt(false),
                    }
                }
                Some("S") => {
                    let on = p.next().map(|s| s != "0");
                    let tx_vfo = p.next().unwrap_or("VFOB");
                    match on {
                        Some(on) => rprt_ext(backend.set_split(on, tx_vfo)),
                        None => rprt(false),
                    }
                }
                Some("I") => rprt_ext(
                    p.next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|f| f.is_finite() && (0.0..=1e12).contains(f))
                        .map_or(Some(false), |f| backend.set_split_freq(f.round() as u64)),
                ),
                Some("X") => {
                    let mode = p.next().unwrap_or("");
                    let pbw = p.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                    if mode.is_empty() {
                        rprt(false)
                    } else {
                        rprt_ext(backend.set_split_mode(mode, pbw))
                    }
                }
                Some("J") => rprt_ext(
                    p.next()
                        .and_then(|s| s.parse::<i32>().ok())
                        .map_or(Some(false), |hz| backend.set_rit(hz)),
                ),
                Some("Z") => rprt_ext(
                    p.next()
                        .and_then(|s| s.parse::<i32>().ok())
                        .map_or(Some(false), |hz| backend.set_xit(hz)),
                ),
                Some("R") => rprt_ext(p.next().map_or(Some(false), |s| backend.set_rptr_shift(s))),
                Some("O") => rprt_ext(
                    p.next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|f| f.is_finite() && f.abs() <= 1e12)
                        .map_or(Some(false), |f| backend.set_rptr_offset(f.round() as i64)),
                ),
                Some("C") => rprt_ext(
                    p.next()
                        .and_then(|s| s.parse::<u32>().ok())
                        .map_or(Some(false), |t| backend.set_ctcss(t)),
                ),
                // Unknown command → Hamlib's "not implemented".
                _ => "RPRT -11\n".into(),
            };
            Handled::Reply(reply)
        }
    }
}

/// Detect a rigctld PTT-set request — `T <n>` in either spelling, via THE canonicaliser
/// ([`canonical_line`], same one the dispatcher uses, so the two can never again disagree
/// about whether a line keyed the rig) — and return the requested key state (`n != 0` is
/// key-down; only 0 is key-up), so a connection can fail-safe unkey on drop. `None` = not
/// a PTT-set line.
fn parse_ptt_set(line: &str) -> Option<bool> {
    let line = canonical_line(line.trim());
    let mut t = line.split_whitespace();
    match t.next() {
        Some("T") => t.next().and_then(|s| s.parse::<i32>().ok()).map(|v| v != 0),
        _ => None,
    }
}

/// Serve one client connection until EOF or `q`. Each line is one request.
pub fn serve_connection(stream: TcpStream, backend: Arc<dyn RigBackend>) {
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "?".into());
    backend.client_event(&peer, true);
    let reader = BufReader::new(stream);
    // Track this connection's PTT so a dropped/EOF'd broker client (WSJT-X or N1MM
    // crashing / closing mid-transmit) can't leave the rig keyed forever — the
    // original code only ever unkeyed on an explicit `T 0`.
    let mut asserted_ptt = false;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if let Some(v) = parse_ptt_set(&line) {
            asserted_ptt = v;
        }
        match handle_command(&line, backend.as_ref()) {
            Handled::Reply(r) => {
                if !r.is_empty() && writer.write_all(r.as_bytes()).is_err() {
                    break;
                }
            }
            Handled::Close => break,
        }
    }
    // Connection ended with PTT still asserted by this client → fail-safe unkey.
    // `set_ptt(false)` (broker_ptt(false)) is always honored and idempotent, so
    // this is safe even if Nexus itself is not transmitting.
    // ...UNLESS Nexus itself is transmitting: its own Rig is a client of this broker, and a
    // transient reconnect of that connection must not unkey the active over. Nexus's own TX
    // safety (idle self-heal + daemon-Drop key-up) still recovers a genuinely stuck rig once
    // Nexus stops wanting TX. This is the native-CI-V PTT-flicker fix.
    if asserted_ptt {
        if backend.owner_transmitting() {
            crate::civ::diag::note(
                "rigctld_server: client disconnected with PTT asserted, but Nexus is transmitting → fail-safe SKIPPED (flicker fix)",
            );
        } else {
            crate::civ::diag::note(
                "rigctld_server: a PTT-asserting client disconnected, Nexus not transmitting → fail-safe unkey",
            );
            backend.set_ptt(false);
        }
    }
    backend.client_event(&peer, false);
}

/// Run the broker accept loop, a thread per client. Blocks; spawn it on its own
/// thread. Backed by `backend` (shared with Nexus's rig).
pub fn serve(listener: TcpListener, backend: Arc<dyn RigBackend>) {
    for stream in listener.incoming().flatten() {
        // WinSock accept() inherits the listener's non-blocking mode (Linux doesn't).
        // serve_connection needs blocking reads — force it, in case a caller ever
        // hands us a non-blocking listener (the CivDaemon broker bug, generalized).
        let _ = stream.set_nonblocking(false);
        let b = Arc::clone(&backend);
        std::thread::spawn(move || serve_connection(stream, b));
    }
}

/// Like [`serve`], but returns (releasing the port) once `shutdown` is set — so the CAT
/// broker can be turned on/off or re-pointed at a new port WITHOUT restarting Nexus.
/// Accept is polled non-blocking ~5×/s so the flag is honored promptly.
pub fn serve_until(
    listener: TcpListener,
    backend: Arc<dyn RigBackend>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let _ = listener.set_nonblocking(true);
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false); // per-client blocking reads
                let b = Arc::clone(&backend);
                std::thread::spawn(move || serve_connection(stream, b));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(_) => break,
        }
    }
    // `listener` drops here → the port is released for a rebind.
}

/// What answered when we asked a TCP port whether it is a rigctld.
///
/// The distinction this type exists to make cost an operator a working station: "something is
/// listening" is NOT "a rigctld is listening". An SDR console's raw CAT server is also
/// something listening, and connecting to it as if it spoke rigctld produces a rig that never
/// answers and a message that blames the radio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortReply {
    /// Nothing there, or nothing said: connect refused, or connected and the port stayed
    /// quiet for the whole budget. A rigctld that is merely slow lands here too — which is
    /// the safe side, because it only costs a spawn attempt.
    Silent,
    /// A rigctld-protocol server (Hamlib's, or another Nexus's broker). Safe to share.
    Rigctld,
    /// Something answered, in some other protocol. Carries the first line it sent, trimmed
    /// and capped — evidence to quote back to the operator, never to act on silently.
    NotRigctld(String),
}

/// How much of a foreign greeting we keep. Thetis's is ~75 bytes; this is room for a long
/// fork banner without letting a chatty server write our error message for us.
const REPLY_SNIPPET_MAX: usize = 200;

/// Judge the bytes a probed port sent back. Pure, so it can be tested against real captures
/// instead of against a socket.
///
/// **Accept only what a rigctld actually says.** Hamlib 4.7.1 answers `\chk_vfo` with the bare
/// `vfo_opt` flag — `"0\n"` (or `"1\n"` under `-o`), printed by `rigctl_parse.c` with the
/// `RPRT` trailer suppressed. Nexus's own broker (and older Hamlib, per its man page) answers
/// `"CHKVFO 0\n"`. `RPRT <n>` is accepted as well: nothing but a rigctld-protocol server emits
/// it, and it means the port understood the command even if it refused it.
///
/// **The bias is deliberate.** A false negative costs one spawn attempt. A false positive is
/// the bug this replaces: a silent connection to the wrong kind of server. So anything we do
/// not recognise is [`PortReply::NotRigctld`], not a guess.
pub fn classify_probe_reply(bytes: &[u8]) -> PortReply {
    let text = String::from_utf8_lossy(bytes);
    // One line is the whole of a rigctld reply. A `;`-framed CAT greeting has no newline at
    // all, so this takes the lot — which is what we want to quote.
    let first = text
        .lines()
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .trim();
    if first.is_empty() {
        return PortReply::Silent;
    }
    if matches!(first, "0" | "1" | "CHKVFO 0" | "CHKVFO 1") || first.starts_with("RPRT ") {
        return PortReply::Rigctld;
    }
    let mut snippet: String = first.chars().take(REPLY_SNIPPET_MAX).collect();
    if snippet.chars().count() < first.chars().count() {
        snippet.push('…');
    }
    PortReply::NotRigctld(snippet)
}

/// Ask `addr` whether it is a rigctld we can share, and REPORT WHAT ANSWERED.
///
/// Sends `\chk_vfo` and reads one line back inside `timeout`. Two servers are being told
/// apart here, and both talk on connect-or-command: a rigctld says nothing until asked and
/// then answers one line; an SDR console's CAT server (Thetis, and PowerSDR forks) greets
/// immediately and then ignores anything without a `;`. Writing the question first and
/// reading once serves both — the greeting is already in the socket by then.
///
/// The read budget is `timeout` in total, not per read, so the worst case is unchanged from
/// the single-read version this replaces (connect + one timeout).
/// Ask `addr` whether it is a rigctld we can share, and REPORT WHAT ANSWERED.
///
/// Sends `\chk_vfo` and reads one line back inside `timeout`. Two servers are being told
/// apart here, and both talk on connect-or-command: a rigctld says nothing until asked and
/// then answers one line; an SDR console's CAT server (Thetis, and PowerSDR forks) greets
/// immediately and then ignores anything without a `;`. Writing the question first and
/// reading once serves both — the greeting is already in the socket by then.
///
/// The read budget is `timeout` in total, not per read, so the worst case is unchanged from
/// the single-read version this replaces (connect + one timeout).
/// Ask the rigctld on `addr` WHICH RIG MODEL it is serving, via `\dump_state`.
///
/// WHY THIS EXISTS. [`probe_cat_port`] establishes that *a rigctld* is listening, and the
/// coexist branch then connects through it. What it could never establish is which RADIO that
/// daemon drives — the monitor path's comment says exactly this, and refuses to coexist at all
/// for that reason, calling it "the dual-radio crossed-CAT bug". The active radio coexists
/// anyway, so it inherited the bug.
///
/// Demonstrated on a two-radio macOS station, 2026-08-17: a stray rigctld left by a previous run
/// held the FTX-1's port, Nexus adopted it, took its dial (145.000, a Hamlib dummy) as the rig's,
/// and left the real radio's serial port untouched. The pill looked fine.
///
/// The premise was simply out of date: `\dump_state`'s SECOND line is the served model. A Hamlib
/// dummy answers `1`, an FT-710 answers `1049`. That is enough to refuse a daemon that is driving
/// something other than the rig this profile describes.
///
/// `None` means "could not establish" — not "mismatch". A daemon that does not answer, answers
/// The model line of a `\dump_state` reply: line 1 is the protocol version, line 2 the model.
///
/// Split out so the parse is testable without a socket — and because the shape is the sort of
/// thing that changes between Hamlib versions, so it must fail to `None` rather than guess.
pub fn parse_dump_state_model(bytes: &[u8]) -> Option<u32> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let _protocol = lines.next()?;
    lines.next()?.trim().parse::<u32>().ok()
}

/// something unparseable, or speaks a protocol variant we do not recognise must NOT be treated as
/// foreign: refusing on absent evidence would break the sharing setups this branch exists for.
pub fn served_rig_model(addr: &str, timeout: std::time::Duration) -> Option<u32> {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    let sa = addr.to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&sa, timeout).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    stream.write_all(b"\\dump_state\n").ok()?;
    let deadline = std::time::Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    // Two lines is all we need (protocol version, then the model), but they can arrive in one
    // packet or several; stop as soon as a second newline is in hand.
    while buf.len() < 2048 && buf.iter().filter(|b| **b == b'\n').count() < 2 {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left < std::time::Duration::from_millis(1)
            || stream.set_read_timeout(Some(left)).is_err()
        {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    parse_dump_state_model(&buf)
}

pub fn probe_cat_port(addr: &str, timeout: std::time::Duration) -> PortReply {
    use std::io::Read;
    use std::net::ToSocketAddrs;
    let Ok(mut addrs) = addr.to_socket_addrs() else {
        return PortReply::Silent;
    };
    let Some(sa) = addrs.next() else {
        return PortReply::Silent;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&sa, timeout) else {
        return PortReply::Silent;
    };
    let _ = stream.set_write_timeout(Some(timeout));
    if stream.write_all(b"\\chk_vfo\n").is_err() {
        return PortReply::Silent;
    }
    let deadline = std::time::Instant::now() + timeout;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 256];
    while buf.len() < REPLY_SNIPPET_MAX * 2 {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        // Below a millisecond the timeval rounds to zero, which the OS reads as "block
        // forever" — never hand that to the socket.
        if left < std::time::Duration::from_millis(1)
            || stream.set_read_timeout(Some(left)).is_err()
        {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,  // peer closed
            Err(_) => break, // timed out / reset
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if buf.contains(&b'\n') {
            break; // a rigctld's answer is one line, and it has arrived
        }
    }
    classify_probe_reply(&buf)
}

/// Probe whether a rigctld (or compatible broker — maybe another Nexus) is already
/// listening on `addr`, so we can connect THROUGH it instead of fighting for the
/// serial port. Short timeout; never blocks startup long.
///
/// The boolean form, for callers that only branch. Callers that must EXPLAIN a failure want
/// [`probe_cat_port`] — the bytes are the evidence.
pub fn probe_rigctld(addr: &str, timeout: std::time::Duration) -> bool {
    matches!(probe_cat_port(addr, timeout), PortReply::Rigctld)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockRig {
        freq: Mutex<u64>,
        ptt: Mutex<bool>,
        mode: Mutex<(String, u32)>,
    }
    impl Default for MockRig {
        fn default() -> Self {
            MockRig {
                freq: Mutex::new(14_074_000),
                ptt: Mutex::new(false),
                mode: Mutex::new(("USB".into(), 2700)),
            }
        }
    }
    impl RigBackend for MockRig {
        fn freq_hz(&self) -> u64 {
            *self.freq.lock().unwrap()
        }
        fn mode(&self) -> (String, u32) {
            self.mode.lock().unwrap().clone()
        }
        fn ptt(&self) -> bool {
            *self.ptt.lock().unwrap()
        }
        fn set_freq(&self, hz: u64) -> bool {
            *self.freq.lock().unwrap() = hz;
            true
        }
        fn set_mode(&self, mode: &str, pbw: u32) -> bool {
            *self.mode.lock().unwrap() = (mode.to_string(), pbw);
            true
        }
        fn set_ptt(&self, on: bool) -> bool {
            *self.ptt.lock().unwrap() = on;
            true
        }
    }

    fn reply(line: &str, b: &dyn RigBackend) -> String {
        match handle_command(line, b) {
            Handled::Reply(r) => r,
            Handled::Close => "\0CLOSE".into(),
        }
    }

    /// #53 conformance: real rigctld accepts every verb in BOTH spellings — the short letter
    /// and the `\long_name` — and real clients mix them freely (VarAC sends long forms).
    /// The broker only knew the letters, so `\get_freq` answered RPRT -11: to the client,
    /// a rig that refuses to say its frequency. Worse, `parse_ptt_set` recognised
    /// `\set_ptt` while the dispatcher did not — the connection recorded a key-down that
    /// was never keyed, and the disconnect fail-safe then "un-keyed" a transmission that
    /// never happened (it fails SAFE, but it acts on a fiction). One canonicaliser, used
    /// by both, is the root fix.
    #[test]
    fn long_form_verbs_answer_exactly_like_their_short_letters() {
        let b = MockRig::default();
        assert_eq!(reply("\\get_freq", &b), "14074000\n");
        assert_eq!(reply("\\set_freq 7035000", &b), "RPRT 0\n");
        assert_eq!(*b.freq.lock().unwrap(), 7_035_000);
        assert_eq!(reply("\\get_mode", &b), "USB\n2700\n");
        assert_eq!(reply("\\set_mode FT8 3000", &b), "RPRT 0\n");
        assert_eq!(reply("\\get_ptt", &b), "0\n");
        assert_eq!(reply("\\set_ptt 1", &b), "RPRT 0\n");
        assert!(
            *b.ptt.lock().unwrap(),
            "\\set_ptt must actually key the backend"
        );
        assert_eq!(reply("\\set_ptt 0", &b), "RPRT 0\n");
        assert_eq!(reply("\\get_vfo", &b), "VFOA\n");
        assert_eq!(reply("\\get_split_vfo", &b), "0\nVFOA\n");
        // Hamlib's own long spellings for the extended verbs map too — unimplemented on this
        // mock, so they answer RPRT -11 exactly like their letters (NOT like unknown verbs
        // answering from half a parse).
        assert_eq!(reply("\\set_level RFPOWER 0.5", &b), "RPRT -11\n");
        assert_eq!(reply("\\get_level RFPOWER", &b), "RPRT -11\n");
        // An unknown long verb is still unknown.
        assert_eq!(reply("\\warble", &b), "RPRT -11\n");
    }

    /// The morse long form takes the rest of the line as text, exactly like `b`.
    #[test]
    fn long_form_send_morse_keeps_its_spaces() {
        struct MorseRig(Mutex<String>);
        impl RigBackend for MorseRig {
            fn freq_hz(&self) -> u64 {
                0
            }
            fn mode(&self) -> (String, u32) {
                ("USB".into(), 2700)
            }
            fn ptt(&self) -> bool {
                false
            }
            fn set_freq(&self, _: u64) -> bool {
                true
            }
            fn set_mode(&self, _: &str, _: u32) -> bool {
                true
            }
            fn set_ptt(&self, _: bool) -> bool {
                true
            }
            fn send_morse(&self, text: &str) -> Option<bool> {
                *self.0.lock().unwrap() = text.to_string();
                Some(true)
            }
        }
        let b = MorseRig(Mutex::new(String::new()));
        assert_eq!(reply("\\send_morse CQ CQ DE KD9TAW", &b), "RPRT 0\n");
        assert_eq!(*b.0.lock().unwrap(), "CQ CQ DE KD9TAW");
    }

    #[test]
    fn protocol_get_set_commands() {
        let b = MockRig::default();
        assert_eq!(reply("f", &b), "14074000\n");
        assert_eq!(reply("F 7035000", &b), "RPRT 0\n");
        assert_eq!(*b.freq.lock().unwrap(), 7_035_000);
        assert_eq!(reply("f", &b), "7035000\n");
        assert_eq!(reply("m", &b), "USB\n2700\n");
        assert_eq!(reply("M FT8 3000", &b), "RPRT 0\n");
        assert_eq!(reply("m", &b), "FT8\n3000\n");
        assert_eq!(reply("t", &b), "0\n");
        assert_eq!(reply("T 1", &b), "RPRT 0\n");
        assert_eq!(reply("t", &b), "1\n");
        assert_eq!(reply("v", &b), "VFOA\n");
        // Malformed set → error; unknown command → not-implemented. (`Z` became the real
        // XIT verb with the extended-verb dispatch, so use a genuinely unknown letter.)
        assert_eq!(reply("F notanumber", &b), "RPRT -1\n");
        assert_eq!(reply("G", &b), "RPRT -11\n");
        assert!(matches!(handle_command("q", &b), Handled::Close));
    }

    #[test]
    fn set_freq_accepts_hamlib_float_wire_form() {
        // Hamlib's netrigctl sends freq as printf %lf, e.g. `F 14074000.000000`.
        // A u64 parse rejected this and every real client's band change failed.
        let b = MockRig::default();
        assert_eq!(reply("F 14074000.000000", &b), "RPRT 0\n");
        assert_eq!(*b.freq.lock().unwrap(), 14_074_000);
        // Fractional Hz rounds to nearest.
        assert_eq!(reply("F 7035000.6", &b), "RPRT 0\n");
        assert_eq!(*b.freq.lock().unwrap(), 7_035_001);
        // Integer form (loggers that send it) and negatives/garbage still handled.
        assert_eq!(reply("F 21074000", &b), "RPRT 0\n");
        assert_eq!(*b.freq.lock().unwrap(), 21_074_000);
        assert_eq!(reply("F -1", &b), "RPRT -1\n");
        assert_eq!(reply("F notanumber", &b), "RPRT -1\n");
        // inf / NaN / absurd magnitudes must be rejected, not saturate the cast
        // to u64::MAX and set a garbage dial with a false RPRT 0.
        let last = *b.freq.lock().unwrap();
        assert_eq!(reply("F inf", &b), "RPRT -1\n");
        assert_eq!(reply("F 1e30", &b), "RPRT -1\n");
        assert_eq!(reply("F NaN", &b), "RPRT -1\n");
        assert_eq!(
            *b.freq.lock().unwrap(),
            last,
            "a rejected F must not move the dial"
        );
    }

    #[test]
    fn ptt_set_parser_tracks_key_state_for_failsafe_unkey() {
        // Any non-zero key-down state is "asserted" (matches the T handler); 0 is up.
        assert_eq!(parse_ptt_set("T 1"), Some(true));
        assert_eq!(parse_ptt_set("T 3"), Some(true)); // ON_DATA (WSJT-X rear audio)
        assert_eq!(parse_ptt_set("T 0"), Some(false));
        assert_eq!(parse_ptt_set("\\set_ptt 1"), Some(true)); // long form
        assert_eq!(parse_ptt_set("F 14074000"), None); // unrelated command
        assert_eq!(parse_ptt_set("T"), None); // malformed → don't change state
    }

    #[test]
    fn set_ptt_keys_on_any_nonzero_state() {
        // RIG_PTT_ON_MIC(2)/RIG_PTT_ON_DATA(3) are key-down; WSJT-X with a Rear/Data
        // audio source sends `T 3`. Keying only on "1" left the rig un-keyed on TX.
        let b = MockRig::default();
        assert_eq!(reply("T 3", &b), "RPRT 0\n");
        assert!(*b.ptt.lock().unwrap(), "T 3 (ON_DATA) must key the rig");
        assert_eq!(reply("T 0", &b), "RPRT 0\n");
        assert!(!*b.ptt.lock().unwrap(), "T 0 (OFF) must un-key");
        assert_eq!(reply("T 2", &b), "RPRT 0\n");
        assert!(*b.ptt.lock().unwrap(), "T 2 (ON_MIC) must key the rig");
        assert_eq!(reply("T 1", &b), "RPRT 0\n");
        assert!(*b.ptt.lock().unwrap(), "T 1 (ON) must key the rig");
        // Malformed PTT arg → error, no key change.
        assert_eq!(reply("T x", &b), "RPRT -1\n");
    }

    #[test]
    fn disconnect_failsafe_respects_owner_transmitting() {
        // A client that keyed (T 1) then dropped its connection: the fail-safe unkeys ONLY when
        // Nexus itself is NOT transmitting. When Nexus IS on the air (its own Rig reconnecting),
        // the fail-safe must stand down — else it steals the over (the IC-9700 CI-V flicker).
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        struct TxRig {
            ptt: Mutex<bool>,
            owner_tx: bool,
        }
        impl RigBackend for TxRig {
            fn freq_hz(&self) -> u64 {
                0
            }
            fn mode(&self) -> (String, u32) {
                ("USB".into(), 0)
            }
            fn ptt(&self) -> bool {
                *self.ptt.lock().unwrap()
            }
            fn set_freq(&self, _: u64) -> bool {
                true
            }
            fn set_mode(&self, _: &str, _: u32) -> bool {
                true
            }
            fn set_ptt(&self, on: bool) -> bool {
                *self.ptt.lock().unwrap() = on;
                true
            }
            fn owner_transmitting(&self) -> bool {
                self.owner_tx
            }
        }
        // (owner_transmitting, keyed-after-the-client-drops)
        for (owner_tx, keyed_after) in [(false, false), (true, true)] {
            let backend: Arc<dyn RigBackend> = Arc::new(TxRig {
                ptt: Mutex::new(false),
                owner_tx,
            });
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let b = backend.clone();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                serve_connection(stream, b);
            });
            let mut client = TcpStream::connect(addr).unwrap();
            client.write_all(b"T 1\n").unwrap();
            client.flush().unwrap();
            drop(client); // client vanished mid-key (crash / reconnect)
            server.join().unwrap(); // runs the fail-safe path to completion
            assert_eq!(
                backend.ptt(),
                keyed_after,
                "owner_transmitting={owner_tx}: rig should be {}keyed after the client drops",
                if keyed_after { "" } else { "un" }
            );
        }
    }

    /// The connection-log hook: one connected + one disconnected event per client, with the
    /// peer, so the operator can SEE their logger attach to the share address (#53).
    #[test]
    fn a_client_lifetime_is_two_logged_events() {
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        struct EventRig(Mutex<Vec<(String, bool)>>);
        impl RigBackend for EventRig {
            fn freq_hz(&self) -> u64 {
                0
            }
            fn mode(&self) -> (String, u32) {
                ("USB".into(), 2700)
            }
            fn ptt(&self) -> bool {
                false
            }
            fn set_freq(&self, _: u64) -> bool {
                true
            }
            fn set_mode(&self, _: &str, _: u32) -> bool {
                true
            }
            fn set_ptt(&self, _: bool) -> bool {
                true
            }
            fn client_event(&self, peer: &str, connected: bool) {
                self.0.lock().unwrap().push((peer.to_string(), connected));
            }
        }
        let backend = Arc::new(EventRig(Mutex::new(Vec::new())));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let b: Arc<dyn RigBackend> = backend.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, b);
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"f\nq\n").unwrap();
        client.flush().unwrap();
        server.join().unwrap();
        let events = backend.0.lock().unwrap().clone();
        assert_eq!(events.len(), 2, "one connect + one disconnect: {events:?}");
        assert!(events[0].1 && !events[1].1, "connected then disconnected");
        assert_eq!(events[0].0, events[1].0, "same peer on both events");
        assert!(
            events[0].0.starts_with("127.0.0.1:"),
            "the peer address is real: {}",
            events[0].0
        );
    }

    /// The other half of the canonicaliser fix, at the CONNECTION level: a long-form key
    /// really keys, so the drop fail-safe acts on a key-down that actually happened. Before,
    /// `\set_ptt 1` answered RPRT -11 (never keyed) while still setting `asserted_ptt`.
    #[test]
    fn a_long_form_key_is_real_so_the_drop_failsafe_unkeys_it() {
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        let backend: Arc<dyn RigBackend> = Arc::new(MockRig::default());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let b = backend.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, b);
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"\\set_ptt 1\n").unwrap();
        client.flush().unwrap();
        // Wait for the key to land before dropping, else the drop can race the write.
        for _ in 0..200 {
            if backend.ptt() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(backend.ptt(), "the long-form key must reach the backend");
        drop(client);
        server.join().unwrap();
        assert!(
            !backend.ptt(),
            "fail-safe unkeys the drop-while-keyed client"
        );
    }

    /// #53's mechanism, demonstrated and then pinned out of our broker.
    ///
    /// rogerloxton's VarAC log shows `Convert.ToInt64("14105000\n14105000\n")`: his read
    /// TIMED OUT behind the shared Hamlib daemon's serial round-trip, he re-asked, the late
    /// reply arrived alongside the second, and VarAC — which never drains — stayed one reply
    /// behind forever. FIRST a fake slow server reproduces those exact bytes (the positive
    /// control: the diagnosis, made mechanical); THEN the same impatient client against OUR
    /// broker gets clean one-reply-per-read framing, because the broker answers from cached
    /// state in microseconds — there is no stall to time out on.
    #[test]
    fn a_timed_out_reask_doubles_replies_on_a_slow_server_but_not_on_the_broker() {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        // -- the impatient, never-draining client (VarAC-shaped) --------------------------
        // ask `f`; read once with a short timeout; on timeout ask again; then read once.
        fn varac_shaped_read(addr: std::net::SocketAddr, patience: Duration) -> String {
            let mut c = TcpStream::connect(addr).unwrap();
            c.set_read_timeout(Some(patience)).unwrap();
            c.write_all(b"f\n").unwrap();
            let mut buf = [0u8; 256];
            let n = match c.read(&mut buf) {
                Ok(n) => n,
                Err(_) => {
                    // timed out → re-ask, then read whatever has arrived by now
                    c.write_all(b"f\n").unwrap();
                    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    std::thread::sleep(Duration::from_millis(300)); // let BOTH replies land
                    c.read(&mut buf).unwrap_or(0)
                }
            };
            String::from_utf8_lossy(&buf[..n]).into_owned()
        }

        // -- control: a rigctld whose serial round-trip outlives the client's patience ----
        let slow = TcpListener::bind("127.0.0.1:0").unwrap();
        let slow_addr = slow.local_addr().unwrap();
        std::thread::spawn(move || {
            let (stream, _) = slow.accept().unwrap();
            let mut w = stream.try_clone().unwrap();
            let r = std::io::BufReader::new(stream);
            for line in r.lines() {
                let Ok(line) = line else { break };
                if line.trim() == "f" {
                    std::thread::sleep(Duration::from_millis(150)); // the serial stall
                    let _ = w.write_all(b"14105000\n");
                }
            }
        });
        let doubled = varac_shaped_read(slow_addr, Duration::from_millis(30));
        assert_eq!(
            doubled, "14105000\n14105000\n",
            "control: the slow-daemon path must reproduce the exact bytes from the issue's log"
        );

        // -- our broker under the same client: no stall, so the timeout never fires -------
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let backend: Arc<dyn RigBackend> = Arc::new(MockRig::default());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, backend);
        });
        let clean = varac_shaped_read(addr, Duration::from_millis(1000));
        assert_eq!(
            clean, "14074000\n",
            "the broker answers within the client's patience — one ask, one reply"
        );
    }

    #[test]
    fn extended_verbs_default_to_not_implemented() {
        // A backend that only fills in the CORE set (like the live CAT broker) must behave
        // byte-identically to the pre-extension broker: every extended verb → RPRT -11.
        let b = MockRig::default();
        for cmd in [
            "l STRENGTH",
            "L RFPOWER 0.5",
            "u RIT",
            "U RIT 1",
            "b CQ CQ DE W9XYZ",
            "\\stop_morse",
            "S 1 VFOB",
            "I 14076000",
            "J -50",
            "Z 100",
            "R +",
            "O 600000",
            "C 1000",
        ] {
            assert_eq!(reply(cmd, &b), "RPRT -11\n", "default for {cmd:?}");
        }
        // Malformed args on extended verbs → RPRT -1 (error), not -11.
        assert_eq!(reply("J notanumber", &b), "RPRT -1\n");
        assert_eq!(reply("L RFPOWER", &b), "RPRT -1\n");
    }

    /// A backend implementing the extended verbs (like the native CI-V daemon).
    struct ExtRig {
        base: MockRig,
        morse: Mutex<Vec<String>>,
        rit: Mutex<i32>,
    }
    impl RigBackend for ExtRig {
        fn freq_hz(&self) -> u64 {
            self.base.freq_hz()
        }
        fn mode(&self) -> (String, u32) {
            self.base.mode()
        }
        fn ptt(&self) -> bool {
            self.base.ptt()
        }
        fn set_freq(&self, hz: u64) -> bool {
            self.base.set_freq(hz)
        }
        fn set_mode(&self, m: &str, p: u32) -> bool {
            self.base.set_mode(m, p)
        }
        fn set_ptt(&self, on: bool) -> bool {
            self.base.set_ptt(on)
        }
        fn level(&self, name: &str) -> Option<String> {
            match name {
                "STRENGTH" => Some("-12".to_string()),
                "RFPOWER" => Some("0.50".to_string()),
                _ => None,
            }
        }
        fn set_level(&self, name: &str, _v: &str) -> Option<bool> {
            Some(name == "RFPOWER" || name == "KEYSPD")
        }
        fn func(&self, token: &str) -> Option<bool> {
            (token == "RIT").then_some(true)
        }
        fn send_morse(&self, text: &str) -> Option<bool> {
            self.morse.lock().unwrap().push(text.to_string());
            Some(true)
        }
        fn stop_morse(&self) -> Option<bool> {
            Some(true)
        }
        fn set_rit(&self, hz: i32) -> Option<bool> {
            *self.rit.lock().unwrap() = hz;
            Some(true)
        }
    }

    #[test]
    fn extended_verbs_dispatch_to_an_implementing_backend() {
        let b = ExtRig {
            base: MockRig::default(),
            morse: Mutex::new(Vec::new()),
            rit: Mutex::new(0),
        };
        // Levels: `l` replies the value line; `L` acks.
        assert_eq!(reply("l STRENGTH", &b), "-12\n");
        assert_eq!(reply("l RFPOWER", &b), "0.50\n");
        assert_eq!(reply("l NOSUCH", &b), "RPRT -11\n");
        assert_eq!(reply("L RFPOWER 0.5", &b), "RPRT 0\n");
        // Funcs.
        assert_eq!(reply("u RIT", &b), "1\n");
        assert_eq!(reply("u NOSUCH", &b), "RPRT -11\n");
        // Morse keeps its spaces (the whole rest of the line is the message).
        assert_eq!(reply("b CQ CQ DE W9XYZ K", &b), "RPRT 0\n");
        assert_eq!(b.morse.lock().unwrap()[0], "CQ CQ DE W9XYZ K");
        assert_eq!(reply("\\stop_morse", &b), "RPRT 0\n");
        // RIT (negative offsets parse).
        assert_eq!(reply("J -120", &b), "RPRT 0\n");
        assert_eq!(*b.rit.lock().unwrap(), -120);
    }

    #[test]
    fn dump_state_and_chk_vfo_shape() {
        let b = MockRig::default();
        assert_eq!(reply("\\chk_vfo", &b), "CHKVFO 0\n");
        let ds = reply("\\dump_state", &b);
        let lines: Vec<&str> = ds.lines().collect();
        assert_eq!(lines[0], "0", "protocol version 0 (skips proto-1 trailer)");
        assert_eq!(lines[2], "1", "ITU region");
        // Range rows are 7 fields; terminators are all-zero.
        assert!(lines.contains(&"0 0 0 0 0 0 0"), "range terminator present");
        assert!(lines.contains(&"0 0"), "ts/filter terminator present");
        // Every line parses as the expected token shape (no stray text).
        assert!(lines[3].split_whitespace().count() == 7);
    }

    #[test]
    fn broker_serves_a_localhost_client_end_to_end() {
        let backend: Arc<dyn RigBackend> = Arc::new(MockRig::default());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let b = Arc::clone(&backend);
        std::thread::spawn(move || serve(listener, b));

        let mut client = TcpStream::connect(addr).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut rd = BufReader::new(client.try_clone().unwrap());

        let mut line = String::new();
        client.write_all(b"f\n").unwrap();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "14074000\n");

        // A logger sets the dial through the broker → Nexus's backend updates.
        client.write_all(b"F 21074000\n").unwrap();
        line.clear();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "RPRT 0\n");
        assert_eq!(backend.freq_hz(), 21_074_000);

        // And it reads back the new frequency.
        client.write_all(b"f\n").unwrap();
        line.clear();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "21074000\n");
    }

    #[test]
    fn serve_until_serves_then_stops_and_frees_the_port_on_shutdown() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let backend: Arc<dyn RigBackend> = Arc::new(MockRig::default());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let sd = shutdown.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            serve_until(listener, backend, sd);
            let _ = done_tx.send(()); // signals the accept loop returned
        });

        // It's live: a client can query freq through it.
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut rd = BufReader::new(client.try_clone().unwrap());
        client.write_all(b"f\n").unwrap();
        let mut line = String::new();
        rd.read_line(&mut line).unwrap();
        assert_eq!(line, "14074000\n");

        // Flip shutdown → the loop exits (hot-disable, no restart) and the port frees.
        shutdown.store(true, Ordering::Relaxed);
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok(),
            "serve_until returns once shutdown is set"
        );
        assert!(
            TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "the port is released for a rebind"
        );
    }

    #[test]
    fn probe_detects_a_running_broker_and_absence() {
        // A bound broker is detected (connect-through path); a dead port is not.
        let backend: Arc<dyn RigBackend> = Arc::new(MockRig::default());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let b = Arc::clone(&backend);
        std::thread::spawn(move || serve(listener, b));
        let to = std::time::Duration::from_millis(500);
        assert!(
            probe_rigctld(&addr.to_string(), to),
            "running broker detected"
        );
        // An unused high port: nothing there.
        assert!(
            !probe_rigctld("127.0.0.1:1", to),
            "no broker on a dead port"
        );
    }

    /// The exact bytes a Hermes Lite 2 operator's Thetis sent Nexus (field report 2026-08).
    /// Thetis's TCP/IP CAT server greets every client on connect with
    /// `#Thetis TCP/IP Cat - <window title>#;`.
    const THETIS_BANNER: &str =
        "#Thetis TCP/IP Cat - Thetis v2.10.3.13 x64 (04/01/26) HL2 Beta 2 (MI0BOT)#;";

    /// Stand up a listener that behaves like Thetis's TCP/IP CAT server: greet on connect,
    /// then answer nothing that isn't `;`-framed (`\chk_vfo\n` carries no `;`, so
    /// `ParseReceiveBuffer` discards it and never replies). Returns its address.
    fn fake_thetis() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(4) {
                let Ok(mut s) = stream else { continue };
                let _ = s.write_all(THETIS_BANNER.as_bytes());
                let _ = s.flush();
                // Hold the socket open, silent, the way the real server does.
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
        });
        addr
    }

    /// THE ROOT CAUSE of the Thetis field report. The probe used to end in
    /// `matches!(stream.read(&mut buf), Ok(n) if n > 0)` — true on ANY bytes, never once
    /// looking at them. Thetis's greeting IS those bytes, so a raw Kenwood-dialect CAT port
    /// read as "a rigctld is already here", `open_cat` took the coexist branch, and Nexus
    /// spoke rigctld protocol at a rig-CAT server until the deadline expired.
    #[test]
    fn a_raw_cat_greeting_is_not_a_rigctld() {
        let addr = fake_thetis();
        let to = std::time::Duration::from_millis(600);
        assert!(
            !probe_rigctld(&addr, to),
            "a CAT server's greeting is not a rigctld handshake"
        );
        // And the greeting comes BACK, verbatim — it is the evidence the operator-facing
        // message quotes, and the only thing that lets us name the program.
        assert_eq!(
            probe_cat_port(&addr, to),
            PortReply::NotRigctld(THETIS_BANNER.to_string())
        );
    }

    /// What each kind of answer means, from real captures. The accept side is the one that
    /// must stay wide: a rigctld we fail to recognise gets a needless spawn attempt.
    #[test]
    fn probe_reply_classification() {
        use PortReply::*;
        // Hamlib 4.7.1: `chk_vfo` prints the bare vfo_opt flag, RPRT suppressed. `1` is the
        // same daemon started with `-o`.
        assert_eq!(classify_probe_reply(b"0\n"), Rigctld);
        assert_eq!(classify_probe_reply(b"1\n"), Rigctld);
        // Nexus's own broker, and older Hamlib per its man page.
        assert_eq!(classify_probe_reply(b"CHKVFO 0\n"), Rigctld);
        assert_eq!(classify_probe_reply(b"CHKVFO 0\r\n"), Rigctld);
        // Understood the command and refused it — still a rigctld.
        assert_eq!(classify_probe_reply(b"RPRT -11\n"), Rigctld);
        // Nothing said.
        assert_eq!(classify_probe_reply(b""), Silent);
        assert_eq!(classify_probe_reply(b"\r\n"), Silent);
        // Raw rig CAT: `;`-framed, no newline. THE FIELD REPORT.
        assert_eq!(
            classify_probe_reply(THETIS_BANNER.as_bytes()),
            NotRigctld(THETIS_BANNER.to_string())
        );
        // A bare Kenwood-dialect answer, and a web server on the port — neither is a rigctld.
        assert_eq!(
            classify_probe_reply(b"FA00014074000;"),
            NotRigctld("FA00014074000;".into())
        );
        assert_eq!(
            classify_probe_reply(b"HTTP/1.1 400 Bad Request\r\nServer: nginx\r\n"),
            NotRigctld("HTTP/1.1 400 Bad Request".into())
        );
        // A chatty server does not get to write our error message.
        let flood = "x".repeat(500);
        let Some(NotRigctld(s)) = Some(classify_probe_reply(flood.as_bytes())) else {
            panic!("a flood of bytes is not a rigctld")
        };
        assert_eq!(
            s.chars().count(),
            REPLY_SNIPPET_MAX + 1,
            "capped, with an ellipsis"
        );
    }
    /// `\dump_state`'s second line is the served rig's model — the fact that lets the coexist
    /// branch ask "is this MY radio?" instead of assuming. Both replies below are real: the
    /// Hamlib dummy and an FT-710, captured from the station on 2026-08-17.
    #[test]
    fn dump_state_names_the_rig_the_daemon_is_serving() {
        let dummy = b"1\n1\n0\n150000.000000 1500000000.000000 0x1ff -1 -1 0x17e00007 0x1f\n";
        let ft710 = b"1\n1049\n0\n30000.000000 60000000.000000 0x420201dbf -1 -1 0x10000003\n";
        assert_eq!(parse_dump_state_model(dummy), Some(1));
        assert_eq!(parse_dump_state_model(ft710), Some(1049));
    }

    /// EVERY "cannot tell" must be None, never a wrong number: refusing on a misread reply would
    /// take CAT away from the sharing setups the coexist branch exists to support.
    #[test]
    fn an_unreadable_dump_state_is_undecided_rather_than_wrong() {
        assert_eq!(parse_dump_state_model(b""), None);
        assert_eq!(parse_dump_state_model(b"1\n"), None, "no model line yet");
        assert_eq!(
            parse_dump_state_model(b"1\nRPRT -1\n"),
            None,
            "an error reply"
        );
        assert_eq!(parse_dump_state_model(b"garbage\nalso garbage\n"), None);
        // Blank lines must not shift the line the model is read from.
        assert_eq!(parse_dump_state_model(b"1\n\n1049\n"), Some(1049));
    }
}
