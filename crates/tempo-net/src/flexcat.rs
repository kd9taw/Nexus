//! FlexRadio SmartSDR TCP API client (port 4992) — the command/status channel used to create and
//! steer a *panadapter* display object so the radio streams its FFT to us (the actual FFT frames
//! arrive over VITA-49 UDP, parsed in [`crate::flexvita`]).
//!
//! The wire protocol is line-based ASCII:
//!   * the radio greets with `V<version>` then `H<handle-hex>`;
//!   * the client sends commands `C<seq>|<command>`;
//!   * the radio replies `R<seq>|<code-hex>|<response>` and pushes async `S<handle>|<body>` status
//!     and `M<code-hex>|<text>` messages.
//!
//! The PURE protocol layer (line + pan-status parsing, command encoding) is unit-tested here; the
//! socket orchestration ([`FlexCat`]) needs a real radio and is validated on-air.
//!
//! HONESTY NOTE: written to the published SmartSDR Ethernet API + the open-source FlexLib, and
//! unit-tested against synthetic frames — NOT yet confirmed against live hardware (no Flex on the
//! dev LAN). The UI gates the native panadapter behind this until an operator confirms it.
//!
//! PROVENANCE: original implementation of FlexRadio's PUBLICLY-DOCUMENTED SmartSDR Ethernet API
//! (github.com/flexradio/smartsdr-api-docs) + the open VITA-49 standard; NO FlexLib/vendor SDK
//! code is copied or bundled. "FlexRadio"/"SmartSDR" are trademarks used NOMINATIVELY (to state
//! compatibility only). See the repo-root `NOTICE`, "Rig-control protocol interoperability".

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// The SmartSDR API TCP port.
pub const FLEX_API_PORT: u16 = 4992;

/// Ceiling on the frames [`FlexCat::command`] parks for [`FlexCat::recv`] (see `stash`). Deep
/// enough for a whole reply window's worth of status on a busy radio, bounded so a caller that
/// only ever issues commands cannot turn the radio's status stream into unbounded memory growth.
const PENDING_CAP: usize = 512;

/// How long [`FlexCat::connect`] may spend opening the TCP session before giving up.
///
/// ⚠️ THIS BOUND IS RADIO-LOOP LIVENESS, not politeness. `connect` is the FIRST statement of both
/// Flex worker control threads, and a thread parked in a bare `TcpStream::connect` cannot observe
/// its `stop` flag — so the worker's `Drop`, which runs ON THE RADIO LOOP (a settings save, a radio
/// switch, the DAX starvation fallback), waited out the OS SYN timeout: ~21 s on Windows, ~127 s on
/// Linux for a black-holed address, e.g. a home LAN IP typed into the Flex field and then used from
/// away (Flex audit 2026-08-17, finding #1003). While that join blocks, `step()` does not run: no
/// capture drain, no slot boundary, no PTT deadline, no tune ceiling — the loop that unkeys the
/// transmitter must never be blockable by a network peer. Every other network client in the tree
/// already bounds its connect (n3fjp, aprsis, mqtt, dxkeeper, cluster, rigctld_server); this one
/// did not.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// One decoded line from the radio's TCP stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlexMsg {
    /// `V<version>` — the API version greeting.
    Version(String),
    /// `H<handle-hex>` — our client handle.
    Handle(u32),
    /// `R<seq>|<code-hex>|<response>` — a reply to command `seq` (`code` 0 = success).
    Reply { seq: u32, code: u32, msg: String },
    /// `S<handle-hex>|<body>` — an async status update (e.g. a `display pan …` object changed).
    Status { handle: u32, body: String },
    /// `M<code-hex>|<text>` — an async human-readable message from the radio.
    Message { code: u32, text: String },
    /// Anything we don't recognize (kept for logging, never fatal).
    Unknown(String),
}

