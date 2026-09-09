//! The contest session — a DURABLE object with its own lifecycle.
//!
//! ⭐ **The load-bearing rule of this module: the sent exchange is a per-ROW value.
//! The session holds only the CURRENT one.** [`ContestSession::my_exchange`] is the
//! SOURCE a new row copies from; it is never the RECORD of a row already logged. A
//! mobile changes it mid-contest and every row already in the log must keep what it
//! actually sent.
//!
//! ⭐ **And it has no renderer.** `my_exchange` is a bare `Vec<FieldValue>` with no
//! `Display`, no `to_string` and no join helper anywhere in this crate. The one
//! function that renders a sent exchange is
//! [`sent_exchange`](super::render::sent_exchange), and it takes a ROW. Writing the
//! defect this module exists to prevent therefore requires hand-rolling a `format!`
//! over a vector of structs — which is visible in review, instead of looking exactly
//! like correct code. Three separate emitters had that defect before the rule existed;
//! `render.rs`'s `no_renderer_reaches_a_session` guard is what keeps a fourth from
//! being written.
//!
//! **Lifecycle: restored, never rebuilt.** Entering a contest mode RESTORES an open
//! session and creates one only when none is open. Rebuilding it from settings on
//! every mode entry is what made a Run↔S&P toggle silently revert an "I moved" — the
//! rows after the toggle went back to sending the old county, on the air.
use super::spec::{ExchangeSpec, FieldValue, RoleSelector, RoleSpec};

/// Where the operator is, as DECLARED — the durable input, not the derived output.
///
/// A mobile crossing a state line changes ROLE, not just QTH, so a resolved-once
/// `role: String` cannot follow it. The session stores this and
/// [`ContestSession::role`] is a function of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MyLocation {
    /// The county I am in, for a QSO party's in-state role. `None` everywhere else.
    pub county: Option<String>,
    /// My state or province — and, for Field Day, my ARRL/RAC section, which is
    /// what Cabrillo's `LOCATION` header means for that event (§6.1).
    pub state: String,
    /// I am outside the W/VE role space entirely (the `dx` role of a QSO party).
    pub dxcc: bool,
}

/// The contact being worked right now, and the exchange ISSUED to it.
///
/// This is where a serial lives between being composed and being logged. Without it
/// the only value reachable at log time is the session's live counter — the
/// session-level-value-describing-a-particular-QSO shape this module exists to remove,
/// surviving one step upstream of where it was removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlightQso {
    /// The peer this exchange was composed for. A second compose for the SAME peer
    /// (a repeat, an AGN, a re-call) renders from here and issues nothing.
    pub peer: String,
    /// The exchange as composed — including the serial that was issued.
    pub tx: Vec<FieldValue>,
    pub since_unix: u64,
}

/// ⭐ **Where this session's merged contacts go — per session, default OFF (§18.1).**
///
/// The merge (`super::merge`) is the only path by which a contest contact becomes a
/// `QsoRecord`, and the general log path enqueues every record it writes to every
/// connector. That is exactly the behaviour the Field-Day upload invariant was written
/// to prevent, moved from log time to merge time — so the merge may enqueue, but only
/// when the operator has turned it on **for this session**, and the session carries its
/// own destination.
///
/// **Why a property of the session and not a setting.** The operator report this
/// answers is *"Nexus sends my contacts to my general logbook on WRL — how do I change
/// it for the QSO party?"* A global toggle is one the operator must remember to change
/// back, and then back again; a session that ends takes its answer with it.
///
/// **Both halves are required, and neither alone opts anybody in** — the shape the
/// standing N1MM broadcast already uses: a switch with nowhere to send is off, and a
/// destination alone must not enqueue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadPolicy {
    /// The switch. **`false` on every new session**, which is the ruling.
    pub enabled: bool,
    /// The destination logbooks, by connector id (`"qrz"`, `"clublog"`, `"wrl"`, …).
    /// Empty means nowhere, so an enabled policy with no destination enqueues nothing.
    pub destinations: Vec<String>,
}

/// ⚠️ **What the per-session control does NOT close, stated where the operator reads
/// it.** ClubLog's catch-up sweep re-queues every logged QSO ClubLog never accepted,
/// regardless of this control — so merged contest contacts WILL reach ClubLog the next
/// time a ClubLog password is saved. An operator who reads "upload: off" and gets a
/// ClubLog upload anyway has been misled by us, not surprised by ClubLog. Permanent
/// per-connector exclusion needs a per-record connector mask at merge time, which is
/// not in this pass.
pub const UPLOAD_CLUBLOG_SWEEP_HINT: &str = "Off means this session's contacts are not \
queued for upload when you merge them into your logbook. One exception, and it is not \
ours to switch off: saving a ClubLog password re-queues every contact ClubLog has not \
accepted, including these.";

/// ⭐ **§3.3 ruling 3, as the message the operator reads.** A location change that
/// resolves to a DIFFERENT role is not a move: it flips `sends`, `receives` and the
/// multiplier universe, which is a separate entry and a separate Cabrillo file. The
/// action refuses and offers the correct one instead of inventing a mixed-shape log
/// no sponsor's template describes.
pub const MOVE_CHANGES_ROLE: &str = "You've moved into a different multiplier region — \
that's a separate entry. End this session and start a new one?";

