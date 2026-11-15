//! ⭐ **The operator's own contests** — "Build your own contest": a small definition the
//! operator writes (or imports from a friend), compiled into an ordinary [`FdRuleset`] and kept
//! in a USER layer beside the rules table.
//!
//! **The definition is not a rules-file row.** A rules file says everything a sponsor's contest
//! needs, in ~80 keys; a custom contest says what a club sprint needs — a name, a Cabrillo
//! token, the modes, a date window, up to five exchange fields from a FIXED menu, a dupe rule
//! and points per mode class — and [`compile`] writes the rest. Every menu entry compiles onto a
//! slot shape a shipped contest already uses, so nothing downstream (the session, the entry
//! strip, the macros, the exporters) meets a shape it has not met before.
//!
//! ⚠️ **The compiled ruleset passes the SAME validator a rules file does**
//! ([`validate_ruleset`](super::validate_ruleset)). A definition the shipped loader would refuse
//! cannot reach a session; the refusals this module adds on top are only the ones a hand-written
//! definition needs and a sponsor's file never does (a token that collides with a shipped
//! contest, a one-off date years out).
//!
//! ⚠️ **No personal data, ever** (the maintainer's ruling on sharing): the file is what Export
//! writes and Import reads, and an operator shares it. Values the operator TYPES for their own
//! exchange live in Settings. `deny_unknown_fields` refuses a file that carries anything else.
//!
//! **Ids.** Every custom id starts [`ID_PREFIX`]; a rules file may not use the prefix
//! ([`parse_spec`](super::parse_spec)) and the lookups read the rules table first, so a custom
//! contest can never shadow a shipped one, nor be shadowed by a download.
//!
//! **FT8/FT4.** A custom contest sends WSJT-X's standard messages on FT8/FT4: its id is no
//! activity [`FtMessages::of_contest`](crate::fieldday::FtMessages::of_contest) knows, and it never
//! bans a mode, so [`FtMessages::for_contest`](crate::fieldday::FtMessages::for_contest) never
//! hands it Field Day's. [`ft_shape`] NAMES the WSJT-X contest message an exchange matches, for the
//! builder to say so; nothing on the air reads it.

use super::{
    days_from_civil, table, validate_ruleset, EventWindow, FdRuleset, RulesetSpec,
    CURRENT_RULES_YEAR,
};

/// What every custom contest id starts with.
pub const ID_PREFIX: &str = "custom_";
/// The definition file's `format` value.
pub const FORMAT: &str = "nexus-custom-contest";
/// The definition-file version this build reads and writes.
pub const VERSION: u32 = 1;
/// The most exchange fields a definition may declare — the validator's own limit on what one
/// station receives.
pub const MAX_FIELDS: usize = 5;
/// The longest a one-off contest may run, and a weekend one: the rules validator's own bound.
pub const MAX_DURATION_HOURS: u64 = 72;
/// How far ahead a one-off date may be: two years. A date further out is a typo far more often
/// than it is a contest.
pub const MAX_ONCE_LEAD_SECS: u64 = 2 * 366 * 86_400;
/// The modes a definition may name.
pub const MODES: &[&str] = &["CW", "SSB", "FM", "RTTY", "PSK", "FT8", "FT4"];
/// The exchange menu, in the builder's order.
pub const FIELDS: &[&str] = &[
    "rst",
    "serial",
    "state_province",
    "fd_class",
    "arrl_section",
    "cq_zone",
    "itu_zone",
    "power",
    "grid4",
    "grid6",
    "age",
    "name",
    "text",
];
/// The most free-text fields a definition may declare (`TXT1`, `TXT2`).
pub const MAX_TEXT_FIELDS: usize = 2;
/// The longest a text field's caption may be.
const MAX_LABEL_CHARS: usize = 24;
/// The highest points a contact may be worth.
const MAX_POINTS: u32 = 100;

