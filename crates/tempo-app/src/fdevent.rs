//! Field Day club-event state — the policy half of the Nexus↔Nexus sync whose
//! wire half is `tempo_net::fdsync`.
//!
//! The HOST holds a [`ClubLog`]: every position's rows merged idempotently by
//! `(position id, seq)`, an append-only NDJSON event journal (one merged row
//! per line, flushed per merge — no whole-file clobber window, unlike the ADIF
//! journal's rewrite), the club dupe index, sections, per-position stats and
//! the scored total. **The club log is always reconstructible as the union of
//! the position journals** — the invariant that makes "host dies, nothing
//! lost" true: a restarted host replays its own journal, and every position
//! re-pushes its tail for free because the merge is idempotent.
//!
//! A non-host POSITION holds only the compact [`ClubMirror`]: club dupe keys
//! (the while-typing verdict), sections, counters and board rows pushed down
//! by the host. No per-QSO attribution — which is why the scoreboard server
//! runs at the host (`Engine::fd_board_snapshot` is `Some` only there).
//!
//! Dupe semantics are N3FJP's: a CROSS-POSITION dupe is a **warning, never a
//! lock** — the host keeps both rows, exports dedupe earliest-wins, and the
//! score counts unique keys (order-independent: the key set is the same
//! whichever row "wins"). A position's OWN-log dupe stays the hard refusal it
//! has always been (`FieldDayLog::log_submode_at` → false). A contest whose
//! sponsor wants duplicates REPORTED ([`DupeRule::log_dupes`]) keeps every row in
//! the export instead, the later one marked, exactly as a position's own log does.
//!
//! ⭐ **A club runs ONE ruleset — the host's contest — and everything is read
//! from it**: the exchange the rows are resolved against, the dupe key, the
//! scoring and multipliers, the Cabrillo token and headers. A position logging a
//! different contest is refused at JOIN, by name ([`ClubLog::join_refusal`]), so is
//! one on another station call than the host's outside ARRL Field Day
//! ([`ClubLog::call_refusal`]), and a contest the club log cannot run faithfully is
//! refused before a club is built at all ([`club_refusal`]).
//!
//! Pure logic, no sockets — unit-testable. Engine wiring: `Engine::fd_club_*`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use tempo_core::contest::{
    carrier, CabrilloEntrant, ContestSession, DupeRule, ExchangeSpec, FieldKind, FieldValue,
};
use tempo_core::fd_rules::FdRuleset;
use tempo_core::fieldday::{FdEvent, FieldDayLog};
use tempo_net::fdsync::{ClubState, PosReport, WireBoardRow, WireField, WireQso};

/// Why a club cannot run a contest's ruleset — a reason the screen names, never a
/// silent wrong log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClubRefusal {
    /// The exchange carries a serial number. The entry's numbers must run in ONE
    /// sequence (Sweepstakes, CQ WPX and the California QSO Party all check them), and
    /// each position issues its own, so a merged log would send the same number twice.
    Serial,
    /// The sponsor's Cabrillo template carries a transmitter-id column (CQ WW, CQ WPX),
    /// whose value must say which transmitter made each contact — a number club sync
    /// does not assign.
    TransmitterColumn,
    /// The rules this build loaded have no ruleset for the contest (a downloaded rules
    /// file that dropped one). There is nothing to run, and the Field Day fallback a
    /// position's own session takes is not one a club may take.
    NoRuleset,
}

impl ClubRefusal {
    /// The code the UI keys its sentence by (`clubSyncRefusal` in `ui/src/fdEvent.ts`).
    pub fn code(self) -> &'static str {
        match self {
            ClubRefusal::Serial => "serial",
            ClubRefusal::TransmitterColumn => "transmitter",
            ClubRefusal::NoRuleset => "unknown",
        }
    }

    /// The same reason as words, for the host's own log line and the error a refused
    /// `fd_host_start` returns.
    pub fn sentence(self) -> &'static str {
        match self {
            ClubRefusal::Serial => {
                "its serial numbers must run in one sequence for the whole entry, and every \
                 position gives out its own"
            }
            ClubRefusal::TransmitterColumn => {
                "its log must say which transmitter made each contact, and club sync does not \
                 number the transmitters"
            }
            ClubRefusal::NoRuleset => "the contest rules this Nexus loaded do not include it",
        }
    }
}

/// ⭐ **Whether the club log can run this ruleset, decided from the ruleset's own data**
/// — `None` to run it, or the reason it cannot.
///
/// Data, not a list of contest names, so a rules-file row for a new QSO party gets club
/// sync the moment it is in the table, while a contest the merged log would get wrong is
/// refused before any club is built. Everything else a ruleset can say — its exchange,
/// roles, dupe key, mode groups, multipliers and caps, bonus stations, Cabrillo token,
/// headers and location slot, and whether duplicates are reported — the club log reads
/// from the ruleset like a position's own log does. The UI mirrors the answer per
/// contest (`CLUB_SYNC_REFUSED` in `ui/src/fdEvent.ts`), and a test holds the two
/// together.
pub fn club_refusal(rs: &FdRuleset) -> Option<ClubRefusal> {
    // An arm of a `OneOf` counts: a serial is a serial in whichever slot it rides.
    fn serial(k: &FieldKind) -> bool {
        match k {
            FieldKind::Serial { .. } => true,
            FieldKind::OneOf(arms) => arms.iter().any(serial),
            _ => false,
        }
    }
    if rs.exchange.fields.iter().any(|f| serial(&f.kind)) {
        return Some(ClubRefusal::Serial);
    }
    if rs.transmitter_column {
        return Some(ClubRefusal::TransmitterColumn);
    }
    None
}

/// Is this rules-file id one of the two Field Day events — the contests whose club log
/// is pinned byte for byte to what 1.x wrote?
pub fn is_field_day_id(event_id: &str) -> bool {
    matches!(event_id.trim(), "" | "arrlfd" | "wfd")
}

/// A rules-file id as a club compares it: trimmed, and the blank picker default read as
/// ARRL Field Day — what `set_mode` builds for it.
pub fn canonical_contest(event_id: &str) -> String {
    match event_id.trim() {
        "" => "arrlfd".to_string(),
        id => id.to_string(),
    }
}

/// The sponsor's contest id for a rules-file id — what a refusal names — or the id
/// itself for one this build's rules table does not carry.
pub fn contest_name(event_id: &str) -> String {
    tempo_core::fd_rules::ruleset_by_id(event_id, tempo_core::fd_rules::CURRENT_RULES_YEAR)
        .map_or_else(|| event_id.to_string(), |rs| rs.contest_id.to_string())
}

/// ⭐ **The sentence for a position logging one contest at a club running another** —
/// one wording, whichever end finds it (the host at JOIN, or the position on a welcome).
/// `club` is the club's contest as named; `mine` is the position's rules-file id.
pub fn contest_mismatch(club: &str, mine: &str) -> String {
    format!(
        "this club is running {club}, and this Nexus is logging {mine}. Pick {club} as the \
         contest on the Contesting tab in Settings, then rejoin. Contacts you log meanwhile \
         stay in your own log.",
        mine = contest_name(mine),
    )
}

/// A QSO party's exchange role as an operator knows it: the side of the state line it
/// sends for, and what that side sends. The role ids are the rules file's
/// (`exchange.roles[].id`); one this build has no words for is named by its id.
fn role_words(role: &str) -> (String, &'static str) {
    match role {
        "in_state" => ("in-state".into(), "its county"),
        "out_of_state" | "w_ve" => ("out-of-state".into(), "its state or province"),
        "dx" => ("DX".into(), "DX"),
        other => (format!("\"{other}\""), "another exchange"),
    }
}

/// ⭐ **The sentence for a position set up in another exchange role than the club's** — in a
/// QSO party, inside the state against outside it (or DX). Where Settings says a station is
/// decides which exchange it sends, and a club entry is one station in one place, so every
/// position sends the club's. One wording for the position's screen and the host's.
/// `contest` is the club's contest as named; `club` and `mine` are role ids.
pub fn role_mismatch(contest: &str, club: &str, mine: &str) -> String {
    let ((club_side, club_sends), (my_side, my_sends)) = (role_words(club), role_words(mine));
    format!(
        "this club sends the {club_side} {contest} exchange ({club_sends}), and this Nexus is \
         set up to send the {my_side} one ({my_sends}). Set State or province and County under \
         Your station data on the Contesting tab in Settings to the club's, then turn Field Day \
         mode off and on again: this Nexus rejoins by itself. Contacts you log meanwhile stay \
         in your own log."
    )
}

/// ⭐ **The sentence for a position on another station call than the club's** — the call
/// goes on the air, and the club's file claims every contact under the host's. One wording
/// for the position's screen and the host's. `contest` is the club's contest as named;
/// `club` and `mine` are the two calls as the screens show them.
pub fn call_mismatch(contest: &str, club: &str, mine: &str) -> String {
    format!(
        "this club is on the air as {club} and this Nexus as {mine}, and in {contest} every \
         position of a club entry sends the club's call. Set Callsign on the air under Who's \
         who at this event on the Contesting tab in Settings to {club}, then turn Field Day \
         mode off and on again: this Nexus rejoins by itself. Contacts you log meanwhile stay \
         in your own log."
    )
}

/// A field vector as it travels — the wire's and the journal's one shape.
pub fn to_wire_fields(vs: &[FieldValue]) -> Vec<WireField> {
    vs.iter()
        .map(|v| WireField {
            k: v.key.to_string(),
            d: v.domain.unwrap_or("").to_string(),
            r: v.raw.clone(),
        })
        .collect()
}

/// …and back, resolved against the exchange this club is running. Slots the
/// running exchange does not declare are dropped, which is
/// [`carrier::resolve`]'s rule and deliberately the same one: a wire triple and
/// a journal triple are the same triple.
pub fn from_wire_fields(ws: &[WireField], spec: &ExchangeSpec) -> Vec<FieldValue> {
    ws.iter()
        .filter_map(|w| carrier::resolve(&w.k, &w.d, &w.r, spec))
        .collect()
}

/// One merged club-log row — the design's reconciled shape (also what the
/// scoreboard seam's `FdBoardRow` mirrors). Serialized one-per-line into the
/// host's event journal.
///
/// ⚠️ **EVERY field added here after 1.x is `#[serde(default)]`, and the legacy
/// `class`/`section` pair became defaulted with them.** The journal replay is a
/// tolerant loop — `if let Ok(row) = serde_json::from_str::<MergedRow>(line)` —
/// which SKIPS what it cannot decode and reports nothing. One required field
/// added here and the whole pre-upgrade journal decodes as nothing: the host
/// comes up clean and empty, every position's ack watermark resets to 0, and a
/// position that has gone off the air is simply gone. Silently — the sync chip
/// reads Synced either way. `a_1x_host_journal_survives_the_v2_upgrade` is the
/// test that goes red if this rule is ever broken.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MergedRow {
    /// Position id (8-hex, per MACHINE, not per seat).
    pub posid: String,
    /// Per-position monotonic seq — `(posid, seq)` is the row's identity.
    pub seq: u64,
    pub call: String,
    /// LEGACY (1.x): the Field Day class they sent. `ex` is synthesised from it.
    #[serde(default)]
    pub class: String,
    /// LEGACY (1.x): the ARRL/RAC section they sent. `ex` is synthesised from it.
    #[serde(default)]
    pub section: String,
    /// The exchange THEY sent, as data — synthesised from `class`/`section` when
    /// a legacy row carries none.
    #[serde(default)]
    pub ex: Vec<WireField>,
    /// The exchange the LOGGING POSITION sent on this contact. Empty on a legacy
    /// row, which falls back to the club session's — what a Field Day host means.
    #[serde(default)]
    pub mex: Vec<WireField>,
    pub band: String,
    /// Scoring class: "DIG" | "CW" | "PH".
    pub mode_class: String,
    /// Actual on-air mode behind "DIG" ("FT8", "RTTY", …); "" = n/a.
    #[serde(default)]
    pub submode: String,
    /// The logging position's clock at log time. Earliest-wins dedupe orders
    /// by this (merge order breaks ties), and NOTHING ever adjusts it.
    pub when_unix: u64,
    /// Operator at the key, stamped by the position when the row was built.
    #[serde(default)]
    pub operator: String,
    /// ⭐ **The BIRD this contact was worked through** — the satellite's own catalogue
    /// name, empty for the terrestrial contact that is almost every row.
    ///
    /// It is here because it is part of the DUPE KEY under ARRL Field Day's rule
    /// (*"Show them listed separately on the summary sheet as a separate 'band'"*), and
    /// [`dkey`](Self::dkey) is one of the four sites that must build the identical key.
    /// Without it the host would judge every position's satellite contacts as
    /// terrestrial ones on the downlink's band — the same collision the rule removes,
    /// one layer out.
    ///
    /// ⚠️ `#[serde(default)]`, on this type's own standing rule: one required field
    /// added here and the whole pre-upgrade journal decodes as nothing, silently.
    #[serde(default)]
    pub sat: String,
    /// A single-channel FM satellite rather than a linear transponder — the other half
    /// of the key's satellite input. Defaulted for the same reason, and `false` is the
    /// safe side: it under-reports ARRL's one-QSO limit instead of refusing a legal
    /// contact through a linear bird.
    #[serde(default)]
    pub sat_fm: bool,
}

impl MergedRow {
    /// A wire row → the stored shape (field names per the design, not the
    /// wire's short forms).
    pub fn from_wire(q: &WireQso) -> Self {
        MergedRow {
            posid: q.pos.clone(),
            seq: q.seq,
            call: q.call.clone(),
            class: q.class.clone(),
            section: q.sect.clone(),
            ex: q.ex.clone(),
            mex: q.mex.clone(),
            band: q.band.clone(),
            mode_class: q.mode.clone(),
            submode: q.sub.clone(),
            when_unix: q.when,
            operator: q.op.clone(),
            sat: q.sat.clone(),
            sat_fm: q.sat_fm,
        }
    }

    /// ⭐ **THE one synthesis, and it is called from one place.** A row that
    /// carries the legacy `class`/`section` pair and no `ex` gets `ex` built from
    /// it, resolved through the running exchange so a synthesised slot is
    /// indistinguishable from one a v2 position sent (a `SECTION` value carries
    /// the domain that matched it, and an export tag and a multiplier bucket are
    /// chosen by that domain — dropping it would make a legacy row's exchange
    /// subtly unlike a native one).
    ///
    /// Both paths that produce a `MergedRow` — the wire decoder and the journal
    /// replay — funnel through [`ClubLog::merge_row`], which is the single call
    /// site. Two synthesis sites would drift; there is one, and it is not
    /// reachable any other way.
    fn synthesize_legacy_ex(&mut self, spec: &ExchangeSpec) {
        if !self.ex.is_empty() {
            return;
        }
        if self.class.trim().is_empty() && self.section.trim().is_empty() {
            return;
        }
        self.ex = to_wire_fields(
            &["CLASS", "SECTION"]
                .iter()
                .zip([self.class.as_str(), self.section.as_str()])
                .filter_map(|(k, v)| spec.value(k, v))
                .collect::<Vec<_>>(),
        );
    }

    /// The LEGACY club dupe key — `(CALL, band, MODE CLASS)`, the shape a v1
    /// position's while-typing check reads. Band travels verbatim (the DTO and
    /// the UI compare it against `snap.radio.band`, which is lower case), which
    /// is why this is not `dkey`'s first three components.
    pub fn dupe_key(&self) -> (String, String, String) {
        (
            self.call.to_uppercase(),
            self.band.clone(),
            self.mode_class.to_ascii_uppercase(),
        )
    }

    /// The club dupe key under the ruleset's own rule, built by the SAME
    /// `DupeRule::key_of` the position's own log and the while-typing verdict use
    /// — four sites in two languages must build the identical key, so there is
    /// one builder.
    ///
    /// For Field Day the rule is `(call, band, mode class)` with both field lists
    /// empty, so this is exactly the triple `dupe_key` returned before.
    pub fn dkey(&self, rule: &DupeRule, spec: &ExchangeSpec) -> Vec<String> {
        rule.key_of(
            &self.call,
            &self.band,
            &self.mode_class,
            &from_wire_fields(&self.ex, spec),
            &from_wire_fields(&self.mex, spec),
            tempo_core::contest::SatKey {
                bird: &self.sat,
                single_channel_fm: self.sat_fm,
            },
        )
    }
}

