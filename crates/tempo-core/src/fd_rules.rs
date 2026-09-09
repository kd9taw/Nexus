//! The centralized Field Day ruleset — SFD (ARRL Field Day) and WFD (Winter
//! Field Day) rules defined in exactly ONE place so every surface (sequencer,
//! scoring, exports, N3FJP, the sections board) stays consistent across all
//! modes, and a pre-contest refresh is a one-table edit (Field Day spec §2 +
//! its §7 update model).
//!
//! Since the rules-as-data change the PARAMETERS live in `fd_rules.seed.json`
//! (bundled via `include_str!`) — dates/window rules, points, power tiers, the
//! bonus menu, sections, banned modes, assistance policy — while the ENGINE
//! stays code: scoring math, dupe enforcement, exchange grammar, the
//! weekend-window weekday MATH ([`FdRuleset::event_window`] interprets the
//! data's window rule), and the exporters. A downloaded `fd-rules.json` can
//! replace the seed at startup through [`install_from`] — the same
//! set-once-`OnceLock`, `&'static`-borrows, next-launch-activation pattern as
//! `propagation::dxcc::init_from`, with the same seed-floor rule (the bundled
//! seed wins over an invalid or older download) and the same LOUD
//! [`RulesInitError::AlreadyInitialized`] when something read the ruleset
//! before the startup install (a code-ordering regression, not a state).
//!
//! The file carries a `schema` number and this build reads **schema 2**. It is
//! a hard refusal, not a best effort: [`RulesetSpec`] gained `exchange`,
//! `domains`, `dupe` and `scoring` as REQUIRED blocks, and the only other way
//! to express that would be to serde-default them — which would let a rules
//! file that forgot a block load and score as though its author had decided
//! something (spec §8c). A schema-1 file is therefore refused by name here.
//!
//! An older build refuses THIS file too, and keeps its own bundled seed — but
//! not by name: `parse_spec` opens with `serde_json::from_str`, and a 1.x
//! `RulesetSpec` declares `scoring: String` where schema 2 writes a block, so
//! serde fails first with `bad JSON: invalid type: map, expected a string` and
//! the schema check never runs. Same refusal, different message; do not credit
//! the version check with it.
//!
//! ⚠️ **Schema 2 is still being filled in, and additions to it are ADDITIVE by
//! rule.** The `Scoring` refactor added `scoring.multipliers` as a REQUIRED key
//! rather than bumping to schema 3, and that choice has one cost worth naming:
//! a schema-2 file published BEFORE it — there is one published artifact and
//! one URL — is refused by this build until the workflow republishes the seed
//! (which the same merge does). The refusal is inert: `fd_rules_download_if_newer`
//! validates the candidate before writing anything, so the good local copy and
//! the seed floor are untouched and Field Day keeps scoring exactly as it did.
//! The reverse direction is what the additive rule buys — a shipped 1.11.x build
//! ignores an unknown key, so it keeps receiving rules updates. A key RENAMED or
//! RESHAPED inside schema 2 would take that away, permanently; that is a schema
//! bump, not an addition.
//!
//! `rules_year` stamps each ruleset; the pinned per-event score fixtures in the
//! tests below run against [`ruleset`] = the BUNDLED SEED (an installed file is
//! invisible to them by design — its visibility to the operator is the status
//! surface and the Cabrillo `X-NEXUS-RULES-YEAR` header), so they fail if a
//! seed edit changes a score without a matching test update, catching drift
//! before it ships.

use crate::contest::{
    ModePoints, MultScope, MultSource, MultiplierRule, PointsRule, PostMultiplier,
};
use crate::fieldday::FdEvent;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The bundled parameters seed — the floor a downloaded file must beat, and
/// what [`ruleset`] activates when nothing (valid) was installed.
const SEED: &str = include_str!("fd_rules.seed.json");

/// The rules year this build targets. Only 2026 rulesets exist today, so
/// [`ruleset`] selects purely on the event; the year is carried for the
/// forthcoming multi-year table and to stamp exports.
pub const CURRENT_RULES_YEAR: u16 = 2026;

/// One Field Day bonus (replaces the old `tempo_app::FD_BONUSES` tuple table).
/// `id` is the stable settings key; `points` is what a claimed bonus scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bonus {
    pub id: &'static str,
    pub label: &'static str,
    pub points: u32,
}

/// One ARRL/RAC Field Day section: the exchange abbreviation sent on the air
/// (e.g. `WI`), its full name, and the ARRL division it sits in (so the
/// worked-sections board can lay the cells out division-by-division).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub code: &'static str,
    pub name: &'static str,
    pub division: &'static str,
}

/// When a station counts again — re-exported from [`crate::contest`], where the key
/// BUILDER lives beside it.
///
/// Both Field Day events declare `(call, band, mode class)` and no exchange slots,
/// which is the shipped rule unchanged. The two slot lists are what a QSO party needs:
/// `by_fields` for working someone else's mobile, `by_sent_fields` for being one.
pub use crate::contest::DupeRule;

/// A time window for one occurrence of an event (Unix seconds, UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventWindow {
    pub start_unix: u64,
    pub end_unix: u64,
}

/// The event's assistance policy — advisory DATA for a UI warn chip (never an
/// enforcement input; scoring and dupes don't consult it). Ships DORMANT
/// (everything allowed) until the sponsor's actual rules text is read and
/// quoted in the seed's `_provenance` — see that field before flipping a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssistancePolicy {
    pub spotting_allowed: bool,
    pub cluster_allowed: bool,
    /// i18n catalog key for the advisory note (`""` = none).
    pub assistance_note_key: &'static str,
}

/// Which Saturday of the month anchors the event weekend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekendRule {
    /// The nth Saturday whose Sunday is still in the month (SFD: 4th of June).
    NthFull(u8),
    /// The last such Saturday (WFD: last full weekend of January — the
    /// Feb-spill correction lives in "full", not here).
    LastFull,
}

/// The event-window RULE — the parameters half of the date computation. The
/// weekday MATH that interprets it stays code ([`FdRuleset::event_window`]);
/// `overrides` (absolute Unix per year) exists for a sponsor moving a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRule {
    pub month: u32,
    pub weekend: WeekendRule,
    pub start_hour_utc: u64,
    pub duration_hours: u64,
    pub overrides: &'static [(u16, EventWindow)],
}

/// The complete rules for one Field Day event in one year — the single source
/// every surface reads.
#[derive(Debug)]
pub struct FdRuleset {
    pub event: FdEvent,
    pub rules_year: u16,
    pub contest_id: &'static str,
    /// The exchange this event runs, as data. Byte-for-byte the exchange
    /// `contest::field_day` ships (cross-checked in the tests below).
    ///
    /// ⚠️ Nothing CONSUMES this yet — `contest::field_day()` is still the
    /// definition the RTTY sequencer copies against. Wiring the loaded block
    /// through the parser is a later batch; doing it here would have made this
    /// batch a behaviour change.
    pub exchange: &'static crate::contest::ExchangeSpec,
    pub scoring: crate::contest::Scoring,
    pub bonuses: &'static [Bonus],
    pub dupe_rule: DupeRule,
    /// Tempo (FT1 keyboard chat) is a first-class FD contact surface for this
    /// event: WFD `true` (the digital-friendly event), SFD `false`.
    pub tempo_fd: bool,
    /// On-air modes this event's rules BAN outright (uppercase ADIF-style
    /// names). WFD 2026 bans every WSJT mode while explicitly keeping RTTY and
    /// SSTV legal as Digital; ARRL FD bans none. Advisory data for a UI guard —
    /// scoring and dupes never consult it.
    pub banned_modes: &'static [&'static str],
    /// Display-only objectives menu (WFD; empty today — WFD reuses the bonus
    /// menu, and a real WFD objectives table flows through this field later).
    pub objectives: &'static [Bonus],
    /// Domains this ruleset DECLARES in the rules file. The reserved derived
    /// ids ([`arrl_sections_domain`], [`fd_sections_domain`]) are NOT in here —
    /// they are computed from the top-level section list, and a file that tries
    /// to declare one is refused. Empty for both Field Day events: neither
    /// needs a domain the loader does not already derive.
    pub domains: &'static [&'static crate::contest::Domain],
    pub assistance: AssistancePolicy,
    /// Advisory posture — `"warn"` only today (warn, never remove or disable a
    /// surface — operator ruling).
    pub enforcement: &'static str,
    /// The algorithmic event-window rule, as data (spec §2.3 — the parameters
    /// are never hand-edited in code anymore; they ride the rules file).
    pub window: WindowRule,
}

impl FdRuleset {
    /// Points for one claimed bonus id (`None` = unknown id, scores nothing) —
    /// the moved `fd_bonus_points` semantics.
    pub fn bonus(&self, id: &str) -> Option<u32> {
        self.bonuses.iter().find(|b| b.id == id).map(|b| b.points)
    }

    /// Total points for a set of claimed bonus ids (unknown ids score nothing).
    pub fn bonus_points(&self, claimed: &[String]) -> u32 {
        claimed.iter().filter_map(|id| self.bonus(id)).sum()
    }

    /// True if the ACTUAL on-air mode (e.g. `FT8`, `RTTY`) is banned by this
    /// event's rules (case-insensitive; whitespace trimmed). An empty mode — a
    /// legacy row with no recorded actual mode — is never banned.
    pub fn mode_banned(&self, mode: &str) -> bool {
        let up = mode.trim().to_ascii_uppercase();
        self.banned_modes.iter().any(|&m| m == up)
    }

    /// This event's window for `year`: a per-year override wins outright, else
    /// the weekday math interprets [`WindowRule`] (which weekend / start hour /
    /// duration are DATA; the full-weekend Saturday walk stays code).
    pub fn event_window(&self, year: u16) -> EventWindow {
        if let Some(&(_, w)) = self.window.overrides.iter().find(|(y, _)| *y == year) {
            return w;
        }
        let month = self.window.month;
        let sats = full_weekend_saturdays(year as i64, month, days_in_month(year as i64, month));
        let sat = match self.window.weekend {
            // Validation caps n at 4 and every month has ≥ 3 full weekends, so
            // the clamp-to-last is a never-taken guard, not a behavior.
            WeekendRule::NthFull(n) => sats
                .get(n as usize - 1)
                .or(sats.last())
                .copied()
                .expect("every month has a full weekend"),
            WeekendRule::LastFull => *sats.last().expect("every month has a full weekend"),
        };
        window(
            year as i64,
            month,
            sat,
            self.window.start_hour_utc,
            self.window.duration_hours * 3600,
        )
    }

    /// The currently-running occurrence if `now` is inside one, else the next
    /// one — what the banner/countdown DTO carries (the TS date math this
    /// replaces hardcoded a 24 h duration and dropped WFD's final six hours).
    pub fn next_or_running(&self, now_unix: u64) -> EventWindow {
        let year = civil_year_of_unix(now_unix);
        let w = self.event_window(year);
        if now_unix < w.end_unix {
            w
        } else {
            self.event_window(year + 1)
        }
    }
}

/// The active ruleset for an event + year: the newest `rules_year` ≤ `year`,
/// falling back to the newest available (only 2026 rulesets ship today).
/// Reads the loaded table — the bundled seed, or a file [`install_from`]
/// activated at startup.
pub fn ruleset(event: FdEvent, year: u16) -> &'static FdRuleset {
    let mine = || {
        table()
            .rulesets
            .iter()
            .copied()
            .filter(|r| r.event == event)
    };
    mine()
        .filter(|r| r.rules_year <= year)
        .max_by_key(|r| r.rules_year)
        .or_else(|| mine().max_by_key(|r| r.rules_year))
        .expect("validation guarantees a ruleset per event")
}

/// The ARRL/RAC section master list — the section universe the worked-sections
/// board (spec §5) and setup validation read from. Ordered and grouped by ARRL
/// division so the board renders one tidy block per division; the ordering is
/// mirrored (and guard-tested) in ui/src/features/arrlSections.ts. 71 US ARRL
/// sections + 12 RAC (Canada) = 83, carried by the rules data.
pub fn sections() -> &'static [Section] {
    table().sections
}

/// True if `code` is a known ARRL/RAC section (case-insensitive; leading/trailing
/// whitespace trimmed). The section universe is [`sections`] — the same list
/// the worked-sections board and setup validation read.
pub fn valid_section(code: &str) -> bool {
    let up = code.trim().to_ascii_uppercase();
    sections().iter().any(|s| s.code == up)
}

/// The Field Day SECTION slot's domain: the 83 ARRL/RAC section codes plus the `MX`
/// and `DX` extensions DX stations send — exactly the set the RTTY parser accepted
/// inline as `valid_section(t) || t == "MX" || t == "DX"`. Derived from the same
/// validated [`sections`] table, so the two can never disagree; nothing here
/// duplicates a code list a human would have to keep in step.
///
/// ⚠️ Same ordering rule as [`ruleset`]: calling this LOADS the rules table, so it
/// must never run before the startup [`install_from`] or the bundled seed is locked in
/// for the session (see `tests/fd_rules_too_late.rs`). Its only caller is
/// `contest::field_day()`, reached from `Engine::set_rtty_auto` — an operator action.
///
/// `MX`/`DX` carry their own code as their label: they are not sections and have no
/// section name, and inventing a display name would be inventing data.
pub fn fd_sections_domain() -> &'static crate::contest::Domain {
    static D: OnceLock<crate::contest::Domain> = OnceLock::new();
    D.get_or_init(|| derive_fd_sections(sections()))
}