/// ⭐ **One operator contest, as the file Export writes and Import reads** (format v1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomDef {
    /// Always [`FORMAT`].
    pub format: String,
    /// Always [`VERSION`].
    pub version: u32,
    /// `custom_<slug>`, made once from the name ([`id_for_name`]) and stable after: a journal
    /// names its contest by it.
    pub id: String,
    /// The operator's own words. Never translated.
    pub name: String,
    /// The short name the badge shows; `""` shows the name.
    #[serde(default)]
    pub short: String,
    /// The Cabrillo `CONTEST:` token and the ADIF `CONTEST_ID`, `[A-Z0-9-]{1,32}`.
    pub cabrillo_contest: String,
    /// The modes the contest runs on, from [`MODES`]. Advisory: nothing is banned.
    pub modes: Vec<String>,
    pub window: CustomWindow,
    /// The exchange, in send order: one to [`MAX_FIELDS`] entries from the menu.
    pub exchange: Vec<CustomField>,
    /// The word after CQ on FT8/FT4 for a grid-only exchange: `TEST` (NA VHF, ARRL Digi) or
    /// `WW` (WW Digi). Read by [`ft_shape`] only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ft_cq: Option<String>,
    /// For an RST + serial exchange: the operator ASKED for WSJT-X's ARRL RTTY Roundup messages
    /// on FT8/FT4. The builder asks, and the answer defaults to no — an RST and a serial is the
    /// most common club-contest exchange, and most of those are not meant to run as the Roundup.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ft_roundup: bool,
    /// `once` (a call once per contest), `band` (once per band) or `band_mode` (once per band
    /// and mode class).
    pub dupe: String,
    /// Points per contact, by mode class. Absent is one point each.
    #[serde(default)]
    pub points: CustomPoints,
}

/// When a custom contest runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CustomWindow {
    /// Every year: the `n`th full weekend of `month` (`weekend: "nth_full"`), or the last
    /// (`"last_full"`), starting `start_hour_utc` hours after 0000Z on its Saturday (0..=47).
    Weekend {
        month: u32,
        weekend: String,
        #[serde(default)]
        n: u8,
        start_hour_utc: u64,
        duration_hours: u64,
    },
    /// One date only: `start_utc` is `YYYY-MM-DDTHH:MMZ`.
    Once {
        start_utc: String,
        duration_hours: u64,
    },
}

/// One exchange field: a menu entry, and for a text field the caption the operator gave it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomField {
    pub field: String,
    /// A caption shown beside the field. Never sent on the air.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

/// Points per contact by mode class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomPoints {
    #[serde(rename = "CW", default = "one")]
    pub cw: u32,
    #[serde(rename = "PH", default = "one")]
    pub ph: u32,
    #[serde(rename = "DIG", default = "one")]
    pub dig: u32,
}

fn one() -> u32 {
    1
}

impl Default for CustomPoints {
    fn default() -> Self {
        Self {
            cw: 1,
            ph: 1,
            dig: 1,
        }
    }
}

/// ⭐ **The WSJT-X 3.0.2 contest message an exchange has the SHAPE of** (`Configuration.hpp`,
/// `SpecialOperatingActivity`). NA VHF and ARRL Digi are one entry: they are identical on air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FtActivity {
    /// `FIELD_DAY`: class and section.
    FieldDay,
    /// `NA_VHF` / `ARRL_DIGI`: a four-character grid, `CQ TEST`.
    NaVhf,
    /// `WW_DIGI`: a four-character grid, `CQ WW`.
    WwDigi,
    /// `RTTY` (ARRL RTTY Roundup): RST, then a serial or a state/province.
    RttyRoundup,
    /// `EU_VHF`: RS, serial and a six-character grid.
    EuVhf,
}

/// The fields of `def`'s exchange, in order.
fn shape(def: &CustomDef) -> Vec<&str> {
    def.exchange.iter().map(|f| f.field.as_str()).collect()
}

/// ⭐ **Which WSJT-X contest message `def`'s exchange matches** — exact and order-sensitive, and
/// only when FT8 or FT4 is one of its modes. Pure: it names a match, it transmits nothing.
///
/// | exchange | activity |
/// |---|---|
/// | `fd_class, arrl_section` | Field Day |
/// | `grid4`, `ft_cq` `TEST` / `WW` | NA VHF / WW Digi |
/// | `rst, serial` or `rst, state_province` | ARRL RTTY Roundup |
/// | `rst, serial, grid6` | EU VHF |
///
/// Anything else — reordered, an extra or a missing field, a grid-only exchange with no CQ word —
/// is `None`, which is WSJT-X's standard messages.
///
/// ⚠️ **EU VHF's RS is two digits on the air whatever the slot says**: WSJT-X sends `52`–`59` in
/// that message, and a definition that ticks FT8 has a three-digit RST slot (only an SSB/FM-only
/// definition has a two-digit one, and that one has no FT8). Matching on the digit count would
/// make the shape unreachable.
pub fn ft_shape(def: &CustomDef) -> Option<FtActivity> {
    if !def.modes.iter().any(|m| m == "FT8" || m == "FT4") {
        return None;
    }
    match shape(def).as_slice() {
        ["fd_class", "arrl_section"] => Some(FtActivity::FieldDay),
        ["grid4"] => match def.ft_cq.as_deref() {
            Some("TEST") => Some(FtActivity::NaVhf),
            Some("WW") => Some(FtActivity::WwDigi),
            _ => None,
        },
        ["rst", "serial"] | ["rst", "state_province"] => Some(FtActivity::RttyRoundup),
        ["rst", "serial", "grid6"] => Some(FtActivity::EuVhf),
        _ => None,
    }
}