/// What the host remembers about one position (identity + presence).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClubPosition {
    /// Friendly label ("CW tent"), from the JOIN and refreshed by every
    /// presence report (so a rename mid-event lands). Empty until a position
    /// sends one — what an unnamed position reads as is the UI's business.
    pub label: String,
    /// Station callsign from the JOIN.
    pub call: String,
    /// Presence, from `pos` reports (band board fodder).
    pub band: String,
    pub mode: String,
    pub operator: String,
    pub freq: u64,
    /// Merged rows from this position (raw — dupes included).
    pub qsos_raw: u64,
    /// Newest merged row's own timestamp.
    pub last_qso_unix: u64,
    /// Host clock when this position was last heard from on its socket.
    pub last_seen_unix: u64,
    /// High-water acked seq (what `welcome` reports back on a rejoin).
    pub acked: u64,
    /// The position's clock minus this host's, in ms, as its last measured report said;
    /// `None` until one does (a Nexus older than the measurement never will). The host's
    /// own board shows it ([`ClubLog::clock_ms`]); it rides no board line, so the lines
    /// every position is sent are no longer for it.
    pub clock_ms: Option<i64>,
}

/// A position this host turned away, for the host's own screen ([`ClubLog::refused`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The name and station call its JOIN gave.
    pub label: String,
    pub call: String,
    /// The sentence it was sent, verbatim.
    pub reason: String,
    /// Host clock at its last refused JOIN.
    pub at_unix: u64,
}

/// How long the host keeps showing a refused position after its last try. A refused
/// position tries again at least every 15 s while its sync is on, so a minute without one
/// means it has stopped.
const REFUSED_SHOWN_SECS: u64 = 60;
/// The most refused positions the host keeps: the JOIN that fills this list comes off the
/// network, and a peer that cycles position ids must not grow it without bound.
const MAX_REFUSED: usize = 16;

/// The club's claimed score, part by part ([`ClubLog::score_with`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClubScore {
    pub qso_points: u32,
    /// QSO points after the power tier (equal to them for an event with none).
    pub powered: u32,
    /// The multiplier count, caps applied — `None` for a contest with no multiplier
    /// (both Field Days), never a `0` standing in for "not applicable".
    pub mults: Option<u32>,
    /// The ticked bonus menu plus the bonus stations the club worked.
    pub bonus: u32,
    pub total: u32,
}

/// The host-side merged club log. See the module header for the invariants.
#[derive(Debug, Default)]
pub struct ClubLog {
    /// The Field Day event, for the surfaces that only a Field Day club has (the
    /// spectator scoreboard). For any other contest it is the enum's default and
    /// nothing reads it: [`event_id`](Self::event_id) names the ruleset.
    pub event: FdEvent,
    /// ⭐ **The rules-file id of the ruleset this club runs** (`"arrlfd"`, `"wfd"`,
    /// `"ilqp"`) — the key every rule below is read through, as a position's own log
    /// reads its session's.
    ///
    /// `FdEvent` cannot be that key: it has two arms and `from_code` reads every other
    /// id as ARRL Field Day, which is how hosting the Illinois QSO Party built an ARRL
    /// Field Day club log in silence.
    pub event_id: String,
    /// The contest id of the contest this club is running (the ruleset's ADIF
    /// `CONTEST_ID` — `"ARRL-FIELD-DAY"`, `"IL QSO Party"`), which the JOIN gate's
    /// refusals name: whether a position may be served is a question about the
    /// CONTEST, and the refusal has to say which.
    pub contest_id: String,
    /// The operator-facing event name (beacon + welcome).
    pub event_name: String,
    rows: Vec<MergedRow>,
    /// `(posid, seq)` → merged (the idempotence index).
    ids: HashSet<(String, u64)>,
    /// Club dupe keys in FIRST-SEEN ORDER — append-only, so a down-flow delta
    /// is "everything past your cursor" and cursors never invalidate.
    ///
    /// `dupes_list` is the LEGACY `(call, band, mode class)` triple and is
    /// **index-parallel** to this one: both grow by exactly one entry per new key,
    /// so one cursor addresses both and the two can never disagree about what has
    /// been worked. Whether the legacy list reaches the wire is decided once, in
    /// [`club_state`](Self::club_state).
    dkeys_list: Vec<Vec<String>>,
    dkeys_set: HashSet<Vec<String>>,
    dupes_list: Vec<(String, String, String)>,
    /// Sections in first-seen order (same append-only contract).
    sections_list: Vec<String>,
    sections_set: HashSet<String>,
    positions: HashMap<String, ClubPosition>,
    /// Merge ARRIVAL times (host clock), pruned to the trailing hour — the
    /// per-position rate meter. In-memory only: after a host restart the rate
    /// honestly reads 0 until fresh merges arrive.
    arrivals: VecDeque<(u64, String)>,
    /// The append-only event journal. `None` = not journaling (tests).
    journal: Option<std::fs::File>,
    /// Positions this host turned away, by position id ([`note_refused`](Self::note_refused)).
    refused: HashMap<String, Refused>,
    /// `(positions, shown)` while the board last sent was as big as one club line carries,
    /// from [`tempo_net::fdsync::board_fit`] ([`note_board_sent`](Self::note_board_sent)).
    /// The host's warning reads it ([`board_full`](Self::board_full)), so the screen never
    /// builds a board of its own to measure.
    board_full: Option<(usize, usize)>,
}