/// ⭐ **The station data §3.4 names as the source of a sent slot** — the operator's
/// answers, as one value object, so a session constructor reads them by the SAME names
/// the rules file declares.
///
/// It is a struct rather than a `&Settings` because this crate holds no settings and
/// must not learn to: the chain the loader validates is *rules file names a source →
/// `SENT_SLOT_SETTINGS` says this build can supply it → `tempo-app` proves every name
/// in that list is a real serde field*. This type is the third link's shape. Adding a
/// field here without adding it to `SENT_SLOT_SETTINGS` reaches nothing; adding it
/// there without adding it here is a `Err` naming the slot, which is the loud half.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StationData {
    pub fd_class: String,
    pub fd_section: String,
    pub contest_qth_county: String,
    pub contest_qth_state: String,
    pub contest_check: String,
    pub contest_cq_zone: String,
    pub contest_itu_zone: String,
    pub contest_power: String,
    pub mygrid: String,
    /// I am outside the W/VE role space entirely — the `dx` role of a QSO party.
    /// Declared, never inferred from a blank state: an operator who has simply not
    /// filled the form in is not a DX entrant, and treating them as one would send
    /// `DX` on the air from Ohio.
    pub dxcc: bool,
}

impl StationData {
    /// The value a `"setting:<name>"` source names, or `None` for a name this build
    /// does not carry — which the caller turns into a refusal naming the slot.
    fn setting(&self, name: &str) -> Option<&str> {
        Some(match name {
            "fd_class" => &self.fd_class,
            "fd_section" => &self.fd_section,
            "contest_qth_county" => &self.contest_qth_county,
            "contest_qth_state" => &self.contest_qth_state,
            "contest_check" => &self.contest_check,
            "contest_cq_zone" => &self.contest_cq_zone,
            "contest_itu_zone" => &self.contest_itu_zone,
            "contest_power" => &self.contest_power,
            "mygrid" => &self.mygrid,
            _ => return None,
        })
    }
}

/// One run of one contest: the object BOTH logs carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestSession {
    /// Stable across restart; stamped on every row it owns.
    pub id: String,
    /// ⭐ **The RULES-FILE event id** (`"arrlfd"`, `"wfd"`, `"tnqp"`, …) — how every
    /// reader of this session finds the ruleset that governs it.
    ///
    /// It is not [`contest_id`](Self::contest_id): that is the Cabrillo token, which is
    /// the sponsor's vocabulary and not a key into the rules table. Until this field
    /// existed the log looked its ruleset up through `FdEvent::from_contest_id`, whose
    /// two arms are ARRL and Winter Field Day and whose else-branch is ARRL — so a
    /// Tennessee QSO Party session would have been duped, scored and headed under ARRL
    /// Field Day's rules without anything saying so.
    pub event_id: String,
    /// The Cabrillo `CONTEST` token — `"ARRL-FIELD-DAY"`, `"WFD"`, `"CQP"`.
    pub contest_id: String,
    pub rules_year: u16,
    /// The exchange this session runs.
    ///
    /// ⚠️ **A deviation from the design sketch, and the reason is mechanical.** §3's
    /// struct has no such field and gives `role()` the signature `fn role(&self) ->
    /// &'static RoleSpec`. A session cannot evaluate a [`RoleSelector`] without the
    /// role list, so either the spec rides on the session or every caller of `role()`
    /// passes it — and the second shape lets two callers disagree about which
    /// exchange a session is running. It rides here.
    pub exchange: &'static ExchangeSpec,
    /// WHERE I AM — the durable input `role` is derived from.
    pub my_location: MyLocation,
    /// The exchange I am sending RIGHT NOW: the template a composer fills and the
    /// entry strip shows. **The source of a row's sent side, never the record of
    /// one.** Copied onto a row at log time, and read for nothing else.
    pub my_exchange: Vec<FieldValue>,
    /// The contact in flight, and the exchange issued to it. Cleared on log or on
    /// abandon; a repeat renders FROM it.
    pub in_flight: Option<InFlightQso>,
    pub start_unix: u64,
    /// The ruleset window, or operator-set for an unlisted event.
    pub end_unix: u64,
    /// The next serial to ISSUE. The issued value lands in `in_flight.tx` and then on
    /// the row's own `tx`; this counter is state, not history.
    pub next_serial: u32,
    /// Operator-facing (`"TNQP 2026"`).
    pub label: String,
    /// ⭐ **The entry declaration Cabrillo's `CATEGORY-OPERATOR` header states.**
    ///
    /// It lives on the SESSION because the session IS the entry: one run of one
    /// contest under one callsign is exactly the thing a sponsor scores as an entry.
    /// Until this field existed the header was the string literal `MULTI-OP`, so
    /// every solo Field Day entry Nexus exported claimed more than one operator was
    /// at the station.
    pub entry_category: super::cabrillo::OperatorCategory,
    /// The mode-split `CONTEST` tokens, `(mode class, id)` — `[("CW",
    /// "ARRL-SS-CW"), ("PH", "ARRL-SS-SSB")]`.
    ///
    /// **Empty for a contest whose entry is one file**, which is both Field Day
    /// events and everything else pass one ships. When it is not empty, a log
    /// spanning two ids is refused rather than submitted under one of them
    /// ([`contest_id_for`](Self::contest_id_for)).
    pub contest_id_by_mode: Vec<(String, String)>,
    /// Cabrillo's trailing transmitter-id column, and the id THIS position writes in
    /// it — `None` when the sponsor's QSO template has no such column.
    ///
    /// One field rather than a flag beside a number, so "the template has the column"
    /// and "this is my id" cannot disagree. `None` for both Field Day events and for
    /// everything else pass one ships; ARRL DX, CQ WW and CQ WPX carry the column and
    /// declare it in the batch that reads each sponsor's own template (§6.2 — the
    /// per-contest column order is never taken from a compilation).
    pub transmitter_id: Option<u8>,
    /// Where this session's merged contacts go — **default OFF** (§18.1). See
    /// [`UploadPolicy`].
    pub upload: UploadPolicy,
}

