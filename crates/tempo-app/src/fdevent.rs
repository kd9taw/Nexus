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
//! has always been (`FieldDayLog::log_submode_at` → false).
//!
//! Pure logic, no sockets — unit-testable. Engine wiring: `Engine::fd_club_*`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use tempo_core::contest::{carrier, ContestSession, DupeRule, ExchangeSpec, FieldValue};
use tempo_core::fieldday::{FdEvent, FieldDayLog};
use tempo_net::fdsync::{ClubState, PosReport, WireBoardRow, WireField, WireQso};

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
}

/// The host-side merged club log. See the module header for the invariants.
#[derive(Debug, Default)]
pub struct ClubLog {
    /// Which event this club is running (scoring + export ids).
    pub event: FdEvent,
    /// The Cabrillo token of the contest this club is running.
    ///
    /// It reads [`FdEvent::contest_id`] today, because a club can only run a Field
    /// Day event in this build — the session that will set it independently is
    /// batch 5's. It is here now because the JOIN gate needs it: whether an older
    /// position may be served is a question about the CONTEST, and the refusal has
    /// to name it.
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
    pub fn new(event: FdEvent, event_name: &str) -> Self {
        ClubLog {
            event,
            contest_id: event.contest_id().to_string(),
            event_name: event_name.to_string(),
            ..Default::default()
        }
    }

    /// The exchange this club's rows are read against, and the rule its dupe keys
    /// are built by. Both come from the event because a club can only run a Field
    /// Day event in this build (see [`contest_id`](Self::contest_id)).
    fn exchange(&self) -> &'static ExchangeSpec {
        tempo_core::contest::field_day(self.event)
    }

    fn dupe_rule(&self) -> DupeRule {
        tempo_core::fd_rules::ruleset(self.event, tempo_core::fd_rules::CURRENT_RULES_YEAR)
            .dupe_rule
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
        let dkey = row.dkey(&self.dupe_rule(), self.exchange());
        if self.dkeys_set.insert(dkey.clone()) {
            self.dkeys_list.push(dkey);
            // Index-parallel, always built, sent only for a Field Day club.
            self.dupes_list.push(row.dupe_key());
        }
        let sect = row.section.trim().to_uppercase();
        if !sect.is_empty() && self.sections_set.insert(sect.clone()) {
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
        pos.last_seen_unix = now;
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
        let mut seen: HashSet<Vec<String>> = HashSet::new();
        let mut keep: Vec<usize> = Vec::new();
        for i in order {
            if seen.insert(self.rows[i].dkey(&rule, spec)) {
                keep.push(i);
            }
        }
        keep.sort_unstable(); // back to log order
        keep
    }

    /// The deduped (earliest-wins) club log as a [`FieldDayLog`] under the
    /// HOST's station identity — the one artifact both exports and the score
    /// derive from, so they can never disagree with each other.
    pub fn unique_log(&self, mycall: &str, class: &str, section: &str) -> FieldDayLog {
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
        let mut session = ContestSession::field_day(self.event, class, section);
        // ⭐ A CLUB log IS a multi-operator entry, and this is the ONE place that is
        // true by construction: these rows were worked at several positions under one
        // callsign, which is exactly what `CATEGORY-OPERATOR: MULTI-OP` declares. The
        // session's default is `SINGLE-OP` because a lone operator is the default
        // user; a host merging positions is not that operator, and it says so here
        // rather than leaving the header to a picker nobody at a field site touched.
        session.entry_category = tempo_core::contest::OperatorCategory::MultiOp;
        let mut log = FieldDayLog::new(mycall, session, "");
        for i in self.earliest_unique_indices() {
            let r = &self.rows[i];
            log.band = r.band.clone();
            let rx = from_wire_fields(&r.ex, spec);
            let tx = if r.mex.is_empty() {
                log.session.my_exchange.clone()
            } else {
                from_wire_fields(&r.mex, spec)
            };
            // Never refused: the indices are already key-unique.
            log.log_exchange_at(&r.call, rx, tx, &r.mode_class, &r.submode, 0, r.when_unix);
        }
        log
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
        let rs =
            tempo_core::fd_rules::ruleset(self.event, tempo_core::fd_rules::CURRENT_RULES_YEAR);
        let log = self.unique_log(mycall, class, section);
        let (qso_pts, powered) = rs.scoring.qso_and_powered(log.score_rows(), power_mult);
        let bonus = rs.bonus_points(bonuses);
        (qso_pts, powered, bonus, powered + bonus)
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
    /// Local minus host clock at the last welcome (warn > 30 s, NEVER adjust).
    pub skew_secs: i64,
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
        self.last_error = None;
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

    /// One presence report, dial fixed — these tests are about the name.
    fn report(name: &str, band: &str, mode: &str, op: &str) -> PosReport {
        PosReport {
            band: band.into(),
            mode: mode.into(),
            op: op.into(),
            freq: 14_032_100,
            name: name.into(),
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

    /// A REAL 1.x host journal — twelve NDJSON rows written by the shipped
    /// `MergedRow` (byte-identical between `main` and this branch's base), three
    /// positions, all three mode classes, two digital submodes, one unrecorded
    /// operator and one cross-position dupe — with the Cabrillo and ADIF that
    /// build exported from it.
    ///
    /// ⚠️ The `.cbr`'s line 2 moved once since capture, deliberately:
    /// `CONTEST: ARRL-FIELD-DAY` → `CONTEST: ARRL-FD` (694 bytes to 687), because
    /// that header is a Cabrillo token and `ARRL-FIELD-DAY` is the ADIF one. See
    /// the module header of `tempo-core/tests/fd_goldens.rs`. The `.adi` did NOT
    /// move — its `CONTEST_ID` is the ADIF value and stays that way.
    const J1X: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.jsonl");
    const J1X_CBR: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.cbr");
    const J1X_ADI: &str = include_str!("../tests/fixtures/fd-1x-journal/fd_event_1x.adi");

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
}
