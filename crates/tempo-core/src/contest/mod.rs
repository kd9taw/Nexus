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

pub mod exchanges;
pub mod scoring;
pub mod spec;

pub use exchanges::{casual, field_day};

pub use scoring::{ModePoints, PointsRule, PostMultiplier, ScoreRow, Scoring};

pub use spec::{
    AdifTags, Domain, ExchangeSpec, FieldKind, FieldSpec, FieldValue, RoleSelector, RoleSpec,
    SerialScope,
};
