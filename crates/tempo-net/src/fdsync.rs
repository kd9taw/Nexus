//! Nexus↔Nexus Field Day club sync — the wire layer: NDJSON over one
//! persistent TCP connection per position, plus a UDP discovery beacon.
//!
//! One club "position" (a Nexus instance) holds one TCP connection to the host
//! and streams `(position id, seq)`-identified QSO rows up; the host merges
//! them idempotently and pushes compact club state (dupe keys, sections, the
//! multiplier values worked, score, band board) back down the same socket.
//! The host's own contacts enter through an identical loopback connection, so
//! a host is just another position. This module owns ONLY the wire: framing,
//! the message vocabulary, the accept loop and the position-side pump. Policy
//! (the club log, the journal, dupe semantics) lives behind the
//! [`ClubBackend`] / [`PositionSync`] traits in the caller — the aprsis split.
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
//! Pinned by `the_inbound_surface_is_data_plane_only` below. Those rows go in only as
//! the position the peer's connection JOINED as, never as another position, whose
//! sequence numbers they would take (pinned by
//! `a_row_under_another_positions_id_is_refused_and_merges_nothing`), and what they
//! hold is the policy layer's to check before the club's files see it.
//!
//! ⭐ **A JOIN proves the position it names** (v3): it carries that position's club key
//! ([`PositionKey`]), and the backend holds every later JOIN under that position id to the key
//! it pinned at the first ([`ClubBackend::join`]). Position ids are on every board line; the
//! key is on none. This module takes the key from the JOIN line to that one call and nowhere
//! else, and never logs or prints it: its `Debug` shows none of it.
//!
//! ⭐ **A served JOIN holds its position until the host gives it to another laptop**: the
//! backend answers each JOIN with a hold ([`JoinAccept::hold`], never on the wire), and the loop
//! hands it back with every row and at every tick. Once the host's operator has given the
//! position to the laptop it belongs to, the backend refuses the old hold, and the loop sends
//! the refusal and closes, so nothing that connection sends afterwards is merged or acked. Each
//! served JOIN has a link as well ([`JoinAccept::link`]), handed back once its connection ends
//! ([`ClubBackend::disconnect`]), so the backend knows which positions a laptop is connected as.
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
//! Lines are capped at [`MAX_LINE_BYTES`].
//!
//! ⭐ **A position takes back its OWN contacts** (v4): `retract` and `restore` name one of its
//! rows by seq, only on the connection that JOINED as it, and the host answers each with
//! `retracted`/`restored` and, after every welcome, one `retracted` line per row of that
//! position it holds taken back, before the snap. The host keeps a taken-back row (a
//! tombstone, never a deleted row) and every connection gets a fresh snap
//! ([`ClubBackend::counts`]'s generation), so every position's club view drops it.
//!
//! ⭐ **Every laptop lists the club's contacts** (v5): the host sends each connection the club
//! log as its own screen lists it ([`ClubListView`]: the newest contacts, newest first, and how
//! many the club holds) on `rows` lines ([`ClubRows`]), the whole list after every welcome and
//! from then on only what changed ([`ListSent`]). A laptop keeps no more than that list, so its
//! memory is the same at a club's ten-thousandth contact as at its first. View only: the lines
//! carry nothing a position can send back.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Protocol version, carried in `join`/`welcome`/`beacon`. A host refuses a
/// JOIN with a higher version (the joiner is newer — it knows things we
/// don't); same-or-lower joins reach the policy layer, with unknown fields ignored.
///
/// **v5 lists the club's contacts on every laptop** ([`Msg::Rows`]). It went to 5 because a v4
/// position ignores a `rows` line, so its operator would look at a club board with no club log
/// under it, and a v5 position at a v4 host would wait for one that never comes: a v5 position
/// joining an older host is refused there by that host's own version check, verbatim, and an
/// older position is refused by name at a v5 host (the policy layer's).
///
/// **v4 takes a contact back** ([`Msg::Retract`], [`Msg::Restore`]). It went to 4 because the
/// addition is not harmless either way: a v3 host reads a `retract` and ignores it, so a v4
/// position's take-back would stay in that club's Cabrillo with nothing on any screen. So a v4
/// position joining a v3 (or v2) host is refused there by that host's own version check,
/// verbatim, and an older position is refused by name at a v4 host (the policy layer's, which
/// says to update Nexus on that laptop): one build on every laptop.
///
/// **v3 proves the position** ([`PositionKey`] on the JOIN). It went to 3 so that neither
/// direction is quiet: a v3 position joining a v2 host is refused there by that host's own
/// version check, verbatim, and a v2 position, which has no key to send, is refused by name at
/// a v3 host (the policy layer's, which says to update Nexus on that laptop). Served instead,
/// any peer could join under another position's id by claiming to be older.
///
/// **v2 carries the exchange as data** ([`WireQso::ex`]/[`WireQso::mex`],
/// [`ClubState::dkeys`]). It went to 2 so the new→old direction stays loud: a v2
/// position joining a v1 host is refused there, verbatim, at hour zero. Until v3 the
/// old→new direction was served — every v2 field is `#[serde(default)]` and the
/// legacy `class`/`sect` pair is still read — EXCEPT when the host was running a
/// contest a v1 position cannot enter, show or send (`ClubBackend::join` takes the
/// joiner's `v` for that decision, and for v3's).
pub const PROTO_VERSION: u32 = 5;
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
/// What the host answers a contact sent under another position than the one its connection
/// JOINED as, before it closes that connection: every JOIN gate decided on that position, so a
/// row naming another is refused rather than merged into a log no gate let it reach. A Nexus
/// whose position id changed while it was connected rejoins under the new one.
pub const ANOTHER_POSITIONS_ROW: &str = "this Nexus sent the host a contact as another club \
     position than the one it joined as, and the host takes each position's contacts only on \
     its own connection. It rejoins by itself; contacts you log meanwhile stay in your own log.";

/// ⭐ **A position's club key**: the secret a Nexus makes once for itself (32 random bytes,
/// as 64 lower-case hex digits) and sends on every JOIN, so a host can tell the laptop that
/// first joined as a position from any other naming the same position id. A host keeps only
/// its hash (`ClubBackend::join`).
///
/// ⛔ It is a credential: never logged, printed or shown, and on no board line. So this type
/// has no `Display`, its `Debug` prints none of it, and the one way to read it is
/// [`secret`](Self::secret), for the host's hash and nothing else. On the wire it is a plain
/// string (`serde(transparent)`), and an empty one is never written.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PositionKey(String);

impl PositionKey {
    /// A key from its secret: 64 hex digits from a Nexus, anything at all from a peer that
    /// is not one ([`is_club_key`](Self::is_club_key)).
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Is this a key a Nexus makes: exactly 64 characters of `0` to `9` and `a` to `f`?
    pub fn is_club_key(&self) -> bool {
        self.0.len() == 64
            && self
                .0
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    /// The key itself — for the hash a host pins and compares, and for nothing else.
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PositionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() {
            "PositionKey(none)"
        } else {
            "PositionKey(hidden)"
        })
    }
}

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
    /// ⭐ **The host's operator removed this position from the event** (`ClubLog::remove`). Its
    /// contacts stay in the club's log and on its line; every laptop's board says it was removed
    /// with the next club line, where it used to learn only from the line going quiet, a ⚠ 15 s
    /// (`DEAD_SECS`) and a heartbeat later.
    ///
    /// Additive, so no version bump: never written while false, which is every row an older
    /// host writes and every row but a removed position's, and an older position ignores it
    /// (its line goes quiet as before).
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
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
    /// ⭐ **The multiplier values newly worked club-wide**, `(slot, value)`: every value a
    /// multiplier board shows (an Illinois county, a state, an ARRL section) that a contact the
    /// club counts brought in, whichever position logged it — so every laptop's board lights it.
    ///
    /// Append-only like [`sections`](Self::sections), and past the DUPE cursor: a value first
    /// comes in with the contact that brings a new dupe key (`ClubLog`'s rule), so the delta that
    /// carries the key carries the value, and a take-back rebuilds both under a new generation.
    ///
    /// Additive, so no version bump in either direction: absent when empty, which is every line
    /// an older host writes, and a position reading none shows its own board as before; an older
    /// position ignores it, as it ignores every field it does not know.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mults: Vec<(String, String)>,
    /// Claimed club total (host's power multiplier + bonuses).
    #[serde(default)]
    pub score: u32,
    /// Merged club rows (raw, dupes included — honesty over flattery).
    #[serde(default)]
    pub qsos: u64,
    #[serde(default)]
    pub board: Vec<WireBoardRow>,
}

/// ⭐ **One contact of the club's log as every laptop lists it** (v5): the columns the host's own
/// screen draws, read there, and the position that logged it. `(pos, seq)` is its identity, and
/// `ord` the host's merge order, which breaks a tie between two contacts made in one second
/// exactly as the host's list breaks it. Short names, empty values left off: a `rows` line is
/// capped at [`MAX_LINE_BYTES`] like every other.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WireLogRow {
    pub pos: String,
    pub seq: u64,
    pub ord: u64,
    pub call: String,
    /// Field Day's two received slots.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sect: String,
    /// Any other contest's received values, one per slot of the club's session, in its order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rcvd: Vec<String>,
    pub band: String,
    /// Scoring class: "DIG" | "CW" | "PH".
    pub mode: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sub: String,
    /// The logging position's clock when it was logged.
    pub when: u64,
    /// The name the host's board shows the position under (`""` for none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The club's files hold an earlier contact of the same key: a duplicate.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dupe: bool,
}

impl WireLogRow {
    fn id(&self) -> (String, u64) {
        (self.pos.clone(), self.seq)
    }
}

/// ⭐ **The club log as the host's own screen lists it**, which every connection is sent
/// ([`ClubBackend::club_list`]): how many contacts the club holds, and the newest of them, newest
/// first. `stamp` changes whenever anything the list shows may have, so a connection sends
/// nothing while it stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClubListView {
    pub stamp: u64,
    pub total: u64,
    pub rows: Vec<WireLogRow>,
}

/// One `rows` line host→position (v5): what changed in the club list since the last. `reset`
/// (the first line after a welcome) empties the laptop's list first; `out` names rows that left
/// it, `put` rows that came in or changed. A list too long for one line goes over several, `out`
/// first, every one carrying the total.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ClubRows {
    #[serde(default, skip_serializing_if = "is_false")]
    pub reset: bool,
    #[serde(default)]
    pub total: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out: Vec<(String, u64)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub put: Vec<WireLogRow>,
}

/// ⭐ **The club list a laptop holds** — what the host's `rows` lines built ([`apply`](Self::apply)),
/// newest first, as the host's own screen lists it; never more than `cap` rows, whatever a host
/// sends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClubList {
    pub total: u64,
    pub rows: Vec<WireLogRow>,
}

impl ClubList {
    /// One `rows` line: `reset` empties the list first, then the rows `out` names leave, the rows
    /// `put` carries come in (replacing their earlier selves), and the total is the line's. Kept
    /// in the host's order — the time each was made, newest first, the later merge first within
    /// one second — and cut to `cap`.
    pub fn apply(&mut self, line: &ClubRows, cap: usize) {
        if line.reset {
            self.rows.clear();
        }
        let gone: std::collections::HashSet<(&str, u64)> =
            line.out.iter().map(|(p, s)| (p.as_str(), *s)).collect();
        let fresh: std::collections::HashSet<(&str, u64)> =
            line.put.iter().map(|r| (r.pos.as_str(), r.seq)).collect();
        self.rows.retain(|r| {
            let id = (r.pos.as_str(), r.seq);
            !gone.contains(&id) && !fresh.contains(&id)
        });
        self.rows.extend(line.put.iter().cloned());
        self.rows
            .sort_by_key(|r| std::cmp::Reverse((r.when, r.ord)));
        self.rows.truncate(cap);
        self.total = line.total;
    }
}

/// ⭐ **What one connection has been sent of the club list**, so each line after the first
/// carries only what changed: a new contact is one row in and, once the list is full, one out.
#[derive(Debug, Default)]
pub struct ListSent {
    stamp: Option<u64>,
    total: u64,
    rows: std::collections::HashMap<(String, u64), WireLogRow>,
}

impl ListSent {
    /// Write what `view` changes in the list this connection holds, as `rows` lines inside the
    /// cap; with `reset`, the whole list, its first line emptying the laptop's. Nothing while the
    /// view's stamp is the one last sent.
    pub fn write(
        &mut self,
        w: &mut impl Write,
        view: &ClubListView,
        reset: bool,
    ) -> std::io::Result<()> {
        if !reset && self.stamp == Some(view.stamp) {
            return Ok(());
        }
        if reset {
            self.rows.clear();
        }
        let now: std::collections::HashMap<(String, u64), &WireLogRow> =
            view.rows.iter().map(|r| (r.id(), r)).collect();
        let mut out: Vec<(String, u64)> = self
            .rows
            .keys()
            .filter(|id| !now.contains_key(*id))
            .cloned()
            .collect();
        out.sort();
        let put: Vec<&WireLogRow> = view
            .rows
            .iter()
            .filter(|r| self.rows.get(&r.id()) != Some(*r))
            .collect();
        if reset || !out.is_empty() || !put.is_empty() || view.total != self.total {
            write_club_rows(w, reset, view.total, &out, &put)?;
        }
        self.stamp = Some(view.stamp);
        self.total = view.total;
        self.rows = view.rows.iter().map(|r| (r.id(), r.clone())).collect();
        Ok(())
    }
}

