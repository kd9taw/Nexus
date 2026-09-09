//! ARRL Field Day mode: the Class+Section exchange, an auto-sequencer that runs
//! operator-initiated two-way contacts, a dupe-checked log with scoring, and
//! ADIF / Cabrillo export.
//!
//! ⚠️ **Field Day has no section multiplier**, whatever this comment said until
//! 2026-09-08. The ARRL FD score is QSO points × the power tier plus claimed
//! bonuses — nothing in [`fd_rules`](crate::fd_rules) reads a section — and
//! [`FieldDayLog::sections`] is a display count, not a score input.
//!
//! Field Day requires operator-initiated contacts (no fully-automated QSOs), and
//! the exchange is **Class + ARRL/RAC Section** (e.g. `3A WI`). FT1 carries this
//! natively in one frame: `<to> <de> <class> <section>` (and the rogered
//! `<to> <de> R <class> <section>`).

use crate::message::Msg;
use modes::Decode;
use std::collections::HashSet;

/// Which Field Day event is running — they share the exchange SHAPE
/// (designator + ARRL/RAC section) but differ in designator grammar, contest
/// ids, and scoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FdEvent {
    /// ARRL Field Day (4th full June weekend): class like `3A`, phone 1 pt,
    /// CW/digital 2 pts, power multiplier, ~100-pt bonus menu.
    #[default]
    ArrlFd,
    /// Winter Field Day (last full January weekend): category like `2O`
    /// (count + Home/Indoor/Mobile/Outdoor).
    WinterFd,
}

impl FdEvent {
    pub fn contest_id(self) -> &'static str {
        match self {
            FdEvent::ArrlFd => "ARRL-FIELD-DAY",
            FdEvent::WinterFd => "WFD",
        }
    }
    /// The rules-file `event` id for this event — the inverse of
    /// [`from_code`](Self::from_code), and the key
    /// [`fd_rules::ruleset`](crate::fd_rules::ruleset) looks a ruleset up by.
    ///
    /// ⭐ A rules file's `event` is a plain id, not an `FdEvent`: the rules table
    /// carries contests this enum has no arm for (the state QSO parties), and giving
    /// each one an `FdEvent` arm would put every non-exhaustive match in the tree on
    /// the critical path of adding a contest. `FdEvent` stays what it is — the two
    /// Field Day events the app has dedicated behaviour for — and this is the one
    /// place the two vocabularies meet.
    pub fn code(self) -> &'static str {
        match self {
            FdEvent::ArrlFd => "arrlfd",
            FdEvent::WinterFd => "wfd",
        }
    }
    /// Every event this enum can name. The rules floor is derived from it (see
    /// [`fd_rules`](crate::fd_rules)), because these are exactly the events looked up
    /// through an INFALLIBLE accessor.
    pub const ALL: [FdEvent; 2] = [FdEvent::ArrlFd, FdEvent::WinterFd];
    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("wfd") {
            FdEvent::WinterFd
        } else {
            FdEvent::ArrlFd
        }
    }
    /// The inverse of [`contest_id`](Self::contest_id) — how a log recovers its event
    /// from the session it was opened with, so the two can never disagree.
    pub fn from_contest_id(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("WFD") {
            FdEvent::WinterFd
        } else {
            FdEvent::ArrlFd
        }
    }
}

/// Per-QSO points by operating mode class (both events: phone 1, CW/digital 2 —
/// the long-standing ARRL FD values; WFD currently matches for the base QSO
/// point, with its own multiplier system handled at the score layer).
pub fn qso_points_for_mode(mode: &str) -> u32 {
    match mode.to_ascii_uppercase().as_str() {
        "PH" | "PHONE" | "SSB" | "FM" => 1,
        _ => 2, // CW + digital
    }
}

/// A logged contest contact.
///
/// ⭐ **Both sides of the exchange live HERE, per row, and that is the whole of the
/// mobile fix.** `class`/`section` used to be the received half and there was no sent
/// half at all: the log carried ONE `Exchange` and `cabrillo()` wrote it on every QSO
/// line, so an operator who changed county re-labelled every contact made before the
/// change. A datum belongs on the row if it can change while a session is open, and the
/// sent exchange, the issued serial and the role all can.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedQso {
    pub call: String,
    /// The exchange THEY sent me. For Field Day this is class + section, which is
    /// exactly what the `class`/`section` pair held.
    pub rx: Vec<crate::contest::FieldValue>,
    /// The exchange I sent THEM, **as sent on this contact** — including the serial
    /// that was issued to it. Copied from the session at log time and never re-read
    /// from it afterwards.
    pub tx: Vec<crate::contest::FieldValue>,
    /// The [`RoleSpec`](crate::contest::RoleSpec) id this contact was worked under
    /// (`""` for a symmetric contest, which is both Field Day events).
    ///
    /// Carried per row because crossing a state line changes which role I am, and a
    /// row worked before the crossing must keep the one it was worked under. A session
    /// refuses that transition rather than mixing two roles in one log (§3.3 ruling 3);
    /// this field is what makes a mixed log DETECTABLE instead of aspirational.
    pub role: String,
    /// The DXCC/WAE entity for [`call`](Self::call), resolved once at log time rather
    /// than at export time.
    ///
    /// ⭐ Filled from [`contest::resolve_call`](crate::contest::resolve_call), which the
    /// composition root installs over the cty.dat resolver in `crates/propagation` — a
    /// crate that depends on this one, so this crate can never call it directly. `None`
    /// in a build with no resolver installed, and for a call the country file cannot
    /// place. Read by [`MultSource::DxccEntity`](crate::contest::MultSource::DxccEntity)
    /// (CQ WW's country multiplier) and, with [`continent`](Self::continent), by
    /// [`PointsRule::ByRelation`](crate::contest::PointsRule::ByRelation).
    pub entity: Option<String>,
    /// The continent code for [`call`](Self::call) — `AF`/`AS`/`EU`/`NA`/`OC`/`SA` —
    /// resolved with [`entity`](Self::entity) and from the same answer, so the two can
    /// never come from different readings of the same call.
    pub continent: Option<String>,
    /// The CQ WPX prefix for [`call`](Self::call), resolved once at log time by
    /// [`wpx_prefix`](crate::contest::wpx_prefix) — the sponsor's own §V.C.1 rule, not a
    /// country-file lookup, so it is filled in every build. `None` for a row whose call
    /// cannot be read as one.
    pub prefix: Option<String>,
    pub band: String,
    /// Mode class for scoring + per-band-mode dupes: "DIG" | "CW" | "PH".
    pub mode: String,
    /// The ACTUAL on-air mode behind a "DIG" class (ADIF name, uppercase:
    /// "FT8", "RTTY", "SSTV"…). Empty = not recorded (legacy rows) — exports
    /// fall back to the historical class map. WFD bans the WSJT modes but
    /// allows RTTY/SSTV, so an export must never claim "FT8" for an RTTY QSO.
    pub submode: String,
    pub slot: u64,
    /// Unix seconds when the contact was logged — Cabrillo requires a real
    /// `yyyy-mm-dd hhmm` per QSO line (the old `----------` placeholder would
    /// FAIL ARRL submission). 0 in legacy/test paths = falls back to the
    /// placeholder rather than inventing a date.
    pub when_unix: u64,
    /// Per-position monotonic sequence number — this contact's half of the
    /// club-sync QSO id `(position id, seq)`. Stamped by
    /// [`FieldDayLog::log_submode_at`] at log time (every entry point funnels
    /// there), journaled as `APP_NEXUS_QSEQ`, and restored by
    /// [`FieldDayLog::merge_adif`]. Deliberately clock-free: positions' clocks
    /// disagree at a generator-powered site, so ids never derive from time.
    /// 0 = never assigned (rows built by paths that predate sync).
    pub seq: u64,
}

impl LoggedQso {
    /// The ADIF mode name behind this row's scoring class — the actual on-air submode
    /// when one was recorded, and the historical class map for a legacy row that has
    /// none.
    ///
    /// One function because **two emitters must not disagree about what mode a contact
    /// was worked on**: the contest ADIF journal and the general-log merge both answer
    /// this question, and a row exported as RTTY in one and FT8 in the other is a
    /// contact the operator cannot reconcile afterwards.
    pub fn recorded_mode(&self) -> &str {
        if self.submode.is_empty() {
            match self.mode.as_str() {
                "CW" => "CW",
                "PH" => "SSB",
                _ => "FT8",
            }
        } else {
            self.submode.as_str()
        }
    }

    /// One slot of what THEY sent me (`""` when the row does not carry it).
    pub fn rcvd(&self, key: &str) -> &str {
        field_raw(&self.rx, key)
    }

    /// One slot of what I sent THEM (`""` when the row does not carry it).
    ///
    /// A single slot, from the ROW — never an exchange rendered from a session. An
    /// emitter that wants the whole sent exchange calls
    /// [`contest::sent_exchange`](crate::contest::sent_exchange), which also takes a row.
    pub fn sent(&self, key: &str) -> &str {
        field_raw(&self.tx, key)
    }

    /// The class they sent — Field Day's name for [`rcvd("CLASS")`](Self::rcvd).
    ///
    /// A convenience for the Field Day consumers that have not been generalised yet
    /// (the club sync, the scoreboard, the interop emitters). It reads the ROW, which
    /// is the direction that matters; the batch that generalises those surfaces reads
    /// `rx` by slot id instead and this goes away with the last caller.
    pub fn class(&self) -> &str {
        self.rcvd("CLASS")
    }

    /// The section they sent — Field Day's name for [`rcvd("SECTION")`](Self::rcvd).
    pub fn section(&self) -> &str {
        self.rcvd("SECTION")
    }
}

fn field_raw<'a>(vals: &'a [crate::contest::FieldValue], key: &str) -> &'a str {
    vals.iter()
        .find(|v| v.key == key)
        .map(|v| v.raw.as_str())
        .unwrap_or("")
}

/// A dupe-checked contest log with scoring — and **the log IS the session**: its
/// lifetime is the session's, so its rows need no per-row session id.
#[derive(Debug)]
pub struct FieldDayLog {
    pub mycall: String,
    /// The run of the contest this log belongs to.
    ///
    /// ⭐ **Replaces `myexch: Exchange`, which was one sent exchange for the whole
    /// log.** The session holds the exchange being sent RIGHT NOW; what a given contact
    /// sent is on that contact's row. `cabrillo()` reads the row, and nothing
    /// downstream reads this to describe a past contact.
    pub session: crate::contest::ContestSession,
    pub band: String,
    pub event: FdEvent,
    /// The ACTUAL on-air digital mode currently keyed (ADIF-style name, e.g.
    /// "FT8", "FT4"), stamped by the engine at FD entry and on every tier
    /// change — the funnel that fills [`LoggedQso::submode`] for the digital
    /// sequencer's own [`log`](Self::log) calls, so a WFD RTTY/FT4 contact is
    /// never exported or pushed as "FT8". Applies to the "DIG" class only:
    /// CW/PH manual entries ARE their on-air mode and keep an empty submode.
    pub current_submode: String,
    qsos: Vec<LoggedQso>,
    /// The dupe index, keyed by the ruleset's own [`DupeRule`](crate::contest::DupeRule)
    /// as an ordered `Vec<String>` rather than by a `(call, band, mode)` tuple — a
    /// mobile in a new county is a new station, and a tuple cannot say so.
    worked: HashSet<Vec<String>>,
    /// ⭐ **The `constant_sent` warning raised at LOG time** (§6.3) — `Some` once a
    /// contact has been logged whose sent value for a slot the role declares constant
    /// disagrees with the log's first row.
    ///
    /// It is stored rather than returned because the log path's answer is already
    /// "logged / dupe" and the guard **warns, never refuses**: the software cannot
    /// know whether the old rows or the new one carry the typo, so it must not throw
    /// away a real contact on a guess. The surface reads it and shows it; clearing it
    /// is the surface's business ([`clear_constant_sent_warning`](Self::clear_constant_sent_warning)).
    constant_sent_warning: Option<crate::contest::ConstantSentMismatch>,
    /// The next [`LoggedQso::seq`] to stamp. Starts at 1; a journal restore
    /// advances it past every restored seq so a fresh session's rows continue
    /// the per-position monotonic sequence instead of colliding with rows the
    /// club host already merged.
    next_seq: u64,
}