/// Parse one newline-stripped line from the SmartSDR TCP stream. Pure — no I/O.
pub fn parse_line(line: &str) -> FlexMsg {
    let line = line.trim_end_matches(['\r', '\n']);
    let Some((tag, rest)) = line.split_at_checked(1) else {
        return FlexMsg::Unknown(line.to_string());
    };
    match tag {
        "V" => FlexMsg::Version(rest.to_string()),
        "H" => u32::from_str_radix(rest.trim(), 16)
            .map(FlexMsg::Handle)
            .unwrap_or_else(|_| FlexMsg::Unknown(line.to_string())),
        "R" => {
            // R<seq>|<code-hex>|<response>
            let mut it = rest.splitn(3, '|');
            let seq = it.next().and_then(|s| s.trim().parse::<u32>().ok());
            let code = it
                .next()
                .and_then(|s| u32::from_str_radix(s.trim(), 16).ok());
            match (seq, code) {
                (Some(seq), Some(code)) => FlexMsg::Reply {
                    seq,
                    code,
                    msg: it.next().unwrap_or("").to_string(),
                },
                _ => FlexMsg::Unknown(line.to_string()),
            }
        }
        "S" => {
            // S<handle-hex>|<body>
            let (h, body) = rest.split_once('|').unwrap_or((rest, ""));
            u32::from_str_radix(h.trim(), 16)
                .map(|handle| FlexMsg::Status {
                    handle,
                    body: body.to_string(),
                })
                .unwrap_or_else(|_| FlexMsg::Unknown(line.to_string()))
        }
        "M" => {
            let (c, text) = rest.split_once('|').unwrap_or((rest, ""));
            let code = u32::from_str_radix(c.trim(), 16).unwrap_or(0);
            FlexMsg::Message {
                code,
                text: text.to_string(),
            }
        }
        _ => FlexMsg::Unknown(line.to_string()),
    }
}

/// The fields we care about from a `display pan 0x<id> …` status body: the panadapter's stream id
/// (which the UDP FFT packets are tagged with) plus its current center/bandwidth so we can label
/// the display. All optional — a status update carries only the keys that changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanStatus {
    /// Panadapter object id (`0x40000000`-style), present on the create reply / first status.
    pub pan_id: Option<u32>,
    /// The VITA stream id the FFT UDP packets carry (may differ from `pan_id`).
    pub stream_id: Option<u32>,
    pub center_mhz: Option<f64>,
    pub bandwidth_mhz: Option<f64>,
    pub x_pixels: Option<u32>,
}

/// A `stream 0x<id> type=dax_rx dax_channel=<n> …` status object — how the radio tells us the VITA
/// stream id assigned to a DAX RX channel we created.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DaxStreamStatus {
    pub stream_id: Option<u32>,
    pub dax_channel: Option<u8>,
    /// The `client_handle` the stream belongs to (so we only accept OUR streams).
    pub client_handle: Option<u32>,
}

/// Parse a `stream 0x<id> type=dax_rx dax_channel=<n> client_handle=0x<h> …` status body. Returns
/// `None` unless it's a `stream …` line whose `type` is `dax_rx`. Pure.
pub fn parse_dax_stream_status(body: &str) -> Option<DaxStreamStatus> {
    let rest = body.strip_prefix("stream ")?;
    let mut it = rest.split_whitespace();
    let mut st = DaxStreamStatus::default();
    if let Some(first) = it.next() {
        st.stream_id = parse_hex_id(first);
    }
    let mut is_dax_rx = false;
    for tok in it {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "type" => is_dax_rx = v == "dax_rx",
            "dax_channel" | "daxiq_channel" => st.dax_channel = v.parse().ok(),
            "client_handle" => st.client_handle = parse_hex_id(v),
            _ => {}
        }
    }
    is_dax_rx.then_some(st)
}

/// One meter DEFINITION from the control plane — the `id → (source, name, unit)` registry the VITA
/// `0x8002` value packets are decoded against. Flex meter ids are per-session, so consumers match on
/// `source` + `name` (e.g. SLC/LEVEL = S-meter), never the numeric id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeterDef {
    pub index: u16,
    /// Meter source: `SLC` (slice), `TX`/`TX-…`, `RAD`, `AMP`, `COD-…`.
    pub source: String,
    /// Meter name: `LEVEL` (S-meter), `FWDPWR`, `REFPWR`, `SWR`, `ALC`, `MICPEAK`, `PATEMP`, …
    pub name: String,
    /// Unit string driving [`crate::flexvita::convert_meter_raw`]: `dBm`/`dBFS`/`SWR`/`Volts`/…
    pub unit: String,
}

/// Parse a `meter <i>.src=…#<i>.nam=…#<i>.unit=…#…` status body into meter definitions. Unusually,
/// the tokens are `#`-separated (not space-separated) and prefixed by the meter index, and a line
/// can define several meters at once — grouped here by index. A `meter <i> removed` line yields
/// nothing. Pure.
pub fn parse_meter_defs(body: &str) -> Vec<MeterDef> {
    let Some(rest) = body.strip_prefix("meter ") else {
        return Vec::new();
    };
    let mut by_index: std::collections::BTreeMap<u16, MeterDef> = std::collections::BTreeMap::new();
    for tok in rest.split('#') {
        let Some((lhs, value)) = tok.split_once('=') else {
            continue; // e.g. "<i> removed" — not a key=value
        };
        let Some((idx_str, key)) = lhs.split_once('.') else {
            continue;
        };
        let Ok(idx) = idx_str.trim().parse::<u16>() else {
            continue;
        };
        let def = by_index.entry(idx).or_default();
        def.index = idx;
        match key {
            "src" => def.source = value.to_string(),
            "nam" => def.name = value.to_string(),
            "unit" => def.unit = value.to_string(),
            _ => {}
        }
    }
    by_index.into_values().collect()
}

