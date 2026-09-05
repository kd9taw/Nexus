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
use tempo_core::fieldday::{Exchange, FdEvent, FieldDayLog};
use tempo_net::fdsync::{ClubState, PosReport, WireBoardRow, WireQso};

/// One merged club-log row — the design's reconciled shape (also what the
/// scoreboard seam's `FdBoardRow` mirrors). Serialized one-per-line into the
/// host's event journal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MergedRow {
    /// Position id (8-hex, per MACHINE, not per seat).
    pub posid: String,
    /// Per-position monotonic seq — `(posid, seq)` is the row's identity.
    pub seq: u64,
    pub call: String,
    pub class: String,
    pub section: String,
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
            band: q.band.clone(),
            mode_class: q.mode.clone(),
            submode: q.sub.clone(),
            when_unix: q.when,
            operator: q.op.clone(),
        }
    }

    /// The club dupe key — EXACTLY `FieldDayLog.worked`'s
    /// `(CALL, band, MODE CLASS)` shape, so the position-side union check
    /// agrees with the own-log one byte for byte.
    pub fn dupe_key(&self) -> (String, String, String) {
        (
            self.call.to_uppercase(),
            self.band.clone(),
            self.mode_class.to_ascii_uppercase(),
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
    /// The operator-facing event name (beacon + welcome).
    pub event_name: String,
    rows: Vec<MergedRow>,
    /// `(posid, seq)` → merged (the idempotence index).
    ids: HashSet<(String, u64)>,
    /// Club dupe keys in FIRST-SEEN ORDER — append-only, so a down-flow delta
    /// is "everything past your cursor" and cursors never invalidate.
    dupes_list: Vec<(String, String, String)>,
    dupes_set: HashSet<(String, String, String)>,
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
            event_name: event_name.to_string(),
            ..Default::default()
        }
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
    fn merge_row(&mut self, row: MergedRow, now: u64) -> bool {
        let id = (row.posid.clone(), row.seq);
        if row.seq == 0 || !self.ids.insert(id) {
            return false; // seq 0 is "never assigned" — refuse, don't guess
        }
        let key = row.dupe_key();
        if self.dupes_set.insert(key.clone()) {
            self.dupes_list.push(key);
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

    /// The append-only cursors: (dupe keys, sections) totals.
    pub fn counts(&self) -> (usize, usize) {
        (self.dupes_list.len(), self.sections_list.len())
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
        let mut seen: HashSet<(String, String, String)> = HashSet::new();
        let mut keep: Vec<usize> = Vec::new();
        for i in order {
            if seen.insert(self.rows[i].dupe_key()) {
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
        let mut log = FieldDayLog::new(mycall, Exchange::new(class, section), "");
        log.event = self.event;
        for i in self.earliest_unique_indices() {
            let r = &self.rows[i];
            log.band = r.band.clone();
            // Never refused: the indices are already key-unique.
            log.log_submode_at(
                &r.call,
                &r.class,
                &r.section,
                &r.mode_class,
                &r.submode,
                0,
                r.when_unix,
            );
        }
        log
    }

    /// Unique (scoring) rows count.
    pub fn qsos_unique(&self) -> u64 {
        self.dupes_list.len() as u64
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
        let (qso_pts, powered) = rs.scoring.qso_and_powered(&log, power_mult);
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
            dupes: self.dupes_list.get(dupes_from..).unwrap_or(&[]).to_vec(),
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
    pub fn export_cabrillo(&self, mycall: &str, class: &str, section: &str) -> String {
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
    /// Club dupe keys — the while-typing verdict unions these with own log.
    pub dupes: HashSet<(String, String, String)>,
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
            self.sections.clear();
        }
        for k in &st.dupes {
            self.dupes.insert(k.clone());
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
            class: "2A".into(),
            sect: sect.into(),
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
        let cab = club.export_cabrillo("W9ABC", "3A", "WI");
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
            sections: vec!["CT".into()],
            score: 10,
            qsos: 1,
            board: vec![],
        });
        m.apply(&ClubState {
            reset: false,
            dupes: vec![("K1ABC".into(), "40m".into(), "CW".into())],
            sections: vec!["EMA".into()],
            score: 14,
            qsos: 2,
            board: vec![],
        });
        assert_eq!(m.dupes.len(), 2, "deltas union");
        assert_eq!((m.score, m.qsos), (14, 2), "scalars overwrite");
        // A rejoin snap (host restarted into a new event) CLEARS before applying.
        m.apply(&ClubState {
            reset: true,
            dupes: vec![("N0XYZ".into(), "20m".into(), "PH".into())],
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
}