impl FieldDayLog {
    /// A log for one run of one contest.
    ///
    /// [`event`](Self::event) is taken from the session rather than defaulted, so the
    /// two cannot disagree about which sponsor's rules are running — the class letter
    /// sets are disjoint, and a log whose event says ARRL while its exchange says
    /// Winter would validate `2O` against `ABCDEF`.
    pub fn new(mycall: &str, session: crate::contest::ContestSession, band: &str) -> Self {
        let event = FdEvent::from_contest_id(&session.contest_id);
        Self {
            mycall: mycall.to_string(),
            session,
            band: band.to_string(),
            event,
            current_submode: String::new(),
            qsos: Vec::new(),
            worked: HashSet::new(),
            constant_sent_warning: None,
            next_seq: 1,
        }
    }

    /// ⭐ **The ruleset governing this log — found through the SESSION's rules-file
    /// event id**, which is the only key that can name a contest that is not one of the
    /// two Field Day events.
    ///
    /// [`event`](Self::event) cannot: it is an [`FdEvent`], a two-arm enum, and
    /// `FdEvent::from_contest_id` maps everything that is not `WFD` onto ARRL Field
    /// Day. A Tennessee QSO Party log read through it would take ARRL Field Day's dupe
    /// rule (no exchange slots — so a mobile in a new county would read as a dupe), its
    /// points table and its Cabrillo id, and nothing anywhere would say so.
    ///
    /// The `unwrap_or_else` is not a fallback in the "and if not, guess" sense: the id
    /// came out of the rules table, which is a set-once `OnceLock`, so the lookup that
    /// found it cannot stop finding it. It keeps Field Day answering rather than
    /// panicking if that ever became false.
    pub fn ruleset(&self) -> &'static crate::fd_rules::FdRuleset {
        crate::fd_rules::ruleset_by_id(&self.session.event_id, self.session.rules_year)
            .unwrap_or_else(|| {
                crate::fd_rules::ruleset(self.event, crate::fd_rules::CURRENT_RULES_YEAR)
            })
    }

    /// The dupe key this log's ruleset declares, as data.
    ///
    /// Read from the ruleset each time rather than cached at construction, because
    /// [`event`](Self::event) is public and is assigned after `new` on several paths; a
    /// cached copy would silently answer for the wrong event.
    pub fn dupe_rule(&self) -> crate::contest::DupeRule {
        self.ruleset().dupe_rule
    }

    /// Point this log at a Field Day event, keeping [`event`](Self::event) and the
    /// SESSION in step.
    ///
    /// The two are one fact — `new` derives the first from the second — and assigning
    /// the public field alone is exactly the disagreement `new`'s own doc warns about:
    /// since the exports read the session, an `event` set by itself would flip the
    /// class letters and nothing else. Field Day only; a QSO party's session is built
    /// by [`ContestSession::for_ruleset`](crate::contest::ContestSession::for_ruleset)
    /// and never re-pointed.
    pub fn set_event(&mut self, event: FdEvent) {
        self.event = event;
        self.session.event_id = event.code().to_string();
        self.session.contest_id = event.contest_id().to_string();
    }

    /// The `constant_sent` warning raised at log time, if any (§6.3). `None` until a
    /// contact contradicts the log's first row.
    pub fn constant_sent_warning(&self) -> Option<&crate::contest::ConstantSentMismatch> {
        self.constant_sent_warning.as_ref()
    }

    /// Dismiss the log-time warning — the surface has shown it.
    pub fn clear_constant_sent_warning(&mut self) {
        self.constant_sent_warning = None;
    }

    /// ⭐ **FIRING SITE 2 of 3 (§6.3): the thorough scan, over the whole log**, with
    /// the row counts on each side. `None` = this log sends one value per constant slot
    /// and is one entry as far as that rule goes.
    ///
    /// Public because the club host runs it over its own merged rows (site 3) through
    /// the same function rather than a second walk that could disagree.
    pub fn constant_sent_scan(&self) -> Option<crate::contest::ConstantSentMismatch> {
        let spec = self.session.exchange;
        let first = self.qsos.first()?;
        crate::contest::constant_sent::scan(
            crate::contest::role_for(first, spec),
            self.qsos.iter().map(|q| q.tx.as_slice()),
        )
    }

    /// This log's dupe index, as the ruleset's own ordered keys.
    ///
    /// Exposed so the session-scoped B4 (§3.1) can UNION it with the general log's
    /// [`worked_keys_since`](crate::logbook::Logbook::worked_keys_since) instead of
    /// rebuilding it per snapshot — one index, one key shape, both halves built by the
    /// same [`DupeRule`](crate::contest::DupeRule).
    pub fn worked_keys(&self) -> &HashSet<Vec<String>> {
        &self.worked
    }

    /// The highest [`LoggedQso::seq`] this log has stamped or restored — the
    /// position's sync high-water mark (`join.max_seq`, and the base the
    /// outbox "rows past the host's ack" is computed from). 0 = empty log.
    pub fn max_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// Already worked this call on this band IN THIS MODE CLASS? (ARRL FD
    /// rules: each station counts once per band-mode — CW, digital and phone
    /// are separate contacts.) The digital sequencer always logs "DIG".
    pub fn is_dupe(&self, call: &str) -> bool {
        self.is_dupe_mode(call, "DIG")
    }

    /// The verdict BEFORE the exchange has been copied — call, band and mode class
    /// only. The sequencers ask this on hearing a bare CQ, when nothing else is known.
    ///
    /// ⚠️ **A rule that keys on exchange slots cannot be judged from these three, so it
    /// answers `false` rather than guessing.** That is the safe direction and it is not
    /// symmetric: under-reporting costs one duplicate contact that scores zero, while
    /// over-reporting REFUSES a legal one — which is the exact defect that makes the
    /// shipped engine unusable for a QSO party. Both Field Day events key on these
    /// three and nothing else, so for them this is the whole key, not a prefix of it.
    pub fn is_dupe_mode(&self, call: &str, mode: &str) -> bool {
        self.worked_key(call, &self.band, mode)
    }

    /// Whether an EXPLICIT `(call, band, mode class)` key is in the dupe
    /// index — unlike [`is_dupe_mode`](Self::is_dupe_mode) it does not assume
    /// the log's current band. The club sync uses it to subtract own-log keys
    /// from the club dupe set (only club-ONLY keys ship to the UI). It carries
    /// `is_dupe_mode`'s exchange-slot caveat for the same reason.
    pub fn worked_key(&self, call: &str, band: &str, mode: &str) -> bool {
        let rule = self.dupe_rule();
        if !rule.by_fields.is_empty() || !rule.by_sent_fields.is_empty() {
            return false;
        }
        self.worked
            .contains(&rule.key_of(call, band, mode, &[], &[]))
    }

    /// The exact verdict, over a whole candidate contact — the check `log_submode_at`
    /// itself makes, exposed so a caller can ask before it commits.
    pub fn is_dupe_row(
        &self,
        call: &str,
        band: &str,
        mode: &str,
        rx: &[crate::contest::FieldValue],
        tx: &[crate::contest::FieldValue],
    ) -> bool {
        self.worked
            .contains(&self.dupe_rule().key_of(call, band, mode, rx, tx))
    }

    /// Log a contact. Returns false (and logs nothing) if it's a dupe.
    pub fn log(&mut self, call: &str, class: &str, section: &str, slot: u64) -> bool {
        self.log_mode_at(call, class, section, "DIG", slot, now_unix())
    }

    /// As [`log`](Self::log) with an explicit timestamp (tests / replays).
    pub fn log_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        self.log_mode_at(call, class, section, "DIG", slot, when_unix)
    }

    /// All-mode entry (the CW/Phone cockpits log FD contacts too): mode is the
    /// scoring class "DIG" | "CW" | "PH".
    pub fn log_mode_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        // The "DIG" class covers many on-air modes, so a digital entry stamps
        // [`current_submode`](Self::current_submode) (what the engine says is
        // actually keyed). CW/PH ARE their on-air mode — no submode.
        let submode = if mode.eq_ignore_ascii_case("DIG") {
            self.current_submode.clone()
        } else {
            String::new()
        };
        self.log_submode_at(call, class, section, mode, &submode, slot, when_unix)
    }

    /// As [`log_mode_at`](Self::log_mode_at) but also recording the ACTUAL
    /// on-air mode behind the scoring class (e.g. class "DIG", submode "RTTY")
    /// so exports emit the real mode. Dupes stay keyed on the CLASS — FD/WFD
    /// digital is ONE mode class, so an FT8 QSO dupes the same-band RTTY one.
    #[allow(clippy::too_many_arguments)] // log_mode_at + the one extra field
    pub fn log_submode_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        // Both sides, as data, BEFORE the dupe check — the key reads them (a mobile in
        // a new county is a new station, in both directions), so a check that ran first
        // would be judging a different contact from the one about to be logged.
        let rx = self.received(class, section);
        let tx = self.session.tx_for_row(call, mode);
        self.log_exchange_at(call, rx, tx, mode, submode, slot, when_unix)
    }

    /// [`log_submode_at`](Self::log_submode_at) with BOTH sides of the exchange given
    /// rather than derived — the club host's path.
    ///
    /// ⭐ The host merges rows from every position, and each of them sent its OWN
    /// exchange. Deriving the sent side here would stamp the host's on all of them,
    /// which is harmless for one Field Day club and wrong the moment two positions send
    /// different exchanges. So the caller that HAS the row's own sides passes them, and
    /// [`log_submode_at`](Self::log_submode_at) — the position's own logging path,
    /// where the session IS the source — is one call into this.
    #[allow(clippy::too_many_arguments)] // both exchange sides plus log_mode_at's own
    pub fn log_exchange_at(
        &mut self,
        call: &str,
        rx: Vec<crate::contest::FieldValue>,
        tx: Vec<crate::contest::FieldValue>,
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        let mode = mode.to_ascii_uppercase();
        let band = self.band.clone();
        let key = self.dupe_rule().key_of(call, &band, &mode, &rx, &tx);
        if self.worked.contains(&key) {
            return false;
        }
        self.worked.insert(key);
        // ⭐ FIRING SITE 1 of 3 (§6.3): the cheap one, at the moment the operator can
        // still fix it. O(1) — a rule that says "every row carries the same value" is
        // fully checked by comparing this row against the first. It WARNS: the contact
        // below is logged either way.
        if let Some(first) = self.qsos.first() {
            if let Some(m) = crate::contest::constant_sent::against_first(
                crate::contest::role_for(first, self.session.exchange),
                &first.tx,
                &tx,
            ) {
                self.constant_sent_warning = Some(m);
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        // ⭐ RESOLVED ONCE, HERE, and never again: what a row counts for is a fact about
        // the moment it was logged. A per-snapshot re-resolution would let a country-file
        // update silently rescore contacts already in a submitted log.
        let placed = crate::contest::resolve_call(call);
        self.qsos.push(LoggedQso {
            call: call.to_string(),
            rx,
            tx,
            role: self.session.role().id.to_string(),
            entity: placed.map(|p| p.entity.to_string()),
            continent: placed.map(|p| p.continent.to_string()),
            prefix: crate::contest::wpx_prefix(call),
            band,
            mode,
            submode: submode.trim().to_ascii_uppercase(),
            slot,
            when_unix,
            seq,
        });
        // The contact is logged, so nothing is in flight any more: the next exchange
        // composed issues its own serial instead of re-sending this one's.
        self.session.clear_in_flight();
        true
    }

    /// ⭐ **Log a contact whose received exchange is a FIELD VECTOR** — the path a
    /// contest that receives something other than `(class, section)` logs through.
    ///
    /// `fields` is `(slot id, raw)` in the role's receive order, exactly as the entry
    /// strip renders its boxes. Values are resolved through
    /// [`ExchangeSpec::copied`](crate::contest::ExchangeSpec::copied), so a
    /// [`FieldKind::OneOf`](crate::contest::FieldKind::OneOf) slot records the arm that
    /// actually matched — which is what the multiplier bucket and the ADIF column are
    /// later chosen by (§2.4). A pair naming a slot this exchange does not declare is
    /// DROPPED rather than invented: a value with no slot is not a value.
    ///
    /// [`log_submode_at`](Self::log_submode_at) is the Field Day shape of this call and
    /// stays, because the FT sequencer and the club host both hand it exactly two
    /// positional values off the air.
    #[allow(clippy::too_many_arguments)] // the field vector plus log_mode_at's own
    pub fn log_fields_at(
        &mut self,
        call: &str,
        fields: &[(String, String)],
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        let spec = self.session.exchange;
        let rx: Vec<crate::contest::FieldValue> = fields
            .iter()
            .filter_map(|(k, v)| spec.copied(k, v))
            .collect();
        let tx = self.session.tx_for_row(call, mode);
        self.log_exchange_at(call, rx, tx, mode, submode, slot, when_unix)
    }

    /// A received Field Day exchange as a field vector, resolved against the exchange
    /// this session runs.
    ///
    /// ⚠️ The raw values are stored VERBATIM. Uppercasing here would rewrite what a
    /// shipped log holds and move the §8(a) goldens; normalisation belongs to whatever
    /// copied the value off the air.
    fn received(&self, class: &str, section: &str) -> Vec<crate::contest::FieldValue> {
        let spec = self.session.exchange;
        ["CLASS", "SECTION"]
            .iter()
            .zip([class, section])
            .filter_map(|(k, v)| spec.value(k, v))
            .collect()
    }

    pub fn qso_count(&self) -> usize {
        self.qsos.len()
    }

    pub fn qsos(&self) -> &[LoggedQso] {
        &self.qsos
    }

    /// Mutable rows — **tests only**, and deliberately not a public API.
    ///
    /// Every production path into this log stamps a club-sync sequence (log time
    /// stamps one; a journal restore backfills one, and `filter(|&v| v > 0)` means even
    /// a journaled zero backfills). That is exactly why the merge's defence against an
    /// unstamped row cannot be reached from outside: the shape it refuses has to be
    /// built by hand.
    #[cfg(test)]
    pub(crate) fn qsos_mut(&mut self) -> &mut [LoggedQso] {
        &mut self.qsos
    }

    /// This log's rows as the scorer reads them.
    ///
    /// The seam that took scoring off this type: `Scoring::qso_and_powered` used to
    /// take a `&FieldDayLog`, which is what welded the scoring math to Field Day's
    /// own log. Borrowed and lazy, because the score is recomputed on every
    /// snapshot tick and must stay O(rows) with no allocation.
    /// ⚠️ `+ Clone` is load-bearing: [`Scoring::score`](crate::contest::Scoring::score)
    /// walks these rows TWICE — once for points, once for multipliers — and a
    /// once-through iterator would force it to collect a `Vec` on every snapshot tick.
    pub fn score_rows(&self) -> impl Iterator<Item = crate::contest::ScoreRow<'_>> + Clone {
        let mine = self.session.my_call_location;
        self.qsos.iter().map(move |q| crate::contest::ScoreRow {
            mode_class: &q.mode,
            band: &q.band,
            role: &q.role,
            rx: &q.rx,
            entity: q.entity.as_deref(),
            prefix: q.prefix.as_deref(),
            // ⭐ The relation is computed HERE because this is the one place both sides
            // are in hand: the session knows where I am, the row knows where they were.
            // Neither the scorer (which has no session) nor the row (which has no `me`)
            // can answer it alone.
            relation: crate::contest::Relation::between(
                mine.as_ref().map(|l| l.as_pair()),
                q.entity.as_deref().zip(q.continent.as_deref()),
            ),
        })
    }

    /// Distinct ARRL/RAC sections worked — a DISPLAY count for the worked-sections
    /// board, **not a multiplier**: neither Field Day event has one, and no scoring
    /// path reads this. (Said otherwise here until 2026-09-08, on the very function
    /// a reader would check.)
    pub fn sections(&self) -> usize {
        self.qsos
            .iter()
            .map(|q| q.section())
            .collect::<HashSet<_>>()
            .len()
    }

    /// The distinct sections worked — the identities behind the
    /// [`sections`](Self::sections) count, sorted for a stable board order
    /// (the worked-sections color board, spec §5).
    pub fn worked_sections(&self) -> Vec<String> {
        let mut sections: Vec<String> = self
            .qsos
            .iter()
            .map(|q| q.section().to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        sections.sort();
        sections
    }

    /// The distinct values RECEIVED in one exchange slot, sorted — the generalisation
    /// of [`worked_sections`](Self::worked_sections) that one block of the multiplier
    /// display (§9) colours in.
    ///
    /// Blanks are dropped. A board colours cells by code, so a blank matches nothing
    /// either way; dropping it keeps a legacy row with no value out of a count that
    /// would otherwise read one too high.
    pub fn worked_values(&self, slot: &str) -> Vec<String> {
        let mut vals: Vec<String> = self
            .qsos
            .iter()
            .map(|q| q.rcvd(slot))
            .filter(|v| !v.is_empty())
            .map(|v| v.to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        vals.sort();
        vals
    }

    /// Per-mode QSO points (phone 1, CW/digital 2) — power multiplier and
    /// bonuses are applied at the score layer (engine), not here.
    pub fn qso_points(&self) -> u32 {
        self.qsos.iter().map(|q| qso_points_for_mode(&q.mode)).sum()
    }

    /// Export the log as ADIF records (one `<EOR>` per QSO).
    pub fn adif(&self) -> String {
        let mut s = String::from("ADIF Export from Nexus\n<PROGRAMID:5>Nexus\n<EOH>\n");
        // ⭐ Has the sent exchange moved at any point in this log? If not — and it has
        // not, for either Field Day event, which send one exchange all weekend — the
        // session fallback gives every row back exactly and no row needs a carrier.
        // The moment ONE row disagrees, EVERY row gets its own `APP_NEXUS_MYEX`, not
        // just the ones that differ: a later move must not be able to re-label the rows
        // that happen to match the session today.
        let sent_moved = self.qsos.iter().any(|q| q.tx != self.session.my_exchange);
        for q in &self.qsos {
            s.push_str(&adif_field("CALL", &q.call));
            // ⚠️ A MODE OUTSIDE ADIF'S ENUMERATION IS A DROPPED RECORD, NOT A COSMETIC ONE.
            // This wrote `q.submode` raw, and the tiers stamp names ADIF has never heard of
            // ("TempoFast", "TempoDeep", "FT2") — the main logbook already solved that with
            // `logbook::adif_submode`, whose own comment spells out the cost: a bare
            // <MODE:3>FT2 misses all three legs of TQSL's MODE%SUBMODE → SUBMODE → MODE
            // cascade and the record is DROPPED with "Invalid MODE". The Field Day exporter
            // simply never called it, so a Field Day contact worked on a Tempo tier was lost
            // on upload. Same cascade now, so the two exports cannot disagree.
            let recorded = q.recorded_mode();
            match crate::logbook::adif_submode(recorded) {
                Some((parent, sub)) => {
                    s.push_str(&adif_field("MODE", parent));
                    s.push_str(&adif_field("SUBMODE", sub));
                }
                None => s.push_str(&adif_field("MODE", recorded)),
            }
            s.push_str(&adif_field("BAND", &q.band));
            // A real date/time so [`merge_adif`](Self::merge_adif) can restore
            // `when_unix` (and Cabrillo keeps its ARRL-required timestamps
            // across a restart). Legacy rows without a stamp omit both fields
            // rather than inventing a date.
            if q.when_unix > 0 {
                let (date, time) = adif_datetime(q.when_unix);
                s.push_str(&adif_field("QSO_DATE", &date));
                s.push_str(&adif_field("TIME_ON", &time));
            }
            // The SESSION's Cabrillo token, not the `FdEvent`'s: a QSO party has no
            // `FdEvent` arm, and reading one would export every party as Field Day.
            s.push_str(&adif_field("CONTEST_ID", &self.session.contest_id));
            // The RECEIVED exchange under its own slots' ADIF tags, in the role's
            // receive order — for Field Day exactly the <CLASS> and <ARRL_SECT> this
            // has always written, from the row instead of from two named columns.
            let spec = self.session.exchange;
            let role = crate::contest::role_for(q, spec);
            let standard: Vec<(&'static str, String)> = standard_rcvd_slots(spec, role)
                .map(|(key, tag)| (tag, q.rcvd(key).to_string()))
                .collect();
            for (tag, val) in &standard {
                s.push_str(&adif_field(tag, val));
            }
            // ⭐ The private carrier rides ONLY when the standard tags cannot give the
            // row back — and that condition is not a guess, it is the restore run
            // forward: `rx_from_standard` is the SAME function `restore_row` falls back
            // to, so "the fallback is exact" and "no tag is written" are one fact and
            // cannot drift. For Field Day both fallbacks are exact (class and section
            // ARE the received exchange; the sent exchange does not move), so nothing
            // is written and the §8(a) ADIF golden does not move. `sent_moved` above is
            // the same test for the other direction, taken over the whole log.
            if rx_from_standard(spec, role, |tag| {
                standard
                    .iter()
                    .find(|(t, _)| *t == tag)
                    .map(|(_, v)| v.clone())
            }) != q.rx
            {
                s.push_str(&adif_field(
                    "APP_NEXUS_EX",
                    &crate::contest::carrier::encode(&q.rx),
                ));
            }
            if sent_moved {
                s.push_str(&adif_field(
                    "APP_NEXUS_MYEX",
                    &crate::contest::carrier::encode(&q.tx),
                ));
            }
            // Same rule for the role: written only when it is not the session's own,
            // which for a symmetric contest (both Field Day events) is never.
            if q.role != self.session.role().id {
                s.push_str(&adif_field("APP_NEXUS_ROLE", &q.role));
            }
            // The per-position sync sequence (APP_-namespaced per the ADIF
            // spec, so every other consumer ignores it). Legacy 0 rows omit
            // the tag — restore backfills them in row order.
            if q.seq > 0 {
                s.push_str(&adif_field("APP_NEXUS_QSEQ", &q.seq.to_string()));
            }
            s.push_str("<EOR>\n");
        }
        s
    }

    /// Merge a previously-flushed ADIF journal (see [`adif`](Self::adif)) back
    /// into this log — the restore half of the durable Field Day backup, so a
    /// restart mid-event doesn't reset the contest log. Rows missing a CALL or
    /// stamped before `min_when_unix` are skipped (a previous event's journal
    /// self-expires), rows already in the dupe index are skipped, and garbage
    /// input merges nothing — never an error. Restored dupe keys keep the ROW's
    /// band, so they survive a mid-event QSY.
    pub fn merge_adif(&mut self, text: &str, min_when_unix: u64) {
        // Minimal `<NAME:len>value` tokenizer mirroring logbook.rs `parse_adif`
        // (this journal only needs the handful of FD tags).
        let body = match text.to_ascii_uppercase().find("<EOH>") {
            Some(i) => &text[i + 5..],
            None => text,
        };
        let mut cur: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'<' {
                i += 1;
                continue;
            }
            let end = match body[i..].find('>') {
                Some(e) => i + e,
                None => break,
            };
            let tag = &body[i + 1..end];
            i = end + 1;
            if tag.eq_ignore_ascii_case("EOR") {
                self.restore_row(&cur, min_when_unix);
                cur.clear();
                continue;
            }
            // NAME:len or NAME:len:type
            let mut parts = tag.splitn(3, ':');
            let name = parts.next().unwrap_or("").to_ascii_uppercase();
            let len: usize = parts
                .next()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(0);
            let val = body.get(i..i + len).unwrap_or("").to_string();
            i += len;
            cur.insert(name, val);
        }
    }

    /// One tokenized journal record → the log (the dupe-checked insert half of
    /// [`merge_adif`](Self::merge_adif)).
    fn restore_row(&mut self, f: &std::collections::HashMap<String, String>, min_when_unix: u64) {
        let Some(call) = f.get("CALL").filter(|c| !c.trim().is_empty()) else {
            return;
        };
        // ADIF MODE → (mode class, actual mode): the reverse of the map in
        // [`adif`](Self::adif) — keep the two in step. A digital MODE keeps its
        // identity as the submode so RTTY/SSTV rows survive the round-trip.
        let (mode, submode) = match f.get("MODE").map(|m| m.to_ascii_uppercase()) {
            Some(m) if m == "CW" => ("CW", String::new()),
            Some(m) if m == "SSB" => ("PH", String::new()),
            Some(m) => ("DIG", m),
            None => ("DIG", String::new()),
        };
        // QSO_DATE (yyyymmdd) + TIME_ON (hhmmss) → when_unix. Unparseable rows
        // stamp 0 and fall to the age gate (never a panic on garbage input).
        let parse_dt = |d: &str, t: &str| -> Option<u64> {
            Some(unix_from_ymdhms(
                d.get(0..4)?.parse().ok()?,
                d.get(4..6)?.parse().ok()?,
                d.get(6..8)?.parse().ok()?,
                t.get(0..2)?.parse().ok()?,
                t.get(2..4)?.parse().ok()?,
                t.get(4..6)?.parse().ok()?,
            ))
        };
        let when_unix = match (f.get("QSO_DATE"), f.get("TIME_ON")) {
            (Some(d), Some(t)) => parse_dt(d, t).unwrap_or(0),
            _ => 0,
        };
        if when_unix < min_when_unix {
            return;
        }
        // The ROW's band, not the log's current one — a restored dupe key must
        // keep its original band across a mid-event QSY.
        let band = f.get("BAND").cloned().unwrap_or_default();
        let spec = self.session.exchange;
        // ⭐ The row's ROLE, then its two exchange sides. A 1.x journal carries none of
        // the three tags, and each falls back to what that build meant by its absence:
        // the session's role, the standard <CLASS>/<ARRL_SECT> columns, and the
        // session's current sent exchange. For Field Day all three fallbacks are exact
        // — which is why the writer above emits no tag at all for a Field Day row, and
        // why a 1.x journal restores to a byte-identical log.
        let role = f
            .get("APP_NEXUS_ROLE")
            .cloned()
            .unwrap_or_else(|| self.session.role().id.to_string());
        let role_spec = spec
            .roles
            .iter()
            .find(|r| r.id == role)
            .unwrap_or(self.session.role());
        let rx = match f.get("APP_NEXUS_EX") {
            Some(v) => crate::contest::carrier::decode(v, spec),
            None => rx_from_standard(spec, role_spec, |tag| f.get(tag).cloned()),
        };
        let tx = match f.get("APP_NEXUS_MYEX") {
            Some(v) => crate::contest::carrier::decode(v, spec),
            None => self.session.my_exchange.clone(),
        };
        let key = self.dupe_rule().key_of(call, &band, mode, &rx, &tx);
        if self.worked.contains(&key) {
            return;
        }
        self.worked.insert(key);
        // The journaled sync seq round-trips; a legacy row without the tag
        // backfills the next free seq in row order (1..n on a whole legacy
        // journal), so pre-sync journals join the sequence deterministically.
        let seq = f
            .get("APP_NEXUS_QSEQ")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(self.next_seq);
        self.next_seq = self.next_seq.max(seq + 1);
        // Resolved on the RESTORE path too, and from the call the row carries — a
        // journal reload or an ADIF merge must score identically to the live log, and a
        // row restored with no entity would drop a country multiplier the operator
        // already worked.
        let placed = crate::contest::resolve_call(call);
        self.qsos.push(LoggedQso {
            call: call.clone(),
            rx,
            tx,
            role,
            entity: placed.map(|p| p.entity.to_string()),
            continent: placed.map(|p| p.continent.to_string()),
            prefix: crate::contest::wpx_prefix(call),
            band,
            mode: mode.to_string(),
            submode,
            slot: 0,
            when_unix,
            seq,
        });
    }

    /// Export the log as a Cabrillo entry — headers (§6.1) then one QSO line per
    /// contact (§6.2) — for the given band frequency (kHz).
    ///
    /// `Err` carries the reason this log is not ONE submittable entry, in words meant
    /// for the export dialog. Today there is exactly one such reason and no shipped
    /// contest can reach it: a mode-split contest whose rows span both modes submits
    /// as two entries, and a file cannot be both. It is fallible rather than
    /// best-effort because the alternative is writing a file that looks right and is
    /// scored under the wrong id.
    pub fn cabrillo(&self, freq_khz: u32) -> Result<String, String> {
        let spec = self.session.exchange;
        // ⭐ The `CONTEST` token, resolved against the mode classes this log actually
        // holds. A mode-split contest submits a separate entry per mode, so one file
        // holding both is refused BY NAME rather than filed under whichever id came
        // first (§6.2). Nothing this build ships is split, so this is always the
        // session's own id — read from the SESSION, which is the only place that can
        // name a contest `FdEvent` has no arm for (see [`Self::ruleset`]).
        // ⭐ FIRING SITE 2 of 3, at the point of use (§6.3): a log whose rows send two
        // different values for a slot the sponsor says is constant is NOT one
        // submittable entry — SS-Rules v2.1 §4.4.2, *"The same Check must be sent
        // throughout the contest"* — which is the same class of refusal this function
        // already makes for a mode-split log holding both modes. It refuses to export
        // SILENTLY; the message names both values and the row counts so the export
        // dialog can say what to fix. (Logging a contact is never blocked: that is
        // site 1, which warns.)
        if let Some(m) = self.constant_sent_scan() {
            return Err(m.message());
        }
        let mut classes: Vec<&str> = self.qsos.iter().map(|q| q.mode.as_str()).collect();
        classes.sort_unstable();
        classes.dedup();
        let headers = crate::contest::CabrilloHeaders {
            // ⭐ …then translated out of ADIF's namespace into Cabrillo's. The id
            // above is an ADIF `CONTEST_ID` enumeration value, which is NOT the
            // Cabrillo `CONTEST:` token for every contest — ARRL Field Day is
            // `ARRL-FIELD-DAY` to ADIF and `ARRL-FD` to Cabrillo. This is the last
            // step before the header and the only place the two registries meet;
            // the ADIF export above reads the id directly and must keep doing so.
            //
            // ⚠️ The id it translates comes from the SESSION, not from `self.event`:
            // `FdEvent` has no arm for a QSO party, so reading it here would hand
            // every party's log to the Cabrillo mapper under Field Day's id and get
            // `ARRL-FD` back.
            contest: crate::contest::cabrillo_contest_token(&crate::contest::resolve_contest_id(
                &self.session.contest_id,
                &self.session.contest_id_by_mode,
                &classes,
            )?)
            .to_string(),
            callsign: self.mycall.clone(),
            // ⭐ The declaration, not a literal. `CATEGORY-OPERATOR: MULTI-OP` was
            // hardcoded here, so every solo entry submitted a claim that more than
            // one operator was at the station.
            category_operator: self.session.entry_category,
            // LOCATION is a per-ENTRY value, so it reads the session's declared
            // location and not a row: Cabrillo puts it in a header, once, for exactly
            // that reason. The per-contact truth is on the QSO lines below.
            location: self.session.my_location.state.clone(),
            created_by: "Nexus".to_string(),
            // Which rules data scored this log (X- headers are Cabrillo-legal and
            // ignored by robots) — a fetched rules file with different parameters
            // is visible on the artifact an operator actually submits.
            x_headers: vec![(
                "X-NEXUS-RULES-YEAR".to_string(),
                self.ruleset().rules_year.to_string(),
            )],
        };
        let mut s = headers.render();
        for q in &self.qsos {
            // QSO: freq mo date time mycall myexch call exch — ARRL requires a
            // REAL `yyyy-mm-dd hhmm`; the old `----------` placeholder failed
            // submission. Legacy rows without a stamp keep the placeholder so
            // we never invent a time.
            let (date, time) = if q.when_unix > 0 {
                cabrillo_datetime(q.when_unix)
            } else {
                ("----------".to_string(), "----".to_string()) // HHMM is 4 chars
            };
            // Mode token per Cabrillo 3.0: CW, PH phone, RY for RTTY rows
            // (WFD prefers it), DG for other/unrecorded digital — a legal
            // fallback either event.
            let mo = match q.mode.as_str() {
                "CW" => "CW",
                "PH" => "PH",
                _ if q.submode == "RTTY" => "RY",
                _ => "DG",
            };
            // Per-QSO frequency from ITS band. The caller's dial is a fallback ONLY for a
            // row with no band recorded at all — never for a band we simply failed to map,
            // because that is how a 23 cm club contact came to export as 20 m: the club
            // exporter passes a hardcoded 14 MHz and every unmapped band took it. A band we
            // do not recognise now rides through as itself; a wrong-looking token in one
            // field beats a confident lie about which band a contact was made on, and ARRL
            // Field Day requires the worked list sorted BY BAND.
            let mapped = band_to_cabrillo_freq(&q.band);
            let raw = q.band.trim();
            let freq: String = match (mapped, raw.is_empty()) {
                (Some(f), _) => f.to_string(),
                (None, false) => raw.to_string(),
                (None, true) => freq_khz.to_string(),
            };
            // ⭐ THE EXCHANGE COLUMNS COME FROM THIS ROW, never from the log or the
            // session. `self.myexch.class`/`.section` sat here until batch 3 and were
            // written on EVERY line, so an operator who changed county re-labelled
            // every contact made before the change — silently, in the file they submit.
            //
            // ⭐ **The callsign columns are STRUCTURAL — they sit outside the
            // exchange — and a side whose exchange ALSO declares a `Call` slot does not
            // repeat it.** A template of exchange slots alone loses both callsigns on
            // every contest, and an unsubmittable line is the failure mode; Sweepstakes
            // is the one contest whose on-air exchange carries a callsign too, so
            // without the exception its line would show four. ARRL's own published SS
            // template puts the callsign in the structural column (`E= Your call`,
            // `J= The call of the station you worked`) with serial/precedence/check/
            // section after it, so the slot's Cabrillo home IS that column — see
            // [`contest::cabrillo::side_declares_call`] for the template and the legend.
            // Read off the slot list, so there is nothing a ruleset can declare
            // inconsistently.
            let role = crate::contest::role_for(q, spec);
            let mut cols: Vec<String> = vec![freq, mo.to_string(), date, time];
            let is_call = |k: &str| crate::contest::is_call_slot(spec, k);
            cols.push(self.mycall.clone());
            cols.extend(
                crate::contest::sent_exchange(q, spec)
                    .into_iter()
                    .filter(|v| !is_call(v.key))
                    .map(|v| v.raw)
                    .filter(|r| !r.is_empty()),
            );
            cols.push(q.call.clone());
            cols.extend(
                role.receives
                    .iter()
                    .filter(|k| !is_call(k))
                    .map(|k| q.rcvd(k))
                    .filter(|v| !v.is_empty())
                    .map(str::to_string),
            );
            // The trailing transmitter-id column, present only where the sponsor's own
            // template has one — never for either Field Day event.
            if let Some(t) = self.session.transmitter_id {
                cols.push(t.to_string());
            }
            s.push_str(&format!("QSO: {}\n", cols.join(" ")));
        }
        s.push_str("END-OF-LOG:\n");
        Ok(s)
    }
}

/// The received slots that have a standard ADIF column, as `(slot id, tag)`, in the
/// role's receive order.
///
/// ⭐ **ONE list, read by the ADIF writer and by the restore.** The write direction and
/// the read direction being the same function is what makes "the standard columns are
/// enough for this row" and "no private carrier was written" the same fact rather than
/// two claims that can drift apart. A slot with no `rcvd` tag has no standard column
/// either way round, and rides [`carrier`](crate::contest::carrier) instead.
fn standard_rcvd_slots(
    spec: &'static crate::contest::ExchangeSpec,
    role: &'static crate::contest::RoleSpec,
) -> impl Iterator<Item = (&'static str, &'static str)> {
    role.receives
        .iter()
        .filter_map(move |key| Some((*key, spec.field(key)?.adif.rcvd?)))
}

/// The received exchange as the standard ADIF columns carry it — the restore FALLBACK
/// for a journal with no `APP_NEXUS_EX`, and the writer's test for whether that tag is
/// needed at all. `get` resolves an ADIF tag to its value.
fn rx_from_standard(
    spec: &'static crate::contest::ExchangeSpec,
    role: &'static crate::contest::RoleSpec,
    get: impl Fn(&str) -> Option<String>,
) -> Vec<crate::contest::FieldValue> {
    standard_rcvd_slots(spec, role)
        .filter_map(|(key, tag)| spec.value(key, &get(tag)?))
        .collect()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A band-representative frequency in kHz for the Cabrillo QSO line. Field Day
/// isn't scored by frequency, and we only store the band per contact — so a
/// multi-band log must stamp each row with ITS band, not the single dial the
/// export happened to be sitting on. `None` for an unrecognized band → caller's
/// fallback.
fn band_to_cabrillo_freq(band: &str) -> Option<&'static str> {
    Some(match band.trim().to_ascii_lowercase().as_str() {
        // HF: a representative frequency in kHz.
        "160m" => "1800",
        "80m" => "3500",
        "60m" => "5330",
        "40m" => "7000",
        "30m" => "10100",
        "20m" => "14000",
        "17m" => "18068",
        "15m" => "21000",
        "12m" => "24890",
        "10m" => "28000",
        // ⚠️ 50 MHz AND UP IS A BAND TOKEN, NOT KILOHERTZ. The Cabrillo QSO-data spec:
        // "freq is frequency or band: 1800 or actual frequency in kHz […] 50 / 70 / 144 /
        // 222 / 432 / 902 / 1.2G / 2.3G". Winter Field Day repeats the rule and machine-parses
        // the QSO lines out of whatever is uploaded, so this is the half that actually gets
        // read. We used to emit 50000 / 144000 / 222000, and 70 cm as 420000 — wrong form, and
        // for 70 cm not even a sensible frequency; the token is 432.
        "6m" => "50",
        "4m" => "70",
        "2m" => "144",
        "1.25m" => "222",
        "70cm" => "432",
        "33cm" => "902",
        "23cm" => "1.2G",
        "13cm" => "2.3G",
        "9cm" => "3.4G",
        "6cm" => "5.7G",
        "3cm" => "10G",
        _ => return None,
    })
}

/// Unix seconds → ("yyyy-mm-dd", "hhmm") in UTC for two Cabrillo fields.
fn cabrillo_datetime(unix: u64) -> (String, String) {
    let (y, mo, d, h, mi, _s) = civil_from_unix(unix);
    (format!("{y:04}-{mo:02}-{d:02}"), format!("{h:02}{mi:02}"))
}

/// Unix seconds → ("yyyymmdd", "hhmmss") in UTC for ADIF QSO_DATE / TIME_ON.
fn adif_datetime(unix: u64) -> (String, String) {
    let (y, mo, d, h, mi, s) = civil_from_unix(unix);
    (
        format!("{y:04}{mo:02}{d:02}"),
        format!("{h:02}{mi:02}{s:02}"),
    )
}

/// Unix seconds → civil UTC (y, mo, d, h, mi, s) (Howard Hinnant's
/// civil-from-days; no external date crate needed for a few export fields).
fn civil_from_unix(unix: u64) -> (i64, u32, u32, u32, u32, u32) {
    let secs_of_day = unix % 86_400;
    let days = (unix / 86_400) as i64;
    let (h, mi, s) = (
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
        (secs_of_day % 60) as u32,
    );
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}

/// Civil UTC → Unix seconds — the inverse of [`civil_from_unix`], for restoring
/// journal timestamps (mirrors `logbook.rs::unix_from_ymdhms`).
fn unix_from_ymdhms(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> u64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + (h as i64) * 3600 + (mi as i64) * 60 + s as i64;
    secs.max(0) as u64
}

fn adif_field(name: &str, value: &str) -> String {
    format!("<{}:{}>{} ", name, value.len(), value)
}

// ---- Auto-sequencer -------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdState {
    /// Monitoring; will answer the first CQ heard (search & pounce).
    Listening,
    /// Calling CQ FD (running).
    CallingCq,
    /// (S&P) sent my exchange; awaiting the runner's rogered exchange.
    AwaitExchange,
    /// (Running) got a caller's exchange; awaiting their RR73 confirmation.
    AwaitConfirm,
    /// Contact complete (logged).
    Done,
}

/// One station's Field Day auto-sequencer.
#[derive(Debug)]
pub struct FieldDayStation {
    pub mygrid: String,
    pub state: FdState,
    pub pending: Option<Msg>,
    pub dxcall: Option<String>,
    /// A caller's exchange, remembered until their RR73 lets us log it.
    peer_exch: Option<(String, String, String)>, // (call, class, section)
    pub log: FieldDayLog,
    pub transcript: Vec<String>,
}

impl FieldDayStation {
    fn mycall(&self) -> &str {
        &self.log.mycall
    }

    /// A running station (calls CQ FD).
    pub fn running(
        mycall: &str,
        mygrid: &str,
        session: crate::contest::ContestSession,
        band: &str,
    ) -> Self {
        Self {
            mygrid: mygrid.to_string(),
            state: FdState::CallingCq,
            pending: Some(Msg::Cq {
                de: mycall.to_string(),
                grid: mygrid.to_string(),
                // Field Day's running CQ is the directed "CQ FD <call> <grid>" —
                // the `FD` token advertises a Field Day CQ (`to_text` renders it,
                // `is_cq_dir` accepts it). S&P still answers ANY CQ, FD or plain.
                dir: "FD".to_string(),
            }),
            dxcall: None,
            peer_exch: None,
            log: FieldDayLog::new(mycall, session, band),
            transcript: Vec::new(),
        }
    }

    /// A search-and-pounce station (answers CQs).
    pub fn search_and_pounce(
        mycall: &str,
        mygrid: &str,
        session: crate::contest::ContestSession,
        band: &str,
    ) -> Self {
        Self {
            mygrid: mygrid.to_string(),
            state: FdState::Listening,
            pending: None,
            dxcall: None,
            peer_exch: None,
            log: FieldDayLog::new(mycall, session, band),
            transcript: Vec::new(),
        }
    }

    pub fn done(&self) -> bool {
        self.state == FdState::Done && self.pending.is_none()
    }

    pub fn outgoing(&self) -> Option<Msg> {
        self.pending.clone()
    }

    pub fn after_tx(&mut self) {
        if self.state == FdState::Done {
            self.pending = None;
        }
    }

    /// Return a finished station (`Done`, closing frame already sent) to its
    /// starting posture so it works the NEXT contact instead of going silent
    /// after a single QSO — Run → back to calling CQ FD, S&P → back to
    /// listening. The contest log is kept, so it remains the dupe/score source.
    pub fn rearm(&mut self, running: bool) {
        self.dxcall = None;
        self.peer_exch = None;
        if running {
            self.state = FdState::CallingCq;
            self.pending = Some(Msg::Cq {
                de: self.mycall().to_string(),
                grid: self.mygrid.clone(),
                dir: "FD".to_string(),
            });
        } else {
            self.state = FdState::Listening;
            self.pending = None;
        }
    }

    fn my_exch_msg(&self, to: &str, roger: bool) -> Msg {
        Msg::FieldDay {
            to: to.to_string(),
            de: self.mycall().to_string(),
            roger,
            // What is going on the air RIGHT NOW is exactly what the session holds,
            // so this reads the session — and it is the one reader that legitimately
            // does. `Msg::FieldDay` has typed class and section fields of its own, so
            // no exchange is rendered here; a reader DESCRIBING a past contact reads
            // that contact's row instead (`contest::sent_exchange`).
            class: self.log.session.field("CLASS").to_string(),
            section: self.log.session.field("SECTION").to_string(),
        }
    }

    /// Process the signals decoded this slot and advance the exchange.
    pub fn observe(&mut self, decodes: &[Decode], slot: u64) {
        for d in decodes {
            let m = Msg::parse(&d.message);
            match (self.state, &m) {
                // S&P: heard a CQ → answer with my exchange.
                (FdState::Listening, Msg::Cq { de, .. }) => {
                    if self.log.is_dupe(de) {
                        self.transcript.push(format!("skip dupe {de}"));
                        continue;
                    }
                    self.dxcall = Some(de.clone());
                    self.pending = Some(self.my_exch_msg(de, false));
                    self.state = FdState::AwaitExchange;
                    self.transcript.push(format!("answer CQ {de}"));
                }
                // Running: a caller sent their exchange → roger + send mine.
                (
                    FdState::CallingCq,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: false,
                        class,
                        section,
                    },
                ) if to == self.mycall() => {
                    self.dxcall = Some(de.clone());
                    self.peer_exch = Some((de.clone(), class.clone(), section.clone()));
                    self.pending = Some(self.my_exch_msg(de, true));
                    self.state = FdState::AwaitConfirm;
                    self.transcript
                        .push(format!("caller {de} {class} {section} → R + my exch"));
                }
                // Running (out-of-order tolerance): a caller that ROGERED its
                // exchange (roger:true) while we're still calling CQ — the plain
                // exchange was dropped, or the caller pre-rogered. The class +
                // section are carried in the rogered frame, so skip straight to
                // logging and close with RR73 rather than stalling on CQ.
                // Happy-path callers send roger:false, so this never fires on the
                // normal sequence.
                (
                    FdState::CallingCq,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: true,
                        class,
                        section,
                    },
                ) if to == self.mycall() => {
                    self.dxcall = Some(de.clone());
                    self.log.log(de, class, section, slot);
                    self.pending = Some(Msg::Rr73 {
                        to: de.clone(),
                        de: self.mycall().to_string(),
                    });
                    self.state = FdState::Done;
                    self.transcript.push(format!(
                        "caller {de} {class} {section} rogered early → log + RR73"
                    ));
                }
                // S&P: the runner rogered + sent their exchange → log + RR73.
                (
                    FdState::AwaitExchange,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: true,
                        class,
                        section,
                    },
                ) if to == self.mycall() => {
                    self.log.log(de, class, section, slot);
                    self.pending = Some(Msg::Rr73 {
                        to: de.clone(),
                        de: self.mycall().to_string(),
                    });
                    self.state = FdState::Done;
                    self.transcript
                        .push(format!("logged {de} {class} {section}; send RR73"));
                }
                // Running: caller confirmed → log them. A bare `73` (Bye73)
                // completes too — if the caller closes with 73 instead of
                // RR73/RRR the contact must not stall (their exchange is already
                // in hand from the CallingCq step). Happy-path callers send RR73,
                // so the added Bye73 alternative never fires on the normal flow.
                (FdState::AwaitConfirm, Msg::Rr73 { to, .. })
                | (FdState::AwaitConfirm, Msg::Rrr { to, .. })
                | (FdState::AwaitConfirm, Msg::Bye73 { to, .. })
                    if to == self.mycall() =>
                {
                    if let Some((call, class, section)) = self.peer_exch.take() {
                        self.log.log(&call, &class, &section, slot);
                        self.transcript
                            .push(format!("logged {call} {class} {section}"));
                    }
                    self.pending = None;
                    self.state = FdState::Done;
                }
                _ => {}
            }
        }
    }
}