impl ContestSession {
    /// A session for a Field Day event, from the class and section the operator has
    /// declared in Settings.
    ///
    /// The declared location is the ARRL/RAC section: that is what Cabrillo's
    /// `LOCATION` header means for this event, and the header reads it from here.
    ///
    /// ⚠️ Calls [`super::field_day`], which loads the rules table for the section
    /// domain, so it carries `fd_rules::ruleset`'s ordering rule — never construct one
    /// before the startup `fd_rules::install_from`.
    pub fn field_day(event: crate::fieldday::FdEvent, class: &str, section: &str) -> Self {
        let exchange = super::field_day(event);
        let section = section.trim().to_ascii_uppercase();
        // The SECTION slot is an `Enum`, so its value carries the domain that matched
        // it — which is what an export tag and a multiplier bucket are later chosen by.
        // `ExchangeSpec::value` is the one place that resolution lives.
        let my_exchange = ["CLASS", "SECTION"]
            .iter()
            .zip([class.trim().to_ascii_uppercase(), section.clone()])
            .filter_map(|(k, v)| exchange.value(k, &v))
            .collect();
        Self {
            id: format!("{}:{}", event.contest_id(), section),
            event_id: event.code().to_string(),
            contest_id: event.contest_id().to_string(),
            rules_year: crate::fd_rules::CURRENT_RULES_YEAR,
            exchange,
            my_location: MyLocation {
                county: None,
                state: section,
                dxcc: false,
            },
            my_exchange,
            in_flight: None,
            start_unix: 0,
            end_unix: 0,
            next_serial: 1,
            label: exchange.name.to_string(),
            // ⭐ SINGLE-OP, not the `MULTI-OP` this header was hardcoded to. A lone
            // operator is the case the literal was wrong about, and a club running a
            // multi-operator entry is already configuring positions.
            entry_category: super::cabrillo::OperatorCategory::default(),
            // Neither Field Day event splits its entry by mode.
            contest_id_by_mode: Vec::new(),
            // …and neither sponsor's QSO template carries a transmitter column.
            transmitter_id: None,
            // §18.1: OFF, on every new session, without exception. It is not read from
            // a setting — a global default is the thing this control replaces.
            upload: UploadPolicy::default(),
        }
    }

