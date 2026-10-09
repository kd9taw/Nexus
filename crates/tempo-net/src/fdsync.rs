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
//! The ping doubles as a position's clock probe ([`ClockSample`]): the position times its
//! own ping, the host stamps its pong, and the position measures how far its clock is from
//! the host's. That is shown, on the position's club line and the host's board, and it
//! reaches no clock: nothing on either side sets, steps or steers one, FT's slot clock
//! included.
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
///
/// **v2 carries the exchange as data** ([`WireQso::ex`]/[`WireQso::mex`],
/// [`ClubState::dkeys`]). It went to 2 so the new→old direction stays loud: a v2
/// position joining a v1 host is refused there, verbatim, at hour zero. The
/// old→new direction is served — every v2 field is `#[serde(default)]` and the
/// legacy `class`/`sect` pair is still read — EXCEPT when the host is running a
/// contest a v1 position cannot enter, show or send, which the host refuses at
/// JOIN naming the version and the contest (`ClubBackend::join` takes the
/// joiner's `v` for exactly that decision).
pub const PROTO_VERSION: u32 = 2;
/// Default host TCP port. Arbitrary but conflict-checked against the ham
/// ecosystem's squatters: 2237 (WSJT-X UDP), 2242 (JS8Call), 1100 (N3FJP),
/// 12060 (N1MM) are all avoided. A setting, not a constant, at the caller.
pub const DEFAULT_TCP_PORT: u16 = 42073;
/// UDP port the host's once-a-second discovery beacon broadcasts on.
pub const BEACON_PORT: u16 = 42074;
/// One NDJSON line's byte cap, its `\n` included — bounds a hostile peer's memory
/// cost. Every release reads with this cap and drops the connection over a longer
/// line, so it can never grow: the biggest legit state (a `snap` with thousands of
/// dupe keys) is chunked by the sender instead ([`write_club_state`]).
pub const MAX_LINE_BYTES: usize = 8 * 1024;
/// At most this many dupe keys per `snap`/`club` line, and fewer when the line's
/// bytes run out first ([`write_club_state`]). Halved from 100 for v2: a line now
/// carries the legacy triple AND the generalised key for the same entry, and a
/// generalised key is longer than a triple (a QSO party's is five components, one
/// of them a county name). The mirror unions chunks, so chunking is invisible to
/// state.
pub const SNAP_DUPES_PER_LINE: usize = 50;
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
/// What a position says when its host sends a line past [`MAX_LINE_BYTES`]: no release
/// can read one, so the connection drops on it and the next one meets the same line.
/// Only a host older than [`write_club_state`]'s bound sends one — for a big club.
pub const LINE_PAST_THE_CAP: &str = "the host sent club state longer than the 8 KB line every \
     Nexus reads, so this position cannot sync and keeps trying: the host's club has more \
     positions than its Nexus can carry. Update the host's Nexus. Contacts you log meanwhile \
     stay in your own log and go up when it can.";

/// One slot of an exchange on the wire: the `(key, domain, raw)` triple, not a
/// pair. `d` is the domain that matched an `Enum` value (`""` for every other
/// kind) and it travels because a multiplier bucket and an export tag are later
/// chosen by it — a receiver that had to re-resolve it would be guessing with
/// its own rules file, not the sender's.
///
/// Short field names because a `snap` line is capped at [`MAX_LINE_BYTES`] and a
/// club's rows are the bulk of the traffic.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WireField {
    /// Slot id (`"CLASS"`, `"SECTION"`, `"NR"`…).
    pub k: String,
    /// Domain id that matched, `""` for a non-`Enum` kind.
    #[serde(default)]
    pub d: String,
    /// The value as sent/copied, verbatim.
    #[serde(default)]
    pub r: String,
}

/// One Field Day QSO on the wire — the `(pos, seq)` pair is its identity;
/// everything else is the row the club log stores.
///
/// ⚠️ **`class`/`sect` are LEGACY and defaulted, and removing them would be the
/// field failure §12(B) exists to design out.** A v1 position streams them and
/// nothing else; a required field it does not send makes every one of its rows
/// undecodable, and after [`MAX_GARBAGE_LINES`] the host drops that tent's
/// connection — silently, mid-event. They stay, the host synthesises `ex` from
/// them, and both sides keep working.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WireQso {
    /// Position id (8-hex, per machine).
    pub pos: String,
    /// Per-position monotonic sequence (`LoggedQso::seq`). Never 0.
    pub seq: u64,
    pub call: String,
    /// LEGACY (v1): the Field Day class they sent. Defaulted — see the type docs.
    #[serde(default)]
    pub class: String,
    /// LEGACY (v1): the ARRL/RAC section they sent. Defaulted — see the type docs.
    #[serde(default)]
    pub sect: String,
    /// The exchange THEY sent, as data. Empty from a v1 position, and the host
    /// synthesises it from `class`/`sect`.
    #[serde(default)]
    pub ex: Vec<WireField>,
    /// The exchange the LOGGING POSITION sent on this contact.
    ///
    /// ⭐ Without it the host has only its own, and `ClubLog::unique_log` rebuilt
    /// every position's rows under the HOST's class and section — harmless for one
    /// Field Day club, wrong the moment two positions send different exchanges,
    /// which is every QSO party with a mobile. Empty from a v1 position, which
    /// falls back to the club session's sent exchange: exactly what a Field Day
    /// host means today.
    #[serde(default)]
    pub mex: Vec<WireField>,
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
    /// ⭐ **The BIRD this contact was worked through** — the satellite's own catalogue
    /// name, `""` for the terrestrial contact that is almost every row.
    ///
    /// On the wire because it is part of the DUPE KEY under ARRL Field Day's rule (a
    /// satellite is listed as a separate band), and the host builds that key from what
    /// the position sends. A v1/v2 position sends neither this nor `sat_fm`, and both
    /// default — which is exactly what that position meant, since it could not log a
    /// satellite contact into a contest at all.
    #[serde(default)]
    pub sat: String,
    /// A single-channel FM satellite rather than a linear transponder (ARRL limits the
    /// first to one QSO per station). Defaulted, and `false` is the safe side: it
    /// under-reports the limit rather than refusing a legal contact through a linear
    /// bird.
    #[serde(default)]
    pub sat_fm: bool,
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
    /// LEGACY (v1) club dupe keys `(call, band, mode class)` — the position's
    /// while-typing dupe verdict unions these with its own log.
    ///
    /// ⚠️ **Sent only while the club is running Field Day, and EMPTY otherwise.**
    /// A v1 position keeps receiving exactly what it has always received for the
    /// event it can actually run; for any other contest the triple is not the
    /// dupe rule, so sending one would give that position a WRONG warning. Empty
    /// degrades it to no club warning at all, which is the honest failure.
    #[serde(default)]
    pub dupes: Vec<(String, String, String)>,
    /// The club dupe keys under the ruleset's own `DupeRule` (`tempo_core::contest`
    /// — not a dependency of this crate, which owns only the wire) — the
    /// generalised shape, in the same first-seen order and past the same cursor as
    /// [`dupes`](Self::dupes), so the two can never disagree about what has been
    /// worked. `dupes` is this list projected back to its first three components.
    #[serde(default)]
    pub dkeys: Vec<Vec<String>>,
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
        /// ⭐ **The contest this position is logging** — the rules-file id (`"arrlfd"`,
        /// `"ilqp"`), which is what its rows' exchange means.
        ///
        /// The host refuses a position logging a different contest, by name: a club log
        /// keys, scores and exports every row by ONE ruleset, and a row from another is
        /// read against the wrong exchange. Defaulted, and needing no version bump in
        /// either direction: an older host ignores the field, and an older position
        /// sends `""`, which the host serves for Field Day exactly as before and refuses
        /// for anything else.
        #[serde(default)]
        contest: String,
        /// ⭐ **The exchange role this position sends under** (`"in_state"`, `"w_ve"`,
        /// `"dx"`): a QSO party gives a station inside the state one exchange and one
        /// outside it another, chosen by where the station is set up. A club entry is one
        /// station in one place, so the host refuses a position set up in another role, by
        /// name. `""` for a contest with one role (both Field Days) and from a position older
        /// than the field, which the host serves as before — and never written when empty,
        /// so those JOINs are the bytes they always were. An older host ignores it.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        role: String,
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
        /// The contest the host's club log runs (the rules-file id), or `""` from a host
        /// too old to say — which ran a Field Day club log whatever contest it had
        /// selected, so a position logging anything else must not stream to it
        /// ([`PositionSync::host_contest`]).
        #[serde(default)]
        contest: String,
    },
    /// pos→host, one QSO row.
    Qso(WireQso),
    /// host→pos: rows up to and including `seq` are merged (idempotent —
    /// re-pushing an acked row is a no-op, so re-sync needs no bookkeeping).
    Ack { seq: u64 },
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
        /// This position's clock minus the host's, in ms, as it measured it over the
        /// timed ping — the host's board shows it. `None` (absent on the wire) = not
        /// measured: no round trip has closed, or a Nexus older than the field. An older
        /// host ignores it; v1.14.0's and v1.17.0's own decoders were shown this line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clock_ms: Option<i64>,
    },
    /// host→pos on join: full club state (chunked by [`SNAP_DUPES_PER_LINE`]).
    Snap(ClubState),
    /// host→pos after: club-state delta + fresh board.
    Club(ClubState),
    /// Either side's keepalive, and a position's clock probe. A position stamps `t0`, its
    /// own wall clock (Unix ms) as the line leaves, and the host's [`Msg::Pong`] closes the
    /// round trip a [`ClockSample`] is made of. `0` means an untimed ping: the host's own,
    /// and every ping from a Nexus older than the field.
    ///
    /// ⭐ Adding the field needed no version bump, and that was MEASURED, not assumed: the
    /// unit variant this used to be is decoded by serde's internally tagged visitor, which
    /// skips every key but `t`, so v1.14.0's and v1.17.0's own decoders read
    /// `{"t":"ping","t0":…}` as `Ping` and their hosts answer it. Zero is never written, so
    /// an untimed ping is still exactly `{"t":"ping"}`.
    Ping {
        #[serde(default, skip_serializing_if = "is_zero")]
        t0: u64,
    },
    /// The answer to a ping. A host answering a timed ping echoes its `t0` and adds its own
    /// wall clock (Unix ms) when the ping came in (`t1`) and as this line leaves (`t2`).
    /// All three are `0`, and absent on the wire, otherwise: exactly the `{"t":"pong"}` an
    /// older peer sends, which is no sample, so its position keeps the welcome's clock.
    Pong {
        #[serde(default, skip_serializing_if = "is_zero")]
        t0: u64,
        #[serde(default, skip_serializing_if = "is_zero")]
        t1: u64,
        #[serde(default, skip_serializing_if = "is_zero")]
        t2: u64,
    },
    /// RESERVED (defined so a future ship needs no version bump; ignored on
    /// receive today — `FieldDayLog` is append-only with no edit UI).
    Retract { pos: String, seq: u64 },
    /// host→pos refusal (version mismatch etc.) — shown to the operator
    /// verbatim.
    Error { msg: String },
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
    /// The contest the club runs (the rules-file id) — the welcome's `contest`.
    pub contest: String,
}