/// [`fd_sections_domain`]'s body, over an EXPLICIT section slice.
///
/// ⚠️ It exists split out because [`build`] runs INSIDE `TABLE.get_or_init`, so
/// anything there that reached for the accessor — which goes through
/// [`sections`] → `table()` → the same `OnceLock` — would re-enter it and
/// DEADLOCK. (It did: the exchange block's `enum` slots resolve `fd_sections`,
/// and the first version of that resolution hung the whole test binary.) So
/// `build` derives from the slice it is already holding, and only callers
/// OUTSIDE the initialiser use the accessor.
fn derive_fd_sections(secs: &'static [Section]) -> crate::contest::Domain {
    {
        let mut values: Vec<(&'static str, &'static str)> =
            secs.iter().map(|s| (s.code, s.name)).collect();
        values.push(("MX", "MX"));
        values.push(("DX", "DX"));
        crate::contest::Domain {
            id: "fd_sections",
            // Received: <ARRL_SECT>, the CONTACTED station's section, which the Field
            // Day ADIF exporter already writes. Sent: <MY_ARRL_SECT>, the LOGGING
            // station's — checked against adif.org's own field list before it shipped
            // (ADIF 3.1.7, "updated 2026-03-22", read 2026-09-09; see
            // `contest::adif`'s header for the citation and the negative controls),
            // because nothing in this tree corroborated the name and an invented ADIF
            // field ships into other people's logbooks.
            //
            // ⚠️ This domain is the sections PLUS `MX`/`DX`, and neither is a member
            // of ADIF's ARRL Section enumeration (90 entries in 3.1.7; both absent).
            // A DX station's own `MY_ARRL_SECT: DX` is therefore an out-of-enumeration
            // value — exactly as the RECEIVED `<ARRL_SECT:2>DX` this build has shipped
            // since before the contest model existed. Kept symmetric deliberately: one
            // rule for both directions beats a sent side that silently drops what the
            // received side writes.
            adif: crate::contest::AdifTags {
                rcvd: Some("ARRL_SECT"),
                sent: Some("MY_ARRL_SECT"),
            },
            values: Box::leak(values.into_boxed_slice()),
        }
    }
}

/// The 83 ARRL/RAC section codes as a [`contest::Domain`](crate::contest::Domain)
/// — the plain section universe, with none of Field Day's `MX`/`DX`
/// extensions.
///
/// This is where §8(d)'s "exactly 83" assertion now lives as a property of a
/// DOMAIN rather than of the file. Derived from the same validated [`sections`]
/// table as [`fd_sections_domain`], so the two can never disagree and neither
/// can drift from the list the worked-sections board renders.
///
/// ⚠️ The codes deliberately do NOT move into the rules file's own `domains`
/// array. A [`Section`] carries three attributes (`code`, `name`, `division`)
/// and a `Domain` value is a `(code, label)` PAIR — `division` is what the
/// board groups by and what the TypeScript mirror guard pins, and it would have
/// nowhere to live. So `arrl_sections` and `fd_sections` are RESERVED ids the
/// loader derives, and a file that declares either is refused.
///
/// Same ordering rule as [`ruleset`]: this LOADS the rules table.
pub fn arrl_sections_domain() -> &'static crate::contest::Domain {
    static D: OnceLock<crate::contest::Domain> = OnceLock::new();
    D.get_or_init(|| derive_arrl_sections(sections()))
}

/// [`arrl_sections_domain`]'s body over an explicit slice — same re-entrancy
/// reason as [`derive_fd_sections`].
fn derive_arrl_sections(secs: &'static [Section]) -> crate::contest::Domain {
    {
        let values: Vec<(&'static str, &'static str)> =
            secs.iter().map(|s| (s.code, s.name)).collect();
        crate::contest::Domain {
            id: "arrl_sections",
            // Received: <ARRL_SECT>, the CONTACTED station's. Sent:
            // <MY_ARRL_SECT>, the LOGGING station's — verified against ADIF
            // 3.1.7 ("updated 2026-03-22", read 2026-09-09); see
            // [`derive_fd_sections`] and `contest::adif`'s header. The two
            // section domains keep the same tags deliberately: they are the same
            // universe, one of them with Field Day's MX/DX extensions.
            adif: crate::contest::AdifTags {
                rcvd: Some("ARRL_SECT"),
                sent: Some("MY_ARRL_SECT"),
            },
            values: Box::leak(values.into_boxed_slice()),
        }
    }
}

/// Domain ids the loader DERIVES from the top-level `sections` list. A rules
/// file that declares one of these in its own `domains` array is refused: it
/// would be a second copy of a list that already exists, free to drift from it,
/// and unable to carry `division` at all.
const RESERVED_DOMAIN_IDS: [&str; 2] = ["arrl_sections", "fd_sections"];

// ---------------------------------------------------------------------------
// The rules table: parse + validate + leak — and the startup-only install seam
// (the dxcc::init_from pattern).
// ---------------------------------------------------------------------------

/// The loaded rules — `&'static` per-event rulesets (leaked once at load, so
/// every existing field type keeps working) + the shared section universe.
struct RulesTable {
    /// The rules file's `generated` ISO stamp — the freshness key the download
    /// client compares, and what the status surfaces show.
    generated: &'static str,
    rulesets: &'static [&'static FdRuleset],
    sections: &'static [Section],
}

static TABLE: OnceLock<RulesTable> = OnceLock::new();

fn table() -> &'static RulesTable {
    TABLE.get_or_init(|| {
        build(
            parse_spec(SEED)
                .expect("the bundled fd-rules seed must parse — a build-time invariant"),
        )
    })
}

/// What loading learned about a rules file — the numbers the meta stamp and
/// the status surface carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesStats {
    pub generated: String,
    /// The newest `rules_year` in the file.
    pub rules_year: u16,
    pub sections: usize,
}

/// Why [`install_from`] refused a candidate file. Each cause is distinct so
/// the caller can log what actually happened; `AlreadyInitialized` is the one
/// that names a CODE bug (something read the ruleset before the startup
/// install) rather than a bad file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesInitError {
    /// Doesn't parse, or fails structural validation.
    Invalid(String),
    /// Valid, but its `generated` stamp is older than the bundled seed's (an
    /// app upgrade can bundle newer data than an old download) — the seed wins.
    OlderThanSeed { candidate: String, embedded: String },
    /// The table was already loaded — something called [`ruleset`] (or another
    /// accessor) before the startup install ran. The seed is locked in for
    /// this session; the ordering must be fixed in code, not retried.
    AlreadyInitialized,
}

impl std::fmt::Display for RulesInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RulesInitError::Invalid(why) => write!(f, "not a valid fd-rules file: {why}"),
            RulesInitError::OlderThanSeed { candidate, embedded } => write!(
                f,
                "candidate generated {candidate} is older than the bundled seed's {embedded} — the seed wins"
            ),
            RulesInitError::AlreadyInitialized => write!(
                f,
                "rules table already loaded — something read the ruleset before the startup install"
            ),
        }
    }
}

/// Validate an fd-rules text WITHOUT touching the global table — the scratch
/// parse the download client runs before installing a file to disk (a corrupt
/// download must never replace a good local copy).
pub fn validate(text: &str) -> Result<RulesStats, String> {
    parse_spec(text).map(|s| stats_of(&s))
}

/// Install a downloaded fd-rules.json as THE rules table for this process.
/// Startup-only: call before anything reads [`ruleset`]/[`sections`] (the
/// engine's FD scoring runs in its very first snapshot), because the
/// `OnceLock` is set once and every `&'static` borrow hangs off it — there is
/// no live swap, a downloaded file activates at the next launch.
///
/// Seed floor: the bundled seed wins over a candidate that is invalid
/// ([`RulesInitError::Invalid`]) or older ([`RulesInitError::OlderThanSeed`])
/// — on any `Err` the global is untouched and the seed activates lazily as
/// before. `AlreadyInitialized` means something read the table first; the
/// CALLER must log it loudly (it is a code-ordering regression).
pub fn install_from(text: &str) -> Result<RulesStats, RulesInitError> {
    let spec = parse_spec(text).map_err(RulesInitError::Invalid)?;
    let embedded = seed_generated();
    // ISO-8601 UTC stamps compare correctly as plain strings; equal is allowed
    // on purpose (an upgrade can bundle exactly the file already downloaded).
    if spec.generated.as_str() < embedded {
        return Err(RulesInitError::OlderThanSeed {
            candidate: spec.generated.clone(),
            embedded: embedded.to_string(),
        });
    }
    let stats = stats_of(&spec);
    if TABLE.set(build(spec)).is_err() {
        // The freshly-built (leaked) table is dropped from reach here — a
        // one-shot ~10 KB leak on a path that names a code bug to fix anyway.
        return Err(RulesInitError::AlreadyInitialized);
    }
    Ok(stats)
}

/// The BUNDLED seed's `generated` stamp — the floor [`install_from`] compares
/// against, and the status surface's "what would we fall back to". Cached;
/// never touches the global table.
pub fn seed_generated() -> &'static str {
    static G: OnceLock<String> = OnceLock::new();
    G.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct GeneratedOnly {
            generated: String,
        }
        serde_json::from_str::<GeneratedOnly>(SEED)
            .map(|g| g.generated)
            .unwrap_or_default()
    })
}

/// The BUNDLED seed's ruleset event ids — the floor a downloaded file must
/// contain (§8d's inversion: a download may ADD contests, never REMOVE one this
/// build ships with).
///
/// Deliberately parsed with its own minimal struct, exactly like
/// [`seed_generated`]: calling [`parse_spec`] here would recurse, because
/// `parse_spec` is the very function that consults this list.
fn seed_events() -> &'static [String] {
    static E: OnceLock<Vec<String>> = OnceLock::new();
    E.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct EventsOnly {
            rulesets: Vec<EventOnly>,
        }
        #[derive(serde::Deserialize)]
        struct EventOnly {
            event: String,
        }
        serde_json::from_str::<EventsOnly>(SEED)
            .map(|e| e.rulesets.into_iter().map(|r| r.event).collect())
            .unwrap_or_default()
    })
}

/// The ACTIVE rules data's `generated` stamp — whichever file won at startup.
/// NB this loads the table (with the seed) if nothing has yet, exactly like
/// [`ruleset`].
pub fn active_generated() -> &'static str {
    table().generated
}

/// The ACTIVE table's newest `rules_year`, for the status surface. Same
/// load-if-needed caveat as [`active_generated`].
pub fn active_rules_year() -> u16 {
    table()
        .rulesets
        .iter()
        .map(|r| r.rules_year)
        .max()
        .unwrap_or(CURRENT_RULES_YEAR)
}

// ---- serde specs + structural validation ----------------------------------

#[derive(Debug, serde::Deserialize)]
struct FileSpec {
    schema: u32,
    generated: String,
    rulesets: Vec<RulesetSpec>,
    sections: Vec<SectionSpec>,
}

#[derive(Debug, serde::Deserialize)]
struct SectionSpec {
    code: String,
    name: String,
    division: String,
}

#[derive(Debug, serde::Deserialize)]
struct RulesetSpec {
    event: String,
    rules_year: u16,
    contest_id: String,
    window: WindowSpec,
    scoring: ScoringSpec,
    dupe: DupeSpec,
    domains: Vec<DomainSpec>,
    exchange: ExchangeBlockSpec,
    bonuses: Vec<BonusSpec>,
    banned_modes: Vec<String>,
    tempo_fd: bool,
    assistance: AssistanceSpec,
    enforcement: String,
    #[serde(default)]
    objectives: Vec<BonusSpec>,
}

/// One ADIF tag pair in the rules FILE, one tag per direction.
///
/// Both halves are required `String`s and `""` is the explicit "no standard
/// ADIF column this direction; the value rides the private carrier (§3.5)"
/// marker. NOT `Option<String>`: serde fills a missing `Option` with `None`
/// with no attribute at all, so an optional tag would let a file that simply
/// forgot the sent side load as though its author had decided there was no
/// sent-side column. Absent must be a refusal; empty must be a decision.
#[derive(Debug, serde::Deserialize)]
struct AdifTagsSpec {
    /// What THEY sent me. `""` = no standard column that way round.
    rcvd: String,
    /// What I sent them. `""` = no standard column that way round.
    sent: String,
}

/// One legal value of a file-declared domain: the code that goes on the air and
/// the name a human reads.
#[derive(Debug, serde::Deserialize)]
struct DomainValueSpec {
    code: String,
    label: String,
}

/// A named set of legal values a rules file declares for itself — a QSO party's
/// county list, a state list, a precedence set.
#[derive(Debug, serde::Deserialize)]
struct DomainSpec {
    /// Stable id, `^[a-z][a-z0-9_]*$`, unique within the ruleset and never one
    /// of [`RESERVED_DOMAIN_IDS`].
    id: String,
    adif: AdifTagsSpec,
    values: Vec<DomainValueSpec>,
}

/// What kind of value an exchange slot holds, in the rules file.
///
/// Internally tagged on `type`, so a slot reads
/// `{ "type": "pattern", "re": "^[0-9]{1,2}[ABCDEF]$" }`. The arms are exactly
/// [`contest::FieldKind`](crate::contest::FieldKind)'s nine — a tenth would
/// mean a contest the model does not cover, not a special case to bolt on.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KindSpec {
    Rst { digits: u8 },
    Serial { scope: String },
    Enum { domain: String },
    Pattern { re: String },
    Number { min: u32, max: u32 },
    Grid { chars: u8 },
    Text { max_len: u8 },
    Call,
    OneOf { of: Vec<KindSpec> },
}

/// One exchange slot in the rules file.
#[derive(Debug, serde::Deserialize)]
struct FieldBlockSpec {
    /// SLOT ID, not an export tag: uppercase, unique, named by the roles.
    key: String,
    /// The on-air label that introduces the field. `""` = positional.
    label: String,
    required: bool,
    adif: AdifTagsSpec,
    kind: KindSpec,
}