    /// ⭐ **A session for ANY shipped ruleset** — the constructor that makes a contest
    /// other than Field Day reachable at all.
    ///
    /// Field Day had [`field_day`](Self::field_day) and nothing else had anything, so
    /// batch 8's four QSO-party rulesets, their county domains and their station-data
    /// settings were all present and none of them could be entered. This is that gap,
    /// and it is deliberately ONE function: a per-contest constructor is how two
    /// contests come to disagree about which slot a county goes in.
    ///
    /// **Everything comes off the ruleset.** The exchange, the roles, the Cabrillo
    /// token, the rules year — all read from `rs`, so a rules-file edit moves the
    /// session and a code edit is not needed to add a fifth contest.
    ///
    /// ⭐ **The role is DERIVED, never passed in.** [`MyLocation`] is built from the
    /// operator's station data and [`role`](Self::role) evaluates the ruleset's own
    /// [`RoleSelector`]s against it. A `role` parameter would let the caller name a
    /// role the location contradicts — and a wrong role sends a wrong exchange, on the
    /// air, for the whole contest.
    ///
    /// ⚠️ **It is fallible, and every `Err` is a sentence for the operator**, because
    /// the alternative to refusing here is transmitting a blank or an out-of-domain
    /// value. Refused when: a sent slot's source is one no session can supply; a
    /// `"setting:"` source names a field [`StationData`] does not carry; a required
    /// sent slot resolves empty (the operator has not said where they are); or a
    /// resolved value is in none of the slot's declared domains (an Ohio county typed
    /// into a Tennessee party).
    ///
    /// ⚠️ Reads the rules table through `rs`, so it carries
    /// [`fd_rules::ruleset`](crate::fd_rules::ruleset)'s ordering rule — never call it
    /// before the startup `fd_rules::install_from`.
    pub fn for_ruleset(
        rs: &'static crate::fd_rules::FdRuleset,
        station: &StationData,
    ) -> Result<Self, String> {
        let my_location = MyLocation {
            county: Some(station.contest_qth_county.trim().to_ascii_uppercase())
                .filter(|c| !c.is_empty()),
            state: station.contest_qth_state.trim().to_ascii_uppercase(),
            dxcc: station.dxcc,
        };
        // The role is evaluated by asking a session, not by re-implementing the
        // selector walk beside `role()` — one evaluation, one answer, and an "I moved"
        // that lands in a different role is detected by the same code path.
        let mut s = Self {
            id: String::new(),
            event_id: rs.event.to_string(),
            contest_id: rs.contest_id.to_string(),
            rules_year: rs.rules_year,
            exchange: rs.exchange,
            my_location,
            my_exchange: Vec::new(),
            in_flight: None,
            start_unix: 0,
            end_unix: 0,
            next_serial: 1,
            label: rs.exchange.name.to_string(),
            entry_category: super::cabrillo::OperatorCategory::default(),
            contest_id_by_mode: Vec::new(),
            transmitter_id: None,
            upload: UploadPolicy::default(),
        };
        let role = s.role();
        let mut my_exchange = Vec::new();
        for key in role.sends {
            let field = s
                .exchange
                .field(key)
                .ok_or_else(|| format!("{key} is not a slot this exchange declares"))?;
            let raw = sent_value(field, role, station, &s.my_location)?;
            if raw.is_empty() {
                if field.required {
                    return Err(format!(
                        "Your {key} is empty — it is part of the exchange you transmit. \
Fill it in on the Contesting tab in Settings."
                    ));
                }
                continue;
            }
            // ⭐ CHECKED, THEN CONFIRMED, in that order. An out-of-domain value is
            // refused here — before it goes on the air, rather than after it is in a
            // submitted log — and what survives is recorded with the domain arm that
            // matched it, at the one moment that arm is a fact rather than a later
            // guess (§2.4).
            if !accepts(field, &raw, role) {
                return Err(format!(
                    "\"{raw}\" is not a value the {key} slot of this contest accepts."
                ));
            }
            my_exchange.push(
                s.exchange
                    .copied(key, &raw)
                    .expect("the slot resolved a few lines above"),
            );
        }
        s.my_exchange = my_exchange;
        // ⭐ **Where the entry says it is, derived the SAME way `move_to` derives it** —
        // the role's first sent `Enum` slot — so a session that is built and then moved
        // cannot disagree with itself about the operator's location.
        //
        // It matters for Field Day and for nothing else in this build: FD's `SECTION`
        // is a plain `Enum` and IS the entry's location (Cabrillo's `LOCATION` header
        // means the section for that event, §6.1), while a QSO party's `QTH` is a
        // `OneOf` and is therefore left alone — the county an in-state operator sends
        // is not the state their role is selected on.
        if let Some(loc) = role
            .sends
            .iter()
            .find(|k| {
                matches!(
                    s.exchange.field(k).map(|f| f.kind),
                    Some(super::FieldKind::Enum { .. })
                )
            })
            .and_then(|k| s.my_exchange.iter().find(|v| v.key == *k))
        {
            s.my_location.state = loc.raw.clone();
        }
        // The id names the entry: one contest, from one place. `role()` is stable while
        // the location is, and a location change that would move it ends the session
        // (§3.3 ruling 3) rather than renaming this.
        s.id = format!(
            "{}:{}",
            rs.contest_id,
            s.my_location
                .county
                .clone()
                .unwrap_or_else(|| s.my_location.state.clone())
        );
        Ok(s)
    }

    /// The connector destinations this session's merge may enqueue to — **empty
    /// whenever the control is off**, which is what makes "default OFF" a property of
    /// one function rather than of every caller that remembers to check the flag.
    pub fn upload_destinations(&self) -> &[String] {
        if self.upload.enabled {
            &self.upload.destinations
        } else {
            &[]
        }
    }

    /// The Cabrillo `CONTEST` token for a log holding these mode classes.
    ///
    /// One method rather than a field read plus a lookup, so no caller can resolve a
    /// mode-split id one way while another resolves it a second way. See
    /// [`resolve_contest_id`](super::cabrillo::resolve_contest_id) for why a log
    /// spanning two ids is refused instead of submitted under the first.
    pub fn contest_id_for(&self, mode_classes: &[&str]) -> Result<String, String> {
        super::cabrillo::resolve_contest_id(
            &self.contest_id,
            &self.contest_id_by_mode,
            mode_classes,
        )
    }