/// How far back a host journal replay reaches. Matches the position ADIF journal's own
/// four-day expiry — see `ClubLog::attach_journal_since` for why they must agree.
const STALE_EVENT_SECS: u64 = 4 * 86_400;

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl ClubLog {
    /// A club running one of the two Field Day events.
    pub fn new(event: FdEvent, event_name: &str) -> Self {
        ClubLog {
            event,
            event_id: event.code().to_string(),
            contest_id: event.contest_id().to_string(),
            event_name: event_name.to_string(),
            ..Default::default()
        }
    }

    /// ⭐ **A club running ANY ruleset** — the constructor the host uses for every
    /// contest, Field Day included (a Field Day ruleset lands on exactly what
    /// [`new`](Self::new) builds). Whether the ruleset CAN run is [`club_refusal`]'s
    /// question, asked before this is called.
    pub fn for_ruleset(rs: &'static FdRuleset, event_name: &str) -> Self {
        if is_field_day_id(rs.event) {
            return Self::new(FdEvent::from_code(rs.event), event_name);
        }
        ClubLog {
            event_id: rs.event.to_string(),
            contest_id: rs.contest_id.to_string(),
            event_name: event_name.to_string(),
            ..Default::default()
        }
    }

    /// ⭐ **This club's log rebuilt from rows it already merged** — the journal replay's
    /// path (no arrival stamped, nothing journaled, no presence), for a read-only surface
    /// that scores the club OFF the engine lock. The spectator scoreboard clones
    /// [`rows`](Self::rows) under the lock and rebuilds here, so its dedupe, its exchange
    /// resolution and its unique log are this type's own, never a second reading of them.
    pub fn replayed(rs: &'static FdRuleset, rows: &[MergedRow]) -> Self {
        let mut club = Self::for_ruleset(rs, "");
        for row in rows {
            club.merge_row(row.clone(), 0);
        }
        club
    }

    /// The ruleset this club runs — through the RULES-FILE id, the one key that can
    /// name a contest `FdEvent` has no arm for. The fallback only keeps a default-built
    /// log answering; [`for_ruleset`](Self::for_ruleset) took the id from the table.
    pub fn ruleset(&self) -> &'static FdRuleset {
        tempo_core::fd_rules::ruleset_by_id(
            &self.event_id,
            tempo_core::fd_rules::CURRENT_RULES_YEAR,
        )
        .unwrap_or_else(|| {
            tempo_core::fd_rules::ruleset(self.event, tempo_core::fd_rules::CURRENT_RULES_YEAR)
        })
    }

    /// The Field Day event this club RUNS, or `None` for any other contest.
    ///
    /// A Field Day club's exchange stays [`contest::field_day`](tempo_core::contest::field_day)'s
    /// and its exports stay the bytes 1.x wrote (`a_1x_host_journal_survives_the_v2_upgrade`
    /// pins them), so the two places that differ ask this.
    pub fn field_day_event(&self) -> Option<FdEvent> {
        is_field_day_id(&self.event_id).then_some(self.event)
    }

    /// The exchange this club's rows are read against — the ruleset's own, and for a
    /// Field Day club the shipped Field Day exchange it has always been.
    fn exchange(&self) -> &'static ExchangeSpec {
        match self.field_day_event() {
            Some(event) => tempo_core::contest::field_day(event),
            None => self.ruleset().exchange,
        }
    }

    /// The rule this club's dupe keys are built by — the ruleset's own, which is
    /// what a position running the same contest builds its keys with.
    fn dupe_rule(&self) -> DupeRule {
        self.ruleset().dupe_rule
    }

    /// Is this club running one of the two Field Day events — the contests a v1
    /// position can enter, display and transmit?
    pub fn is_field_day(&self) -> bool {
        self.contest_id == FdEvent::ArrlFd.contest_id()
            || self.contest_id == FdEvent::WinterFd.contest_id()
    }

    /// ⭐ **§18.2 — why a joining position of protocol version `v` cannot be
    /// served, or `None` to serve it.**
    ///
    /// A v1 position running Field Day is served exactly as it always was, so a
    /// mixed-version Field Day club is unaffected. Running anything else, it is
    /// refused AT JOIN: it cannot enter, display or transmit that contest's
    /// exchange, so its rows would arrive with an empty exchange and score as
    /// zero-multiplier ones. A club discovering at hour six that one tent's 300
    /// contacts carry no county is worse than that tent knowing at hour zero.
    ///
    /// The message names the version AND the contest because it is the only thing
    /// its reader has: a bare "incompatible" leaves an operator on a field at 0200
    /// with no idea what to do. It also says what happens to the contacts they log
    /// meanwhile, which is true — the position journals them, and its outbox is
    /// "every own row past the host's ack", so they all go up on the first join
    /// that succeeds.
    pub fn version_refusal(&self, v: u32) -> Option<String> {
        if v >= tempo_net::fdsync::PROTO_VERSION || self.is_field_day() {
            return None;
        }
        Some(format!(
            "this club is running {contest} and needs club sync v{need} — this Nexus \
             speaks v{v}, which cannot enter, show or send the {contest} exchange. \
             Update this Nexus and rejoin: contacts you log meanwhile stay in your own \
             log and go up when you do.",
            contest = self.contest_id,
            need = tempo_net::fdsync::PROTO_VERSION,
        ))
    }

    /// ⭐ **Why a joining position cannot be served, or `None` to serve it** — the
    /// version rule ([`version_refusal`](Self::version_refusal)) first, then the CONTEST
    /// the position says it is logging (`contest`, a rules-file id; `""` from a
    /// position too old to send one).
    ///
    /// A club log reads every row against ONE ruleset, so a position logging another
    /// contest is refused AT JOIN and BY NAME: its rows would be resolved against the
    /// wrong exchange and keyed by the wrong rule, and a club finding that out at
    /// submission has nothing left to fix it with. Both Field Days are contests here,
    /// so a Winter Field Day position is refused by an ARRL Field Day club too.
    ///
    /// A position that names no contest is served by a Field Day club exactly as every
    /// position before the field was, and refused by any other: it cannot say what its
    /// rows mean, and the club cannot check.
    pub fn join_refusal(&self, v: u32, contest: &str) -> Option<String> {
        if let Some(msg) = self.version_refusal(v) {
            return Some(msg);
        }
        let theirs = match contest.trim() {
            "" if self.is_field_day() => return None,
            "" => {
                return Some(format!(
                    "this club is running {club}, and this Nexus is too old to say which \
                     contest it is logging. Update this Nexus and rejoin: contacts you log \
                     meanwhile stay in your own log and go up when you do.",
                    club = self.contest_id,
                ))
            }
            id => id,
        };
        if canonical_contest(theirs) == self.event_id {
            return None;
        }
        Some(contest_mismatch(&self.contest_id, theirs))
    }

    /// ⭐ **Why a joining position on another station call cannot be served, or `None` to
    /// serve it** — `club` is the host's call, the one the club's file is written under, and
    /// `theirs` the JOIN's: Settings' "Callsign on the air" on that position, which every
    /// Nexus has sent since club sync began.
    ///
    /// A club entry is one station on the air under one call: ARRL Field Day rule 6.12 ("All
    /// stations for a single entry must be operated under one callsign"), Winter Field Day
    /// ("one or many stations, all using the same callsign") and the Illinois QSO Party
    /// ("Each Mobile or Rover vehicle is considered one station and must use only one
    /// call"). A position on another call sends that call on the air while the club's file
    /// claims its contacts under the host's, so it is refused at JOIN, by name, before its
    /// first contact reaches the club.
    ///
    /// ⚠️ **Except ARRL Field Day**, whose GOTA station "must use a different callsign from
    /// the primary Field Day station" (rule 4.1.1.1) and whose contacts "may be claimed for
    /// credit by its primary Field Day operation" (4.1.1.5): a position there is served on
    /// any call. Winter Field Day has no GOTA station.
    ///
    /// Compared trimmed and case-blind, and nothing more: `W9XYZ/P` is another call on the
    /// air, and none of the rules the club runs that were read for this (ARRL Field Day and
    /// VHF, Winter Field Day, and the Illinois, New York, Ohio, Tennessee and Texas QSO
    /// parties) counts a portable suffix as the same station; an ARRL VHF rover signs `/R`
    /// as an entry of its own. A call either end cannot say (empty) is served as before.
    pub fn call_refusal(&self, club: &str, theirs: &str) -> Option<String> {
        let (club, theirs) = (club.trim(), theirs.trim());
        if club.is_empty()
            || theirs.is_empty()
            || club.eq_ignore_ascii_case(theirs)
            || self.event_id == FdEvent::ArrlFd.code()
        {
            return None;
        }
        Some(call_mismatch(
            &self.contest_id,
            &club.to_uppercase(),
            &theirs.to_uppercase(),
        ))
    }

    /// Open (creating if absent) the append-only journal at `path`, replaying
    /// any rows already in it — the host-restart recovery. Replayed rows are
    /// NOT re-journaled. Call once, before serving.
    pub fn attach_journal(&mut self, path: &PathBuf) -> std::io::Result<()> {
        self.attach_journal_since(path, now_unix().saturating_sub(STALE_EVENT_SECS))
    }

    /// [`Self::attach_journal`] with the cutoff exposed, so a test can age a journal
    /// without waiting days.
    ///
    /// ⚠️ THE CUTOFF EXISTS BECAUSE ITS ABSENCE SILENTLY ATE A WHOLE POSITION'S LOG, on
    /// the DEFAULT settings. The host journal is named from the event name, and the shipped
    /// default event name is EMPTY — which slugs to the literal "event", so every host that
    /// never typed a name shares ONE file, for ever. Replaying it a year later restored last
    /// year's rows AND their per-position ack watermarks; the position's own ADIF journal had
    /// self-expired at four days, so it restarted its sequence at 1; and the host then
    /// refused every new contact as a `(posid, seq)` it already held — while the sync chip
    /// read "Synced", because the merge returns the stored ack whether or not the row landed.
    /// A position worked a full event into a host that kept none of it, with nothing on
    /// screen wrong, discovered at submission.
    ///
    /// Four days is the position journal's own expiry (see `restore_field_day_if_enabled`),
    /// deliberately: the two halves of the same event must age out together or the watermark
    /// outlives the log it describes, which is exactly this bug.
    pub fn attach_journal_since(
        &mut self,
        path: &PathBuf,
        oldest_unix: u64,
    ) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                // Tolerant per line: one torn tail line (power loss mid-append)
                // must not poison the rest of the journal.
                if let Ok(row) = serde_json::from_str::<MergedRow>(line) {
                    // A row from a previous event carries a stale watermark; taking it
                    // makes this event's contacts look like duplicates.
                    //
                    // A row with NO timestamp (0) is kept. It cannot be aged, and silently
                    // dropping a contact we merely cannot date is the same class of mistake
                    // this cutoff exists to fix — an undateable row is a row somebody worked.
                    if row.when_unix != 0 && row.when_unix < oldest_unix {
                        continue;
                    }
                    self.merge_row(row, 0);
                }
            }
        }
        self.journal = Some(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?,
        );
        Ok(())
    }

    /// Merge one wire row at host time `now`; returns the (possibly
    /// unchanged) high-water ack for the row's position. Idempotent: a known
    /// `(posid, seq)` changes nothing — which is what makes every re-push
    /// after an outage free.
    pub fn merge(&mut self, q: &WireQso, now: u64) -> u64 {
        self.merge_row(MergedRow::from_wire(q), now);
        self.positions.get(&q.pos).map(|p| p.acked).unwrap_or(0)
    }

    /// [`merge`](Self::merge) minus the wire type; `now == 0` = a journal
    /// replay (no arrival stamped, nothing re-journaled).
    fn merge_row(&mut self, mut row: MergedRow, now: u64) -> bool {
        let id = (row.posid.clone(), row.seq);
        if row.seq == 0 || !self.ids.insert(id) {
            return false; // seq 0 is "never assigned" — refuse, don't guess
        }
        // THE synthesis point, and the only one: both producers of a MergedRow —
        // the wire decoder and the journal replay — reach the club log through
        // here, so a legacy row's exchange is reconstructed once or never.
        row.synthesize_legacy_ex(self.exchange());
        // ⭐ A satellite contact the contest gives no credit (Winter Field Day) is kept in
        // the club's rows and journal, and acked and counted on its position's board like
        // any row, but it counts for nothing in the club: no dupe key and no section, so it
        // never stands in the way of a contact that does count.
        let counts = row.sat.is_empty() || self.ruleset().satellite_credit;
        let dkey = row.dkey(&self.dupe_rule(), self.exchange());
        if counts && self.dkeys_set.insert(dkey.clone()) {
            self.dkeys_list.push(dkey);
            // Index-parallel, always built, sent only for a Field Day club.
            self.dupes_list.push(row.dupe_key());
        }
        let sect = row.section.trim().to_uppercase();
        if counts && !sect.is_empty() && self.sections_set.insert(sect.clone()) {
            self.sections_list.push(sect);
        }
        let pos = self.positions.entry(row.posid.clone()).or_default();
        pos.qsos_raw += 1;
        pos.last_qso_unix = pos.last_qso_unix.max(row.when_unix);
        pos.acked = pos.acked.max(row.seq);
        if now > 0 {
            pos.last_seen_unix = now;
            self.arrivals.push_back((now, row.posid.clone()));
            self.prune_arrivals(now);
        }
        if let Some(j) = &mut self.journal {
            // One line per merged row, flushed per merge: append-only, so a
            // crash can cost at most the final line — and the position that
            // sent it re-pushes it on reconnect anyway.
            if let Ok(line) = serde_json::to_string(&row) {
                let _ = j.write_all(line.as_bytes());
                let _ = j.write_all(b"\n");
                let _ = j.flush();
            }
        }
        self.rows.push(row);
        true
    }

    fn prune_arrivals(&mut self, now: u64) {
        while self
            .arrivals
            .front()
            .is_some_and(|(t, _)| now.saturating_sub(*t) > 3600)
        {
            self.arrivals.pop_front();
        }
    }

    /// A position joined (or rejoined): remember its identity, return the
    /// high-water ack it should stream past.
    pub fn join(&mut self, posid: &str, label: &str, call: &str, now: u64) -> u64 {
        self.refused.remove(posid);
        let pos = self.positions.entry(posid.to_string()).or_default();
        if !label.trim().is_empty() {
            pos.label = label.trim().to_string();
        }
        if !call.trim().is_empty() {
            pos.call = call.trim().to_uppercase();
        }
        pos.last_seen_unix = now;
        pos.acked
    }

    /// A JOIN this host refused, and the sentence it sent — kept for the host's own screen
    /// while the position keeps trying ([`refused`](Self::refused)), under the position id its
    /// join clears. The name and call come off the network, so each is cut to 64 characters,
    /// and the list to [`MAX_REFUSED`], the oldest going first.
    pub fn note_refused(&mut self, posid: &str, label: &str, call: &str, reason: &str, now: u64) {
        let cut = |s: &str| s.trim().chars().take(64).collect::<String>();
        self.refused.insert(
            posid.to_string(),
            Refused {
                label: cut(label),
                call: cut(call).to_uppercase(),
                reason: reason.to_string(),
                at_unix: now,
            },
        );
        while self.refused.len() > MAX_REFUSED {
            let oldest = self
                .refused
                .iter()
                .min_by_key(|(id, r)| (r.at_unix, (*id).clone()))
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                self.refused.remove(&id);
            }
        }
    }

    /// The positions refused within the last minute, by name (unnamed ones by id after) —
    /// one that has stopped trying drops off, and one that joined went at its join.
    pub fn refused(&self, now: u64) -> Vec<&Refused> {
        let mut out: Vec<(&String, &Refused)> = self
            .refused
            .iter()
            .filter(|(_, r)| now.saturating_sub(r.at_unix) <= REFUSED_SHOWN_SECS)
            .collect();
        out.sort_by(|(a_id, a), (b_id, b)| {
            (a.label.is_empty(), &a.label, *a_id).cmp(&(b.label.is_empty(), &b.label, *b_id))
        });
        out.into_iter().map(|(_, r)| r).collect()
    }

    /// A position's presence report. `r.name` is its CURRENT friendly name
    /// and is applied exactly like [`Self::join`]'s label: a non-empty one
    /// wins (that is how a rename mid-event reaches the board), an empty one
    /// changes nothing — an older peer sends no name at all, and treating
    /// that as "clear it" would blank a label the join already established.
    pub fn position_status(&mut self, posid: &str, r: &PosReport, now: u64) {
        let pos = self.positions.entry(posid.to_string()).or_default();
        if !r.name.trim().is_empty() {
            pos.label = r.name.trim().to_string();
        }
        pos.band = r.band.clone();
        pos.mode = r.mode.to_uppercase();
        pos.operator = r.op.to_uppercase();
        pos.freq = r.freq;
        // An unmeasured report is no news either: a reconnect's first report leaves
        // before its first round trip has closed.
        if r.clock_ms.is_some() {
            pos.clock_ms = r.clock_ms;
        }
        pos.last_seen_unix = now;
    }

    /// A position's clock minus this host's, in ms, as its last measured report said —
    /// the host's board column. `None` for a position that has not measured one.
    pub fn clock_ms(&self, posid: &str) -> Option<i64> {
        self.positions.get(posid).and_then(|p| p.clock_ms)
    }

    /// Note what the board just sent to a position could carry
    /// ([`tempo_net::fdsync::board_fit`]): the board rides every club line, and one too
    /// long for a line is cut to the positions heard from most recently.
    pub fn note_board_sent(&mut self, board: &[WireBoardRow]) {
        let fit = tempo_net::fdsync::board_fit(board);
        self.board_full = (!fit.room).then_some((board.len(), fit.rows.len()));
    }

    /// `Some((positions, shown))` once the board the positions are sent is as big as one club
    /// line carries — no room left for one more position as long as its longest, or already
    /// cut to `shown` of `positions`. `None` before.
    pub fn board_full(&self) -> Option<(usize, usize)> {
        self.board_full
    }

    /// Stamp a position's liveness (any socket activity counts — the board's
    /// stale marks are about the LINK, not about logging cadence).
    pub fn mark_seen(&mut self, posid: &str, now: u64) {
        if let Some(pos) = self.positions.get_mut(posid) {
            pos.last_seen_unix = now;
        }
    }

    /// The append-only cursors: (dupe keys, sections) totals. The dupe cursor
    /// addresses both key lists — they are index-parallel by construction.
    pub fn counts(&self) -> (usize, usize) {
        (self.dkeys_list.len(), self.sections_list.len())
    }

    pub fn rows(&self) -> &[MergedRow] {
        &self.rows
    }

    pub fn positions(&self) -> &HashMap<String, ClubPosition> {
        &self.positions
    }

    pub fn qsos_raw(&self) -> u64 {
        self.rows.len() as u64
    }

    pub fn dupe_keys(&self) -> &[(String, String, String)] {
        &self.dupes_list
    }

    /// The same keys under the ruleset's own rule — the generalised shape.
    pub fn dkeys(&self) -> &[Vec<String>] {
        &self.dkeys_list
    }

    pub fn sections(&self) -> &[String] {
        &self.sections_list
    }

    /// Earliest-wins unique attribution: for every club dupe key, the row
    /// that logged it FIRST by the position's own clock (merge order breaks
    /// ties — an offline position's late re-push of an EARLIER contact takes
    /// the key back, which is the honest reading of "earliest").
    fn earliest_unique_indices(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        order.sort_by_key(|&i| (self.rows[i].when_unix, i));
        let rule = self.dupe_rule();
        let spec = self.exchange();
        let credit = self.ruleset().satellite_credit;
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        let mut keep: Vec<usize> = Vec::new();
        for i in order {
            // A satellite contact the contest gives no credit is in no file and no score.
            if !credit && !self.rows[i].sat.is_empty() {
                continue;
            }
            if seen.insert(self.rows[i].dkey(&rule, spec)) {
                keep.push(i);
            }
        }
        keep.sort_unstable(); // back to log order
        keep
    }

    /// The rows the club EXPORTS, in the order it writes them.
    ///
    /// A Field Day club writes its earliest-wins unique rows in merge order, which is
    /// what 1.x wrote and what its goldens pin. Every other club writes in the order the
    /// contacts were MADE (each position's own clock, merge order breaking a tie) — a
    /// club log is several positions' logs interleaved, and a sponsor reads it as one
    /// station's — and a contest that wants duplicates reported
    /// ([`DupeRule::log_dupes`]) keeps them all: the later of two is marked by the log
    /// itself as it is written, so it scores zero and still reaches the file, where
    /// removing it would hand the other station a not-in-log.
    fn export_indices(&self) -> Vec<usize> {
        if self.field_day_event().is_some() {
            return self.earliest_unique_indices();
        }
        let mut keep = if self.dupe_rule().log_dupes {
            (0..self.rows.len()).collect()
        } else {
            self.earliest_unique_indices()
        };
        keep.sort_by_key(|&i| (self.rows[i].when_unix, i));
        keep
    }

    /// The row each contact of [`unique_log_with`](Self::unique_log_with) is built from,
    /// in that log's own order — an index into [`rows`](Self::rows) — so a surface can say
    /// which position logged each of the club's contacts without re-deriving the dedupe.
    pub fn unique_log_rows(&self) -> Vec<usize> {
        self.export_indices()
    }

    /// The deduped (earliest-wins) club log as a [`FieldDayLog`] under the
    /// HOST's station identity — the one artifact both exports and the score
    /// derive from, so they can never disagree with each other.
    pub fn unique_log(&self, mycall: &str, class: &str, section: &str) -> FieldDayLog {
        self.unique_log_with(
            mycall,
            ContestSession::field_day(self.event, class, section),
        )
    }

    /// [`unique_log`](Self::unique_log) for any ruleset: the club's rows rebuilt under
    /// `session`, which the caller builds from the HOST's station data for the contest
    /// this club runs (`Engine::fd_club_session`) — the same session a position running
    /// that contest logs under, so a club file and a position's file are written by one
    /// exporter.
    pub fn unique_log_with(&self, mycall: &str, mut session: ContestSession) -> FieldDayLog {
        // ⭐ BOTH sides come from the ROW. Every merged row used to be rebuilt under
        // the HOST's class and section — the same defect `cabrillo()` carried until
        // batch 3, one layer up — which is harmless for one Field Day club and wrong
        // the moment two positions send different exchanges, which is every QSO party
        // with a mobile. `mex` is what closes it.
        //
        // The host's own session is still the FALLBACK, and only that: a legacy row
        // carries no `mex`, and what a 1.x host meant by its absence is "the club's
        // sent exchange", which is exactly right for Field Day.
        let spec = self.exchange();
        // ⭐ A CLUB log IS a multi-operator entry, and this is the ONE place that is
        // true by construction: these rows were worked at several positions under one
        // callsign, which is exactly what `CATEGORY-OPERATOR: MULTI-OP` declares. The
        // session's default is `SINGLE-OP` because a lone operator is the default
        // user; a host merging positions is not that operator, and it says so here
        // rather than leaving the header to a picker nobody at a field site touched.
        session.entry_category = tempo_core::contest::OperatorCategory::MultiOp;
        // ⭐ WHO LOGGED EACH CONTACT, for every club but a Field Day one. The sponsor's
        // `OPERATORS` header is everyone the rows name (ILQP: *"make sure entry class,
        // personal information, call sign, station location, and club affiliation are
        // correctly shown"*), and the club file is the one the sponsor receives. A Field
        // Day club's rows name nobody, because its exports are pinned to the bytes 1.x
        // wrote, which carried no `OPERATOR`.
        let names_operators = self.field_day_event().is_none();
        let station = mycall.trim().to_ascii_uppercase();
        let mut log = FieldDayLog::new(mycall, session, "");
        for i in self.export_indices() {
            let r = &self.rows[i];
            log.band = r.band.clone();
            if names_operators {
                // A position with nobody named at the key sends the station's own call
                // (`Engine::fd_sync_outbox`, for the band board), and a row's operator is
                // never the station's own call (`LoggedQso::operator`) — so that call reads
                // back as nobody, not as an operator somebody named.
                let op = r.operator.trim().to_ascii_uppercase();
                log.operator = if op == station { String::new() } else { op };
            }
            let rx = from_wire_fields(&r.ex, spec);
            let tx = if r.mex.is_empty() {
                log.session.my_exchange.clone()
            } else {
                from_wire_fields(&r.mex, spec)
            };
            // Never refused: the indices are key-unique, or the ruleset logs its
            // duplicates and marks them.
            log.log_exchange_at(&r.call, rx, tx, &r.mode_class, &r.submode, 0, r.when_unix);
        }
        log
    }

    /// ⭐ **FIRING SITE 3 of 3 (§6.3): the club host, over the MERGED rows' own
    /// `mex`.**
    ///
    /// ⚠️ **The rule is per-LOG, so no per-position guard can see this one.** Two
    /// positions configured with different Sweepstakes checks under one club callsign
    /// each send a value that is perfectly constant *within that position's log*, and
    /// the entry the host submits is invalid. The host is the only place both halves
    /// are in the same collection, which is why this site exists and is named here
    /// rather than discovered when a club's entry is rejected.
    ///
    /// It runs the SAME scan the position's own export runs
    /// ([`FieldDayLog::constant_sent_scan`]) over the merged log, so the two cannot
    /// come to disagree about what "constant" means.
    pub fn constant_sent_mismatch(
        &self,
        mycall: &str,
        class: &str,
        section: &str,
    ) -> Option<tempo_core::contest::ConstantSentMismatch> {
        self.unique_log(mycall, class, section).constant_sent_scan()
    }

    /// Unique (scoring) rows count.
    pub fn qsos_unique(&self) -> u64 {
        self.dkeys_list.len() as u64
    }

    /// Club score under the HOST's power multiplier + claimed bonuses (the
    /// design's flagged judgment call: per-position multipliers are ignored —
    /// an ARRL entry is one station, one power tier; the operator saw and
    /// accepted this). Returns `(qso_pts, powered, bonus, total)`.
    pub fn scored(
        &self,
        mycall: &str,
        class: &str,
        section: &str,
        power_mult: u32,
        bonuses: &[String],
    ) -> (u32, u32, u32, u32) {
        let s = self.score_with(
            mycall,
            ContestSession::field_day(self.event, class, section),
            power_mult,
            bonuses,
            &[],
        );
        (s.qso_points, s.powered, s.bonus, s.total)
    }

    /// ⭐ **The club's claimed score under its own ruleset** — the arithmetic a position's
    /// own log shows (`Engine::fd_score`, `field_day_display`), over the club's rows: QSO
    /// points, the power tier where the event has one, the multipliers with their caps,
    /// then the ticked bonus menu plus the bonus stations the club worked (ILQP's two club
    /// calls, *"added to the final score"*, once for the whole entry however many
    /// positions worked them).
    ///
    /// For a Field Day club this is exactly [`scored`](Self::scored): neither event has a
    /// multiplier or a bonus station, so the total is the powered points plus the menu —
    /// except that Winter Field Day scores by objectives, so its total is the host's ticked
    /// `objectives` applied the sponsor's way (`FdRuleset::claimed_total`).
    pub fn score_with(
        &self,
        mycall: &str,
        session: ContestSession,
        power_mult: u32,
        bonuses: &[String],
        objectives: &[String],
    ) -> ClubScore {
        let rs = self.ruleset();
        let log = self.unique_log_with(mycall, session);
        let (qso_points, powered, mults, scored) = rs.scoring.score(log.score_rows(), power_mult);
        let bonus = rs.bonus_points(bonuses) + log.bonus_station_points();
        ClubScore {
            qso_points,
            powered,
            mults,
            bonus,
            total: rs.claimed_total(qso_points, scored, bonus, objectives),
        }
    }

    /// The band-board rows (one per known position), stalest-last untouched —
    /// display order is the UI's business. `age` is seconds since last heard.
    pub fn board_rows(&self, now: u64) -> Vec<WireBoardRow> {
        // Per-position uniq + rate in one pass each.
        let mut uniq: HashMap<&str, u64> = HashMap::new();
        for i in self.earliest_unique_indices() {
            *uniq.entry(self.rows[i].posid.as_str()).or_insert(0) += 1;
        }
        let mut rate: HashMap<&str, u64> = HashMap::new();
        for (t, p) in &self.arrivals {
            if now.saturating_sub(*t) <= 3600 {
                *rate.entry(p.as_str()).or_insert(0) += 1;
            }
        }
        let mut ids: Vec<&String> = self.positions.keys().collect();
        ids.sort();
        ids.iter()
            .map(|id| {
                let p = &self.positions[*id];
                WireBoardRow {
                    pos: (*id).clone(),
                    // The position's OWN name, and empty when it has none — the raw
                    // position id used to be the fallback, which put "9a85f060" on the
                    // club board where an operator expects a tent name. That id is
                    // internal plumbing (it exists so two positions' contacts can never
                    // collide) and is not something to show anybody. The UI decides what
                    // an unnamed position reads as, because that fallback is prose and
                    // prose belongs in the catalogs, not in a Rust string literal.
                    name: p.label.clone(),
                    band: p.band.clone(),
                    mode: p.mode.clone(),
                    op: p.operator.clone(),
                    qsos: p.qsos_raw,
                    uniq: uniq.get(id.as_str()).copied().unwrap_or(0),
                    rate: rate.get(id.as_str()).copied().unwrap_or(0),
                    age: now.saturating_sub(p.last_seen_unix.min(now)),
                }
            })
            .collect()
    }

    /// Down-flow state past the given cursors, with the current board.
    /// `(0, 0)` = the full join snapshot. Score fields come from the host's
    /// settings, passed in by the engine.
    pub fn club_state(
        &self,
        dupes_from: usize,
        sections_from: usize,
        score: u32,
        now: u64,
    ) -> ClubState {
        ClubState {
            reset: false, // the wire layer stamps the snap's first chunk
            // The legacy triple ships ONLY for a Field Day club: for any other
            // contest it is not the dupe rule, and a v1 position given one would
            // show a WRONG while-typing warning rather than none. Empty is the
            // honest failure. `dkeys` always ships — a v2 position reads that.
            dupes: if self.is_field_day() {
                self.dupes_list.get(dupes_from..).unwrap_or(&[]).to_vec()
            } else {
                Vec::new()
            },
            dkeys: self.dkeys_list.get(dupes_from..).unwrap_or(&[]).to_vec(),
            sections: self
                .sections_list
                .get(sections_from..)
                .unwrap_or(&[])
                .to_vec(),
            score,
            qsos: self.qsos_raw(),
            board: self.board_rows(now),
        }
    }

    /// Club ADIF export, deduped earliest-wins (the submittable artifact).
    pub fn export_adif(&self, mycall: &str, class: &str, section: &str) -> String {
        self.unique_log(mycall, class, section).adif()
    }

    /// [`export_adif`](Self::export_adif) for any ruleset, under the club session
    /// (see [`unique_log_with`](Self::unique_log_with)).
    pub fn export_adif_with(&self, mycall: &str, session: ContestSession) -> String {
        self.unique_log_with(mycall, session).adif()
    }

    /// [`export_cabrillo`](Self::export_cabrillo) for any ruleset: the club session's
    /// contest token and headers, and the entrant's own lines (`NAME`, `EMAIL`, `CLUB`,
    /// `ENTRY-CLASS`, `OPERATORS`), each written only where the ruleset lists it — the
    /// position's own exporter, over the club's rows.
    ///
    /// ⭐ **A Field Day club's `OPERATORS` are read off its rows here.** Those rows name nobody
    /// in the rebuilt log ([`unique_log_with`](Self::unique_log_with): the exports are pinned
    /// to the bytes 1.x wrote), yet the club's file is the one the sponsor receives, and
    /// Winter Field Day's lists OPERATORS: everyone the positions said was at the key, then
    /// the operators typed. A row naming the station's own call names nobody, as it does
    /// there. ARRL Field Day's rules list no OPERATORS, and its file is unchanged.
    pub fn export_cabrillo_with(
        &self,
        mycall: &str,
        session: ContestSession,
        entrant: &CabrilloEntrant,
    ) -> Result<String, String> {
        let mut entrant = entrant.clone();
        if self.field_day_event().is_some() {
            let station = mycall.trim().to_ascii_uppercase();
            let mut named: Vec<String> = Vec::new();
            for i in self.export_indices() {
                let op = self.rows[i].operator.trim().to_ascii_uppercase();
                if !op.is_empty() && op != station && !named.contains(&op) {
                    named.push(op);
                }
            }
            named.push(entrant.operators.clone());
            entrant.operators = named.join(" ");
        }
        self.unique_log_with(mycall, session)
            .cabrillo_with(0, &entrant)
    }

    /// Club Cabrillo export, deduped earliest-wins.
    ///
    /// ⚠️ THE DIAL ARGUMENT IS A LAST RESORT AND MUST NOT LOOK LIKE A BAND. It used to be a
    /// hardcoded `14_000`, which every row with an unmapped band silently borrowed — so a
    /// 23 cm club contact exported as 20 m. A club log is multi-band by definition and the
    /// host has no single dial to speak for it, so there is no honest frequency to pass:
    /// `0` reaches the exporter only for a row that recorded no band at all, and a zero in
    /// that field reads as missing rather than as a confident wrong answer. Every row that
    /// HAS a band now carries its own, mapped or verbatim.
    ///
    /// `Err` carries the reason the log is not one submittable entry — today only a
    /// mode-split contest whose rows span both modes (§6.2), which neither Field Day
    /// event is. The message is the operator's, so it is passed up rather than
    /// collapsed into a missing file.
    pub fn export_cabrillo(
        &self,
        mycall: &str,
        class: &str,
        section: &str,
    ) -> Result<String, String> {
        self.unique_log(mycall, class, section).cabrillo(0)
    }
}

