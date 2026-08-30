//! Nexus↔Nexus Field Day club sync — the wire layer: NDJSON over one
//! persistent TCP connection per position, plus a UDP discovery beacon.
//!
//! One club "position" (a Nexus instance) holds one TCP connection to the host
//! and streams `(position id, seq)`-identified QSO rows up; the host merges
//! them idempotently and pushes compact club state (dupe keys, sections,
//! score, band board) back down the same socket. The host's own contacts
//! enter through an identical loopback connection, so a host is just another
//! position. This module owns ONLY the wire: framing, the message vocabulary,
//! the accept loop and the position-side pump. Policy (the club log, the
//! journal, dupe semantics) lives behind the [`ClubBackend`] /
//! [`PositionSync`] traits in the caller — the aprsis split.
//!
//! ## SAFETY — the inbound surface is DATA-PLANE ONLY
//!
//! The host listener is the app's one deliberate non-loopback inbound socket
//! (bound only while the operator's "Host a club event" switch is on). Unlike
//! the WSJT-X inbound socket (whose Reply arms TX), **no fdsync message can
//! key TX, touch CAT, or change settings**: the entire inbound vocabulary is
//! "rows into the club log + position presence", and the [`ClubBackend`]
//! trait — the only thing the socket loop can reach — simply has no
//! capability beyond that. Unknown message types and unknown fields are
//! ignored (forward compatibility AND attack surface: a hostile LAN peer's
//! worst case is garbage rows in the club log, which the operator sees).
//! Pinned by `the_inbound_surface_is_data_plane_only` below.
//!
//! Wire format: one JSON object per `\n`-terminated line, tagged by `"t"`.
//! `v` (protocol version) travels only in `join`/`welcome`/`beacon`; the host
//! refuses a higher version with an `error` line the position shows verbatim.
//! Lines are capped at [`MAX_LINE_BYTES`]; `retract` is reserved (defined,
//! parsed, never acted on) so shipping edits later needs no version bump.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Protocol version, carried in `join`/`welcome`/`beacon`. A host refuses a
/// JOIN with a higher version (the joiner is newer — it knows things we
/// don't); same-or-lower joins are served, with unknown fields ignored.
pub const PROTO_VERSION: u32 = 1;
/// Default host TCP port. Arbitrary but conflict-checked against the ham
/// ecosystem's squatters: 2237 (WSJT-X UDP), 2242 (JS8Call), 1100 (N3FJP),
/// 12060 (N1MM) are all avoided. A setting, not a constant, at the caller.
pub const DEFAULT_TCP_PORT: u16 = 42073;
/// UDP port the host's once-a-second discovery beacon broadcasts on.
pub const BEACON_PORT: u16 = 42074;
/// One NDJSON line's byte cap — bounds a hostile peer's memory cost. The
/// biggest legit line (a `snap` with thousands of dupe keys) is chunked by
/// the sender instead ([`SNAP_DUPES_PER_LINE`]).
pub const MAX_LINE_BYTES: usize = 8 * 1024;
/// Dupe keys per `snap`/`club` line — keeps every line under
/// [`MAX_LINE_BYTES`] (a key is ≤ ~40 bytes on the wire; 100 ≈ 4 KB worst
/// case). The mirror unions chunks, so chunking is invisible to state.
pub const SNAP_DUPES_PER_LINE: usize = 100;
/// Host connection cap — bounds a SYN-happy peer. A real club runs ~25
/// positions; 64 leaves room for reconnect races.
pub const MAX_CONNECTIONS: usize = 64;
/// Ping cadence (either side), and the heartbeat cadence for board refresh.
pub const PING_SECS: u64 = 5;
/// Silence on the socket after which either side treats the link as down.
pub const DEAD_SECS: u64 = 15;
/// Consecutive undecodable lines before a connection is dropped (a peer that
/// is not speaking this protocol at all).
const MAX_GARBAGE_LINES: u32 = 32;

/// One Field Day QSO on the wire — the `(pos, seq)` pair is its identity;
/// everything else is the row the club log stores.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WireQso {
    /// Position id (8-hex, per machine).
    pub pos: String,
    /// Per-position monotonic sequence (`LoggedQso::seq`). Never 0.
    pub seq: u64,
    pub call: String,
    pub class: String,
    pub sect: String,
    pub band: String,
    /// Scoring mode class: "DIG" | "CW" | "PH".
    pub mode: String,
    /// Actual on-air mode behind a "DIG" class ("FT8", "RTTY", …); "" = n/a.
    #[serde(default)]
    pub sub: String,
    /// Unix seconds the contact was logged (the logging position's clock).
    pub when: u64,
    /// Operator at the key when logged ("" = unrecorded).
    #[serde(default)]
    pub op: String,
}

/// One band-board row: where a position is and how it is doing. Host-computed;
/// `age` is seconds since the host last heard from the position.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WireBoardRow {
    pub pos: String,
    pub name: String,
    pub band: String,
    pub mode: String,
    pub op: String,
    /// Raw merged rows from this position.
    pub qsos: u64,
    /// Rows that score (cross-position dupes merge but score 0).
    #[serde(default)]
    pub uniq: u64,
    /// Merged rows in the trailing 60 min (the contest rate meter).
    pub rate: u64,
    pub age: u64,
}

/// Club state pushed host→position in `snap` (full, on join) and `club`
/// (delta) lines. `dupes`/`sections` are APPEND-ONLY at the host (the club
/// log has no retraction), so a delta is simply "everything past what this
/// connection already sent" and the mirror unions them.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ClubState {
    /// True on the FIRST chunk of a join `snap`: the mirror clears its lists
    /// before applying, so a rejoin after a host restart (possibly a NEW
    /// event) cannot union a dead event's keys. Deltas never set it.
    #[serde(default)]
    pub reset: bool,
    /// Club dupe keys `(call, band, mode class)` — the position's while-typing
    /// dupe verdict unions these with its own log.
    #[serde(default)]
    pub dupes: Vec<(String, String, String)>,
    /// ARRL/RAC sections newly worked club-wide.
    #[serde(default)]
    pub sections: Vec<String>,
    /// Claimed club total (host's power multiplier + bonuses).
    #[serde(default)]
    pub score: u32,
    /// Merged club rows (raw, dupes included — honesty over flattery).
    #[serde(default)]
    pub qsos: u64,
    #[serde(default)]
    pub board: Vec<WireBoardRow>,
}

