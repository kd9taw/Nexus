//! The contest exchange model — the SHAPE of an on-air exchange, and nothing about
//! how any one mode COPIES it.
//!
//! This module exists because `rtty::seq::ExchangeSchema` was the right shape in the
//! wrong place. That type is a PARSER schema: its `FieldKind::Rst` normalises the
//! lost-FIGS garble `TOO` back to `599` and forgives the `5NN` cut convention, and its
//! `FdClass` forgives `EA` → `3A`. Those are RTTY DECODE ARTEFACTS. On CW, phone or FT8
//! a station that sends `TOO` sent `TOO`, and a schema that silently rewrites it is
//! logging something nobody transmitted. It also has NO SENT SIDE, so every asymmetric
//! contest (a QSO party's in-state vs out-of-state roles, ARRL DX's W/VE vs DX) is
//! inexpressible; its domains were hardcoded (`FieldKind::Section` called
//! `fd_rules::valid_section` directly); and it lived under `rtty/`, importing
//! `super::demod::DecodedChar`, which put a contest engine downstream of an RTTY
//! demodulator.
//!
//! So: the SHAPE lives here, mode-neutral and dependency-free. The tolerances stay in
//! `rtty::seq`, which is the only place they are true. `rtty::seq` is this module's
//! first consumer and, at this batch, its only one.
//!
//! ⚠️ Nothing in [`spec`] reads a clock, a setting, or the rules table. The
//! [`exchanges`] submodule's [`field_day`] DOES load the rules table (for the section
//! domain) and therefore carries `fd_rules::ruleset`'s ordering rule — see its docs.
//!
//! [`scoring`] joined it for the same reason the shape did: `fd_rules::ScoringModel`
//! took a `&FieldDayLog`, which welded the scoring math to one contest's log type. It
//! is mode-neutral and log-neutral here, and reads a log through [`ScoreRow`].
//!
//! ⭐ **[`session`] holds what a run of a contest IS, and it holds only the CURRENT
//! sent exchange.** What a given contact sent lives on that contact's own row. There is
//! deliberately no function anywhere that produces a sent exchange from a session or a
//! log, because that is the shape that relabelled every row already logged the moment a
//! mobile changed county. The one function that renders a sent exchange is
//! [`render::sent_exchange`], and it takes a ROW. [`carrier`] is how a field vector
//! rides one ADIF tag, and [`dupe`] is the ordered key those vectors feed.

/// The contest scoring class — `"CW"` | `"PH"` | `"DIG"` — of an ADIF mode name.
///
/// The contest log stores the CLASS (`LoggedQso::mode`); the general log stores the
/// on-air ADIF mode. A dupe key built over both halves needs one vocabulary, and this
/// is the map between them — built on [`reconcile::mode_class`](crate::reconcile::mode_class)
/// rather than beside it, so the voice vocabulary cannot come to mean one thing for a
/// LoTW match and another for a contest dupe.
///
/// ⚠️ A record with no mode maps to `""`, not to `"DIG"`. A mode-less row must not
/// silently take on the class the operator happens to be running: that would make it a
/// dupe against a contact it may have nothing to do with, and over-reporting a dupe
/// refuses a legal contact.
pub fn mode_class(adif_mode: &str) -> &'static str {
    match crate::reconcile::mode_class(adif_mode) {
        "CW" => "CW",
        "Phone" => "PH",
        "Other" => "",
        _ => "DIG",
    }
}

pub mod adif;
pub mod cabrillo;
pub mod carrier;
pub mod dupe;
pub mod exchanges;
pub mod merge;
pub mod render;
pub mod scoring;
pub mod session;
pub mod spec;

pub use exchanges::{casual, field_day};

pub use adif::directed_columns;

pub use cabrillo::{resolve_contest_id, side_declares_call, CabrilloHeaders, OperatorCategory};

pub use dupe::{DupeRule, KEY_SEP};

pub use render::{role_for, sent_exchange, sent_exchange_string};

pub use merge::{merge_into_general, MergeReport};

pub use session::{
    ContestSession, InFlightQso, MyLocation, StationData, UploadPolicy, MOVE_CHANGES_ROLE,
    UPLOAD_CLUBLOG_SWEEP_HINT,
};

pub use scoring::{
    boards, BoardSpec, ModePoints, MultScope, MultSource, MultiplierRule, PointsRule,
    PostMultiplier, ScoreRow, Scoring,
};

pub use spec::{
    AdifTags, Domain, ExchangeSpec, FieldKind, FieldSpec, FieldValue, RoleSelector, RoleSpec,
    SerialScope,
};