/// Run a Field Day exchange between a running and an S&P station over the
/// virtual channel. Stops when both have logged the contact or `max_slots`.
pub fn run_loopback_fieldday(
    running: &mut FieldDayStation,
    sp: &mut FieldDayStation,
    snr_db: f32,
    max_slots: u64,
) {
    use crate::channel::{to_i16, VirtualAir, ON_TIME_OFFSET};
    use crate::tx;

    let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, 0xFD0001);
    for slot in 0..max_slots {
        let (txs, rxs): (&mut FieldDayStation, &mut FieldDayStation) = if slot % 2 == 0 {
            (&mut *running, &mut *sp)
        } else {
            (&mut *sp, &mut *running)
        };
        if let Some(msg) = txs.outgoing() {
            let text = msg.to_text();
            let frame = tx::build(&text, tempo_fast::SAMPLE_RATE, 1500.0);
            let rx_f32 = air.receive(&frame.wave, ON_TIME_OFFSET, snr_db);
            let decodes: Vec<Decode> = tempo_fast::decode_frame(
                &to_i16(&rx_f32),
                200,
                2900,
                3,
                "",
                "",
                0,
                (slot as i64).wrapping_mul(4000), // monotonic ms for IR-HARQ keying
            )
            .into_iter()
            .map(Into::into)
            .collect();
            rxs.observe(&decodes, slot);
            txs.after_tx();
        }
        if running.log.qso_count() >= 1 && sp.log.qso_count() >= 1 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    /// ⚠️ THE CONTEST LOG *HAS* A PER-CONTACT BAND — what it has no way to do
    /// is let a CALLER name it. Pinned because the Field Day guide said "the
    /// Field Day log has no per-contact band field", which is false and sends
    /// a future fixer looking for a field to add rather than a parameter.
    ///
    /// `LoggedQso::band` is real and is written per contact, so one log can
    /// hold two bands and the exports get them right. But every entry point —
    /// `log`, `log_at`, `log_mode_at`, `log_submode_at` — funnels into
    /// `log_submode_at`, which stamps `self.band`, the log's ONE current band.
    /// No signature takes a band. That is why `Engine::fd_log_manual` opening
    /// with `sync_fd_band()` makes a catch-up entry file on the dial you are on
    /// NOW: not a missing field, an unreachable one.
    ///
    /// Goes red the day a caller can name the band — which is the cue to delete
    /// the "log as you work" caveat from docs/guide/contesting-pota.md and
    /// docs/guide/satellites.md.
    #[test]
    fn every_contact_carries_a_band_and_it_is_always_the_logs_own() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "70cm",
        );
        assert!(log.log_mode_at("W1AW", "1D", "IL", "PH", 0, 100));
        // The only way the band moves: the LOG's band, not an argument.
        log.band = "20m".into();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "PH", 0, 200));

        let bands: Vec<&str> = log.qsos().iter().map(|q| q.band.as_str()).collect();
        assert_eq!(
            bands,
            ["70cm", "20m"],
            "the per-contact band field stopped recording the log's band at log time"
        );
    }

    #[test]
    fn arrl_scoring_per_mode_and_band_mode_dupes() {
        // ARRL FD: phone 1 pt, CW/digital 2 pts; a station counts once per
        // band PER MODE CLASS (the old (call, band) dupe key under-counted —
        // K1ABC on 20m CW and 20m FT8 are two legal contacts).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 100));
        assert!(
            log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, 110),
            "same call, new mode"
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "PH", 0, 120));
        assert!(
            !log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 130),
            "band+mode dupe"
        );
        assert!(
            !log.log_mode_at("k1abc", "2A", "CT", "dig", 0, 140),
            "case-insensitive dupe"
        );
        assert_eq!(log.qso_count(), 3);
        assert_eq!(log.qso_points(), 2 + 2 + 1, "DIG 2 + CW 2 + PH 1");
        // Cabrillo mode tokens per row.
        let cab = log
            .cabrillo(14074)
            .expect("a single-mode event exports one entry");
        assert!(cab.contains(" DG "));
        assert!(cab.contains(" CW "));
        assert!(cab.contains(" PH "));
        // The rules-data stamp (Cabrillo-legal X- header, robots ignore it).
        assert!(cab.contains("X-NEXUS-RULES-YEAR: 2026\n"));
        // WFD event flips the contest ids in both exports.
        log.set_event(FdEvent::WinterFd);
        assert!(log
            .cabrillo(14074)
            .expect("a single-mode event exports one entry")
            .contains("CONTEST: WFD"));
        assert!(log.adif().contains("WFD"));
    }

    #[test]
    fn cabrillo_lines_carry_real_utc_timestamps() {
        // 2026-06-27 18:05:00 UTC (Field Day Saturday) = 1782583500.
        let (d, t) = cabrillo_datetime(1_782_583_500);
        assert_eq!((d.as_str(), t.as_str()), ("2026-06-27", "1805"));
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_at("K1ABC", "2A", "CT", 4, 1_782_583_500));
        let cab = log
            .cabrillo(14074)
            .expect("a single-mode event exports one entry");
        // Frequency is band-derived (20m → 14000 kHz), not the passed dial.
        assert!(
            cab.contains("QSO: 14000 DG 2026-06-27 1805 W9XYZ 3A WI K1ABC 2A CT"),
            "real date/time on the QSO line (ARRL submission requires it): {cab}"
        );
        assert!(!cab.contains("----------"), "no placeholder when stamped");
    }

    #[test]
    fn a_tempo_tier_contact_exports_as_a_mode_adif_actually_defines() {
        // ⚠️ THIS WAS A LOST CONTACT, not a formatting nit. The Field Day ADIF wrote the
        // recorded mode straight into <MODE>, and the tiers stamp names ADIF has never heard
        // of — TempoFast, TempoDeep, FT2. TQSL walks MODE%SUBMODE → SUBMODE → MODE and a bare
        // <MODE:10>TempoFast misses all three legs, so the record is DROPPED with "Invalid
        // MODE". The main logbook has solved this for years in `logbook::adif_submode`; the
        // Field Day exporter just never called it, so a Field Day contact worked on a Tempo
        // tier vanished on upload while an FT8 one beside it went through.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_submode_at("K1ABC", "2A", "CT", "DIG", "TempoFast", 4, 1_782_583_500));
        let adif = log.adif();
        assert!(
            adif.contains("<MODE:4>MFSK") && adif.contains("<SUBMODE:9>TEMPOFAST"),
            "a Tempo tier must ride as MODE=MFSK + SUBMODE, or TQSL drops the record: {adif}"
        );
        assert!(
            !adif.contains("<MODE:9>TempoFast"),
            "TempoFast is not an ADIF MODE and must never be written as one: {adif}"
        );

        // POSITIVE CONTROL: a mode ADIF really does define is written plainly, with no
        // SUBMODE invented for it — otherwise this test would pass against a change that
        // wrapped everything.
        let mut plain = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(plain.log_submode_at("K2DEF", "2A", "CT", "DIG", "RTTY", 4, 1_782_583_500));
        let plain_adif = plain.adif();
        assert!(
            plain_adif.contains("<MODE:4>RTTY"),
            "RTTY is a real MODE: {plain_adif}"
        );
        assert!(
            !plain_adif.contains("SUBMODE"),
            "…and needs no SUBMODE: {plain_adif}"
        );
    }

    #[test]
    fn cabrillo_says_the_band_the_contact_was_actually_made_on() {
        // ⚠️ TWO WAYS THIS LIED ABOUT A CONTACT'S BAND, both found by auditing our output
        // against the Cabrillo spec and Winter Field Day's parser.
        //
        // 1. ABOVE 50 MHz THE FIELD IS A BAND TOKEN, NOT KILOHERTZ. The spec's own words:
        //    "freq is frequency or band: 1800 or actual frequency in kHz […] 50 / 70 / 144 /
        //    222 / 432 / 902 / 1.2G". WFD states the same rule and MACHINE-PARSES the QSO
        //    lines out of whatever is uploaded, so a 6-digit value where a token belongs is a
        //    parse problem there. 70 cm was wrong twice over — the token is 432 and 420000 was
        //    not even a sensible kHz for it.
        // 2. AN UNRECOGNISED BAND TOOK THE CALLER'S FALLBACK, and the club exporter passed a
        //    HARDCODED 14 MHz. A 23 cm club contact exported as 20 m. ARRL Field Day does not
        //    read Cabrillo at all, but it DOES require a list of stations worked sorted BY
        //    BAND — so this corrupted the one artifact they actually use.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        for (band, call) in [
            ("20m", "K1ABC"),
            ("6m", "K2DEF"),
            ("2m", "K3GHI"),
            ("70cm", "K4JKL"),
            ("23cm", "K5MNO"),
            ("4m", "K6PQR"),
        ] {
            log.band = band.to_string();
            assert!(
                log.log_at(call, "2A", "CT", 4, 1_782_583_500),
                "{band} logged"
            );
        }
        let cab = log
            .cabrillo(14_000)
            .expect("a single-mode event exports one entry");

        // HF stays kHz.
        assert!(
            cab.contains("QSO: 14000 DG 2026-06-27 1805 W9XYZ 3A WI K1ABC"),
            "20m: {cab}"
        );
        // Above 50 MHz is the BAND, per the spec's own list.
        for (token, call) in [("50", "K2DEF"), ("144", "K3GHI"), ("432", "K4JKL")] {
            assert!(
                cab.contains(&format!(
                    "QSO: {token} DG 2026-06-27 1805 W9XYZ 3A WI {call}"
                )),
                "expected band token {token} for {call}: {cab}"
            );
        }
        // 4 m and 23 cm are legal Field Day bands and were not in the table at all — they
        // took the caller's fallback and claimed to be somewhere else entirely.
        assert!(
            cab.contains("QSO: 70 DG 2026-06-27 1805 W9XYZ 3A WI K6PQR"),
            "4m: {cab}"
        );
        assert!(
            cab.contains("QSO: 1.2G DG 2026-06-27 1805 W9XYZ 3A WI K5MNO"),
            "23cm: {cab}"
        );
        // THE PIN: nothing above 6 m may claim to be on 20 m.
        assert_eq!(
            cab.matches("QSO: 14000 ").count(),
            1,
            "only the 20 m contact may say 14000 — a band we cannot map must never borrow \
             another band's frequency: {cab}"
        );
    }

    #[test]
    fn adif_round_trip_restores_log_and_dupes() {
        // The FD contest log lives only in memory, so the exit flush's ADIF is
        // its sole durable copy — merging it back into a fresh log (a restart
        // mid-event) must restore the QSOs, the dupe index, the sections and
        // real timestamps (not the '----------' Cabrillo placeholder).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_560));
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        let adif = log.adif();

        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&adif, 0);
        assert_eq!(restored.qso_count(), 3, "all three contacts restored");
        for (call, mode) in [("K1ABC", "DIG"), ("K2DEF", "CW"), ("N0GHI", "PH")] {
            assert!(
                restored.is_dupe_mode(call, mode),
                "{call} {mode} restored into the dupe index"
            );
        }
        assert_eq!(restored.sections(), 3, "sections survive the round-trip");
        assert_eq!(restored.qso_points(), 2 + 2 + 1, "DIG 2 + CW 2 + PH 1");
        let cab = restored
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        assert!(
            cab.contains("2026-06-27"),
            "restored rows keep their real timestamp: {cab}"
        );
        assert!(!cab.contains("----------"), "no placeholder after restore");
    }

    #[test]
    fn adif_and_cabrillo_carry_the_actual_digital_mode() {
        // An RTTY WFD QSO must never export MODE=FT8 (a banned mode there) —
        // the recorded actual mode wins; rows without one keep the legacy
        // FT8/DG fallback so old logs export unchanged.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2M", "EPA"),
            "20m",
        );
        log.set_event(FdEvent::WinterFd);
        assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", " rtty ", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1O", "EMA", "DIG", 0, 1_782_583_560));
        let adif = log.adif();
        assert!(
            adif.contains("<MODE:4>RTTY"),
            "RTTY row exports its real (normalized) mode: {adif}"
        );
        assert!(adif.contains("<MODE:3>FT8"), "unrecorded row falls back");
        let cab = log
            .cabrillo(14_080)
            .expect("a single-mode event exports one entry");
        assert!(cab.contains(" RY "), "Cabrillo RY for the RTTY row: {cab}");
        assert!(cab.contains(" DG "), "DG fallback for the unrecorded row");
        // FD/WFD digital is ONE mode class: an RTTY try after the same-band
        // FT8 contact is a dupe, and both rows score the digital 2 points.
        assert!(
            !log.log_submode_at("K2DEF", "1O", "EMA", "DIG", "RTTY", 0, 1_782_583_620),
            "RTTY dupes the same-band digital contact"
        );
        assert_eq!(log.qso_points(), 4);
    }

    #[test]
    fn actual_mode_survives_the_adif_round_trip() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2M", "EPA"),
            "20m",
        );
        log.set_event(FdEvent::WinterFd);
        assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", "RTTY", 0, 1_782_583_500));
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "2M", "EPA"),
            "20m",
        );
        restored.set_event(FdEvent::WinterFd);
        restored.merge_adif(&log.adif(), 0);
        assert_eq!(restored.qso_count(), 1);
        let q = &restored.qsos()[0];
        assert_eq!((q.mode.as_str(), q.submode.as_str()), ("DIG", "RTTY"));
        assert!(
            restored.is_dupe_mode("K1ABC", "DIG"),
            "dupe key restored on the mode CLASS"
        );
        assert!(
            restored.adif().contains("<MODE:4>RTTY"),
            "re-export keeps RTTY"
        );
        assert!(restored
            .cabrillo(14_080)
            .expect("a single-mode event exports one entry")
            .contains(" RY "));
    }

    #[test]
    fn seq_is_stamped_monotonic_and_round_trips_as_app_nexus_qseq() {
        // Every entry point funnels into log_submode_at, so seqs are 1..n in
        // log order regardless of mode/path.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_560));
        assert_eq!(log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(log.max_seq(), 2);
        let adif = log.adif();
        assert!(
            adif.contains("<APP_NEXUS_QSEQ:1>1") && adif.contains("<APP_NEXUS_QSEQ:1>2"),
            "seq journaled per row: {adif}"
        );

        // Restore keeps each row's seq and the NEXT log entry continues past
        // the restored high-water — the property that makes a restart safe
        // against re-issuing an id the club host already merged.
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&adif, 0);
        assert_eq!(
            restored.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [1, 2],
            "seqs survive the round-trip"
        );
        assert!(restored.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        assert_eq!(
            restored.qsos()[2].seq,
            3,
            "fresh rows continue the sequence"
        );
    }

    #[test]
    fn legacy_journal_without_qseq_backfills_in_row_order() {
        // A pre-sync journal has no APP_NEXUS_QSEQ tags at all: restore assigns
        // 1..n in row order (deterministic, so a re-restore re-derives the same
        // ids) and new contacts continue from there.
        let legacy = "<EOH>\n\
            <CALL:5>K1ABC <MODE:3>FT8 <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180500 \
            <CLASS:2>2A <ARRL_SECT:2>CT <EOR>\n\
            <CALL:5>K2DEF <MODE:2>CW <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180600 \
            <CLASS:2>1D <ARRL_SECT:3>EMA <EOR>\n";
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.merge_adif(legacy, 0);
        assert_eq!(
            log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [1, 2],
            "legacy rows backfill 1..n in row order"
        );
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        assert_eq!(log.qsos()[2].seq, 3);
        // A mixed journal (one stamped row, one legacy) never collides: the
        // backfill takes the next free seq at the point the row restores.
        let mixed = "<EOH>\n\
            <CALL:5>K1ABC <MODE:3>FT8 <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180500 \
            <CLASS:2>2A <ARRL_SECT:2>CT <APP_NEXUS_QSEQ:1>5 <EOR>\n\
            <CALL:5>K2DEF <MODE:2>CW <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180600 \
            <CLASS:2>1D <ARRL_SECT:3>EMA <EOR>\n";
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        log.merge_adif(mixed, 0);
        assert_eq!(
            log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [5, 6],
            "backfill continues past a restored explicit seq"
        );
        assert_eq!(log.max_seq(), 6);
    }

    #[test]
    fn merge_adif_age_gate_and_garbage_merge_nothing() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        let adif = log.adif();

        // A min_when_unix newer than every row (a previous event's journal)
        // restores nothing — the backup self-expires.
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        restored.merge_adif(&adif, 1_782_583_501);
        assert_eq!(restored.qso_count(), 0, "expired journal merges nothing");

        // Garbage input merges nothing and never errors.
        restored.merge_adif("not adif <CALL:junk><EOR> \u{fe0f}<QSO_DATE:8>x<EOR>", 0);
        assert_eq!(restored.qso_count(), 0, "garbage merges nothing");
    }

    use super::*;
    use crate::contest::ContestSession;

    fn dec(msg: &str) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr: -5,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            raw: None,
            mode: None,
        }
    }

    #[test]
    fn dupe_check_and_scoring() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert!(log.log("K2DEF", "3A", "IL", 1));
        assert!(log.log("N0ABC", "1D", "MN", 2));
        assert!(!log.log("K2DEF", "3A", "IL", 3)); // dupe on same band
        assert_eq!(log.qso_count(), 2);
        assert_eq!(log.sections(), 2); // IL, MN
        assert_eq!(log.qso_points(), 4); // 2 pts each
        assert!(log.adif().contains("ARRL_SECT") && log.adif().contains("K2DEF"));
        let cab = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        assert_eq!(cab.matches("QSO:").count(), 2);
        assert!(cab.contains("W9XYZ 3A WI K2DEF 3A IL"));
    }

    #[test]
    fn cabrillo_frequency_is_per_qso_band_not_the_export_dial() {
        // Each row's frequency comes from the QSO's own band, not the single dial
        // the export happened to pass (which stamped every row before the fix).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_at("K1ABC", "2A", "CT", 1, 1_782_583_500)); // 20m
        let cab = log
            .cabrillo(99999)
            .expect("a single-mode event exports one entry"); // dial fallback must NOT appear for a known band
        assert!(
            cab.contains("QSO: 14000 "),
            "20m QSO stamped 14000 kHz: {cab}"
        );
        assert!(
            !cab.contains("QSO: 99999 "),
            "the export dial is not stamped on a known band"
        );
    }

    #[test]
    fn worked_sections_returns_the_distinct_set_sorted() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert!(log.log("K2DEF", "3A", "IL", 1));
        assert!(log.log("N0ABC", "1D", "MN", 2));
        assert!(log.log_mode_at("W1AW", "2A", "IL", "CW", 0, 100)); // IL again, new mode
                                                                    // The count and the identities agree; identities are the distinct set,
                                                                    // sorted (IL once, though worked twice), and stable.
        assert_eq!(log.worked_sections(), vec!["IL", "MN"]);
        assert_eq!(log.worked_sections().len(), log.sections());
    }

    #[test]
    fn observe_runs_sp_side() {
        // S&P station hears a CQ, sends exchange, then the runner's rogered exch.
        let mut sp = FieldDayStation::search_and_pounce(
            "K2DEF",
            "FN31",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "IL"),
            "20M",
        );
        sp.observe(&[dec("CQ W9XYZ EN37")], 0);
        assert_eq!(sp.state, FdState::AwaitExchange);
        assert_eq!(sp.outgoing().unwrap().to_text(), "W9XYZ K2DEF 2A IL");
        sp.observe(&[dec("K2DEF W9XYZ R 3A WI")], 1);
        assert_eq!(sp.state, FdState::Done);
        assert_eq!(sp.log.qso_count(), 1);
        assert_eq!(sp.log.qsos()[0].class(), "3A");
        assert_eq!(sp.log.qsos()[0].section(), "WI");
    }

    #[test]
    fn running_station_calls_directed_cq_fd() {
        // The running station advertises a DIRECTED Field Day CQ so it reads as
        // "CQ FD" on the air — and an S&P station still answers it.
        let run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        let cq = run.outgoing().unwrap().to_text();
        assert!(cq.contains("CQ FD"), "directed FD CQ: {cq}");
        assert_eq!(cq, "CQ FD W9XYZ EN37");
        let mut sp = FieldDayStation::search_and_pounce(
            "K2DEF",
            "FN31",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "IL"),
            "20M",
        );
        sp.observe(&[dec(&cq)], 0);
        assert_eq!(
            sp.state,
            FdState::AwaitExchange,
            "S&P answers a directed FD CQ"
        );
    }

    #[test]
    fn running_station_rearms_and_works_a_second_caller() {
        // Regression for the RUN dead-end: a running station worked exactly one
        // contact and then went silent (Done, no return to CQ). It must re-arm.
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0); // caller's exchange
        assert_eq!(run.state, FdState::AwaitConfirm);
        run.observe(&[dec("W9XYZ K2DEF RR73")], 1); // caller confirms → we log
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        assert!(run.done(), "closed after the first QSO");

        // The fix: the engine re-arms a `done()` RUN station back to calling CQ.
        run.rearm(true);
        assert_eq!(run.state, FdState::CallingCq);
        assert!(
            run.outgoing().unwrap().to_text().contains("CQ FD"),
            "back on CQ FD after re-arm"
        );

        // It now works a SECOND caller, and the first is still a dupe (the
        // in-station contest log survived the re-arm).
        run.observe(&[dec("W9XYZ N0ABC 1D MN")], 2);
        assert_eq!(run.state, FdState::AwaitConfirm, "answers the next caller");
        run.observe(&[dec("W9XYZ N0ABC RR73")], 3);
        assert_eq!(run.log.qso_count(), 2, "second contact logs after re-arm");
        assert!(run.log.is_dupe("K2DEF"), "first caller remains a dupe");
    }

    #[test]
    fn loopback_completes_the_exchange_on_the_happy_path() {
        // The full round-trip over the real modem still completes unchanged after
        // the directed-CQ-FD switch: both stations log the OTHER's exchange.
        let mut running = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        let mut sp = FieldDayStation::search_and_pounce(
            "K2DEF",
            "FN31",
            ContestSession::field_day(FdEvent::ArrlFd, "2A", "IL"),
            "20M",
        );
        run_loopback_fieldday(&mut running, &mut sp, 15.0, 40);
        assert_eq!(
            running.log.qso_count(),
            1,
            "running: {:?}",
            running.transcript
        );
        assert_eq!(sp.log.qso_count(), 1, "sp: {:?}", sp.transcript);
        assert_eq!(
            running.log.qsos()[0].section(),
            "IL",
            "running logged the caller"
        );
        assert_eq!(sp.log.qsos()[0].section(), "WI", "S&P logged the runner");
    }

    #[test]
    fn running_logs_on_a_bare_73() {
        // Caller closes with a bare `73` instead of RR73/RRR: the contact must
        // still complete + log (the caller's exchange was captured at CallingCq),
        // not stall in AwaitConfirm.
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0); // caller's exchange
        assert_eq!(run.state, FdState::AwaitConfirm);
        run.observe(&[dec("W9XYZ K2DEF 73")], 1); // bare 73, not RR73
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        let q = &run.log.qsos()[0];
        assert_eq!(
            (q.call.as_str(), q.class(), q.section()),
            ("K2DEF", "2A", "IL")
        );
    }

    #[test]
    fn running_skips_ahead_on_an_early_rogered_exchange() {
        // A caller that ROGERS its exchange while we're still calling CQ (the
        // plain exchange dropped, or the caller pre-rogered): the class + section
        // are in the rogered frame, so skip straight to logging + close with RR73
        // instead of stalling on CQ.
        let mut run = FieldDayStation::running(
            "W9XYZ",
            "EN37",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20M",
        );
        assert_eq!(run.state, FdState::CallingCq);
        run.observe(&[dec("W9XYZ K2DEF R 2A IL")], 0); // rogered, skipping the plain form
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        let q = &run.log.qsos()[0];
        assert_eq!(
            (q.call.as_str(), q.class(), q.section()),
            ("K2DEF", "2A", "IL")
        );
        assert_eq!(
            run.outgoing().unwrap().to_text(),
            "K2DEF W9XYZ RR73",
            "closes with RR73 to the caller"
        );
    }

    // -- §3.3: the sent exchange is a per-ROW value ---------------------------

    /// The log a mobile makes: two rows either side of a move, and the move must not
    /// reach backwards.
    ///
    /// Field Day has no county, so the move here is a SECTION change. That is the same
    /// mechanism (`session.my_exchange` is edited between contacts) on the exchange
    /// this build actually ships, which is the only exchange there is to test it on.
    fn moved_log() -> FieldDayLog {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_000_000));
        // — "I moved": the declared location AND the exchange derived from it, which
        // is one action and not two. The Cabrillo `LOCATION` header follows the
        // session (it is a per-ENTRY value); the QSO lines follow their own rows.
        log.session.my_location.state = "IL".into();
        log.session.my_exchange = vec![
            log.session.exchange.value("CLASS", "3A").unwrap(),
            log.session.exchange.value("SECTION", "IL").unwrap(),
        ];
        assert!(log.log_mode_at("W1AW", "1D", "CT", "CW", 0, 1_782_000_060));
        log
    }

    /// ⭐ **§10's mobile-history test.** Log either side of a move, export, and the
    /// first row still carries what it actually sent.
    #[test]
    fn moving_the_session_does_not_relabel_a_row_already_logged() {
        let log = moved_log();
        let cbr = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let lines: Vec<&str> = cbr.lines().filter(|l| l.starts_with("QSO:")).collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains("W9XYZ 3A WI K1ABC 2A EMA"),
            "row 1 was relabelled by a move that happened after it: {}",
            lines[0]
        );
        assert!(
            lines[1].contains("W9XYZ 3A IL W1AW 1D CT"),
            "row 2 does not carry the exchange it sent: {}",
            lines[1]
        );
        // The header is the ENTRY's declared location — the session's, deliberately
        // not row 1's. Cabrillo puts it in a header, once, for exactly that reason.
        assert!(cbr.contains("\nLOCATION: IL\n"), "{cbr}");
    }

    /// POSITIVE CONTROL for the test above, and it is the whole reason that green is
    /// evidence. The same fixture written the way `cabrillo()` used to write it — the
    /// LOG's one sent exchange on every line — MUST produce different bytes. If it does
    /// not, the assertions pass because the two values happen to agree rather than
    /// because the writer reads the row.
    #[test]
    fn the_mobile_history_assertion_discriminates() {
        let log = moved_log();
        let from_the_row = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let from_the_session: String = from_the_row
            .lines()
            .map(|l| {
                if l.starts_with("QSO:") {
                    // The defect, reconstructed: every line takes the session's
                    // CURRENT sent exchange instead of its own row's.
                    l.replace("W9XYZ 3A WI", "W9XYZ 3A IL")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(
            from_the_row.trim_end(),
            from_the_session.trim_end(),
            "the byte comparison cannot see a relabelled row"
        );
    }

    /// ⭐ **A moved sent exchange survives a restart**, which is what makes the test
    /// above hold across the journal rather than only in memory.
    ///
    /// The pre-move row's `tx` differs from the session's current one, so the writer
    /// stamps it with `APP_NEXUS_MYEX`; the post-move row matches and falls back. Both
    /// come back, which is why the exported bytes are unchanged.
    #[test]
    fn a_moved_sent_exchange_survives_the_journal() {
        let log = moved_log();
        let journal = log.adif();
        assert!(
            journal.contains("APP_NEXUS_MYEX"),
            "the row that no longer matches the session was journaled with no sent side"
        );
        let mut restored = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "IL"),
            "20m",
        );
        restored.merge_adif(&journal, 0);
        assert_eq!(restored.qso_count(), 2);
        assert_eq!(
            restored
                .cabrillo(14_074)
                .expect("a single-mode event exports one entry"),
            log.cabrillo(14_074)
                .expect("a single-mode event exports one entry")
        );
        // POSITIVE CONTROL: strip the carrier and the pre-move row falls back to the
        // session's current exchange, which is the WRONG section — so the tag is what
        // is carrying the answer, not the fallback happening to be right.
        let stripped: String = journal
            .lines()
            .map(|l| match l.find("<APP_NEXUS_MYEX:") {
                Some(i) => format!("{}{}", &l[..i], &l[l.rfind("<EOR>").unwrap_or(l.len())..]),
                None => l.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut blind = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "IL"),
            "20m",
        );
        blind.merge_adif(&stripped, 0);
        assert_ne!(
            blind
                .cabrillo(14_074)
                .expect("a single-mode event exports one entry"),
            log.cabrillo(14_074)
                .expect("a single-mode event exports one entry"),
            "APP_NEXUS_MYEX is not what restores the pre-move row"
        );
    }

    /// §4, direction 2: being the mobile is what `by_sent_fields` is for, and Field
    /// Day declares neither list — so the same station on the same band and mode is a
    /// dupe whatever the session is sending, and the key is exactly the shipped tuple.
    #[test]
    fn field_days_key_ignores_the_sent_side_because_its_rule_names_no_slots() {
        let mut log = moved_log();
        let rule = log.dupe_rule();
        assert_eq!(rule.by_fields, &[] as &[&str]);
        assert_eq!(rule.by_sent_fields, &[] as &[&str]);
        assert!(
            !log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_000_120),
            "a move must not un-dupe a station under a rule that names no sent slot"
        );
    }
}