/// ⭐ **The match the operator chose to use** — [`ft_shape`], except that an RST + serial exchange
/// is the Roundup only when the operator said so ([`CustomDef::ft_roundup`]).
pub fn ft_activity(def: &CustomDef) -> Option<FtActivity> {
    let found = ft_shape(def)?;
    if shape(def).as_slice() == ["rst", "serial"] && !def.ft_roundup {
        return None;
    }
    Some(found)
}

/// ⭐ **A fresh id for `name`**: `custom_` and the name in lower-case letters, digits and `_`,
/// with `_2`, `_3`… appended while `taken` says the id is in use (Import's "Keep both").
pub fn id_for_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug: String = slug.trim_end_matches('_').chars().take(40).collect();
    let slug = slug.trim_end_matches('_');
    let base = format!(
        "{ID_PREFIX}{}",
        if slug.is_empty() { "contest" } else { slug }
    );
    if !taken(&base) {
        return base;
    }
    (2u32..)
        .map(|n| format!("{base}_{n}"))
        .find(|id| !taken(id))
        .expect("an unbounded counter finds a free id")
}

/// Is `id` a well-formed custom id: [`ID_PREFIX`], then `[a-z0-9_]`, 64 characters at most?
fn is_custom_id(id: &str) -> bool {
    id.len() <= 64
        && id.len() > ID_PREFIX.len()
        && id.starts_with(ID_PREFIX)
        && super::is_event_id(id)
}

/// Parse `YYYY-MM-DDTHH:MMZ` to Unix seconds.
fn parse_utc(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 17 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    if b[16] != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let t = s.get(r)?;
        t.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| t.parse().ok())?
    };
    let (y, mo, d, h, mi) = (
        num(0..4)?,
        num(5..7)?,
        num(8..10)?,
        num(11..13)?,
        num(14..16)?,
    );
    if !(1..=12).contains(&mo) || h > 23 || mi > 59 || y < 1970 {
        return None;
    }
    if d == 0 || d > super::days_in_month(y as i64, mo) {
        return None;
    }
    let days = days_from_civil(y as i64, mo, d);
    Some(days as u64 * 86_400 + h as u64 * 3600 + mi as u64 * 60)
}

