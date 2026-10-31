//! The Logbook's field filters (`ui/src/features/logFilter.ts` `LogFilter`): call, band, mode,
//! dates, place, QSL state per service, contest, award need and free text, combined with AND —
//! and beside them, the one search box and the needs-confirmation chip of [`super::LogQuery`].
//!
//! # Who answers which field
//!
//! **SQL answers every field it can state exactly** ([`crate::logbook::sqlite`]'s
//! `filter_sql`): the statement that reads the rows for an order carries the filter as its
//! `WHERE`, so a row a filter refuses is never read, decoded or handed to the order. That is the
//! store doing the work, never the window — no window holds the log.
//!
//! **Four fields depend on what SQL cannot know**, and the caller decides them on each row the
//! statement hands back ([`LogFilter::decided_by_caller`], `OrderBuilder::keep`):
//!
//! - `entity`, `continent` and `cq_zone` are cty.dat's answers (with the record's own DXCC number,
//!   `CONT` and `CQZ` first), and the table lives in another crate. The store's `entity` column is
//!   cty.dat's answer **as of the day the row was written**, so filtering on it would disagree with
//!   the country the Logbook shows after a cty.dat update.
//! - `need` is the award fold's answer: which band slots are worked and not confirmed.
//! - `path` is the grade of the contact's path: its own stamp, else the display-only grade of the
//!   log, which the caller holds outside the log.
//!
//! **Free text** with any letter outside ASCII is decided here ([`LogFilter::text_matches`]):
//! SQLite's `LIKE` folds ASCII case only, so `münchen` would never find `MÜNCHEN`. ASCII text is
//! SQL's.

use super::{js_lower, js_trim, QsoRecord};
use serde::{Deserialize, Serialize};

/// What one QSL service holds for a contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QslState {
    /// The service (or the card) confirmed it.
    Confirmed,
    /// Sent: uploaded (waiting, taken or a duplicate), or a card marked sent.
    Sent,
    /// Never sent, or every upload bounced.
    NotSent,
}

/// Club Log takes uploads and confirms nothing, so it is only sent or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SentState {
    Sent,
    NotSent,
}

/// The QSL state asked of each service; a service left out is not filtered on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QslFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lotw: Option<QslState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eqsl: Option<QslState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<QslState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qrz: Option<QslState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clublog: Option<SentState>,
}

impl QslFilter {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether any state asked is "sent" or "not sent": what an upload stamp or a card marked sent
    /// moves, and those move the log's `content_rev` and not its `index_rev`.
    pub fn reads_sent(&self) -> bool {
        [self.lotw, self.eqsl, self.card, self.qrz]
            .into_iter()
            .flatten()
            .any(|s| s != QslState::Confirmed)
            || self.clublog.is_some()
    }
}

/// The awards a contact can still be needed for on its own band: its slot there is worked and not
/// confirmed, so confirming it adds credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AwardNeed {
    Dxcc,
    Was,
    Waz,
    Vucc,
    Ffma,
}

/// The path a contact took, as the Logbook's path column grades it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathFilter {
    /// The grade names the long path.
    Long,
    /// The route the grade names (the short path when it names none) crosses the polar cap.
    Polar,
}

/// The Logbook's field filters. An empty field filters nothing; the fields set combine with AND.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogFilter {
    /// A call prefix (`K1`), or with `*` / `?` a pattern for the whole call (`*/P`, `K?ABC`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub call: String,
    /// A band label (`20m`): the row's band slot, its frequency's band where its own label names
    /// none (`band_key`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub band: String,
    /// A mode as the Logbook names it: the sidebands are SSB (`mode_key`), and a SUBMODE counts
    /// (`FT4` finds an `MFSK` contact whose submode is FT4).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// UTC seconds, from inclusive and to exclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_unix: Option<u64>,
    /// The DXCC entity the Logbook shows for the row. Decided by the caller.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub entity: String,
    /// `AF` … `SA`. Decided by the caller.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub continent: String,
    /// The CQ zone the contact counts toward. Decided by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cq_zone: Option<u8>,
    /// The ITU zone the record holds (ADIF `ITUZ`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub itu_zone: Option<u8>,
    /// A grid prefix.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub grid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub state: String,
    /// The start of an ADIF `CNTY` (`MA,MIDDLESEX`; `MA,` for every one in the state).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub county: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub iota: String,
    /// Part of a park, summit or WWFF reference — theirs or the station's own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    /// One of the station's own references, whole: a row whose `MY_SIG_INFO` list (one park, or a
    /// two-fer's comma list) holds it. `US-1000` never finds `US-10000`, and a hunter's contact
    /// with that park finds nothing. What an activation's contacts are, with `from_unix`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub my_ref: String,
    #[serde(default, skip_serializing_if = "QslFilter::is_empty")]
    pub qsl: QslFilter,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contest: String,
    /// Decided by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub need: Option<AwardNeed>,
    /// Words in the comment, the name or the QTH.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The path the contact's grade names — its own stamp, else the display-only grade of the
    /// log, which lives outside it. Decided by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathFilter>,
}

/// A text field as the filter reads it: trimmed as JavaScript trims, and empty for nothing asked.
pub(crate) fn field(s: &str) -> &str {
    js_trim(s)
}

impl LogFilter {
    /// Nothing set: every text field blank (a field cleared to spaces filters nothing) and no
    /// other field asked.
    pub fn is_empty(&self) -> bool {
        let LogFilter {
            call,
            band,
            mode,
            from_unix,
            to_unix,
            entity,
            continent,
            cq_zone,
            itu_zone,
            grid,
            state,
            county,
            iota,
            reference,
            my_ref,
            qsl,
            contest,
            need,
            text,
            path,
        } = self;
        [
            call, band, mode, entity, continent, grid, state, county, iota, reference, my_ref,
            contest, text,
        ]
        .iter()
        .all(|s| field(s).is_empty())
            && from_unix.is_none()
            && to_unix.is_none()
            && cq_zone.is_none()
            && itu_zone.is_none()
            && need.is_none()
            && path.is_none()
            && qsl.is_empty()
    }

    /// Whether a field the caller decides is set ([`Self`]'s header): the rows SQL hands back
    /// must each pass the caller's test as well.
    pub fn decided_by_caller(&self) -> bool {
        !field(&self.entity).is_empty()
            || !field(&self.continent).is_empty()
            || self.cq_zone.is_some()
            || self.need.is_some()
            || self.path.is_some()
    }

    /// Whether the rows this filter lets through can change without the log's `index_rev`
    /// moving: an upload stamp and a card marked sent move only `content_rev`.
    pub fn reads_sent(&self) -> bool {
        self.qsl.reads_sent()
    }

    /// The free text, lower-cased as JavaScript does, when it is left to Rust because it holds a
    /// letter outside ASCII; `None` when there is none, or SQL answers it.
    pub(crate) fn text_in_rust(&self) -> Option<String> {
        let t = field(&self.text);
        (!t.is_empty() && !t.is_ascii()).then(|| js_lower(t))
    }

    /// The columns [`Self::text_matches`] reads.
    pub const TEXT_COLUMNS: [&'static str; 3] = ["comment", "name", "qth"];

    /// Whether `r` passes the free text, where it is Rust's to decide ([`Self::text_in_rust`]);
    /// always where SQL answers it.
    pub fn text_matches(&self, r: &QsoRecord) -> bool {
        let Some(t) = self.text_in_rust() else {
            return true;
        };
        [&r.comment, &r.name, &r.qth]
            .into_iter()
            .any(|f| f.as_deref().is_some_and(|s| js_lower(s).contains(&t)))
    }
}