/// A `slice <n> key=value …` status object — which slice is the focused RX (`active=1`), which is
/// the TX slice (`tx=1`, distinct from active), and its DAX channel. Used to bind DAX to the REAL
/// active slice instead of assuming slice 0.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SliceStatus {
    /// Slice index (decimal, e.g. `slice 0`).
    pub num: u32,
    /// Slice occupied (`in_use=1`); `in_use=0` = removed.
    pub in_use: Option<bool>,
    /// The focused RX slice.
    pub active: Option<bool>,
    /// THE TX slice (`tx=1`) — may differ from the active RX slice, and is NOT PTT.
    pub tx_slice: Option<bool>,
    /// The slice's DAX RX channel assignment (`dax=<n>`).
    pub dax_channel: Option<u8>,
    /// Owning client (hex) — a slice is ours iff this matches our client handle.
    pub client_handle: Option<u32>,
}

/// Parse a `slice <n> key=value …` status body. `None` if it isn't a `slice` line. Pure.
pub fn parse_slice_status(body: &str) -> Option<SliceStatus> {
    let rest = body.strip_prefix("slice ")?;
    let mut it = rest.split_whitespace();
    let num = it.next()?.parse::<u32>().ok()?;
    let mut st = SliceStatus {
        num,
        ..Default::default()
    };
    for tok in it {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "in_use" => st.in_use = Some(v == "1"),
            "active" => st.active = Some(v == "1"),
            "tx" => st.tx_slice = Some(v == "1"),
            "dax" => st.dax_channel = v.parse().ok(),
            "client_handle" => st.client_handle = parse_hex_id(v),
            _ => {}
        }
    }
    Some(st)
}

/// Parse the stream id from a `stream create …` command REPLY body — `stream=0x…`, `id=0x…`, or a
/// bare `0x…`/decimal handle. Used to learn our dax_tx stream id from the create reply. Pure.
pub fn parse_create_stream_id(body: &str) -> Option<u32> {
    for tok in body.split_whitespace() {
        if let Some(v) = tok
            .strip_prefix("stream=")
            .or_else(|| tok.strip_prefix("id="))
        {
            if let Some(id) = parse_hex_id(v) {
                return Some(id);
            }
        }
    }
    parse_hex_id(body.split_whitespace().next()?)
}

/// Parse a `display pan 0x<id> key=value …` status body into the fields we track. Pure.
/// Returns `None` when the body isn't a `display pan` line.
pub fn parse_pan_status(body: &str) -> Option<PanStatus> {
    let rest = body.strip_prefix("display pan ")?;
    let mut it = rest.split_whitespace();
    let mut st = PanStatus::default();
    // First token after "display pan " is the object id (0x…), the rest are key=value.
    if let Some(first) = it.next() {
        st.pan_id = parse_hex_id(first);
    }
    for tok in it {
        let Some((k, v)) = tok.split_once('=') else {
            continue;
        };
        match k {
            "center" => st.center_mhz = v.parse().ok(),
            "bandwidth" => st.bandwidth_mhz = v.parse().ok(),
            "x_pixels" | "xpixels" => st.x_pixels = v.parse().ok(),
            "stream_id" | "client_handle" => {} // handled below via explicit keys if present
            _ => {}
        }
        if k == "stream_id" {
            st.stream_id = parse_hex_id(v);
        }
    }
    Some(st)
}

/// Parse a `0x…`-or-decimal object/stream id.
fn parse_hex_id(s: &str) -> Option<u32> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

/// Encode a command line `C<seq>|<command>` (newline included).
pub fn encode_command(seq: u32, command: &str) -> String {
    format!("C{seq}|{command}\n")
}