/// How a role is matched against the operator.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SelectorSpec {
    MyLocationIn { locations: Vec<String> },
    MyCategoryIs { category: String },
    Always,
}

/// One side of an asymmetric contest, in the rules file.
#[derive(Debug, serde::Deserialize)]
struct RoleBlockSpec {
    /// `""` for the single role of a symmetric contest.
    id: String,
    selector: SelectorSpec,
    sends: Vec<String>,
    receives: Vec<String>,
    constant_sent: Vec<String>,
}

/// A whole exchange, in the rules file.
#[derive(Debug, serde::Deserialize)]
struct ExchangeBlockSpec {
    name: String,
    fields: Vec<FieldBlockSpec>,
    roles: Vec<RoleBlockSpec>,
}

/// The dupe key for one ruleset, as data.
///
/// Replaces the `DUPE_CALL_BAND_MODE` const every ruleset used to share. The
/// SHAPE is unchanged and so is the enforcement — the check itself still lives
/// in [`FieldDayLog`](crate::fieldday::FieldDayLog); this is what an event
/// DECLARES its key to be.
///
/// §11.3 generalises this to `by_fields` / `by_sent_fields` naming exchange
/// slots, and the validator cross-checks that go with it; that is batch 3.
#[derive(Debug, serde::Deserialize)]
struct DupeSpec {
    /// A station counts once per callsign. Required to be `true` — see
    /// `parse_spec`.
    by_call: bool,
    /// …and separately per band.
    by_band: bool,
    /// …and separately per mode class (PH / CW / DIG).
    by_mode_class: bool,
    /// RECEIVED slot ids — working someone else's mobile: same call, same band,
    /// same mode, a DIFFERENT county is a new contact.
    ///
    /// ⚠️ **Required, not serde-defaulted.** A defaulted list means a rules file
    /// that forgot a sponsor's mobile rule loads and silently refuses legal
    /// contacts; the version number is where a loud failure belongs.
    by_fields: Vec<String>,
    /// SENT slot ids — BEING the mobile. Needed for the same sponsor's rule as
    /// `by_fields`: when I move and work the same station again THEIR exchange is
    /// unchanged, so a key built from the received side alone refuses my own
    /// legal contact.
    by_sent_fields: Vec<String>,
}

/// The whole scoring model for one ruleset, as one block.
///
/// `model` used to sit beside two flat siblings (`points_by_mode_class` and
/// `power_tiers`) that were just as much part of "how this event scores".
/// JSON cannot hold both `"scoring": "powered_multiplier"` and
/// `"scoring": { … }`, and a block containing only the model string beside two
/// flat siblings is a rename dressed as a block — so the block takes all three.
///
/// ⚠️ No `#[serde(default)]` and no `Option` anywhere in here, and that is not
/// stylistic: serde silently fills a missing `Option` field with `None` even
/// with no attribute at all, so an optional block would let a rules file that
/// forgot how an event scores load and score as though its author had decided
/// something (spec §8c). Absent must be loud, and the schema number is what
/// makes it loud.
#[derive(Debug, serde::Deserialize)]
struct ScoringSpec {
    /// Which POST-multiplier profile this event runs: `"powered_multiplier"`
    /// (ARRL FD — a legal power tier) or `"objectives"` (WFD — objectives
    /// applied at submission, so nothing multiplies on the air). Both carry the
    /// claimed bonus menu.
    ///
    /// ⚠️ The Rust model behind this is three independent axes
    /// ([`contest::Scoring`](crate::contest::Scoring)), and this string names a
    /// pair of them; a contest whose points are not per-mode-class cannot be
    /// written in this file yet. That is deliberate: widening it is a change to
    /// the PUBLISHED artifact, and there is one file and one URL, so a shape
    /// change strands every shipped build's rules updates on its bundled seed
    /// (spec §8d). It widens in the batch that ships the first contest needing
    /// it, not one written speculatively ahead of one.
    model: String,
    /// Per-mode-class QSO points. `PH`, `CW` and `DIG` are all required.
    points_by_mode_class: BTreeMap<String, u32>,
    /// Legal power multipliers, strictly ascending.
    power_tiers: Vec<u32>,
    /// The multiplier universes this event counts. **`[]` is the explicit "this
    /// event has no multiplier"** — both Field Day events write it — and the key
    /// is REQUIRED so that a rules file which simply forgot CQ WW's zones cannot
    /// load and score a log at a fraction of its real total (spec §8c: absent
    /// must be loud).
    multipliers: Vec<MultiplierSpec>,
}

/// One [`MultiplierRule`] in the rules file.
#[derive(Debug, serde::Deserialize)]
struct MultiplierSpec {
    id: String,
    source: MultSourceSpec,
    scope: MultScopeSpec,
    /// Values that do NOT count. `[]` = every value counts.
    excluding: Vec<String>,
    /// Which roles count this multiplier. `[]` = every role.
    roles: Vec<String>,
}

/// Where a multiplier's value comes from, in the rules file — internally tagged
/// on `type`, exactly like [`KindSpec`].
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MultSourceSpec {
    /// An exchange slot the worked station sent me. `domain` names which arm of
    /// a `one_of` slot counts; `""` is the explicit "the whole value, whatever
    /// it matched", the same statement `""` makes in an `adif` tag pair.
    Field {
        key: String,
        domain: String,
    },
    DxccEntity,
    Prefix,
}

/// The scope vocabulary is CLOSED — serde refuses an unknown one by name, which
/// is what stops `"per_hour"` from loading as a silent per-log count.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
// The shared `Per` prefix is the wire vocabulary, one variant per `MultScope`
// arm; dropping it here would make the file's names and the type's names differ
// for a lint's sake, and `rename_all` derives the wire strings from these.
#[allow(clippy::enum_variant_names)]
enum MultScopeSpec {
    PerLog,
    PerBand,
    PerMode,
    PerBandMode,
}

#[derive(Debug, serde::Deserialize)]
struct WindowSpec {
    month: u32,
    weekend: String,
    #[serde(default)]
    n: u8,
    start_hour_utc: u64,
    duration_hours: u64,
    #[serde(default)]
    overrides: BTreeMap<String, WindowOverrideSpec>,
}

#[derive(Debug, serde::Deserialize)]
struct WindowOverrideSpec {
    start_unix: u64,
    end_unix: u64,
}

#[derive(Debug, serde::Deserialize)]
struct BonusSpec {
    id: String,
    label: String,
    points: u32,
}

#[derive(Debug, serde::Deserialize)]
struct AssistanceSpec {
    spotting_allowed: bool,
    cluster_allowed: bool,
    #[serde(default)]
    assistance_note_key: String,
}

fn event_of(s: &str) -> Option<FdEvent> {
    match s {
        "arrlfd" => Some(FdEvent::ArrlFd),
        "wfd" => Some(FdEvent::WinterFd),
        _ => None,
    }
}

fn stats_of(spec: &FileSpec) -> RulesStats {
    RulesStats {
        generated: spec.generated.clone(),
        rules_year: spec
            .rulesets
            .iter()
            .map(|r| r.rules_year)
            .max()
            .unwrap_or(0),
        sections: spec.sections.len(),
    }
}