/// Everything the socket loop can do to the application — and, deliberately,
/// everything it CANNOT: there is no method here that keys TX, touches CAT,
/// or writes a setting, so no inbound byte can reach any of those (the
/// data-plane-only property in the module header).
pub trait ClubBackend: Send + Sync {
    /// A JOIN whose version is not NEWER than ours. `Err(msg)` refuses it (sent
    /// verbatim, then the connection closes).
    ///
    /// ⭐ `v` is the joiner's protocol version, and it reaches the backend because
    /// whether an OLDER position may be served is a question about the CONTEST, not
    /// about the wire: a v1 position can run Field Day perfectly and cannot enter,
    /// show or send a QSO-party exchange. The wire layer owns "newer than us is
    /// refused"; the policy layer owns "older than us, and this contest", and it
    /// owns the wording too, because only it knows the contest to name.
    ///
    /// `contest` is the JOIN's own (`""` from a position too old to send one), and it
    /// reaches the backend for the same reason: whether a position logging that
    /// contest may join this club is the policy layer's question. So does `role`, the
    /// exchange role the position sends under (`""` when it has none to say).
    #[allow(clippy::too_many_arguments)]
    fn join(
        &self,
        v: u32,
        pos: &str,
        name: &str,
        call: &str,
        max_seq: u64,
        contest: &str,
        role: &str,
    ) -> Result<JoinAccept, String>;
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

/// The wall clock in Unix milliseconds, `0` before the epoch — read, never written. The
/// ping/pong stamps are the only use: what they measure is shown, and nothing here or
/// behind either trait can set, step or steer a clock.
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `skip_serializing_if` for the ping/pong stamps: an untimed line stays byte-for-byte the
/// one every older peer sends.
fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// The bytes one value adds to a JSON array: its own, and the comma before it unless it is
/// the array's first.
fn grows_by<T: Serialize>(value: &T, first: bool) -> usize {
    serde_json::to_string(value).map_or(0, |s| s.len()) + usize::from(!first)
}

/// What one club line can carry of a board ([`board_fit`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardFit {
    /// The rows that fit, in the board's own order — every one of them when the board fits.
    pub rows: Vec<WireBoardRow>,
    /// Whether the line still has room for one more row as long as the longest here.
    /// `false` is the host's warning that the next position to join may not fit.
    pub room: bool,
}

/// ⭐ **How much of `board` one club line can carry**: all of it when it fits, and otherwise
/// the rows heard from most recently (the smallest `age`), as many as fit, in the board's own
/// order — so a position that has gone quiet is the first one left out.
///
/// Measured against the longest line a board rides — a heartbeat's `club` line, a byte longer
/// than a snapshot's first chunk, carrying the largest score and count a line can — so the
/// rows kept do not move as the score grows, and a snapshot and every heartbeat after it show
/// the same board.
///
/// The board cannot be split instead: every mirror replaces its board with each chunk's that
/// has rows in it, so a board over two lines would show only the second.
pub fn board_fit(board: &[WireBoardRow]) -> BoardFit {
    let mut used = encode_line(&Msg::Club(ClubState {
        score: u32::MAX,
        qsos: u64::MAX,
        ..Default::default()
    }))
    .len();
    let sizes: Vec<usize> = board.iter().map(|r| grows_by(r, true)).collect();
    let mut freshest: Vec<usize> = (0..board.len()).collect();
    freshest.sort_by_key(|&i| board[i].age);
    let mut kept = vec![false; board.len()];
    let mut n = 0;
    for i in freshest {
        let grow = sizes[i] + usize::from(n > 0);
        if used + grow <= MAX_LINE_BYTES {
            used += grow;
            kept[i] = true;
            n += 1;
        }
    }
    let longest = sizes.iter().copied().max().unwrap_or(0);
    BoardFit {
        rows: board
            .iter()
            .zip(&kept)
            .filter(|(_, &k)| k)
            .map(|(r, _)| r.clone())
            .collect(),
        room: n == board.len() && used + longest + usize::from(n > 0) <= MAX_LINE_BYTES,
    }
}

/// Send a `ClubState` as one or more lines, every one of them inside
/// [`MAX_LINE_BYTES`].
///
/// The FIRST line carries the scalars and the board, the whole board and on that line
/// only — or, when even the board alone would not fit, the rows of it that do
/// ([`board_fit`]). The key lists and the sections then fill that line and as many more
/// as it takes, each line taking what fits, at most [`SNAP_DUPES_PER_LINE`] key pairs a
/// line. Every mirror since club sync shipped (1.10) unions both lists and clears them only
/// on a snapshot's first chunk, so where an entry lands changes no state — and a club that
/// fit before writes exactly the bytes it always did. The first line used to carry every
/// section whatever its size, which put a Field Day club of about 38 positions, or a QSO
/// party that had worked every QTH, past the cap: each position dropped the connection on
/// that line and reconnected into the same one.
///
/// ⚠️ The two key lists are chunked by the SAME index range, because the host
/// builds them index-parallel — entry `n` of `dupes` and entry `n` of `dkeys`
/// are the same worked station under two rules.
///
/// The range runs to the LONGER of the two, not to `dkeys`. `ClubLog::club_state`
/// only ever produces `dkeys` at least as long as `dupes`, so today the two are the
/// same number — but chunking over `dkeys` alone means a `ClubState` with dupes and
/// no dkeys silently ships nothing, and "unreachable and documented" is one
/// refactor away from "reachable and silent" on a path whose failure mode is a
/// tent's dupe warnings quietly not arriving. Taking the max costs one `max` call
/// and removes the hazard instead of describing it.
///
/// An entry too long for a line of its own is left out rather than sent past the cap,
/// where every position would drop the connection on it. Only a hostile peer's row can
/// make one: every line it sent was capped too.
fn write_club_state(w: &mut impl Write, st: ClubState, snap: bool) -> std::io::Result<()> {
    let ClubState {
        reset: _,
        dupes,
        dkeys,
        sections,
        score,
        qsos,
        board,
    } = st;
    let line = |part: ClubState| {
        encode_line(&if snap {
            Msg::Snap(part)
        } else {
            Msg::Club(part)
        })
    };
    let empty = line(ClubState {
        score,
        qsos,
        ..Default::default()
    })
    .len();
    let pair_len = |i: usize| {
        dupes.get(i).map_or(0, |d| grows_by(d, true))
            + dkeys.get(i).map_or(0, |k| grows_by(k, true))
    };
    let fits_alone = |len: usize| empty + len <= MAX_LINE_BYTES;
    let pairs: Vec<usize> = (0..dkeys.len().max(dupes.len()))
        .filter(|&i| fits_alone(pair_len(i)))
        .collect();
    let sections: Vec<&String> = sections
        .iter()
        .filter(|s| fits_alone(grows_by(s, true)))
        .collect();
    let board = board_fit(&board).rows;
    let (mut next_pair, mut next_section) = (0usize, 0usize);
    let mut first = true;
    loop {
        let mut part = ClubState {
            reset: snap && first,
            score,
            qsos,
            board: if first { board.clone() } else { Vec::new() },
            ..Default::default()
        };
        let mut used = line(part.clone()).len();
        while let Some(&i) = pairs.get(next_pair) {
            let (d, k) = (dupes.get(i), dkeys.get(i));
            let grow = d.map_or(0, |d| grows_by(d, part.dupes.is_empty()))
                + k.map_or(0, |k| grows_by(k, part.dkeys.is_empty()));
            if part.dkeys.len().max(part.dupes.len()) == SNAP_DUPES_PER_LINE
                || used + grow > MAX_LINE_BYTES
            {
                break;
            }
            part.dupes.extend(d.cloned());
            part.dkeys.extend(k.cloned());
            used += grow;
            next_pair += 1;
        }
        while let Some(s) = sections.get(next_section) {
            let grow = grows_by(s, part.sections.is_empty());
            if used + grow > MAX_LINE_BYTES {
                break;
            }
            part.sections.push((*s).clone());
            used += grow;
            next_section += 1;
        }
        w.write_all(line(part).as_bytes())?;
        first = false;
        if next_pair >= pairs.len() && next_section >= sections.len() {
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
                        contest,
                        role,
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
                        let accept =
                            match backend.join(v, &pos, &name, &call, max_seq, &contest, &role) {
                                Ok(a) => a,
                                Err(msg) => {
                                    let _ = writer
                                        .write_all(encode_line(&Msg::Error { msg }).as_bytes());
                                    break;
                                }
                            };
                        let welcome = Msg::Welcome {
                            v: PROTO_VERSION,
                            event: accept.event,
                            host_call: accept.host_call,
                            acked: accept.acked,
                            now_unix: now_unix(),
                            contest: accept.contest,
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
                        clock_ms,
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
                                    clock_ms,
                                },
                            );
                        }
                    }
                    Msg::Ping { t0 } => {
                        // A timed ping is a position measuring its clock against ours:
                        // answer with the echo and our wall clock as it came in and as the
                        // pong leaves — one reading plus the monotonic time between, so a
                        // step in our own clock cannot land between the two. An untimed
                        // ping (every older position's) gets the plain pong it always got.
                        let pong = if t0 == 0 {
                            Msg::Pong {
                                t0: 0,
                                t1: 0,
                                t2: 0,
                            }
                        } else {
                            let (t1, read) = (now_unix_ms(), Instant::now());
                            Msg::Pong {
                                t0,
                                t1,
                                t2: t1 + read.elapsed().as_millis() as u64,
                            }
                        };
                        if writer.write_all(encode_line(&pong).as_bytes()).is_err() {
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
                    .write_all(encode_line(&Msg::Ping { t0: 0 }).as_bytes())
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
    /// This position's clock minus the host's, in ms, as it measured it. `None` = not
    /// measured, which the host reads as no news, as it reads an empty `name`: a
    /// reconnect's first report leaves before its first round trip has closed.
    pub clock_ms: Option<i64>,
}

/// One answered round trip of a position's timed ping, as four wall-clock readings in Unix
/// milliseconds: `t0` the ping left this position, `t1` it reached the host, `t2` the host's
/// pong left, `t3` the pong arrived back here.
///
/// `t3` is `t0` plus the round trip as the MONOTONIC clock measured it, not a second wall
/// reading, so a step in this PC's wall clock while the ping was out (WSL2 steps its own
/// about every 30 s) cannot bend the round trip or make it negative. The pump builds one
/// only from a pong that echoes the `t0` it is waiting for and carries both host stamps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockSample {
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub t3: u64,
}