/// The 50 states, DC, the 14 Canadian areas and `DX`, spelt as WSJT-X's packer spells them
/// (`packjt77.f90` v3.0.2, `pack77_3`'s `cmult`: `NWT`, `PEI`, `NF`, `LB`). Deliberately not the
/// shipped `arrl_rtty_qth` domain, which writes `NT` and `PE`.
///
/// ⚠️ `DX` is not in `cmult`: WSJT-X sends a DX station's serial instead.
const US_VE_QTH: &[(&str, &str)] = &[
    ("AL", "Alabama"),
    ("AK", "Alaska"),
    ("AZ", "Arizona"),
    ("AR", "Arkansas"),
    ("CA", "California"),
    ("CO", "Colorado"),
    ("CT", "Connecticut"),
    ("DE", "Delaware"),
    ("FL", "Florida"),
    ("GA", "Georgia"),
    ("HI", "Hawaii"),
    ("ID", "Idaho"),
    ("IL", "Illinois"),
    ("IN", "Indiana"),
    ("IA", "Iowa"),
    ("KS", "Kansas"),
    ("KY", "Kentucky"),
    ("LA", "Louisiana"),
    ("ME", "Maine"),
    ("MD", "Maryland"),
    ("MA", "Massachusetts"),
    ("MI", "Michigan"),
    ("MN", "Minnesota"),
    ("MS", "Mississippi"),
    ("MO", "Missouri"),
    ("MT", "Montana"),
    ("NE", "Nebraska"),
    ("NV", "Nevada"),
    ("NH", "New Hampshire"),
    ("NJ", "New Jersey"),
    ("NM", "New Mexico"),
    ("NY", "New York"),
    ("NC", "North Carolina"),
    ("ND", "North Dakota"),
    ("OH", "Ohio"),
    ("OK", "Oklahoma"),
    ("OR", "Oregon"),
    ("PA", "Pennsylvania"),
    ("RI", "Rhode Island"),
    ("SC", "South Carolina"),
    ("SD", "South Dakota"),
    ("TN", "Tennessee"),
    ("TX", "Texas"),
    ("UT", "Utah"),
    ("VT", "Vermont"),
    ("VA", "Virginia"),
    ("WA", "Washington"),
    ("WV", "West Virginia"),
    ("WI", "Wisconsin"),
    ("WY", "Wyoming"),
    ("DC", "District of Columbia"),
    ("NB", "New Brunswick"),
    ("NS", "Nova Scotia"),
    ("QC", "Quebec"),
    ("ON", "Ontario"),
    ("MB", "Manitoba"),
    ("SK", "Saskatchewan"),
    ("AB", "Alberta"),
    ("BC", "British Columbia"),
    ("NWT", "Northwest Territories"),
    ("NF", "Newfoundland"),
    ("LB", "Labrador"),
    ("NU", "Nunavut"),
    ("YT", "Yukon"),
    ("PEI", "Prince Edward Island"),
    ("DX", "Outside the US and Canada"),
];

/// The id of [`US_VE_QTH`] inside a compiled ruleset.
const US_VE_QTH_ID: &str = "us_ve_qth";

/// The rules-file slot one menu entry compiles to: `(key, adif rcvd, adif sent, source, kind)`.
///
/// `nth_text` numbers a `text` field (1, 2): its slot is `TXT1` or `TXT2`.
///
/// ⭐ `age`, `name` and `text` send what the operator TYPES for this contest
/// ([`SENT_SOURCE_ENTERED`](super::SENT_SOURCE_ENTERED)), kept in Settings and never in this file.
fn slot(field: &str, rst_digits: u8, nth_text: usize) -> Option<serde_json::Value> {
    use serde_json::json;
    let (key, rcvd, sent, source, kind) = match field {
        "rst" => (
            "RST".to_string(),
            "RST_RCVD",
            "RST_SENT",
            "constant",
            json!({"type": "rst", "digits": rst_digits}),
        ),
        "serial" => (
            "NR".into(),
            "SRX",
            "STX",
            "serial",
            json!({"type": "serial", "scope": "per_contest"}),
        ),
        "state_province" => (
            "QTH".into(),
            "",
            "",
            "setting:contest_qth_state",
            json!({"type": "enum", "domain": US_VE_QTH_ID}),
        ),
        // Field Day's own slot, tags and all; the class pattern is WSJT-X's (1 to 32
        // transmitters, A to F).
        "fd_class" => (
            "CLASS".into(),
            "CLASS",
            "",
            "setting:fd_class",
            json!({"type": "pattern", "re": "^([1-9]|[12][0-9]|3[0-2])[A-F]$"}),
        ),
        "arrl_section" => (
            "SECTION".into(),
            "ARRL_SECT",
            "MY_ARRL_SECT",
            "setting:fd_section",
            json!({"type": "enum", "domain": "fd_sections"}),
        ),
        "cq_zone" => (
            "ZN".into(),
            "CQZ",
            "MY_CQ_ZONE",
            "setting:contest_cq_zone",
            json!({"type": "number", "min": 1, "max": 40}),
        ),
        "itu_zone" => (
            "ITU".into(),
            "ITUZ",
            "MY_ITU_ZONE",
            "setting:contest_itu_zone",
            json!({"type": "number", "min": 1, "max": 90}),
        ),
        "power" => (
            "PWR".into(),
            "",
            "",
            "setting:contest_power",
            json!({"type": "text", "max_len": 4}),
        ),
        "grid4" | "grid6" => (
            "GRID".into(),
            "GRIDSQUARE",
            "MY_GRIDSQUARE",
            "setting:mygrid",
            json!({"type": "grid", "chars": if field == "grid4" { 4 } else { 6 }}),
        ),
        "age" => (
            "AGE".into(),
            "AGE",
            "",
            super::SENT_SOURCE_ENTERED,
            json!({"type": "number", "min": 0, "max": 99}),
        ),
        "name" => (
            "NAME".into(),
            "NAME",
            "",
            super::SENT_SOURCE_ENTERED,
            json!({"type": "text", "max_len": 12}),
        ),
        "text" => (
            format!("TXT{nth_text}"),
            "",
            "",
            super::SENT_SOURCE_ENTERED,
            json!({"type": "text", "max_len": 12}),
        ),
        _ => return None,
    };
    Some(json!({
        "key": key,
        "label": "",
        "required": true,
        "adif": {"rcvd": rcvd, "sent": sent},
        "source": source,
        "kind": kind,
    }))
}