/// The whole wire vocabulary. Internally tagged by `t`; unknown tags land on
/// [`Msg::Unknown`] (ignored — forward compatibility), unknown fields inside
/// a known tag are ignored by serde's default.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Msg {
    /// pos→host, first line on a connection.
    Join {
        v: u32,
        pos: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        call: String,
        /// The position's own high-water seq (its journal's max).
        #[serde(default)]
        max_seq: u64,
    },
    /// host→pos, the join's answer.
    Welcome {
        v: u32,
        #[serde(default)]
        event: String,
        #[serde(default)]
        host_call: String,
        /// Host's high-water ack for this position — the position streams
        /// every own row with `seq > acked` (the outbox definition).
        #[serde(default)]
        acked: u64,
        /// Host wall clock, for the >30 s skew warning (never adjusted).
        #[serde(default)]
        now_unix: u64,
    },
    /// pos→host, one QSO row.
    Qso(WireQso),
    /// host→pos: rows up to and including `seq` are merged (idempotent —
    /// re-pushing an acked row is a no-op, so re-sync needs no bookkeeping).
    Ack {
        seq: u64,
    },
    /// pos→host presence: current band/mode/operator/dial (the band board).
    Pos {
        #[serde(default)]
        band: String,
        #[serde(default)]
        mode: String,
        #[serde(default)]
        op: String,
        #[serde(default)]
        freq: u64,
        /// The position's friendly name, re-sent on EVERY report so a rename
        /// in Settings reaches the board without rebuilding the connection —
        /// the `join` line's `name` is a one-shot, and renaming used to leave
        /// the board showing the name (or the raw position id) the connection
        /// was born with. Adding it needs no version bump in either
        /// direction: every field here is `#[serde(default)]` and unknown
        /// fields inside a known tag are ignored, so a v1 host reading a
        /// newer position simply drops it and a newer host reading a v1
        /// position sees `""`. That is why `""` MUST mean "no news" at the
        /// host and never "clear the label".
        #[serde(default)]
        name: String,
    },
    /// host→pos on join: full club state (chunked by [`SNAP_DUPES_PER_LINE`]).
    Snap(ClubState),
    /// host→pos after: club-state delta + fresh board.
    Club(ClubState),
    Ping,
    Pong,
    /// RESERVED (defined so a future ship needs no version bump; ignored on
    /// receive today — `FieldDayLog` is append-only with no edit UI).
    Retract {
        pos: String,
        seq: u64,
    },
    /// host→pos refusal (version mismatch etc.) — shown to the operator
    /// verbatim.
    Error {
        msg: String,
    },
    /// Any `t` this build does not know. Ignored.
    #[serde(other)]
    Unknown,
}

/// Encode one message as its NDJSON line (trailing `\n` included).
pub fn encode_line(msg: &Msg) -> String {
    let mut s = serde_json::to_string(msg).unwrap_or_else(|_| "{}".into());
    s.push('\n');
    s
}

/// Decode one line. `None` = not JSON / not an object with a known shape —
/// the caller counts garbage; `Some(Msg::Unknown)` = well-formed but a type
/// this build doesn't know — silently ignored.
pub fn decode_line(line: &str) -> Option<Msg> {
    serde_json::from_str::<Msg>(line.trim()).ok()
}

/// Read one `\n`-terminated line, capped at [`MAX_LINE_BYTES`].
/// `Ok(Some(line))` = a line; `Ok(None)` = clean EOF; `Err` = socket error,
/// timeout (`WouldBlock`/`TimedOut` — the caller's duty tick), or an
/// over-long line (`InvalidData` — protocol violation, drop the connection).
pub fn read_capped_line(r: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(256);
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > MAX_LINE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "fdsync line exceeds the 8 KB cap",
        ));
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

// ---------------------------------------------------------------------------
// Host side
// ---------------------------------------------------------------------------

/// What the host answers a version-accepted JOIN with (the welcome's fields
/// minus what the wire layer owns — `v` and `now_unix`).
#[derive(Clone, Debug, Default)]
pub struct JoinAccept {
    pub event: String,
    pub host_call: String,
    /// Host's high-water ack for the joining position.
    pub acked: u64,
}