/// `rows` lines for one change, each inside [`MAX_LINE_BYTES`]: `out` before `put`, so a laptop
/// that caps its list never drops a row for one that is leaving. An entry too long for a line of
/// its own is left out (only a hostile peer's row could make one). Measured off the encoder.
fn write_club_rows(
    w: &mut impl Write,
    reset: bool,
    total: u64,
    out: &[(String, u64)],
    put: &[&WireLogRow],
) -> std::io::Result<()> {
    let fresh = |reset: bool| ClubRows {
        reset,
        total,
        ..Default::default()
    };
    let fits = |part: &ClubRows| encode_line(&Msg::Rows(part.clone())).len() <= MAX_LINE_BYTES;
    // Push one entry (`Ok` a row leaving, `Err` a row coming in) onto a line; pop it again.
    let add = |part: &mut ClubRows, item: Result<&(String, u64), &WireLogRow>| match item {
        Ok(id) => part.out.push(id.clone()),
        Err(r) => part.put.push(r.clone()),
    };
    let undo = |part: &mut ClubRows, item: Result<&(String, u64), &WireLogRow>| {
        if item.is_ok() {
            part.out.pop();
        } else {
            part.put.pop();
        }
    };
    let mut part = fresh(reset);
    let mut lines = String::new();
    for item in out.iter().map(Ok).chain(put.iter().map(|r| Err(*r))) {
        add(&mut part, item);
        if fits(&part) {
            continue;
        }
        undo(&mut part, item);
        if part.out.is_empty() && part.put.is_empty() {
            continue; // too long even alone
        }
        let full = std::mem::replace(&mut part, fresh(false));
        lines.push_str(&encode_line(&Msg::Rows(full)));
        add(&mut part, item);
        if !fits(&part) {
            undo(&mut part, item);
        }
    }
    lines.push_str(&encode_line(&Msg::Rows(part)));
    w.write_all(lines.as_bytes())
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
        /// The position's station call, its Settings' "Callsign on the air", sent by every
        /// release since club sync began. The host refuses one that is not its own, by name,
        /// except at ARRL Field Day (whose GOTA station must use another), and serves an
        /// empty one as before.
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
        /// ⭐ **This position's club key** ([`PositionKey`]), on every v3 JOIN: the host pins its
        /// hash at the position's first JOIN of an event and refuses, by name, a later JOIN
        /// under that position id with any other. Absent from an older position's JOIN, which
        /// the host refuses by its version; never written empty. An older host ignores it.
        #[serde(default, skip_serializing_if = "PositionKey::is_empty")]
        key: PositionKey,
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
    /// pos→host (v4): take back this position's own row `seq` — a tombstone at the host, never
    /// a deleted row, kept whether or not the row has reached it yet. Only on the connection
    /// that JOINED as `pos`; the host answers [`Msg::Retracted`]. (Reserved and ignored by v1 to
    /// v3, which is why sending it needed v4.)
    Retract { pos: String, seq: u64 },
    /// pos→host (v4): put this position's own row `seq` back, lifting the tombstone. The position
    /// sends the row itself first, so a host that never had it merges it. Answered with
    /// [`Msg::Restored`].
    Restore { pos: String, seq: u64 },
    /// host→pos (v4): the club log holds this position's row `seq` taken back — the answer to a
    /// `retract`, and, after every welcome and before the snap, one line per row of the
    /// position the host holds taken back.
    Retracted { seq: u64 },
    /// host→pos (v4): the club log holds this position's row `seq` back — the answer to a
    /// `restore`.
    Restored { seq: u64 },
    /// host→pos (v5): the club list as the host's own screen lists it — the whole of it after
    /// every welcome (after the snap), and then what changed ([`ListSent`]).
    Rows(ClubRows),
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
    /// ⭐ **This JOIN's hold on its position**, the backend's own number: never on the wire,
    /// handed back with every row ([`ClubBackend::merge`]) and at every tick
    /// ([`ClubBackend::held`]). A host can give a position to another laptop, and a connection
    /// whose hold is older than that sends nothing more as it.
    pub hold: u64,
    /// ⭐ **This JOIN's link**, the backend's own number for the open connection it came in on:
    /// never on the wire, and handed back once, when that connection closes or JOINs again
    /// ([`ClubBackend::disconnect`]), so the backend always knows which positions a laptop is
    /// connected as.
    pub link: u64,
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
    ///
    /// ⭐ `key` is the JOIN's [`PositionKey`] (empty from an older position). The backend
    /// holds a JOIN under a position id it has seen to the key that position first joined
    /// with, and keeps no more of it than its hash: the key reaches this call and no other.
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
        key: &PositionKey,
    ) -> Result<JoinAccept, String>;
    /// Merge one row into the club log (idempotent on `(pos, seq)`); returns
    /// the new high-water ack for `row.pos`. `row.pos` is always the position this
    /// connection JOINED as, which every [`join`](Self::join) gate decided on: the socket
    /// loop refuses a row naming any other ([`ANOTHER_POSITIONS_ROW`]).
    ///
    /// `hold` is that JOIN's ([`JoinAccept::hold`]). `Err(msg)` = the host has given the
    /// position to another laptop since: the row is not merged or acked, `msg` is sent
    /// verbatim, and the connection closes. Decided under the same lock as the give, so no
    /// row this connection sends after it reaches the club log.
    fn merge(&self, hold: u64, row: &WireQso) -> Result<u64, String>;
    /// Does this connection still hold the position it JOINED as (`hold`, as for
    /// [`merge`](Self::merge))? `Err(msg)` once the host gave it to another laptop: sent
    /// verbatim, and the connection closes. Asked at every tick, so a connection that sends
    /// no row is closed too.
    fn held(&self, pos: &str, hold: u64) -> Result<(), String>;
    /// A position's presence report (band board fodder). `report.name` is the
    /// position's current friendly name — EMPTY MEANS "no news" (an older
    /// peer sends none), never "clear the label".
    fn position_status(&self, pos: &str, report: &PosReport);
    /// Cheap change detector: (dupe keys total, sections total, generation), read together.
    /// Within one generation both lists are append-only, so "count grew" == "there is a delta
    /// to send". A new generation means they were rebuilt (a contact taken back or put back, or
    /// a position given to another laptop), and every connection is sent a fresh snap: a
    /// position's mirror only unions a delta.
    fn counts(&self) -> (usize, usize, u64);
    /// Club state past the given cursors (`0, 0` = the full join snapshot),
    /// with the current board. `mark_seen` names the asking position so the
    /// host can stamp its last-seen (stale board rows are marked, not hidden).
    fn club_state(&self, dupes_from: usize, sections_from: usize, mark_seen: &str) -> ClubState;
    /// The connection the JOIN served with `link` ([`JoinAccept::link`]) came in on has closed,
    /// or JOINed again. Called once per served JOIN, on every way out of the socket loop, so a
    /// backend that counts the laptops connected as a position never counts one that has gone.
    fn disconnect(&self, pos: &str, link: u64);
    /// ⭐ **Take back `pos`'s own row `seq`** — a tombstone the club log keeps (the row stays,
    /// out of every count, key and file), whether or not the row has arrived. `pos` is always
    /// the position this connection JOINED as: the socket loop refuses a take-back naming any
    /// other ([`ANOTHER_POSITIONS_ROW`]), so a position can take back only its own. `hold` and
    /// `Err` as for [`merge`](Self::merge). Idempotent: a second take-back changes nothing.
    fn retract(&self, hold: u64, pos: &str, seq: u64) -> Result<(), String>;
    /// Put `pos`'s own row `seq` back, lifting its tombstone; as [`retract`](Self::retract).
    fn restore(&self, hold: u64, pos: &str, seq: u64) -> Result<(), String>;
    /// The seqs of `pos`'s rows the club log holds taken back, ascending — sent to it after
    /// every welcome, so a position that put one back while away knows to say so.
    fn retracted(&self, pos: &str) -> Vec<u64>;
    /// ⭐ The club list as the host's own screen lists it ([`ClubListView`]), asked at every tick:
    /// cheap while its stamp stands, so the backend builds it once per change, not once per
    /// connection.
    fn club_list(&self) -> Arc<ClubListView>;
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

/// `skip_serializing_if` for a board row's flags: a row without one is the bytes it always was.
fn is_false(v: &bool) -> bool {
    !*v
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
/// ([`board_fit`]). The key lists, the sections and then the multiplier values fill that
/// line and as many more as it takes, each line taking what fits, at most
/// [`SNAP_DUPES_PER_LINE`] key pairs a line. Every mirror since club sync shipped (1.10)
/// unions its lists and clears them only on a snapshot's first chunk, so where an entry
/// lands changes no state — and a club that fit before, with no multiplier values to send,
/// writes exactly the bytes it always did. A line an older position reads with only
/// multiplier values on it changes nothing there: its scalars are the ones it already holds.
/// The first line used to carry every section whatever its size, which put a Field Day club
/// of about 38 positions, or a QSO party that had worked every QTH, past the cap: each
/// position dropped the connection on that line and reconnected into the same one.
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
        mults,
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
    // The multiplier list is left off a line while it is empty, so its first value brings the
    // key with it: what that costs, measured off the encoder rather than counted by hand.
    let mults_key = line(ClubState {
        mults: vec![Default::default()],
        ..Default::default()
    })
    .len()
        - line(ClubState::default()).len()
        - grows_by(&<(String, String)>::default(), true);
    let mults: Vec<&(String, String)> = mults
        .iter()
        .filter(|m| fits_alone(mults_key + grows_by(m, true)))
        .collect();
    let board = board_fit(&board).rows;
    let (mut next_pair, mut next_section, mut next_mult) = (0usize, 0usize, 0usize);
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
        while let Some(m) = mults.get(next_mult) {
            let grow = if part.mults.is_empty() {
                mults_key + grows_by(m, true)
            } else {
                grows_by(m, false)
            };
            if used + grow > MAX_LINE_BYTES {
                break;
            }
            part.mults.push((*m).clone());
            used += grow;
            next_mult += 1;
        }
        w.write_all(line(part).as_bytes())?;
        first = false;
        if next_pair >= pairs.len() && next_section >= sections.len() && next_mult >= mults.len() {
            return Ok(());
        }
    }
}

/// The position a host-side connection JOINED as, that JOIN's hold on it and its link. Dropping
/// it tells the backend the link is gone ([`ClubBackend::disconnect`]), so every way out of the
/// socket loop does, a second JOIN on the same connection included.
struct Joined {
    backend: Arc<dyn ClubBackend>,
    pos: String,
    hold: u64,
    link: u64,
}

impl Drop for Joined {
    fn drop(&mut self) {
        self.backend.disconnect(&self.pos, self.link);
    }
}