/// Every refusal a definition can meet before the shared validator: the ones a hand-written
/// definition needs and a sponsor's rules file never does.
fn check(def: &CustomDef, now_unix: u64) -> Result<(), String> {
    if def.format != FORMAT {
        return Err(format!(
            "not a custom contest file (format {:?})",
            def.format
        ));
    }
    if def.version != VERSION {
        return Err(format!(
            "custom contest file version {} (this build reads version {VERSION})",
            def.version
        ));
    }
    if !is_custom_id(&def.id) {
        return Err(format!(
            "id {:?} must be {ID_PREFIX} then lower-case letters, digits and _",
            def.id
        ));
    }
    if def.name.trim().is_empty() {
        return Err("the contest needs a name".into());
    }
    if def.name.chars().count() > 60 || def.short.chars().count() > 20 {
        return Err("the name is longer than 60 characters, or the short name than 20".into());
    }
    if def
        .name
        .chars()
        .chain(def.short.chars())
        .any(char::is_control)
    {
        return Err("the name carries a control character".into());
    }
    let token = def.cabrillo_contest.as_str();
    if token.is_empty()
        || token.len() > 32
        || !token
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "Cabrillo contest name {token:?} must be 1 to 32 of A-Z, 0-9 and -"
        ));
    }
    if let Some(shipped) = table().rulesets.iter().find(|r| {
        r.contest_id.eq_ignore_ascii_case(token)
            || crate::contest::cabrillo_contest_token(r.contest_id).eq_ignore_ascii_case(token)
    }) {
        return Err(format!(
            "Cabrillo contest name {token} is the built-in contest {:?}'s — use the built-in one",
            shipped.event
        ));
    }
    if let Some(other) = super::user()
        .into_iter()
        .find(|r| r.event != def.id && r.contest_id.eq_ignore_ascii_case(token))
    {
        return Err(format!(
            "Cabrillo contest name {token} is already used by your contest {:?}",
            other.event
        ));
    }
    if def.modes.is_empty() {
        return Err("tick at least one mode".into());
    }
    for (i, m) in def.modes.iter().enumerate() {
        if !MODES.contains(&m.as_str()) {
            return Err(format!("unknown mode {m:?}"));
        }
        if def.modes[..i].contains(m) {
            return Err(format!("mode {m} is listed twice"));
        }
    }
    let duration = match &def.window {
        CustomWindow::Weekend {
            month,
            weekend,
            n,
            start_hour_utc,
            duration_hours,
        } => {
            if !(1..=12).contains(month) {
                return Err(format!("month {month}"));
            }
            match (weekend.as_str(), n) {
                ("nth_full", 1..=4) | ("last_full", _) => {}
                ("nth_full", _) => {
                    return Err(format!(
                        "full weekend {n} (pick the 1st to the 4th, or the last)"
                    ))
                }
                _ => return Err(format!("unknown weekend rule {weekend:?}")),
            }
            if *start_hour_utc > 47 {
                return Err(format!(
                    "start hour {start_hour_utc} (0 to 47 hours after 0000Z on the Saturday)"
                ));
            }
            *duration_hours
        }
        CustomWindow::Once {
            start_utc,
            duration_hours,
        } => {
            let start = parse_utc(start_utc).ok_or_else(|| {
                format!("start {start_utc:?} must be a UTC date and time like 2026-12-05T14:00Z")
            })?;
            if start > now_unix.saturating_add(MAX_ONCE_LEAD_SECS) {
                return Err(format!("start {start_utc} is more than two years away"));
            }
            *duration_hours
        }
    };
    if duration == 0 || duration > MAX_DURATION_HOURS {
        return Err(format!(
            "duration {duration} hours (1 to {MAX_DURATION_HOURS})"
        ));
    }
    if def.exchange.is_empty() || def.exchange.len() > MAX_FIELDS {
        return Err(format!(
            "the exchange has {} fields (1 to {MAX_FIELDS})",
            def.exchange.len()
        ));
    }
    let mut slots: Vec<&str> = Vec::new();
    for f in &def.exchange {
        if !FIELDS.contains(&f.field.as_str()) {
            return Err(format!("unknown exchange field {:?}", f.field));
        }
        let slot = if f.field.starts_with("grid") {
            "grid"
        } else {
            f.field.as_str()
        };
        if slot == "text" {
            if slots.iter().filter(|s| **s == "text").count() == MAX_TEXT_FIELDS {
                return Err(format!(
                    "the exchange carries more than {MAX_TEXT_FIELDS} text fields"
                ));
            }
        } else if slots.contains(&slot) {
            return Err(format!("the exchange carries {} twice", f.field));
        }
        slots.push(slot);
        if f.label.chars().count() > MAX_LABEL_CHARS || f.label.chars().any(char::is_control) {
            return Err(format!(
                "the caption of {} is longer than {MAX_LABEL_CHARS} characters, or carries a control character",
                f.field
            ));
        }
    }
    if !matches!(def.dupe.as_str(), "once" | "band" | "band_mode") {
        return Err(format!("unknown dupe rule {:?}", def.dupe));
    }
    if let Some(cq) = def.ft_cq.as_deref() {
        if !matches!(cq, "TEST" | "WW") {
            return Err(format!("FT8/FT4 CQ word {cq:?} (TEST or WW)"));
        }
    }
    let p = def.points;
    if [p.cw, p.ph, p.dig].iter().any(|&x| x > MAX_POINTS) {
        return Err(format!("points above {MAX_POINTS} per contact"));
    }
    Ok(())
}