/// Is `id` a well-formed domain id — `^[a-z][a-z0-9_]*$`? Lowercase snake so a
/// domain id is never confused with an exchange SLOT id, which is uppercase.
fn is_domain_id(id: &str) -> bool {
    let mut cs = id.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_lowercase())
        && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Is `t` a usable ADIF tag name — `""` (the explicit "no column this
/// direction" marker) or `^[A-Z][A-Z0-9_]*$`?
fn is_adif_tag(t: &str) -> bool {
    if t.is_empty() {
        return true;
    }
    let mut cs = t.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_uppercase())
        && cs.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Does `id` name a domain this ruleset can resolve — one it declares itself,
/// or one of the reserved ids the loader derives from the section list? The one
/// resolution path, so the file and the loader can never disagree about which
/// domains exist.
fn resolve_domain(r: &RulesetSpec, id: &str) -> bool {
    RESERVED_DOMAIN_IDS.contains(&id) || r.domains.iter().any(|d| d.id == id)
}

/// Validate one [`KindSpec`], recursing into `one_of` arms. `tag`/`key` are
/// only for the message.
fn check_kind(r: &RulesetSpec, tag: &str, key: &str, k: &KindSpec) -> Result<(), String> {
    match k {
        KindSpec::Rst { digits } => {
            // 2 on phone, 3 on CW/digital. Zero would index an empty slice
            // downstream; anything above 3 is not an RST.
            if !(2..=3).contains(digits) {
                return Err(format!(
                    "{tag}: {key} rst digits {digits} (expected 2 or 3)"
                ));
            }
        }
        KindSpec::Serial { scope } => match scope.as_str() {
            "per_contest" => {}
            "per_band" => {
                return Err(format!(
                    "{tag}: {key} serial scope per_band is not supported \
                     (this build allocates one series per contest)"
                ))
            }
            other => return Err(format!("{tag}: {key} unknown serial scope {other:?}")),
        },
        KindSpec::Enum { domain } => {
            if !resolve_domain(r, domain) {
                return Err(format!("{tag}: {key} names unknown domain {domain:?}"));
            }
        }
        KindSpec::Pattern { re } => {
            // Anchored on both ends or it is not the pattern it claims: an
            // unanchored `[0-9]{1,2}[ABCDEF]` matches inside any longer token.
            if re.is_empty() || !re.starts_with('^') || !re.ends_with('$') {
                return Err(format!("{tag}: {key} pattern {re:?} is not ^…$-anchored"));
            }
        }
        KindSpec::Number { min, max } => {
            if min > max {
                return Err(format!("{tag}: {key} number min {min} > max {max}"));
            }
        }
        KindSpec::Grid { chars } => {
            if !matches!(chars, 4 | 6) {
                return Err(format!("{tag}: {key} grid chars {chars} (expected 4 or 6)"));
            }
        }
        KindSpec::Text { max_len } => {
            if *max_len == 0 {
                return Err(format!("{tag}: {key} text max_len 0"));
            }
        }
        KindSpec::Call => {}
        KindSpec::OneOf { of } => {
            // One arm is not a choice; zero is not a slot.
            if of.len() < 2 {
                return Err(format!("{tag}: {key} one_of needs at least 2 arms"));
            }
            for arm in of {
                check_kind(r, tag, key, arm)?;
            }
        }
    }
    Ok(())
}

/// Parse + the structural validation the loader, the download client AND the
/// publish gate all run.
///
/// It is THE authority on what a rules file may be: `.github/workflows/fd-rules.yml`
/// reaches this same function through `src/bin/fd-rules-check.rs` before pushing
/// the seed to the rolling `fd-rules` Release, so there is nothing to keep in
/// step with. (There was: a node port of this function, which four rounds of
/// parity fixes never reconciled.) A fixture that disagrees with this function
/// is a wrong fixture.
fn parse_spec(text: &str) -> Result<FileSpec, String> {
    let spec: FileSpec = serde_json::from_str(text).map_err(|e| format!("bad JSON: {e}"))?;
    // §8(d). A widened RulesetSpec has exactly two expressible forms: bump the
    // schema, or serde-default every new block. §8(c) rules the second out — a
    // defaulted `exchange` means a rules file that forgot one loads and scores
    // as though the contest had no exchange. So the version number is the loud
    // failure, and it names BOTH numbers so a refusal says which side is stale.
    if spec.schema != 2 {
        return Err(format!(
            "schema {} (this build reads schema 2)",
            spec.schema
        ));
    }
    if spec.generated.is_empty() {
        return Err("empty `generated` stamp".into());
    }
    // The inversion (§8d): a download may ADD contests, never REMOVE one the
    // BUNDLED seed carries. Strictly stronger than the hardcoded pair this
    // replaces — it grows with the seed instead of having to be re-edited
    // alongside it, which is exactly the drift that would otherwise appear the
    // first time a contest is added.
    for want in seed_events() {
        if !spec.rulesets.iter().any(|r| &r.event == want) {
            return Err(format!("missing the `{want}` ruleset"));
        }
    }
    let mut seen_events: Vec<(&str, u16)> = Vec::new();
    for r in &spec.rulesets {
        let tag = format!("ruleset {}/{}", r.event, r.rules_year);
        if event_of(&r.event).is_none() {
            return Err(format!("{tag}: unknown event"));
        }
        if seen_events.contains(&(r.event.as_str(), r.rules_year)) {
            return Err(format!("{tag}: duplicate event+year"));
        }
        seen_events.push((r.event.as_str(), r.rules_year));
        if r.contest_id.is_empty() {
            return Err(format!("{tag}: empty contest_id"));
        }
        // The same four checks as before the block landed, on the block's own
        // paths. Messages are deliberately unchanged: they are what the corpus
        // fixtures match on.
        if !matches!(
            r.scoring.model.as_str(),
            "powered_multiplier" | "objectives"
        ) {
            return Err(format!(
                "{tag}: unknown scoring model {:?}",
                r.scoring.model
            ));
        }
        for k in ["PH", "CW", "DIG"] {
            if !r.scoring.points_by_mode_class.contains_key(k) {
                return Err(format!("{tag}: points_by_mode_class misses {k}"));
            }
        }
        if r.scoring.power_tiers.is_empty() {
            return Err(format!("{tag}: empty power_tiers"));
        }
        if !r.scoring.power_tiers.windows(2).all(|w| w[0] < w[1]) {
            return Err(format!("{tag}: power_tiers not strictly ascending"));
        }
        // A dupe rule that does not key on the callsign is not a dupe rule.
        // Every contest in the researched set keys on it, so a file saying
        // otherwise is a mistake far more often than it is a new contest shape
        // — and the failure mode if it is wrong is a log full of contacts that
        // should have been refused as dupes.
        if !r.dupe.by_call {
            return Err(format!(
                "{tag}: dupe.by_call is false (a dupe rule must key on the callsign)"
            ));
        }
        // File-declared domains. `arrl_sections` / `fd_sections` are DERIVED
        // from the top-level section list (Ruling B0-C) — a file declaring one
        // would be a second copy of a list it cannot fully represent, since a
        // Domain value is a (code, label) pair and a Section also carries a
        // division.
        let mut domain_ids: Vec<&str> = Vec::new();
        for d in &r.domains {
            if !is_domain_id(&d.id) {
                return Err(format!(
                    "{tag}: domain id {:?} is not ^[a-z][a-z0-9_]*$",
                    d.id
                ));
            }
            if RESERVED_DOMAIN_IDS.contains(&d.id.as_str()) {
                return Err(format!(
                    "{tag}: domain id {:?} is reserved (derived from the section list)",
                    d.id
                ));
            }
            if domain_ids.contains(&d.id.as_str()) {
                return Err(format!("{tag}: duplicate domain id {:?}", d.id));
            }
            domain_ids.push(&d.id);
            if !is_adif_tag(&d.adif.rcvd) || !is_adif_tag(&d.adif.sent) {
                return Err(format!("{tag}: domain {} has a malformed adif tag", d.id));
            }
            if d.values.is_empty() {
                return Err(format!("{tag}: domain {} has no values", d.id));
            }
            let mut codes: Vec<&str> = Vec::new();
            for v in &d.values {
                if v.code.is_empty() || v.code != v.code.to_ascii_uppercase() {
                    return Err(format!(
                        "{tag}: domain {} code {:?} not uppercase",
                        d.id, v.code
                    ));
                }
                if codes.contains(&v.code.as_str()) {
                    return Err(format!(
                        "{tag}: domain {} duplicate code {:?}",
                        d.id, v.code
                    ));
                }
                codes.push(&v.code);
                if v.label.is_empty() {
                    return Err(format!(
                        "{tag}: domain {} code {} has no label",
                        d.id, v.code
                    ));
                }
            }
        }
        // The exchange block (§2.5). Every rule here is a rules bug that must
        // be a REFUSAL rather than a runtime lookup miss on the air.
        let x = &r.exchange;
        if x.name.is_empty() {
            return Err(format!("{tag}: exchange has no name"));
        }
        let mut keys: Vec<&str> = Vec::new();
        for f in &x.fields {
            if f.key.is_empty() || f.key != f.key.to_ascii_uppercase() {
                return Err(format!("{tag}: exchange slot {:?} not uppercase", f.key));
            }
            if keys.contains(&f.key.as_str()) {
                return Err(format!("{tag}: duplicate exchange slot {:?}", f.key));
            }
            keys.push(&f.key);
            if !is_adif_tag(&f.adif.rcvd) || !is_adif_tag(&f.adif.sent) {
                return Err(format!("{tag}: slot {} has a malformed adif tag", f.key));
            }
            check_kind(r, &tag, &f.key, &f.kind)?;
        }
        if x.roles.is_empty() {
            return Err(format!("{tag}: exchange has no roles"));
        }
        let mut role_ids: Vec<&str> = Vec::new();
        for role in &x.roles {
            if role_ids.contains(&role.id.as_str()) {
                return Err(format!("{tag}: duplicate role id {:?}", role.id));
            }
            role_ids.push(&role.id);
            for key in role
                .sends
                .iter()
                .chain(&role.receives)
                .chain(&role.constant_sent)
            {
                if !keys.contains(&key.as_str()) {
                    return Err(format!(
                        "{tag}: role {:?} names undeclared slot {key:?}",
                        role.id
                    ));
                }
            }
            // constant_sent constrains something I actually transmit. Read the
            // other way round it would refuse legal contacts, because every
            // station I work legitimately sends a different value.
            for key in &role.constant_sent {
                if !role.sends.contains(key) {
                    return Err(format!(
                        "{tag}: role {:?} constant_sent {key:?} is not in sends",
                        role.id
                    ));
                }
            }
            // Five is the layout budget at the 1024 px supported floor. A limit
            // the layout cannot honour is not a limit.
            if role.receives.len() > 5 {
                return Err(format!(
                    "{tag}: role {:?} receives {} fields (max 5)",
                    role.id,
                    role.receives.len()
                ));
            }
            if matches!(role.selector, SelectorSpec::Always) && x.roles.len() != 1 {
                return Err(format!(
                    "{tag}: role {:?} selector `always` must be the only role \
                     (a role after it could never be reached)",
                    role.id
                ));
            }
            if let SelectorSpec::MyLocationIn { locations } = &role.selector {
                if locations.is_empty() {
                    return Err(format!("{tag}: role {:?} my_location_in is empty", role.id));
                }
            }
            if let SelectorSpec::MyCategoryIs { category } = &role.selector {
                if category.is_empty() {
                    return Err(format!("{tag}: role {:?} my_category_is is empty", role.id));
                }
            }
        }
        // The multiplier rules (§2.5). Nothing SCORES a multiplier yet — no
        // shipped ruleset declares one — so every check here is the difference
        // between a rules bug caught at load and a multiplier that silently
        // counts nothing on contest Saturday. They are validated on the way in
        // for the same reason the exchange is: the loader is the only place
        // that sees the file and the exchange in the same breath.
        // ⭐ A dupe key that names a slot nobody exchanges is a rule that can never
        // fire, and the way it fails is silent: every contact keys on the empty string
        // in that position, so the mobile rule the sponsor wrote simply does not
        // happen. Read against the wrong direction it is worse — `by_fields` against
        // `sends` would key my own constant exchange and refuse legal contacts — so the
        // two lists are checked against the two directions separately.
        for key in &r.dupe.by_fields {
            if !x.roles.iter().any(|role| role.receives.contains(key)) {
                return Err(format!(
                    "{tag}: dupe.by_fields names slot {key:?}, which no role receives"
                ));
            }
        }
        for key in &r.dupe.by_sent_fields {
            if !x.roles.iter().any(|role| role.sends.contains(key)) {
                return Err(format!(
                    "{tag}: dupe.by_sent_fields names slot {key:?}, which no role sends"
                ));
            }
        }
        let mut mult_ids: Vec<&str> = Vec::new();
        for m in &r.scoring.multipliers {
            if m.id.is_empty() {
                return Err(format!("{tag}: empty multiplier id"));
            }
            if mult_ids.contains(&m.id.as_str()) {
                return Err(format!("{tag}: duplicate multiplier id {:?}", m.id));
            }
            mult_ids.push(&m.id);
            if let MultSourceSpec::Field { key, domain } = &m.source {
                // A multiplier counts values the OTHER station sent me, so its
                // slot has to be one some role receives. Read against `sends`
                // it would count my own constant exchange once and stop.
                if !x.roles.iter().any(|role| role.receives.contains(key)) {
                    return Err(format!(
                        "{tag}: multiplier {:?} names slot {key:?}, which no role receives",
                        m.id
                    ));
                }
                if !domain.is_empty() && !resolve_domain(r, domain) {
                    return Err(format!(
                        "{tag}: multiplier {:?} names unknown domain {domain:?}",
                        m.id
                    ));
                }
            }
            for role_id in &m.roles {
                if !x.roles.iter().any(|role| &role.id == role_id) {
                    return Err(format!(
                        "{tag}: multiplier {:?} names undeclared role {role_id:?}",
                        m.id
                    ));
                }
            }
        }
        let mut ids: Vec<&str> = Vec::new();
        for b in r.bonuses.iter().chain(&r.objectives) {
            if b.id.is_empty() {
                return Err(format!("{tag}: empty bonus id"));
            }
            if ids.contains(&b.id.as_str()) {
                return Err(format!("{tag}: duplicate bonus id {:?}", b.id));
            }
            ids.push(&b.id);
        }
        for m in &r.banned_modes {
            if m.is_empty() || *m != m.to_ascii_uppercase() {
                return Err(format!("{tag}: banned mode {m:?} not uppercase"));
            }
        }
        if r.enforcement != "warn" {
            return Err(format!(
                "{tag}: enforcement {:?} (this build only warns — never removes or disables)",
                r.enforcement
            ));
        }
        let w = &r.window;
        if !(1..=12).contains(&w.month) {
            return Err(format!("{tag}: window month {}", w.month));
        }
        match w.weekend.as_str() {
            "nth_full" if (1..=4).contains(&w.n) => {}
            "last_full" => {}
            _ => return Err(format!("{tag}: window weekend {:?} n={}", w.weekend, w.n)),
        }
        if w.start_hour_utc >= 24 {
            return Err(format!("{tag}: window start_hour_utc {}", w.start_hour_utc));
        }
        if !(1..=72).contains(&w.duration_hours) {
            return Err(format!("{tag}: window duration_hours {}", w.duration_hours));
        }
        for (y, o) in &w.overrides {
            if y.parse::<u16>().is_err() {
                return Err(format!("{tag}: override year {y:?}"));
            }
            if o.start_unix >= o.end_unix {
                return Err(format!("{tag}: override {y} start ≥ end"));
            }
        }
    }
    // The section universe is pinned (71 US + 12 RAC): the TS mirror guard and
    // the board layout both assume it, so a file that grows or shrinks it must
    // land in lockstep with a code release, not as a data push.
    if spec.sections.len() != 83 {
        return Err(format!("{} sections (expected 83)", spec.sections.len()));
    }
    let mut codes: Vec<&str> = Vec::new();
    for s in &spec.sections {
        if s.code.is_empty() || s.code != s.code.to_ascii_uppercase() {
            return Err(format!("section code {:?} not uppercase", s.code));
        }
        if codes.contains(&s.code.as_str()) {
            return Err(format!("duplicate section code {:?}", s.code));
        }
        codes.push(&s.code);
        if s.name.is_empty() || s.division.is_empty() {
            return Err(format!("section {} misses name/division", s.code));
        }
    }
    Ok(spec)
}

// ---- build: leak the validated spec into the existing &'static shapes ------

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// A rules-file ADIF tag (`""` = no standard column this direction) as the
/// `Option` the `contest` types use. The file says it with an empty string
/// because serde must be able to tell "absent" from "deliberately none"; the
/// Rust side says it with `None` because there is nothing to tell apart once
/// the file has been validated.
fn leak_opt_tag(t: String) -> Option<&'static str> {
    if t.is_empty() {
        None
    } else {
        Some(leak_str(t))
    }
}