/// Everything the socket loop can do to the application — and, deliberately,
/// everything it CANNOT: there is no method here that keys TX, touches CAT,
/// or writes a setting, so no inbound byte can reach any of those (the
/// data-plane-only property in the module header).
pub trait ClubBackend: Send + Sync {
    /// A version-accepted JOIN. `Err(msg)` refuses it (sent verbatim, then
    /// the connection closes).
    fn join(&self, pos: &str, name: &str, call: &str, max_seq: u64) -> Result<JoinAccept, String>;
    /// Merge one row into the club log (idempotent on `(pos, seq)`); returns
    /// the new high-water ack for `row.pos`.
    fn merge(&self, row: &WireQso) -> u64;
    /// A position's presence report (band board fodder). `report.name` is the
    /// position's current friendly name — EMPTY MEANS "no news" (an older
    /// peer sends none), never "clear the label".
    fn position_status(&self, pos: &str, report: &PosReport);
    /// Cheap change detector: (dupe keys total, sections total). Both are
    /// append-only, so "count grew" == "there is a delta to send".
    fn counts(&self) -> (usize, usize);
    /// Club state past the given cursors (`0, 0` = the full join snapshot),
    /// with the current board. `mark_seen` names the asking position so the
    /// host can stamp its last-seen (stale board rows are marked, not hidden).
    fn club_state(&self, dupes_from: usize, sections_from: usize, mark_seen: &str) -> ClubState;
    /// The position's connection dropped.
    fn disconnect(&self, pos: &str);
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Send a `ClubState` as one or more lines, chunking the dupe list so every
/// line stays under the cap. Only the FIRST chunk carries sections/score/
/// board (the mirror overwrites scalars, unions lists).
fn write_club_state(w: &mut impl Write, st: ClubState, snap: bool) -> std::io::Result<()> {
    let ClubState {
        reset: _,
        dupes,
        sections,
        score,
        qsos,
        board,
    } = st;
    let mut first = true;
    let mut chunks = dupes.chunks(SNAP_DUPES_PER_LINE);
    loop {
        let chunk: Vec<_> = chunks.next().unwrap_or(&[]).to_vec();
        let part = ClubState {
            reset: snap && first,
            dupes: chunk,
            sections: if first { sections.clone() } else { Vec::new() },
            score,
            qsos,
            board: if first { board.clone() } else { Vec::new() },
        };
        let msg = if snap {
            Msg::Snap(part)
        } else {
            Msg::Club(part)
        };
        w.write_all(encode_line(&msg).as_bytes())?;
        first = false;
        if chunks.len() == 0 {
            return Ok(());
        }
    }
}

/// One host-side connection: JOIN handshake, then the duplex pump — rows and
/// presence up, acks and club state down, pings both ways. Read timeout
/// doubles as the duty tick (club deltas, the 5 s heartbeat, the dead-man).
fn serve_club_connection(
    stream: TcpStream,
    backend: Arc<dyn ClubBackend>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = stream.set_nodelay(true);
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let mut joined: Option<String> = None;
    let mut sent_dupes = 0usize;
    let mut sent_sections = 0usize;
    let mut garbage_run = 0u32;
    let mut last_rx = Instant::now();
    let mut last_beat = Instant::now();
    // The host's shutdown flag closes LIVE connections too, not just the
    // accept loop — turning hosting off (or re-porting) must actually end
    // the sessions, or a disabled host would keep serving stale state.
    while !shutdown.load(Ordering::Relaxed) {
        match read_capped_line(&mut reader) {
            Ok(None) => break, // clean EOF
            Ok(Some(line)) => {
                last_rx = Instant::now();
                let msg = match decode_line(&line) {
                    Some(m) => {
                        garbage_run = 0;
                        m
                    }
                    None => {
                        garbage_run += 1;
                        if garbage_run >= MAX_GARBAGE_LINES {
                            break; // not speaking this protocol at all
                        }
                        continue;
                    }
                };
                match msg {
                    Msg::Join {
                        v,
                        pos,
                        name,
                        call,
                        max_seq,
                    } => {
                        if v > PROTO_VERSION {
                            let _ = writer.write_all(
                                encode_line(&Msg::Error {
                                    msg: format!(
                                        "this host speaks Field Day sync v{PROTO_VERSION}, \
                                         you sent v{v} — update the host's Nexus"
                                    ),
                                })
                                .as_bytes(),
                            );
                            break;
                        }
                        let accept = match backend.join(&pos, &name, &call, max_seq) {
                            Ok(a) => a,
                            Err(msg) => {
                                let _ =
                                    writer.write_all(encode_line(&Msg::Error { msg }).as_bytes());
                                break;
                            }
                        };
                        let welcome = Msg::Welcome {
                            v: PROTO_VERSION,
                            event: accept.event,
                            host_call: accept.host_call,
                            acked: accept.acked,
                            now_unix: now_unix(),
                        };
                        if writer.write_all(encode_line(&welcome).as_bytes()).is_err() {
                            break;
                        }
                        // Full snapshot: read the cursors BEFORE the state so a
                        // merge racing in between is re-sent, never skipped
                        // (the mirror dedups; a skipped key would stay lost).
                        let (d, s) = backend.counts();
                        let st = backend.club_state(0, 0, &pos);
                        if write_club_state(&mut writer, st, true).is_err() {
                            break;
                        }
                        sent_dupes = d;
                        sent_sections = s;
                        joined = Some(pos);
                    }
                    Msg::Qso(row) => {
                        if joined.is_some() {
                            let acked = backend.merge(&row);
                            if writer
                                .write_all(encode_line(&Msg::Ack { seq: acked }).as_bytes())
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Msg::Pos {
                        band,
                        mode,
                        op,
                        freq,
                        name,
                    } => {
                        if let Some(pos) = &joined {
                            backend.position_status(
                                pos,
                                &PosReport {
                                    band,
                                    mode,
                                    op,
                                    freq,
                                    name,
                                },
                            );
                        }
                    }
                    Msg::Ping => {
                        if writer
                            .write_all(encode_line(&Msg::Pong).as_bytes())
                            .is_err()
                        {
                            break;
                        }
                    }
                    // Pong: freshness already noted via last_rx. Retract is
                    // reserved; Unknown is a newer peer's message; the rest
                    // are host→pos vocabulary a position should never send.
                    // All ignored — nothing here may reach beyond ClubBackend.
                    _ => {}
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break, // socket error or the 8 KB line-cap violation
        }
        if last_rx.elapsed().as_secs() >= DEAD_SECS {
            break;
        }
        // Duty tick: club delta the moment there is one; ping + board-bearing
        // heartbeat every PING_SECS.
        if let Some(pos) = &joined {
            let (d, s) = backend.counts();
            let beat = last_beat.elapsed().as_secs() >= PING_SECS;
            if d > sent_dupes || s > sent_sections || beat {
                let st = backend.club_state(sent_dupes, sent_sections, pos);
                if write_club_state(&mut writer, st, false).is_err() {
                    break;
                }
                sent_dupes = d;
                sent_sections = s;
            }
            if beat {
                if writer
                    .write_all(encode_line(&Msg::Ping).as_bytes())
                    .is_err()
                {
                    break;
                }
                last_beat = Instant::now();
            }
        }
    }
    if let Some(pos) = joined {
        backend.disconnect(&pos);
    }
}

/// Run the host accept loop, a thread per position, until `shutdown` is set —
/// the `rigctld_server::serve_until` shape, so the src-tauri manager can turn
/// hosting on/off (or re-port it) without a restart. Accept is polled
/// non-blocking ~5×/s; the listener drops (releasing the port) on return.
pub fn serve_until(
    listener: TcpListener,
    backend: Arc<dyn ClubBackend>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = listener.set_nonblocking(true);
    let live = Arc::new(AtomicUsize::new(0));
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if live.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                    drop(stream); // flood cap — refuse quietly
                    continue;
                }
                let _ = stream.set_nonblocking(false); // per-client blocking reads
                let b = Arc::clone(&backend);
                let live2 = Arc::clone(&live);
                let sd = Arc::clone(&shutdown);
                live.fetch_add(1, Ordering::Relaxed);
                std::thread::spawn(move || {
                    serve_club_connection(stream, b, sd);
                    live2.fetch_sub(1, Ordering::Relaxed);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(_) => break,
        }
    }
    // `listener` drops here → the port is released for a rebind.
}

// ---------------------------------------------------------------------------
// Position side
// ---------------------------------------------------------------------------

/// One presence report: what a position is doing right now, plus the name it
/// wants on the board. Built by the position for [`Msg::Pos`] and handed
/// STRAIGHT ON to the host's [`ClubBackend`] — one shape, so the two ends
/// cannot drift. A struct rather than the tuple the position half used to
/// pass: four of its five fields are strings, so the compiler is the only
/// thing that can stop `name` and `op` swapping places.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PosReport {
    pub band: String,
    /// Scoring mode class: "DIG" | "CW" | "PH".
    pub mode: String,
    pub op: String,
    /// Dial, Hz.
    pub freq: u64,
    /// The position's friendly name ("CW tent"). Empty = "no news" to the
    /// host, which keeps whatever label it already knows.
    pub name: String,
}

/// The position pump's view of the application. Mirrors [`ClubBackend`]'s
/// discipline: the pump can read the outbox and deliver club state, nothing
/// more.
pub trait PositionSync: Send + Sync {
    /// This position's identity for the JOIN line:
    /// `(pos id, friendly name, station call, own max seq)`.
    fn identity(&self) -> (String, String, String, u64);
    /// Own rows with `seq > after` (ascending) — THE outbox definition; no
    /// separate queue exists to corrupt.
    fn outbox_after(&self, after: u64) -> Vec<WireQso>;
    /// The accepted welcome (host's ack high-water, event, host call, host
    /// clock — the caller warns on >30 s skew; nothing is ever adjusted).
    fn on_welcome(&self, acked: u64, event: &str, host_call: &str, now_unix: u64);
    fn on_ack(&self, seq: u64);
    /// A `snap` (full, `snap=true`) or `club` (delta) line: union the lists,
    /// overwrite the scalars.
    fn on_club(&self, snap: bool, st: &ClubState);
    /// A host `error` line — shown to the operator verbatim.
    fn on_error(&self, msg: &str);
    /// Current presence for the band board — `(band, mode class, operator,
    /// dial Hz, friendly name)`; `None` = don't report this tick. The name
    /// rides along on every report so a rename propagates live.
    fn position_report(&self) -> Option<PosReport>;
    /// Link up/down transitions (drives the Offline/Behind/Synced chip).
    fn on_link(&self, connected: bool);
}

/// One connected session: join → welcome → stream the gap → duplex pump.
/// Returns when the link dies or `shutdown` is set. `Ok(true)` = a welcome
/// was received (reset the reconnect backoff).
fn run_position_session(
    addr: &str,
    backend: &Arc<dyn PositionSync>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<bool> {
    let sock_addr = addr
        .parse::<std::net::SocketAddr>()
        .or_else(|_| {
            use std::net::ToSocketAddrs;
            addr.to_socket_addrs()?
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(3))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    let _ = stream.set_nodelay(true);
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let (pos, name, call, max_seq) = backend.identity();
    writer.write_all(
        encode_line(&Msg::Join {
            v: PROTO_VERSION,
            pos,
            name,
            call,
            max_seq,
        })
        .as_bytes(),
    )?;
    backend.on_link(true);
    let mut welcomed = false;
    let mut acked = 0u64;
    let mut sent_to = 0u64; // rows in flight (seq high-water we already wrote)
    let mut last_rx = Instant::now();
    let mut last_beat = Instant::now();
    let mut last_report: Option<PosReport> = None;
    let result = loop {
        if shutdown.load(Ordering::Relaxed) {
            break Ok(welcomed);
        }
        match read_capped_line(&mut reader) {
            Ok(None) => break Ok(welcomed),
            Ok(Some(line)) => {
                last_rx = Instant::now();
                match decode_line(&line) {
                    Some(Msg::Welcome {
                        event,
                        host_call,
                        acked: a,
                        now_unix,
                        ..
                    }) => {
                        welcomed = true;
                        acked = a;
                        sent_to = a; // everything past the ack re-streams below
                        backend.on_welcome(a, &event, &host_call, now_unix);
                    }
                    Some(Msg::Ack { seq }) => {
                        acked = acked.max(seq);
                        backend.on_ack(seq);
                    }
                    Some(Msg::Snap(st)) => backend.on_club(true, &st),
                    Some(Msg::Club(st)) => backend.on_club(false, &st),
                    Some(Msg::Ping) => {
                        writer.write_all(encode_line(&Msg::Pong).as_bytes())?;
                    }
                    Some(Msg::Error { msg }) => {
                        backend.on_error(&msg);
                        break Ok(welcomed);
                    }
                    _ => {} // Pong / Unknown / host-bound vocabulary — ignore
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => break Err(e),
        }
        if last_rx.elapsed().as_secs() >= DEAD_SECS {
            break Ok(welcomed); // host silent → reconnect
        }
        if welcomed {
            // Stream the outbox: everything past what is both acked and
            // already written this session (a fresh QSO extends it live).
            let from = sent_to.max(acked);
            for row in backend.outbox_after(from) {
                sent_to = sent_to.max(row.seq);
                writer.write_all(encode_line(&Msg::Qso(row)).as_bytes())?;
            }
            let beat = last_beat.elapsed().as_secs() >= PING_SECS;
            let report = backend.position_report();
            if report.is_some() && (beat || report != last_report) {
                if let Some(PosReport {
                    band,
                    mode,
                    op,
                    freq,
                    name,
                }) = report.clone()
                {
                    writer.write_all(
                        encode_line(&Msg::Pos {
                            band,
                            mode,
                            op,
                            freq,
                            name,
                        })
                        .as_bytes(),
                    )?;
                }
                last_report = report;
            }
            if beat {
                writer.write_all(encode_line(&Msg::Ping).as_bytes())?;
                last_beat = Instant::now();
            }
        }
    };
    backend.on_link(false);
    result
}

/// The position-side client pump with reconnect backoff (the cluster-feed
/// shape): connect → join → stream the gap → pump, forever, until `shutdown`.
/// Backoff 1 s → 2 s → 4 s … capped at 15 s; a session that got a welcome
/// resets it. Idempotent merge makes every reconnect's re-push free.
pub fn run_position_until(addr: &str, backend: Arc<dyn PositionSync>, shutdown: Arc<AtomicBool>) {
    let mut backoff = 1u64;
    while !shutdown.load(Ordering::Relaxed) {
        match run_position_session(addr, &backend, &shutdown) {
            Ok(true) => backoff = 1,
            _ => backoff = (backoff * 2).min(15),
        }
        // Sleep in small steps so shutdown is honored promptly.
        let until = Instant::now() + Duration::from_secs(backoff);
        while Instant::now() < until && !shutdown.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery beacon
// ---------------------------------------------------------------------------

/// One club event heard on the LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeaconInfo {
    pub event: String,
    pub call: String,
    /// `ip:port` ready to join (ip from the datagram's source address).
    pub host: String,
}

/// The host's once-a-second broadcast line.
pub fn beacon_line(event: &str, call: &str, port: u16) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "t": "beacon",
            "v": PROTO_VERSION,
            "event": event,
            "call": call,
            "port": port,
        })
    )
}

/// Parse a beacon datagram → `(event, call, port)`. Garbage-tolerant like
/// `flexdisc::parse_discovery`: `None` for anything that isn't ours.
pub fn parse_beacon(datagram: &[u8]) -> Option<(String, String, u16)> {
    let v: serde_json::Value = serde_json::from_slice(datagram).ok()?;
    if v.get("t")?.as_str()? != "beacon" {
        return None;
    }
    let port = u16::try_from(v.get("port")?.as_u64()?).ok()?;
    Some((
        v.get("event")
            .and_then(|e| e.as_str())
            .unwrap_or("")
            .to_string(),
        v.get("call")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        port,
    ))
}

/// A broadcast-enabled UDP socket for the host's beacon sender.
pub fn beacon_socket() -> std::io::Result<UdpSocket> {
    let sock = UdpSocket::bind(("0.0.0.0", 0))?;
    sock.set_broadcast(true)?;
    Ok(sock)
}

/// Send one beacon (best-effort — AP-isolated Wi-Fi eats broadcast, which is
/// why manual `host:port` entry always remains).
pub fn send_beacon(sock: &UdpSocket, event: &str, call: &str, port: u16) {
    let _ = sock.send_to(
        beacon_line(event, call, port).as_bytes(),
        ("255.255.255.255", BEACON_PORT),
    );
}

/// Listen for host beacons for up to `secs` and return every distinct event
/// heard — the "Find club events" button. `SO_REUSEADDR` so two Nexus
/// instances on one box can both listen (the flexdisc socket shape).
pub fn discover(secs: u64) -> std::io::Result<Vec<BeaconInfo>> {
    let raw = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    raw.set_reuse_address(true)?;
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], BEACON_PORT).into();
    raw.bind(&addr.into())?;
    let sock: UdpSocket = raw.into();
    sock.set_read_timeout(Some(Duration::from_millis(400)))?;
    let deadline = Instant::now() + Duration::from_secs(secs.clamp(1, 10));
    let mut found: Vec<BeaconInfo> = Vec::new();
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        // Err = the 400 ms read timeout ticking — keep listening.
        if let Ok((n, from)) = sock.recv_from(&mut buf) {
            if let Some((event, call, port)) = parse_beacon(&buf[..n]) {
                let host = format!("{}:{port}", from.ip());
                if !found.iter().any(|b| b.host == host) {
                    found.push(BeaconInfo { event, call, host });
                }
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ---- codec -----------------------------------------------------------

    #[test]
    fn every_message_round_trips_through_its_ndjson_line() {
        let msgs = vec![
            Msg::Join {
                v: 1,
                pos: "a1b2c3d4".into(),
                name: "CW tent".into(),
                call: "KD9TAW".into(),
                max_seq: 42,
            },
            Msg::Welcome {
                v: 1,
                event: "W9ABC FD".into(),
                host_call: "W9ABC".into(),
                acked: 37,
                now_unix: 1_782_583_500,
            },
            Msg::Qso(WireQso {
                pos: "a1b2c3d4".into(),
                seq: 38,
                call: "W1AW".into(),
                class: "2A".into(),
                sect: "CT".into(),
                band: "20m".into(),
                mode: "DIG".into(),
                sub: "FT8".into(),
                when: 1_782_583_500,
                op: "KD9TAW".into(),
            }),
            Msg::Ack { seq: 38 },
            Msg::Pos {
                band: "20m".into(),
                mode: "CW".into(),
                op: "KD9TAW".into(),
                freq: 14_032_100,
                name: "CW tent".into(),
            },
            Msg::Snap(ClubState {
                reset: true,
                dupes: vec![("W1AW".into(), "20m".into(), "DIG".into())],
                sections: vec!["CT".into()],
                score: 1234,
                qsos: 312,
                board: vec![WireBoardRow {
                    pos: "a1b2c3d4".into(),
                    name: "CW tent".into(),
                    band: "20m".into(),
                    mode: "CW".into(),
                    op: "KD9TAW".into(),
                    qsos: 57,
                    uniq: 55,
                    rate: 23,
                    age: 2,
                }],
            }),
            Msg::Club(ClubState::default()),
            Msg::Ping,
            Msg::Pong,
            Msg::Retract {
                pos: "a1b2c3d4".into(),
                seq: 7,
            },
            Msg::Error { msg: "nope".into() },
        ];
        for m in msgs {
            let line = encode_line(&m);
            assert!(line.ends_with('\n') && !line[..line.len() - 1].contains('\n'));
            let back = decode_line(&line).expect("decodes");
            // Compare via re-encoding (Msg is not PartialEq — ClubState is).
            assert_eq!(encode_line(&back), line, "round-trip of {line}");
        }
    }

    #[test]
    fn wire_tags_match_the_documented_sketch() {
        // The `t` values are the protocol — pin them so a rename can't ship
        // silently.
        assert!(encode_line(&Msg::Ping).contains("\"t\":\"ping\""));
        assert!(encode_line(&Msg::Ack { seq: 1 }).contains("\"t\":\"ack\""));
        let j = encode_line(&Msg::Join {
            v: 1,
            pos: "p".into(),
            name: String::new(),
            call: String::new(),
            max_seq: 0,
        });
        assert!(j.contains("\"t\":\"join\"") && j.contains("\"max_seq\""));
        let w = encode_line(&Msg::Welcome {
            v: 1,
            event: String::new(),
            host_call: "X".into(),
            acked: 0,
            now_unix: 0,
        });
        assert!(w.contains("\"t\":\"welcome\"") && w.contains("\"host_call\""));
    }

    #[test]
    fn unknown_types_and_fields_are_ignored_not_errors() {
        // A NEWER peer's message type parses as Unknown (ignored)…
        assert!(matches!(
            decode_line(r#"{"t":"hologram","x":1}"#),
            Some(Msg::Unknown)
        ));
        // …reserved retract parses (and is then ignored by both loops)…
        assert!(matches!(
            decode_line(r#"{"t":"retract","pos":"a","seq":3}"#),
            Some(Msg::Retract { .. })
        ));
        // …unknown fields inside a known type are dropped…
        assert!(matches!(
            decode_line(r#"{"t":"ack","seq":9,"flavor":"grape"}"#),
            Some(Msg::Ack { seq: 9 })
        ));
        // …and non-JSON is None (garbage-counted by the loops).
        assert!(decode_line("MAIL FROM:<spam>").is_none());
        assert!(decode_line("").is_none());
    }

    #[test]
    fn read_capped_line_enforces_the_8kb_cap() {
        let long = format!("{}\n", "x".repeat(MAX_LINE_BYTES + 10));
        let mut r = std::io::Cursor::new(long.into_bytes());
        let err = read_capped_line(&mut r).expect_err("over-cap line refused");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        // Control: a line AT a sane size passes through intact.
        let ok = format!("{}\n", "y".repeat(1000));
        let mut r = std::io::Cursor::new(ok.clone().into_bytes());
        assert_eq!(
            read_capped_line(&mut r).unwrap().as_deref(),
            Some(ok.as_str())
        );
        // EOF is None, not an error.
        assert_eq!(read_capped_line(&mut r).unwrap(), None);
    }

    // ---- beacon ----------------------------------------------------------

    #[test]
    fn beacon_parses_and_rejects_garbage() {
        let line = beacon_line("W9ABC Field Day", "W9ABC", 42073);
        assert_eq!(
            parse_beacon(line.as_bytes()),
            Some(("W9ABC Field Day".into(), "W9ABC".into(), 42073))
        );
        // The flexdisc positive/negative pair: garbage yields None, never a
        // panic — and a near-miss (right shape, wrong tag) is rejected too.
        assert_eq!(parse_beacon(b"GET / HTTP/1.1\r\n"), None);
        assert_eq!(parse_beacon(&[0u8; 64]), None);
        assert_eq!(
            parse_beacon(br#"{"t":"discovery","port":4992}"#),
            None,
            "a non-beacon JSON datagram is not ours"
        );
        assert_eq!(
            parse_beacon(br#"{"t":"beacon","port":"junk"}"#),
            None,
            "a beacon with an unusable port is dropped"
        );
    }

    // ---- host loop over a real socket ------------------------------------

    /// Instrumented fake club: records every trait call, merges idempotently.
    #[derive(Default)]
    struct FakeClub {
        calls: Mutex<Vec<String>>,
        merged: Mutex<std::collections::HashMap<(String, u64), WireQso>>,
        acked: Mutex<std::collections::HashMap<String, u64>>,
    }
    impl FakeClub {
        fn log(&self, s: impl Into<String>) {
            self.calls.lock().unwrap().push(s.into());
        }
    }
    impl ClubBackend for FakeClub {
        fn join(
            &self,
            pos: &str,
            _name: &str,
            _call: &str,
            _max_seq: u64,
        ) -> Result<JoinAccept, String> {
            self.log(format!("join {pos}"));
            Ok(JoinAccept {
                event: "TEST FD".into(),
                host_call: "W9ABC".into(),
                acked: *self.acked.lock().unwrap().get(pos).unwrap_or(&0),
            })
        }
        fn merge(&self, row: &WireQso) -> u64 {
            self.log(format!("merge {} {}", row.pos, row.seq));
            self.merged
                .lock()
                .unwrap()
                .entry((row.pos.clone(), row.seq))
                .or_insert_with(|| row.clone());
            let mut acked = self.acked.lock().unwrap();
            let e = acked.entry(row.pos.clone()).or_insert(0);
            *e = (*e).max(row.seq);
            *e
        }
        fn position_status(&self, pos: &str, r: &PosReport) {
            self.log(format!("pos {pos} {} name={}", r.band, r.name));
        }
        fn counts(&self) -> (usize, usize) {
            (self.merged.lock().unwrap().len(), 0)
        }
        fn club_state(&self, dupes_from: usize, _sections_from: usize, _seen: &str) -> ClubState {
            let m = self.merged.lock().unwrap();
            let mut dupes: Vec<_> = m
                .values()
                .map(|r| (r.call.clone(), r.band.clone(), r.mode.clone()))
                .collect();
            dupes.sort();
            ClubState {
                reset: false,
                dupes: dupes.into_iter().skip(dupes_from).collect(),
                sections: Vec::new(),
                score: 0,
                qsos: m.len() as u64,
                board: Vec::new(),
            }
        }
        fn disconnect(&self, pos: &str) {
            self.log(format!("disconnect {pos}"));
        }
    }

    fn start_host(backend: Arc<FakeClub>) -> (std::net::SocketAddr, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
        let addr = listener.local_addr().unwrap();
        let sd = Arc::new(AtomicBool::new(false));
        let sd2 = sd.clone();
        let b: Arc<dyn ClubBackend> = backend;
        std::thread::spawn(move || serve_until(listener, b, sd2));
        (addr, sd)
    }

    /// A blocking client helper: send lines, read replies until `want` of a
    /// given tag arrived or 5 s passed.
    fn talk(addr: std::net::SocketAddr, lines: &[Msg], read_for_ms: u64) -> Vec<Msg> {
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        for m in lines {
            w.write_all(encode_line(m).as_bytes()).unwrap();
        }
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(read_for_ms);
        while Instant::now() < deadline {
            match read_capped_line(&mut r) {
                Ok(Some(line)) => {
                    if let Some(m) = decode_line(&line) {
                        got.push(m);
                    }
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        got
    }

    fn join_msg(pos: &str, max_seq: u64) -> Msg {
        Msg::Join {
            v: PROTO_VERSION,
            pos: pos.into(),
            name: "tent".into(),
            call: "KD9TAW".into(),
            max_seq,
        }
    }

    fn qso(pos: &str, seq: u64, call: &str) -> Msg {
        Msg::Qso(WireQso {
            pos: pos.into(),
            seq,
            call: call.into(),
            class: "2A".into(),
            sect: "CT".into(),
            band: "20m".into(),
            mode: "DIG".into(),
            sub: "FT8".into(),
            when: 1_782_583_500,
            op: "OP".into(),
        })
    }

    #[test]
    fn host_serves_join_ack_and_idempotent_repush() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());

        // join → welcome + snap; two rows; then RE-PUSH row 1 (the outage
        // re-sync) — it must ack but merge nothing new.
        let got = talk(
            addr,
            &[
                join_msg("aaaa0001", 0),
                qso("aaaa0001", 1, "W1AW"),
                qso("aaaa0001", 2, "K1ABC"),
                qso("aaaa0001", 1, "W1AW"), // the idempotent re-push
            ],
            700,
        );
        sd.store(true, Ordering::Relaxed);

        assert!(
            matches!(got.first(), Some(Msg::Welcome { acked: 0, .. })),
            "welcome first: {got:?}"
        );
        assert!(
            got.iter().any(|m| matches!(m, Msg::Snap(_))),
            "snap follows the welcome: {got:?}"
        );
        let acks: Vec<u64> = got
            .iter()
            .filter_map(|m| match m {
                Msg::Ack { seq } => Some(*seq),
                _ => None,
            })
            .collect();
        assert_eq!(
            acks,
            [1, 2, 2],
            "every push acked; the re-push acks the high-water"
        );

        let merged = club.merged.lock().unwrap();
        assert_eq!(
            merged.len(),
            2,
            "the same (pos, seq) twice merges ONCE — re-push is free"
        );
        // POSITIVE CONTROL for the idempotence claim: a NEW seq from the same
        // position DID merge (the check can tell the difference).
        assert!(merged.contains_key(&("aaaa0001".into(), 2)));
        drop(merged);

        // Down-flow was observed: the club delta after the merges reached the
        // wire (a Club line beyond the initial Snap).
        assert!(
            got.iter()
                .any(|m| matches!(m, Msg::Club(st) if !st.dupes.is_empty())),
            "club delta carries the merged dupe keys: {got:?}"
        );
    }

    #[test]
    fn host_refuses_a_newer_protocol_with_a_verbatim_error() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let got = talk(
            addr,
            &[Msg::Join {
                v: PROTO_VERSION + 1,
                pos: "bbbb0001".into(),
                name: String::new(),
                call: String::new(),
                max_seq: 0,
            }],
            500,
        );
        sd.store(true, Ordering::Relaxed);
        assert!(
            matches!(got.first(), Some(Msg::Error { msg }) if msg.contains("update the host")),
            "a newer joiner is refused with a human-readable error: {got:?}"
        );
        // Control: the backend never even saw the join.
        assert!(club.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn the_inbound_surface_is_data_plane_only() {
        // THE PIN for the module-header safety claim. Feed the host loop a
        // hostile stream: control-plane-SHAPED messages (a "settings" write,
        // a "tx" command, a CAT poke — none of which exist in the vocabulary,
        // so they parse as Unknown), the reserved retract, host-side
        // vocabulary echoed back, and a legitimate row. The instrumented
        // backend records every trait call: the ONLY effects that may exist
        // are the ClubBackend data-plane calls, because the trait object is
        // the only thing the socket loop can reach — it has no TX, CAT, or
        // settings capability to invoke.
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let s = TcpStream::connect(addr).unwrap();
        let mut w = s.try_clone().unwrap();
        w.write_all(encode_line(&join_msg("cccc0001", 0)).as_bytes())
            .unwrap();
        for hostile in [
            r#"{"t":"settings","fd_host_enable":false,"mycall":"EVIL"}"#,
            r#"{"t":"tx","enable":true,"message":"CQ CQ"}"#,
            r#"{"t":"cat","freq":14074000,"ptt":true}"#,
            r#"{"t":"halt_tx"}"#,
            r#"{"t":"retract","pos":"cccc0001","seq":1}"#,
            r#"{"t":"welcome","v":1,"acked":999}"#,
            r#"{"t":"ack","seq":999}"#,
            "not even json",
        ] {
            w.write_all(format!("{hostile}\n").as_bytes()).unwrap();
        }
        w.write_all(encode_line(&qso("cccc0001", 1, "W1AW")).as_bytes())
            .unwrap();
        w.write_all(
            encode_line(&Msg::Pos {
                band: "20m".into(),
                mode: "CW".into(),
                op: "OP".into(),
                freq: 0,
                name: "CW tent".into(),
            })
            .as_bytes(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(600));
        drop(w);
        drop(s);
        std::thread::sleep(Duration::from_millis(300));
        sd.store(true, Ordering::Relaxed);

        let calls = club.calls.lock().unwrap().clone();
        // POSITIVE CONTROL: the legitimate traffic DID reach the backend —
        // the recorder works.
        assert!(
            calls.iter().any(|c| c == "merge cccc0001 1"),
            "the real row merged: {calls:?}"
        );
        assert!(calls.iter().any(|c| c.starts_with("pos cccc0001")));
        // THE PIN: nothing beyond the data-plane vocabulary was invoked, and
        // the hostile lines produced NO backend call at all beyond it.
        for c in &calls {
            assert!(
                c.starts_with("join ")
                    || c.starts_with("merge ")
                    || c.starts_with("pos ")
                    || c.starts_with("disconnect "),
                "a non-data-plane effect escaped the socket loop: {c}"
            );
        }
    }

    #[test]
    fn serve_until_stops_and_frees_the_port_on_shutdown() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club);
        assert!(TcpStream::connect(addr).is_ok(), "serving before shutdown");
        sd.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
        let rebind = TcpListener::bind(addr);
        assert!(rebind.is_ok(), "port released after shutdown");
    }

    // ---- position pump over a real socket --------------------------------

    /// Instrumented fake position: three rows in its "journal".
    struct FakePosition {
        acked: Mutex<u64>,
        club_qsos: Mutex<u64>,
        linked: Mutex<Vec<bool>>,
        errors: Mutex<Vec<String>>,
        welcome_now: Mutex<u64>,
        /// The operator's Settings field, renameable mid-session.
        name: Mutex<String>,
    }
    impl Default for FakePosition {
        fn default() -> Self {
            Self {
                acked: Mutex::new(0),
                club_qsos: Mutex::new(0),
                linked: Mutex::new(Vec::new()),
                errors: Mutex::new(Vec::new()),
                welcome_now: Mutex::new(0),
                name: Mutex::new("SSB tent".into()),
            }
        }
    }
    impl PositionSync for FakePosition {
        fn identity(&self) -> (String, String, String, u64) {
            ("dddd0001".into(), "SSB tent".into(), "KD9TAW".into(), 3)
        }
        fn outbox_after(&self, after: u64) -> Vec<WireQso> {
            (after + 1..=3)
                .map(|seq| WireQso {
                    pos: "dddd0001".into(),
                    seq,
                    call: format!("W{seq}AW"),
                    class: "2A".into(),
                    sect: "CT".into(),
                    band: "20m".into(),
                    mode: "PH".into(),
                    sub: String::new(),
                    when: 1_782_583_500,
                    op: "OP".into(),
                })
                .collect()
        }
        fn on_welcome(&self, _acked: u64, _event: &str, _host: &str, now_unix: u64) {
            *self.welcome_now.lock().unwrap() = now_unix;
        }
        fn on_ack(&self, seq: u64) {
            let mut a = self.acked.lock().unwrap();
            *a = (*a).max(seq);
        }
        fn on_club(&self, _snap: bool, st: &ClubState) {
            *self.club_qsos.lock().unwrap() = st.qsos;
        }
        fn on_error(&self, msg: &str) {
            self.errors.lock().unwrap().push(msg.to_string());
        }
        fn position_report(&self) -> Option<PosReport> {
            Some(PosReport {
                band: "20m".into(),
                mode: "PH".into(),
                op: "OP".into(),
                freq: 14_285_000,
                name: self.name.lock().unwrap().clone(),
            })
        }
        fn on_link(&self, up: bool) {
            self.linked.lock().unwrap().push(up);
        }
    }

    #[test]
    fn position_pump_joins_streams_the_gap_and_hears_the_club() {
        let club = Arc::new(FakeClub::default());
        // Host already has row 1 (a previous session) → welcome acks 1, the
        // pump must stream ONLY rows 2..=3 (the gap).
        club.merge(&WireQso {
            pos: "dddd0001".into(),
            seq: 1,
            call: "W1AW".into(),
            class: "2A".into(),
            sect: "CT".into(),
            band: "20m".into(),
            mode: "PH".into(),
            sub: String::new(),
            when: 1,
            op: String::new(),
        });
        club.calls.lock().unwrap().clear(); // the seed above is not wire traffic
        let (addr, host_sd) = start_host(club.clone());

        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));

        // Give the pump a moment to join + stream + get acks + a club line.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && *posn.acked.lock().unwrap() < 3 {
            std::thread::sleep(Duration::from_millis(50));
        }
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        host_sd.store(true, Ordering::Relaxed);

        assert_eq!(*posn.acked.lock().unwrap(), 3, "all rows acked");
        assert_eq!(
            club.merged.lock().unwrap().len(),
            3,
            "host holds the union (1 pre-merged + the 2-row gap)"
        );
        // The gap really was a gap: row 1 was NOT re-merged as a fresh call
        // (idempotence would hide it; the call log proves the stream shape).
        let calls = club.calls.lock().unwrap();
        let merges: Vec<_> = calls.iter().filter(|c| c.starts_with("merge")).collect();
        assert_eq!(
            merges,
            ["merge dddd0001 2", "merge dddd0001 3"],
            "only the gap streamed"
        );
        drop(calls);
        assert!(
            *posn.club_qsos.lock().unwrap() >= 3,
            "club down-flow arrived"
        );
        assert!(
            *posn.welcome_now.lock().unwrap() > 0,
            "welcome carried the host clock (the skew warning's input)"
        );
        assert_eq!(
            *posn.linked.lock().unwrap(),
            vec![true, false],
            "link chip saw up then down"
        );
    }
    #[test]
    fn a_rename_reaches_the_host_on_the_next_report_without_rejoining() {
        // THE BUG (club Field Day, 2026-08): the position's name travelled in
        // the JOIN line and nowhere else, so renaming it in Settings left the
        // club band board showing whatever the connection was born with —
        // the old name, or nothing (the raw position id) for a position that
        // was unnamed when it joined. Only rebuilding the connection fixed
        // it, and nothing told the operator that.
        let club = Arc::new(FakeClub::default());
        let (addr, host_sd) = start_host(club.clone());
        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));

        let saw = |needle: &str| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if club.calls.lock().unwrap().iter().any(|c| c == needle) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            false
        };
        assert!(
            saw("pos dddd0001 20m name=SSB tent"),
            "the first report carries the name at all: {:?}",
            club.calls.lock().unwrap()
        );
        // The operator renames the position in Settings, mid-event.
        *posn.name.lock().unwrap() = "GOTA tent".into();
        assert!(
            saw("pos dddd0001 20m name=GOTA tent"),
            "the rename reached the host: {:?}",
            club.calls.lock().unwrap()
        );
        // ...on the LIVE connection: no second join, and the link never
        // dropped (a reconnect would have carried the new name anyway, which
        // is exactly the bug's workaround, so this is the assertion that
        // makes the test about the fix).
        assert_eq!(
            club.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.starts_with("join "))
                .count(),
            1,
            "one join for the whole session"
        );
        assert_eq!(
            *posn.linked.lock().unwrap(),
            vec![true],
            "the link stayed up across the rename"
        );

        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        host_sd.store(true, Ordering::Relaxed);
    }

    #[test]
    fn an_older_peers_nameless_report_arrives_as_no_news() {
        // Forward/backward compatibility under PROTO_VERSION 1: a v1 position
        // that predates the field sends a `pos` line with no `name`, and the
        // host must hear "" — which its backend reads as "no news", never as
        // "clear the label" (pinned on the policy side in
        // `tempo_app::fdevent`).
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let s = TcpStream::connect(addr).unwrap();
        let mut w = s.try_clone().unwrap();
        w.write_all(encode_line(&join_msg("eeee0001", 0)).as_bytes())
            .unwrap();
        w.write_all(b"{\"t\":\"pos\",\"band\":\"40m\",\"mode\":\"CW\",\"op\":\"OP\"}\n")
            .unwrap();
        std::thread::sleep(Duration::from_millis(400));
        drop(w);
        drop(s);
        sd.store(true, Ordering::Relaxed);
        let calls = club.calls.lock().unwrap().clone();
        assert!(
            calls.iter().any(|c| c == "pos eeee0001 40m name="),
            "the nameless report still landed, with an empty name: {calls:?}"
        );
    }
}