/// A position's take-back (`gone`) or its undo of row `seq`, answered on its connection
/// (`retracted`/`restored`). Before a JOIN it reaches nothing; naming another position than the
/// one JOINed as, or once the host gave the position away, it is refused and `false` closes the
/// connection, as for a row.
fn answer_mark(
    w: &mut impl Write,
    backend: &Arc<dyn ClubBackend>,
    joined: Option<&Joined>,
    gone: bool,
    pos: &str,
    seq: u64,
) -> bool {
    let Some(j) = joined else {
        return true;
    };
    let answer = if j.pos != pos {
        Err(ANOTHER_POSITIONS_ROW.to_string())
    } else if gone {
        backend
            .retract(j.hold, pos, seq)
            .map(|()| Msg::Retracted { seq })
    } else {
        backend
            .restore(j.hold, pos, seq)
            .map(|()| Msg::Restored { seq })
    };
    match answer {
        Ok(line) => w.write_all(encode_line(&line).as_bytes()).is_ok(),
        Err(msg) => {
            let _ = w.write_all(encode_line(&Msg::Error { msg }).as_bytes());
            false
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
    // The position this connection JOINED as, that JOIN's hold on it, and its link.
    let mut joined: Option<Joined> = None;
    let mut sent_dupes = 0usize;
    let mut sent_sections = 0usize;
    // The generation the cursors above belong to: a new one is a fresh snap.
    let mut sent_generation = 0u64;
    // What this connection has been sent of the club list.
    let mut list = ListSent::default();
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
                        key,
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
                        // A JOIN again on this connection: the one before lets go of its link
                        // first, whatever this one is answered.
                        drop(joined.take());
                        let accept = match backend
                            .join(v, &pos, &name, &call, max_seq, &contest, &role, &key)
                        {
                            Ok(a) => a,
                            Err(msg) => {
                                let _ =
                                    writer.write_all(encode_line(&Msg::Error { msg }).as_bytes());
                                break;
                            }
                        };
                        // Held from the moment the backend served it, so a welcome that cannot
                        // be written still lets go of the link.
                        joined = Some(Joined {
                            backend: Arc::clone(&backend),
                            pos: pos.clone(),
                            hold: accept.hold,
                            link: accept.link,
                        });
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
                        // Which of this position's rows the club holds taken back, before the
                        // snap, so the position has the host's whole view before it sends a mark.
                        let gone: String = backend
                            .retracted(&pos)
                            .into_iter()
                            .map(|seq| encode_line(&Msg::Retracted { seq }))
                            .collect();
                        if writer.write_all(gone.as_bytes()).is_err() {
                            break;
                        }
                        // Full snapshot: read the cursors BEFORE the state so a
                        // merge racing in between is re-sent, never skipped
                        // (the mirror dedups; a skipped key would stay lost).
                        let (d, s, g) = backend.counts();
                        let st = backend.club_state(0, 0, &pos);
                        if write_club_state(&mut writer, st, true).is_err() {
                            break;
                        }
                        sent_dupes = d;
                        sent_sections = s;
                        sent_generation = g;
                        // ⭐ The whole club list, its first line emptying the laptop's: a laptop
                        // back from away lists the club as it stands, not as it left it.
                        list = ListSent::default();
                        if list.write(&mut writer, &backend.club_list(), true).is_err() {
                            break;
                        }
                    }
                    // ⭐ Merged only as the position this connection JOINED as: every gate
                    // above decided on that JOIN, so a row naming another position is refused,
                    // never merged into a log no gate let it reach. And only while that JOIN
                    // still holds it: a position the host gave to another laptop takes no more.
                    Msg::Qso(row) => match &joined {
                        Some(j) if j.pos == row.pos => match backend.merge(j.hold, &row) {
                            Ok(acked) => {
                                if writer
                                    .write_all(encode_line(&Msg::Ack { seq: acked }).as_bytes())
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(msg) => {
                                let _ =
                                    writer.write_all(encode_line(&Msg::Error { msg }).as_bytes());
                                break;
                            }
                        },
                        Some(_) => {
                            let _ = writer.write_all(
                                encode_line(&Msg::Error {
                                    msg: ANOTHER_POSITIONS_ROW.to_string(),
                                })
                                .as_bytes(),
                            );
                            break;
                        }
                        None => {}
                    },
                    // ⭐ A take-back and its undo: only as the position this connection JOINED
                    // as, as a row is, and only while that JOIN holds it — so a laptop takes
                    // back its own contacts and nobody else's.
                    Msg::Retract { pos, seq } => {
                        if !answer_mark(&mut writer, &backend, joined.as_ref(), true, &pos, seq) {
                            break;
                        }
                    }
                    Msg::Restore { pos, seq } => {
                        if !answer_mark(&mut writer, &backend, joined.as_ref(), false, &pos, seq) {
                            break;
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
                        if let Some(j) = &joined {
                            backend.position_status(
                                &j.pos,
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
                    // Pong: freshness already noted via last_rx. Unknown is a newer
                    // peer's message; the rest are host→pos vocabulary a position
                    // should never send.
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
        // heartbeat every PING_SECS. First, whether this JOIN still holds its position: one the
        // host gave to another laptop is told why and closed within a tick, rows or none.
        if let Some(j) = &joined {
            if let Err(msg) = backend.held(&j.pos, j.hold) {
                let _ = writer.write_all(encode_line(&Msg::Error { msg }).as_bytes());
                break;
            }
            let (d, s, g) = backend.counts();
            let beat = last_beat.elapsed().as_secs() >= PING_SECS;
            if g != sent_generation {
                // The lists were rebuilt (a contact taken back or put back, or a position given
                // to another laptop): the cursors name nothing now, and a delta cannot take a
                // key away, so the whole state again.
                let st = backend.club_state(0, 0, &j.pos);
                if write_club_state(&mut writer, st, true).is_err() {
                    break;
                }
                (sent_dupes, sent_sections, sent_generation) = (d, s, g);
            } else if d > sent_dupes || s > sent_sections || beat {
                let st = backend.club_state(sent_dupes, sent_sections, &j.pos);
                if write_club_state(&mut writer, st, false).is_err() {
                    break;
                }
                sent_dupes = d;
                sent_sections = s;
            }
            // What changed in the club list: a contact in, one out, a duplicate marked.
            if list
                .write(&mut writer, &backend.club_list(), false)
                .is_err()
            {
                break;
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
    // `joined` drops here: the backend hears its link is gone.
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

/// One take-back fact a position owes its host ([`Outbox::marks`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    /// This position took back its own row `seq`, and the host does not hold it so.
    Retract { pos: String, seq: u64 },
    /// This position put its row back, and the host holds it taken back: the row itself, which
    /// the pump sends before the `restore` so a host that never had it merges it.
    Restore(Box<WireQso>),
}

/// ⭐ **What a position owes its host, read in one go**
/// ([`PositionSync::outbox_after`]): its take-back marks, then its rows. One read, so the two
/// cannot disagree about a contact taken back or put back between them, and the pump sends
/// the marks first: a host never acks a row past one this position took back before it
/// heard of the take-back, which is what keeps "every own live row past the ack" the whole
/// of what a host lacks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outbox {
    pub marks: Vec<Mark>,
    pub rows: Vec<WireQso>,
}

/// The position pump's view of the application. Mirrors [`ClubBackend`]'s
/// discipline: the pump can read the outbox and deliver club state, nothing
/// more.
pub trait PositionSync: Send + Sync {
    /// This position's identity for the JOIN line:
    /// `(pos id, friendly name, station call, own max seq)`.
    fn identity(&self) -> (String, String, String, u64);
    /// This position's club key for the JOIN line ([`PositionKey`]): what proves to the host
    /// that this laptop is the one that first joined as this position id.
    fn key(&self) -> PositionKey;
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
    /// Own live rows with `seq > after` (ascending), and every take-back the host's view
    /// ([`on_retracted`](Self::on_retracted)) differs from — THE outbox definition; no
    /// separate queue exists to corrupt.
    fn outbox_after(&self, after: u64) -> Outbox;
    /// The accepted welcome (host's ack high-water, event, host call, host
    /// clock — the caller warns on >30 s skew; nothing is ever adjusted).
    fn on_welcome(&self, acked: u64, event: &str, host_call: &str, now_unix: u64);
    fn on_ack(&self, seq: u64);
    /// The host holds own row `seq` taken back (`gone`) or back (`!gone`): a `retracted` or
    /// `restored` line. The view starts empty at each welcome.
    fn on_retracted(&self, seq: u64, gone: bool);
    /// A `snap` (full, `snap=true`) or `club` (delta) line: union the lists,
    /// overwrite the scalars.
    fn on_club(&self, snap: bool, st: &ClubState);
    /// A `rows` line (v5): what changed in the club list the host's screen shows.
    fn on_rows(&self, rows: &ClubRows);
    /// A host `error` line — shown to the operator verbatim.
    fn on_error(&self, msg: &str);
    /// Current presence for the band board — `(band, mode class, operator,
    /// dial Hz, friendly name)`; `None` = don't report this tick. The name
    /// rides along on every report so a rename propagates live.
    fn position_report(&self) -> Option<PosReport>;
    /// Link up/down transitions (drives the Offline/Behind/Synced chip).
    fn on_link(&self, connected: bool);
    /// One attempt to reach the host ended before a connection opened, and why. The joining
    /// laptop's screen says which, with what to check; the pump tries again on its backoff, as
    /// after any session.
    fn on_unreachable(&self, why: Unreachable);
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

/// Why a position's connection to its host did not open ([`PositionSync::on_unreachable`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreachable {
    /// The join address is not one to connect to: not `address:port`, or a name this laptop
    /// cannot find.
    BadAddress,
    /// The host's computer answered that nothing listens on that port: Nexus there is not
    /// hosting, or hosts on another port.
    Refused,
    /// Nothing answered: the connection timed out or found no route. The host is off or on
    /// another network, or its firewall dropped the connection, which is also how a Windows
    /// computer that is not hosting answers.
    NoAnswer,
}

impl Unreachable {
    /// The DTO string the screens switch on.
    pub fn code(self) -> &'static str {
        match self {
            Unreachable::BadAddress => "badAddress",
            Unreachable::Refused => "refused",
            Unreachable::NoAnswer => "noAnswer",
        }
    }

    /// What a failed connect tells the operator: refused, or no answer. Every other way a
    /// connect fails leaves the same things to check as a timeout does.
    pub fn of(e: &std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::ConnectionRefused {
            Unreachable::Refused
        } else {
            Unreachable::NoAnswer
        }
    }
}

/// How long a position waits for the host to answer its connection.
const CONNECT_SECS: u64 = 3;

/// The join address as a position connects to it, worked out without looking a name up: what
/// was typed, with the club's port ([`DEFAULT_TCP_PORT`]) after an address typed without one, so
/// "192.168.1.10" joins 192.168.1.10:42073. The screens show this, so the port a position tried
/// is in front of the operator when no host answers there.
pub fn join_target(typed: &str) -> String {
    let typed = typed.trim();
    match typed.parse::<std::net::IpAddr>() {
        Ok(ip) => std::net::SocketAddr::new(ip, DEFAULT_TCP_PORT).to_string(),
        Err(_) if !typed.is_empty() && !typed.contains(':') => {
            format!("{typed}:{DEFAULT_TCP_PORT}")
        }
        Err(_) => typed.to_string(),
    }
}

/// The address [`join_target`] names, looked up: `None` when it is not one to connect to.
pub fn join_address(typed: &str) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    join_target(typed).to_socket_addrs().ok()?.next()
}

/// One connected session: join → welcome → stream the gap → duplex pump.
/// Returns when the link dies or `shutdown` is set. `Ok(true)` = a welcome
/// was received (reset the reconnect backoff).
fn run_position_session(
    addr: &str,
    backend: &Arc<dyn PositionSync>,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<bool> {
    let Some(sock_addr) = join_address(addr) else {
        backend.on_unreachable(Unreachable::BadAddress);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a join address",
        ));
    };
    let stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(CONNECT_SECS))
        .inspect_err(|e| backend.on_unreachable(Unreachable::of(e)))?;
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
            key: backend.key(),
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
    // ⭐ The host's view of this position's take-backs is in once its first snap has come (it
    // lists them between the welcome and the snap): until then nothing of the log is sent, so
    // no mark is made against half a view and no row is acked past a take-back the host has
    // not heard of.
    let mut have_view = false;
    // Each mark sent this session, by seq (`true` = a take-back): sent again only when what
    // is owed changes, and every session starts afresh.
    let mut marked: std::collections::HashMap<u64, bool> = std::collections::HashMap::new();
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
                    Some(Msg::Snap(st)) => {
                        backend.on_club(true, &st);
                        have_view = welcomed;
                    }
                    Some(Msg::Club(st)) => backend.on_club(false, &st),
                    Some(Msg::Retracted { seq }) => backend.on_retracted(seq, true),
                    Some(Msg::Restored { seq }) => backend.on_retracted(seq, false),
                    Some(Msg::Rows(rows)) => backend.on_rows(&rows),
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
        if welcomed && have_view {
            // Stream the outbox: the take-backs the host lacks first, then everything past
            // what is both acked and already written this session (a fresh QSO extends it
            // live).
            let from = sent_to.max(acked);
            let Outbox { marks, rows } = backend.outbox_after(from);
            for mark in marks {
                match mark {
                    Mark::Retract { pos, seq } => {
                        if marked.insert(seq, true) != Some(true) {
                            writer.write_all(encode_line(&Msg::Retract { pos, seq }).as_bytes())?;
                        }
                    }
                    // The row first: a host that never had it merges it, kept out until the
                    // restore lifts its tombstone.
                    Mark::Restore(row) => {
                        if marked.insert(row.seq, false) != Some(false) {
                            let (pos, seq) = (row.pos.clone(), row.seq);
                            writer.write_all(encode_line(&Msg::Qso(*row)).as_bytes())?;
                            writer.write_all(encode_line(&Msg::Restore { pos, seq }).as_bytes())?;
                        }
                    }
                }
            }
            for row in rows {
                sent_to = sent_to.max(row.seq);
                writer.write_all(encode_line(&Msg::Qso(row)).as_bytes())?;
            }
        }
        if welcomed {
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

/// One IPv4 address this computer holds on a network, with the adapter it is on: what the
/// host's screen lists for a joiner to type, and the subnet its beacon is broadcast to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanAddr {
    /// The adapter's name as the system gives it ("Wi-Fi", "eth0"); empty when it gives none.
    pub adapter: String,
    pub ip: std::net::Ipv4Addr,
    /// The subnet's prefix length: 24 for a 255.255.255.0 netmask.
    pub prefix: u8,
}

impl LanAddr {
    /// The subnet's own broadcast address, `None` for a /31 or a /32: a point-to-point link or
    /// an address on its own (a VPN's) has none.
    pub fn broadcast(&self) -> Option<std::net::Ipv4Addr> {
        if self.prefix >= 31 {
            return None;
        }
        let mask = u32::MAX
            .checked_shl(32 - u32::from(self.prefix))
            .unwrap_or(0);
        Some(std::net::Ipv4Addr::from(u32::from(self.ip) | !mask))
    }
}

/// Every IPv4 address this computer holds on an adapter that is up, loopback left out, in the
/// order the system lists them. Empty when the system cannot list them.
pub fn lan_addrs() -> Vec<LanAddr> {
    system_lan_addrs()
}

/// [`lan_addrs`] from `getifaddrs`: adapters that are up and running (a cable unplugged leaves
/// an address on an adapter that reaches nobody).
#[cfg(unix)]
fn system_lan_addrs() -> Vec<LanAddr> {
    let mut out = Vec::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `head` with a list that is freed below, and nothing taken from
    // it outlives that: the name is copied and the addresses are read by value.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return out;
    }
    let wanted = (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_uint;
    let mut node = head;
    while !node.is_null() {
        // SAFETY: `node` is a node of the list getifaddrs gave, which is not freed yet.
        let ifa = unsafe { &*node };
        node = ifa.ifa_next;
        if ifa.ifa_flags & wanted != wanted
            || ifa.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint != 0
            || ifa.ifa_addr.is_null()
            || ifa.ifa_netmask.is_null()
            || ifa.ifa_name.is_null()
        {
            continue;
        }
        // SAFETY: a non-null `ifa_addr` points at a sockaddr whose family names its shape, and
        // for AF_INET it and the netmask beside it are sockaddr_in.
        let (ip, mask) = unsafe {
            if i32::from((*ifa.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            let ip = (*(ifa.ifa_addr as *const libc::sockaddr_in))
                .sin_addr
                .s_addr;
            let mask = (*(ifa.ifa_netmask as *const libc::sockaddr_in))
                .sin_addr
                .s_addr;
            (u32::from_be(ip), u32::from_be(mask))
        };
        // SAFETY: a non-null `ifa_name` is the adapter's NUL-terminated name.
        let adapter = unsafe { std::ffi::CStr::from_ptr(ifa.ifa_name) };
        out.push(LanAddr {
            adapter: adapter.to_string_lossy().into_owned(),
            ip: ip.into(),
            prefix: mask.leading_ones() as u8,
        });
    }
    // SAFETY: `head` came from getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(head) };
    out
}

/// [`lan_addrs`] from `GetAdaptersAddresses`: adapters that are up, by their own names ("Wi-Fi",
/// "Ethernet", "vEthernet (WSL)").
#[cfg(windows)]
fn system_lan_addrs() -> Vec<LanAddr> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    // iptypes.h and winerror.h; the crate's features carry neither.
    const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    // u64 words keep the buffer aligned for the structures written into it. The size asked
    // for comes back in `size` when the buffer is too small, and an adapter can come up
    // between two calls, so three tries.
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u64> = Vec::new();
    let mut listed = false;
    for _ in 0..3 {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: `buf` holds `size` bytes, aligned for IP_ADAPTER_ADDRESSES_LH.
        let rc = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET.0),
                flags,
                None,
                Some(buf.as_mut_ptr().cast()),
                &mut size,
            )
        };
        if rc == 0 {
            listed = true;
            break;
        }
        if rc != ERROR_BUFFER_OVERFLOW {
            break;
        }
    }
    let mut out = Vec::new();
    if !listed {
        return out;
    }
    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter.is_null() {
        // SAFETY: each node, and every pointer in it, lies in `buf`, which outlives the walk.
        let a = unsafe { &*adapter };
        adapter = a.Next;
        if a.OperStatus != IfOperStatusUp || a.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
            continue;
        }
        let name = if a.FriendlyName.is_null() {
            String::new()
        } else {
            // SAFETY: a non-null FriendlyName is a NUL-terminated wide string in `buf`.
            unsafe { a.FriendlyName.to_string() }.unwrap_or_default()
        };
        let mut unicast = a.FirstUnicastAddress;
        while !unicast.is_null() {
            // SAFETY: as above.
            let u = unsafe { &*unicast };
            unicast = u.Next;
            let sa = u.Address.lpSockaddr;
            // SAFETY: a non-null lpSockaddr is a sockaddr whose family names its shape.
            if sa.is_null() || unsafe { (*sa).sa_family } != AF_INET {
                continue;
            }
            // SAFETY: an AF_INET sockaddr is a SOCKADDR_IN.
            let b = unsafe { (*(sa as *const SOCKADDR_IN)).sin_addr.S_un.S_un_b };
            out.push(LanAddr {
                adapter: name.clone(),
                ip: std::net::Ipv4Addr::new(b.s_b1, b.s_b2, b.s_b3, b.s_b4),
                prefix: u.OnLinkPrefixLength,
            });
        }
    }
    out
}

/// No adapter list on a system with neither API: the beacon goes where it always did.
#[cfg(not(any(unix, windows)))]
fn system_lan_addrs() -> Vec<LanAddr> {
    Vec::new()
}

/// Where one beacon goes: the limited broadcast, 255.255.255.255, which leaves a computer by one
/// adapter only, and then the broadcast address of each subnet in `lan`, which leaves by the
/// adapter on that subnet. So a host with Wi-Fi, a cable and the virtual adapters WSL, Hyper-V
/// or a VPN add is heard on every network it is on. Each address once, in `lan`'s order.
pub fn beacon_targets(lan: &[LanAddr]) -> Vec<std::net::Ipv4Addr> {
    let mut out = vec![std::net::Ipv4Addr::BROADCAST];
    for b in lan.iter().filter_map(LanAddr::broadcast) {
        if !out.contains(&b) {
            out.push(b);
        }
    }
    out
}

/// Send one beacon to each of `targets` ([`beacon_targets`]), best-effort: AP-isolated Wi-Fi
/// eats broadcast, which is why typing the host's address always remains.
pub fn send_beacon(
    sock: &UdpSocket,
    event: &str,
    call: &str,
    port: u16,
    targets: &[std::net::Ipv4Addr],
) {
    send_beacon_to(sock, &beacon_line(event, call, port), targets, BEACON_PORT);
}

/// [`send_beacon`]'s datagrams, to `beacon_port` (a test's own port; the club's is
/// [`BEACON_PORT`]).
fn send_beacon_to(sock: &UdpSocket, line: &str, targets: &[std::net::Ipv4Addr], beacon_port: u16) {
    for ip in targets {
        let _ = sock.send_to(line.as_bytes(), (*ip, beacon_port));
    }
}

/// How long "Find club events" listens: five of a host's once-a-second beacons, so a datagram or
/// two lost on busy Wi-Fi still finds it.
pub const DISCOVER_SECS: u64 = 5;

/// Listen for host beacons for up to `secs` and return every distinct event
/// heard — the "Find club events" button. `SO_REUSEADDR` so two Nexus
/// instances on one box can both listen (the flexdisc socket shape).
pub fn discover(secs: u64) -> std::io::Result<Vec<BeaconInfo>> {
    discover_on(BEACON_PORT, secs)
}

/// [`discover`] on `beacon_port` (a test's own port; the club's is [`BEACON_PORT`]).
fn discover_on(beacon_port: u16, secs: u64) -> std::io::Result<Vec<BeaconInfo>> {
    let raw = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    raw.set_reuse_address(true)?;
    let addr: std::net::SocketAddr = ([0, 0, 0, 0], beacon_port).into();
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

    /// The most rows a laptop's list holds in these tests, as the app's own cap does.
    const LIST_CAP: usize = 100;

    fn list_row(pos: &str, seq: u64, when: u64, ord: u64) -> WireLogRow {
        WireLogRow {
            pos: pos.into(),
            seq,
            ord,
            call: format!("K{seq}ABC"),
            rcvd: vec!["599".into(), "COOK".into()],
            band: "20m".into(),
            mode: "CW".into(),
            when,
            name: format!("{pos} tent"),
            ..Default::default()
        }
    }

    /// Every `rows` line `bytes` holds, each checked against the cap.
    fn rows_lines(bytes: &[u8]) -> Vec<ClubRows> {
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .lines()
            .map(|l| {
                assert!(l.len() < MAX_LINE_BYTES, "a line past the cap: {}", l.len());
                match decode_line(l) {
                    Some(Msg::Rows(r)) => r,
                    other => panic!("not a rows line: {other:?}"),
                }
            })
            .collect()
    }

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
                key: PositionKey::default(),
            },
            // A v3 position's JOIN: the key travels as a plain string.
            Msg::Join {
                v: PROTO_VERSION,
                pos: "a1b2c3d4".into(),
                name: "CW tent".into(),
                call: "W9ABC".into(),
                max_seq: 42,
                contest: "arrlfd".into(),
                role: String::new(),
                key: throwaway_key(),
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
                mults: vec![("SECTION".into(), "CT".into())],
                score: 1234,
                qsos: 312,
                board: vec![
                    WireBoardRow {
                        pos: "a1b2c3d4".into(),
                        name: "CW tent".into(),
                        band: "20m".into(),
                        mode: "CW".into(),
                        op: "KD9TAW".into(),
                        qsos: 57,
                        uniq: 55,
                        rate: 23,
                        age: 2,
                        removed: false,
                    },
                    WireBoardRow {
                        pos: "b1b2c3d4".into(),
                        name: "SSB tent".into(),
                        qsos: 9,
                        age: 40,
                        removed: true,
                        ..Default::default()
                    },
                ],
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
            Msg::Restore {
                pos: "a1b2c3d4".into(),
                seq: 7,
            },
            Msg::Retracted { seq: 7 },
            Msg::Restored { seq: 7 },
            Msg::Rows(ClubRows {
                reset: true,
                total: 12,
                out: vec![("bbbb0002".into(), 3)],
                put: vec![list_row("cccc0003", 4, 1_782_583_500, 9)],
            }),
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
            key: PositionKey::default(),
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
        // The take-back vocabulary, as the bytes go out.
        assert_eq!(
            encode_line(&Msg::Retract {
                pos: "a1b2c3d4".into(),
                seq: 7
            }),
            "{\"t\":\"retract\",\"pos\":\"a1b2c3d4\",\"seq\":7}\n"
        );
        assert_eq!(
            encode_line(&Msg::Restore {
                pos: "a1b2c3d4".into(),
                seq: 7
            }),
            "{\"t\":\"restore\",\"pos\":\"a1b2c3d4\",\"seq\":7}\n"
        );
        assert_eq!(
            encode_line(&Msg::Retracted { seq: 7 }),
            "{\"t\":\"retracted\",\"seq\":7}\n"
        );
        assert_eq!(
            encode_line(&Msg::Restored { seq: 7 }),
            "{\"t\":\"restored\",\"seq\":7}\n"
        );
    }

    #[test]
    fn unknown_types_and_fields_are_ignored_not_errors() {
        // A NEWER peer's message type parses as Unknown (ignored)…
        assert!(matches!(
            decode_line(r#"{"t":"hologram","x":1}"#),
            Some(Msg::Unknown)
        ));
        // …a take-back parses (a host acts on it only from the position that sent it)…
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
        refuse_older: Mutex<Option<String>>,
        /// The contest each JOIN named, in arrival order — what reached the policy layer.
        contests: Mutex<Vec<String>>,
        /// The key each JOIN carried, in arrival order (throwaway values, test-only).
        keys: Mutex<Vec<String>>,
        /// The role each JOIN named, in arrival order.
        roles: Mutex<Vec<String>>,
        /// Each presence report's clock, in arrival order.
        clocks: Mutex<Vec<(String, Option<i64>)>>,
        /// The hold the next JOIN is served with, and the only one `held` accepts: moving it
        /// is the host giving the position away.
        hold: Mutex<u64>,
        /// The last link a served JOIN was given; each is one more.
        links: Mutex<u64>,
        /// The rows taken back, by `(pos, seq)`.
        gone: Mutex<std::collections::HashSet<(String, u64)>>,
        /// What `counts` says the generation is: moving it is the club rebuilding its lists.
        generation: Mutex<u64>,
        /// The club list `club_list` answers: changing it (and its stamp) is the club's list
        /// moving.
        list: Mutex<ClubListView>,
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
            key: &PositionKey,
        ) -> Result<JoinAccept, String> {
            self.log(format!("join v{v} {pos}"));
            self.contests.lock().unwrap().push(contest.to_string());
            self.roles.lock().unwrap().push(role.to_string());
            self.keys.lock().unwrap().push(key.secret().to_string());
            // The guard is dropped before the branch, not held across it: an
            // `if let` scrutinee lives until the end of the body, which is how a
            // lock taken here would still be held while the arm runs.
            let refusal = self.refuse_older.lock().unwrap().clone();
            if v < PROTO_VERSION {
                if let Some(msg) = refusal {
                    return Err(msg);
                }
            }
            let link = {
                let mut last = self.links.lock().unwrap();
                *last += 1;
                *last
            };
            Ok(JoinAccept {
                event: "TEST FD".into(),
                host_call: "W9ABC".into(),
                acked: *self.acked.lock().unwrap().get(pos).unwrap_or(&0),
                contest: "arrlfd".into(),
                hold: *self.hold.lock().unwrap(),
                link,
            })
        }
        fn merge(&self, hold: u64, row: &WireQso) -> Result<u64, String> {
            self.held(&row.pos, hold)?;
            self.log(format!("merge {} {}", row.pos, row.seq));
            self.merged
                .lock()
                .unwrap()
                .entry((row.pos.clone(), row.seq))
                .or_insert_with(|| row.clone());
            let mut acked = self.acked.lock().unwrap();
            let e = acked.entry(row.pos.clone()).or_insert(0);
            *e = (*e).max(row.seq);
            Ok(*e)
        }
        fn held(&self, _pos: &str, hold: u64) -> Result<(), String> {
            if hold == *self.hold.lock().unwrap() {
                Ok(())
            } else {
                Err(format!("given away {hold}"))
            }
        }
        fn position_status(&self, pos: &str, r: &PosReport) {
            self.log(format!("pos {pos} {} name={}", r.band, r.name));
            self.clocks
                .lock()
                .unwrap()
                .push((pos.to_string(), r.clock_ms));
        }
        fn counts(&self) -> (usize, usize, u64) {
            (
                self.merged.lock().unwrap().len(),
                0,
                *self.generation.lock().unwrap(),
            )
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
                mults: Vec::new(),
                score: 0,
                qsos: m.len() as u64,
                board: Vec::new(),
            }
        }
        fn disconnect(&self, pos: &str, link: u64) {
            self.log(format!("disconnect {pos} {link}"));
        }
        fn retract(&self, hold: u64, pos: &str, seq: u64) -> Result<(), String> {
            self.held(pos, hold)?;
            self.log(format!("retract {pos} {seq}"));
            self.gone.lock().unwrap().insert((pos.to_string(), seq));
            Ok(())
        }
        fn restore(&self, hold: u64, pos: &str, seq: u64) -> Result<(), String> {
            self.held(pos, hold)?;
            self.log(format!("restore {pos} {seq}"));
            self.gone.lock().unwrap().remove(&(pos.to_string(), seq));
            Ok(())
        }
        fn retracted(&self, pos: &str) -> Vec<u64> {
            let gone = self.gone.lock().unwrap();
            let mut seqs: Vec<u64> = gone
                .iter()
                .filter(|(p, _)| p == pos)
                .map(|(_, s)| *s)
                .collect();
            seqs.sort_unstable();
            seqs
        }
        fn club_list(&self) -> Arc<ClubListView> {
            Arc::new(self.list.lock().unwrap().clone())
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
            key: throwaway_key(),
        }
    }

    /// A key for one test, never a real one: 64 hex digits from the operating system's
    /// randomly seeded hasher, new on every run.
    fn throwaway_key() -> PositionKey {
        use std::hash::BuildHasher;
        let seeded = std::hash::RandomState::new();
        PositionKey::new(
            (0..4u8)
                .map(|i| format!("{:016x}", seeded.hash_one(i)))
                .collect(),
        )
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

    /// ⭐ **A connection merges contacts only as the position it JOINED as.** Every gate the
    /// host runs at JOIN (the version, the contest, the role, the call) decides on that JOIN,
    /// and the row's own `pos` used to choose whose log the row went into: a peer let in as
    /// one position could write into another's (taking its `seq`s, so that position's own
    /// contacts would then merge as repeats and be dropped) or into one no gate ever let in.
    /// Refused by name, and the connection closes. CONTROL: the same peer's own row merges.
    #[test]
    fn a_row_under_another_positions_id_is_refused_and_merges_nothing() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let got = talk(
            addr,
            &[
                join_msg("aaaa0001", 0),
                qso("bbbb0002", 1, "W1AW"),
                qso("aaaa0001", 1, "K1ABC"),
            ],
            700,
        );
        let control = talk(
            addr,
            &[join_msg("aaaa0001", 0), qso("aaaa0001", 1, "K1ABC")],
            700,
        );
        sd.store(true, Ordering::Relaxed);
        let calls = club.calls.lock().unwrap().clone();
        assert!(
            !calls.iter().any(|c| c.starts_with("merge bbbb0002")),
            "a row under a position this connection did not join as merged: {calls:?}"
        );
        assert!(
            got.iter()
                .any(|m| matches!(m, Msg::Error { msg } if msg.as_str() == ANOTHER_POSITIONS_ROW)),
            "the peer is told why: {got:?}"
        );
        assert!(
            !got.iter().any(|m| matches!(m, Msg::Ack { .. })),
            "nothing is acked on that connection: {got:?}"
        );
        assert!(
            calls.iter().any(|c| c == "merge aaaa0001 1"),
            "CONTROL: its own row merges: {calls:?}"
        );
        assert!(
            control.iter().any(|m| matches!(m, Msg::Ack { seq: 1 })),
            "{control:?}"
        );
    }

    /// ⭐ **A connection whose position the host gave to another laptop is told why and closed,
    /// rows or none, and nothing it sends afterwards is merged or acked.** Two connections are
    /// served as one position; the backend then refuses their hold, as a host does once its
    /// operator gives the position away. The one that sends nothing is closed at a tick; the one
    /// that sends a contact has it refused. CONTROL: before the give each one's contact merges,
    /// and after it a new JOIN is served with the new hold and its contact merges.
    #[test]
    fn a_connection_whose_position_was_given_away_is_told_why_and_closed() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let open = |m: &[Msg]| {
            let s = TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            let mut w = s.try_clone().unwrap();
            for line in m {
                w.write_all(encode_line(line).as_bytes()).unwrap();
            }
            (w, BufReader::new(s))
        };
        let read = |r: &mut BufReader<TcpStream>, ms: u64| {
            let mut got = Vec::new();
            let deadline = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < deadline {
                match read_capped_line(r) {
                    Ok(Some(line)) => got.extend(decode_line(&line)),
                    Ok(None) => return (got, true),
                    Err(_) => {}
                }
            }
            (got, false)
        };
        // One after the other, so each one's ack is its own contact's.
        let (_idle_w, mut idle) = open(&[join_msg("aaaa0001", 0), qso("aaaa0001", 1, "K1ABC")]);
        let (before_idle, _) = read(&mut idle, 600);
        let (mut busy_w, mut busy) = open(&[join_msg("aaaa0001", 0), qso("aaaa0001", 2, "N0XYZ")]);
        let (before_busy, _) = read(&mut busy, 600);
        *club.hold.lock().unwrap() = 1;
        busy_w
            .write_all(encode_line(&qso("aaaa0001", 3, "W1FAK")).as_bytes())
            .unwrap();
        let (after_idle, idle_closed) = read(&mut idle, 1500);
        let (after_busy, busy_closed) = read(&mut busy, 1500);
        let control = talk(
            addr,
            &[join_msg("aaaa0001", 0), qso("aaaa0001", 4, "W5DEF")],
            700,
        );
        sd.store(true, Ordering::Relaxed);
        let calls = club.calls.lock().unwrap().clone();
        let told = |got: &[Msg]| {
            got.iter()
                .any(|m| matches!(m, Msg::Error { msg } if msg.as_str() == "given away 0"))
        };
        let mut wrong = Vec::new();
        for (what, got, closed) in [
            ("the connection that sent nothing", &after_idle, idle_closed),
            (
                "the connection that sent a contact",
                &after_busy,
                busy_closed,
            ),
        ] {
            if !told(got) || !closed {
                wrong.push(format!(
                    "{what}: told {}, closed {closed}: {got:?}",
                    told(got)
                ));
            }
            if got.iter().any(|m| matches!(m, Msg::Ack { .. })) {
                wrong.push(format!("{what} had a contact acked: {got:?}"));
            }
        }
        if calls.iter().any(|c| c == "merge aaaa0001 3") {
            wrong.push(format!("a contact sent after the give merged: {calls:?}"));
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        assert!(
            before_idle.iter().any(|m| matches!(m, Msg::Ack { seq: 1 }))
                && before_busy.iter().any(|m| matches!(m, Msg::Ack { seq: 2 })),
            "CONTROL: each merged before the give: {before_idle:?} {before_busy:?}"
        );
        assert!(
            control.iter().any(|m| matches!(m, Msg::Ack { seq: 4 })),
            "CONTROL: a JOIN after the give is served and its contact merges: {control:?}"
        );
    }

    /// ⭐ **Every JOIN served on a connection hands its link back once, whichever way the
    /// connection ends**: when the peer closes, when it JOINs again (the first link goes before
    /// the second JOIN is asked), and when the host gives its position away. A refused JOIN was
    /// given no link and hands none back. CONTROL: a connection still open has handed back
    /// nothing.
    #[test]
    fn every_served_join_hands_its_link_back_once_whichever_way_its_connection_ends() {
        let club = Arc::new(FakeClub::default());
        *club.refuse_older.lock().unwrap() = Some("too old".into());
        let (addr, sd) = start_host(club.clone());
        let links = || -> Vec<String> {
            club.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.starts_with("join ") || c.starts_with("disconnect "))
                .cloned()
                .collect()
        };
        talk(
            addr,
            &[join_msg("aaaa0001", 0), join_msg("aaaa0001", 0)],
            600,
        );
        let older = Msg::Join {
            v: PROTO_VERSION - 1,
            pos: "cccc0003".into(),
            name: "tent".into(),
            call: "W9ABC".into(),
            max_seq: 0,
            contest: String::new(),
            role: String::new(),
            key: PositionKey::default(),
        };
        talk(addr, &[older], 400);
        let s = TcpStream::connect(addr).unwrap();
        let mut w = s.try_clone().unwrap();
        w.write_all(encode_line(&join_msg("bbbb0002", 0)).as_bytes())
            .unwrap();
        std::thread::sleep(Duration::from_millis(600));
        let open = links();
        *club.hold.lock().unwrap() = 1;
        std::thread::sleep(Duration::from_millis(800));
        let given = links();
        sd.store(true, Ordering::Relaxed);
        drop((w, s));
        let (v, older_v) = (PROTO_VERSION, PROTO_VERSION - 1);
        assert_eq!(
            open,
            [
                format!("join v{v} aaaa0001"),
                "disconnect aaaa0001 1".to_string(),
                format!("join v{v} aaaa0001"),
                "disconnect aaaa0001 2".to_string(),
                format!("join v{older_v} cccc0003"),
                format!("join v{v} bbbb0002"),
            ],
            "each link back once; the refused JOIN had none; CONTROL: the open one has not"
        );
        assert_eq!(
            given[open.len()..],
            ["disconnect bbbb0002 3"],
            "the connection the give closed"
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
        *club.refuse_older.lock().unwrap() = Some(
            "this club's host needs club sync v3 on every laptop. Update Nexus on this laptop."
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
                if msg == &club.refuse_older.lock().unwrap().clone().unwrap()),
            "the refusal reaches the operator unaltered: {got:?}"
        );
        // POSITIVE CONTROL: the SAME host serves a current position. A gate that
        // refused everybody would pass the assertion above.
        let (addr2, sd2) = start_host(club.clone());
        let ok = talk(addr2, &[join_msg("bbbb0002", 0)], 500);
        sd2.store(true, Ordering::Relaxed);
        assert!(
            ok.iter().any(|m| matches!(m, Msg::Welcome { .. })),
            "control: a current position is welcomed by the same host: {ok:?}"
        );
    }

    /// ⭐ THE new→old direction, and why `PROTO_VERSION` moves at all: a v5 position joining
    /// an older host — v2 (1.12 to 1.17.0), v3 (the test builds before take-back) or v4 (the
    /// releases before every laptop listed the club's contacts) — is
    /// refused there, loudly, at hour zero, and shows the operator that host's own words.
    ///
    /// Each old host is a socket speaking its build's rule, because a shipped build cannot be
    /// linked in here; the bytes it answers with are its `fdsync.rs`'s own format string with
    /// its version in it (`"this host speaks Field Day sync v{PROTO_VERSION}, you sent v{v} —
    /// update the host's Nexus"`, the same in 1.17.0, the v3 builds and v4), which is what that
    /// build writes.
    #[test]
    fn a_v5_position_joining_an_older_host_gets_that_hosts_refusal_verbatim() {
        for old in [2u32, 3, 4] {
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
                    // The old build's rule, verbatim: refuse a higher version and close.
                    if v > old {
                        let msg = format!(
                            "this host speaks Field Day sync v{old}, you sent v{v} — update the \
                             host's Nexus"
                        );
                        let _ = w.write_all(encode_line(&Msg::Error { msg }).as_bytes());
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
                5,
                "this build's position announces v5, which is what makes the v{old} host refuse it"
            );
            assert_eq!(
                pos.errors.lock().unwrap().as_slice(),
                [format!(
                    "this host speaks Field Day sync v{old}, you sent v5 — update the host's Nexus"
                )],
                "the v{old} host's own words reach the operator unaltered"
            );
        }
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
            mults: Vec::new(),
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
                key: throwaway_key(),
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
                if msg == "this host speaks Field Day sync v5, you sent v6 — update the host's Nexus"),
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
        // so they parse as Unknown), a take-back of its own row (data, like a
        // row), host-side vocabulary echoed back, and a legitimate row. The instrumented
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
                    || c.starts_with("retract ")
                    || c.starts_with("restore ")
                    || c.starts_with("pos ")
                    || c.starts_with("disconnect "),
                "a non-data-plane effect escaped the socket loop: {c}"
            );
        }
    }

    /// Read lines off `r` for `ms`, decoded, in arrival order.
    fn read_for(r: &mut BufReader<TcpStream>, ms: u64) -> Vec<Msg> {
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            match read_capped_line(r) {
                Ok(Some(line)) => got.extend(decode_line(&line)),
                Ok(None) => break,
                Err(_) => {}
            }
        }
        got
    }

    fn connected(addr: std::net::SocketAddr) -> (TcpStream, BufReader<TcpStream>) {
        let s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        (s.try_clone().unwrap(), BufReader::new(s))
    }

    fn send(w: &mut TcpStream, m: &Msg) {
        w.write_all(encode_line(m).as_bytes()).unwrap();
    }

    /// ⭐ **A position takes back and puts back its OWN rows, on its own connection, and is
    /// answered by value**: `retract` reaches the backend as the JOINed position and is answered
    /// `retracted`, `restore` likewise `restored`. A take-back naming ANOTHER position is refused
    /// with the sentence a row naming one gets and the connection closes, reaching nothing; one
    /// sent before any JOIN reaches nothing either; and one on a connection whose position the
    /// host gave away is refused with the backend's own sentence.
    #[test]
    fn a_position_takes_back_only_its_own_rows_on_its_own_connection() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let retracts = || -> Vec<String> {
            club.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.starts_with("retract ") || c.starts_with("restore "))
                .cloned()
                .collect()
        };

        // Before any JOIN: nothing reaches the backend.
        let (mut w, mut r) = connected(addr);
        send(
            &mut w,
            &Msg::Retract {
                pos: "aaaa0001".into(),
                seq: 1,
            },
        );
        let early = read_for(&mut r, 300);
        assert!(early.is_empty(), "no answer before a JOIN: {early:?}");
        assert!(retracts().is_empty(), "nothing before a JOIN");

        // Its own row, on its own connection.
        send(&mut w, &join_msg("aaaa0001", 0));
        send(&mut w, &qso("aaaa0001", 1, "W1AW"));
        send(
            &mut w,
            &Msg::Retract {
                pos: "aaaa0001".into(),
                seq: 1,
            },
        );
        send(
            &mut w,
            &Msg::Restore {
                pos: "aaaa0001".into(),
                seq: 1,
            },
        );
        let got = read_for(&mut r, 600);
        assert!(
            got.iter().any(|m| matches!(m, Msg::Retracted { seq: 1 })),
            "the take-back is answered: {got:?}"
        );
        assert!(
            got.iter().any(|m| matches!(m, Msg::Restored { seq: 1 })),
            "the restore is answered: {got:?}"
        );
        assert_eq!(retracts(), ["retract aaaa0001 1", "restore aaaa0001 1"]);
        drop((w, r));

        // Another position's row, from a connection JOINed as this one: refused and closed.
        let theirs = talk(
            addr,
            &[
                join_msg("bbbb0002", 0),
                Msg::Retract {
                    pos: "aaaa0001".into(),
                    seq: 1,
                },
                qso("bbbb0002", 1, "K1ABC"),
            ],
            600,
        );
        assert!(
            theirs
                .iter()
                .any(|m| matches!(m, Msg::Error { msg } if msg == ANOTHER_POSITIONS_ROW)),
            "another position's take-back is refused by name: {theirs:?}"
        );
        assert!(
            !theirs.iter().any(|m| matches!(m, Msg::Ack { .. })),
            "…and the connection closed on it: the row after it was never read: {theirs:?}"
        );
        assert_eq!(
            retracts(),
            ["retract aaaa0001 1", "restore aaaa0001 1"],
            "nothing reached the backend for another position"
        );

        // A position given away since its JOIN: the backend's refusal, verbatim.
        let (mut w, mut r) = connected(addr);
        send(&mut w, &join_msg("cccc0003", 0));
        let _ = read_for(&mut r, 300);
        *club.hold.lock().unwrap() = 7;
        send(
            &mut w,
            &Msg::Retract {
                pos: "cccc0003".into(),
                seq: 2,
            },
        );
        let given = read_for(&mut r, 600);
        sd.store(true, Ordering::Relaxed);
        assert!(
            given
                .iter()
                .any(|m| matches!(m, Msg::Error { msg } if msg.starts_with("given away"))),
            "{given:?}"
        );
        assert!(
            !retracts().iter().any(|c| c.contains("cccc0003")),
            "nothing was taken back for a position given away"
        );
    }

    /// ⭐ **After the welcome, the host says which of this position's rows it holds taken back,
    /// one line each, BEFORE the snap** — so a position that put one back while away sees it
    /// before it sends anything, and nobody else's are listed.
    #[test]
    fn the_hosts_take_backs_for_a_position_come_after_its_welcome_and_before_its_snap() {
        let club = Arc::new(FakeClub::default());
        for (pos, seq) in [("aaaa0001", 7), ("bbbb0002", 5), ("aaaa0001", 3)] {
            club.gone.lock().unwrap().insert((pos.to_string(), seq));
        }
        let (addr, sd) = start_host(club.clone());
        let got = talk(addr, &[join_msg("aaaa0001", 0)], 500);
        sd.store(true, Ordering::Relaxed);
        let shape: Vec<String> = got
            .iter()
            .take(4)
            .map(|m| match m {
                Msg::Welcome { .. } => "welcome".into(),
                Msg::Retracted { seq } => format!("retracted {seq}"),
                Msg::Snap(st) if st.reset => "snap".into(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(shape, ["welcome", "retracted 3", "retracted 7", "snap"]);
    }

    /// ⭐ **A new generation sends every connection a fresh snap** (`reset` set), so a position's
    /// mirror — which only unions a delta — drops a key the club's rebuilt lists no longer
    /// hold. CONTROL: the same wait with the generation unchanged brings no second snap.
    #[test]
    fn a_new_generation_sends_every_connection_a_fresh_snap() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let snaps = |got: &[Msg]| {
            got.iter()
                .filter(|m| matches!(m, Msg::Snap(st) if st.reset))
                .count()
        };
        let (mut w, mut r) = connected(addr);
        send(&mut w, &join_msg("aaaa0001", 0));
        let first = read_for(&mut r, 500);
        assert_eq!(snaps(&first), 1, "the join's own snap: {first:?}");
        let still = read_for(&mut r, 700);
        assert_eq!(
            snaps(&still),
            0,
            "CONTROL: no new generation, no snap: {still:?}"
        );
        *club.generation.lock().unwrap() += 1;
        let after = read_for(&mut r, 700);
        sd.store(true, Ordering::Relaxed);
        assert_eq!(
            snaps(&after),
            1,
            "one fresh snap for the new generation: {after:?}"
        );
    }

    /// A position with rows 1 and 3 live, row 4 taken back here, and row 2 put back here — the
    /// outbox computed as the engine computes it, from its own rows and the host's view.
    #[derive(Default)]
    struct TakingBack {
        /// The host's view: own rows it holds taken back.
        host_gone: Mutex<std::collections::BTreeSet<u64>>,
    }
    impl TakingBack {
        fn row(seq: u64) -> WireQso {
            match qso("eeee0005", seq, &format!("W{seq}AW")) {
                Msg::Qso(q) => q,
                _ => unreachable!(),
            }
        }
    }
    impl PositionSync for TakingBack {
        fn identity(&self) -> (String, String, String, u64) {
            ("eeee0005".into(), "Phone tent".into(), "KD9TAW".into(), 4)
        }
        fn key(&self) -> PositionKey {
            throwaway_key()
        }
        fn contest(&self) -> String {
            "arrlfd".into()
        }
        fn host_contest(&self, _contest: &str) -> Result<(), String> {
            Ok(())
        }
        fn outbox_after(&self, after: u64) -> Outbox {
            let gone = self.host_gone.lock().unwrap();
            let mut marks = Vec::new();
            if !gone.contains(&4) {
                marks.push(Mark::Retract {
                    pos: "eeee0005".into(),
                    seq: 4,
                });
            }
            if gone.contains(&2) {
                marks.push(Mark::Restore(Box::new(Self::row(2))));
            }
            Outbox {
                marks,
                rows: [1, 2, 3]
                    .into_iter()
                    .filter(|&s| s > after)
                    .map(Self::row)
                    .collect(),
            }
        }
        fn on_welcome(&self, _a: u64, _e: &str, _h: &str, _n: u64) {
            self.host_gone.lock().unwrap().clear();
        }
        fn on_ack(&self, _seq: u64) {}
        fn on_retracted(&self, seq: u64, gone: bool) {
            let mut g = self.host_gone.lock().unwrap();
            if gone {
                g.insert(seq);
            } else {
                g.remove(&seq);
            }
        }
        fn on_club(&self, _snap: bool, _st: &ClubState) {}
        fn on_rows(&self, _rows: &ClubRows) {}
        fn on_error(&self, _msg: &str) {}
        fn position_report(&self) -> Option<PosReport> {
            None
        }
        fn on_link(&self, _connected: bool) {}
        fn on_unreachable(&self, _why: Unreachable) {}
        fn on_clock(&self, _sample: ClockSample) {}
    }

    /// ⭐ **The pump sends nothing of its log until it has the host's view, then its take-backs
    /// before its rows, and each once.** A scripted host welcomes it (acked 2), says it holds row
    /// 2 taken back, and holds its snap back 400 ms: no row and no mark may come before the snap.
    /// Then, in order: the take-back of row 4, row 2 and its restore, then row 3 — and once the
    /// host has answered both, neither is sent again.
    #[test]
    fn a_position_sends_its_take_backs_before_its_rows_once_it_has_the_hosts_view() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            let mut log: Vec<(bool, String)> = Vec::new(); // (after the snap, line)
            let heard = |r: &mut BufReader<TcpStream>, log: &mut Vec<(bool, String)>, after, ms| {
                for m in read_for(r, ms) {
                    let line = match m {
                        Msg::Qso(q) => format!("qso {}", q.seq),
                        Msg::Retract { seq, .. } => format!("retract {seq}"),
                        Msg::Restore { seq, .. } => format!("restore {seq}"),
                        _ => continue,
                    };
                    log.push((after, line));
                }
            };
            heard(&mut r, &mut log, false, 300); // the JOIN
            for m in [
                Msg::Welcome {
                    v: PROTO_VERSION,
                    event: "TEST FD".into(),
                    host_call: "W9ABC".into(),
                    acked: 2,
                    now_unix: now_unix(),
                    contest: "arrlfd".into(),
                },
                Msg::Retracted { seq: 2 },
            ] {
                send(&mut w, &m);
            }
            heard(&mut r, &mut log, false, 400);
            send(
                &mut w,
                &Msg::Snap(ClubState {
                    reset: true,
                    ..Default::default()
                }),
            );
            heard(&mut r, &mut log, true, 500);
            for m in [Msg::Retracted { seq: 4 }, Msg::Restored { seq: 2 }] {
                send(&mut w, &m);
            }
            heard(&mut r, &mut log, true, 800);
            log
        });
        let pos: Arc<dyn PositionSync> = Arc::new(TakingBack::default());
        let sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos, sd2));
        let log = host.join().unwrap();
        sd.store(true, Ordering::Relaxed);
        let _ = pump.join();
        assert!(
            log.iter().all(|(after, _)| *after),
            "nothing of the log before the host's view is in: {log:?}"
        );
        let lines: Vec<&str> = log.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            ["retract 4", "qso 2", "restore 2", "qso 3"],
            "take-backs first, each once, then the rows past the ack"
        );
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
        /// Every attempt that did not reach the host, and why, in order.
        unreachable: Mutex<Vec<Unreachable>>,
        /// This position's club key, a throwaway one.
        key: PositionKey,
        /// The club list the host's `rows` lines built.
        list: Mutex<ClubList>,
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
                unreachable: Mutex::new(Vec::new()),
                key: throwaway_key(),
                list: Mutex::new(ClubList::default()),
            }
        }
    }
    impl PositionSync for FakePosition {
        fn identity(&self) -> (String, String, String, u64) {
            ("dddd0001".into(), "SSB tent".into(), "KD9TAW".into(), 3)
        }
        fn key(&self) -> PositionKey {
            self.key.clone()
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
        fn outbox_after(&self, after: u64) -> Outbox {
            let rows = (after + 1..=3)
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
                .collect();
            Outbox {
                marks: Vec::new(),
                rows,
            }
        }
        fn on_welcome(&self, _acked: u64, _event: &str, _host: &str, now_unix: u64) {
            *self.welcome_now.lock().unwrap() = now_unix;
        }
        fn on_ack(&self, seq: u64) {
            let mut a = self.acked.lock().unwrap();
            *a = (*a).max(seq);
        }
        fn on_retracted(&self, _seq: u64, _gone: bool) {}
        fn on_club(&self, _snap: bool, st: &ClubState) {
            *self.club_qsos.lock().unwrap() = st.qsos;
        }
        fn on_rows(&self, rows: &ClubRows) {
            self.list.lock().unwrap().apply(rows, LIST_CAP);
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
        fn on_unreachable(&self, why: Unreachable) {
            self.unreachable.lock().unwrap().push(why);
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
        let seed = WireQso {
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
        };
        club.merge(0, &seed).unwrap();
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
        assert_eq!(
            *posn.unreachable.lock().unwrap(),
            Vec::<Unreachable>::new(),
            "a host that answered is never reported unreachable"
        );
        assert_eq!(
            *club.keys.lock().unwrap(),
            [posn.key.secret().to_string()],
            "the pump's JOIN carried the position's key to the host's backend"
        );
    }

    // ---- the club list on every laptop (v5) ------------------------------

    /// A host's club log, modelled: every row it holds, with its merge order, and the view its
    /// own screen lists (the newest `LIST_CAP` by time made, the later merge first).
    #[derive(Default)]
    struct ModelClub {
        rows: Vec<WireLogRow>,
        next_ord: u64,
        stamp: u64,
    }
    impl ModelClub {
        fn add(&mut self, pos: &str, seq: u64, when: u64) {
            self.next_ord += 1;
            self.rows.push(list_row(pos, seq, when, self.next_ord));
            self.stamp += 1;
        }
        fn view(&self) -> ClubListView {
            let mut rows = self.rows.clone();
            rows.sort_by_key(|r| std::cmp::Reverse((r.when, r.ord)));
            rows.truncate(LIST_CAP);
            ClubListView {
                stamp: self.stamp,
                total: self.rows.len() as u64,
                rows,
            }
        }
    }

    /// ⭐ **A laptop's list is the host's after every change, and each change costs a line or
    /// two**: five positions log two thousand contacts each, interleaved, with offline catch-ups
    /// landing among older contacts, take-backs, restores, a duplicate marked late and a renamed
    /// position. After EVERY change the laptop's list (built only from the lines it was sent)
    /// equals the host's view, row for row, and every line is inside the cap. The bytes are the
    /// measurement the traffic bound is read from: a new contact at the top of a full list is one
    /// row in and one out.
    #[test]
    fn a_laptops_club_list_follows_the_hosts_through_ten_thousand_contacts() {
        let mut club = ModelClub::default();
        let mut sent = ListSent::default();
        let mut laptop = ClubList::default();
        let mut wire = Vec::new();
        sent.write(&mut wire, &club.view(), true).unwrap();
        for l in rows_lines(&wire) {
            laptop.apply(&l, LIST_CAP);
        }
        let (mut bytes, mut lines, mut changes) = (0usize, 0usize, 0usize);
        let mut step = |club: &ModelClub, sent: &mut ListSent, laptop: &mut ClubList| {
            let mut wire = Vec::new();
            sent.write(&mut wire, &club.view(), false).unwrap();
            let got = rows_lines(&wire);
            lines += got.len();
            bytes += wire.len();
            changes += 1;
            for l in &got {
                laptop.apply(l, LIST_CAP);
            }
            let v = club.view();
            assert_eq!(
                (laptop.total, &laptop.rows),
                (v.total, &v.rows),
                "change {changes}"
            );
        };
        let positions = ["aaaa0001", "bbbb0002", "cccc0003", "dddd0004", "eeee0005"];
        let mut seqs = [0u64; 5];
        let mut taken: Vec<WireLogRow> = Vec::new();
        for i in 0..10_000u64 {
            let p = (i % 5) as usize;
            seqs[p] += 1;
            // Two contacts often share a second; a position back from away now and then
            // sends one made ten minutes ago.
            let when = 1_782_583_200 + i / 2 - if i % 97 == 0 { 600 } else { 0 };
            club.add(positions[p], seqs[p], when);
            step(&club, &mut sent, &mut laptop);
            if i % 501 == 500 {
                // A take-back of the newest, and every other time its restore.
                let at = club.rows.len() - 1;
                taken.push(club.rows.remove(at));
                club.stamp += 1;
                step(&club, &mut sent, &mut laptop);
            } else if i % 1003 == 1002 {
                if let Some(r) = taken.pop() {
                    club.rows.push(r);
                    club.stamp += 1;
                    step(&club, &mut sent, &mut laptop);
                }
            }
            if i % 777 == 776 {
                // A late re-push makes an earlier contact the duplicate; a rename.
                let n = club.rows.len();
                club.rows[n - 1].dupe = !club.rows[n - 1].dupe;
                club.rows[n / 2].name = format!("renamed {i}");
                club.stamp += 1;
                step(&club, &mut sent, &mut laptop);
            }
        }
        // The measurement: per change, one or two lines of a few hundred bytes, never the list.
        let per_change = bytes / changes;
        eprintln!("club list traffic: {changes} changes, {lines} lines, {bytes} bytes, {per_change} bytes a change");
        assert!(per_change < 600, "{per_change} bytes a change");
        assert!(
            lines <= changes + changes / 50,
            "{lines} lines for {changes} changes"
        );
    }

    /// ⭐ **A full list of the longest rows goes over several lines, each inside the cap**, and the
    /// laptop's list is the host's; a list that stands sends nothing at all, and a list a laptop
    /// held before a rejoin is emptied by the first line, never unioned.
    #[test]
    fn a_full_list_of_long_rows_goes_over_lines_inside_the_cap() {
        let long = "X".repeat(64);
        let view = ClubListView {
            stamp: 7,
            total: 9_999,
            rows: (0..LIST_CAP as u64)
                .map(|i| {
                    let mut r = list_row(&format!("{i:08x}"), i + 1, 1_782_583_200 - i, i);
                    r.rcvd = vec![long.clone(); 4];
                    r.name = long.clone();
                    r.call = "W9ABCDEFGHIJKLM".into();
                    r
                })
                .collect(),
        };
        let mut sent = ListSent::default();
        let mut wire = Vec::new();
        sent.write(&mut wire, &view, true).unwrap();
        let got = rows_lines(&wire);
        assert!(got.len() > 3, "{} lines", got.len());
        assert!(got[0].reset && got[1..].iter().all(|l| !l.reset));
        let mut laptop = ClubList {
            total: 3,
            // Newer than every row of the list, so only the reset can take it off.
            rows: vec![list_row("dead0001", 1, 1_900_000_000, 1)],
        };
        for l in &got {
            laptop.apply(l, LIST_CAP);
        }
        assert_eq!((laptop.total, &laptop.rows), (view.total, &view.rows));
        // It stands: nothing.
        let mut again = Vec::new();
        sent.write(&mut again, &view, false).unwrap();
        assert!(again.is_empty());
        // CONTROL: a new stamp with one row marked a duplicate is that one row.
        let mut next = view.clone();
        next.stamp = 8;
        next.rows[5].dupe = true;
        let mut wire = Vec::new();
        sent.write(&mut wire, &next, false).unwrap();
        let got = rows_lines(&wire);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].put.len(), got[0].out.len()), (1, 0));
    }

    /// ⭐ **The host sends a joining laptop the whole club list after its snap, and then what
    /// changed**, over a real socket through the real pump; the laptop's list is the club's.
    #[test]
    fn the_host_sends_the_club_list_after_the_snap_and_then_what_changed() {
        let club = Arc::new(FakeClub::default());
        *club.list.lock().unwrap() = ClubListView {
            stamp: 1,
            total: 2,
            rows: vec![
                list_row("bbbb0002", 1, 1_782_583_260, 2),
                list_row("aaaa0001", 1, 1_782_583_200, 1),
            ],
        };
        let (addr, host_sd) = start_host(club.clone());
        // On the wire: welcome, the snap, then the list, its first line a reset.
        let got = talk(addr, &[join_msg("cccc0003", 0)], 600);
        let tags: Vec<&str> = got
            .iter()
            .filter_map(|m| match m {
                Msg::Welcome { .. } => Some("welcome"),
                Msg::Snap(_) => Some("snap"),
                Msg::Rows(r) if r.reset => Some("rows"),
                _ => None,
            })
            .collect();
        assert_eq!(&tags[..3], ["welcome", "snap", "rows"], "{got:?}");

        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));
        let wait_for = |want: &ClubListView| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                let l = posn.list.lock().unwrap().clone();
                if (l.total, &l.rows) == (want.total, &want.rows) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!(
                "the laptop never listed {want:?}: {:?}",
                posn.list.lock().unwrap()
            );
        };
        let first = club.list.lock().unwrap().clone();
        wait_for(&first);
        // A contact arrives and one is taken back: the laptop follows.
        let next = ClubListView {
            stamp: 2,
            total: 2,
            rows: vec![
                list_row("cccc0003", 1, 1_782_583_300, 3),
                list_row("aaaa0001", 1, 1_782_583_200, 1),
            ],
        };
        *club.list.lock().unwrap() = next.clone();
        wait_for(&next);
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        host_sd.store(true, Ordering::Relaxed);
    }

    /// ⭐ **A JOIN carries its position's key to the host's backend, and nothing prints it.**
    /// The key is a credential (`PositionKey`): the JOIN line is the one place it is written,
    /// in full, and the host's backend the one place it is handed. `Debug` (every `{:?}` of a
    /// message, a panic's, a failing assertion's) shows none of it. An older position's JOIN,
    /// which has none, is the bytes it always was. CONTROL: the encoded line does carry it, so
    /// the searches below can find it where it is.
    #[test]
    fn a_join_carries_its_key_to_the_host_and_nothing_prints_it() {
        let club = Arc::new(FakeClub::default());
        let (addr, sd) = start_host(club.clone());
        let join = join_msg("aaaa0001", 0);
        let Msg::Join { key, .. } = &join else {
            unreachable!()
        };
        let secret = key.secret().to_string();
        assert!(key.is_club_key(), "a throwaway key has a Nexus key's shape");
        let line = encode_line(&join);
        assert!(line.contains(&secret), "CONTROL: the JOIN line carries it");
        let got = talk(addr, std::slice::from_ref(&join), 500);
        sd.store(true, Ordering::Relaxed);
        assert!(
            got.iter().any(|m| matches!(m, Msg::Welcome { .. })),
            "{got:?}"
        );
        assert_eq!(*club.keys.lock().unwrap(), std::slice::from_ref(&secret));
        for shown in [
            format!("{join:?}"),
            format!("{key:?}"),
            format!("{:?}", decode_line(&line)),
        ] {
            assert!(!shown.contains(&secret), "{shown}");
            assert!(shown.contains("PositionKey(hidden)"), "{shown}");
        }
        let older = encode_line(&Msg::Join {
            v: 2,
            pos: "aaaa0001".into(),
            name: String::new(),
            call: String::new(),
            max_seq: 0,
            contest: String::new(),
            role: String::new(),
            key: PositionKey::default(),
        });
        assert_eq!(
            older,
            "{\"t\":\"join\",\"v\":2,\"pos\":\"aaaa0001\",\"name\":\"\",\"call\":\"\",\"max_seq\":0,\"contest\":\"\"}\n"
        );
        for not_one in ["", "abcd", &"A".repeat(64), &format!("{secret}0")] {
            assert!(
                !PositionKey::new(not_one.into()).is_club_key(),
                "{not_one:?}"
            );
        }
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
            key: throwaway_key(),
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
            removed: false,
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
            mults: Vec::new(),
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
                // No release before this one sent a multiplier value.
                mults: Vec::new(),
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

    /// `n` multiplier values worked club-wide, as an Illinois QSO Party club sends them: an
    /// in-state club that has worked every county and state holds 165 of `QTH`.
    fn mult_values(n: usize) -> Vec<(String, String)> {
        (0..n)
            .map(|i| ("QTH".to_string(), format!("Q{i:03}")))
            .collect()
    }

    /// The multiplier values a run of lines carries, in the order they came.
    fn mults_read(lines: &[String]) -> Vec<(String, String)> {
        lines
            .iter()
            .flat_map(|line| match decode_line(line) {
                Some(Msg::Snap(st)) | Some(Msg::Club(st)) => st.mults,
                _ => panic!("a club-state line: {line}"),
            })
            .collect()
    }

    /// ⭐ **The club's multiplier values reach every position whole, every line inside the
    /// cap** — beside a board as big as one line carries, in a join snapshot and a heartbeat
    /// alike: they fill what the key lists and the sections leave on a line, then lines of their
    /// own. And the rest of the state is the one the same club sends without them.
    #[test]
    fn the_clubs_multiplier_values_reach_every_position_inside_the_cap() {
        for (rows, field_day) in [(40, true), (40, false), (0, true)] {
            let without = big_club(rows, field_day);
            let mut with = without.clone();
            with.mults = mult_values(600);
            for snap in [true, false] {
                let lines = lines_of(with.clone(), snap);
                assert_eq!(
                    mults_read(&lines),
                    with.mults,
                    "{rows} positions, field day {field_day}, snap {snap}: every value, in order"
                );
                // `mirror_of` panics on a line past the cap.
                assert_eq!(
                    mirror_of(&lines),
                    mirror_of(&lines_of(without.clone(), snap)),
                    "the board, the keys, the sections and the counters are as without them"
                );
            }
        }
    }

    /// ⭐ **An older position reads the lines carrying the club's multiplier values, and a removed
    /// position's flag, exactly as it reads them without.** The shapes below are `ClubState` and
    /// `WireBoardRow` as every release before the two declares them, decoded by serde's own rule
    /// for a field a type does not name (dropped): a position built on them folds every line as
    /// it always did — the lists unioned, a snapshot's first chunk clearing them, the scalars
    /// overwritten, the board replaced by a chunk that has rows — and holds the same state either
    /// way. A line that carries only multiplier values carries the scalars it already holds.
    /// CONTROL: the lines do carry the list and the flag.
    #[test]
    fn an_older_position_reads_the_multiplier_values_lines_as_it_read_them_without() {
        #[derive(Deserialize, Debug, Clone, PartialEq, Default)]
        struct OldRow {
            pos: String,
            name: String,
            band: String,
            mode: String,
            op: String,
            qsos: u64,
            #[serde(default)]
            uniq: u64,
            rate: u64,
            age: u64,
        }
        #[derive(Deserialize, Debug, Default)]
        struct OldState {
            #[serde(default)]
            reset: bool,
            #[serde(default)]
            dupes: Vec<(String, String, String)>,
            #[serde(default)]
            dkeys: Vec<Vec<String>>,
            #[serde(default)]
            sections: Vec<String>,
            #[serde(default)]
            score: u32,
            #[serde(default)]
            qsos: u64,
            #[serde(default)]
            board: Vec<OldRow>,
        }
        #[derive(Deserialize, Debug)]
        #[serde(tag = "t", rename_all = "lowercase")]
        enum OldMsg {
            Snap(OldState),
            Club(OldState),
            #[serde(other)]
            Unknown,
        }
        type Held = (
            std::collections::BTreeSet<(String, String, String)>,
            std::collections::BTreeSet<Vec<String>>,
            std::collections::BTreeSet<String>,
            u32,
            u64,
            Vec<OldRow>,
        );
        let fold = |lines: &[String]| -> Held {
            let mut held = Held::default();
            for line in lines {
                let st = match serde_json::from_str::<OldMsg>(line.trim())
                    .unwrap_or_else(|e| panic!("an older position cannot decode {line}: {e}"))
                {
                    OldMsg::Snap(st) | OldMsg::Club(st) => st,
                    OldMsg::Unknown => panic!("read as a message it does not know: {line}"),
                };
                if st.reset {
                    held.0.clear();
                    held.1.clear();
                    held.2.clear();
                }
                held.0.extend(st.dupes);
                held.1.extend(st.dkeys);
                held.2.extend(st.sections);
                held.3 = st.score;
                held.4 = st.qsos;
                if !st.board.is_empty() || st.reset {
                    held.5 = st.board;
                }
            }
            held
        };
        for (rows, field_day) in [(10, true), (10, false), (40, true), (0, false)] {
            let without = big_club(rows, field_day);
            let mut with = without.clone();
            with.mults = mult_values(165);
            if rows > 1 {
                with.board[1].removed = true;
            }
            for snap in [true, false] {
                let new = lines_of(with.clone(), snap);
                assert!(
                    new.iter()
                        .any(|l| l.contains("\"mults\":[[\"QTH\",\"Q000\"]")),
                    "CONTROL: the lines carry the list"
                );
                assert_eq!(
                    new.iter().any(|l| l.contains("\"removed\":true")),
                    rows > 1,
                    "CONTROL: and the flag, on the removed position's line only"
                );
                let old = lines_of(without.clone(), snap);
                assert_eq!(
                    fold(&new),
                    fold(&old),
                    "{rows} positions, field day {field_day}, snap {snap}"
                );
                assert!(!fold(&new).5.is_empty() || rows == 0, "the board was read");
            }
        }
    }

    /// ⭐ **A position reading an older host's lines holds no multiplier values**, so its board is
    /// its own, as it always was (`club_board_loopback.rs` holds the screen to that). The lines are
    /// the bytes a host built before the list writes (`old_lines`). CONTROL: the same club from a
    /// host that sends the list.
    #[test]
    fn an_older_hosts_lines_carry_no_multiplier_values() {
        for (rows, field_day) in [(10, true), (10, false)] {
            let st = big_club(rows, field_day);
            for snap in [true, false] {
                assert!(mults_read(&old_lines(&st, snap)).is_empty());
            }
            let mut with = st.clone();
            with.mults = mult_values(3);
            assert_eq!(
                mults_read(&lines_of(with.clone(), true)),
                with.mults,
                "CONTROL"
            );
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

    // ---- why a join attempt did not reach the host ------------------------

    /// Run a pump at `addr` until it has reported one attempt that did not reach the host, or
    /// `secs` passed, and hand back what it reported and how its link went.
    fn first_unreachable(addr: &str, secs: u64) -> (Vec<Unreachable>, Vec<bool>) {
        let posn = Arc::new(FakePosition::default());
        let pos_backend: Arc<dyn PositionSync> = posn.clone();
        let pump_sd = Arc::new(AtomicBool::new(false));
        let (a, sd2) = (addr.to_string(), pump_sd.clone());
        let pump = std::thread::spawn(move || run_position_until(&a, pos_backend, sd2));
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline && posn.unreachable.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
        pump_sd.store(true, Ordering::Relaxed);
        pump.join().unwrap();
        let why = posn.unreachable.lock().unwrap().clone();
        let linked = posn.linked.lock().unwrap().clone();
        (why, linked)
    }

    #[test]
    fn an_address_typed_without_a_port_joins_the_club_port() {
        assert_eq!(join_target("192.168.1.10"), "192.168.1.10:42073");
        assert_eq!(join_target(" 192.168.1.10 "), "192.168.1.10:42073");
        assert_eq!(join_target("fe80::1"), "[fe80::1]:42073");
        assert_eq!(join_target("laptop-a"), "laptop-a:42073");
        // A port typed is the port joined.
        assert_eq!(join_target("192.168.1.10:42111"), "192.168.1.10:42111");
        assert_eq!(join_target("[fe80::1]:42111"), "[fe80::1]:42111");
        assert_eq!(join_target(""), "");
        assert_eq!(
            join_address("192.168.1.10"),
            Some(([192, 168, 1, 10], DEFAULT_TCP_PORT).into())
        );
        // Not addresses, and nothing is looked up for them.
        assert_eq!(join_address(""), None);
        assert_eq!(join_address("192.168.1.10:"), None);
        assert_eq!(join_address("192.168.1.10:99999"), None);
    }

    #[test]
    fn a_join_where_nothing_listens_is_reported_refused() {
        // Below the ephemeral range, so nothing but a bind naming it can take the port once it
        // is let go ([`listener_below_the_ephemeral_range`]): nothing listens there.
        let listener = listener_below_the_ephemeral_range();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let (why, linked) = first_unreachable(&addr.to_string(), 5);
        assert_eq!(why.first(), Some(&Unreachable::Refused));
        assert_eq!(linked, Vec::<bool>::new(), "no link came up");
    }

    #[test]
    fn a_join_nothing_answers_is_reported_as_no_answer() {
        // A listener that takes one connection and is never accepted from: its queue is full,
        // so the kernel drops the next one's SYN and that connect times out, as one to a host
        // whose firewall drops it does.
        let raw = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .unwrap();
        let any: std::net::SocketAddr = ([127, 0, 0, 1], 0).into();
        raw.bind(&any.into()).unwrap();
        raw.listen(0).unwrap();
        let listener: TcpListener = raw.into();
        let addr = listener.local_addr().unwrap();
        let _queued = TcpStream::connect(addr).expect("the one connection the queue holds");
        let started = Instant::now();
        let (why, linked) = first_unreachable(&addr.to_string(), 2 * CONNECT_SECS + 2);
        assert_eq!(why.first(), Some(&Unreachable::NoAnswer));
        assert!(
            started.elapsed() >= Duration::from_secs(CONNECT_SECS),
            "the attempt waited out its timeout"
        );
        assert_eq!(linked, Vec::<bool>::new(), "no link came up");
        drop(listener);
    }

    #[test]
    fn a_join_address_that_is_not_one_is_reported_as_such() {
        for typed in ["192.168.1.10:99999", ""] {
            let (why, linked) = first_unreachable(typed, 2);
            assert_eq!(why.first(), Some(&Unreachable::BadAddress), "{typed:?}");
            assert_eq!(linked, Vec::<bool>::new(), "{typed:?}");
        }
    }

    #[test]
    fn each_failed_connect_is_named_for_what_it_tells_the_operator() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            Unreachable::of(&Error::from(ErrorKind::ConnectionRefused)),
            Unreachable::Refused
        );
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::HostUnreachable,
            ErrorKind::NetworkUnreachable,
            ErrorKind::PermissionDenied,
        ] {
            assert_eq!(
                Unreachable::of(&Error::from(kind)),
                Unreachable::NoAnswer,
                "{kind:?}"
            );
        }
        assert_eq!(Unreachable::BadAddress.code(), "badAddress");
        assert_eq!(Unreachable::Refused.code(), "refused");
        assert_eq!(Unreachable::NoAnswer.code(), "noAnswer");
    }

    // ---- the beacon on every network the host is on -------------------------

    fn lan(adapter: &str, ip: [u8; 4], prefix: u8) -> LanAddr {
        LanAddr {
            adapter: adapter.into(),
            ip: ip.into(),
            prefix,
        }
    }

    #[test]
    fn a_subnets_broadcast_address_is_its_host_bits_set() {
        assert_eq!(
            lan("Wi-Fi", [192, 168, 1, 10], 24).broadcast(),
            Some([192, 168, 1, 255].into())
        );
        assert_eq!(
            lan("vEthernet (WSL)", [172, 29, 160, 1], 20).broadcast(),
            Some([172, 29, 175, 255].into())
        );
        assert_eq!(
            lan("Ethernet", [10, 1, 2, 3], 8).broadcast(),
            Some([10, 255, 255, 255].into())
        );
        assert_eq!(
            lan("cable, no DHCP", [169, 254, 7, 9], 16).broadcast(),
            Some([169, 254, 255, 255].into())
        );
        assert_eq!(lan("VPN", [10, 8, 0, 2], 32).broadcast(), None);
        assert_eq!(lan("link", [10, 9, 0, 0], 31).broadcast(), None);
    }

    #[test]
    fn the_beacon_goes_to_each_subnet_the_host_is_on_as_well_as_the_limited_broadcast() {
        // A Windows laptop with Wi-Fi, a cable and the adapter WSL adds: 255.255.255.255 leaves
        // by one of them only.
        let host = [
            lan("Wi-Fi", [192, 168, 1, 10], 24),
            lan("Ethernet", [10, 0, 0, 5], 24),
            lan("vEthernet (WSL)", [172, 29, 160, 1], 20),
            // Two addresses on one subnet: its broadcast address once.
            lan("Ethernet", [10, 0, 0, 6], 24),
            // A VPN's address alone has no broadcast address.
            lan("VPN", [10, 8, 0, 2], 32),
        ];
        assert_eq!(
            beacon_targets(&host),
            vec![
                std::net::Ipv4Addr::BROADCAST,
                [192, 168, 1, 255].into(),
                [10, 0, 0, 255].into(),
                [172, 29, 175, 255].into(),
            ]
        );
        assert_eq!(
            beacon_targets(&[]),
            vec![std::net::Ipv4Addr::BROADCAST],
            "a host that cannot list its adapters still sends what it always did"
        );
    }

    // Linux: every 127.0.0.0/8 address is this computer's, so two receivers can share a port.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_beacon_reaches_every_address_it_is_sent_to() {
        // Two receivers on one port, at two loopback addresses: one datagram each.
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = first.local_addr().unwrap().port();
        let second = UdpSocket::bind(("127.0.0.2", port)).unwrap();
        let sender = beacon_socket().unwrap();
        let line = beacon_line("ILQP rehearsal", "W9XYZ", 42073);
        send_beacon_to(
            &sender,
            &line,
            &[[127, 0, 0, 1].into(), [127, 0, 0, 2].into()],
            port,
        );
        for rx in [&first, &second] {
            rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 512];
            let (n, _) = rx.recv_from(&mut buf).expect("a beacon arrived");
            assert_eq!(
                parse_beacon(&buf[..n]),
                Some(("ILQP rehearsal".into(), "W9XYZ".into(), 42073))
            );
        }
    }

    /// A UDP port nothing holds, below the kernel's ephemeral range, for a test that lets it go
    /// and binds it again: only a bind that names it can land there meanwhile (the reasoning of
    /// [`listener_below_the_ephemeral_range`], for UDP's own ports).
    fn udp_port_below_the_ephemeral_range() -> u16 {
        use std::hash::BuildHasher;
        let low = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
            .ok()
            .and_then(|range| range.split_whitespace().next()?.parse().ok())
            .unwrap_or(32_768u16);
        let (floor, ceiling) = (20_000u16, low.min(32_768));
        (0..64u8)
            .map(|attempt| {
                let r = std::hash::RandomState::new().hash_one(attempt);
                floor + (r % u64::from(ceiling - floor)) as u16
            })
            .find(|&port| UdpSocket::bind(("0.0.0.0", port)).is_ok())
            .expect("a free UDP port below the ephemeral range")
    }

    #[test]
    fn find_club_events_hears_a_host_whose_first_beacon_comes_three_seconds_in() {
        // A port of the test's own, never the club's.
        let port = udp_port_below_the_ephemeral_range();
        let sender = std::thread::spawn(move || {
            let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
            std::thread::sleep(Duration::from_secs(3));
            for _ in 0..2 {
                let line = beacon_line("ILQP rehearsal", "W9XYZ", 42073);
                send_beacon_to(&sock, &line, &[[127, 0, 0, 1].into()], port);
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        let found = discover_on(port, DISCOVER_SECS).unwrap();
        sender.join().unwrap();
        assert_eq!(
            found.iter().map(|b| b.event.as_str()).collect::<Vec<_>>(),
            ["ILQP rehearsal"]
        );
    }

    #[test]
    fn this_computers_addresses_include_the_one_it_sends_from() {
        // The address the system picks to send toward a documentation address (a UDP connect
        // sends nothing). No route at all = nothing to compare with on this computer.
        let probe = UdpSocket::bind("0.0.0.0:0").unwrap();
        let Ok(()) = probe.connect("192.0.2.1:9") else {
            eprintln!("no IPv4 route on this computer: nothing to compare with");
            return;
        };
        let std::net::IpAddr::V4(from) = probe.local_addr().unwrap().ip() else {
            unreachable!("an IPv4 socket")
        };
        let mine = lan_addrs();
        assert!(
            mine.iter().any(|a| a.ip == from),
            "{from} is not among {mine:?}"
        );
        assert!(
            mine.iter().all(|a| !a.ip.is_loopback() && a.prefix <= 32),
            "{mine:?}"
        );
    }
}