    /// **`role` is not a field.** It is evaluated from [`Self::my_location`] every
    /// time, so a location change moves `sends`, `receives`, the Cabrillo column order
    /// and the multiplier universe together or not at all.
    ///
    /// The first selector that matches wins; when none does, the LAST role is the
    /// catch-all — the position §2.3's Ohio QSO Party `dx` role occupies. Field Day
    /// has one unconditional role, so this is total for every shipped exchange.
    pub fn role(&self) -> &'static RoleSpec {
        let roles = self.exchange.roles;
        roles
            .iter()
            .find(|r| self.selects(&r.selector))
            .or_else(|| roles.last())
            .expect("an ExchangeSpec always declares at least one role")
    }

    fn selects(&self, sel: &RoleSelector) -> bool {
        match sel {
            RoleSelector::Always => true,
            RoleSelector::MyLocationIn(list) => {
                !self.my_location.dxcc
                    && list
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(&self.my_location.state))
            }
            // The only `CATEGORY-*` value this session declares is
            // [`Self::entry_category`] (`CATEGORY-OPERATOR`), and no exchange in pass
            // one selects a role on it — the QSO parties select on location. Which
            // AXIS a selector names is therefore still undetermined, and answering it
            // here would be guessing; the batch that ships an exchange selecting on a
            // category decides, and reads the field it names. Never matching is the
            // safe half: it falls through to the catch-all role rather than claiming
            // a category the operator never set.
            RoleSelector::MyCategoryIs(_) => false,
        }
    }

    /// ONE slot of the exchange I am sending right now, by key (`""` when the session
    /// does not carry that slot).
    ///
    /// ⚠️ **This is not the renderer §3.3 makes unrepresentable, and the difference is
    /// the direction of time.** The forbidden thing turns a session into a STRING that
    /// then describes a contact already logged; this reads one slot of what is about to
    /// go on the air, for a frame with typed fields of its own (`Msg::FieldDay` carries
    /// `class` and `section`, not a rendered exchange). Nothing that describes a past
    /// row may call it — those read the ROW, through
    /// [`sent_exchange`](super::render::sent_exchange).
    pub fn field(&self, key: &str) -> &str {
        self.my_exchange
            .iter()
            .find(|v| v.key == key)
            .map(|v| v.raw.as_str())
            .unwrap_or("")
    }

    /// Issue the sent exchange for `peer`, or return the one already in flight for
    /// them.
    ///
    /// A serial is issued when an exchange is composed **for a peer that does not
    /// already have one in flight**. A repeat, an AGN, a re-call and a CQ all render
    /// from the in-flight value and advance nothing: sending your exchange twice must
    /// not send two different numbers, because the second is the one the other
    /// operator copies.
    pub fn compose_for(&mut self, peer: &str, now_unix: u64) -> &InFlightQso {
        let peer = peer.trim().to_ascii_uppercase();
        if self.in_flight.as_ref().is_some_and(|f| f.peer == peer) {
            return self.in_flight.as_ref().expect("just matched");
        }
        let mut tx = self.my_exchange.clone();
        for v in &mut tx {
            if matches!(
                self.exchange.field(v.key).map(|f| f.kind),
                Some(super::FieldKind::Serial { .. })
            ) {
                v.raw = self.next_serial.to_string();
                self.next_serial += 1;
            }
        }
        self.in_flight = Some(InFlightQso {
            peer,
            tx,
            since_unix: now_unix,
        });
        self.in_flight.as_ref().expect("just set")
    }

    /// The exchange to stamp on a row for `peer`: the one issued to them if there is
    /// one, else the session's current sent exchange.
    ///
    /// This is the ONLY place `my_exchange` is copied, and it copies it onto a row.
    pub fn tx_for_row(&self, peer: &str, mode_class: &str) -> Vec<FieldValue> {
        let mut tx = match &self.in_flight {
            Some(f) if f.peer.eq_ignore_ascii_case(peer.trim()) => f.tx.clone(),
            _ => self.my_exchange.clone(),
        };
        // ⭐ **The one thing on a sent exchange that is a property of the ROW, not of
        // the session: an RST's digit count.** §2.2's own definition of the field is
        // "2 digits on phone, 3 on CW and digital", and the session has no mode — it is
        // one session across CW and phone — so the constant it holds is the 3-digit
        // form and the row is where the phone case becomes true. A `599` on a phone
        // line of a submitted log is a report the operator never sent.
        //
        // It touches only a slot the ruleset declares `Rst` AND sources as a
        // `"constant"`: an RST the operator typed or a modem copied is a value somebody
        // actually exchanged, and rewriting one is what `FieldSpec`'s own header
        // forbids.
        if mode_class.eq_ignore_ascii_case("PH") {
            for v in &mut tx {
                if let Some(f) = self.exchange.field(v.key) {
                    if f.source == "constant" && matches!(f.kind, super::FieldKind::Rst { .. }) {
                        v.raw = "59".to_string();
                    }
                }
            }
        }
        tx
    }

    /// The contact is logged (or abandoned): nothing is in flight any more.
    pub fn clear_in_flight(&mut self) {
        self.in_flight = None;
    }

    /// ⭐ **"I moved" (§4.1) — edit the exchange this session is COMPOSING.**
    ///
    /// `values` names `(slot id, raw)` for the slots that changed; every other slot
    /// keeps what it had. It takes effect on the NEXT contact and touches nothing
    /// already written:
    ///
    /// * rows already logged carry their own `tx` and are not reachable from here;
    /// * an exchange already **in flight** is left alone — it has been sent to that
    ///   peer, and the number they copied is the number that must be logged.
    ///
    /// **Where the location comes from.** The role's first sent [`FieldKind::Enum`]
    /// slot is the one a [`RoleSelector::MyLocationIn`] reads, so that slot's new
    /// value is the new [`MyLocation::state`]. Field Day's is `SECTION`. (Batch 8's
    /// QSO parties are where a second component — the county — joins it, and where a
    /// role genuinely moves.)
    ///
    /// ⚠️ **Refuses a move that changes ROLE** — §3.3 ruling 3. Crossing a state line
    /// flips `sends`, `receives` and the whole multiplier universe, which is a
    /// separate entry and a separate Cabrillo file, not a move. The caller shows
    /// [`MOVE_CHANGES_ROLE`] and offers to end the session.
    ///
    /// ⚠️ **This writes the SESSION only.** The other half of ruling 2 — the setting a
    /// genuinely new session starts from — belongs to the caller, because this crate
    /// holds no settings. `Engine::contest_i_moved` writes both or neither.
    ///
    /// An `Enum` slot is checked against its domain, because a section that is not a
    /// section goes on the air and then into a submitted log. Other kinds are stored
    /// with the same trim + uppercase [`Self::field_day`] applies and no further
    /// check: a `Pattern` needs a matcher this crate does not carry, and
    /// approximating one would be the thing [`FieldKind::Pattern`] forbids.
    pub fn move_to(&mut self, values: &[(&str, &str)]) -> Result<(), String> {
        let role = self.role();
        let mut next = self.my_exchange.clone();
        for (key, raw) in values {
            if !role.sends.contains(key) {
                return Err(format!("{key} is not a slot this role sends"));
            }
            let field = self
                .exchange
                .field(key)
                .ok_or_else(|| format!("{key} is not a slot this exchange declares"))?;
            let raw = raw.trim().to_ascii_uppercase();
            if let super::FieldKind::Enum { domain } = field.kind {
                if !domain.contains(&raw) {
                    return Err(format!("\"{raw}\" is not a {} value", domain.id));
                }
            }
            let v = self
                .exchange
                .value(key, &raw)
                .expect("the slot resolved one line above");
            match next.iter_mut().find(|x| x.key == v.key) {
                Some(slot) => *slot = v,
                None => next.push(v),
            }
        }
        let mut where_now = self.my_location.clone();
        if let Some(loc) = role
            .sends
            .iter()
            .find(|k| {
                matches!(
                    self.exchange.field(k).map(|f| f.kind),
                    Some(super::FieldKind::Enum { .. })
                )
            })
            .and_then(|k| next.iter().find(|v| v.key == *k))
        {
            where_now.state = loc.raw.clone();
        }
        // Ruling 3, evaluated rather than asserted: the candidate location is what
        // `role()` reads, so ask the candidate which role it is in.
        let mut candidate = self.clone();
        candidate.my_location = where_now.clone();
        if candidate.role().id != role.id {
            return Err(MOVE_CHANGES_ROLE.to_string());
        }
        self.my_exchange = next;
        self.my_location = where_now;
        Ok(())
    }
}