#[cfg(test)]
mod cabrillo_header_tests {
    use super::*;
    use crate::contest::{ContestSession, OperatorCategory};

    fn one_qso_log(entry: OperatorCategory) -> FieldDayLog {
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.entry_category = entry;
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        log
    }

    /// ⭐ THE SHIPPED BUG. `cabrillo()` wrote `CATEGORY-OPERATOR: MULTI-OP` as a
    /// string literal, so every solo Field Day entry Nexus has ever exported claimed
    /// more than one operator was at the station — in the file ARRL scores.
    #[test]
    fn a_single_operator_entry_declares_single_op() {
        let cab = one_qso_log(OperatorCategory::SingleOp)
            .cabrillo(14_074)
            .expect("a single-mode event exports");
        assert!(
            cab.contains("CATEGORY-OPERATOR: SINGLE-OP\n"),
            "a single-op entry still submits a MULTI-OP header:\n{cab}"
        );
        assert!(
            !cab.contains("MULTI-OP"),
            "the literal survived somewhere:\n{cab}"
        );
    }

    /// POSITIVE CONTROL: a club entry still declares `MULTI-OP`, so the test above is
    /// the declaration being read and not the token being renamed.
    #[test]
    fn a_club_entry_still_declares_multi_op() {
        let cab = one_qso_log(OperatorCategory::MultiOp)
            .cabrillo(14_074)
            .expect("a single-mode event exports");
        assert!(cab.contains("CATEGORY-OPERATOR: MULTI-OP\n"), "{cab}");
    }