/// The outcome of one [`FlexCat::recv`].
///
/// ⚠️ `Idle` AND `Closed` ARE NOT THE SAME THING, and collapsing them into `None` is what turned a
/// dropped connection into a busy-spin (Flex audit 2026-08-17, finding #1000). The reader thread
/// owns the only `Sender` and `break`s out on peer-close or a hard I/O error, which DISCONNECTS the
/// channel — and `recv_timeout` on a disconnected channel returns INSTANTLY, it does not wait out
/// the timeout. A worker loop whose only pacing is that timeout then free-runs: the panadapter
/// thread took the shared engine mutex twice per iteration, millions of iterations a second, and
/// starved the radio loop and every Tauri command behind it until the operator turned the feature
/// off. Callers must treat `Closed` as terminal — exit (or reconnect with backoff), never continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlexRecv {
    /// A parsed line from the radio.
    Msg(FlexMsg),
    /// The timeout elapsed with nothing to read. The connection is still up.
    Idle,
    /// The reader thread is gone: the radio closed the session or the socket errored. TERMINAL —
    /// every later `recv` returns this immediately.
    Closed,
}

/// A live TCP connection to a Flex's SmartSDR API. Spawns a reader thread that parses lines into a
/// channel; `command` sends a `C…` line and awaits the matching `R…` reply.
pub struct FlexCat {
    stream: TcpStream,
    rx: mpsc::Receiver<FlexMsg>,
    seq: u32,
    /// OUR SmartSDR client handle, from the `H<hex>` greeting — the only thing on this wire that
    /// tells our objects from another client's.
    ///
    /// ⚠️ WHY IT LIVES ON THE READER THREAD AND NOT IN THE CALLER'S LOOP. A Flex is a MULTI-CLIENT
    /// radio: the SmartSDR GUI, a Maestro, N1MM and a second Nexus window can all be attached, and
    /// every `sub … all` subscription this crate issues delivers OTHER clients' objects too. The
    /// handle was parsed into [`FlexMsg::Handle`] and then dropped on the floor by both worker
    /// loops (their only arm is `Status`), so nothing downstream could answer "is this pan / this
    /// stream / this slice mine?" — the root the 2026-08-17 Flex audit put under findings
    /// #1012/#1013/#1014/#1016/#1017. Capturing it HERE means neither a worker's `_` arm nor a
    /// `command` reply window (which drains the channel) can lose it.
    ///
    /// NEEDS-BENCH — the second half of #1017, deliberately not implemented: SmartSDR v3's
    /// `client bind client_id=<gui client id>` associates this TCP session with a GUI client so
    /// slice/pan writes land in the right place on a multiFLEX radio. Whether an UNBOUND client's
    /// writes are accepted, refused, or land on a foreign client's objects is the radio's call and
    /// cannot be settled from source. RECIPE: with SmartSDR GUI running, attach Nexus, run
    /// `sub client all`, note the `client 0x… client_id=…` line, then compare `slice set` /
    /// `display pan create` behaviour with and without a preceding `client bind`.
    handle: Arc<Mutex<Option<u32>>>,
    /// Frames [`command`](Self::command) pulled off the channel while waiting for its reply, so
    /// [`recv`](Self::recv) still delivers them — see `stash`.
    pending: Mutex<VecDeque<FlexMsg>>,
}

