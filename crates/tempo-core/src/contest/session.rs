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

/// One run of one contest: the object BOTH logs carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestSession {
    /// Stable across restart; stamped on every row it owns.
    pub id: String,
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
            // §18.1: OFF, on every new session, without exception. It is not read from
            // a setting — a global default is the thing this control replaces.
            upload: UploadPolicy::default(),
        }
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
            // The entry category is a `CATEGORY-*` header value, and the picker that
            // supplies it is batch 6. No exchange in pass one selects on it; when one
            // does, the category joins the session beside `my_location` and this arm
            // reads it. Never matching is the safe half: it falls through to the
            // catch-all role rather than claiming a category the operator never set.
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
    pub fn tx_for_row(&self, peer: &str) -> Vec<FieldValue> {
        match &self.in_flight {
            Some(f) if f.peer.eq_ignore_ascii_case(peer.trim()) => f.tx.clone(),
            _ => self.my_exchange.clone(),
        }
    }

    /// The contact is logged (or abandoned): nothing is in flight any more.
    pub fn clear_in_flight(&mut self) {
        self.in_flight = None;
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
        assert_eq!(serial_of(&s.tx_for_row("W1AW")), "1");
        // A row for somebody else is not described by W1AW's issue.
        assert_eq!(serial_of(&s.tx_for_row("K1ABC")), "0");
        s.clear_in_flight();
        assert_eq!(serial_of(&s.tx_for_row("W1AW")), "0");
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