/// The rules-file row `def` stands for, and its one-off window.
fn spec_of(def: &CustomDef) -> Result<(RulesetSpec, Option<EventWindow>), String> {
    use serde_json::json;
    let phone_only = def.modes.iter().all(|m| m == "SSB" || m == "FM");
    let rst_digits = if phone_only { 2 } else { 3 };
    let mut nth_text = 0;
    let fields: Vec<serde_json::Value> = def
        .exchange
        .iter()
        .map(|f| {
            nth_text += usize::from(f.field == "text");
            slot(&f.field, rst_digits, nth_text).expect("checked: a menu field")
        })
        .collect();
    let keys: Vec<serde_json::Value> = fields.iter().map(|f| f["key"].clone()).collect();
    let domains = if def.exchange.iter().any(|f| f.field == "state_province") {
        json!([{
            "id": US_VE_QTH_ID,
            "adif": {"rcvd": "", "sent": ""},
            "values": US_VE_QTH
                .iter()
                .map(|(code, label)| json!({"code": code, "label": label}))
                .collect::<Vec<_>>(),
        }])
    } else {
        json!([])
    };
    let (window, once) = match &def.window {
        CustomWindow::Weekend {
            month,
            weekend,
            n,
            start_hour_utc,
            duration_hours,
        } => (
            json!({
                "month": month, "weekend": weekend, "n": n,
                "start_hour_utc": start_hour_utc, "duration_hours": duration_hours,
            }),
            None,
        ),
        CustomWindow::Once {
            start_utc,
            duration_hours,
        } => {
            let start = parse_utc(start_utc).expect("checked");
            // The rule the validator reads is never used: `once` is the window for every
            // year (`FdRuleset::event_window`).
            let month = start_utc[5..7].parse::<u32>().expect("checked");
            (
                json!({
                    "month": month, "weekend": "nth_full", "n": 1,
                    "start_hour_utc": 0, "duration_hours": duration_hours,
                }),
                Some(EventWindow {
                    start_unix: start,
                    end_unix: start + duration_hours * 3600,
                }),
            )
        }
    };
    let row = json!({
        "event": def.id,
        "rules_year": CURRENT_RULES_YEAR,
        "contest_id": def.cabrillo_contest,
        "window": window,
        "scoring": {
            "model": "none",
            "points_by_mode_class": {"CW": def.points.cw, "PH": def.points.ph, "DIG": def.points.dig},
            "power_tiers": [],
            "multipliers": [],
            "relation_points": [],
            "band_points": [],
        },
        "transmitter_column": false,
        "dupe": {
            "by_call": true,
            "by_band": def.dupe != "once",
            "by_mode_class": def.dupe == "band_mode",
            "by_fields": [],
            "by_sent_fields": [],
            "log_dupes": true,
        },
        "domains": domains,
        "exchange": {
            "name": def.id,
            "fields": fields,
            "roles": [{
                "id": "",
                "selector": {"type": "always"},
                "sends": keys,
                "receives": keys,
                "constant_sent": [],
            }],
        },
        "bonuses": [],
        // ⚠️ Never a banned mode, FT8 and FT4 above all: `FtMessages::for_contest` hands a
        // contest that bans the tier's mode Field Day's messages, with a blank class.
        "banned_modes": [],
        "tempo_fd": false,
        "assistance": {"spotting_allowed": true, "cluster_allowed": true},
        "enforcement": "warn",
        "cabrillo": {
            "headers": ["CATEGORY-MODE", "CATEGORY-POWER", "CLAIMED-SCORE"],
        },
    });
    let spec: RulesetSpec = serde_json::from_value(row).map_err(|e| format!("internal: {e}"))?;
    Ok((spec, once))
}