fn leak_bonuses(v: Vec<BonusSpec>) -> &'static [Bonus] {
    Box::leak(
        v.into_iter()
            .map(|b| Bonus {
                id: leak_str(b.id),
                label: leak_str(b.label),
                points: b.points,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// The two DERIVED domains, built from the table's own section slice and
/// handed down through `build` — never fetched from the public accessors,
/// which would re-enter `TABLE.get_or_init` and deadlock (see
/// [`derive_fd_sections`]).
struct Reserved {
    arrl_sections: &'static crate::contest::Domain,
    fd_sections: &'static crate::contest::Domain,
}

/// A validated [`KindSpec`] as the neutral [`contest::FieldKind`]. `domains` is
/// the ruleset's already-built domain list; a reserved id resolves to the
/// derived domain instead. Every lookup here is infallible because
/// `check_kind` already refused anything that would miss.
fn build_kind(
    k: KindSpec,
    domains: &[&'static crate::contest::Domain],
    reserved: &Reserved,
) -> crate::contest::FieldKind {
    use crate::contest::FieldKind as K;
    match k {
        KindSpec::Rst { digits } => K::Rst { digits },
        // `check_kind` refuses every scope but per_contest, so this build never
        // constructs a PerBand — the variant exists so a file asking for it is
        // refused by name rather than silently scored as per-contest.
        KindSpec::Serial { .. } => K::Serial {
            scope: crate::contest::SerialScope::PerContest,
        },
        KindSpec::Enum { domain } => K::Enum {
            domain: match domain.as_str() {
                "arrl_sections" => reserved.arrl_sections,
                "fd_sections" => reserved.fd_sections,
                other => domains
                    .iter()
                    .copied()
                    .find(|d| d.id == other)
                    .expect("validated by resolve_domain"),
            },
        },
        KindSpec::Pattern { re } => K::Pattern { re: leak_str(re) },
        KindSpec::Number { min, max } => K::Number { min, max },
        KindSpec::Grid { chars } => K::Grid { chars },
        KindSpec::Text { max_len } => K::Text { max_len },
        KindSpec::Call => K::Call,
        KindSpec::OneOf { of } => K::OneOf(Box::leak(
            of.into_iter()
                .map(|a| build_kind(a, domains, reserved))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
    }
}

/// Leak a list of slot ids as `&'static [&'static str]`.
fn leak_keys(v: Vec<String>) -> &'static [&'static str] {
    Box::leak(
        v.into_iter()
            .map(leak_str)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// A validated [`ExchangeBlockSpec`] as the neutral [`contest::ExchangeSpec`].
fn build_exchange(
    x: ExchangeBlockSpec,
    domains: &[&'static crate::contest::Domain],
    reserved: &Reserved,
) -> &'static crate::contest::ExchangeSpec {
    let fields: Vec<crate::contest::FieldSpec> = x
        .fields
        .into_iter()
        .map(|f| crate::contest::FieldSpec {
            key: leak_str(f.key),
            adif: crate::contest::AdifTags {
                rcvd: leak_opt_tag(f.adif.rcvd),
                sent: leak_opt_tag(f.adif.sent),
            },
            // `""` in the file means positional — the same statement `None`
            // makes in the type.
            label: if f.label.is_empty() {
                None
            } else {
                Some(leak_str(f.label))
            },
            required: f.required,
            kind: build_kind(f.kind, domains, reserved),
        })
        .collect();
    let roles: Vec<crate::contest::RoleSpec> = x
        .roles
        .into_iter()
        .map(|r| crate::contest::RoleSpec {
            id: leak_str(r.id),
            selector: match r.selector {
                SelectorSpec::MyLocationIn { locations } => {
                    crate::contest::RoleSelector::MyLocationIn(leak_keys(locations))
                }
                SelectorSpec::MyCategoryIs { category } => {
                    crate::contest::RoleSelector::MyCategoryIs(leak_str(category))
                }
                SelectorSpec::Always => crate::contest::RoleSelector::Always,
            },
            sends: leak_keys(r.sends),
            receives: leak_keys(r.receives),
            constant_sent: leak_keys(r.constant_sent),
        })
        .collect();
    Box::leak(Box::new(crate::contest::ExchangeSpec {
        name: leak_str(x.name),
        fields: Box::leak(fields.into_boxed_slice()),
        roles: Box::leak(roles.into_boxed_slice()),
    }))
}

/// One-time at load (validated spec in, `&'static` table out) — the leak IS
/// the lifetime strategy: every consumer keeps its `&'static` field types.
fn build(spec: FileSpec) -> RulesTable {
    // Sections FIRST. The exchange blocks' `enum` slots resolve the reserved
    // `fd_sections` / `arrl_sections` ids, which are derived from this list —
    // and they must be derived from the slice rather than fetched through the
    // public accessors, because `build` runs inside `TABLE.get_or_init` and
    // those accessors would re-enter it.
    let sections_static: &'static [Section] = Box::leak(
        spec.sections
            .into_iter()
            .map(|s| Section {
                code: leak_str(s.code),
                name: leak_str(s.name),
                division: leak_str(s.division),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    let reserved = Reserved {
        arrl_sections: Box::leak(Box::new(derive_arrl_sections(sections_static))),
        fd_sections: Box::leak(Box::new(derive_fd_sections(sections_static))),
    };
    let rulesets: Vec<&'static FdRuleset> = spec
        .rulesets
        .into_iter()
        .map(|r| {
            let points = ModePoints {
                ph: r.scoring.points_by_mode_class["PH"],
                cw: r.scoring.points_by_mode_class["CW"],
                dig: r.scoring.points_by_mode_class["DIG"],
            };
            let power_tiers: &'static [u32] = Box::leak(r.scoring.power_tiers.into_boxed_slice());
            // The file's `model` string names a POST-multiplier profile; both
            // profiles also carry the claimed bonus menu, which is what the two
            // old `ScoringModel` arms each did implicitly by having the callers
            // add `bonus_points` afterwards.
            let post: &'static [PostMultiplier] = Box::leak(
                vec![
                    match r.scoring.model.as_str() {
                        "powered_multiplier" => PostMultiplier::PowerTier { tiers: power_tiers },
                        // Validated to be one of the two above.
                        _ => PostMultiplier::Objectives {
                            at_submission: true,
                        },
                    },
                    PostMultiplier::Bonuses,
                ]
                .into_boxed_slice(),
            );
            let multipliers: &'static [MultiplierRule] = Box::leak(
                r.scoring
                    .multipliers
                    .into_iter()
                    .map(|m| MultiplierRule {
                        id: leak_str(m.id),
                        source: match m.source {
                            MultSourceSpec::Field { key, domain } => MultSource::Field {
                                key: leak_str(key),
                                // "" in the file means "the whole value,
                                // whatever arm it matched" — the same statement
                                // `None` makes in the type.
                                domain: (!domain.is_empty()).then(|| leak_str(domain) as &str),
                            },
                            MultSourceSpec::DxccEntity => MultSource::DxccEntity,
                            MultSourceSpec::Prefix => MultSource::Prefix,
                        },
                        scope: match m.scope {
                            MultScopeSpec::PerLog => MultScope::PerLog,
                            MultScopeSpec::PerBand => MultScope::PerBand,
                            MultScopeSpec::PerMode => MultScope::PerMode,
                            MultScopeSpec::PerBandMode => MultScope::PerBandMode,
                        },
                        excluding: leak_keys(m.excluding),
                        roles: leak_keys(m.roles),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let scoring = crate::contest::Scoring {
                qso_points: PointsRule::ByModeClass(points),
                multipliers,
                post,
            };
            let mut overrides: Vec<(u16, EventWindow)> = r
                .window
                .overrides
                .iter()
                .map(|(y, o)| {
                    (
                        y.parse::<u16>().expect("validated"),
                        EventWindow {
                            start_unix: o.start_unix,
                            end_unix: o.end_unix,
                        },
                    )
                })
                .collect();
            overrides.sort_unstable_by_key(|(y, _)| *y);
            // Domains FIRST: the exchange's `enum` slots resolve against them,
            // and a reserved id resolves to the derived domain instead.
            let domains_built: &'static [&'static crate::contest::Domain] = Box::leak(
                r.domains
                    .into_iter()
                    .map(|d| {
                        let values: Vec<(&'static str, &'static str)> = d
                            .values
                            .into_iter()
                            .map(|v| (leak_str(v.code) as &'static str, leak_str(v.label) as _))
                            .collect();
                        &*Box::leak(Box::new(crate::contest::Domain {
                            id: leak_str(d.id),
                            adif: crate::contest::AdifTags {
                                // "" in the file means "no standard column this
                                // direction" — it becomes None here, which is
                                // the same statement in the type.
                                rcvd: leak_opt_tag(d.adif.rcvd),
                                sent: leak_opt_tag(d.adif.sent),
                            },
                            values: Box::leak(values.into_boxed_slice()),
                        }))
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let exchange_built = build_exchange(r.exchange, domains_built, &reserved);
            &*Box::leak(Box::new(FdRuleset {
                event: event_of(&r.event).expect("validated"),
                rules_year: r.rules_year,
                contest_id: leak_str(r.contest_id),
                scoring,
                bonuses: leak_bonuses(r.bonuses),
                domains: domains_built,
                exchange: exchange_built,
                dupe_rule: DupeRule {
                    by_call: r.dupe.by_call,
                    by_band: r.dupe.by_band,
                    by_mode_class: r.dupe.by_mode_class,
                    by_fields: leak_keys(r.dupe.by_fields),
                    by_sent_fields: leak_keys(r.dupe.by_sent_fields),
                },
                tempo_fd: r.tempo_fd,
                banned_modes: Box::leak(
                    r.banned_modes
                        .into_iter()
                        .map(leak_str)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                objectives: leak_bonuses(r.objectives),
                assistance: AssistancePolicy {
                    spotting_allowed: r.assistance.spotting_allowed,
                    cluster_allowed: r.assistance.cluster_allowed,
                    assistance_note_key: leak_str(r.assistance.assistance_note_key),
                },
                enforcement: leak_str(r.enforcement),
                window: WindowRule {
                    month: r.window.month,
                    weekend: match r.window.weekend.as_str() {
                        "nth_full" => WeekendRule::NthFull(r.window.n),
                        _ => WeekendRule::LastFull,
                    },
                    start_hour_utc: r.window.start_hour_utc,
                    duration_hours: r.window.duration_hours,
                    overrides: Box::leak(overrides.into_boxed_slice()),
                },
            }))
        })
        .collect();
    RulesTable {
        generated: leak_str(spec.generated),
        rulesets: Box::leak(rulesets.into_boxed_slice()),
        sections: sections_static,
    }
}

// ---- Algorithmic event dates: the weekday-math interpreter (spec §2.3) -----

const SATURDAY: i64 = 6; // 0 = Sunday … 6 = Saturday

/// Saturdays of `month` whose Saturday+Sunday both fall within `[1, last_day]`.
fn full_weekend_saturdays(year: i64, month: u32, last_day: u32) -> Vec<u32> {
    (1..=last_day)
        .filter(|&d| d < last_day && weekday(days_from_civil(year, month, d)) == SATURDAY)
        .collect()
}

fn window(year: i64, month: u32, sat_day: u32, start_hour: u64, dur_secs: u64) -> EventWindow {
    let start = days_from_civil(year, month, sat_day) as u64 * 86_400 + start_hour * 3600;
    EventWindow {
        start_unix: start,
        end_unix: start + dur_secs,
    }
}

/// Weekday (0 = Sunday … 6 = Saturday) of a day count since the Unix epoch
/// (1970-01-01 was a Thursday).
fn weekday(days: i64) -> i64 {
    (days.rem_euclid(7) + 4) % 7
}

/// Days since 1970-01-01 for a civil UTC date (Howard Hinnant's algorithm;
/// mirrors `fieldday::unix_from_ymdhms` — no date crate needed).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i64, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

/// Civil UTC year of a Unix timestamp (Hinnant's `civil_from_days`, year part).
fn civil_year_of_unix(unix: u64) -> u16 {
    let z = (unix / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contest::ContestSession;
    use crate::fieldday::FieldDayLog;

    /// The RTTY parser accepted `valid_section(t) || t == "MX" || t == "DX"`
    /// (rtty/seq.rs at 82eb3112). Batch 0 replaces that inline test with a domain
    /// membership test, so the domain must be EXACTLY that set — 83 sections plus
    /// the two literals a DX station sends — or a legal contact stops being loggable.
    #[test]
    fn the_fd_section_domain_is_the_83_sections_plus_mx_and_dx() {
        let d = fd_sections_domain();
        assert_eq!(d.id, "fd_sections");
        assert_eq!(d.values.len(), 85, "83 ARRL/RAC sections + MX + DX");
        for s in sections() {
            assert!(
                d.contains(s.code),
                "section {} missing from the domain",
                s.code
            );
        }
        assert!(d.contains("MX"));
        assert!(d.contains("DX"));
        // Same normalisation as valid_section, both directions.
        assert!(d.contains(" wi "));
        assert!(!d.contains("ZZ"));
        assert!(
            !valid_section("MX"),
            "MX is NOT a section — it is an FD extension"
        );
        // ⭐ **Both directions, and the sent side is a SOURCE CLAIM that was checked.**
        // Through batch 5 the sent half shipped absent because a grep of this tree
        // found `MY_ARRL_SECT` nowhere — but an absence from our code is not an
        // absence from ADIF. Batch 6 read the document: ADIF Specification V 3.1.7,
        // `https://adif.org/317/ADIF_317.htm`, "Released ADIF Version 3.1.7, updated
        // 2026-03-22", fetched and searched in full 2026-09-09. `MY_ARRL_SECT` is a
        // QSO field there — "the logging station's ARRL section" — with `MY_COUNTY`,
        // `MY_SECTION`, `MY_CLASS` and two invented names as the negative controls
        // that returned nothing.
        assert_eq!(d.adif.rcvd, Some("ARRL_SECT"));
        assert_eq!(d.adif.sent, Some("MY_ARRL_SECT"));
    }

    /// Build a log from `(call, mode-class)` pairs — distinct calls so nothing
    /// dupes; class/section are constant (irrelevant to the point math).
    fn log_with(contacts: &[(&str, &str)]) -> FieldDayLog {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        for (i, (call, mode)) in contacts.iter().enumerate() {
            assert!(log.log_mode_at(call, "2A", "IL", mode, 0, 100 + i as u64));
        }
        log
    }

    #[test]
    fn sfd_pinned_score_10_qsos_x2_power_w1aw_bonus() {
        // 10 distinct QSOs: 4 phone (1 pt) + 3 CW + 3 DIG (2 pt) = 16 QSO pts.
        let log = log_with(&[
            ("PH1AA", "PH"),
            ("PH2AA", "PH"),
            ("PH3AA", "PH"),
            ("PH4AA", "PH"),
            ("CW1AA", "CW"),
            ("CW2AA", "CW"),
            ("CW3AA", "CW"),
            ("DG1AA", "DIG"),
            ("DG2AA", "DIG"),
            ("DG3AA", "DIG"),
        ]);
        assert_eq!(log.qso_count(), 10);
        let rs = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR);
        let (qso_pts, powered) = rs.scoring.qso_and_powered(log.score_rows(), 2);
        assert_eq!(qso_pts, 16, "4×1 + 6×2");
        assert_eq!(powered, 32, "16 QSO pts × ×2 power tier");
        let bonus = rs.bonus_points(&["w1aw-bulletin".to_string()]);
        assert_eq!(bonus, 100, "the W1AW-bulletin bonus");
        assert_eq!(powered + bonus, 132, "the score-board total");
    }

    #[test]
    fn wfd_scores_raw_qso_points_regardless_of_power() {
        // Winter FD is QSOs × (objectives+1); with no objective values in the
        // data the on-air total is RAW QSO points — no ARRL power multiplier,
        // even at the ×5 tier — and it flags multipliers-at-submission.
        let log = log_with(&[("K1ABC", "CW"), ("W1AW", "PH"), ("N0XYZ", "DIG")]);
        assert_eq!(log.qso_points(), 2 + 1 + 2);
        let rs = ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR);
        let (qso_pts, powered) = rs.scoring.qso_and_powered(log.score_rows(), 5);
        assert_eq!((qso_pts, powered), (5, 5), "raw points, power tier ignored");
        assert!(
            rs.scoring.post.contains(&PostMultiplier::Objectives {
                at_submission: true
            }),
            "{:?}",
            rs.scoring.post
        );
        assert_eq!(rs.scoring.power_tiers(), None, "no on-air power multiplier");
    }

    #[test]
    fn tempo_fd_is_wfd_only() {
        assert!(
            ruleset(FdEvent::WinterFd, 2026).tempo_fd,
            "WFD is a Tempo FD event"
        );
        assert!(!ruleset(FdEvent::ArrlFd, 2026).tempo_fd, "SFD is not");
    }

    #[test]
    fn contest_ids_match_the_event() {
        assert_eq!(ruleset(FdEvent::ArrlFd, 2026).contest_id, "ARRL-FIELD-DAY");
        assert_eq!(ruleset(FdEvent::WinterFd, 2026).contest_id, "WFD");
        // The ruleset id must never drift from the export id.
        for e in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            assert_eq!(ruleset(e, 2026).contest_id, e.contest_id());
        }
    }

    #[test]
    fn bonus_lookup_matches_the_old_table_semantics() {
        let rs = ruleset(FdEvent::ArrlFd, 2026);
        assert_eq!(rs.bonus("w1aw-bulletin"), Some(100));
        assert_eq!(rs.bonus("web-submission"), Some(50));
        assert_eq!(rs.bonus("not-a-bonus"), None, "unknown id scores nothing");
        assert_eq!(
            rs.bonus_points(&[
                "w1aw-bulletin".into(),
                "web-submission".into(),
                "junk".into()
            ]),
            150,
        );
        assert_eq!(rs.bonuses.len(), 15, "the full ARRL bonus menu");
    }

    #[test]
    fn arrl_sections_are_complete_and_unique() {
        use std::collections::HashSet;
        // 71 US ARRL sections + 12 RAC = the full ~85-section universe.
        assert_eq!(sections().len(), 83, "the ARRL/RAC section master list");
        // No duplicate codes (a copy-paste slip would double-count a section).
        let codes: HashSet<&str> = sections().iter().map(|s| s.code).collect();
        assert_eq!(codes.len(), sections().len(), "section codes are unique");
        // Codes are stored canonically (uppercase, non-empty) and every section
        // names a division so the board can group it.
        for s in sections() {
            assert!(!s.code.is_empty() && s.code == s.code.to_ascii_uppercase());
            assert!(!s.name.is_empty() && !s.division.is_empty(), "{}", s.code);
        }
        // Spot-check the tricky split-state + RAC entries the spec calls out.
        for code in [
            "EMA", "WMA", "STX", "NTX", "WTX", "SDG", "ORG", "SCV", "NNY", "GTA", "NT",
        ] {
            assert!(codes.contains(code), "missing section {code}");
        }
    }

    /// `key: 'value'` from a one-object-per-line TS table row (quote-splitting,
    /// the settings.rs RadioProfilePatch-guard tolerance — comments and
    /// non-matching lines simply miss).
    fn ts_str_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        let pat = format!("{key}: '");
        let i = line.find(&pat)?;
        let rest = &line[i + pat.len()..];
        let end = rest.find('\'')?;
        Some(&rest[..end])
    }

    /// `key: <digits>` from the same row shape.
    fn ts_num_field(line: &str, key: &str) -> Option<u32> {
        let pat = format!("{key}:");
        let i = line.find(&pat)?;
        let digits: String = line[i + pat.len()..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    }

    /// CROSS-LANGUAGE SYNC GUARD (the settings.rs RadioProfilePatch idiom): the
    /// UI hand-mirrors the section universe in ui/src/features/arrlSections.ts
    /// so the worked-sections board renders without a backend round-trip. The
    /// two lists had no guard — a section respelled, moved, added or dropped on
    /// one side would silently desynchronize the board from setup validation
    /// and scoring. This reads the TS source itself and compares codes, names,
    /// divisions, ORDER (the board layout contract — arrlSections.ts preserves
    /// the seed ordering by stated intent) and the division grouping, in both
    /// directions, naming the drifted entry. Compares against [`sections`] =
    /// the BUNDLED SEED (the TS mirror is the offline fallback universe).
    #[test]
    fn the_typescript_section_mirror_matches_arrl_sections_exactly() {
        let ts_src = include_str!("../../../ui/src/features/arrlSections.ts");
        // Section rows are one object per line: { code: 'DE', name: 'Delaware',
        // division: 'Atlantic' }. The per-division block headers carry a
        // `division:` but no `code:`, so keying on `code:` skips them.
        let ts: Vec<(&str, &str, &str)> = ts_src
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && !l.starts_with('*') && !l.starts_with("/*"))
            .filter_map(|l| {
                Some((
                    ts_str_field(l, "code")?,
                    ts_str_field(l, "name")?,
                    ts_str_field(l, "division")?,
                ))
            })
            .collect();

        // A parser that found nothing would pass every compare below it.
        assert!(
            ts.len() > 50,
            "parsed only {} section rows out of arrlSections.ts — the parser is \
             broken, not the mirror",
            ts.len()
        );

        // Both directions by code first, so a missing entry is NAMED rather
        // than surfacing as a count mismatch.
        let rust_codes: Vec<&str> = sections().iter().map(|s| s.code).collect();
        let ts_codes: Vec<&str> = ts.iter().map(|r| r.0).collect();
        let missing_in_ts: Vec<&&str> = rust_codes
            .iter()
            .filter(|c| !ts_codes.contains(c))
            .collect();
        assert!(
            missing_in_ts.is_empty(),
            "section(s) {missing_in_ts:?} exist in the seed's sections but NOT in \
             ui/src/features/arrlSections.ts — the board can never show them worked"
        );
        let missing_in_rust: Vec<&&str> = ts_codes
            .iter()
            .filter(|c| !rust_codes.contains(c))
            .collect();
        assert!(
            missing_in_rust.is_empty(),
            "section(s) {missing_in_rust:?} exist in ui/src/features/arrlSections.ts \
             but NOT in the seed's sections — the board shows a section \
             valid_section() rejects"
        );
        assert_eq!(ts.len(), 83, "83 = 71 US ARRL + 12 RAC, both sides");

        // Same entry at every index: order (the board layout), name and
        // division per code.
        for (i, (rust, ts_row)) in sections().iter().zip(&ts).enumerate() {
            assert_eq!(
                (rust.code, rust.name, rust.division),
                *ts_row,
                "section #{i} diverged between the seed's sections and \
                 arrlSections.ts (same-order is the board layout contract)"
            );
        }

        // Division grouping order (first occurrence) matches — redundant with
        // the per-index compare above, but it names a GROUPING drift as such.
        let mut rust_divs: Vec<&str> = Vec::new();
        for s in sections() {
            if !rust_divs.contains(&s.division) {
                rust_divs.push(s.division);
            }
        }
        let mut ts_divs: Vec<&str> = Vec::new();
        for (_, _, d) in &ts {
            if !ts_divs.contains(d) {
                ts_divs.push(d);
            }
        }
        assert_eq!(
            rust_divs, ts_divs,
            "the division block order drifted between the two lists"
        );
    }

    /// SAME GUARD for the hand-mirrored bonus menu: the seed's bonus menu vs
    /// the `FD_BONUSES` table in ui/src/components/FieldDayView.tsx. Ids and
    /// points only — the LABELS deliberately differ (the seed labels are the
    /// moved tuple-table strings; the TS labels are the invariant display
    /// strings the checklist renders), so comparing them would pin an
    /// intentional difference, not catch drift. A points or id drift is the
    /// one that mis-scores a claimed bonus.
    #[test]
    fn the_typescript_bonus_mirror_matches_fd_bonuses_exactly() {
        let bonuses = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR).bonuses;
        let ts_src = include_str!("../../../ui/src/components/FieldDayView.tsx");
        // Pull just the FD_BONUSES table body (the file declares other objects).
        let head = "export const FD_BONUSES";
        let start = ts_src
            .find(head)
            .expect("FieldDayView.tsx declares FD_BONUSES");
        // Slice from the initializer's `= [`, not the declaration (whose
        // `FdBonus[]` type annotation carries the file's first `]`).
        let body = &ts_src[start..];
        let open = body.find("= [").expect("the table has an initializer");
        let body = &body[open + 3..];
        let end = body.find(']').expect("the table is closed");
        let body = &body[..end];

        let ts: Vec<(&str, u32)> = body
            .lines()
            .filter_map(|l| Some((ts_str_field(l, "id")?, ts_num_field(l, "points")?)))
            .collect();
        assert!(
            ts.len() > 10,
            "parsed only {} bonus rows out of FieldDayView.tsx — the parser is \
             broken, not the mirror",
            ts.len()
        );

        let rust_ids: Vec<&str> = bonuses.iter().map(|b| b.id).collect();
        let ts_ids: Vec<&str> = ts.iter().map(|r| r.0).collect();
        let missing_in_ts: Vec<&&str> = rust_ids.iter().filter(|c| !ts_ids.contains(c)).collect();
        assert!(
            missing_in_ts.is_empty(),
            "bonus id(s) {missing_in_ts:?} exist in the seed's bonus menu but NOT in \
             FieldDayView.tsx — the checklist can never claim them"
        );
        let missing_in_rust: Vec<&&str> = ts_ids.iter().filter(|c| !rust_ids.contains(c)).collect();
        assert!(
            missing_in_rust.is_empty(),
            "bonus id(s) {missing_in_rust:?} exist in FieldDayView.tsx but NOT in \
             the seed's bonus menu — a claimed checkbox that scores nothing"
        );
        assert_eq!(ts.len(), 15, "the full ARRL bonus menu, both sides");
        for (i, (rust, ts_row)) in bonuses.iter().zip(&ts).enumerate() {
            assert_eq!(
                (rust.id, rust.points),
                *ts_row,
                "bonus #{i} diverged (id or points) between the seed's bonus menu \
                 and FieldDayView.tsx"
            );
        }
    }

    #[test]
    fn valid_section_accepts_known_case_insensitively_and_rejects_junk() {
        assert!(valid_section("WI"));
        assert!(valid_section("wi"), "case-insensitive");
        assert!(valid_section("  eMa "), "trims + case-insensitive");
        assert!(valid_section("ONS"), "a RAC section");
        assert!(!valid_section("ZZ"), "not a section");
        assert!(!valid_section(""), "empty is not a section");
        assert!(!valid_section("WISCONSIN"), "the name is not the code");
    }

    #[test]
    fn event_windows_are_algorithmic_and_dodge_the_feb_spill() {
        // 4th full weekend of June 2026 = the 27th (Sat) at 1800Z.
        assert_eq!(full_weekend_saturdays(2026, 6, 30), vec![6, 13, 20, 27]);
        let sfd = ruleset(FdEvent::ArrlFd, 2026).event_window(2026);
        assert_eq!(sfd.start_unix % 86_400, 18 * 3600, "1800Z start");
        assert_eq!(
            sfd.start_unix,
            days_from_civil(2026, 6, 27) as u64 * 86_400 + 18 * 3600,
            "June 27 2026"
        );
        assert_eq!(
            sfd.end_unix - sfd.start_unix,
            27 * 3600,
            "27-hour SFD period"
        );
        // Last FULL weekend of January 2026 = the 24th, NOT the 31st (whose
        // Sunday spills into February).
        assert_eq!(full_weekend_saturdays(2026, 1, 31), vec![3, 10, 17, 24]);
        let wfd = ruleset(FdEvent::WinterFd, 2026).event_window(2026);
        assert_eq!(wfd.start_unix % 86_400, 16 * 3600, "1600Z start");
        assert_eq!(
            wfd.start_unix,
            days_from_civil(2026, 1, 24) as u64 * 86_400 + 16 * 3600,
            "January 24 2026 — the Feb-spill correction"
        );
        // WFD is a 30-HOUR event (1600Z Sat → 21:59Z Sun); the old 24 h window
        // dropped the final six hours. Exclusive 2200Z Sunday end = 21:59 close.
        assert_eq!(
            wfd.end_unix - wfd.start_unix,
            30 * 3600,
            "30-hour WFD period"
        );
        assert_eq!(wfd.end_unix % 86_400, 22 * 3600, "2200Z Sunday end");
    }

    #[test]
    fn next_or_running_returns_the_running_window_then_rolls_the_year() {
        let wfd = ruleset(FdEvent::WinterFd, 2026);
        let w26 = wfd.event_window(2026);
        // Before the event: this year's window.
        assert_eq!(wfd.next_or_running(w26.start_unix - 86_400), w26);
        // INSIDE the final six hours (the slice the 24 h TS math dropped):
        // still the running window, not next January's.
        assert_eq!(wfd.next_or_running(w26.end_unix - 3600), w26);
        // After the end: next year's.
        assert_eq!(wfd.next_or_running(w26.end_unix), wfd.event_window(2027));
        // Same shape for SFD, whose gap to next year crosses the new year.
        let sfd = ruleset(FdEvent::ArrlFd, 2026);
        let s26 = sfd.event_window(2026);
        assert_eq!(sfd.next_or_running(s26.end_unix - 1), s26);
        assert_eq!(sfd.next_or_running(s26.end_unix), sfd.event_window(2027));
    }

    #[test]
    fn civil_year_of_unix_matches_known_dates() {
        // 2026-01-01 00:00:00 UTC and 2025-12-31 23:59:59 UTC straddle the year.
        let jan1_2026 = days_from_civil(2026, 1, 1) as u64 * 86_400;
        assert_eq!(civil_year_of_unix(jan1_2026), 2026);
        assert_eq!(civil_year_of_unix(jan1_2026 - 1), 2025);
        assert_eq!(civil_year_of_unix(0), 1970);
    }

    #[test]
    fn a_window_override_beats_the_algorithmic_rule() {
        // Build (leak) a throwaway table whose SFD carries a 2027 override —
        // never touches the global TABLE (D7: install itself is integration-
        // tested in its own process).
        let mut spec: serde_json::Value = serde_json::from_str(SEED).unwrap();
        spec["rulesets"][0]["window"]["overrides"]["2027"] =
            serde_json::json!({ "start_unix": 1_000_000, "end_unix": 2_000_000 });
        let t = build(parse_spec(&spec.to_string()).expect("override spec validates"));
        let sfd = t
            .rulesets
            .iter()
            .find(|r| r.event == FdEvent::ArrlFd)
            .unwrap();
        assert_eq!(
            sfd.event_window(2027),
            EventWindow {
                start_unix: 1_000_000,
                end_unix: 2_000_000
            },
            "the override wins for its year"
        );
        // Other years still come from the rule.
        assert_eq!(sfd.event_window(2026).start_unix % 86_400, 18 * 3600);
    }

    /// The structural validation, both directions: the seed passes (the
    /// positive control — a validator that rejects everything would "pass"
    /// every reject case below), and each named corruption is refused.
    #[test]
    fn validation_accepts_the_seed_and_rejects_each_corruption() {
        assert!(parse_spec(SEED).is_ok(), "the bundled seed must validate");

        let corrupt = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            f(&mut v);
            parse_spec(&v.to_string())
        };
        // A schema-1 file is a file from an OLDER build; schema 3 is one from a
        // NEWER build. Both are refused, and the message names both numbers.
        assert!(
            corrupt(&|v| v["schema"] = 1.into())
                .unwrap_err()
                .contains("schema 1"),
            "a schema-1 file is refused by name"
        );
        assert!(
            corrupt(&|v| v["schema"] = 3.into())
                .unwrap_err()
                .contains("schema 3"),
            "a file from a future build is refused by name"
        );
        assert!(
            corrupt(&|v| v["rulesets"][1]["event"] = "arrlfd".into())
                .unwrap_err()
                .contains("wfd"),
            "missing wfd event"
        );
        assert!(
            corrupt(&|v| v["rulesets"][0]["bonuses"][1]["id"] = "emergency-power".into())
                .unwrap_err()
                .contains("duplicate bonus id"),
        );
        assert!(
            corrupt(&|v| v["rulesets"][0]["scoring"]["power_tiers"] = serde_json::json!([]))
                .unwrap_err()
                .contains("power_tiers"),
        );
        assert!(corrupt(&|v| {
            v["sections"].as_array_mut().unwrap().pop();
        })
        .unwrap_err()
        .contains("82 sections"),);
        assert!(
            corrupt(&|v| v["rulesets"][0]["window"]["month"] = 13.into())
                .unwrap_err()
                .contains("month"),
        );
        assert!(
            corrupt(&|v| v["rulesets"][0]["enforcement"] = "block".into())
                .unwrap_err()
                .contains("enforcement"),
            "this build only warns"
        );
        assert!(corrupt(
            &|v| v["rulesets"][0]["scoring"]["points_by_mode_class"] = serde_json::json!({"PH": 1})
        )
        .unwrap_err()
        .contains("points_by_mode_class"),);
        assert!(validate("not json").is_err(), "garbage is refused");
    }

    /// §8(d): the schema number is the LOUD failure. A schema-1 file must be
    /// refused by a schema-2 build with a message that names both numbers, and
    /// the seed — now schema 2 — must load. The pair is the point: a validator
    /// that refused everything would "pass" the first assertion alone.
    #[test]
    fn schema_2_is_this_builds_number_and_schema_1_is_refused_by_name() {
        assert!(parse_spec(SEED).is_ok(), "the bundled schema-2 seed loads");
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["schema"] = 1.into();
        let e = parse_spec(&v.to_string()).unwrap_err();
        assert!(e.contains("schema 1"), "names what it got: {e}");
        assert!(e.contains("schema 2"), "names what it reads: {e}");
    }

    /// §8(d)'s inversion: a downloaded file may ADD contests but never REMOVE
    /// one the bundled seed carries — strictly stronger than the hardcoded
    /// `["arrlfd", "wfd"]` pair it replaces, because it grows with the seed
    /// instead of having to be re-edited alongside it.
    #[test]
    fn a_file_missing_a_seeded_ruleset_is_refused_and_the_same_file_with_it_loads() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        let dropped = v["rulesets"].as_array_mut().unwrap().pop().expect("wfd");
        let e = parse_spec(&v.to_string()).unwrap_err();
        assert!(e.contains("wfd"), "names the missing ruleset: {e}");
        // POSITIVE CONTROL: put it back and the very same file must load.
        v["rulesets"].as_array_mut().unwrap().push(dropped);
        assert!(
            parse_spec(&v.to_string()).is_ok(),
            "the control file must load"
        );
    }

    /// `seed_events` must read the SEED, not a hardcoded pair — otherwise the
    /// inversion above is the old rule wearing a new name.
    #[test]
    fn seed_events_are_read_from_the_bundled_seed() {
        assert_eq!(seed_events(), ["arrlfd", "wfd"]);
    }

    /// §11.1: the seed gains CONTENT, not behaviour. `scoring` absorbs the two
    /// sibling keys that were always part of "today's model" (Ruling B1-B), and
    /// the numbers inside are the same numbers they were when they were flat.
    #[test]
    fn the_scoring_block_carries_todays_model_unchanged() {
        const FD_POINTS: ModePoints = ModePoints {
            ph: 1,
            cw: 2,
            dig: 2,
        };
        let arrl = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR);
        assert_eq!(
            arrl.scoring,
            crate::contest::Scoring {
                qso_points: PointsRule::ByModeClass(FD_POINTS),
                multipliers: &[],
                post: &[
                    PostMultiplier::PowerTier { tiers: &[1, 2, 5] },
                    PostMultiplier::Bonuses,
                ],
            }
        );
        let wfd = ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR);
        assert_eq!(
            wfd.scoring,
            crate::contest::Scoring {
                qso_points: PointsRule::ByModeClass(FD_POINTS),
                multipliers: &[],
                post: &[
                    PostMultiplier::Objectives {
                        at_submission: true
                    },
                    PostMultiplier::Bonuses,
                ],
            }
        );
    }

    /// §11.2: `MultiplierRule` lands with **no shipped ruleset using one**, and
    /// that is a decision the seed has to state rather than a gap. Neither Field
    /// Day event has a multiplier — the ARRL FD score is QSO points × power plus
    /// bonuses, and `FieldDayLog::sections()` is a display count — so both write
    /// `"multipliers": []`.
    ///
    /// The POSITIVE CONTROL for this pair is in `tests/fd_rules_install.rs`: an
    /// installed file that DOES declare multipliers reaches the built ruleset
    /// intact, which is what makes the two empties above a decision rather than a
    /// loader that drops the block on the floor.
    #[test]
    fn no_shipped_ruleset_declares_a_multiplier() {
        for e in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            let s = ruleset(e, CURRENT_RULES_YEAR).scoring;
            assert!(s.multipliers.is_empty(), "{e:?}: {:?}", s.multipliers);
        }
    }

    /// …and the key is REQUIRED, so a rules file that simply forgot a contest's
    /// multipliers is refused rather than scoring its log at a fraction of the
    /// real total (§8c: absent must be loud, `[]` must be a decision).
    #[test]
    fn a_scoring_block_with_no_multipliers_key_is_refused() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0]["scoring"]
            .as_object_mut()
            .unwrap()
            .remove("multipliers");
        let e = parse_spec(&v.to_string()).unwrap_err();
        assert!(e.contains("multipliers"), "{e}");
        // POSITIVE CONTROL: the same file with the key present loads.
        assert!(parse_spec(SEED).is_ok());
    }

    /// §2.5's multiplier checks, each with the control that the same file
    /// without that one mutation loads. A multiplier counts what the OTHER
    /// station sent, so its slot must be one some role RECEIVES; its domain must
    /// resolve; and a `roles` filter must name a role that exists, or the
    /// universe it selects is empty and the contest silently scores no
    /// multipliers at all.
    #[test]
    fn the_multiplier_checks_refuse_a_rule_the_exchange_cannot_serve() {
        let with = |m: serde_json::Value| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            v["rulesets"][0]["scoring"]["multipliers"] = serde_json::json!([m]);
            parse_spec(&v.to_string())
        };
        let good = serde_json::json!({
            "id": "section",
            "source": { "type": "field", "key": "SECTION", "domain": "" },
            "scope": "per_log",
            "excluding": [],
            "roles": [],
        });
        // POSITIVE CONTROL first: a well-formed rule against Field Day's own
        // exchange loads, so every refusal below is about its one mutation.
        assert!(with(good.clone()).is_ok(), "{:?}", with(good.clone()));

        let mutate = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut m = good.clone();
            f(&mut m);
            with(m).unwrap_err()
        };
        assert!(
            mutate(&|m| m["id"] = "".into()).contains("empty multiplier id"),
            "{}",
            mutate(&|m| m["id"] = "".into())
        );
        assert!(
            mutate(&|m| m["source"]["key"] = "CLASS_NOT_RECEIVED".into())
                .contains("which no role receives")
        );
        assert!(mutate(&|m| m["source"]["domain"] = "counties".into())
            .contains("names unknown domain \"counties\""));
        assert!(mutate(&|m| m["roles"] = serde_json::json!(["in_state"]))
            .contains("names undeclared role \"in_state\""));
        // The empty role id IS Field Day's only role, so naming it must LOAD —
        // otherwise the check above would be refusing every legal filter.
        assert!(with(serde_json::json!({
            "id": "section",
            "source": { "type": "field", "key": "SECTION", "domain": "" },
            "scope": "per_log",
            "excluding": [],
            "roles": [""],
        }))
        .is_ok());
        // Two rules may not share an id: one board, two universes, and nothing
        // downstream able to say which is which.
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0]["scoring"]["multipliers"] = serde_json::json!([good, good]);
        assert!(parse_spec(&v.to_string())
            .unwrap_err()
            .contains("duplicate multiplier id"));
    }

    /// The scope vocabulary is CLOSED. An unknown scope must be refused by name
    /// rather than falling back to a per-log count — the difference between
    /// "this file is wrong" and a contest quietly scoring one multiplier where
    /// it should have scored one per band.
    #[test]
    fn an_unknown_multiplier_scope_is_refused_by_name() {
        let scoped = |scope: &str| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            v["rulesets"][0]["scoring"]["multipliers"] = serde_json::json!([{
                "id": "section",
                "source": { "type": "field", "key": "SECTION", "domain": "" },
                "scope": scope,
                "excluding": [],
                "roles": [],
            }]);
            parse_spec(&v.to_string())
        };
        let e = scoped("per_hour").unwrap_err();
        assert!(e.contains("per_hour"), "{e}");
        // POSITIVE CONTROL: all four real scopes load through the same path.
        for s in ["per_log", "per_band", "per_mode", "per_band_mode"] {
            assert!(scoped(s).is_ok(), "{s} must load");
        }
    }

    /// The additive rule the module header rests on, MEASURED rather than
    /// reasoned: this parser IGNORES a key it does not know, so a key added to
    /// schema 2 by a later batch does not stop an already-shipped build from
    /// receiving rules updates. (Nothing here declares
    /// `#[serde(deny_unknown_fields)]`, and this test is what would notice if
    /// something did.) A key RENAMED or RESHAPED is a different act with a
    /// different cost — the schema number exists for that.
    #[test]
    fn an_unknown_key_is_ignored_so_schema_2_can_grow_additively() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0]["scoring"]["a_key_from_a_later_batch"] = serde_json::json!({"x": [1]});
        v["rulesets"][0]["a_ruleset_key_from_a_later_batch"] = 7.into();
        assert!(parse_spec(&v.to_string()).is_ok());
        // NEGATIVE CONTROL: the same insertion where a key is REQUIRED is not
        // forgiving at all — an unknown key is ignored, a missing one is not.
        v["rulesets"][0]["scoring"]
            .as_object_mut()
            .unwrap()
            .remove("power_tiers");
        assert!(parse_spec(&v.to_string())
            .unwrap_err()
            .contains("power_tiers"));
    }

    /// §8(c): a missing block must be a LOUD failure. serde defaults a missing
    /// `Option` field silently even with no `#[serde(default)]` attribute, which
    /// is exactly why nothing in the new blocks is optional.
    #[test]
    fn a_ruleset_with_no_scoring_block_is_refused() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0].as_object_mut().unwrap().remove("scoring");
        let e = parse_spec(&v.to_string()).unwrap_err();
        assert!(e.contains("scoring"), "{e}");
        // POSITIVE CONTROL: the same file with the block present loads.
        assert!(parse_spec(SEED).is_ok());
    }

    /// The three checks that used to sit on flat sibling keys must still fire,
    /// now that they read `scoring.*` — each with the control that the seed's
    /// own value in that slot loads.
    #[test]
    fn the_moved_scoring_checks_still_fire_on_their_new_path() {
        let corrupt = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            f(&mut v);
            parse_spec(&v.to_string())
        };
        assert!(
            corrupt(&|v| v["rulesets"][0]["scoring"]["model"] = "made_up".into())
                .unwrap_err()
                .contains("unknown scoring model"),
        );
        assert!(corrupt(&|v| {
            v["rulesets"][0]["scoring"]["points_by_mode_class"] = serde_json::json!({"PH": 1});
        })
        .unwrap_err()
        .contains("points_by_mode_class"),);
        assert!(corrupt(
            &|v| v["rulesets"][0]["scoring"]["power_tiers"] = serde_json::json!([5, 2, 1])
        )
        .unwrap_err()
        .contains("power_tiers"),);
        assert!(
            corrupt(&|v| v["rulesets"][0]["scoring"]["power_tiers"] = serde_json::json!([]))
                .unwrap_err()
                .contains("power_tiers"),
        );
        assert!(
            parse_spec(SEED).is_ok(),
            "control: the seed's own values load"
        );
    }

    /// The dupe key is now DATA rather than the `DUPE_CALL_BAND_MODE` const,
    /// and it must still be today's key for both events: a station counts once
    /// per (call, band, mode class).
    #[test]
    fn the_dupe_block_reaches_the_ruleset_and_is_todays_key() {
        for e in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            let d = ruleset(e, CURRENT_RULES_YEAR).dupe_rule;
            assert!(
                d.by_call && d.by_band && d.by_mode_class,
                "{e:?}: (call, band, mode class)"
            );
        }
    }

    /// A dupe rule that does not key on the callsign is not a dupe rule — every
    /// contest in the researched set keys on it, and a file saying otherwise is
    /// far more likely to be a mistake than a new contest shape. Refused by
    /// name, with the positive control that the same file with `by_call` true
    /// loads.
    #[test]
    fn a_dupe_rule_that_ignores_the_callsign_is_refused() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0]["dupe"]["by_call"] = false.into();
        assert!(parse_spec(&v.to_string()).unwrap_err().contains("by_call"));
        v["rulesets"][0]["dupe"]["by_call"] = true.into();
        assert!(
            parse_spec(&v.to_string()).is_ok(),
            "control: by_call true loads"
        );
    }

    /// §8(c) again, on this block: absent is loud, never a silent default.
    #[test]
    fn a_ruleset_with_no_dupe_block_is_refused() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0].as_object_mut().unwrap().remove("dupe");
        assert!(parse_spec(&v.to_string()).unwrap_err().contains("dupe"));
        assert!(parse_spec(SEED).is_ok(), "control");
    }

    /// Ruling B0-C: the section codes are DERIVED, never duplicated into the
    /// file. A rules file that declared a reserved id would create a second,
    /// drifting copy of a list whose third attribute (`division`) a `Domain`
    /// cannot even hold.
    #[test]
    fn a_file_may_not_redefine_a_reserved_domain_id() {
        for id in ["arrl_sections", "fd_sections"] {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            v["rulesets"][0]["domains"] = serde_json::json!([{
                "id": id,
                "adif": { "rcvd": "ARRL_SECT", "sent": "" },
                "values": [{ "code": "WI", "label": "Wisconsin" }]
            }]);
            let e = parse_spec(&v.to_string()).unwrap_err();
            assert!(e.contains(id) && e.contains("reserved"), "{id}: {e}");
        }
        // POSITIVE CONTROL: a NON-reserved domain with the same shape loads, so
        // the refusal is about the id and not about domains being rejected
        // wholesale.
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0]["domains"] = serde_json::json!([{
            "id": "us_ca",
            "adif": { "rcvd": "STATE", "sent": "" },
            "values": [{ "code": "WI", "label": "Wisconsin" }]
        }]);
        assert!(
            parse_spec(&v.to_string()).is_ok(),
            "a real domain must load"
        );
    }

    /// §8(d): the 83-section assertion, expressed on the domain (Ruling B0-C).
    #[test]
    fn the_arrl_sections_domain_is_exactly_the_83() {
        let d = arrl_sections_domain();
        assert_eq!(d.id, "arrl_sections");
        assert_eq!(d.values.len(), 83);
        assert_eq!(d.adif.rcvd, Some("ARRL_SECT"));
        // Verified against ADIF 3.1.7 in batch 6 — see
        // `the_fd_section_domain_is_the_83_sections_plus_mx_and_dx` for the
        // citation and the negative controls.
        assert_eq!(d.adif.sent, Some("MY_ARRL_SECT"));
        assert!(!d.contains("MX"), "MX is an FD extension, not a section");
        assert!(!d.contains("DX"), "…and so is DX");
        assert!(d.contains("WI") && d.contains(" wi "));
        assert_eq!(
            fd_sections_domain().values.len(),
            85,
            "…which fd_sections carries, being the 83 plus MX and DX"
        );
    }

    /// The per-domain structural rules, each with the control that the same
    /// file with that one property corrected loads.
    #[test]
    fn a_malformed_domain_is_refused_by_the_property_that_is_wrong() {
        let with = |doms: serde_json::Value| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            v["rulesets"][0]["domains"] = doms;
            parse_spec(&v.to_string())
        };
        let good = serde_json::json!([{
            "id": "us_ca", "adif": { "rcvd": "STATE", "sent": "" },
            "values": [{ "code": "WI", "label": "Wisconsin" }]
        }]);
        assert!(with(good.clone()).is_ok(), "control: the good shape loads");

        // Duplicate id within one ruleset.
        assert!(with(serde_json::json!([
            { "id": "us_ca", "adif": { "rcvd": "STATE", "sent": "" },
              "values": [{ "code": "WI", "label": "Wisconsin" }] },
            { "id": "us_ca", "adif": { "rcvd": "STATE", "sent": "" },
              "values": [{ "code": "IL", "label": "Illinois" }] }
        ]))
        .unwrap_err()
        .contains("duplicate domain id"));

        // Empty values.
        assert!(with(serde_json::json!([
            { "id": "us_ca", "adif": { "rcvd": "STATE", "sent": "" }, "values": [] }
        ]))
        .unwrap_err()
        .contains("no values"));

        // A code that is not already uppercase.
        assert!(with(serde_json::json!([
            { "id": "us_ca", "adif": { "rcvd": "STATE", "sent": "" },
              "values": [{ "code": "wi", "label": "Wisconsin" }] }
        ]))
        .unwrap_err()
        .contains("not uppercase"));

        // A malformed ADIF tag name.
        assert!(with(serde_json::json!([
            { "id": "us_ca", "adif": { "rcvd": "state name", "sent": "" },
              "values": [{ "code": "WI", "label": "Wisconsin" }] }
        ]))
        .unwrap_err()
        .contains("adif"));

        // A malformed domain id.
        assert!(with(serde_json::json!([
            { "id": "US-CA", "adif": { "rcvd": "STATE", "sent": "" },
              "values": [{ "code": "WI", "label": "Wisconsin" }] }
        ]))
        .unwrap_err()
        .contains("domain id"));
    }

    /// §8(c): absent is loud. The seed writes `"domains": []` EXPLICITLY —
    /// Field Day needs no file-declared domain, and saying so in the file is
    /// the point, because a missing key must be a load failure rather than an
    /// empty list nobody chose.
    #[test]
    fn a_ruleset_with_no_domains_key_is_refused_but_an_empty_list_loads() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0].as_object_mut().unwrap().remove("domains");
        assert!(parse_spec(&v.to_string()).unwrap_err().contains("domains"));
        assert!(parse_spec(SEED).is_ok(), "control: the seed's [] loads");
        for e in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            assert!(ruleset(e, CURRENT_RULES_YEAR).domains.is_empty());
        }
    }

    /// Every §2.5 structural rule this batch lands, each paired with the
    /// positive control that the same file with that one property corrected
    /// loads. Without the control a refusal proves only that SOMETHING was
    /// wrong, not that the named rule is what caught it.
    #[test]
    fn the_exchange_structural_rules_each_fire_with_their_control() {
        let at = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            f(&mut v);
            parse_spec(&v.to_string())
        };
        assert!(
            parse_spec(SEED).is_ok(),
            "control: the seed's own block loads"
        );

        // A role naming a slot the file does not declare.
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["roles"][0]["receives"] =
                serde_json::json!(["CLASS", "NOPE"]);
        })
        .unwrap_err()
        .contains("NOPE"));

        // An `enum` slot naming a domain nothing resolves.
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["fields"][1]["kind"] =
                serde_json::json!({ "type": "enum", "domain": "no_such_domain" });
        })
        .unwrap_err()
        .contains("no_such_domain"));

        // §2.6: per-band serials are refused BY NAME rather than silently
        // scored as per-contest.
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["fields"][0]["kind"] =
                serde_json::json!({ "type": "serial", "scope": "per_band" });
        })
        .unwrap_err()
        .contains("per_band"));
        // …and the control: per_contest is accepted.
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["fields"][0]["kind"] =
                serde_json::json!({ "type": "serial", "scope": "per_contest" });
        })
        .is_ok());

        // `always` is only legal as the ONLY role — a second role after it
        // could never be reached.
        assert!(at(&|v| {
            let extra = serde_json::json!({
                "id": "second", "selector": { "type": "always" },
                "sends": ["CLASS"], "receives": ["CLASS"], "constant_sent": []
            });
            v["rulesets"][0]["exchange"]["roles"]
                .as_array_mut()
                .unwrap()
                .push(extra);
        })
        .unwrap_err()
        .contains("always"));

        // §2.5: five received fields is the layout budget at the 1024 px
        // supported floor. A limit the layout cannot honour is not a limit.
        let six = serde_json::json!(["A", "B", "C", "D", "E", "F"]);
        let five = serde_json::json!(["A", "B", "C", "D", "E"]);
        let slots = |n: usize| {
            let mut out = vec![];
            for k in ["A", "B", "C", "D", "E", "F"].iter().take(n) {
                out.push(serde_json::json!({
                    "key": k, "label": "", "required": false,
                    "adif": { "rcvd": "", "sent": "" },
                    "kind": { "type": "text", "max_len": 8 }
                }));
            }
            serde_json::Value::Array(out)
        };
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["fields"] = slots(6);
            v["rulesets"][0]["exchange"]["roles"][0]["sends"] = six.clone();
            v["rulesets"][0]["exchange"]["roles"][0]["receives"] = six.clone();
        })
        .unwrap_err()
        .contains("receives"));
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["fields"] = slots(5);
            v["rulesets"][0]["exchange"]["roles"][0]["sends"] = five.clone();
            v["rulesets"][0]["exchange"]["roles"][0]["receives"] = five.clone();
        })
        .is_ok());

        // `constant_sent` must be a subset of `sends`: it constrains something
        // I actually transmit.
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["roles"][0]["constant_sent"] =
                serde_json::json!(["SECTION"]);
            v["rulesets"][0]["exchange"]["roles"][0]["sends"] = serde_json::json!(["CLASS"]);
        })
        .unwrap_err()
        .contains("constant_sent"));

        // §2.1.1: `adif.sent` must be WRITTEN. An absent key is the
        // direction-blind shape that rule exists to kill.
        assert!(at(&|v| {
            v["rulesets"][0]["exchange"]["fields"][0]["adif"]
                .as_object_mut()
                .unwrap()
                .remove("sent");
        })
        .unwrap_err()
        .contains("sent"));
    }

    /// §8(c) on the last of the four blocks.
    #[test]
    fn a_ruleset_with_no_exchange_block_is_refused() {
        let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
        v["rulesets"][0].as_object_mut().unwrap().remove("exchange");
        assert!(parse_spec(&v.to_string()).unwrap_err().contains("exchange"));
        assert!(parse_spec(SEED).is_ok(), "control");
    }

    /// The seed's exchange block SPELLS OUT today's behaviour (§11.1). Nothing
    /// reads it yet, so this cross-check is the only thing that stops it
    /// drifting from the shipped `contest::field_day()` before the batch that
    /// wires it through — including the per-event class letters, which are the
    /// one place the two events genuinely differ.
    #[test]
    fn the_seeded_exchange_block_matches_the_shipped_field_day_exchange() {
        for e in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            let shipped = crate::contest::field_day(e);
            let loaded = ruleset(e, CURRENT_RULES_YEAR).exchange;
            assert_eq!(loaded.name, shipped.name, "{e:?}");
            assert_eq!(loaded.fields.len(), shipped.fields.len(), "{e:?}");
            for (a, b) in loaded.fields.iter().zip(shipped.fields) {
                assert_eq!(a.key, b.key, "{e:?}");
                assert_eq!(a.label, b.label, "{e:?} {}", a.key);
                assert_eq!(a.required, b.required, "{e:?} {}", a.key);
                assert_eq!(a.kind, b.kind, "{e:?} {}", a.key);
                assert_eq!(a.adif, b.adif, "{e:?} {}", a.key);
            }
            assert_eq!(loaded.roles.len(), 1, "{e:?}");
            assert_eq!(loaded.roles[0].sends, shipped.roles[0].sends, "{e:?}");
            assert_eq!(loaded.roles[0].receives, shipped.roles[0].receives, "{e:?}");
        }
        // The two events must NOT be the same block: the class letters differ.
        let arrl = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR).exchange;
        let wfd = ruleset(FdEvent::WinterFd, CURRENT_RULES_YEAR).exchange;
        assert_ne!(
            arrl.field("CLASS").unwrap().kind,
            wfd.field("CLASS").unwrap().kind,
            "the seed must carry each sponsor's own class letters"
        );
    }

    #[test]
    fn assistance_policy_ships_dormant_until_provenance_is_verified() {
        // Both events: everything allowed, no note key — the advisory chip
        // (item ④) has nothing to warn about until the sponsor's rules text is
        // read and quoted in the seed's `_provenance` (see that field).
        for e in [FdEvent::ArrlFd, FdEvent::WinterFd] {
            let a = ruleset(e, CURRENT_RULES_YEAR).assistance;
            assert!(a.spotting_allowed && a.cluster_allowed, "{e:?} dormant");
            assert_eq!(a.assistance_note_key, "");
            assert_eq!(ruleset(e, CURRENT_RULES_YEAR).enforcement, "warn");
        }
    }

    #[test]
    fn wfd_bans_wsjt_modes_but_never_rtty_or_sstv() {
        let wfd = ruleset(FdEvent::WinterFd, 2026);
        // The whole WSJT suite is out at WFD 2026…
        for m in ["FT8", "FT4", "FST4", "JT65", "Q65", "MSK144", "WSPR"] {
            assert!(wfd.mode_banned(m), "{m} is banned at WFD");
        }
        assert!(wfd.mode_banned(" ft8 "), "case-insensitive + trimmed");
        // …while RTTY and SSTV are explicitly legal Digital, and the classic
        // mode classes are untouched. A legacy row with no recorded actual
        // mode is never flagged.
        for m in ["RTTY", "SSTV", "CW", "SSB", ""] {
            assert!(!wfd.mode_banned(m), "{m:?} is not banned at WFD");
        }
        // ARRL FD bans nothing.
        let sfd = ruleset(FdEvent::ArrlFd, 2026);
        assert!(sfd.banned_modes.is_empty());
        assert!(!sfd.mode_banned("FT8"));
    }
}