impl FlexCat {
    /// Connect to `ip:4992` and start the reader thread. Reads the `V`/`H` greeting is left to the
    /// caller (they arrive as the first messages on the channel).
    pub fn connect(ip: &str) -> std::io::Result<FlexCat> {
        // BOUNDED — see `CONNECT_TIMEOUT`. `connect_timeout` needs a resolved `SocketAddr`, so the
        // (overwhelmingly common) literal-IP case parses straight through with no name lookup at
        // all; a hostname falls back to `ToSocketAddrs`, matching `n3fjp::connect`.
        let addr = format!("{ip}:{FLEX_API_PORT}");
        let target: SocketAddr = addr.parse().or_else(|_| {
            addr.to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "unresolvable Flex API address",
                    )
                })
        })?;
        Self::connect_addr(target, CONNECT_TIMEOUT)
    }

    /// The socket half of [`FlexCat::connect`], split out so the reader-thread lifecycle is
    /// testable against a local listener — the API port is fixed, so a test cannot reach this
    /// through `connect` without squatting on 4992.
    fn connect_addr(target: SocketAddr, timeout: Duration) -> std::io::Result<FlexCat> {
        let stream = TcpStream::connect_timeout(&target, timeout)?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        let (tx, rx) = mpsc::channel();
        let reader = stream.try_clone()?;
        let handle = Arc::new(Mutex::new(None::<u32>));
        let handle_rd = handle.clone();
        std::thread::spawn(move || {
            let mut buf = BufReader::new(reader);
            let mut line = String::new();
            loop {
                match buf.read_line(&mut line) {
                    Ok(0) => break, // peer closed
                    Ok(_) => {
                        let msg = parse_line(&line);
                        // A WHOLE line was consumed — only NOW may the buffer be reset (see the
                        // timeout arm).
                        line.clear();
                        // Our client handle, captured for every consumer of this session (see
                        // `FlexCat::handle`). Still forwarded on the channel: this is a tee, not
                        // a filter.
                        if let FlexMsg::Handle(h) = msg {
                            *handle_rd.lock().unwrap() = Some(h);
                        }
                        if tx.send(msg).is_err() {
                            break; // consumer dropped
                        }
                    }
                    // ⚠️ THE PARTIAL LINE STAYS IN THE BUFFER (Flex audit 2026-08-17, #1004).
                    // `read_line` COMMITS the bytes it appended before a timeout — std's
                    // `append_to_string` guard keeps them when they are valid UTF-8 — so the
                    // `line.clear()` that used to head this loop threw away the first half of any
                    // status the 500 ms read timeout happened to split, and then handed the TAIL
                    // to `parse_line` as if it were a whole line, where a leading `0` matches no
                    // tag and yields `Unknown`. Corruption, not just delay: a lost `display pan`
                    // or `stream … type=dax_rx` status is the ONE place the pan / DAX stream ids
                    // are learned, and a lost `meter` definition is an S-meter that never appears.
                    // It takes a >500 ms mid-line stall — a WAN retransmit, not a LAN event.
                    // Accumulate instead: the next read appends the rest and the line completes.
                    Err(ref e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(FlexCat {
            stream,
            rx,
            seq: 1,
            handle,
            pending: Mutex::new(VecDeque::new()),
        })
    }

    /// OUR client handle from the `H…` greeting, once the radio has sent it (`None` for the first
    /// few ms of a session). Compare it against a status object's `client_handle` before steering
    /// or removing that object — see the field docs on [`FlexCat::handle`].
    pub fn handle(&self) -> Option<u32> {
        *self.handle.lock().unwrap()
    }

    /// Park a frame [`command`](Self::command) pulled off the channel while waiting for its reply.
    ///
    /// ⚠️ THE ASYNC STATUS STREAM IS NOT COMMAND EXHAUST (Flex audit 2026-08-17, #1005/#1047).
    /// `command` drains the channel for its whole reply window, and everything that was not the
    /// matching reply used to be dropped — a window that opens microseconds after the `dax_rx`
    /// create in `flexdax`, and whose length scales with RTT. The dax_rx stream id has exactly one
    /// source (its create reply / the async `stream …` status), so the swallow could leave the
    /// audio path permanently filtered out with the feature looking enabled. Bounded FIFO, so
    /// order is preserved and nothing accumulates without limit.
    fn stash(&self, msg: FlexMsg) {
        let mut q = self.pending.lock().unwrap();
        if q.len() >= PENDING_CAP {
            q.pop_front();
        }
        q.push_back(msg);
    }

    /// Receive the next parsed message (blocks up to `timeout`), distinguishing an idle
    /// connection from a DEAD one — see [`FlexRecv`] for why that distinction is load-bearing.
    /// Frames a `command` parked while awaiting its reply come out here first, in arrival order.
    pub fn recv(&self, timeout: Duration) -> FlexRecv {
        if let Some(m) = self.pending.lock().unwrap().pop_front() {
            return FlexRecv::Msg(m);
        }
        match self.rx.recv_timeout(timeout) {
            Ok(m) => FlexRecv::Msg(m),
            Err(mpsc::RecvTimeoutError::Timeout) => FlexRecv::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => FlexRecv::Closed,
        }
    }

    /// Send a command and wait for its `R<seq>|…` reply (up to `timeout`). Returns the reply
    /// `(code, msg)` — `code == 0` is success. Intervening status/message frames are PARKED for
    /// the next [`recv`](Self::recv) rather than dropped (see `stash`), so a command may be issued
    /// on a session whose async status stream someone else is reading.
    pub fn command(&mut self, command: &str, timeout: Duration) -> std::io::Result<(u32, String)> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1).max(1);
        self.stream
            .write_all(encode_command(seq, command).as_bytes())?;
        let deadline = std::time::Instant::now() + timeout;
        // Anything already parked is older than this command's reply, so it can only be status —
        // leave it queued for `recv` and read the socket for the reply.
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            match self.rx.recv_timeout(remaining) {
                Ok(FlexMsg::Reply {
                    seq: rseq,
                    code,
                    msg,
                }) if rseq == seq => return Ok((code, msg)),
                Ok(other) => self.stash(other),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                // The reader thread is gone (see `FlexRecv::Closed`). Waiting out the window
                // would be a tight spin, since a disconnected channel returns instantly.
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "Flex API connection closed",
                    ))
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "no reply from Flex",
        ))
    }

    /// Fire-and-forget: write a command WITHOUT consuming its reply, returning the sequence
    /// number it was sent with. Use this when a single thread also needs [`recv`] to see the
    /// async status stream — `command` would swallow those `S…` frames while waiting for its
    /// reply, so the panadapter worker sends via `send` and reads replies + status through
    /// `recv` instead. Reply matching (by the returned seq) is the caller's job.
    pub fn send(&mut self, command: &str) -> std::io::Result<u32> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1).max(1);
        self.stream
            .write_all(encode_command(seq, command).as_bytes())?;
        Ok(seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_greeting_and_replies() {
        assert_eq!(
            parse_line("V1.4.0.0\n"),
            FlexMsg::Version("1.4.0.0".to_string())
        );
        assert_eq!(parse_line("H2ABC\n"), FlexMsg::Handle(0x2ABC));
        assert_eq!(
            parse_line("R3|0|0x40000000\n"),
            FlexMsg::Reply {
                seq: 3,
                code: 0,
                msg: "0x40000000".to_string()
            }
        );
        // A non-zero reply code (error) still parses.
        assert_eq!(
            parse_line("R7|50000015|bad\n"),
            FlexMsg::Reply {
                seq: 7,
                code: 0x5000_0015,
                msg: "bad".to_string()
            }
        );
    }

    #[test]
    fn parses_status_and_message_lines() {
        assert_eq!(
            parse_line("S2ABC|display pan 0x40000000 center=14.1\n"),
            FlexMsg::Status {
                handle: 0x2ABC,
                body: "display pan 0x40000000 center=14.1".to_string()
            }
        );
        assert!(matches!(
            parse_line("M10000000|hello\n"),
            FlexMsg::Message { .. }
        ));
        assert!(matches!(parse_line("garbage"), FlexMsg::Unknown(_)));
    }

    #[test]
    fn parses_a_pan_status_body() {
        let st = parse_pan_status(
            "display pan 0x40000000 wnb=0 center=14.100 bandwidth=0.200 x_pixels=1200 stream_id=0x42000000",
        )
        .unwrap();
        assert_eq!(st.pan_id, Some(0x4000_0000));
        assert_eq!(st.stream_id, Some(0x4200_0000));
        assert_eq!(st.center_mhz, Some(14.1));
        assert_eq!(st.bandwidth_mhz, Some(0.2));
        assert_eq!(st.x_pixels, Some(1200));
        // Not a pan line → None.
        assert!(parse_pan_status("slice 0 in_use=1").is_none());
    }

    #[test]
    fn encodes_a_command() {
        assert_eq!(
            encode_command(5, "display pan create x=1200 y=512"),
            "C5|display pan create x=1200 y=512\n"
        );
    }

    #[test]
    fn parses_a_dax_rx_stream_status() {
        let st = parse_dax_stream_status(
            "stream 0x04000000 type=dax_rx dax_channel=1 client_handle=0xABCD1234 in_use=1",
        )
        .unwrap();
        assert_eq!(st.stream_id, Some(0x0400_0000));
        assert_eq!(st.dax_channel, Some(1));
        assert_eq!(st.client_handle, Some(0xABCD_1234));
        // A non-dax_rx stream (e.g. dax_tx / a pan) → None.
        assert!(parse_dax_stream_status("stream 0x05000000 type=dax_tx dax_channel=1").is_none());
        assert!(parse_dax_stream_status("display pan 0x40000000 center=14.1").is_none());
    }

    #[test]
    fn parses_meter_definitions() {
        // The S-meter definition + a forward-power meter in one status line.
        let defs = parse_meter_defs(
            "meter 7.src=SLC#7.num=0#7.nam=LEVEL#7.unit=dBm#7.low=-150.0#7.hi=20.0#\
             12.src=TX#12.nam=FWDPWR#12.unit=dBm",
        );
        assert_eq!(defs.len(), 2);
        assert_eq!(
            defs[0],
            super::MeterDef {
                index: 7,
                source: "SLC".into(),
                name: "LEVEL".into(),
                unit: "dBm".into()
            }
        );
        assert_eq!(defs[1].index, 12);
        assert_eq!(defs[1].name, "FWDPWR");
        // Removal / non-meter lines yield nothing.
        assert!(parse_meter_defs("meter 7 removed").is_empty());
        assert!(parse_meter_defs("slice 0 in_use=1").is_empty());
    }

    #[test]
    fn parses_a_slice_status() {
        let st = parse_slice_status(
            "slice 2 in_use=1 RF_frequency=14.074 mode=DIGU active=1 tx=1 dax=1 client_handle=0xABCD0001",
        )
        .unwrap();
        assert_eq!(st.num, 2);
        assert_eq!(st.in_use, Some(true));
        assert_eq!(st.active, Some(true));
        assert_eq!(st.tx_slice, Some(true));
        assert_eq!(st.dax_channel, Some(1));
        assert_eq!(st.client_handle, Some(0xABCD_0001));
        // Removal + non-slice lines.
        assert_eq!(
            parse_slice_status("slice 0 in_use=0").unwrap().in_use,
            Some(false)
        );
        assert!(parse_slice_status("meter 7.src=SLC").is_none());
    }

    /// A dropped API connection must be REPORTABLE, not silently indistinguishable from a quiet
    /// one. Before this, `recv` returned `Option` and a disconnected channel came back as `None`
    /// INSTANTLY — so the panadapter/DAX worker loops, whose only pacing is this timeout, spun as
    /// fast as the CPU allowed and hammered the engine mutex (audit #1000).
    ///
    /// Both directions, on a real (localhost) socket: `Idle` while the peer holds it open, `Closed`
    /// once it goes away.
    #[test]
    fn a_dropped_connection_reports_closed_rather_than_idling() {
        use std::net::TcpListener;
        let srv = TcpListener::bind("127.0.0.1:0").expect("bind a local stand-in radio");
        let addr = srv.local_addr().expect("addr");
        let cat = FlexCat::connect_addr(addr, Duration::from_secs(2)).expect("connect");
        let (mut peer, _) = srv.accept().expect("accept");

        // Positive control that the reader thread is alive and this is a real connection: a line
        // written by the "radio" arrives parsed.
        peer.write_all(b"V1.4.0.0\n").expect("greet");
        let mut greeted = false;
        for _ in 0..50 {
            if let FlexRecv::Msg(FlexMsg::Version(v)) = cat.recv(Duration::from_millis(20)) {
                assert_eq!(v, "1.4.0.0");
                greeted = true;
                break;
            }
        }
        assert!(
            greeted,
            "the reader thread must deliver a line from the peer"
        );

        // Open but quiet → Idle (this is the state the 300 ms worker pacing depends on).
        assert_eq!(
            cat.recv(Duration::from_millis(50)),
            FlexRecv::Idle,
            "an open, quiet connection must idle out the timeout"
        );

        drop(peer);
        drop(srv);
        let mut closed = false;
        for _ in 0..100 {
            if cat.recv(Duration::from_millis(20)) == FlexRecv::Closed {
                closed = true;
                break;
            }
        }
        assert!(
            closed,
            "a peer close must surface as Closed — reported as Idle it is the busy-spin"
        );
        assert_eq!(
            cat.recv(Duration::from_millis(20)),
            FlexRecv::Closed,
            "Closed is terminal: every later recv says so too"
        );
    }

    /// A connected session against a local stand-in radio: the accepted peer socket (write to it
    /// as the radio) + the client. The listener is dropped — the accepted connection outlives it.
    fn local_session() -> (TcpStream, FlexCat) {
        use std::net::TcpListener;
        let srv = TcpListener::bind("127.0.0.1:0").expect("bind a local stand-in radio");
        let addr = srv.local_addr().expect("addr");
        let cat = FlexCat::connect_addr(addr, Duration::from_secs(2)).expect("connect");
        let (peer, _) = srv.accept().expect("accept");
        (peer, cat)
    }

    /// Poll `recv` for up to ~1 s and return the first message that matches `f`.
    fn wait_for<T>(cat: &FlexCat, mut f: impl FnMut(FlexMsg) -> Option<T>) -> Option<T> {
        for _ in 0..100 {
            if let FlexRecv::Msg(m) = cat.recv(Duration::from_millis(10)) {
                if let Some(v) = f(m) {
                    return Some(v);
                }
            }
        }
        None
    }

    /// A LINE SPLIT BY THE 500 ms READ TIMEOUT MUST SURVIVE IT (audit #1004).
    ///
    /// `read_line` commits the bytes it appended before the timeout, and the loop's old
    /// `line.clear()` threw them away — then parsed the TAIL as a whole line, which matches no tag
    /// and becomes `Unknown`. The frames it corrupts are the ones that carry the pan / DAX stream
    /// ids and the meter registry, so a WAN retransmit could silently cost the whole feature.
    #[test]
    fn a_line_split_by_the_read_timeout_is_reassembled_not_dropped() {
        let (mut peer, cat) = local_session();
        // First half of a status, no newline …
        peer.write_all(b"S2ABC|display pan 0x40000000 center=14.1")
            .expect("first half");
        // … then a stall LONGER than the socket's 500 ms read timeout, so the reader wakes with a
        // partial line in hand (the WAN-retransmit shape).
        std::thread::sleep(Duration::from_millis(900));
        peer.write_all(b"00 bandwidth=0.200000\n").expect("rest");

        let body = wait_for(&cat, |m| match m {
            FlexMsg::Status { body, .. } => Some(body),
            other => panic!("a split line must not surface as {other:?}"),
        });
        assert_eq!(
            body.as_deref(),
            Some("display pan 0x40000000 center=14.100 bandwidth=0.200000"),
            "the two halves must rejoin into the line the radio actually sent"
        );
    }

    /// THE ASYNC STATUS STREAM IS NOT COMMAND EXHAUST (audit #1005/#1047).
    ///
    /// `command` drains the channel for its whole reply window. Everything that was not the
    /// matching reply used to be dropped — including the `stream … type=dax_rx` status that is the
    /// only place the DAX RX stream id is learned, which `flexdax` creates microseconds before it
    /// calls `command`. Both halves asserted: the reply still arrives, and the status survives.
    #[test]
    fn a_command_parks_the_async_status_that_arrives_in_its_reply_window() {
        let (mut peer, mut cat) = local_session();
        let radio = std::thread::spawn(move || {
            let mut rd = BufReader::new(peer.try_clone().expect("clone"));
            let mut line = String::new();
            rd.read_line(&mut line).expect("the command line");
            assert_eq!(line, "C1|stream create type=dax_tx\n");
            // The radio answers the EARLIER create with an async status first, then this reply.
            peer.write_all(b"S2ABC|stream 0x04000000 type=dax_rx dax_channel=1 client_handle=0x2ABC\n")
                .expect("status");
            peer.write_all(b"R1|0|0x84000000\n").expect("reply");
        });

        let (code, msg) = cat
            .command("stream create type=dax_tx", Duration::from_secs(2))
            .expect("a reply inside the window");
        assert_eq!((code, msg.as_str()), (0, "0x84000000"));

        let st = wait_for(&cat, |m| match m {
            FlexMsg::Status { body, .. } => parse_dax_stream_status(&body),
            _ => None,
        })
        .expect("the status that arrived during the reply window must still be delivered");
        assert_eq!(st.stream_id, Some(0x0400_0000));
        radio.join().expect("stand-in radio");
    }

    /// OUR CLIENT HANDLE IS THE ONLY OWNERSHIP DISCRIMINATOR ON THIS WIRE (audit #1017).
    ///
    /// It was parsed into `FlexMsg::Handle` and dropped by both worker loops, so nothing could
    /// tell our pan / stream / slice from the SmartSDR GUI's. Captured on the reader thread, it
    /// survives even the case that used to lose it: a greeting consumed inside a `command` window.
    #[test]
    fn the_greeting_handle_is_captured_even_when_a_command_consumes_it() {
        let (mut peer, mut cat) = local_session();
        assert_eq!(cat.handle(), None, "nothing claimed before the radio greets");
        let radio = std::thread::spawn(move || {
            let mut rd = BufReader::new(peer.try_clone().expect("clone"));
            let mut line = String::new();
            rd.read_line(&mut line).expect("the command line");
            // Greeting INSIDE the reply window — the frame `command` would have eaten.
            peer.write_all(b"V1.4.0.0\nH2ABC\n").expect("greeting");
            peer.write_all(b"R1|0|0x84000000\n").expect("reply");
        });

        cat.command("ping", Duration::from_secs(2)).expect("reply");
        assert_eq!(
            cat.handle(),
            Some(0x2ABC),
            "the session must know which client it is, whoever consumed the H line"
        );
        radio.join().expect("stand-in radio");
    }

    #[test]
    fn parses_a_create_stream_reply() {
        assert_eq!(
            parse_create_stream_id("stream=0x84000000"),
            Some(0x8400_0000)
        );
        assert_eq!(parse_create_stream_id("id=0x84000001"), Some(0x8400_0001));
        assert_eq!(parse_create_stream_id("0x84000002"), Some(0x8400_0002));
        assert_eq!(parse_create_stream_id(""), None);
    }
}