/// The value ONE sent slot takes, from the source the ruleset declares for it (§3.4).
///
/// ⚠️ **It switches on `FieldSpec::source`, never on `FieldSpec::kind`.** Kind says
/// what a slot IS; source says where its value comes from, and the two are genuinely
/// independent — a QSO party's `QTH` and Sweepstakes' `SEC` are both enum-shaped and
/// one is derived from the operator's location while the other is read straight out of
/// `fd_section`. Deriving the source from the kind would be a second mapping that the
/// loader's validator does not check.
fn sent_value(
    field: &'static super::FieldSpec,
    role: &'static RoleSpec,
    station: &StationData,
    my_location: &MyLocation,
) -> Result<String, String> {
    match field.source {
        // An RST is the only constant in the researched set, and its DIGIT COUNT is the
        // ruleset's own declaration — `599` for the 3-digit slot all four QSO parties
        // declare, `59` for a 2-digit one. ⚠️ It does NOT vary with the on-air mode
        // here: the session has no mode, and a mode-aware constant is applied where the
        // mode is known (`FieldDayLog::log_submode_at`, which is per row).
        "constant" => match field.kind {
            super::FieldKind::Rst { digits } if digits <= 2 => Ok("59".to_string()),
            super::FieldKind::Rst { .. } => Ok("599".to_string()),
            _ => Err(format!(
                "{} declares a constant source, but this build has a constant only for \
an RST slot",
                field.key
            )),
        },
        // The PLACEHOLDER, not the number. A serial is issued at compose time, onto
        // `in_flight`, and the row copies it from there (§2.6, batch 3) — this is the
        // template's shape, and the one thing it must not be is the live counter.
        "serial" => Ok("0".to_string()),
        // ⭐ §3.4's `QTH`: one slot, county or state or DX, chosen by ROLE.
        //
        // The role decides rather than the value being probed against each domain in
        // turn, and the difference is not academic: Ohio and Michigan both have a Wayne
        // county, so a Michigan operator whose `contest_qth_county` happens to hold an
        // Ohio abbreviation would be sent out as an Ohio station by a probe and is sent
        // out as `MI` by this. The ids are §2.3's own role vocabulary, which every
        // shipped QSO-party ruleset uses; anything else sends the state, which is the
        // safe half — a state that is not in the slot's domains is REFUSED below,
        // while a county silently accepted is a wrong exchange nobody sees.
        "derived:my_location" => Ok(match role.id {
            "in_state" => my_location.county.clone().unwrap_or_default(),
            "dx" => "DX".to_string(),
            _ => my_location.state.clone(),
        }),
        other => match other.split_once(':') {
            Some(("setting", name)) => station
                .setting(name)
                .map(|v| v.trim().to_ascii_uppercase())
                .ok_or_else(|| {
                    format!(
                        "{} is sourced from the setting {name:?}, which this build does \
not carry",
                        field.key
                    )
                }),
            _ => Err(format!(
                "{} declares source {other:?}, which a session cannot supply",
                field.key
            )),
        },
    }
}