// ---------------------------------------------------------------------------
// Position side
// ---------------------------------------------------------------------------

/// The compact club state a non-host position holds — everything the host
/// pushed down, nothing more (no per-QSO attribution; the scoreboard runs at
/// the host for exactly that reason).
#[derive(Debug, Default, Clone)]
pub struct ClubMirror {
    pub event: String,
    pub host_call: String,
    /// LEGACY club dupe keys — the while-typing verdict unions these with own
    /// log. EMPTY when the host is running anything but Field Day: the triple is
    /// not that contest's dupe rule, and no club warning is honest where a wrong
    /// one is not.
    pub dupes: HashSet<(String, String, String)>,
    /// The same keys under the ruleset's own rule — what a v2 position reads.
    pub dkeys: HashSet<Vec<String>>,
    pub sections: HashSet<String>,
    pub score: u32,
    pub qsos: u64,
    pub board: Vec<WireBoardRow>,
    /// Host's high-water ack for OUR rows (queued = own max seq − this).
    pub acked: u64,
    /// Link liveness + when it went down (the Offline chip's `since`).
    pub connected: bool,
    pub down_since_unix: u64,
    /// This PC's clock minus the host's, whole seconds: the welcome's until the first timed
    /// round trip closes, measured from then on ([`Self::clock`]). The club line shows it
    /// from 2 s and warns past 30 s; NOTHING ever adjusts a clock by it.
    pub skew_secs: i64,
    /// The round-trip measurement behind [`Self::skew_secs`], this session's only.
    pub clock: crate::clubclock::ClubClock,
    /// The last host `error` line, verbatim (version refusal etc.).
    pub last_error: Option<String>,
}

impl ClubMirror {
    /// Apply one `snap`/`club` line. `reset` (the snap's first chunk) clears
    /// the lists first; every chunk unions lists and overwrites scalars.
    pub fn apply(&mut self, st: &ClubState) {
        if st.reset {
            self.dupes.clear();
            self.dkeys.clear();
            self.sections.clear();
        }
        for k in &st.dupes {
            self.dupes.insert(k.clone());
        }
        for k in &st.dkeys {
            self.dkeys.insert(k.clone());
        }
        for s in &st.sections {
            self.sections.insert(s.clone());
        }
        self.score = st.score;
        self.qsos = st.qsos;
        if !st.board.is_empty() || st.reset {
            self.board = st.board.clone();
        }
    }

    pub fn on_welcome(&mut self, acked: u64, event: &str, host_call: &str, skew_secs: i64) {
        self.acked = acked;
        self.event = event.to_string();
        self.host_call = host_call.to_string();
        self.skew_secs = skew_secs;
        // A new session may be with another host, so the measurement starts again; until
        // its first round trip the welcome's whole seconds stand.
        self.clock = Default::default();
        self.last_error = None;
    }

    /// One answered round trip of the timed ping (`fdsync::ClockSample`). Updates what the
    /// club line says, and nothing else.
    pub fn on_clock(&mut self, sample: tempo_net::fdsync::ClockSample) {
        self.clock.add(sample);
        if let Some(secs) = self.clock.skew_secs() {
            self.skew_secs = secs;
        }
    }

    pub fn on_link(&mut self, connected: bool, now: u64) {
        if self.connected && !connected {
            self.down_since_unix = now;
        }
        self.connected = connected;
    }
}

/// The sync chip's four honest states — DERIVED from (link liveness, queued =
/// own max seq − host ack), so it can never disagree with the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// No hosting, no join address — the feature is off.
    Disabled,
    /// Link down; `queued` rows wait in the journal, since `since` (unix).
    Offline {
        queued: u64,
        since: u64,
    },
    /// Link up, rows still streaming.
    Behind {
        queued: u64,
    },
    Synced,
}