/// ⭐ **Compile `def` into a ruleset** — every refusal a definition can meet, then the shared
/// rules validator, then the same builder a rules file goes through. `now_unix` bounds a one-off
/// date. The result is not installed ([`install_custom`](super::install_custom) does that).
///
/// Each call leaks the ruleset's strings and slices (a few KB), the same lifetime strategy the
/// rules table uses: a session borrows them `&'static`.
pub fn compile(def: &CustomDef, now_unix: u64) -> Result<FdRuleset, String> {
    check(def, now_unix)?;
    let (spec, once) = spec_of(def)?;
    validate_ruleset(&spec, true)?;
    let mut rs = super::build_ruleset(spec, &table().reserved);
    rs.window.once = once;
    rs.custom = true;
    rs.ft_activity = ft_activity(def);
    Ok(rs)
}

/// ⭐ **Check `def` without building it** — what a save or an import asks first. The same
/// verdict as [`compile`], and nothing is leaked.
pub fn validate(def: &CustomDef, now_unix: u64) -> Result<(), String> {
    check(def, now_unix)?;
    validate_ruleset(&spec_of(def)?.0, true)
}

/// Parse a definition file's text: the JSON, then [`validate`].
pub fn parse(text: &str, now_unix: u64) -> Result<CustomDef, String> {
    let def: CustomDef =
        serde_json::from_str(text).map_err(|e| format!("not a custom contest file: {e}"))?;
    validate(&def, now_unix)?;
    Ok(def)
}