/// Is `raw` a value this slot's declared shape holds, FOR THIS ROLE?
///
/// Only the shapes whose universe this crate CAN state are checked — an `Enum`'s domain
/// and a `OneOf`'s arms. A `Pattern` needs a matcher this crate does not carry and a
/// `Number`'s bounds are checked by whatever parsed it; approximating either is exactly
/// what [`FieldKind::Pattern`](super::FieldKind::Pattern) forbids, so both pass.
///
/// ⭐ **The role is a parameter because a `OneOf`'s catch-all arm belongs to ONE role,
/// not to the slot.** TNQP and TXQP express "or a U.S. state, Canadian province or DXCC
/// entity" as a free-text arm, because neither sponsor publishes a list of those — so
/// the slot accepts anything and the OUT-OF-STATE role is right to. The IN-STATE role
/// is not: its value is a county, the sponsor publishes exactly which counties, and a
/// county that is not one goes on the air and then into a submitted log. So for
/// `in_state` a slot that declares any `Enum` arm must match one of them.
fn accepts(field: &'static super::FieldSpec, raw: &str, role: &'static RoleSpec) -> bool {
    let in_any_enum_arm = |arms: &'static [super::FieldKind]| {
        arms.iter().any(|a| match a {
            super::FieldKind::Enum { domain } => domain.contains(raw),
            _ => false,
        })
    };
    match field.kind {
        super::FieldKind::Enum { domain } => domain.contains(raw),
        super::FieldKind::OneOf(arms) => {
            let has_enum_arm = arms
                .iter()
                .any(|a| matches!(a, super::FieldKind::Enum { .. }));
            if role.id == "in_state" && has_enum_arm {
                return in_any_enum_arm(arms);
            }
            arms.iter().any(|a| match a {
                super::FieldKind::Enum { domain } => domain.contains(raw),
                _ => true,
            })
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fieldday::FdEvent;

    #[test]
    fn a_field_day_session_declares_its_section_as_its_location() {
        let s = ContestSession::field_day(FdEvent::ArrlFd, "3a", " wi ");
        assert_eq!(s.my_location.state, "WI");
        assert_eq!(s.contest_id, "ARRL-FIELD-DAY");
        assert_eq!(
            s.my_exchange
                .iter()
                .map(|v| (v.key, v.raw.as_str()))
                .collect::<Vec<_>>(),
            vec![("CLASS", "3A"), ("SECTION", "WI")]
        );
        // The SECTION value carries the domain that matched it; CLASS is a pattern
        // and has none.
        assert_eq!(s.my_exchange[0].domain, None);
        assert_eq!(s.my_exchange[1].domain, Some("fd_sections"));
    }

    #[test]
    fn field_days_single_unconditional_role_is_always_the_role() {
        let mut s = ContestSession::field_day(FdEvent::WinterFd, "2O", "WI");
        assert_eq!(s.role().id, "");
        // …and it does not move when the declared location does, because the role is
        // unconditional. (The QSO-party case, where it DOES move, is batch 8.)
        s.my_location.state = "OH".into();
        assert_eq!(s.role().id, "");
        assert_eq!(s.role().sends, &["CLASS", "SECTION"]);
    }

    /// §2.6, both halves. One peer gets ONE number however many times their exchange
    /// is composed; a second peer gets the next one. Field Day sends no serial, so the
    /// exchange under test declares one.
    #[test]
    fn a_serial_is_issued_once_per_peer_not_once_per_compose() {
        let mut s = serial_session();
        let a1 = s.compose_for("W1AW", 10).tx.clone();
        let a2 = s.compose_for("W1AW", 20).tx.clone();
        let a3 = s.compose_for("w1aw", 30).tx.clone();
        assert_eq!(serial_of(&a1), "1");
        assert_eq!(a1, a2, "a repeat renders from the in-flight value");
        assert_eq!(a1, a3, "…and matches the peer case-insensitively");
        assert_eq!(s.next_serial, 2, "one issue, one advance");
        // POSITIVE CONTROL: two different peers really do get two numbers, so the
        // assertion above is about the peer and not about a counter that never moves.
        let b = s.compose_for("K1ABC", 40).tx.clone();
        assert_eq!(serial_of(&b), "2");
        assert_eq!(s.next_serial, 3);
    }

    #[test]
    fn a_row_takes_the_issued_exchange_for_its_own_peer_and_the_live_one_otherwise() {
        let mut s = serial_session();
        s.compose_for("W1AW", 10);
        assert_eq!(serial_of(&s.tx_for_row("W1AW", "DIG")), "1");
        // A row for somebody else is not described by W1AW's issue.
        assert_eq!(serial_of(&s.tx_for_row("K1ABC", "DIG")), "0");
        s.clear_in_flight();
        assert_eq!(serial_of(&s.tx_for_row("W1AW", "DIG")), "0");
    }

    /// §4.1: the move lands on the SESSION, and on the location the role is derived
    /// from — together, because the exchange and the role must never disagree about
    /// where I am.
    #[test]
    fn i_moved_edits_the_composing_exchange_and_the_location_together() {
        let mut s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        s.move_to(&[("SECTION", " il ")]).expect("IL is a section");
        assert_eq!(s.field("SECTION"), "IL");
        assert_eq!(s.my_location.state, "IL");
        // Untouched slots keep what they had — a move is not a re-declaration.
        assert_eq!(s.field("CLASS"), "3A");
        // …and the value still carries the domain that matched it, so the export tag
        // and the multiplier bucket are chosen the same way as on a fresh session.
        assert_eq!(
            s.my_exchange
                .iter()
                .find(|v| v.key == "SECTION")
                .and_then(|v| v.domain),
            Some("fd_sections")
        );
    }

    /// A section that is not a section would go on the air and then into a submitted
    /// log. The domain is the check, and a refusal leaves the session where it was.
    #[test]
    fn i_moved_refuses_a_value_its_domain_does_not_hold() {
        let mut s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        let e = s.move_to(&[("SECTION", "ZZZ")]).unwrap_err();
        assert!(e.contains("fd_sections"), "{e}");
        assert_eq!(s.field("SECTION"), "WI", "a refused move moves nothing");
        assert_eq!(s.my_location.state, "WI");
        // A slot this role does not send is refused by name rather than appended.
        assert!(s.move_to(&[("NOPE", "X")]).is_err());
    }

    /// ⚠️ The exchange ALREADY SENT to the peer in flight is what they copied. A move
    /// takes effect on the NEXT contact and must not rewrite this one.
    #[test]
    fn i_moved_does_not_touch_an_exchange_already_in_flight() {
        let mut s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        s.compose_for("K1ABC", 100);
        s.move_to(&[("SECTION", "IL")]).expect("IL is a section");
        assert_eq!(
            s.tx_for_row("K1ABC", "DIG")
                .iter()
                .find(|v| v.key == "SECTION")
                .map(|v| v.raw.clone()),
            Some("WI".to_string()),
            "the contact in flight was worked from WI"
        );
        // POSITIVE CONTROL: the very next contact does get the new section, so the
        // assertion above is about the in-flight exchange and not about a move that
        // silently did nothing.
        s.clear_in_flight();
        assert_eq!(
            s.tx_for_row("W1AW", "DIG")
                .iter()
                .find(|v| v.key == "SECTION")
                .map(|v| v.raw.clone()),
            Some("IL".to_string())
        );
    }

    /// ⭐ §3.3 ruling 3: a move that lands in a different ROLE is refused with the
    /// named message, and nothing changes. Field Day's one role is unconditional, so
    /// this needs the two-role shape batch 8 ships for real.
    #[test]
    fn i_moved_refuses_a_move_that_changes_role() {
        let mut s = two_role_session();
        // Inside the in-state list: an ordinary move.
        s.move_to(&[("QTH", "TN")])
            .expect("TN keeps the in-state role");
        assert_eq!(s.role().id, "in_state");
        assert_eq!(s.my_location.state, "TN");
        // Across the line: refused, by name, with the session untouched.
        let e = s.move_to(&[("QTH", "CT")]).unwrap_err();
        assert_eq!(e, MOVE_CHANGES_ROLE);
        assert_eq!(s.field("QTH"), "TN", "a refused move moves nothing");
        assert_eq!(s.my_location.state, "TN");
        assert_eq!(s.role().id, "in_state");
    }

    /// A two-role exchange whose roles are selected on location — the shape every QSO
    /// party has and no exchange this build ships does.
    fn two_role_session() -> ContestSession {
        use super::super::spec::{AdifTags, Domain, FieldKind, FieldSpec};
        static STATES: Domain = Domain {
            id: "test_states",
            adif: AdifTags {
                rcvd: Some("STATE"),
                sent: Some("MY_STATE"),
            },
            values: &[
                ("TN", "Tennessee"),
                ("KY", "Kentucky"),
                ("CT", "Connecticut"),
            ],
        };
        static F: &[FieldSpec] = &[FieldSpec {
            key: "QTH",
            adif: AdifTags {
                rcvd: Some("STATE"),
                sent: Some("MY_STATE"),
            },
            label: None,
            required: true,
            source: "derived:my_location",
            kind: FieldKind::Enum { domain: &STATES },
        }];
        static R: &[RoleSpec] = &[
            RoleSpec {
                id: "in_state",
                selector: RoleSelector::MyLocationIn(&["TN", "KY"]),
                sends: &["QTH"],
                receives: &["QTH"],
                constant_sent: &[],
            },
            RoleSpec {
                id: "out_of_state",
                selector: RoleSelector::Always,
                sends: &["QTH"],
                receives: &["QTH"],
                constant_sent: &[],
            },
        ];
        static S: ExchangeSpec = ExchangeSpec {
            name: "tworole",
            fields: F,
            roles: R,
        };
        let mut s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        s.exchange = &S;
        s.my_location = MyLocation {
            county: None,
            state: "KY".into(),
            dxcc: false,
        };
        s.my_exchange = vec![FieldValue {
            key: "QTH",
            raw: "KY".into(),
            domain: Some("test_states"),
        }];
        s
    }

    fn serial_of(tx: &[FieldValue]) -> &str {
        tx.iter()
            .find(|v| v.key == "NR")
            .map(|v| v.raw.as_str())
            .unwrap_or("")
    }

    /// A one-role exchange with a serial slot — the shape Sweepstakes and CQ WPX have
    /// and neither Field Day event does.
    fn serial_session() -> ContestSession {
        use super::super::spec::{AdifTags, FieldKind, FieldSpec, SerialScope};
        static F: &[FieldSpec] = &[FieldSpec {
            key: "NR",
            adif: AdifTags {
                rcvd: Some("SRX"),
                sent: Some("STX"),
            },
            label: None,
            required: true,
            source: "serial",
            kind: FieldKind::Serial {
                scope: SerialScope::PerContest,
            },
        }];
        static R: &[RoleSpec] = &[RoleSpec {
            id: "",
            selector: RoleSelector::Always,
            sends: &["NR"],
            receives: &["NR"],
            constant_sent: &[],
        }];
        static S: ExchangeSpec = ExchangeSpec {
            name: "serialtest",
            fields: F,
            roles: R,
        };
        let mut s = ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI");
        s.exchange = &S;
        s.my_exchange = vec![FieldValue {
            key: "NR",
            raw: "0".into(),
            domain: None,
        }];
        s
    }
}