impl SyncState {
    /// The DTO string the UI switches on.
    pub fn code(&self) -> &'static str {
        match self {
            SyncState::Disabled => "disabled",
            SyncState::Offline { .. } => "offline",
            SyncState::Behind { .. } => "behind",
            SyncState::Synced => "synced",
        }
    }

    /// Derive from the inputs (the single computation, used by engine + tests).
    pub fn derive(enabled: bool, connected: bool, queued: u64, down_since: u64) -> SyncState {
        if !enabled {
            SyncState::Disabled
        } else if !connected {
            SyncState::Offline {
                queued,
                since: down_since,
            }
        } else if queued > 0 {
            SyncState::Behind { queued }
        } else {
            SyncState::Synced
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **The host's list of the positions it turned away**: one entry per position with the
    /// sentence it was sent last, gone when that position joins, dropped a minute after its
    /// last try, and never more than sixteen, the oldest going first.
    #[test]
    fn the_host_lists_each_refused_position_while_it_keeps_trying() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST");
        let shown = |c: &ClubLog, now: u64| -> Vec<(String, String, String)> {
            c.refused(now)
                .into_iter()
                .map(|r| (r.label.clone(), r.call.clone(), r.reason.clone()))
                .collect()
        };
        club.note_refused("aaaa0001", "SSB tent", "w9xyz", "first", 100);
        club.note_refused("aaaa0001", "SSB tent", "w9xyz", "second", 110);
        assert_eq!(
            shown(&club, 120),
            vec![("SSB tent".into(), "W9XYZ".into(), "second".into())],
            "one entry, the latest sentence, the call as a call"
        );
        assert_eq!(
            shown(&club, 170).len(),
            1,
            "kept a minute after the last try"
        );
        assert!(shown(&club, 171).is_empty(), "and not a second longer");
        club.note_refused("bbbb0002", "CW tent", "k9abc", "refused", 200);
        club.join("bbbb0002", "CW tent", "K9ABC", 201);
        assert!(
            shown(&club, 202).is_empty(),
            "a position that joins leaves the list"
        );
        for i in 0..20u64 {
            club.note_refused(&format!("c{i:07}"), "", "K9ZZZ", "refused", 300 + i);
        }
        let ids: Vec<&String> = club.refused.keys().collect();
        assert_eq!(ids.len(), 16, "never more than sixteen");
        assert!(
            (4..20).all(|i| club.refused.contains_key(&format!("c{i:07}"))),
            "the four oldest went first: {ids:?}"
        );
    }

    /// One presence report, dial fixed — these tests are about the name.
    fn report(name: &str, band: &str, mode: &str, op: &str) -> PosReport {
        PosReport {
            band: band.into(),
            mode: mode.into(),
            op: op.into(),
            freq: 14_032_100,
            name: name.into(),
            clock_ms: None,
        }
    }

    fn wq(
        pos: &str,
        seq: u64,
        call: &str,
        band: &str,
        mode: &str,
        sect: &str,
        when: u64,
    ) -> WireQso {
        WireQso {
            pos: pos.into(),
            seq,
            call: call.into(),
            // A v1 position's shape exactly: the legacy pair, and no `ex`/`mex`.
            // Every test below that uses this helper is therefore also a test that
            // a v1 position's rows still reach a v2 host's club log intact.
            class: "2A".into(),
            sect: sect.into(),
            ex: vec![],
            mex: vec![],
            band: band.into(),
            mode: mode.into(),
            sub: if mode == "DIG" {
                "FT8".into()
            } else {
                String::new()
            },
            when,
            op: "OP".into(),
            sat: String::new(),
            sat_fm: false,
        }
    }

    #[test]
    fn merge_is_idempotent_and_orders_dont_matter() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        assert_eq!(
            club.merge(&wq("aaaa", 2, "W1AW", "20m", "DIG", "CT", 200), 10),
            2
        );
        // Out of order: seq 1 arrives after 2 (a reconnect race) — merged fine,
        // ack stays the high-water.
        assert_eq!(
            club.merge(&wq("aaaa", 1, "K1ABC", "20m", "CW", "EMA", 100), 11),
            2
        );
        let before = club.qsos_raw();
        // Idempotence: the same (pos, seq) again changes NOTHING…
        assert_eq!(
            club.merge(&wq("aaaa", 2, "W1AW", "20m", "DIG", "CT", 200), 12),
            2
        );
        assert_eq!(club.qsos_raw(), before, "re-push merged nothing");
        // …POSITIVE CONTROL: a new seq from the same position DOES merge.
        assert_eq!(
            club.merge(&wq("aaaa", 3, "N0XYZ", "40m", "PH", "MN", 300), 13),
            3
        );
        assert_eq!(club.qsos_raw(), before + 1);
        // seq 0 ("never assigned") is refused, not guessed at.
        club.merge(&wq("aaaa", 0, "BAD0", "20m", "CW", "CT", 400), 14);
        assert_eq!(club.qsos_raw(), before + 1);
    }

    #[test]
    fn cross_position_dupe_merges_but_scores_once() {
        // N3FJP semantics: both rows kept (a warning at the position, never a
        // lock), the key counts ONCE for score/sections, exports dedupe.
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "DIG", "CT", 100), 10);
        club.merge(&wq("bbbb", 1, "W1AW", "20m", "DIG", "CT", 200), 11); // the dupe
        club.merge(&wq("bbbb", 2, "W1AW", "40m", "DIG", "CT", 300), 12); // NOT a dupe (new band)
        assert_eq!(club.qsos_raw(), 3, "all rows kept");
        assert_eq!(club.qsos_unique(), 2, "the same-band re-work counts once");
        let (qso_pts, powered, bonus, total) = club.scored("W9ABC", "3A", "WI", 2, &[]);
        assert_eq!(qso_pts, 4, "two unique DIG contacts × 2 pts");
        assert_eq!(powered, 8);
        assert_eq!((bonus, total), (0, 8));
        assert_eq!(club.sections(), ["CT"], "one section however many rows");
    }

    #[test]
    fn journal_replay_reproduces_identical_state() {
        let dir = std::env::temp_dir().join(format!("fdevent-j-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("fd_event_test.jsonl");

        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.attach_journal_since(&path, 0).expect("journal opens");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "DIG", "CT", 100), 10);
        club.merge(&wq("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200), 11);
        club.merge(&wq("aaaa", 2, "N0XYZ", "20m", "PH", "MN", 300), 12);

        // The host restarts: a fresh ClubLog replays the same journal.
        let mut reborn = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        reborn
            .attach_journal_since(&path, 0)
            .expect("journal replays");
        assert_eq!(reborn.rows(), club.rows(), "identical rows after replay");
        assert_eq!(reborn.counts(), club.counts());
        assert_eq!(
            reborn.join("aaaa", "", "", 20),
            2,
            "acked high-water survives the restart — the position streams only its tail"
        );
        // Replay did NOT re-journal: the file has exactly the 3 lines.
        let lines = std::fs::read_to_string(&path).unwrap();
        assert_eq!(lines.lines().count(), 3);

        // A torn tail line (power loss mid-append) poisons nothing.
        std::fs::write(
            &path,
            format!(
                "{lines}{}",
                &serde_json::to_string(&club.rows()[0]).unwrap()[..20]
            ),
        )
        .unwrap();
        let mut torn = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        torn.attach_journal_since(&path, 0)
            .expect("torn journal still opens");
        assert_eq!(
            torn.qsos_raw(),
            3,
            "the 3 whole lines restored, the torn one skipped"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exports_dedupe_earliest_wins_by_the_position_clock() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        // The LATER-MERGED row is the EARLIER contact (an offline position's
        // re-push): earliest-wins must keep IT, not the first-merged one.
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "DIG", "CT", 500), 10);
        let mut earlier = wq("bbbb", 1, "W1AW", "20m", "DIG", "CT", 100);
        earlier.class = "5A".into(); // distinguishable in the export
        club.merge(&earlier, 11);
        let cab = club
            .export_cabrillo("W9ABC", "3A", "WI")
            .expect("a single-mode event exports one entry");
        assert_eq!(cab.matches("QSO:").count(), 1, "deduped to one line");
        assert!(
            cab.contains("W1AW 5A CT"),
            "the earlier contact won, not the first-merged: {cab}"
        );
        let adif = club.export_adif("W9ABC", "3A", "WI");
        assert!(adif.contains("<CLASS:2>5A"), "ADIF agrees: {adif}");
        // Control: a non-dupe key exports alongside.
        club.merge(&wq("aaaa", 2, "K1ABC", "40m", "CW", "EMA", 700), 12);
        assert_eq!(
            club.export_cabrillo("W9ABC", "3A", "WI")
                .expect("a single-mode event exports one entry")
                .matches("QSO:")
                .count(),
            2
        );
    }

    #[test]
    fn board_rows_carry_presence_uniq_and_rate() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.join("aaaa", "CW tent", "KD9TAW", 1000);
        // An empty name = the older-peer shape: a presence report that carries
        // none, which must leave the join's label alone (its own test is below).
        club.position_status("aaaa", &report("", "20m", "cw", "op1"), 1000);
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "CW", "CT", 900), 1000);
        club.merge(&wq("bbbb", 1, "W1AW", "20m", "CW", "CT", 950), 1010); // cross-pos dupe
        let rows = club.board_rows(1015);
        assert_eq!(rows.len(), 2);
        let a = rows.iter().find(|r| r.pos == "aaaa").unwrap();
        assert_eq!(
            (a.name.as_str(), a.band.as_str(), a.mode.as_str()),
            ("CW tent", "20m", "CW")
        );
        assert_eq!(
            (a.qsos, a.uniq, a.rate),
            (1, 1, 1),
            "the earliest holds the unique"
        );
        let b = rows.iter().find(|r| r.pos == "bbbb").unwrap();
        // An unnamed position sends NO name — it does not send its id as one. The id
        // is on the row separately, for keying and nothing else; what an operator reads
        // in place of a missing name is prose, and prose lives in the catalogs. The id
        // WAS the fallback, and it put "9a85f060" on the club board where a tent name
        // belongs (operator report, 2026-08-30).
        assert!(
            b.name.is_empty(),
            "an unnamed position sends no name, not its id: {:?}",
            b.name
        );
        assert_eq!(b.pos, "bbbb", "…while the id itself still rides the row");
        assert_eq!((b.qsos, b.uniq), (1, 0), "the dupe merges but scores 0");
        assert_eq!(a.age, 15, "age = seconds since last heard");
        // Rate window: an arrival >1 h old stops counting.
        let rows = club.board_rows(1000 + 3700);
        assert_eq!(rows.iter().find(|r| r.pos == "aaaa").unwrap().rate, 0);
    }

    /// The host keeps each position's clock as its last MEASURED report said. An unmeasured
    /// report (a reconnect's first, or every one from an older Nexus) leaves it alone, a
    /// position that never measured has none, and a new measurement replaces the old.
    #[test]
    fn the_host_keeps_each_positions_last_measured_clock() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.join("aaaa", "CW tent", "KD9TAW", 1000);
        club.join("bbbb", "SSB tent", "KD9TAW", 1000);
        let clocked = |clock_ms| PosReport {
            clock_ms,
            ..report("", "20m", "cw", "op1")
        };
        club.position_status("aaaa", &clocked(Some(-3_000)), 1001);
        club.position_status("aaaa", &clocked(None), 1002);
        club.position_status("bbbb", &report("", "40m", "ph", "op2"), 1002);
        assert_eq!(club.clock_ms("aaaa"), Some(-3_000));
        assert_eq!(club.clock_ms("bbbb"), None);
        assert_eq!(
            club.clock_ms("cccc"),
            None,
            "a position the host never heard of"
        );
        club.position_status("aaaa", &clocked(Some(400)), 1004);
        assert_eq!(club.clock_ms("aaaa"), Some(400));
    }

    #[test]
    fn last_years_journal_cannot_swallow_this_years_event() {
        // ⚠️ THE DEFAULT-CONFIGURATION DATA LOSS. `fd_event_name` ships EMPTY, and the host
        // journal is named from its slug — an empty slug becomes the literal "event", so
        // every host that never typed a name writes to ONE file for ever. Replaying it a
        // year later restored last year's rows AND their per-position ack watermarks. The
        // position's own ADIF journal self-expires at four days, so it began this year at
        // seq 1 — and the host refused every contact as a `(posid, seq)` it already held,
        // while `merge` returned the stored ack so the position's chip read "Synced". A
        // full event logged into a host that kept none of it, with nothing on screen wrong.
        let dir = std::env::temp_dir().join(format!("nexus-fdj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fd_event_event.jsonl");
        let now = now_unix();
        let last_year = now - 365 * 86_400;

        // Last year's host wrote five rows for this position.
        {
            let mut old = ClubLog::new(FdEvent::ArrlFd, "event");
            old.attach_journal_since(&path, 0).unwrap();
            for seq in 1..=5 {
                old.merge_row(
                    MergedRow {
                        posid: "aaaa1111".into(),
                        seq,
                        call: format!("W1OLD{seq}"),
                        class: "2A".into(),
                        section: "CT".into(),
                        ex: vec![],
                        mex: vec![],
                        band: "20m".into(),
                        mode_class: "PH".into(),
                        submode: String::new(),
                        when_unix: last_year + seq,
                        operator: "KD9TAW".into(),
                        sat: String::new(),
                        sat_fm: false,
                    },
                    last_year,
                );
            }
        }
        assert!(path.exists(), "harness: last year's journal was written");

        // This year's host replays the SAME file.
        let mut host = ClubLog::new(FdEvent::ArrlFd, "event");
        host.attach_journal_since(&path, now.saturating_sub(STALE_EVENT_SECS))
            .unwrap();
        assert_eq!(
            host.rows().len(),
            0,
            "a year-old journal must not be replayed into this year's club log"
        );

        // …so this year's contacts, which restart at seq 1, are accepted.
        for seq in 1..=4 {
            host.merge_row(
                MergedRow {
                    posid: "aaaa1111".into(),
                    seq,
                    call: format!("W2NEW{seq}"),
                    class: "2A".into(),
                    section: "WI".into(),
                    ex: vec![],
                    mex: vec![],
                    band: "40m".into(),
                    mode_class: "CW".into(),
                    submode: String::new(),
                    when_unix: now + seq,
                    operator: "KD9TAW".into(),
                    sat: String::new(),
                    sat_fm: false,
                },
                now,
            );
        }
        assert_eq!(
            host.rows().len(),
            4,
            "this year's four contacts are in the club log"
        );

        // POSITIVE CONTROL: with the cutoff opened up, the bug reappears exactly as reported —
        // otherwise this test would pass against a change that simply broke journal replay.
        let mut naive = ClubLog::new(FdEvent::ArrlFd, "event");
        naive.attach_journal_since(&path, 0).unwrap();
        assert_eq!(
            naive.rows().len(),
            5,
            "control: without a cutoff last year's rows return"
        );
        let accepted = naive.merge_row(
            MergedRow {
                posid: "aaaa1111".into(),
                seq: 1,
                call: "W2NEW1".into(),
                class: "2A".into(),
                section: "WI".into(),
                ex: vec![],
                mex: vec![],
                band: "40m".into(),
                mode_class: "CW".into(),
                submode: String::new(),
                when_unix: now,
                operator: "KD9TAW".into(),
                sat: String::new(),
                sat_fm: false,
            },
            now,
        );
        assert_eq!(
            naive.rows().len(),
            5,
            "control: this year's contact was REFUSED as a dupe"
        );
        assert!(
            !accepted,
            "control: the row was refused as a duplicate — and nothing on screen said so"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_presence_report_renames_a_position_and_a_nameless_one_leaves_it_alone() {
        // The club-Field-Day bug (2026-08-30): the name travelled in the JOIN
        // line only, so an operator who renamed the position — or named one
        // that joined unnamed — watched the board keep the old text until the
        // connection was rebuilt. Presence reports now carry it.
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.join("aaaa", "CW tent", "KD9TAW", 1000);
        club.position_status("aaaa", &report("GOTA tent", "20m", "cw", "op1"), 1001);
        let label = |c: &ClubLog| c.positions()["aaaa"].label.clone();
        assert_eq!(label(&club), "GOTA tent", "the rename landed");
        // ...and the other direction, which is what keeps an older peer from
        // blanking a label it simply doesn't know how to send.
        club.position_status("aaaa", &report("", "20m", "cw", "op1"), 1002);
        assert_eq!(label(&club), "GOTA tent", "a nameless report is no news");
        club.position_status("aaaa", &report("   ", "20m", "cw", "op1"), 1003);
        assert_eq!(label(&club), "GOTA tent", "whitespace is not a name either");
        // A position the host has never heard a join from is still named by
        // its report (the id is plumbing; nobody should ever read it).
        club.position_status("bbbb", &report("SSB tent", "40m", "ph", "op2"), 1004);
        assert_eq!(label(&club), "GOTA tent");
        assert_eq!(club.positions()["bbbb"].label, "SSB tent");
    }

    #[test]
    fn mirror_unions_deltas_resets_on_snap_and_keeps_scalars_current() {
        let mut m = ClubMirror::default();
        m.apply(&ClubState {
            reset: true,
            dupes: vec![("W1AW".into(), "20m".into(), "DIG".into())],
            dkeys: vec![vec!["W1AW".into(), "20M".into(), "DIG".into()]],
            sections: vec!["CT".into()],
            score: 10,
            qsos: 1,
            board: vec![],
        });
        m.apply(&ClubState {
            reset: false,
            dupes: vec![("K1ABC".into(), "40m".into(), "CW".into())],
            dkeys: vec![vec!["K1ABC".into(), "40M".into(), "CW".into()]],
            sections: vec!["EMA".into()],
            score: 14,
            qsos: 2,
            board: vec![],
        });
        assert_eq!(m.dupes.len(), 2, "deltas union");
        assert_eq!(m.dkeys.len(), 2, "…and the generalised keys with them");
        assert_eq!((m.score, m.qsos), (14, 2), "scalars overwrite");
        // A rejoin snap (host restarted into a new event) CLEARS before applying.
        m.apply(&ClubState {
            reset: true,
            dupes: vec![("N0XYZ".into(), "20m".into(), "PH".into())],
            dkeys: vec![vec!["N0XYZ".into(), "20M".into(), "PH".into()]],
            sections: vec!["MN".into()],
            score: 1,
            qsos: 1,
            board: vec![],
        });
        assert_eq!(
            m.dupes.len(),
            1,
            "a snap replaces, never unions a dead event"
        );
        assert!(m
            .dupes
            .contains(&("N0XYZ".into(), "20m".into(), "PH".into())));
        assert_eq!(m.dkeys.len(), 1, "the generalised set is reset too");
        assert!(m
            .dkeys
            .contains(&vec!["N0XYZ".to_string(), "20M".into(), "PH".into()]));
    }

    #[test]
    fn sync_state_is_derived_and_cannot_disagree_with_the_queue() {
        use SyncState::*;
        assert_eq!(SyncState::derive(false, true, 0, 0), Disabled);
        assert_eq!(
            SyncState::derive(true, false, 3, 42),
            Offline {
                queued: 3,
                since: 42
            }
        );
        assert_eq!(SyncState::derive(true, true, 2, 0), Behind { queued: 2 });
        assert_eq!(SyncState::derive(true, true, 0, 0), Synced);
        assert_eq!(SyncState::Synced.code(), "synced");
        assert_eq!(SyncState::derive(true, false, 0, 7).code(), "offline");
    }

    // ---- v2: the wire, the journal, and both compatibility directions -----

    /// A REAL 1.x host journal: twelve NDJSON rows written by the shipped
    /// `MergedRow`, three positions, all three mode classes, two digital submodes,
    /// one unrecorded operator and one cross-position dupe — with the Cabrillo and
    /// ADIF that same build exported from it.
    ///
    /// ⭐ **PROVENANCE, and it is checked rather than asserted in prose.** The bytes
    /// were produced by a build at `f27a2e0a` (this branch's base, pre-batch-4),
    /// whose `MergedRow` is byte-identical to the one released as **`v1.10.3`** —
    /// `diff <(git show v1.10.3:crates/tempo-app/src/fdevent.rs) <(git show
    /// f27a2e0a:…)` over the struct is empty. So these ARE a 1.x host's bytes and a
    /// 1.x host's exports, not a hand-typed guess at them.
    ///
    /// That git check cannot run inside a test, so [`V1MergedRow`] carries the ten
    /// fields v1.10.3 declared, in its order, and
    /// `the_fixture_is_what_v1_10_3s_merged_row_wrote` round-trips every line
    /// through it. The test therefore holds its own definition of what v1 meant
    /// instead of trusting today's type to have stayed honest about it.
    ///
    ///
    /// ⚠️ The `.cbr`'s line 2 moved once since capture, deliberately:
    /// `CONTEST: ARRL-FIELD-DAY` → `CONTEST: ARRL-FD` (694 bytes to 687), because
    /// that header is a Cabrillo token and `ARRL-FIELD-DAY` is the ADIF one. See
    /// the module header of `tempo-core/tests/fd_goldens.rs`. The `.adi` did NOT
    /// move — its `CONTEST_ID` is the ADIF value and stays that way.
    ///
    /// ⚠️ **Two honest caveats.** The twelve contacts are SYNTHETIC — what is real
    /// is the serialization, not somebody's actual Field Day log. And the `.cbr`
    /// and `.adi` capture PRE-UPGRADE exporter behaviour, which is precisely the
    /// property that lets the migration test fail if the migration is wrong.
    const J1X: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.jsonl");
    const J1X_CBR: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.cbr");
    const J1X_ADI: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.adi");

    /// `MergedRow` EXACTLY as `v1.10.3` declared it — the ten fields, in that order,
    /// with that build's serde attributes. Nothing may be added here: its whole job
    /// is to be the frozen record of what a 1.x journal line is, so a future edit to
    /// the live `MergedRow` cannot quietly redefine the thing the fixture is being
    /// checked against.
    #[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
    struct V1MergedRow {
        posid: String,
        seq: u64,
        call: String,
        class: String,
        section: String,
        band: String,
        mode_class: String,
        #[serde(default)]
        submode: String,
        when_unix: u64,
        #[serde(default)]
        operator: String,
    }

    /// ⭐ The fixture proves its own provenance: every line parses as v1.10.3's
    /// `MergedRow` and re-serializes to the identical bytes.
    ///
    /// Round-tripping is what makes this stronger than "the JSON has no `ex` key".
    /// A line with a field v1 never declared, a field missing that v1 required, or
    /// a different field ORDER would all survive a key check and fail here — so
    /// "these bytes are a 1.x journal" stops being a claim in a report and becomes
    /// something the suite re-derives on every run.
    #[test]
    fn the_fixture_is_what_v1_10_3s_merged_row_wrote() {
        let mut n = 0;
        for line in J1X.lines() {
            let v1: V1MergedRow =
                serde_json::from_str(line).expect("every line is a v1.10.3 MergedRow");
            assert_eq!(
                serde_json::to_string(&v1).unwrap(),
                line,
                "the line re-serializes byte-identically under v1.10.3's own shape"
            );
            // …and today's type reads the same row out of those same bytes.
            let now: MergedRow = serde_json::from_str(line).unwrap();
            assert_eq!(
                (now.posid, now.seq, now.call, now.class, now.section),
                (v1.posid, v1.seq, v1.call, v1.class, v1.section),
                "the v2 type reads a v1 line as the v1 type wrote it"
            );
            n += 1;
        }
        assert_eq!(n, 12, "the whole fixture was checked, not an empty file");

        // POSITIVE CONTROL — the check discriminates. A v2-shaped line (the fields
        // this batch ADDED) must NOT round-trip as v1.10.3, or the assertion above
        // would pass for a regenerated fixture that had quietly become a v2 one.
        let v2_line = serde_json::to_string(&MergedRow {
            posid: "aaaa0001".into(),
            seq: 1,
            call: "W1AW".into(),
            class: "2A".into(),
            section: "CT".into(),
            ex: fd_fields(FdEvent::ArrlFd, "2A", "CT"),
            mex: fd_fields(FdEvent::ArrlFd, "3A", "WI"),
            band: "20m".into(),
            mode_class: "PH".into(),
            submode: String::new(),
            when_unix: 1_782_579_600,
            operator: "KD9TAW".into(),
            sat: String::new(),
            sat_fm: false,
        })
        .unwrap();
        let as_v1: V1MergedRow =
            serde_json::from_str(&v2_line).expect("v1 ignores fields it does not know");
        assert_ne!(
            serde_json::to_string(&as_v1).unwrap(),
            v2_line,
            "control: a v2 line does NOT round-trip as v1.10.3 — the check can tell them apart"
        );
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fdevent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A field vector for the Field Day exchange, resolved the way a position does.
    fn fd_fields(event: FdEvent, class: &str, section: &str) -> Vec<WireField> {
        let spec = tempo_core::contest::field_day(event);
        to_wire_fields(
            &["CLASS", "SECTION"]
                .iter()
                .zip([class, section])
                .filter_map(|(k, v)| spec.value(k, v))
                .collect::<Vec<_>>(),
        )
    }

    /// [`wq`]'s row in its v2 shape: the same contact, with the exchange as data
    /// on both sides instead of only the legacy pair. `my` is the class and
    /// section the LOGGING POSITION sent.
    fn with_sent(mut q: WireQso, my: (&str, &str)) -> WireQso {
        q.ex = fd_fields(FdEvent::ArrlFd, &q.class.clone(), &q.sect.clone());
        q.mex = fd_fields(FdEvent::ArrlFd, my.0, my.1);
        q
    }

    /// ⭐ §8g — THE test that fails if the host journal is dropped by the upgrade.
    ///
    /// Add one REQUIRED field to `MergedRow` and every pre-upgrade line decodes as
    /// nothing: the replay loop is `if let Ok(row) = …`, which skips silently. The
    /// host comes up clean, every position's ack watermark resets to 0, and a tent
    /// that has gone off the air is simply gone — with the sync chip still reading
    /// Synced. So this asserts on a real 1.x journal that ALL twelve rows come
    /// back, the dupe keys rebuild, each position's high-water ack is restored, and
    /// the score and both exports are the bytes that build produced.
    #[test]
    fn a_1x_host_journal_survives_the_v2_upgrade() {
        // The fixture is only evidence while it is still a 1.x journal: a
        // regenerated one carrying `ex`/`mex` would pass every assertion below
        // without proving anything about legacy decoding.
        for line in J1X.lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("fixture line is JSON");
            let obj = v.as_object().unwrap();
            assert!(
                !obj.contains_key("ex") && !obj.contains_key("mex"),
                "the fixture must stay a 1.x journal — this line carries v2 fields: {line}"
            );
            assert!(obj.contains_key("class") && obj.contains_key("section"));
        }

        let dir = scratch("j1x");
        let path = dir.join("fd_event_granite.jsonl");
        std::fs::write(&path, J1X).unwrap();

        let mut host = ClubLog::new(FdEvent::ArrlFd, "GRANITE ARC FD");
        host.attach_journal_since(&path, 0).unwrap();

        assert_eq!(
            host.qsos_raw(),
            12,
            "every row of the 1.x journal came back"
        );
        assert_eq!(
            host.qsos_unique(),
            10,
            "the dupe-key set rebuilt identically"
        );
        assert_eq!(
            host.sections(),
            ["CT", "EMA", "MN", "STX", "ONS", "AZ", "PR", "WI", "NLI"],
            "sections rebuilt, in first-seen order"
        );
        // The high-water ack per position — the value whose loss made a position
        // restart at seq 1 into a host that then refused every contact as a dupe.
        for pos in ["aaaa0001", "bbbb0002", "cccc0003"] {
            assert_eq!(host.join(pos, "", "", 0), 4, "{pos} ack watermark restored");
        }
        assert_eq!(
            host.scored("W9ABC", "3A", "WI", 2, &[]),
            (16, 32, 0, 32),
            "the same score the 1.x build computed from these bytes"
        );
        assert_eq!(
            host.export_cabrillo("W9ABC", "3A", "WI")
                .expect("a single-mode event exports one entry"),
            J1X_CBR,
            "the club Cabrillo is byte-identical to what 1.x exported"
        );
        assert_eq!(
            host.export_adif("W9ABC", "3A", "WI"),
            J1X_ADI,
            "…and so is the club ADIF"
        );

        // POSITIVE CONTROL — a green that cannot go red is not a result. Corrupt
        // ONE line: exactly that row is lost and the other eleven are untouched,
        // which proves the assertions above discriminate rather than pass vacuously.
        let mut lines: Vec<&str> = J1X.lines().collect();
        let torn = &lines[4][..30];
        lines[4] = torn;
        let corrupt = dir.join("fd_event_torn.jsonl");
        std::fs::write(&corrupt, lines.join("\n")).unwrap();
        let mut torn_host = ClubLog::new(FdEvent::ArrlFd, "GRANITE ARC FD");
        torn_host.attach_journal_since(&corrupt, 0).unwrap();
        assert_eq!(
            torn_host.qsos_raw(),
            11,
            "control: one corrupted line costs exactly one row"
        );
        assert!(
            !torn_host.rows().iter().any(|r| r.call == "VE3GHI"),
            "control: it is the corrupted row that is missing"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A v1 position's row reaches a v2 host with its exchange intact — the
    /// old→new direction, which is the one that fails SILENTLY if it fails.
    #[test]
    fn a_v1_rows_exchange_is_synthesised_and_carries_its_domain() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), 100);
        let row = &club.rows()[0];
        assert_eq!(
            row.ex,
            vec![
                WireField {
                    k: "CLASS".into(),
                    d: String::new(),
                    r: "2A".into()
                },
                WireField {
                    // The domain travels: an export tag and a multiplier bucket are
                    // chosen by it, so a synthesised slot that dropped it would be
                    // subtly unlike one a v2 position sent.
                    k: "SECTION".into(),
                    d: "fd_sections".into(),
                    r: "CT".into()
                },
            ],
            "the legacy pair was synthesised into the exchange, with its domain"
        );
        assert!(row.mex.is_empty(), "a v1 row sends no sent side");
        assert_eq!(
            row.dkey(
                &tempo_core::fd_rules::ruleset(
                    FdEvent::ArrlFd,
                    tempo_core::fd_rules::CURRENT_RULES_YEAR
                )
                .dupe_rule,
                tempo_core::contest::field_day(FdEvent::ArrlFd)
            ),
            vec!["W1AW", "20M", "PH"],
            "and the generalised key is Field Day's own rule"
        );
        // A row with neither an exchange nor the legacy pair synthesises nothing
        // rather than inventing two empty slots.
        let mut bare = wq("bbbb", 1, "K1ABC", "20m", "CW", "", 200);
        bare.class = String::new();
        club.merge(&bare, 200);
        assert!(
            club.rows()[1].ex.is_empty(),
            "nothing to synthesise from, so nothing synthesised"
        );
    }

    /// ⭐ §11 batch 4's own shippability test: an all-v2 club and a MIXED club run
    /// ARRL Field Day identically. Field Day is shipped, with users; if these three
    /// clubs disagree by one byte, a real club's submission moved.
    #[test]
    fn an_all_v2_club_and_a_mixed_club_run_field_day_identically() {
        let rows: &[(&str, u64, &str, &str, &str, &str, u64)] = &[
            ("aaaa", 1, "W1AW", "20m", "PH", "CT", 100),
            ("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200),
            ("aaaa", 2, "N0XYZ", "20m", "DIG", "MN", 300),
            ("bbbb", 2, "W1AW", "20m", "PH", "CT", 400), // cross-position dupe
            ("cccc", 1, "W5DEF", "15m", "CW", "STX", 500),
        ];
        let build = |v2_from: usize| {
            let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
            for (i, (p, s, c, b, m, sect, w)) in rows.iter().enumerate() {
                // Every position of a Field Day club sends the club's own class and
                // section, so a v2 row's `mex` is the host's — which is exactly what
                // a v1 row falls back to.
                if i >= v2_from {
                    club.merge(&with_sent(wq(p, *s, c, b, m, sect, *w), ("3A", "WI")), *w);
                } else {
                    club.merge(&wq(p, *s, c, b, m, sect, *w), *w);
                }
            }
            club
        };
        let all_v1 = build(rows.len()); // every row legacy-shaped
        let mixed = build(2); // two v1 tents, three v2 tents
        let all_v2 = build(0);
        for (label, club) in [("mixed", &mixed), ("all-v2", &all_v2)] {
            assert_eq!(
                club.export_cabrillo("W9ABC", "3A", "WI")
                    .expect("a single-mode event exports one entry"),
                all_v1
                    .export_cabrillo("W9ABC", "3A", "WI")
                    .expect("a single-mode event exports one entry"),
                "{label} club's Cabrillo moved"
            );
            assert_eq!(
                club.export_adif("W9ABC", "3A", "WI"),
                all_v1.export_adif("W9ABC", "3A", "WI"),
                "{label} club's ADIF moved"
            );
            assert_eq!(
                club.scored("W9ABC", "3A", "WI", 2, &[]),
                all_v1.scored("W9ABC", "3A", "WI", 2, &[]),
                "{label} club's score moved"
            );
            assert_eq!(club.dupe_keys(), all_v1.dupe_keys(), "{label} dupe keys");
            assert_eq!(club.dkeys(), all_v1.dkeys(), "{label} generalised keys");
        }
        // The harness is not vacuous: these clubs really did carry both shapes.
        assert!(all_v2.rows().iter().all(|r| !r.mex.is_empty()));
        assert!(all_v1.rows().iter().all(|r| r.mex.is_empty()));
        assert!(mixed.rows().iter().any(|r| r.mex.is_empty()));
        assert!(mixed.rows().iter().any(|r| !r.mex.is_empty()));
    }

    /// ⭐ The defect `mex` exists to close: the host rebuilt EVERY position's rows
    /// under its OWN class and section. Harmless for one Field Day club, wrong the
    /// moment two positions send different exchanges — a club spanning a section
    /// line, and every QSO party with a mobile.
    #[test]
    fn the_host_exports_each_rows_own_sent_exchange_not_its_own() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TWO-SITE FD");
        club.merge(
            &with_sent(wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), ("3A", "WI")),
            100,
        );
        club.merge(
            &with_sent(
                wq("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200),
                ("5A", "EMA"),
            ),
            200,
        );
        let cab = club
            .export_cabrillo("W9ABC", "3A", "WI")
            .expect("a single-mode event exports one entry");
        assert!(
            cab.contains("W9ABC 3A WI W1AW 2A CT"),
            "the first tent's own sent exchange: {cab}"
        );
        assert!(
            cab.contains("W9ABC 5A EMA K1ABC 2A EMA"),
            "the second tent sent 5A EMA and the line must say so: {cab}"
        );

        // POSITIVE CONTROL: the same two contacts as v1 rows, which carry no sent
        // side, both fall back to the host's — a DIFFERENT byte string. Without
        // this the test above would pass against a build that ignored `mex` and
        // happened to agree with the host on tent one.
        let mut legacy = ClubLog::new(FdEvent::ArrlFd, "TWO-SITE FD");
        legacy.merge(&wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), 100);
        legacy.merge(&wq("bbbb", 1, "K1ABC", "40m", "CW", "EMA", 200), 200);
        let legacy_cab = legacy
            .export_cabrillo("W9ABC", "3A", "WI")
            .expect("a single-mode event exports one entry");
        assert!(
            legacy_cab.contains("W9ABC 3A WI K1ABC 2A EMA"),
            "control: a legacy row falls back to the host's sent exchange: {legacy_cab}"
        );
        assert_ne!(
            cab, legacy_cab,
            "control: the fixture discriminates — reading `mex` changes the bytes"
        );
    }

    /// ⭐ §18.2 — the operator's ruling. A v1 position is refused at JOIN when the
    /// club is running a contest it cannot enter, show or send, and the message
    /// names BOTH the required version and the contest.
    #[test]
    fn a_v1_position_is_refused_only_when_the_club_is_not_running_field_day() {
        // Field Day keeps today's behaviour: a same-or-lower join is served, so a
        // mixed-version Field Day club is unaffected.
        for event in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            let club = ClubLog::new(event, "TEST FD");
            assert!(club.is_field_day());
            assert_eq!(
                club.version_refusal(1),
                None,
                "a v1 tent may still join a Field Day club"
            );
        }

        // Anything else refuses a v1 position.
        let mut qp = ClubLog::new(FdEvent::ArrlFd, "TNQP 2026");
        qp.contest_id = "TN-QSO-PARTY".into();
        assert!(!qp.is_field_day());
        let msg = qp.version_refusal(1).expect("a v1 tent is refused");
        assert_eq!(
            msg,
            "this club is running TN-QSO-PARTY and needs club sync v2 — this Nexus \
             speaks v1, which cannot enter, show or send the TN-QSO-PARTY exchange. \
             Update this Nexus and rejoin: contacts you log meanwhile stay in your own \
             log and go up when you do.",
            "the refusal names the version AND the contest — it is all its reader has"
        );
        // POSITIVE CONTROL for the "names both" claim: neither half is incidental.
        assert!(msg.contains("TN-QSO-PARTY") && msg.contains("v2") && msg.contains("v1"));

        // …and a CURRENT position is served by the same club. Refusing on version
        // when the version is fine would lock every tent out of the QSO party.
        assert_eq!(qp.version_refusal(tempo_net::fdsync::PROTO_VERSION), None);
    }

    /// The legacy triple ships only for Field Day; the generalised key always
    /// does. A v1 position given a triple that is not the running dupe rule would
    /// show a WRONG while-typing warning, which is worse than none.
    #[test]
    fn the_legacy_dupe_triple_ships_only_for_a_field_day_club() {
        let mut club = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        club.merge(&wq("aaaa", 1, "W1AW", "20m", "PH", "CT", 100), 100);
        club.merge(&wq("aaaa", 2, "K1ABC", "40m", "CW", "EMA", 200), 200);
        let st = club.club_state(0, 0, 0, 300);
        assert_eq!(
            st.dupes,
            vec![
                ("W1AW".to_string(), "20m".into(), "PH".into()),
                ("K1ABC".to_string(), "40m".into(), "CW".into()),
            ],
            "a Field Day club ships the triple a v1 position understands, band verbatim"
        );
        assert_eq!(
            st.dkeys,
            vec![
                vec!["W1AW".to_string(), "20M".into(), "PH".into()],
                vec!["K1ABC".to_string(), "40M".into(), "CW".into()],
            ],
            "…and the generalised key beside it, index-parallel"
        );
        // The cursor addresses both lists.
        let delta = club.club_state(1, 0, 0, 300);
        assert_eq!(delta.dupes.len(), 1);
        assert_eq!(delta.dkeys.len(), 1);

        club.contest_id = "TN-QSO-PARTY".into();
        let st = club.club_state(0, 0, 0, 300);
        assert!(
            st.dupes.is_empty(),
            "no club warning beats a wrong one when the triple is not the rule"
        );
        assert_eq!(st.dkeys.len(), 2, "the generalised keys still ship");
    }

    // ---- a club runs the HOST's ruleset, whatever the contest -------------

    fn party(event: &str) -> &'static FdRuleset {
        tempo_core::fd_rules::ruleset_by_id(event, tempo_core::fd_rules::CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("{event} is a shipped ruleset"))
    }

    /// The HOST's session for a contest, from the station data Settings holds — what
    /// `Engine::fd_club_session` builds for a host that is not running it live.
    fn party_session(event: &str, state: &str, county: &str) -> ContestSession {
        ContestSession::for_ruleset(
            party(event),
            &tempo_core::contest::StationData {
                mycall: "W9XYZ".into(),
                mygrid: "EN61".into(),
                contest_qth_state: state.into(),
                contest_qth_county: county.into(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{event} session refused: {e}"))
    }

    /// An exchange as a position sends it: each value resolved through the ruleset's
    /// own spec (`ExchangeSpec::copied`), so the domain that matched travels with it.
    fn party_fields(event: &str, pairs: &[(&str, &str)]) -> Vec<WireField> {
        let spec = party(event).exchange;
        to_wire_fields(
            &pairs
                .iter()
                .filter_map(|(k, v)| spec.copied(k, v))
                .collect::<Vec<_>>(),
        )
    }

    /// One Illinois QSO Party contact as a current position streams it: the county THEY
    /// sent, the county THIS POSITION sent, the RST each way at the digits its mode
    /// uses, and the station call as operator (what a position with nobody named at the
    /// key sends).
    #[allow(clippy::too_many_arguments)] // one wire row's worth of fields
    fn ilqp_row(
        pos: &str,
        seq: u64,
        call: &str,
        band: &str,
        mode: &str,
        sub: &str,
        theirs: &str,
        mine: &str,
        when: u64,
    ) -> WireQso {
        let rst = if mode == "PH" { "59" } else { "599" };
        WireQso {
            pos: pos.into(),
            seq,
            call: call.into(),
            class: String::new(),
            sect: String::new(),
            ex: party_fields("ilqp", &[("RST", rst), ("QTH", theirs)]),
            mex: party_fields("ilqp", &[("RST", rst), ("QTH", mine)]),
            band: band.into(),
            mode: mode.into(),
            sub: sub.into(),
            when,
            op: "W9XYZ".into(),
            sat: String::new(),
            sat_fm: false,
        }
    }

    /// 1700Z, Sunday 18 October 2026 — the party's first minute.
    const ILQP_START: u64 = 1_792_342_800;

    /// ⭐ **THE REVIEW'S PROBE, as a test: an Illinois QSO Party club runs the party's own
    /// rules.**
    ///
    /// Hosting the party used to build an ARRL Field Day club log in silence. These three
    /// rows from one position — K9AAA on 40 m CW from Cook, the same station on RTTY, and
    /// the same station on CW again from Will (a mobile that moved) — produced
    /// `contest_id=ARRL-FIELD-DAY`, dupe keys `[K9AAA 40M CW] [K9AAA 40M DIG]` (RTTY a
    /// second contact, the new county a dupe), merged rows with the county gone, and a
    /// club Cabrillo headed `CONTEST: ARRL-FD` whose QSO lines carried no exchange.
    ///
    /// The sponsor, *"Announcing the 2026 Illinois QSO Party"* (w9awe.org, read
    /// 2026-10-08): *"Stations may be worked once per band and mode (phone and
    /// CW/digital) and once per band/mode/county for IL Mobile and Rover stations"*; and
    /// its `Sample_Excel_Log` heads the file `CONTEST: ILLINOIS QSO PARTY` and writes each
    /// county code on the QSO line — `QSO: 7000 CW 2013-10-20 1714 W9XYZ 599 JODA K9NR 599
    /// KANK`. *"IL stations multiply points by the sum of IL counties, US states, VE
    /// provinces and DXCC countries (maximum 5) worked."*
    #[test]
    fn an_illinois_qso_party_club_runs_the_partys_own_rules() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        assert_eq!(
            (club.event_id.as_str(), club.contest_id.as_str()),
            ("ilqp", "IL QSO Party"),
            "the club runs the party, not ARRL Field Day"
        );
        assert!(club.field_day_event().is_none());
        club.merge(
            &ilqp_row(
                "aaaa",
                1,
                "K9AAA",
                "40m",
                "CW",
                "",
                "COOK",
                "MCLN",
                ILQP_START + 180,
            ),
            1,
        );
        club.merge(
            &ilqp_row(
                "aaaa",
                2,
                "K9AAA",
                "40m",
                "DIG",
                "RTTY",
                "COOK",
                "MCLN",
                ILQP_START + 240,
            ),
            2,
        );
        club.merge(
            &ilqp_row(
                "aaaa",
                3,
                "K9AAA",
                "40m",
                "CW",
                "",
                "WILL",
                "MCLN",
                ILQP_START + 300,
            ),
            3,
        );
        assert_eq!(club.qsos_raw(), 3, "every row merged");
        assert_eq!(
            club.qsos_unique(),
            2,
            "RTTY after CW is the same CW/digital mode; a new county is a new contact"
        );
        let session = party_session("ilqp", "IL", "MCLN");
        let log = club.unique_log_with("W9XYZ", session.clone());
        let qths: Vec<&str> = log.qsos().iter().map(|q| q.rcvd("QTH")).collect();
        assert_eq!(
            qths,
            ["COOK", "WILL"],
            "each contact keeps the county it sent"
        );
        let cab = club
            .export_cabrillo_with("W9XYZ", session.clone(), &CabrilloEntrant::default())
            .expect("one entry");
        assert!(cab.contains("CONTEST: ILLINOIS QSO PARTY\n"), "{cab}");
        assert!(cab.contains("CATEGORY-OPERATOR: MULTI-OP\n"), "{cab}");
        assert!(cab.contains("IL-COUNTY: McLean\n"), "{cab}");
        assert!(
            cab.contains("QSO: 7000 CW 2026-10-18 1703 W9XYZ 599 MCLN K9AAA 599 COOK\n"),
            "the exchange columns, both sides: {cab}"
        );
        assert!(
            cab.contains("QSO: 7000 CW 2026-10-18 1705 W9XYZ 599 MCLN K9AAA 599 WILL\n"),
            "the mobile's new county is its own line: {cab}"
        );
        assert_eq!(
            cab.matches("QSO:").count(),
            2,
            "the RTTY repeat is not a second contact: {cab}"
        );
        // 2 CW contacts × 2 points, × 2 counties (Cook, Will).
        let s = club.score_with("W9XYZ", session, 1, &[], &[]);
        assert_eq!((s.qso_points, s.mults, s.total), (4, Some(2), 8));
        assert!(cab.contains("CLAIMED-SCORE: 8\n"), "{cab}");
    }

    /// ⭐ **Two positions logging the same station: the second is the CLUB dupe** — kept
    /// as a row (a warning at the position, never a lock), scored once, exported once —
    /// and the key the club ships for it is the very key the position's own log builds,
    /// which is what the position's while-typing check compares against.
    #[test]
    fn the_second_position_to_work_a_station_is_the_club_dupe() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        club.merge(
            &ilqp_row(
                "aaaa",
                1,
                "K9BBB",
                "20m",
                "PH",
                "",
                "KANE",
                "MCLN",
                ILQP_START + 60,
            ),
            1,
        );
        club.merge(
            &ilqp_row(
                "bbbb",
                1,
                "K9BBB",
                "20m",
                "PH",
                "",
                "KANE",
                "MCLN",
                ILQP_START + 120,
            ),
            2,
        );
        assert_eq!(
            (club.qsos_raw(), club.qsos_unique()),
            (2, 1),
            "both rows kept, one contact"
        );
        let st = club.club_state(0, 0, 0, 3);
        assert_eq!(st.dkeys.len(), 1, "one club key");
        assert!(st.dupes.is_empty(), "no Field Day triple for a party");
        // The same contact in a position's OWN log, same contest, same station.
        let mut own = FieldDayLog::new("W9XYZ", party_session("ilqp", "IL", "MCLN"), "20m");
        assert!(own.log_fields_at(
            "K9BBB",
            &[
                ("RST".to_string(), "59".to_string()),
                ("QTH".to_string(), "KANE".to_string())
            ],
            "PH",
            "",
            0,
            ILQP_START + 120,
        ));
        assert_eq!(
            st.dkeys[0],
            own.dupe_rule().key(&own.qsos()[0]),
            "the club ships the key the position builds — so the second position is warned"
        );
        let cab = club
            .export_cabrillo_with(
                "W9XYZ",
                party_session("ilqp", "IL", "MCLN"),
                &CabrilloEntrant::default(),
            )
            .unwrap();
        assert_eq!(cab.matches("QSO:").count(), 1, "earliest wins: {cab}");
        assert!(cab.contains(" 1701 W9XYZ 59 MCLN K9BBB 59 KANE\n"), "{cab}");
        // The board credits the earlier position with the contact.
        let board = club.board_rows(10);
        let uniq = |p: &str| board.iter().find(|r| r.pos == p).map(|r| r.uniq);
        assert_eq!((uniq("aaaa"), uniq("bbbb")), (Some(1), Some(0)));
        // CONTROL: the same station from ANOTHER county is a new contact (the mobile rule).
        club.merge(
            &ilqp_row(
                "bbbb",
                2,
                "K9BBB",
                "20m",
                "PH",
                "",
                "DUPG",
                "MCLN",
                ILQP_START + 180,
            ),
            3,
        );
        assert_eq!(club.qsos_unique(), 2);
    }

    /// The party's two club stations are worth 100 each, *"added to the final score"* —
    /// once for the ENTRY, however many positions work them.
    #[test]
    fn the_partys_bonus_stations_count_once_for_the_whole_club() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        club.merge(
            &ilqp_row(
                "aaaa",
                1,
                "W9AWE",
                "40m",
                "CW",
                "",
                "MCDN",
                "MCLN",
                ILQP_START + 60,
            ),
            1,
        );
        club.merge(
            &ilqp_row(
                "bbbb",
                1,
                "W9AWE",
                "20m",
                "PH",
                "",
                "MCDN",
                "MCLN",
                ILQP_START + 120,
            ),
            2,
        );
        let s = club.score_with("W9XYZ", party_session("ilqp", "IL", "MCLN"), 1, &[], &[]);
        // CW 2 + phone 1, × one county (McDonough), + 100 once.
        assert_eq!(
            (s.qso_points, s.mults, s.bonus, s.total),
            (3, Some(1), 100, 103)
        );
    }

    /// ⭐ **The club file names who logged each contact** — the sponsor's `OPERATORS`
    /// header is everyone the rows name, plus whoever the host typed in. A position with
    /// nobody named at the key sends the station call, and that reads as nobody: the
    /// station call is never an operator (`LoggedQso::operator`).
    #[test]
    fn the_club_file_names_the_operators_its_positions_logged_under() {
        let mut club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        let mut named = ilqp_row("aaaa", 1, "K9CCC", "40m", "CW", "", "COOK", "MCLN", 100);
        named.op = "aa9xyz".into();
        club.merge(&named, 1);
        // Nobody named at the key: the wire carries the station call.
        club.merge(
            &ilqp_row("bbbb", 1, "K9DDD", "40m", "CW", "", "COOK", "MCLN", 200),
            2,
        );
        let entrant = CabrilloEntrant {
            entry_class: "UNLIMITED".into(),
            club: "WESTERN ILL AMATEUR RADIO CLUB".into(),
            operators: "W9OP1".into(),
            ..Default::default()
        };
        let cab = club
            .export_cabrillo_with("W9XYZ", party_session("ilqp", "IL", "MCLN"), &entrant)
            .unwrap();
        assert!(cab.contains("OPERATORS: AA9XYZ W9OP1\n"), "{cab}");
        assert!(cab.contains("ENTRY-CLASS: UNLIMITED\n"), "{cab}");
        assert!(
            cab.contains("CLUB: WESTERN ILL AMATEUR RADIO CLUB\n"),
            "{cab}"
        );
        assert!(
            !cab.contains("OPERATORS: W9XYZ"),
            "the station call is not an operator"
        );
    }

    /// ⭐ **A contest whose sponsor wants duplicates REPORTED keeps them in the club file.**
    /// The New York QSO Party cross-checks logs, so a repeat is logged and worth nothing
    /// rather than removed — removing it would hand the other station a not-in-log. The
    /// club does with a second position's repeat exactly what a position's own log does
    /// with its own: the later row is marked, written, and scores zero.
    #[test]
    fn a_club_whose_contest_reports_dupes_keeps_the_repeat_and_scores_it_zero() {
        let rs = party("nyqp");
        assert!(rs.dupe_rule.log_dupes, "fixture: NYQP reports its dupes");
        let mut club = ClubLog::for_ruleset(rs, "NYQP TEST");
        let row = |pos: &str, seq: u64, when: u64| WireQso {
            pos: pos.into(),
            seq,
            call: "K2AAA".into(),
            class: String::new(),
            sect: String::new(),
            ex: party_fields("nyqp", &[("RST", "599"), ("QTH", "BRX")]),
            mex: party_fields("nyqp", &[("RST", "599"), ("QTH", "ALB")]),
            band: "20m".into(),
            mode: "CW".into(),
            sub: String::new(),
            when,
            op: String::new(),
            sat: String::new(),
            sat_fm: false,
        };
        // The LATER-merged row is the earlier contact: time order decides which is marked.
        club.merge(&row("bbbb", 1, 2_000), 1);
        club.merge(&row("aaaa", 1, 1_000), 2);
        let session = party_session("nyqp", "NY", "ALB");
        let log = club.unique_log_with("W2XYZ", session.clone());
        let marks: Vec<(u64, bool)> = log.qsos().iter().map(|q| (q.when_unix, q.dupe)).collect();
        assert_eq!(
            marks,
            [(1_000, false), (2_000, true)],
            "both kept, in the order made, the later one marked"
        );
        assert_eq!(log.qso_count(), 1, "the repeat scores nothing");
        let cab = club
            .export_cabrillo_with("W2XYZ", session, &CabrilloEntrant::default())
            .unwrap();
        assert_eq!(cab.matches("QSO:").count(), 2, "both in the file: {cab}");
        // CONTROL: the Illinois QSO Party does not report dupes — its club file keeps one.
        let mut il = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        il.merge(
            &ilqp_row("bbbb", 1, "K9AAA", "20m", "CW", "", "COOK", "MCLN", 2_000),
            1,
        );
        il.merge(
            &ilqp_row("aaaa", 1, "K9AAA", "20m", "CW", "", "COOK", "MCLN", 1_000),
            2,
        );
        let cab = il
            .export_cabrillo_with(
                "W9XYZ",
                party_session("ilqp", "IL", "MCLN"),
                &CabrilloEntrant::default(),
            )
            .unwrap();
        assert_eq!(cab.matches("QSO:").count(), 1, "{cab}");
    }

    /// ⭐ **Every contest club sync runs writes, for one position's rows, the QSO lines that
    /// position's own log writes** — one exporter, under one session, whichever file it is.
    /// Run over every ruleset the club log accepts, with the outbox's own row shape.
    #[test]
    fn a_club_of_one_position_writes_that_positions_own_qso_lines_in_every_contest_it_runs() {
        // (contest, state, county, the received exchange of two different stations)
        let cases: &[(&str, &str, &str, [&str; 2])] = &[
            ("ilqp", "IL", "MCLN", ["COOK", "WILL"]),
            ("tnqp", "TN", "ANDE", ["BEDF", "BENT"]),
            ("ohqp", "OH", "ADAM", ["ALLE", "ASHL"]),
            ("txqp", "TX", "ANDE", ["ANDR", "ANGE"]),
            ("nyqp", "NY", "ALB", ["BRX", "ALL"]),
            ("arrlvhf_jan", "", "", ["FN31", "EM12"]),
            ("arrlvhf_jun", "", "", ["FN31", "EM12"]),
            ("arrlvhf_sep", "", "", ["FN31", "EM12"]),
        ];
        let mut seen = HashSet::new();
        for (event, state, county, theirs) in cases {
            let rs = party(event);
            assert_eq!(club_refusal(rs), None, "{event} is run by the club log");
            let slot = if event.starts_with("arrlvhf") {
                "GRID"
            } else {
                "QTH"
            };
            let mut own = FieldDayLog::new("W9XYZ", party_session(event, state, county), "20m");
            for (i, value) in theirs.iter().enumerate() {
                let mut fields = vec![(slot.to_string(), value.to_string())];
                if slot == "QTH" {
                    fields.insert(0, ("RST".to_string(), "599".to_string()));
                }
                let call = ["K9AAA", "N9BBB"][i];
                assert!(
                    own.log_fields_at(call, &fields, "CW", "", 0, ILQP_START + 60 * i as u64),
                    "{event}: fixture contact refused"
                );
            }
            // The outbox's own row shape (`Engine::fd_sync_outbox`).
            let mut club = ClubLog::for_ruleset(rs, "TEST");
            for q in own.qsos() {
                club.merge(
                    &WireQso {
                        pos: "aaaa".into(),
                        seq: q.seq,
                        call: q.call.clone(),
                        class: q.class().to_string(),
                        sect: q.section().to_string(),
                        ex: to_wire_fields(&q.rx),
                        mex: to_wire_fields(&q.tx),
                        band: q.band.clone(),
                        mode: q.mode.clone(),
                        sub: q.submode.clone(),
                        when: q.when_unix,
                        op: String::new(),
                        sat: String::new(),
                        sat_fm: false,
                    },
                    1,
                );
            }
            let lines = |cab: &str| -> Vec<String> {
                cab.lines()
                    .filter(|l| l.starts_with("QSO:"))
                    .map(str::to_string)
                    .collect()
            };
            let mine = lines(&own.cabrillo(0).unwrap());
            let club_cab = club
                .export_cabrillo_with(
                    "W9XYZ",
                    party_session(event, state, county),
                    &CabrilloEntrant::default(),
                )
                .unwrap();
            assert_eq!(lines(&club_cab), mine, "{event}: the club's QSO lines");
            assert_eq!(mine.len(), 2, "{event}: both contacts");
            for v in theirs {
                assert!(club_cab.contains(v), "{event}: {v} on a line: {club_cab}");
            }
            let token = tempo_core::contest::cabrillo_contest_token(rs.contest_id).to_string();
            assert!(
                club_cab.contains(&format!("CONTEST: {token}\n")),
                "{event}: the contest's own token: {club_cab}"
            );
            seen.insert(*event);
        }
        // Every ruleset the club log runs is in the table above, Field Day aside (its
        // goldens pin it byte for byte) — a contest added to the seed must be run here.
        for event in seeded_events() {
            if club_refusal(party(&event)).is_none() && !is_field_day_id(&event) {
                assert!(
                    seen.contains(event.as_str()),
                    "{event} is run by the club log and not tested here"
                );
            }
        }
    }

    /// ⭐ **The screen mirrors the contests club sync refuses.** `CLUB_SYNC_REFUSED` in
    /// ui/src/fdEvent.ts must hold, for every seeded ruleset the club log cannot run, the
    /// reason [`club_refusal`] gives — and nothing else — or the screen and the sockets
    /// disagree: a "Not syncing" note over a club that runs, or silence over one that does
    /// not.
    #[test]
    fn the_screen_mirrors_the_contests_club_sync_refuses() {
        let ts = include_str!("../../../ui/src/fdEvent.ts");
        let block = ts
            .split("export const CLUB_SYNC_REFUSED")
            .nth(1)
            .expect("the table is in fdEvent.ts")
            .split("\n}")
            .next()
            .unwrap();
        let mirrored: HashMap<String, String> = block
            .lines()
            .filter_map(|l| {
                let (id, why) = l.trim().split_once(':')?;
                let id = id.trim();
                let ok = !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
                ok.then(|| {
                    let why = why.trim().trim_end_matches(',').trim_matches('\'');
                    (id.to_string(), why.to_string())
                })
            })
            .collect();
        assert!(
            mirrored.len() >= 8,
            "parsed only {} entries — the parser is broken, not the table",
            mirrored.len()
        );
        let engine: HashMap<String, String> = seeded_events()
            .into_iter()
            .filter_map(|e| club_refusal(party(&e)).map(|r| (e, r.code().to_string())))
            .collect();
        assert_eq!(mirrored, engine, "ui/src/fdEvent.ts CLUB_SYNC_REFUSED");
        // POSITIVE CONTROL: the comparison sees one entry missing.
        let mut short = mirrored.clone();
        short.remove("cqp");
        assert_ne!(short, engine);
    }

    /// ⭐ **The log strip builds the key the engine builds** —
    /// `ui/src/features/__fixtures__/contest-dupe-keys.json` is one table of contacts and
    /// their dupe keys, read here against the engine's own rule (from the rules seed) and its
    /// own key builder, and in `contestDupe.keys.shared.test.ts` against the strip's. The
    /// table's rules must be the seed's too, or the strip would be checked against a rule
    /// no contest runs. The club's while-typing warning compares exactly these keys.
    #[test]
    fn the_strips_dupe_key_table_is_the_engines() {
        let table: serde_json::Value = serde_json::from_str(include_str!(
            "../../../ui/src/features/__fixtures__/contest-dupe-keys.json"
        ))
        .expect("the table is JSON");
        let strs = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .expect("a list")
                .iter()
                .map(|s| s.as_str().expect("a string").to_string())
                .collect()
        };
        let rules = table["rules"].as_object().expect("rules");
        for (event, r) in rules {
            let d = party(event).dupe_rule;
            assert_eq!(r["byCall"].as_bool(), Some(d.by_call), "{event} byCall");
            assert_eq!(r["byBand"].as_bool(), Some(d.by_band), "{event} byBand");
            assert_eq!(
                r["byModeClass"].as_bool(),
                Some(d.by_mode_class),
                "{event} byModeClass"
            );
            assert_eq!(
                r["logDupes"].as_bool(),
                Some(d.log_dupes),
                "{event} logDupes"
            );
            assert_eq!(strs(&r["byFields"]), d.by_fields, "{event} byFields");
            assert_eq!(
                strs(&r["bySentFields"]),
                d.by_sent_fields,
                "{event} bySentFields"
            );
            let groups: Vec<Vec<String>> = r["modeClassGroups"]
                .as_array()
                .expect("groups")
                .iter()
                .map(&strs)
                .collect();
            let seed: Vec<Vec<String>> = d
                .mode_class_groups
                .iter()
                .map(|g| g.iter().map(|m| m.to_string()).collect())
                .collect();
            assert_eq!(groups, seed, "{event} modeClassGroups");
        }
        let rows = table["rows"].as_array().expect("rows");
        assert!(rows.len() >= 9, "parsed only {} rows", rows.len());
        for row in rows {
            let event = row["ruleset"].as_str().unwrap();
            let rs = party(event);
            let side = |v: &serde_json::Value| -> Vec<FieldValue> {
                v.as_object()
                    .expect("an exchange")
                    .iter()
                    .filter_map(|(k, raw)| rs.exchange.copied(k, raw.as_str().unwrap()))
                    .collect()
            };
            let key = rs.dupe_rule.key_of(
                row["call"].as_str().unwrap(),
                row["band"].as_str().unwrap(),
                row["mode"].as_str().unwrap(),
                &side(&row["rx"]),
                &side(&row["tx"]),
                tempo_core::contest::SatKey::default(),
            );
            assert_eq!(key, strs(&row["key"]), "{event} row {row}");
        }
    }

    /// Every contest the bundled rules table carries, by rules-file id, read from the
    /// seed itself so a contest added there is seen here.
    fn seeded_events() -> Vec<String> {
        let seed: serde_json::Value =
            serde_json::from_str(include_str!("../../tempo-core/src/fd_rules.seed.json"))
                .expect("the seed is JSON");
        let events: Vec<String> = seed["rulesets"]
            .as_array()
            .expect("the seed lists rulesets")
            .iter()
            .filter_map(|r| r["event"].as_str().map(str::to_string))
            .collect();
        assert!(events.len() >= 18, "parsed only {} rulesets", events.len());
        events
    }

    /// ⭐ **The contests the club log cannot run, and why** — decided from each ruleset's
    /// own data. A serial-number exchange must run in one sequence for the whole entry;
    /// a template with a transmitter column must say which transmitter made each
    /// contact. Everything else runs.
    #[test]
    fn the_contests_club_sync_refuses_are_the_serial_and_transmitter_ones() {
        use ClubRefusal::*;
        let expect: &[(&str, Option<ClubRefusal>)] = &[
            ("arrlfd", None),
            ("wfd", None),
            ("ilqp", None),
            ("tnqp", None),
            ("ohqp", None),
            ("txqp", None),
            ("nyqp", None),
            ("arrlvhf_jan", None),
            ("arrlvhf_jun", None),
            ("arrlvhf_sep", None),
            ("cqp", Some(Serial)),
            ("arrlss_cw", Some(Serial)),
            ("arrlss_ssb", Some(Serial)),
            ("cqwpx_cw", Some(Serial)),
            ("cqwpx_ssb", Some(Serial)),
            ("cqww_cw", Some(TransmitterColumn)),
            ("cqww_ssb", Some(TransmitterColumn)),
            ("cqww_rtty", Some(TransmitterColumn)),
        ];
        for (event, want) in expect {
            assert_eq!(club_refusal(party(event)), *want, "{event}");
        }
    }

    /// ⭐ **A position logging a different contest is refused at JOIN, by name** — the
    /// club and the position's contest both named, and what to do about it. A position too
    /// old to name one is served by a Field Day club as it always was, and refused by any
    /// other.
    #[test]
    fn a_position_logging_another_contest_is_refused_by_name() {
        let v = tempo_net::fdsync::PROTO_VERSION;
        let party_club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        assert_eq!(
            party_club.join_refusal(v, "ilqp"),
            None,
            "same contest: served"
        );
        let msg = party_club
            .join_refusal(v, "arrlfd")
            .expect("a Field Day position is refused by a party club");
        assert_eq!(
            msg,
            "this club is running IL QSO Party, and this Nexus is logging ARRL-FIELD-DAY. \
             Pick IL QSO Party as the contest on the Contesting tab in Settings, then \
             rejoin. Contacts you log meanwhile stay in your own log."
        );
        let old = party_club
            .join_refusal(v, "")
            .expect("a position too old to name its contest cannot join a party club");
        assert!(
            old.contains("IL QSO Party") && old.contains("too old"),
            "{old}"
        );
        // Field Day: an older position is served as before; the OTHER Field Day is not.
        let fd = ClubLog::new(FdEvent::ArrlFd, "TEST FD");
        assert_eq!(
            fd.join_refusal(v, ""),
            None,
            "an older position joins Field Day as before"
        );
        assert_eq!(fd.join_refusal(1, ""), None, "…a v1 one too");
        assert_eq!(fd.join_refusal(v, "arrlfd"), None);
        let wfd = fd
            .join_refusal(v, "wfd")
            .expect("Winter Field Day is another contest");
        assert!(
            wfd.contains("ARRL-FIELD-DAY") && wfd.contains("WFD"),
            "{wfd}"
        );
        // The version rule still comes first, with its own words.
        assert_eq!(
            party_club.join_refusal(1, "ilqp"),
            party_club.version_refusal(1)
        );
    }

    /// ⭐ **A position on another station call than the host's is refused at JOIN, by name**
    /// — both calls named, and where to set the position's. The calls are compared trimmed
    /// and case-blind, and nothing more: a portable suffix is another call on the air. A
    /// call either end cannot say is served as before.
    #[test]
    fn a_position_on_another_call_is_refused_by_name_with_where_to_set_it() {
        let party_club = ClubLog::for_ruleset(party("ilqp"), "ILQP TEST");
        assert_eq!(
            party_club.call_refusal("W9XYZ", " k9abc "),
            Some(
                "this club is on the air as W9XYZ and this Nexus as K9ABC, and in IL QSO Party \
                 every position of a club entry sends the club's call. Set Callsign on the air \
                 under Who's who at this event on the Contesting tab in Settings to W9XYZ, then \
                 turn Field Day mode off and on again: this Nexus rejoins by itself. Contacts \
                 you log meanwhile stay in your own log."
                    .to_string()
            )
        );
        let portable = party_club
            .call_refusal("W9XYZ", "W9XYZ/P")
            .expect("W9XYZ/P is not the club's call on the air");
        assert!(portable.contains("this Nexus as W9XYZ/P,"), "{portable}");
        // CONTROLS: the club's own call joins however it is typed, and a call either end
        // cannot say joins as before.
        assert_eq!(party_club.call_refusal("W9XYZ", "W9XYZ"), None);
        assert_eq!(party_club.call_refusal("W9XYZ", " w9xyz "), None);
        assert_eq!(party_club.call_refusal("W9XYZ", ""), None);
        assert_eq!(party_club.call_refusal("W9XYZ", "  "), None);
        assert_eq!(party_club.call_refusal("", "K9ABC"), None);
    }

    /// ⭐ **Only ARRL Field Day serves a position on another call** — its GOTA station must
    /// use a call of its own (rule 4.1.1.1). Every other contest the club log runs refuses
    /// one, each by its own name: Winter Field Day, the QSO parties and the VHF contests.
    #[test]
    fn only_arrl_field_day_serves_a_position_on_another_call() {
        let (mut served, mut refused) = (Vec::new(), Vec::new());
        for event in seeded_events() {
            let rs = party(&event);
            if club_refusal(rs).is_some() {
                continue;
            }
            match ClubLog::for_ruleset(rs, "TEST").call_refusal("W9XYZ", "K9GOT") {
                None => served.push(event),
                Some(msg) => {
                    assert!(
                        msg.contains(&format!("in {} every", rs.contest_id)),
                        "{event}: {msg}"
                    );
                    refused.push(event);
                }
            }
        }
        assert_eq!(served, vec!["arrlfd".to_string()]);
        for event in ["wfd", "ilqp", "nyqp", "tnqp", "ohqp", "txqp", "arrlvhf_jun"] {
            assert!(refused.iter().any(|e| e == event), "{event}: {refused:?}");
        }
    }

    /// ⭐ **Field Day through the any-ruleset constructor is the Field Day club it always
    /// was** — the 1.x journal replayed into a club built by `for_ruleset` exports the
    /// bytes 1.x exported, and the generic score is the shipped one.
    #[test]
    fn a_field_day_club_built_for_its_ruleset_is_byte_identical_to_1x() {
        let dir = scratch("j1x-ruleset");
        let path = dir.join("fd_event_granite.jsonl");
        std::fs::write(&path, J1X).unwrap();
        let mut host = ClubLog::for_ruleset(party("arrlfd"), "GRANITE ARC FD");
        assert_eq!(host.field_day_event(), Some(FdEvent::ArrlFd));
        host.attach_journal_since(&path, 0).unwrap();
        assert_eq!(host.export_cabrillo("W9ABC", "3A", "WI").unwrap(), J1X_CBR);
        assert_eq!(host.export_adif("W9ABC", "3A", "WI"), J1X_ADI);
        let session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        assert_eq!(
            host.export_cabrillo_with(
                "W9ABC",
                session.clone(),
                &CabrilloEntrant {
                    name: "A NAME".into(),
                    email: "a@example.com".into(),
                    club: "A CLUB".into(),
                    entry_class: "UNLIMITED".into(),
                    operators: "W9OP1".into(),
                    objectives: vec!["wfd-qrp".into()],
                },
            )
            .unwrap(),
            J1X_CBR,
            "the entrant's lines are written only where the ruleset lists them — never for Field Day"
        );
        assert_eq!(host.export_adif_with("W9ABC", session.clone()), J1X_ADI);
        let s = host.score_with("W9ABC", session, 2, &[], &[]);
        assert_eq!(
            (s.qso_points, s.powered, s.bonus, s.total),
            host.scored("W9ABC", "3A", "WI", 2, &[]),
        );
        assert_eq!(s.mults, None, "Field Day has no multiplier");
        let wfd = ClubLog::for_ruleset(party("wfd"), "WFD");
        assert_eq!(
            (wfd.event, wfd.contest_id.as_str()),
            (FdEvent::WinterFd, "WFD")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