    /// ⭐ §6.2 — **both callsign columns are on the line, and Sweepstakes gets
    /// exactly two, not four.**
    ///
    /// The template a previous draft carried was exchange slots only, which loses
    /// both callsigns on every contest and produces an unsubmittable line. Field Day's
    /// golden pins the structural columns; this pins the derived exception — a side
    /// whose slot list declares a `Call` slot does not write that slot a SECOND time
    /// among the exchange columns, because the structural column already is it.
    ///
    /// ⚠️ **The expected line is ARRL's own published template, and it corrects §6.2.**
    /// The design sketch assumed the opposite exception (structural column suppressed,
    /// call third of five) and explicitly reserved the question — *"the one thing this
    /// walk does not settle is the column order, and it must not"* — for the batch that
    /// read the sponsor. <https://www.arrl.org/cabrillo-format-tutorial>, ARRL's own
    /// Cabrillo Format & Tutorial page, read 2026-09-09, publishes a **QSO DATA
    /// TEMPLATE** for Sweepstakes with a lettered legend:
    ///
    /// ```text
    /// QSO: 14000 CW 2009-11-07 2100 W1AW   1 M 38 CT K8MM   1 Q 92 MI
    /// ```
    /// > *"E= Your call. F= Your QSO #. G= Your precedence. H= Your check (the last two
    /// > numbers of the year you were first licensed). I= Your ARRL Section. J= The call
    /// > of the station you worked. K= Their QSO number to you. L= Their precedence.
    /// > M= Their check. N = Their ARRL Section."*
    ///
    /// The callsign is column **E** — first of the side, the structural position every
    /// other contest uses — and serial, precedence, check and section follow it with no
    /// callsign among them. The line below is that shape, contact for contact.
    #[test]
    fn a_sweepstakes_line_carries_each_callsign_exactly_once() {
        let mut session = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        session.exchange = crate::contest::exchanges::sweepstakes_shaped();
        let mut log = FieldDayLog::new("W9XYZ", session, "20m");
        let spec = log.session.exchange;
        let v = |key: &'static str, raw: &str| crate::contest::FieldValue {
            key: spec.field(key).expect("declared slot").key,
            raw: raw.to_string(),
            domain: None,
        };
        // "1 A W9XYZ 74 WI" sent; K2DEF answers with "12 A K2DEF 71 CT".
        let tx = ["1", "A", "W9XYZ", "74", "WI"];
        let rx = ["12", "A", "K2DEF", "71", "CT"];
        let slots = ["NR", "PREC", "CALL", "CK", "SEC"];
        assert!(log.log_exchange_at(
            "K2DEF",
            slots.iter().zip(rx).map(|(k, r)| v(k, r)).collect(),
            slots.iter().zip(tx).map(|(k, r)| v(k, r)).collect(),
            "CW",
            "",
            0,
            1_782_583_500,
        ));
        let cab = log
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let line = cab
            .lines()
            .find(|l| l.starts_with("QSO:"))
            .expect("one QSO line");
        assert_eq!(
            line, "QSO: 14000 CW 2026-06-27 1805 W9XYZ 1 A 74 WI K2DEF 12 A 71 CT",
            "ARRL's own template: <your call> <NR> <PREC> <CK> <SEC> <their call> \
             <NR> <PREC> <CK> <SEC> — the CALL slot is the structural column and is \
             not repeated inside the exchange"
        );
        // §6.2's own control, in its own words: exactly two callsign occurrences.
        assert_eq!(line.matches("W9XYZ").count(), 1, "{line}");
        assert_eq!(line.matches("K2DEF").count(), 1, "{line}");
    }

    /// POSITIVE CONTROL for the line above: Field Day declares no `Call` slot, so
    /// both structural columns ARE written — a writer that suppressed them for
    /// everyone would pass the "exactly once" assertions above by losing them.
    #[test]
    fn a_field_day_line_still_carries_both_structural_callsigns() {
        let cab = one_qso_log(OperatorCategory::SingleOp)
            .cabrillo(14_074)
            .expect("a single-mode event exports one entry");
        let line = cab
            .lines()
            .find(|l| l.starts_with("QSO:"))
            .expect("one QSO line");
        assert_eq!(
            line, "QSO: 14000 CW 2026-06-27 1805 W9XYZ 3A WI K1ABC 2A EMA",
            "my call, my exchange, their call, their exchange"
        );
    }

    /// The trailing transmitter column, and its absence. Neither Field Day sponsor's
    /// template has one, so a Field Day line must not grow a column; a session that
    /// declares one gets it, last.
    #[test]
    fn the_transmitter_column_rides_only_where_a_template_declares_it() {
        let mut log = one_qso_log(OperatorCategory::SingleOp);
        let without = log.cabrillo(14_074).expect("exports");
        assert!(
            without.ends_with("K1ABC 2A EMA\nEND-OF-LOG:\n"),
            "Field Day's line ends with their exchange:\n{without}"
        );
        log.session.transmitter_id = Some(1);
        let with = log.cabrillo(14_074).expect("exports");
        assert!(
            with.ends_with("K1ABC 2A EMA 1\nEND-OF-LOG:\n"),
            "the transmitter id is the LAST column:\n{with}"
        );
    }

    /// …and a checklog, which is neither.
    #[test]
    fn a_checklog_entry_declares_checklog() {
        let cab = one_qso_log(OperatorCategory::Checklog)
            .cabrillo(14_074)
            .expect("a single-mode event exports");
        assert!(cab.contains("CATEGORY-OPERATOR: CHECKLOG\n"), "{cab}");
    }
}