/// The position pump's view of the application. Mirrors [`ClubBackend`]'s
/// discipline: the pump can read the outbox and deliver club state, nothing
/// more.
pub trait PositionSync: Send + Sync {
    /// This position's identity for the JOIN line:
    /// `(pos id, friendly name, station call, own max seq)`.
    fn identity(&self) -> (String, String, String, u64);
    /// The contest this position is logging, for the JOIN line (the rules-file id).
    fn contest(&self) -> String;
    /// The exchange role this position sends under, for the JOIN line (`""` when the
    /// contest has one role).
    fn role(&self) -> String {
        String::new()
    }
    /// The contest the host named in its welcome (`""` from a host too old to name
    /// one). `Err(reason)` = this position must not stream to that host: nothing is
    /// sent, the reason is shown exactly as a host's refusal is, and the session ends
    /// (the pump retries on its backoff, as after any refusal).
    fn host_contest(&self, contest: &str) -> Result<(), String>;
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
    /// One answered round trip of this position's timed ping — what the position measures
    /// its clock against the host's from. Data for the screen and the board, like every
    /// other method here: the trait has no way to set, step or steer a clock.
    fn on_clock(&self, sample: ClockSample);
}

/// Send a timed ping and return what its pong must echo, with the instant it left — the
/// round trip is timed on the monotonic clock from here ([`ClockSample`]).
fn send_timed_ping(w: &mut impl Write) -> std::io::Result<(u64, Instant)> {
    let t0 = now_unix_ms();
    let sent = Instant::now();
    w.write_all(encode_line(&Msg::Ping { t0 }).as_bytes())?;
    Ok((t0, sent))
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
            contest: backend.contest(),
            role: backend.role(),
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
    // The timed ping awaiting its pong: its `t0` and the instant it left. One at a time —
    // a newer ping replaces it, and a pong that does not echo it is no sample.
    let mut ping_out: Option<(u64, Instant)> = None;
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
                        contest,
                        ..
                    }) => {
                        // Checked BEFORE anything is taken from the welcome: a host
                        // running another contest's rules must not be streamed to at
                        // all, and nothing below has happened yet.
                        if let Err(msg) = backend.host_contest(&contest) {
                            backend.on_error(&msg);
                            break Ok(false);
                        }
                        welcomed = true;
                        acked = a;
                        sent_to = a; // everything past the ack re-streams below
                        backend.on_welcome(a, &event, &host_call, now_unix);
                        // Measure now rather than a heartbeat later: until a round trip
                        // closes, the position has only the welcome's whole seconds.
                        ping_out = Some(send_timed_ping(&mut writer)?);
                    }
                    Some(Msg::Pong { t0, t1, t2 }) => {
                        // Only the answer to the ping still out: an older host's untimed
                        // pong carries no stamps, and a stray echo is someone else's.
                        if let Some((sent, at)) = ping_out {
                            if t0 == sent && t1 != 0 && t2 != 0 {
                                ping_out = None;
                                backend.on_clock(ClockSample {
                                    t0,
                                    t1,
                                    t2,
                                    t3: sent + at.elapsed().as_millis() as u64,
                                });
                            }
                        }
                    }
                    Some(Msg::Ack { seq }) => {
                        acked = acked.max(seq);
                        backend.on_ack(seq);
                    }
                    Some(Msg::Snap(st)) => backend.on_club(true, &st),
                    Some(Msg::Club(st)) => backend.on_club(false, &st),
                    Some(Msg::Ping { .. }) => {
                        let pong = Msg::Pong {
                            t0: 0,
                            t1: 0,
                            t2: 0,
                        };
                        writer.write_all(encode_line(&pong).as_bytes())?;
                    }
                    Some(Msg::Error { msg }) => {
                        backend.on_error(&msg);
                        break Ok(welcomed);
                    }
                    _ => {} // Unknown / host-bound vocabulary — ignore
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => {
                // A line past the cap: no release can read one, and the next connection
                // meets the same line. Say so, or the chip simply never settles.
                if e.kind() == std::io::ErrorKind::InvalidData {
                    backend.on_error(LINE_PAST_THE_CAP);
                }
                break Err(e);
            }
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
                    clock_ms,
                }) = report.clone()
                {
                    writer.write_all(
                        encode_line(&Msg::Pos {
                            band,
                            mode,
                            op,
                            freq,
                            name,
                            clock_ms,
                        })
                        .as_bytes(),
                    )?;
                }
                last_report = report;
            }
            if beat {
                ping_out = Some(send_timed_ping(&mut writer)?);
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
                contest: String::new(),
                role: String::new(),
            },
            Msg::Welcome {
                v: 1,
                event: "W9ABC FD".into(),
                host_call: "W9ABC".into(),
                acked: 37,
                now_unix: 1_782_583_500,
                contest: String::new(),
            },
            Msg::Qso(WireQso {
                pos: "a1b2c3d4".into(),
                seq: 38,
                call: "W1AW".into(),
                class: "2A".into(),
                sect: "CT".into(),
                ex: vec![],
                mex: vec![],
                band: "20m".into(),
                mode: "DIG".into(),
                sub: "FT8".into(),
                when: 1_782_583_500,
                op: "KD9TAW".into(),
                sat: String::new(),
                sat_fm: false,
            }),
            Msg::Ack { seq: 38 },
            Msg::Pos {
                band: "20m".into(),
                mode: "CW".into(),
                op: "KD9TAW".into(),
                freq: 14_032_100,
                name: "CW tent".into(),
                clock_ms: Some(-3_000),
            },
            Msg::Snap(ClubState {
                reset: true,
                dupes: vec![("W1AW".into(), "20m".into(), "DIG".into())],
                dkeys: vec![vec!["W1AW".into(), "20M".into(), "DIG".into()]],
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
            Msg::Ping { t0: 0 },
            Msg::Ping {
                t0: 1_791_500_000_123,
            },
            Msg::Pong {
                t0: 0,
                t1: 0,
                t2: 0,
            },
            Msg::Pong {
                t0: 1_791_500_000_123,
                t1: 1_791_500_000_650,
                t2: 1_791_500_000_651,
            },
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
        assert!(encode_line(&Msg::Ping { t0: 0 }).contains("\"t\":\"ping\""));
        assert!(encode_line(&Msg::Ack { seq: 1 }).contains("\"t\":\"ack\""));
        let j = encode_line(&Msg::Join {
            v: 1,
            pos: "p".into(),
            name: String::new(),
            call: String::new(),
            max_seq: 0,
            contest: String::new(),
            role: String::new(),
        });
        assert!(j.contains("\"t\":\"join\"") && j.contains("\"max_seq\""));
        let w = encode_line(&Msg::Welcome {
            v: 1,
            event: String::new(),
            host_call: "X".into(),
            acked: 0,
            now_unix: 0,
            contest: String::new(),
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

    /// ⭐ **The bytes an older Nexus was shown.** The clock stamps went onto the ping and
    /// pong because v1.14.0's and v1.17.0's OWN decoders and socket loops, each release's
    /// `fdsync.rs` built with the serde it locked, read exactly these lines: both decode the
    /// timed ping as `Ping` and the timed pong as `Pong`, and a v1.17.0 host answers the
    /// timed ping with its pong. That proof covers these strings and nothing else, so this
    /// pins the encoder to them: a change here means running it again on the new bytes.
    #[test]
    fn the_new_lines_are_the_bytes_an_older_peer_was_shown() {
        assert_eq!(
            encode_line(&Msg::Ping {
                t0: 1_791_500_000_123
            }),
            "{\"t\":\"ping\",\"t0\":1791500000123}\n"
        );
        assert_eq!(
            encode_line(&Msg::Pong {
                t0: 1_791_500_000_123,
                t1: 1_791_500_000_650,
                t2: 1_791_500_000_651,
            }),
            "{\"t\":\"pong\",\"t0\":1791500000123,\"t1\":1791500000650,\"t2\":1791500000651}\n"
        );
        // Untimed, they are today's lines exactly.
        assert_eq!(encode_line(&Msg::Ping { t0: 0 }), "{\"t\":\"ping\"}\n");
        assert_eq!(
            encode_line(&Msg::Pong {
                t0: 0,
                t1: 0,
                t2: 0
            }),
            "{\"t\":\"pong\"}\n"
        );
        // The presence report, with the clock and without it.
        let pos = |clock_ms| Msg::Pos {
            band: "20m".into(),
            mode: "CW".into(),
            op: "KD9TAW".into(),
            freq: 14_032_100,
            name: "CW tent".into(),
            clock_ms,
        };
        assert_eq!(
            encode_line(&pos(Some(-3_000))),
            "{\"t\":\"pos\",\"band\":\"20m\",\"mode\":\"CW\",\"op\":\"KD9TAW\",\"freq\":14032100,\"name\":\"CW tent\",\"clock_ms\":-3000}\n"
        );
        assert_eq!(
            encode_line(&pos(None)),
            "{\"t\":\"pos\",\"band\":\"20m\",\"mode\":\"CW\",\"op\":\"KD9TAW\",\"freq\":14032100,\"name\":\"CW tent\"}\n"
        );
    }

    /// An older peer's ping and pong decode on this build as UNTIMED, which the pump turns
    /// into no sample at all (the position keeps the welcome's clock).
    #[test]
    fn an_older_peers_ping_and_pong_decode_as_untimed() {
        assert!(matches!(
            decode_line(r#"{"t":"ping"}"#),
            Some(Msg::Ping { t0: 0 })
        ));
        assert!(matches!(
            decode_line(r#"{"t":"pong"}"#),
            Some(Msg::Pong {
                t0: 0,
                t1: 0,
                t2: 0
            })
        ));
        // Control: the stamps are read when they are there.
        assert!(matches!(
            decode_line(r#"{"t":"pong","t0":5,"t1":7,"t2":8}"#),
            Some(Msg::Pong {
                t0: 5,
                t1: 7,
                t2: 8
            })
        ));
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
        /// What the POLICY layer answers an older position with, if anything —
        /// the seam §18.2's refusal comes down.
        refuse_below_v2: Mutex<Option<String>>,
        /// The contest each JOIN named, in arrival order — what reached the policy layer.
        contests: Mutex<Vec<String>>,
        /// The role each JOIN named, in arrival order.
        roles: Mutex<Vec<String>>,
        /// Each presence report's clock, in arrival order.
        clocks: Mutex<Vec<(String, Option<i64>)>>,
    }
    impl FakeClub {
        fn log(&self, s: impl Into<String>) {
            self.calls.lock().unwrap().push(s.into());
        }
    }
    impl ClubBackend for FakeClub {
        fn join(
            &self,
            v: u32,
            pos: &str,
            _name: &str,
            _call: &str,
            _max_seq: u64,
            contest: &str,
            role: &str,
        ) -> Result<JoinAccept, String> {
            self.log(format!("join v{v} {pos}"));
            self.contests.lock().unwrap().push(contest.to_string());
            self.roles.lock().unwrap().push(role.to_string());
            // The guard is dropped before the branch, not held across it: an
            // `if let` scrutinee lives until the end of the body, which is how a
            // lock taken here would still be held while the arm runs.
            let refusal = self.refuse_below_v2.lock().unwrap().clone();
            if v < PROTO_VERSION {
                if let Some(msg) = refusal {
                    return Err(msg);
                }
            }
            Ok(JoinAccept {
                event: "TEST FD".into(),
                host_call: "W9ABC".into(),
                acked: *self.acked.lock().unwrap().get(pos).unwrap_or(&0),
                contest: "arrlfd".into(),
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
            self.clocks
                .lock()
                .unwrap()
                .push((pos.to_string(), r.clock_ms));
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
            let dkeys: Vec<Vec<String>> = dupes
                .iter()
                .map(|(c, b, m)| vec![c.to_uppercase(), b.to_uppercase(), m.to_uppercase()])
                .collect();
            ClubState {
                reset: false,
                dupes: dupes.into_iter().skip(dupes_from).collect(),
                dkeys: dkeys.into_iter().skip(dupes_from).collect(),
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

    /// [`talk`] over RAW lines — the bytes an older build writes, rather than
    /// this build's encoder round-tripping its own types. A compatibility test
    /// that encodes with today's `Msg` proves nothing about yesterday's bytes.
    fn talk_raw(addr: std::net::SocketAddr, lines: &[&str], read_for_ms: u64) -> Vec<Msg> {
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        for l in lines {
            w.write_all(l.as_bytes()).unwrap();
            w.write_all(b"\n").unwrap();
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
            contest: String::new(),
            role: String::new(),
        }
    }

    fn qso(pos: &str, seq: u64, call: &str) -> Msg {
        Msg::Qso(WireQso {
            pos: pos.into(),
            seq,
            call: call.into(),
            class: "2A".into(),
            sect: "CT".into(),
            ex: vec![],
            mex: vec![],
            band: "20m".into(),
            mode: "DIG".into(),
            sub: "FT8".into(),
            when: 1_782_583_500,
            op: "OP".into(),
            sat: String::new(),
            sat_fm: false,
        })
    }

    /// The host answers a TIMED ping with the echo and its own wall clock twice (the ping in,
    /// the pong out), over a real socket; an untimed ping, every older position's, still
    /// gets the plain pong it always got.
    #[test]
    fn the_host_answers_a_timed_ping_with_its_clock_and_an_untimed_one_as_before() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club);
        let join = encode_line(&join_msg("ffff0001", 0));
        let before = now_unix_ms();
        let got = talk_raw(
            addr,
            &[
                join.trim_end(),
                r#"{"t":"ping","t0":123}"#,
                r#"{"t":"ping"}"#,
            ],
            800,
        );
        let after = now_unix_ms();
        sd.store(true, Ordering::Relaxed);
        let pongs: Vec<(u64, u64, u64)> = got
            .iter()
            .filter_map(|m| match m {
                Msg::Pong { t0, t1, t2 } => Some((*t0, *t1, *t2)),
                _ => None,
            })
            .collect();
        assert_eq!(pongs.len(), 2, "one pong per ping: {pongs:?}");
        let (t0, t1, t2) = pongs[0];
        assert_eq!(t0, 123, "the echo of the ping it answers");
        assert!(
            before <= t1 && t1 <= t2 && t2 <= after,
            "the host's clock as the ping came in and as the pong left: {before} ≤ {t1} ≤ {t2} ≤ {after}"
        );
        assert_eq!(pongs[1], (0, 0, 0), "an untimed ping gets today's pong");
    }

    /// A position's report carries its measured clock to the host's backend, and an older
    /// position's report, which has none, arrives as `None`.
    #[test]
    fn a_report_carries_the_positions_clock_to_the_host_and_an_older_ones_none() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let join = encode_line(&join_msg("ffff0002", 0));
        talk_raw(
            addr,
            &[
                join.trim_end(),
                r#"{"t":"pos","band":"20m","mode":"CW","op":"KD9TAW","freq":14032100,"name":"CW tent","clock_ms":-3000}"#,
                r#"{"t":"pos","band":"20m","mode":"CW","op":"KD9TAW","freq":14032100,"name":"CW tent"}"#,
            ],
            500,
        );
        sd.store(true, Ordering::Relaxed);
        assert_eq!(
            *club.clocks.lock().unwrap(),
            [
                ("ffff0002".to_string(), Some(-3_000)),
                ("ffff0002".to_string(), None)
            ]
        );
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

    /// ⭐ THE old→new direction, at the byte level. These are the exact lines a
    /// shipped 1.x build writes — no `ex`, no `mex`, no `dkeys`. If any v2 field
    /// were required, `decode_line` would return `None`, every row from that tent
    /// would count as garbage, and after [`MAX_GARBAGE_LINES`] the host would drop
    /// the connection SILENTLY, mid-event. So this asserts `Some`, not just the
    /// contents.
    #[test]
    fn a_literal_v1_line_still_decodes_on_a_v2_build() {
        let join =
            r#"{"t":"join","v":1,"pos":"aaaa0001","name":"CW tent","call":"KD9TAW","max_seq":7}"#;
        assert!(
            matches!(
                decode_line(join),
                Some(Msg::Join {
                    v: 1,
                    max_seq: 7,
                    ..
                })
            ),
            "a v1 join line must decode, not count as garbage"
        );

        let row = r#"{"t":"qso","pos":"aaaa0001","seq":8,"call":"W1AW","class":"2A","sect":"CT","band":"20m","mode":"DIG","sub":"FT8","when":1782583500,"op":"KD9TAW"}"#;
        let Some(Msg::Qso(q)) = decode_line(row) else {
            panic!("a v1 QSO line must decode: {row}");
        };
        assert_eq!((q.class.as_str(), q.sect.as_str()), ("2A", "CT"));
        assert!(
            q.ex.is_empty() && q.mex.is_empty(),
            "the v2 fields default to empty rather than failing the decode"
        );

        let snap = r#"{"t":"snap","reset":true,"dupes":[["W1AW","20m","DIG"]],"sections":["CT"],"score":10,"qsos":1,"board":[]}"#;
        let Some(Msg::Snap(st)) = decode_line(snap) else {
            panic!("a v1 snap line must decode: {snap}");
        };
        assert_eq!(st.dupes.len(), 1);
        assert!(st.dkeys.is_empty(), "a v1 host sends no generalised keys");

        // POSITIVE CONTROL — the decoder really can tell the shapes apart, so the
        // assertions above are about compatibility and not about everything
        // parsing to empty.
        let v2 = r#"{"t":"qso","pos":"aaaa0001","seq":9,"call":"K1ABC","class":"","sect":"","ex":[{"k":"SECTION","d":"fd_sections","r":"EMA"}],"mex":[{"k":"CLASS","d":"","r":"3A"}],"band":"20m","mode":"CW","sub":"","when":1782583600,"op":"KD9TAW"}"#;
        let Some(Msg::Qso(q2)) = decode_line(v2) else {
            panic!("control: a v2 QSO line must decode too");
        };
        assert_eq!(q2.ex.len(), 1);
        assert_eq!(q2.ex[0].d, "fd_sections");
        assert_eq!(q2.mex.len(), 1);
    }

    /// A v1 position joins a v2 host over a real socket, is welcomed, and its
    /// legacy-shaped rows merge — and the JOINER'S VERSION reaches the policy
    /// layer, which is what §18.2's ruling is decided on.
    #[test]
    fn a_v1_position_joins_a_v2_host_and_its_legacy_rows_merge() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let got = talk_raw(
            addr,
            &[
                r#"{"t":"join","v":1,"pos":"aaaa0001","name":"CW tent","call":"KD9TAW","max_seq":0}"#,
                r#"{"t":"qso","pos":"aaaa0001","seq":1,"call":"W1AW","class":"2A","sect":"CT","band":"20m","mode":"PH","sub":"","when":1782583500,"op":"KD9TAW"}"#,
            ],
            700,
        );
        sd.store(true, Ordering::Relaxed);
        assert!(
            got.iter()
                .any(|m| matches!(m, Msg::Welcome { v, .. } if *v == PROTO_VERSION)),
            "the v1 position is welcomed, and told the host's version: {got:?}"
        );
        let calls = club.calls.lock().unwrap().clone();
        assert!(
            calls.contains(&"join v1 aaaa0001".to_string()),
            "the joiner's version reached the policy layer: {calls:?}"
        );
        assert!(
            calls.contains(&"merge aaaa0001 1".to_string()),
            "…and its row merged: {calls:?}"
        );
        let merged = club.merged.lock().unwrap();
        let row = &merged[&("aaaa0001".to_string(), 1)];
        assert_eq!((row.class.as_str(), row.sect.as_str()), ("2A", "CT"));
    }

    /// §18.2 at the wire: the host sends the policy layer's refusal VERBATIM and
    /// closes, so the position shows the operator a message naming the version and
    /// the contest instead of a bare failure.
    #[test]
    fn the_host_sends_the_policy_layers_version_refusal_verbatim() {
        let club = Arc::new(FakeClub::default());
        *club.refuse_below_v2.lock().unwrap() = Some(
            "this club is running TN-QSO-PARTY and needs club sync v2 — this Nexus \
             speaks v1, which cannot enter, show or send the TN-QSO-PARTY exchange."
                .into(),
        );
        let (addr, sd) = start_host(club.clone());
        let got = talk_raw(
            addr,
            &[r#"{"t":"join","v":1,"pos":"aaaa0001","name":"","call":"","max_seq":0}"#],
            500,
        );
        sd.store(true, Ordering::Relaxed);
        assert!(
            matches!(got.first(), Some(Msg::Error { msg })
                if msg == &club.refuse_below_v2.lock().unwrap().clone().unwrap()),
            "the refusal reaches the operator unaltered: {got:?}"
        );
        // POSITIVE CONTROL: the SAME host serves a current position. A gate that
        // refused everybody would pass the assertion above.
        let (addr2, sd2) = start_host(club.clone());
        let ok = talk_raw(
            addr2,
            &[r#"{"t":"join","v":2,"pos":"bbbb0002","name":"","call":"","max_seq":0}"#],
            500,
        );
        sd2.store(true, Ordering::Relaxed);
        assert!(
            ok.iter().any(|m| matches!(m, Msg::Welcome { .. })),
            "control: a v2 position is welcomed by the same host: {ok:?}"
        );
    }

    /// ⭐ THE new→old direction, and the reason `PROTO_VERSION` went to 2 at all:
    /// a v2 position joining a SHIPPED v1 host is refused there, loudly, at hour
    /// zero — and shows the operator that host's own words.
    ///
    /// The v1 host is a socket speaking v1's rule, because the shipped build
    /// cannot be linked in here; the bytes it answers with are `fdsync.rs`'s v1
    /// error text, which is what a 1.x host actually writes.
    #[test]
    fn a_v2_position_joining_a_v1_host_gets_that_hosts_refusal_verbatim() {
        const V1_ERROR: &str =
            "this host speaks Field Day sync v1, you sent v2 — update the host's Nexus";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen_v = Arc::new(Mutex::new(0u32));
        let seen_v2 = seen_v.clone();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut w = stream.try_clone().unwrap();
            let mut r = BufReader::new(stream);
            let line = read_capped_line(&mut r).unwrap().unwrap();
            if let Some(Msg::Join { v, .. }) = decode_line(&line) {
                *seen_v2.lock().unwrap() = v;
                // v1's rule, verbatim: refuse a higher version and close.
                if v > 1 {
                    let _ = w.write_all(
                        encode_line(&Msg::Error {
                            msg: V1_ERROR.into(),
                        })
                        .as_bytes(),
                    );
                }
            }
        });

        let pos = Arc::new(FakePosition::default());
        let sd = Arc::new(AtomicBool::new(false));
        let p: Arc<dyn PositionSync> = pos.clone();
        let sd2 = sd.clone();
        let a = addr.to_string();
        let h = std::thread::spawn(move || run_position_until(&a, p, sd2));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && pos.errors.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
        sd.store(true, Ordering::Relaxed);
        let _ = h.join();

        assert_eq!(
            *seen_v.lock().unwrap(),
            2,
            "this build's position announces v2, which is what makes the old host refuse it"
        );
        assert_eq!(
            pos.errors.lock().unwrap().as_slice(),
            [V1_ERROR.to_string()],
            "the old host's own words reach the operator unaltered"
        );
    }

    /// ⭐ The two key lists survive CHUNKING index-parallel — asserted across a real
    /// boundary, because `SNAP_DUPES_PER_LINE` is 50 and a club works more than 50
    /// stations in an hour, so every real event crosses it.
    ///
    /// Entry `n` of `dupes` and entry `n` of `dkeys` are the same worked station
    /// under two rules. If chunking could shift one relative to the other, a
    /// position would union a `dupes` triple with a `dkeys` key belonging to a
    /// different contact and warn about the wrong station.
    #[test]
    fn chunking_keeps_dupes_and_dkeys_index_parallel_across_the_boundary() {
        // 120 keys = three chunks at 50, so the seam is exercised twice.
        let n = 120;
        let dupes: Vec<(String, String, String)> = (0..n)
            .map(|i| (format!("W{i}AW"), "20m".into(), "CW".into()))
            .collect();
        let dkeys: Vec<Vec<String>> = dupes
            .iter()
            .map(|(c, b, m)| vec![c.to_uppercase(), b.to_uppercase(), m.clone()])
            .collect();
        let st = ClubState {
            reset: false,
            dupes: dupes.clone(),
            dkeys: dkeys.clone(),
            sections: vec!["WI".into()],
            score: 7,
            qsos: 120,
            board: Vec::new(),
        };

        let mut buf: Vec<u8> = Vec::new();
        write_club_state(&mut buf, st, true).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(
            text.lines().count(),
            3,
            "120 keys at 50 per line is 3 lines"
        );
        assert!(
            text.lines().all(|l| l.len() <= MAX_LINE_BYTES),
            "every chunk stays under the wire's line cap"
        );

        // Reassemble exactly as a position's mirror does, and check the PAIRING
        // rather than just the totals — two lists of the right length that had
        // drifted apart would pass a count check.
        let mut got_dupes = Vec::new();
        let mut got_dkeys = Vec::new();
        for line in text.lines() {
            let Some(Msg::Snap(part)) = decode_line(line) else {
                panic!("each chunk is a snap line: {line}");
            };
            assert_eq!(
                part.dupes.len(),
                part.dkeys.len(),
                "the two lists are cut at the SAME index in every chunk"
            );
            got_dupes.extend(part.dupes);
            got_dkeys.extend(part.dkeys);
        }
        assert_eq!(got_dupes, dupes, "every dupe arrived, in order");
        assert_eq!(got_dkeys, dkeys, "every generalised key arrived, in order");
        for (t, k) in got_dupes.iter().zip(&got_dkeys) {
            assert_eq!(
                t.0.to_uppercase(),
                k[0],
                "entry n of each list is still the same station"
            );
        }

        // A `dupes`-only state (the shape a non-Field-Day club would produce if the
        // legacy projection were ever re-enabled) must not vanish. Chunking over
        // `dkeys` alone shipped NOTHING here — unreachable from `club_state` today,
        // one refactor from reachable, and silent when it happens.
        let mut buf: Vec<u8> = Vec::new();
        write_club_state(
            &mut buf,
            ClubState {
                dupes: dupes.clone(),
                dkeys: Vec::new(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let shipped: usize = String::from_utf8(buf)
            .unwrap()
            .lines()
            .map(|l| match decode_line(l) {
                Some(Msg::Club(p)) => p.dupes.len(),
                _ => panic!("a club line: {l}"),
            })
            .sum();
        assert_eq!(
            shipped, n,
            "a dupes-only state ships all its dupes, not none"
        );

        // …and the empty state still writes exactly one line, which is what tells a
        // joining position "here is the snapshot, it is empty".
        let mut buf: Vec<u8> = Vec::new();
        write_club_state(&mut buf, ClubState::default(), true).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap().lines().count(), 1);
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
                contest: String::new(),
                role: String::new(),
            }],
            500,
        );
        sd.store(true, Ordering::Relaxed);
        assert!(
            matches!(got.first(), Some(Msg::Error { msg }) if msg.contains("update the host")),
            "a newer joiner is refused with a human-readable error: {got:?}"
        );
        // …and the numbers in it are this build's, not a frozen "v1": the message
        // is the ONLY thing that tells the operator which side is behind, and it
        // moved when PROTO_VERSION did.
        assert!(
            matches!(got.first(), Some(Msg::Error { msg })
                if msg == "this host speaks Field Day sync v2, you sent v3 — update the host's Nexus"),
            "the refusal names both versions: {got:?}"
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
            // The clock probe at its extremes: answered on the socket, nothing more.
            r#"{"t":"ping","t0":18446744073709551615}"#,
            r#"{"t":"pong","t0":1,"t1":2,"t2":3}"#,
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
                clock_ms: Some(i64::MIN), // a clock at its extreme is data like any other
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

    /// A listener on a free port BELOW the kernel's ephemeral range, for a test that lets the
    /// port go and then binds it again. In between, a `:0` bind anywhere on the box or an
    /// outgoing connection can be handed any free port in that range, this one included, and
    /// the rebind then fails with nothing wrong in the code under test. Below the range only a
    /// bind that names the port can land (`freePortOutsideEphemeralRange` in
    /// `remote/test/runtime.mjs` makes the same choice).
    fn listener_below_the_ephemeral_range() -> TcpListener {
        use std::hash::BuildHasher;
        let low = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
            .ok()
            .and_then(|range| range.split_whitespace().next()?.parse().ok())
            .unwrap_or(32_768u16);
        let (floor, ceiling) = (20_000u16, low.min(32_768));
        assert!(ceiling > floor, "the ephemeral range starts at {low}");
        (0..64u8)
            .map(|attempt| {
                let r = std::hash::RandomState::new().hash_one(attempt);
                floor + (r % u64::from(ceiling - floor)) as u16
            })
            .find_map(|port| TcpListener::bind(("127.0.0.1", port)).ok())
            .expect("a free port below the ephemeral range")
    }

    #[test]
    fn serve_until_stops_and_frees_the_port_on_shutdown() {
        // Not `start_host`: its `:0` port is free to be handed to another bind during the
        // wait below (a scratch `:0` bind limited to that port was handed it, and the rebind
        // failed).
        let listener = listener_below_the_ephemeral_range();
        let addr = listener.local_addr().unwrap();
        let sd = Arc::new(AtomicBool::new(false));
        let sd2 = sd.clone();
        let club: Arc<dyn ClubBackend> = Arc::new(FakeClub::default());
        std::thread::spawn(move || serve_until(listener, club, sd2));
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
        /// The contests hosts named in their welcomes, in arrival order.
        host_contests: Mutex<Vec<String>>,
        /// What the POLICY layer answers a host's contest with — `Some` refuses it.
        refuse_host: Mutex<Option<String>>,
        /// Every round trip the pump handed back, in arrival order.
        samples: Mutex<Vec<ClockSample>>,
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
                host_contests: Mutex::new(Vec::new()),
                refuse_host: Mutex::new(None),
                samples: Mutex::new(Vec::new()),
            }
        }
    }
    impl PositionSync for FakePosition {
        fn identity(&self) -> (String, String, String, u64) {
            ("dddd0001".into(), "SSB tent".into(), "KD9TAW".into(), 3)
        }
        fn contest(&self) -> String {
            "ilqp".into()
        }
        fn role(&self) -> String {
            "in_state".into()
        }
        fn host_contest(&self, contest: &str) -> Result<(), String> {
            self.host_contests.lock().unwrap().push(contest.to_string());
            // The guard is dropped before the branch, as `FakeClub::join`'s is.
            let refusal = self.refuse_host.lock().unwrap().clone();
            match refusal {
                Some(msg) => Err(msg),
                None => Ok(()),
            }
        }
        fn outbox_after(&self, after: u64) -> Vec<WireQso> {
            (after + 1..=3)
                .map(|seq| WireQso {
                    pos: "dddd0001".into(),
                    seq,
                    call: format!("W{seq}AW"),
                    class: "2A".into(),
                    sect: "CT".into(),
                    ex: vec![],
                    mex: vec![],
                    band: "20m".into(),
                    mode: "PH".into(),
                    sub: String::new(),
                    when: 1_782_583_500,
                    op: "OP".into(),
                    sat: String::new(),
                    sat_fm: false,
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
                clock_ms: None,
            })
        }
        fn on_link(&self, up: bool) {
            self.linked.lock().unwrap().push(up);
        }
        fn on_clock(&self, sample: ClockSample) {
            self.samples.lock().unwrap().push(sample);
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
            ex: vec![],
            mex: vec![],
            band: "20m".into(),
            mode: "PH".into(),
            sub: String::new(),
            when: 1,
            op: String::new(),
            sat: String::new(),
            sat_fm: false,
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

    /// ⭐ **The contest travels both ways at JOIN**, because a club log keys, scores and
    /// exports by one ruleset and the policy layer at each end has to be able to say no.
    /// The position's own contest reaches the host's backend, and the host's reaches the
    /// position's — over a real socket, through the real pump and accept loop.
    #[test]
    fn the_join_names_the_positions_contest_and_the_welcome_names_the_clubs() {
        let club = Arc::new(FakeClub::default());
        let (addr, host_sd) = start_host(club.clone());
        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && *posn.acked.lock().unwrap() < 3 {
            std::thread::sleep(Duration::from_millis(50));
        }
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();

        assert_eq!(
            *club.contests.lock().unwrap(),
            vec!["ilqp".to_string()],
            "the position's contest reached the host's policy layer"
        );
        assert_eq!(
            *club.roles.lock().unwrap(),
            vec!["in_state".to_string()],
            "…and so did the role it sends under"
        );
        assert_eq!(
            *posn.host_contests.lock().unwrap(),
            vec!["arrlfd".to_string()],
            "…and the club's reached the position's"
        );

        // OLDER BYTES, both directions: a JOIN with no `contest` (any build before this
        // field) reaches the backend as "", which is what the policy layer reads as "too
        // old to say" — not a parse failure, and not some contest by default.
        let got = talk_raw(
            addr,
            &[r#"{"t":"join","v":2,"pos":"eeee0001","name":"","call":"KD9TAW","max_seq":0}"#],
            400,
        );
        assert!(
            got.iter().any(|m| matches!(m, Msg::Welcome { .. })),
            "an older position's join still decodes and is answered: {got:?}"
        );
        assert_eq!(
            club.contests.lock().unwrap().last().map(String::as_str),
            Some(""),
            "an older JOIN names no contest"
        );
        assert_eq!(
            club.roles.lock().unwrap().last().map(String::as_str),
            Some(""),
            "…and no role, which the policy layer serves as it always has"
        );
        // A JOIN with no role to say is the bytes every older host was shown: no key at all.
        let no_role = encode_line(&Msg::Join {
            v: PROTO_VERSION,
            pos: "eeee0001".into(),
            name: String::new(),
            call: "KD9TAW".into(),
            max_seq: 0,
            contest: "arrlfd".into(),
            role: String::new(),
        });
        assert!(!no_role.contains("role"), "{no_role}");
        host_sd.store(true, Ordering::Relaxed);
        // …and an older host's WELCOME decodes with no contest, which is what the position
        // has to refuse a non-Field-Day stream on.
        assert!(matches!(
            decode_line(r#"{"t":"welcome","v":2,"event":"X","host_call":"W9ABC","acked":0,"now_unix":1}"#),
            Some(Msg::Welcome { contest, .. }) if contest.is_empty()
        ));
    }

    /// ⭐ A position whose policy layer refuses the host's contest STREAMS NOTHING — the
    /// check runs on the welcome, before the outbox is read — and the reason reaches the
    /// operator the way a host's own refusal does.
    #[test]
    fn a_position_that_refuses_the_hosts_contest_streams_nothing_and_says_why() {
        let club = Arc::new(FakeClub::default());
        let (addr, host_sd) = start_host(club.clone());
        let posn = Arc::new(FakePosition::default());
        *posn.refuse_host.lock().unwrap() = Some("the host runs another contest".into());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && posn.errors.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(50));
        }
        // Long enough that a pump that kept going after the welcome would have streamed.
        std::thread::sleep(Duration::from_millis(400));
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        host_sd.store(true, Ordering::Relaxed);

        assert_eq!(
            posn.errors.lock().unwrap().first().map(String::as_str),
            Some("the host runs another contest"),
            "the refusal is shown, verbatim"
        );
        assert!(
            club.merged.lock().unwrap().is_empty(),
            "not one row reached a host whose contest was refused"
        );
        assert_eq!(
            *posn.welcome_now.lock().unwrap(),
            0,
            "the refused welcome was not taken (no ack, no clock)"
        );
        // POSITIVE CONTROL: the same position with the refusal lifted streams its rows —
        // so the empty merge above is the refusal, not a pump that never connected.
        *posn.refuse_host.lock().unwrap() = None;
        let (addr, host_sd) = start_host(club.clone());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && *posn.acked.lock().unwrap() < 3 {
            std::thread::sleep(Duration::from_millis(50));
        }
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        host_sd.store(true, Ordering::Relaxed);
        assert_eq!(
            club.merged.lock().unwrap().len(),
            3,
            "control: all three rows merged"
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

    /// ⭐ **The pump times its own ping and hands back the round trip its pong closes**, by
    /// value, over a real socket: a scripted host running 45 s ahead of this PC stamps the
    /// pong, and the one sample carries the echo, the host's two stamps and a round trip
    /// measured here. The first timed ping goes out as the welcome is taken, not a heartbeat
    /// later. A pong that closes no outstanding ping (an older host's untimed one, one
    /// echoing another `t0`, a second copy of the answer) is no sample.
    #[test]
    fn the_pump_times_its_ping_and_takes_only_the_answer_it_is_waiting_for() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
        let addr = listener.local_addr().unwrap().to_string();
        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr, pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));

        let (s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        let next_ping = |r: &mut BufReader<TcpStream>| -> Option<(u64, Instant)> {
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if let Ok(Some(line)) = read_capped_line(r) {
                    if let Some(Msg::Ping { t0 }) = decode_line(&line) {
                        return Some((t0, Instant::now()));
                    }
                }
            }
            None
        };
        w.write_all(
            encode_line(&Msg::Welcome {
                v: PROTO_VERSION,
                event: "TEST FD".into(),
                host_call: "W9ABC".into(),
                acked: 3,
                now_unix: now_unix(),
                contest: "ilqp".into(),
            })
            .as_bytes(),
        )
        .unwrap();
        let welcomed = Instant::now();
        let (t0, pinged) = next_ping(&mut r).expect("a ping");
        assert!(t0 > 0, "the position's ping is timed");
        assert!(
            pinged.duration_since(welcomed) < Duration::from_secs(2),
            "measured as the welcome is taken, not a heartbeat later"
        );
        // Not answers: an older host's untimed pong, and one echoing another ping.
        w.write_all(b"{\"t\":\"pong\"}\n").unwrap();
        w.write_all(
            encode_line(&Msg::Pong {
                t0: t0 + 1,
                t1: 5,
                t2: 6,
            })
            .as_bytes(),
        )
        .unwrap();
        // The answer, from a host 45 s ahead that took 1 ms over it — then a second copy.
        let t1 = now_unix_ms() + 45_000;
        let answer = encode_line(&Msg::Pong { t0, t1, t2: t1 + 1 });
        w.write_all(answer.as_bytes()).unwrap();
        w.write_all(answer.as_bytes()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && posn.samples.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300)); // room for a wrong second sample
        pump_sd.store(true, Ordering::Relaxed);
        drop(w);
        drop(r);
        pump.join().unwrap();

        let samples = posn.samples.lock().unwrap().clone();
        assert_eq!(
            samples.len(),
            1,
            "exactly the answer it waited for: {samples:?}"
        );
        let got = samples[0];
        assert_eq!((got.t0, got.t1, got.t2), (t0, t1, t1 + 1));
        assert!(
            got.t3 >= got.t0 && got.t3 - got.t0 < 2_000,
            "the round trip, measured here: {got:?}"
        );
        // Host minus this PC, round-trip corrected: +45 s, give or take half the round trip, plus
        // the stamps' own whole-millisecond rounding. The four stamps are truncated to the
        // millisecond, so the offset can sit up to about 2 ms off even when the round trip
        // itself measures 0 ms: a fast, idle machine measured t3 == t0 and an offset of
        // 45 001 ms, which a bound of the round trip alone (0 ms) refused.
        let offset = ((got.t1 as i64 - got.t0 as i64) + (got.t2 as i64 - got.t3 as i64)) / 2;
        assert!(
            (offset - 45_000).abs() <= (got.t3 - got.t0) as i64 + 2,
            "the host is 45 s ahead: {offset} ms"
        );
    }

    // ---- the 8 KB line, and a club big enough to reach it ------------------

    /// A board row as long as a real one gets: a 16-character position name, an
    /// 8-character operator, 4-digit counts and a 5-digit age — the sizes the club
    /// clock's line budget was measured with.
    fn long_row(i: usize, age: u64) -> WireBoardRow {
        WireBoardRow {
            pos: format!("{:08x}", 0x9a85_f000_u32 + i as u32),
            name: format!("Position {i:02} tent"),
            band: "160m".into(),
            mode: "DIG".into(),
            op: format!("KD9T{i:04}"),
            qsos: 1234,
            uniq: 1234,
            rate: 1234,
            age,
        }
    }

    /// A club's state as its join snapshot carries it: `rows` positions, 120 worked
    /// stations and everything worked so far. A Field Day club sends the legacy triple
    /// beside each key and its 85 ARRL/RAC sections; a QSO party sends five-component
    /// keys only, and its list is the QTHs it has worked — 165 for one that has worked
    /// every county and state.
    fn big_club(rows: usize, field_day: bool) -> ClubState {
        let keys: Vec<Vec<String>> = (0..120)
            .map(|i| {
                let mut k = vec![format!("K9A{i:03}"), "160M".to_string(), "DIG".to_string()];
                if !field_day {
                    k.extend(["COOK".to_string(), "MCLN".to_string()]);
                }
                k
            })
            .collect();
        ClubState {
            reset: false,
            dupes: if field_day {
                keys.iter()
                    .map(|k| (k[0].clone(), k[1].clone(), k[2].clone()))
                    .collect()
            } else {
                Vec::new()
            },
            dkeys: keys,
            sections: if field_day {
                (0..85).map(|i| format!("S{i:02}")).collect()
            } else {
                (0..165).map(|i| format!("Q{i:03}")).collect()
            },
            score: 12_345,
            qsos: 4_321,
            board: (0..rows).map(|i| long_row(i, 12_345)).collect(),
        }
    }

    /// Every line `write_club_state` writes for `st`, each with its `\n` — the bytes a
    /// receiver's cap is measured against.
    fn lines_of(st: ClubState, snap: bool) -> Vec<String> {
        let mut buf = Vec::new();
        write_club_state(&mut buf, st, snap).unwrap();
        String::from_utf8(buf)
            .unwrap()
            .split_inclusive('\n')
            .map(str::to_string)
            .collect()
    }

    /// The lines every release before this one wrote: 50 key pairs a line, and the
    /// sections and the board on the first line whatever their size. Rebuilt here so the
    /// new sender can be compared with it byte for byte, and so the ceiling it had can
    /// still be measured.
    fn old_lines(st: &ClubState, snap: bool) -> Vec<String> {
        let total = st.dkeys.len().max(st.dupes.len());
        let mut out = Vec::new();
        let mut start = 0;
        loop {
            let end = (start + SNAP_DUPES_PER_LINE).min(total);
            let first = start == 0;
            let part = ClubState {
                reset: snap && first,
                dupes: st
                    .dupes
                    .get(start..end.min(st.dupes.len()))
                    .unwrap_or(&[])
                    .to_vec(),
                dkeys: st
                    .dkeys
                    .get(start..end.min(st.dkeys.len()))
                    .unwrap_or(&[])
                    .to_vec(),
                sections: if first {
                    st.sections.clone()
                } else {
                    Vec::new()
                },
                score: st.score,
                qsos: st.qsos,
                board: if first { st.board.clone() } else { Vec::new() },
            };
            out.push(encode_line(&if snap {
                Msg::Snap(part)
            } else {
                Msg::Club(part)
            }));
            if end >= total {
                return out;
            }
            start = end;
        }
    }

    /// What a position's mirror holds after reading `lines`, by the rule every release
    /// since club sync shipped applies them with (`ClubMirror::apply`, the same body in
    /// 1.10.0, 1.10.3, 1.12, 1.13, 1.14 and 1.17): the first chunk of a snapshot clears
    /// the lists, every chunk unions them and overwrites the scalars, and a chunk's board
    /// replaces the board only when it has rows or resets. A line past the cap is not
    /// read at all — the receiver drops the connection on it — so it panics here.
    #[derive(Debug, Default, PartialEq)]
    struct Mirrored {
        dupes: std::collections::HashSet<(String, String, String)>,
        dkeys: std::collections::HashSet<Vec<String>>,
        sections: std::collections::HashSet<String>,
        score: u32,
        qsos: u64,
        board: Vec<WireBoardRow>,
    }
    fn mirror_of(lines: &[String]) -> Mirrored {
        let mut m = Mirrored::default();
        for line in lines {
            assert!(
                line.len() <= MAX_LINE_BYTES,
                "a {} B line: every receiver drops the connection on it",
                line.len()
            );
            let (Some(Msg::Snap(st)) | Some(Msg::Club(st))) = decode_line(line) else {
                panic!("a club-state line: {line}");
            };
            if st.reset {
                m.dupes.clear();
                m.dkeys.clear();
                m.sections.clear();
            }
            m.dupes.extend(st.dupes);
            m.dkeys.extend(st.dkeys);
            m.sections.extend(st.sections);
            m.score = st.score;
            m.qsos = st.qsos;
            if !st.board.is_empty() || st.reset {
                m.board = st.board;
            }
        }
        m
    }

    /// The board a mirror would hold after `lines`, or `None` when a line is past the cap.
    fn board_read(lines: &[String]) -> Option<Vec<WireBoardRow>> {
        lines
            .iter()
            .all(|l| l.len() <= MAX_LINE_BYTES)
            .then(|| mirror_of(lines).board)
    }

    /// ⭐ **A club bigger than the old first line could carry still reaches every
    /// position whole** — every line inside the cap, the board on the first line only,
    /// and the mirror holding exactly what the host sent.
    ///
    /// The join snapshot's first line used to carry the whole board, every section and
    /// 50 key pairs, so a club of 40 positions wrote a first line past 8 KB, and every
    /// position dropped the connection on it and reconnected into the same line, for ever.
    #[test]
    fn a_big_clubs_join_snapshot_keeps_every_line_inside_the_cap() {
        for (rows, field_day) in [(40, true), (40, false)] {
            let st = big_club(rows, field_day);
            let lines = lines_of(st.clone(), true);
            for line in &lines {
                assert!(
                    line.len() <= MAX_LINE_BYTES,
                    "{rows} positions, field day {field_day}: a {} B line",
                    line.len()
                );
            }
            let m = mirror_of(&lines);
            assert_eq!(m.board, st.board, "the board arrived whole, in order");
            assert_eq!(m.dupes, st.dupes.iter().cloned().collect());
            assert_eq!(m.dkeys, st.dkeys.iter().cloned().collect());
            assert_eq!(m.sections, st.sections.iter().cloned().collect());
            assert_eq!((m.score, m.qsos), (st.score, st.qsos));
            for line in &lines[1..] {
                let Some(Msg::Snap(part)) = decode_line(line) else {
                    panic!("a snap chunk: {line}");
                };
                assert!(
                    part.board.is_empty() && !part.reset,
                    "only the first chunk carries the board and the reset"
                );
            }
        }
    }

    /// The other direction of the same property: a club that fits today writes the
    /// bytes it always wrote, every line of them, so nothing about a small club moves.
    #[test]
    fn a_club_that_fit_before_writes_the_bytes_it_always_wrote() {
        for (rows, field_day) in [(10, true), (10, false), (0, true)] {
            let st = big_club(rows, field_day);
            for snap in [true, false] {
                assert_eq!(
                    lines_of(st.clone(), snap),
                    old_lines(&st, snap),
                    "{rows} positions, field day {field_day}, snap {snap}"
                );
            }
        }
    }

    /// ⭐ **A board too long for any line leaves out the positions heard from least
    /// recently, and every line still fits** — rather than a line no position can read,
    /// which every position would drop and reconnect into: the board rides EVERY club
    /// line, the heartbeat's too, so a board that outgrew the line would take the whole
    /// club off the air at once. It cannot be split instead, because a mirror replaces
    /// its board with every chunk's.
    #[test]
    fn a_board_too_long_for_one_line_leaves_out_the_positions_heard_from_least_recently() {
        let mut st = big_club(0, true);
        // Ages that disagree with board order, so "freshest" and "first" cannot coincide.
        st.board = (0..70)
            .map(|i| long_row(i, (i as u64 * 37) % 70 + 10_000))
            .collect();
        for snap in [true, false] {
            let lines = lines_of(st.clone(), snap);
            let board = mirror_of(&lines).board;
            assert!(board.len() < 70, "70 rows cannot fit one line");
            assert!(board.len() > 40, "but most of them do: {}", board.len());
            let oldest_kept = board.iter().map(|r| r.age).max().unwrap();
            let left_out: Vec<&WireBoardRow> =
                st.board.iter().filter(|r| !board.contains(r)).collect();
            assert_eq!(left_out.len(), 70 - board.len());
            assert!(
                left_out.iter().all(|r| r.age > oldest_kept),
                "every row left out was heard from less recently than every row kept"
            );
            let kept_in_order: Vec<&WireBoardRow> =
                st.board.iter().filter(|r| board.contains(r)).collect();
            assert_eq!(
                board.iter().collect::<Vec<_>>(),
                kept_in_order,
                "the rows kept stay in the board's own order"
            );
        }
    }

    /// The ceilings, measured with the row sizes above: the most positions whose board
    /// still reaches every position whole, before this change and after. Printed for the
    /// record; the assertions hold the new floor and the old figures the record quotes.
    #[test]
    fn the_largest_club_whose_board_reaches_every_position_whole() {
        let ceiling = |field_day: bool, write: &dyn Fn(&ClubState) -> Vec<String>| {
            (1..=128)
                .take_while(|&n| {
                    let st = big_club(n, field_day);
                    board_read(&write(&st)).is_some_and(|b| b == st.board)
                })
                .last()
                .unwrap_or(0)
        };
        let old = |st: &ClubState| old_lines(st, true);
        let new = |st: &ClubState| lines_of(st.clone(), true);
        let (fd_old, party_old) = (ceiling(true, &old), ceiling(false, &old));
        let (fd_new, party_new) = (ceiling(true, &new), ceiling(false, &new));
        eprintln!(
            "ceiling: Field Day {fd_old} -> {fd_new} positions; party {party_old} -> {party_new}"
        );
        assert_eq!(
            (fd_old, party_old),
            (38, 37),
            "the old figures, with these rows and keys"
        );
        assert!(fd_new >= 50 && party_new >= 50, "{fd_new} / {party_new}");
        // The heartbeat carries the board too, and the same rows must fit there.
        let st = big_club(fd_new, true);
        assert_eq!(
            board_read(&lines_of(st.clone(), false)),
            Some(st.board),
            "the heartbeat carries the same whole board"
        );
    }

    /// ⭐ **A position sent a line it cannot read says so, instead of reconnecting into it
    /// in silence.** An older host with a big club writes a first line past the cap; the
    /// connection drops on it, the position reconnects, and the same line comes again.
    /// Nothing on screen used to say why the chip never settled.
    #[test]
    fn a_position_sent_a_line_past_the_cap_says_why() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral bind");
        let addr = listener.local_addr().unwrap().to_string();
        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr, pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));
        let (s, _) = listener.accept().unwrap();
        let mut w = s.try_clone().unwrap();
        w.write_all(
            encode_line(&Msg::Welcome {
                v: PROTO_VERSION,
                event: "TEST FD".into(),
                host_call: "W9ABC".into(),
                acked: 3,
                now_unix: now_unix(),
                contest: "ilqp".into(),
            })
            .as_bytes(),
        )
        .unwrap();
        // CONTROL: a line exactly AT the cap is read — the boundary, from the inside.
        let at_cap = |qsos: u64, len: usize| {
            let mut st = ClubState {
                qsos,
                sections: vec![String::new()],
                ..Default::default()
            };
            let pad = len - encode_line(&Msg::Club(st.clone())).len();
            st.sections[0] = "X".repeat(pad);
            encode_line(&Msg::Club(st))
        };
        let fits = at_cap(7, MAX_LINE_BYTES);
        assert_eq!(fits.len(), MAX_LINE_BYTES);
        w.write_all(fits.as_bytes()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && *posn.club_qsos.lock().unwrap() != 7 {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            *posn.club_qsos.lock().unwrap(),
            7,
            "a line at the cap is read"
        );
        assert!(posn.errors.lock().unwrap().is_empty());
        // …and one byte past it is the one no release reads.
        w.write_all(at_cap(8, MAX_LINE_BYTES + 1).as_bytes())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && posn.errors.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(listener);
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        assert_eq!(
            posn.errors.lock().unwrap().first().map(String::as_str),
            Some(LINE_PAST_THE_CAP),
            "the position says why it cannot sync"
        );
        assert_eq!(
            *posn.club_qsos.lock().unwrap(),
            7,
            "the over-long line was not read"
        );
    }
}