/// The file Export writes for `def`: pretty JSON, the definition and nothing else.
pub fn to_file_text(def: &CustomDef) -> String {
    serde_json::to_string_pretty(def).expect("a definition always serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(modes: &[&str], fields: &[&str], ft_cq: Option<&str>) -> CustomDef {
        CustomDef {
            format: FORMAT.into(),
            version: VERSION,
            id: "custom_shape".into(),
            name: "Shape".into(),
            short: String::new(),
            cabrillo_contest: "SHAPE".into(),
            modes: modes.iter().map(|m| m.to_string()).collect(),
            window: CustomWindow::Weekend {
                month: 3,
                weekend: "last_full".into(),
                n: 0,
                start_hour_utc: 0,
                duration_hours: 4,
            },
            exchange: fields
                .iter()
                .map(|f| CustomField {
                    field: f.to_string(),
                    label: String::new(),
                })
                .collect(),
            ft_cq: ft_cq.map(str::to_string),
            ft_roundup: false,
            dupe: "band".into(),
            points: CustomPoints::default(),
        }
    }

    /// ⭐ The shape table: exact and order-sensitive, and only with FT8 or FT4 ticked.
    #[test]
    fn ft_shape_names_the_wsjtx_message_an_exchange_matches() {
        use FtActivity::*;
        let ft = &["FT8"][..];
        type Case<'a> = (
            &'a [&'a str],
            &'a [&'a str],
            Option<&'a str>,
            Option<FtActivity>,
        );
        let cases: &[Case] = &[
            (ft, &["fd_class", "arrl_section"], None, Some(FieldDay)),
            (ft, &["arrl_section", "fd_class"], None, None),
            (ft, &["grid4"], Some("TEST"), Some(NaVhf)),
            (ft, &["grid4"], Some("WW"), Some(WwDigi)),
            (ft, &["grid4"], None, None),
            (ft, &["grid6"], Some("TEST"), None),
            (ft, &["rst", "serial"], None, Some(RttyRoundup)),
            (ft, &["rst", "state_province"], None, Some(RttyRoundup)),
            (ft, &["serial", "rst"], None, None),
            (ft, &["rst", "serial", "grid6"], None, Some(EuVhf)),
            (ft, &["rst", "serial", "grid4"], None, None),
            (ft, &["rst", "serial", "cq_zone"], None, None),
            (
                &["FT4"],
                &["fd_class", "arrl_section"],
                None,
                Some(FieldDay),
            ),
            (&["CW", "RTTY"], &["fd_class", "arrl_section"], None, None),
            (&["CW"], &["rst", "serial"], None, None),
        ];
        for (modes, fields, cq, want) in cases {
            assert_eq!(
                ft_shape(&def(modes, fields, *cq)),
                *want,
                "{modes:?} {fields:?} {cq:?}"
            );
        }
    }

    /// An RST + serial exchange is the Roundup on the air only when the operator asked; the other
    /// Roundup shape and every other match need no answer.
    #[test]
    fn rst_and_serial_is_the_roundup_only_when_asked() {
        let mut d = def(&["FT8"], &["rst", "serial"], None);
        assert_eq!(ft_shape(&d), Some(FtActivity::RttyRoundup));
        assert_eq!(ft_activity(&d), None, "the default answer is no");
        d.ft_roundup = true;
        assert_eq!(ft_activity(&d), Some(FtActivity::RttyRoundup));
        let d = def(&["FT8"], &["rst", "state_province"], None);
        assert_eq!(ft_activity(&d), Some(FtActivity::RttyRoundup));
    }

    #[test]
    fn an_id_is_made_from_the_name_and_kept_unique() {
        assert_eq!(id_for_name("Club Sprint!", |_| false), "custom_club_sprint");
        assert_eq!(id_for_name("  ", |_| false), "custom_contest");
        assert_eq!(
            id_for_name("Club Sprint", |id| id == "custom_club_sprint"
                || id == "custom_club_sprint_2"),
            "custom_club_sprint_3"
        );
        assert!(is_custom_id(&id_for_name(
            "Ünïcode — Sprint 2026",
            |_| false
        )));
    }

    /// An SSB/FM-only contest sends a two-digit report; any other mode makes it three.
    #[test]
    fn the_rst_has_two_digits_only_for_a_phone_only_contest() {
        let digits = |modes: &[&str]| {
            let rs = compile(&def(modes, &["rst"], None), 1_794_614_400).expect("compiles");
            rs.exchange.fields[0].kind
        };
        assert_eq!(
            digits(&["SSB", "FM"]),
            crate::contest::FieldKind::Rst { digits: 2 }
        );
        assert_eq!(
            digits(&["SSB", "CW"]),
            crate::contest::FieldKind::Rst { digits: 3 }
        );
    }

    /// The state/province list is the packer's, with `DX`: 50 states, DC, 14 Canadian areas.
    #[test]
    fn the_us_ve_list_is_wsjtx_packers() {
        assert_eq!(US_VE_QTH.len(), 50 + 1 + 14 + 1);
        for code in ["AK", "HI", "NWT", "PEI", "NF", "LB", "DX"] {
            assert!(US_VE_QTH.iter().any(|(c, _)| *c == code), "{code}");
        }
        for code in ["NT", "PE"] {
            assert!(!US_VE_QTH.iter().any(|(c, _)| *c == code), "{code}");
        }
    }

    /// A rules file may not carry a `custom_` id: the lookups read it first, so it would hide an
    /// operator's contest.
    #[test]
    fn a_rules_file_may_not_use_the_custom_prefix() {
        let mut v: serde_json::Value = serde_json::from_str(super::super::SEED).expect("seed");
        v["rulesets"][2]["event"] = "custom_tnqp".into();
        let err = super::super::validate(&v.to_string()).expect_err("refused");
        assert!(err.contains("the \"custom_\" prefix is reserved"), "{err}");
    }
}
