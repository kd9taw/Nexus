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
    /// Hours from 0000Z on the ANCHOR SATURDAY, `0..=47` — so 41 is 1700Z on the
    /// Sunday, which is when the Tennessee QSO Party starts. Both Field Day events
    /// are under 24 and read exactly as the name suggests.
    pub start_hour_utc: u64,
    pub duration_hours: u64,
    pub overrides: &'static [(u16, EventWindow)],
}

/// The complete rules for one Field Day event in one year — the single source
/// every surface reads.
#[derive(Debug)]
pub struct FdRuleset {
    /// The rules-file event id — `"arrlfd"`, `"wfd"`, `"tnqp"`, `"ohqp"`, `"cqp"`,
    /// `"txqp"`.
    ///
    /// ⭐ **An id, not an [`FdEvent`].** The table carries contests that enum has no
    /// arm for, and it must be able to: giving each new contest an arm would put
    /// every match on `FdEvent` in the tree (184 sites at `aaef8da2`) on the critical
    /// path of adding a row to a data file. [`FdEvent::code`] is the one place the
    /// two vocabularies meet, and [`ruleset`] is still typed on `FdEvent` so the two
    /// Field Day lookups stay infallible.
    pub event: &'static str,
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
    /// ⭐ **An i18n key naming what this event's computed score LEAVES OUT**, or `""`
    /// when the score is complete.
    ///
    /// The state QSO parties need it and Field Day does not. TNQP and TXQP both award
    /// bonus points COMPUTED FROM THE LOG (100 per K4TCG QSO, 500 per Tennessee county
    /// with ≥10 QSOs; 500 per Texas mobile worked in five counties, 1000 per county a
    /// Texas mobile covers with ≥5 QSOs), and TNQP a multiplier gated on a per-county
    /// QSO count. [`PostMultiplier::Bonuses`] is a menu the operator TICKS and
    /// [`MultiplierRule`] cannot express a count threshold, so neither term is in the
    /// total this build shows. **An operator must not read a claimed score off a
    /// number that silently omits their bonuses** — so the omission is data the score
    /// surface renders, not a sentence in a provenance block nobody sees.
    ///
    /// [`PostMultiplier::Bonuses`]: crate::contest::PostMultiplier::Bonuses
    /// [`MultiplierRule`]: crate::contest::MultiplierRule
    pub score_note_key: &'static str,
    /// The algorithmic event-window rule, as data (spec §2.3 — the parameters
    /// are never hand-edited in code anymore; they ride the rules file).
    pub window: WindowRule,
    /// ⭐ **Does this sponsor's Cabrillo QSO template carry the trailing transmitter-id
    /// column?**
    ///
    /// A per-contest fact about the sponsor's own published template, so it belongs in
    /// the rules file beside the rest of them — and `false` is the explicit answer for
    /// the eight contests that shipped before it, one of which (OhQP) publishes the
    /// column as *"not permitted"* rather than merely absent.
    ///
    /// CQ WW and CQ WPX publish it: *"Note for Column 81 (transmitter number): For the
    /// MULTI-ONE and MULTI-TWO categories, the last column in the log indicates which
    /// transmitter made the QSO. It must be a 0 or a 1. This column is not required for
    /// other categories."* (<https://cqww.com/cabrillo.htm>, read 2026-09-09; the WPX
    /// page says the same for MULTI-TWO). Every sample QSO line on both pages carries
    /// the column, which is why a single-position entry writes `0` rather than omitting
    /// it — see [`ContestSession::transmitter_id`](crate::contest::ContestSession::transmitter_id).
    pub transmitter_column: bool,
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
    ruleset_by_id(event.code(), year).expect("validation guarantees a ruleset per Field Day event")
}

/// The active ruleset for a rules-file EVENT ID — the fallible lookup, for a contest
/// [`FdEvent`] has no arm for.
///
/// ⭐ **Fallible on purpose, and that is what [`ruleset`]'s floor rests on.** A rules
/// file that drops `arrlfd` or `wfd` would make [`ruleset`] panic, so those two are
/// refused at load ([`parse_spec`]). A file that drops `tnqp` costs the operator the
/// Tennessee QSO Party and nothing else, because every reader of one comes through
/// here and gets a `None` to handle. The floor guards the panic, not the menu.
pub fn ruleset_by_id(event_id: &str, year: u16) -> Option<&'static FdRuleset> {
    let mine = || {
        table()
            .rulesets
            .iter()
            .copied()
            .filter(|r| r.event == event_id)
    };
    mine()
        .filter(|r| r.rules_year <= year)
        .max_by_key(|r| r.rules_year)
        .or_else(|| mine().max_by_key(|r| r.rules_year))
}

/// The ARRL/RAC section master list — the section universe the worked-sections
/// board (spec §5) and setup validation read from. Ordered and grouped by ARRL
/// division so the board renders one tidy block per division; the ordering is
/// mirrored (and guard-tested) in ui/src/features/arrlSections.ts. 71 US ARRL
/// sections + 14 RAC (Canada) = 85, carried by the rules data.
///
/// ⚠️ The list is ARRL's CURRENT one (arrl.org/section-abbreviations and the generic
/// Field Day section PDF, both read 2026-09-09). It shipped as 83 from a pre-2017 era
/// until then; [`retired_section`] carries what became of the three codes that went.
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

/// A section code ARRL used to publish and does not any more, with what replaced it.
///
/// This exists for ONE reason: an operator's `fd_section` was saved under the old list,
/// and correcting the list must not be the thing that stops them transmitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetiredSection {
    /// The retired code, canonical (uppercase).
    pub code: &'static str,
    /// What the section was called while it existed — the operator recognises this even
    /// when they no longer recognise the code.
    pub name: &'static str,
    /// The current section code(s) that took its place. Exactly one = a RENAME the
    /// sponsor published; more than one = a SPLIT, which nothing here can resolve.
    pub successors: &'static [&'static str],
}

impl RetiredSection {
    /// The single successor when ARRL published a **rename**, `None` for a **split**.
    ///
    /// ⭐ This distinction is the entire migration policy, in one method. A rename is the
    /// sponsor's own alias and can be applied silently, because the operator's section did
    /// not move — only its abbreviation did. A split cannot: it would put a section the
    /// operator never chose into the exchange they transmit for a whole contest, and a
    /// wrong section makes the log unsubmittable. So a split is ASKED, never guessed.
    pub fn rename_target(&self) -> Option<&'static str> {
        match self.successors {
            [only] => Some(only),
            _ => None,
        }
    }
}

/// The three codes the pre-2017 list carried that ARRL's current one does not.
///
/// **Read from the sponsor, not inferred.** The generic Field Day section package
/// (<https://www.arrl.org/files/file/Field-Day/Generic/ARRL-RAC%20Section%20List.pdf>,
/// footer *"Revised 2025"*, read 2026-09-09) prints two of the three transitions inline,
/// which is what makes them safe to apply without asking:
///
/// ```text
/// Golden Horseshoe  GH (formerly GTA)
/// Territories       TER (formerly NT)
/// ```
///
/// `MAR` gets no such line, and that absence is the fact: it is on neither the PDF nor
/// <https://www.arrl.org/section-abbreviations>, while `NB`, `NS` and `PE` are on both as
/// three separate entries. The Maritime section SPLIT, so there is no alias to publish
/// and none is invented here.
const RETIRED_SECTIONS: [RetiredSection; 3] = [
    RetiredSection {
        code: "MAR",
        name: "Maritime",
        successors: &["NB", "NS", "PE"],
    },
    RetiredSection {
        code: "GTA",
        name: "Greater Toronto Area",
        successors: &["GH"],
    },
    RetiredSection {
        code: "NT",
        name: "Northern Territories",
        successors: &["TER"],
    },
];

/// What became of `code`, if ARRL retired it — `None` for a current section, and `None`
/// for junk. Normalised exactly like [`valid_section`], so the two agree on any input.
///
/// ⚠️ A code is either current or retired, never both — a code in both lists would
/// migrate an operator off a section that still exists. `tests/arrl_sections.rs` pins it,
/// along with the rule that every successor named here IS a current section.
pub fn retired_section(code: &str) -> Option<&'static RetiredSection> {
    let up = code.trim().to_ascii_uppercase();
    RETIRED_SECTIONS.iter().find(|r| r.code == up)
}

/// The Field Day SECTION slot's domain: the 85 ARRL/RAC section codes plus the `MX`
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

/// The 85 ARRL/RAC section codes as a [`contest::Domain`](crate::contest::Domain)
/// — the plain section universe, with none of Field Day's `MX`/`DX`
/// extensions.
///
/// This is where §8(d)'s "exactly 85" assertion now lives as a property of a
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

/// The event ids a rules file MUST carry — §8(d)'s inversion: a download may ADD
/// contests, but never remove one this build cannot run without.
///
/// ⭐ **The floor is [`FdEvent::ALL`], not "every ruleset the bundled seed carries",
/// and the difference is which failure it prevents.** [`ruleset`] is INFALLIBLE for
/// an `FdEvent`, so a file without `arrlfd` or `wfd` is not a missing menu entry — it
/// is a panic on the Field Day scoring path. Every other contest is reached through
/// [`ruleset_by_id`], which returns `None`, so a file that drops one degrades to "that
/// contest is not offered".
///
/// Batch 1 derived this list from the seed instead, reasoning that a derived floor
/// grows by itself as contests are added. It does — and what it grows is the set of
/// files the app REFUSES ENTIRELY. A published file that dropped one QSO party would
/// cost every install its Field Day rules updates too, which is the outcome the seed
/// floor exists to prevent, arriving through the guard meant to prevent it. It also
/// made every one of the 57 corpus fixtures have to carry a verbatim copy of every
/// county domain in the seed (≈188 000 lines) to stay loadable, for no gain in what
/// any of them pins.
fn required_events() -> &'static [&'static str] {
    static E: OnceLock<Vec<&'static str>> = OnceLock::new();
    E.get_or_init(|| FdEvent::ALL.iter().map(|e| e.code()).collect())
}

/// Is `id` a well-formed rules-file event id — `^[a-z][a-z0-9_]*$`?
///
/// ⚠️ **This replaces a hardcoded `["arrlfd", "wfd"]` membership test**, which as
/// written refused the very contests the programme adds (spec §8d names it). The
/// vocabulary of events is OPEN — a rules push is how a contest arrives — while the
/// vocabulary of everything an event DECLARES stays closed, which is where a typo
/// still has to be caught.
fn is_event_id(id: &str) -> bool {
    is_domain_id(id)
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
    /// Does the sponsor's Cabrillo QSO template carry the trailing transmitter-id
    /// column? Required, and `false` for every contest that shipped before it — a
    /// `#[serde(default)]` here would let a file that forgot the answer emit a column
    /// OhQP's own sponsor calls *"not permitted"*.
    transmitter_column: bool,
    dupe: DupeSpec,
    domains: Vec<DomainSpec>,
    exchange: ExchangeBlockSpec,
    bonuses: Vec<BonusSpec>,
    banned_modes: Vec<String>,
    tempo_fd: bool,
    assistance: AssistanceSpec,
    enforcement: String,
    /// An i18n key naming what this event's score leaves out; `""` = nothing.
    ///
    /// ⚠️ `#[serde(default)]` here follows `assistance.assistance_note_key`, the file's
    /// own convention for an ADVISORY DISPLAY string: absent means the score carries
    /// no note, which is exactly what both Field Day events mean and is visible on
    /// screen either way. That is not the `exchange`/`scoring` case §8(c) rules on,
    /// where a default would let a file load with a whole behaviour missing.
    #[serde(default)]
    score_note_key: String,
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
    /// ⭐ **Where the value this slot SENDS comes from** (spec §2.5, §3.4) — one of
    /// [`SENT_SOURCE_CONSTANT`], [`SENT_SOURCE_SERIAL`], `"setting:<name>"` naming a
    /// field of [`SENT_SLOT_SETTINGS`], or `"derived:<what>"`. `""` is legal only for
    /// a slot NO role sends.
    ///
    /// **Why this exists, and it is not paperwork.** Round 3's Sweepstakes blocker was
    /// not that a setting was hard to add; it was that NOTHING CHECKED. Batch 8 added
    /// `contest_qth_*` for the QSO parties' counties and batch 9 shipped Sweepstakes,
    /// whose role sends a check and a section — neither of which had a source anywhere
    /// in the build. The failure surfaces as an operator at 1400 on contest Saturday
    /// who cannot fill their own exchange. With this field the rules file will not
    /// load, which is a red gate at development time.
    ///
    /// ⚠️ **`setting:` is checked against a CLOSED list, which is what gives the rule
    /// teeth.** A free-text source would only assert that somebody typed something.
    /// [`SENT_SLOT_SETTINGS`] names the settings this build can actually supply, and
    /// `tempo-app` holds the test that every name in it is a real serde field of
    /// `Settings` — so the chain runs file → loader → struct with no unchecked link.
    ///
    /// ⚠️ **Required, not `#[serde(default)]`** — the same rule [`AdifTagsSpec`] is
    /// written to. Absent must be a refusal; `""` must be a decision ("no role sends
    /// this slot"). A defaulted source would let a file that simply forgot one load as
    /// though its author had decided the slot needs none.
    source: String,
    kind: KindSpec,
}

/// A sent slot whose value is a constant the composer supplies (an RST: `599` on
/// CW/digital, `59` on phone).
const SENT_SOURCE_CONSTANT: &str = "constant";
/// A sent slot filled from the session's serial counter.
const SENT_SOURCE_SERIAL: &str = "serial";

/// ⭐ **The settings a rules file may name as the source of a sent slot** (spec §3.4).
///
/// The frozen `fd_*` names and the `contest_*` block that landed beside them — never
/// replacing them (§8c: renaming a persisted serde field is a silent-reset hazard).
/// A ruleset naming anything else is refused with the name, because a source this
/// build cannot read is the same failure as no source at all, arriving later.
///
/// ⚠️ These are the RUST field names (`snake_case`), which is what
/// `tempo_app::settings::Settings` serialises under; the TypeScript mirror is
/// camelCase and is a separate site (`ui/src/types.ts`).
pub const SENT_SLOT_SETTINGS: &[&str] = &[
    // Frozen, shipped, and read for Sweepstakes' section too — §8(c) freezes the
    // NAME, not the meaning.
    "fd_class",
    "fd_section",
    // New in batch 8, beside them.
    "contest_qth_county",
    "contest_qth_state",
    "contest_check",
    "contest_cq_zone",
    "contest_itu_zone",
    "contest_power",
    // New in batch 9. ⚠️ §11.9 said Sweepstakes needed NO new settings; that held for
    // the STATION data (`contest_check` and the `fd_section` ruling landed in batch 8)
    // and not for the entry category. §6.1's header table sources `PREC` from "the
    // `CATEGORY-*` picker", and batch 8 shipped exactly one of the four
    // (`contest_category_operator`); the other three are what separate Q from A from B
    // from U from S, and without them a Sweepstakes entrant transmits a category
    // nobody declared on every contact of a 24-hour contest.
    "contest_category_operator",
    "contest_category_power",
    "contest_category_assisted",
    "contest_category_station",
    // Identity, long shipped: ARRL VHF sends the operator's own grid, and Sweepstakes
    // sends the operator's own CALL — SS-Rules v2.1 §4.3, *"the call sign must be
    // included during the exchange"*, the one contest in the researched set where the
    // callsign is an exchange slot rather than only the QSO line's own column.
    "mycall",
    "mygrid",
];

/// ⭐ **The derivations a rules file may name, each with the settings it is built
/// from** — the second half of §2.5's chain, and the half that keeps a derivation from
/// being a place to hide a missing setting.
///
/// A slot whose value is assembled rather than read straight out of one field names a
/// derivation: a QSO party's `QTH` is the session's `MyLocation`, which is county *or*
/// state depending on the role, so no single `setting:` token describes it. Naming the
/// derivation's own inputs here means `derived:` is not an escape hatch — every input
/// still has to be a settings field this build carries.
const SENT_SLOT_DERIVATIONS: &[(&str, &[&str])] = &[
    // §3.4: "QTH (county / state / DX) → contest_qth_county, contest_qth_state —
    // this is `my_location` (§3)". One slot, two settings, chosen by role.
    ("my_location", &["contest_qth_county", "contest_qth_state"]),
    // §6.3's `PREC`: Sweepstakes' precedence letter is the entry's declared category
    // restated (SS-Rules v2.1 §4.2), so it is derived from the four `CATEGORY-*` axes
    // and never typed into the exchange. All four inputs are settings this build
    // carries, which is what keeps `derived:` from being a place to hide a missing one.
    (
        "precedence",
        &[
            "contest_category_operator",
            "contest_category_power",
            "contest_category_assisted",
            "contest_category_station",
        ],
    ),
];

/// Is `s` a source this build can honour for a slot some role sends?
fn is_sent_source(s: &str) -> bool {
    match s {
        SENT_SOURCE_CONSTANT | SENT_SOURCE_SERIAL => true,
        _ => match s.split_once(':') {
            Some(("setting", name)) => SENT_SLOT_SETTINGS.contains(&name),
            // A derivation is named, never blank — and every one of ITS inputs is a
            // settings field this build carries, or the derivation is the same "no
            // source" hole wearing a different word.
            Some(("derived", what)) => SENT_SLOT_DERIVATIONS
                .iter()
                .find(|(id, _)| *id == what)
                .is_some_and(|(_, inputs)| inputs.iter().all(|i| SENT_SLOT_SETTINGS.contains(i))),
            _ => false,
        },
    }
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
    /// ⭐ **The QSO-point table for a contest priced by the RELATION between my station
    /// and the station worked** — CQ WW and CQ WPX.
    ///
    /// **`[]` is the explicit "this event prices by mode class"**, the same statement
    /// `multipliers: []` makes about multipliers, and the key is REQUIRED for the same
    /// reason: a file that simply forgot CQ WW's table must not load and score every
    /// contact off a `points_by_mode_class` that means nothing for it.
    ///
    /// ⚠️ **Exactly one of the two is populated.** A non-empty table here requires
    /// `points_by_mode_class` to be EMPTY, and vice versa — two live point tables would
    /// leave a reader to guess which one scored the log, and there is no honest
    /// per-mode-class number to write for a contest that has none.
    ///
    /// ⚠️ This is an ADDITION to schema 2, not a reshape: an already-shipped build
    /// ignores an unknown key, so it keeps receiving rules updates. What it cannot do is
    /// SCORE a relation-priced contest, and the first build that can is the one that
    /// introduced the key.
    relation_points: Vec<RelationPointsSpec>,
}

/// One row of [`ScoringSpec::relation_points`].
#[derive(Debug, serde::Deserialize)]
struct RelationPointsSpec {
    /// `same_country` | `within_north_america` | `same_continent` | `different_continent`
    /// — a CLOSED vocabulary, refused by name, so `same_zone` cannot load as something.
    relation: String,
    /// Band labels this row prices. `[]` = every band (CQ WW's four arms, and CQ WPX's
    /// same-country arm: *"1 point regardless of band"*).
    bands: Vec<String>,
    points: u32,
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
    for want in required_events() {
        if !spec.rulesets.iter().any(|r| r.event == *want) {
            return Err(format!("missing the `{want}` ruleset"));
        }
    }
    let mut seen_events: Vec<(&str, u16)> = Vec::new();
    for r in &spec.rulesets {
        let tag = format!("ruleset {}/{}", r.event, r.rules_year);
        if !is_event_id(&r.event) {
            return Err(format!("{tag}: event id is not ^[a-z][a-z0-9_]*$"));
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
            "powered_multiplier" | "objectives" | "none"
        ) {
            return Err(format!(
                "{tag}: unknown scoring model {:?}",
                r.scoring.model
            ));
        }
        // ⭐ EXACTLY ONE point table is live. `relation_points: []` is the explicit "this
        // event prices by mode class" and is what every contest before CQ WW writes; a
        // populated table means the mode-class map must be empty, because there is no
        // honest per-mode-class number for a contest whose points depend on where the
        // other station is.
        if r.scoring.relation_points.is_empty() {
            for k in ["PH", "CW", "DIG"] {
                if !r.scoring.points_by_mode_class.contains_key(k) {
                    return Err(format!("{tag}: points_by_mode_class misses {k}"));
                }
            }
        } else {
            if !r.scoring.points_by_mode_class.is_empty() {
                return Err(format!(
                    "{tag}: both relation_points and points_by_mode_class are populated \
                     (a contest has ONE point table)"
                ));
            }
            // ⚠️ The VOCABULARY first, then the coverage. A misspelt relation is also a
            // missing one, and reporting the coverage failure for a row that is simply
            // typed wrong sends the reader to the wrong half of the file.
            for x in &r.scoring.relation_points {
                if !matches!(
                    x.relation.as_str(),
                    "same_country"
                        | "within_north_america"
                        | "same_continent"
                        | "different_continent"
                ) {
                    return Err(format!("{tag}: unknown relation {:?}", x.relation));
                }
            }
            // ⭐ The guard the source verification earned. §14 of the design spec carried
            // CQ WPX's two band groups and its North American exception and MISSED the
            // third arm — *"Contacts between stations in the same country are worth 1
            // point regardless of band"* — which would have scored every domestic contact
            // at zero, silently, on a 48-hour contest. A relation the table names nothing
            // for is exactly that defect, so the loader refuses it BY NAME.
            //
            // `within_north_america` is deliberately NOT required: it is an exception to
            // `same_continent`, and a contest that does not grant it falls back to the arm
            // it would have overridden (`PointsRule::points_for`).
            for want in ["same_country", "same_continent", "different_continent"] {
                if !r.scoring.relation_points.iter().any(|x| x.relation == want) {
                    return Err(format!(
                        "{tag}: relation_points declares no {want} row (every contact has \
                         exactly one relation, so a missing arm scores those contacts at \
                         zero without saying so)"
                    ));
                }
            }
        }
        // `none` is the state QSO parties: QSO points × geographic multipliers and
        // nothing after. It is the only model that may declare no power tiers, and it
        // must declare none — a tier list under a model that applies no power
        // multiplier is a claim the scorer would silently ignore, which is exactly how
        // an event ends up scoring by a rule nobody can find in the file.
        if r.scoring.model == "none" {
            if !r.scoring.power_tiers.is_empty() {
                return Err(format!(
                    "{tag}: scoring model \"none\" declares power_tiers \
                     (a model with no post-multiplier applies none)"
                ));
            }
        } else if r.scoring.power_tiers.is_empty() {
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
        let last_role = x.roles.last().expect("roles checked non-empty above");
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
            // ⭐ `always` may be the LAST role, not only the ONLY role. The reason the
            // rule exists is unchanged and is what it still enforces — a role AFTER an
            // unconditional one could never be reached — but "only" also refused the
            // shape the QSO parties are built on: spec §2.3's own worked Ohio example
            // ends `role "dx": Always`, three roles deep, and it is the catch-all
            // `ContestSession::role` already documents itself as falling through to. A
            // DX entrant matches no `my_location_in`, so without a trailing `always`
            // the last role is reached by ordering alone and the file cannot say so.
            if matches!(role.selector, SelectorSpec::Always) && !std::ptr::eq(role, last_role) {
                return Err(format!(
                    "{tag}: role {:?} selector `always` must be the LAST role \
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
        // ⭐ **EVERY SLOT ANY ROLE SENDS HAS A DECLARED SOURCE** (spec §2.5, §3.4).
        //
        // This is the rule that makes "the batch that ships this contest forgot to add
        // the setting its exchange needs" a red gate here rather than an operator who
        // cannot fill their own exchange on contest Saturday. It runs after the role
        // loop because it is the ROLES that say which slots are sent: a slot only a
        // role RECEIVES is copied off the air and has no source to declare.
        for f in &x.fields {
            let sent = x.roles.iter().any(|role| role.sends.contains(&f.key));
            if !sent {
                // A received-only slot must say so with `""`, and must not claim a
                // source it can never use.
                if !f.source.is_empty() {
                    return Err(format!(
                        "{tag}: slot {} declares source {:?} but no role sends it",
                        f.key, f.source
                    ));
                }
                continue;
            }
            if f.source.is_empty() {
                return Err(format!(
                    "{tag}: slot {} is sent but declares no source \
                     (spec §3.4 — name a setting, a derivation, or the serial counter)",
                    f.key
                ));
            }
            if !is_sent_source(&f.source) {
                return Err(format!(
                    "{tag}: slot {} declares source {:?}, which this build cannot supply",
                    f.key, f.source
                ));
            }
        }
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
        // ⭐ Up to 47, not 23: the field is hours from 0000Z on the ANCHOR SATURDAY,
        // and the Tennessee QSO Party starts at 1700Z on the SUNDAY (= 41). Capped at
        // 23 there was no expressible rule for it at all — the sponsor's own PDF
        // publishes the 2026 instants and no annual rule, so a build that could only
        // anchor on the Saturday would have had to pin every year by hand and would
        // show a window 24 hours early for any year nobody pinned.
        if w.start_hour_utc >= 48 {
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
    // The section universe is pinned (71 US + 14 RAC): the TS mirror guard and
    // the board layout both assume it, so a file that grows or shrinks it must
    // land in lockstep with a code release, not as a data push.
    //
    // ⚠️ 85 since the pre-2017 list was corrected (see [`sections`]). Moving this
    // number is not a data push either: the shipped app enforces the same floor, so a
    // rules file with a different count is refused by every build that predates the
    // change — which is exactly the lockstep this line exists to force.
    if spec.sections.len() != 85 {
        return Err(format!("{} sections (expected 85)", spec.sections.len()));
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
            // The validated source travels onto the spec — the session constructor
            // fills one value per sent slot and reads it from here, so the declaration
            // the loader checked is the declaration that is honoured.
            source: leak_str(f.source),
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
            // ⚠️ Indexed, not `.get().unwrap_or(0)`: the validator has already proved all
            // three keys are present whenever this map is the live table. A relation-priced
            // contest writes `{}` here and the map is never read — a silent `0` would be a
            // per-mode-class table nobody declared.
            let points = if r.scoring.points_by_mode_class.is_empty() {
                ModePoints {
                    ph: 0,
                    cw: 0,
                    dig: 0,
                }
            } else {
                ModePoints {
                    ph: r.scoring.points_by_mode_class["PH"],
                    cw: r.scoring.points_by_mode_class["CW"],
                    dig: r.scoring.points_by_mode_class["DIG"],
                }
            };
            let power_tiers: &'static [u32] = Box::leak(r.scoring.power_tiers.into_boxed_slice());
            // The file's `model` string names a POST-multiplier profile; both
            // profiles also carry the claimed bonus menu, which is what the two
            // old `ScoringModel` arms each did implicitly by having the callers
            // add `bonus_points` afterwards.
            let post: &'static [PostMultiplier] = Box::leak(
                match r.scoring.model.as_str() {
                    // ⭐ No post-multiplier at all — the state QSO parties. Not even
                    // `Bonuses`: each of the four declares an empty bonus menu, and
                    // the two that DO have bonuses (TNQP, TXQP) compute them from the
                    // log rather than from a menu the operator ticks, which
                    // `PostMultiplier::Bonuses` cannot express. Declaring the arm
                    // anyway would read as "the bonuses are handled" when they are
                    // deliberately omitted — see the seed's `_provenance`.
                    "none" => vec![],
                    "powered_multiplier" => vec![
                        PostMultiplier::PowerTier { tiers: power_tiers },
                        PostMultiplier::Bonuses,
                    ],
                    // Validated to be one of the three.
                    _ => vec![
                        PostMultiplier::Objectives {
                            at_submission: true,
                        },
                        PostMultiplier::Bonuses,
                    ],
                }
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
            // The two point tables the validator has already proved are mutually
            // exclusive: a populated `relation_points` is the relation-priced contest,
            // and an empty one is the per-mode-class map every earlier contest writes.
            let qso_points = if r.scoring.relation_points.is_empty() {
                PointsRule::ByModeClass(points)
            } else {
                PointsRule::ByRelation(Box::leak(
                    r.scoring
                        .relation_points
                        .into_iter()
                        .map(|x| crate::contest::RelationPoints {
                            relation: match x.relation.as_str() {
                                "same_country" => crate::contest::Relation::SameCountry,
                                "within_north_america" => {
                                    crate::contest::Relation::WithinNorthAmerica
                                }
                                "different_continent" => {
                                    crate::contest::Relation::DifferentContinent
                                }
                                // Validated to be one of the four.
                                _ => crate::contest::Relation::SameContinent,
                            },
                            bands: leak_keys(x.bands),
                            points: x.points,
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ))
            };
            let scoring = crate::contest::Scoring {
                qso_points,
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
                event: leak_str(r.event),
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
                score_note_key: leak_str(r.score_note_key),
                transmitter_column: r.transmitter_column,
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
    /// membership test, so the domain must be EXACTLY that set — 85 sections plus
    /// the two literals a DX station sends — or a legal contact stops being loggable.
    #[test]
    fn the_fd_section_domain_is_the_85_sections_plus_mx_and_dx() {
        let d = fd_sections_domain();
        assert_eq!(d.id, "fd_sections");
        assert_eq!(d.values.len(), 87, "85 ARRL/RAC sections + MX + DX");
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
        // 71 US ARRL sections + 14 RAC = the full 85-section universe.
        assert_eq!(sections().len(), 85, "the ARRL/RAC section master list");
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
            "EMA", "WMA", "STX", "NTX", "WTX", "SDG", "ORG", "SCV", "NNY", "GH", "TER", "NB", "NS",
            "PE",
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
        assert_eq!(ts.len(), 85, "85 = 71 US ARRL + 14 RAC, both sides");

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

    /// SAME GUARD for the RETIRED list: [`RETIRED_SECTIONS`] vs `RETIRED_SECTIONS`
    /// in ui/src/features/arrlSections.ts.
    ///
    /// The two halves say the same thing to the operator at two different moments —
    /// the Settings picker explains a stored `MAR` while they are looking at the
    /// field, and `Engine::set_mode` explains it again if they try to operate
    /// anyway. Drift here is a section named one thing in Settings and another in
    /// the refusal, or a successor offered on one screen and not the other. Reads
    /// the TS source itself, both directions, naming the entry that moved.
    #[test]
    fn the_typescript_retired_section_mirror_matches_rust_exactly() {
        let ts_src = include_str!("../../../ui/src/features/arrlSections.ts");
        // Rows are one per line: `MAR: { name: 'Maritime', successors: ['NB', 'NS', 'PE'] },`
        // Keying on `successors: [` skips the doc comment and every other declaration
        // in the file (the section table's rows carry `code:`, not `successors:`).
        let ts: Vec<(String, String, Vec<String>)> = ts_src
            .lines()
            .filter(|l| l.contains("successors: ["))
            .map(|l| {
                let code = l.trim().split(':').next().unwrap().trim().to_string();
                let name = ts_str_field(l, "name").expect("row names the section");
                let list = {
                    let i = l.find("successors: [").unwrap() + "successors: [".len();
                    let rest = &l[i..];
                    let end = rest.find(']').expect("the successor list closes");
                    rest[..end]
                        .split(',')
                        .map(|s| s.trim().trim_matches('\'').to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                };
                (code, name.to_string(), list)
            })
            .collect();
        assert_eq!(
            ts.len(),
            RETIRED_SECTIONS.len(),
            "the retired-section tables are different sizes — TS has {:?}, Rust has {:?}",
            ts.iter().map(|(c, _, _)| c).collect::<Vec<_>>(),
            RETIRED_SECTIONS.iter().map(|r| r.code).collect::<Vec<_>>()
        );
        // The control: the parse must actually have found rows, or every compare
        // below is vacuous.
        assert!(!ts.is_empty(), "parsed no rows out of arrlSections.ts");
        for (code, name, successors) in &ts {
            let rust = RETIRED_SECTIONS
                .iter()
                .find(|r| r.code == code)
                .unwrap_or_else(|| panic!("arrlSections.ts retires {code}, Rust does not"));
            assert_eq!(rust.name, name, "{code}: the section's name drifted");
            assert_eq!(
                rust.successors, successors,
                "{code}: the replacement codes drifted"
            );
        }
    }

    /// SAME GUARD for the hand-mirrored bonus menu: the seed's bonus menu vs
    /// the `FD_BONUSES` table in ui/src/components/ContestView.tsx. Ids and
    /// points only — the LABELS deliberately differ (the seed labels are the
    /// moved tuple-table strings; the TS labels are the invariant display
    /// strings the checklist renders), so comparing them would pin an
    /// intentional difference, not catch drift. A points or id drift is the
    /// one that mis-scores a claimed bonus.
    #[test]
    fn the_typescript_bonus_mirror_matches_fd_bonuses_exactly() {
        let bonuses = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR).bonuses;
        let ts_src = include_str!("../../../ui/src/components/ContestView.tsx");
        // Pull just the FD_BONUSES table body (the file declares other objects).
        let head = "export const FD_BONUSES";
        let start = ts_src
            .find(head)
            .expect("ContestView.tsx declares FD_BONUSES");
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
            "parsed only {} bonus rows out of ContestView.tsx — the parser is \
             broken, not the mirror",
            ts.len()
        );

        let rust_ids: Vec<&str> = bonuses.iter().map(|b| b.id).collect();
        let ts_ids: Vec<&str> = ts.iter().map(|r| r.0).collect();
        let missing_in_ts: Vec<&&str> = rust_ids.iter().filter(|c| !ts_ids.contains(c)).collect();
        assert!(
            missing_in_ts.is_empty(),
            "bonus id(s) {missing_in_ts:?} exist in the seed's bonus menu but NOT in \
             ContestView.tsx — the checklist can never claim them"
        );
        let missing_in_rust: Vec<&&str> = ts_ids.iter().filter(|c| !rust_ids.contains(c)).collect();
        assert!(
            missing_in_rust.is_empty(),
            "bonus id(s) {missing_in_rust:?} exist in ContestView.tsx but NOT in \
             the seed's bonus menu — a claimed checkbox that scores nothing"
        );
        assert_eq!(ts.len(), 15, "the full ARRL bonus menu, both sides");
        for (i, (rust, ts_row)) in bonuses.iter().zip(&ts).enumerate() {
            assert_eq!(
                (rust.id, rust.points),
                *ts_row,
                "bonus #{i} diverged (id or points) between the seed's bonus menu \
                 and ContestView.tsx"
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
            .find(|r| r.event == FdEvent::ArrlFd.code())
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
        .contains("84 sections"),);
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

    /// §8(d)'s inversion: a downloaded file may ADD contests but never REMOVE one
    /// this build cannot run without — `arrlfd` and `wfd`, the two reached through
    /// the infallible [`ruleset`].
    ///
    /// ⭐ **And the other half, which is what the narrowed floor buys:** dropping a
    /// state QSO party is NOT a refusal. That file still loads, Field Day still gets
    /// its rules update, and the only cost is that [`ruleset_by_id`] returns `None`
    /// for the contest that went away. Under a floor derived from the seed, one
    /// withdrawn QSO party would have cost every install its Field Day updates too.
    #[test]
    fn a_file_missing_a_required_ruleset_is_refused_and_a_dropped_qso_party_is_not() {
        let drop_event = |ev: &str| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            let rs = v["rulesets"].as_array_mut().unwrap();
            let i = rs
                .iter()
                .position(|r| r["event"] == ev)
                .unwrap_or_else(|| panic!("the seed carries {ev}"));
            rs.remove(i);
            parse_spec(&v.to_string())
        };
        for ev in ["arrlfd", "wfd"] {
            let e = drop_event(ev).unwrap_err();
            assert!(e.contains(ev), "names the missing ruleset: {e}");
        }
        // POSITIVE CONTROL, both directions: the untouched file loads, and a file
        // missing a contest OUTSIDE the floor loads too.
        assert!(parse_spec(SEED).is_ok(), "the control file must load");
        for ev in ["tnqp", "ohqp", "cqp", "txqp"] {
            assert!(
                drop_event(ev).is_ok(),
                "a file without {ev} must still load — the floor is not the menu"
            );
        }
    }

    /// ⭐ **The floor is the events reached through the INFALLIBLE accessor**, and
    /// it is derived from [`FdEvent::ALL`] rather than written out here — a
    /// hardcoded pair would drift the moment `FdEvent` gained an arm.
    ///
    /// What it is NOT is "every ruleset the seed carries". The seed now carries four
    /// state QSO parties as well, and each is reached through [`ruleset_by_id`], which
    /// returns `None`: a file that drops one costs the operator that contest. A file
    /// that drops `arrlfd` or `wfd` would make [`ruleset`] panic, which is the whole
    /// reason a floor exists.
    #[test]
    fn the_floor_is_the_events_with_an_infallible_lookup() {
        assert_eq!(required_events(), ["arrlfd", "wfd"]);
        assert_eq!(
            required_events().len(),
            FdEvent::ALL.len(),
            "one floor entry per FdEvent arm"
        );
        // The seed carries MORE than the floor, which is the point of the narrowing.
        let seeded: Vec<&str> = table().rulesets.iter().map(|r| r.event).collect();
        for want in required_events() {
            assert!(seeded.contains(want), "the seed must satisfy its own floor");
        }
        assert!(
            seeded.len() > required_events().len(),
            "the seed ships contests beyond the floor"
        );
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

    /// §8(d): the 85-section assertion, expressed on the domain (Ruling B0-C).
    #[test]
    fn the_arrl_sections_domain_is_exactly_the_85() {
        let d = arrl_sections_domain();
        assert_eq!(d.id, "arrl_sections");
        assert_eq!(d.values.len(), 85);
        assert_eq!(d.adif.rcvd, Some("ARRL_SECT"));
        // Verified against ADIF 3.1.7 in batch 6 — see
        // `the_fd_section_domain_is_the_85_sections_plus_mx_and_dx` for the
        // citation and the negative controls.
        assert_eq!(d.adif.sent, Some("MY_ARRL_SECT"));
        assert!(!d.contains("MX"), "MX is an FD extension, not a section");
        assert!(!d.contains("DX"), "…and so is DX");
        assert!(d.contains("WI") && d.contains(" wi "));
        assert_eq!(
            fd_sections_domain().values.len(),
            87,
            "…which fd_sections carries, being the 85 plus MX and DX"
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
                    "source": "constant",
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

    // ---- the state QSO parties (batch 8) ----------------------------------

    /// A shipped QSO-party ruleset by event id. Every one of these is reached the way
    /// the app reaches it — through the fallible lookup — so a test cannot pass by
    /// consulting a `static` the loader never produced.
    fn party(event: &str) -> &'static FdRuleset {
        ruleset_by_id(event, CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("the seed must carry {event}"))
    }

    fn domain_of(rs: &FdRuleset, id: &str) -> &'static crate::contest::Domain {
        rs.domains
            .iter()
            .copied()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{} must declare the {id} domain", rs.event))
    }

    /// ⭐⭐ **THE TEXAS FINDING, and it is the most important assertion in this batch.**
    ///
    /// txqp.net's rules page says logs *"must use the approved **four-letter** county
    /// abbreviation for the exchange"* — and the sponsor's OWN official list, on
    /// `?page_id=90`, contains **two three-letter codes: `BEE` (Bee) and `LEE` (Lee)**.
    /// 252 + 2 = 254.
    ///
    /// A validator written to the sponsor's PROSE rejects two real Texas counties. The
    /// operator who pays for that is a Texas mobile in Bee or Lee county, and what they
    /// get is an **unsubmittable log** for every QSO made from it — §13's first-named
    /// risk, arriving through a rule that reads like a safe simplification.
    ///
    /// **The `len == 4` rule below is the negative control**, and it is in the test on
    /// purpose: without it this is just a count, and a count cannot show that the
    /// obvious implementation is the wrong one. It must reject exactly Bee and Lee.
    #[test]
    fn txqp_ships_the_sponsors_254_counties_and_bee_and_lee_are_valid() {
        let tx = domain_of(party("txqp"), "tx_counties");
        assert_eq!(tx.values.len(), 254, "the sponsor's list is 254 entries");
        let codes: Vec<&str> = tx.values.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            codes
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            254,
            "no duplicate codes"
        );

        // The whole point: the two three-letter codes are members.
        for (code, name) in [("BEE", "Bee"), ("LEE", "Lee")] {
            assert!(
                tx.contains(code),
                "{code} must validate — it is a real county"
            );
            assert_eq!(
                tx.values.iter().find(|(c, _)| *c == code).map(|(_, n)| *n),
                Some(name),
                "{code} is {name} county"
            );
        }

        // NEGATIVE CONTROL — the validator the sponsor's prose describes. It must
        // reject exactly the two, and nothing else: that is what makes "254" evidence
        // rather than arithmetic.
        let prose_rule = |c: &&str| c.len() == 4;
        let rejected: Vec<&str> = codes.iter().copied().filter(|c| !prose_rule(c)).collect();
        assert_eq!(
            rejected,
            vec!["BEE", "LEE"],
            "a four-letter-only validator rejects exactly Bee and Lee"
        );
        assert_eq!(
            codes.iter().filter(|c| prose_rule(c)).count(),
            252,
            "252 four-letter + 2 three-letter = 254"
        );

        // The codes are the sponsor's, not generated from the names — the scheme is
        // irregular and a first-four-letters rule gets these wrong.
        let mut generated_would_differ = 0;
        for (code, name) in [
            ("BZIA", "Brazoria"),
            ("BZOS", "Brazos"),
            ("CMRN", "Cameron"),
            ("COLN", "Collin"),
            ("COLW", "Collingsworth"),
            ("COML", "Comal"),
            ("COMA", "Comanche"),
            ("DALM", "Dallam"),
            ("DALS", "Dallas"),
        ] {
            assert_eq!(
                tx.values.iter().find(|(c, _)| *c == code).map(|(_, n)| *n),
                Some(name),
                "{code} is the sponsor's code for {name}"
            );
            if &name.to_ascii_uppercase()[..4] != code {
                generated_would_differ += 1;
            }
            // …and most of them are cases a first-four-letters rule gets WRONG.
            // (Comanche → COMA is the one that happens to agree, which is why the
            // scheme has to be taken from the list rather than inferred from a
            // sample that looks regular.)
        }
        assert_eq!(
            generated_would_differ, 8,
            "8 of these 9 sponsor codes are not the county name's first four letters"
        );
    }

    /// ⭐ **The Ohio QSO Party is THREE roles** (§2.3), because ohqp.org publishes
    /// three sending behaviours with a domain for each. A two-role model ships a DX
    /// entrant the wrong exchange, and that error would reach the air.
    ///
    /// The `w_ve` selector list is the other half: the sponsor puts KH6/KL7 in the
    /// W/VE role explicitly, so a list built from the CONUS states would give a KL7
    /// entrant the DX role.
    #[test]
    fn ohqp_declares_three_roles_and_the_w_ve_list_includes_ak_and_hi() {
        let oh = party("ohqp");
        let roles = oh.exchange.roles;
        assert_eq!(
            roles.iter().map(|r| r.id).collect::<Vec<_>>(),
            ["in_state", "w_ve", "dx"],
            "three roles, in the order the sponsor states them"
        );
        // Every role sends and receives the same two slots; what differs is the value
        // the QTH slot can carry, which is what `OneOf` expresses.
        for r in roles {
            assert_eq!(r.sends, ["RST", "QTH"], "role {}", r.id);
            assert_eq!(r.receives, ["RST", "QTH"], "role {}", r.id);
        }
        let Some(RoleSelectorProbe::Location(list)) = roles
            .iter()
            .find(|r| r.id == "w_ve")
            .map(|r| RoleSelectorProbe::of(&r.selector))
        else {
            panic!("w_ve selects on location");
        };
        for s in ["AK", "HI", "OH"] {
            let want = s != "OH";
            assert_eq!(
                list.contains(&s),
                want,
                "{s} in the W/VE selector list: expected {want}"
            );
        }
        assert_eq!(list.len(), 61, "the sponsor's 62 codes less DX");
        // The DX role is the trailing catch-all `ContestSession::role` falls through
        // to, and the file says so rather than relying on ordering alone.
        assert!(matches!(
            roles.last().map(|r| r.selector),
            Some(crate::contest::RoleSelector::Always)
        ));
        // The two multiplier universes the sponsor publishes: 88 counties for
        // everyone, and 150 (88 + 62) for an Ohio station.
        assert_eq!(domain_of(oh, "oh_counties").values.len(), 88);
        assert_eq!(domain_of(oh, "oh_mults").values.len(), 62);
        // The sponsor's own typo is shipped verbatim — the ROBOT matches on the code,
        // and "fixing" a name here would put a display string out of step with it.
        assert_eq!(
            domain_of(oh, "oh_counties")
                .values
                .iter()
                .find(|(c, _)| *c == "AUGL")
                .map(|(_, n)| *n),
            Some("Auglaze"),
            "the sponsor spells Auglaize this way"
        );
    }

    /// A read-only view of a selector, so a test can assert on the location list
    /// without matching a three-arm enum inline four times.
    enum RoleSelectorProbe {
        Location(&'static [&'static str]),
        Other,
    }
    impl RoleSelectorProbe {
        fn of(s: &crate::contest::RoleSelector) -> Self {
            match s {
                crate::contest::RoleSelector::MyLocationIn(l) => Self::Location(l),
                _ => Self::Other,
            }
        }
    }

    /// Every county domain is the size the sponsor's own list is, counted off that
    /// list rather than read off a stated total — which is exactly how `BEE`/`LEE`
    /// surfaced, since no stated total would have shown them.
    #[test]
    fn every_qso_party_county_domain_is_the_sponsors_own_list() {
        for (event, domain, n) in [
            ("tnqp", "tn_counties", 95),
            ("ohqp", "oh_counties", 88),
            ("ohqp", "oh_mults", 62),
            ("cqp", "ca_counties", 58),
            ("cqp", "cqp_states", 63),
            ("txqp", "tx_counties", 254),
        ] {
            let d = domain_of(party(event), domain);
            assert_eq!(d.values.len(), n, "{event}/{domain}");
            assert!(
                d.values.iter().all(|(c, l)| !c.is_empty() && !l.is_empty()),
                "{event}/{domain}: every value is a code AND a name"
            );
        }
        // The non-obvious TNQP mappings, which a first-four-letters rule gets wrong.
        let tn = domain_of(party("tnqp"), "tn_counties");
        for (code, name) in [
            ("HARD", "Hardeman"),
            ("HARN", "Hardin"),
            ("VANB", "Van Buren"),
        ] {
            assert_eq!(
                tn.values.iter().find(|(c, _)| *c == code).map(|(_, n)| *n),
                Some(name)
            );
        }
        // CQP's state list is the 63 subdivision codes and carries no DC: the sponsor
        // labels MD "Maryland & DC".
        let ca = domain_of(party("cqp"), "cqp_states");
        assert!(!ca.contains("DX"), "DX is its own domain in CQP");
        assert!(!ca.contains("DC"), "CQP has no DC code");
        assert_eq!(
            ca.values.iter().find(|(c, _)| *c == "MD").map(|(_, n)| *n),
            Some("Maryland & DC")
        );
    }

    /// Each party's exchange, dupe rule, points and multiplier scope are the sponsor's
    /// own — and each one loads through the real table, so this is the ruleset the app
    /// would run, not a fixture.
    #[test]
    fn every_qso_party_ruleset_loads_with_the_sponsors_shape() {
        for (event, contest_id, slots, pts, scope) in [
            (
                "tnqp",
                "TN-QSO-PARTY",
                ["RST", "QTH"],
                [3, 3, 3],
                MultScope::PerBand,
            ),
            (
                "ohqp",
                "OH-QSO-PARTY",
                ["RST", "QTH"],
                [1, 2, 0],
                MultScope::PerMode,
            ),
            (
                "cqp",
                "CA-QSO-PARTY",
                ["NR", "QTH"],
                [3, 3, 0],
                MultScope::PerLog,
            ),
            (
                "txqp",
                "TX-QSO-PARTY",
                ["RST", "QTH"],
                [2, 3, 3],
                MultScope::PerLog,
            ),
        ] {
            let rs = party(event);
            assert_eq!(rs.contest_id, contest_id);
            assert_eq!(rs.rules_year, 2026);
            for r in rs.exchange.roles {
                assert_eq!(r.sends, slots, "{event} role {} sends", r.id);
                assert_eq!(r.receives, slots, "{event} role {} receives", r.id);
            }
            let PointsRule::ByModeClass(p) = rs.scoring.qso_points else {
                panic!("{event} prices by mode class, not by relation");
            };
            assert_eq!([p.ph, p.cw, p.dig], pts, "{event} QSO points");
            assert!(
                !rs.scoring.multipliers.is_empty(),
                "{event} counts a geographic multiplier"
            );
            for m in rs.scoring.multipliers {
                assert!(
                    matches!(m.scope, s if s == scope),
                    "{event} mult {} scope",
                    m.id
                );
            }
            // A QSO party has no post-multiplier at all — not a power tier, not
            // objectives, and not the claimed bonus menu (see the seed's
            // `_provenance`: TNQP's and TXQP's bonuses are COMPUTED, and this build
            // does not compute them).
            assert!(
                rs.scoring.post.is_empty(),
                "{event} declares no post-multiplier"
            );
            assert!(
                rs.scoring.power_tiers().is_none(),
                "{event} has no power tier"
            );
            assert!(rs.bonuses.is_empty() && rs.objectives.is_empty());
            // Both halves of the mobile rule, which is what the whole batch is for.
            assert_eq!(rs.dupe_rule.by_fields, ["QTH"], "{event}: they moved");
            assert_eq!(rs.dupe_rule.by_sent_fields, ["QTH"], "{event}: I moved");
            assert!(rs.dupe_rule.by_call && rs.dupe_rule.by_band && rs.dupe_rule.by_mode_class);
        }
    }

    /// ⭐ **The two parties whose score is knowingly incomplete SAY SO in data**, so
    /// the omission reaches the operator's screen instead of living in a provenance
    /// block nobody reads. The two whose score is complete must NOT carry a note —
    /// a note on every event is a note nobody reads either.
    #[test]
    fn the_computed_bonus_omission_is_declared_where_it_applies_and_only_there() {
        for event in ["tnqp", "txqp"] {
            assert!(
                !party(event).score_note_key.is_empty(),
                "{event} computes bonuses this build does not, and must say so"
            );
        }
        for event in ["ohqp", "cqp"] {
            assert!(
                party(event).score_note_key.is_empty(),
                "{event}'s score is complete — no note"
            );
        }
        for e in FdEvent::ALL {
            assert!(
                ruleset(e, CURRENT_RULES_YEAR).score_note_key.is_empty(),
                "neither Field Day event's score omits anything"
            );
        }
    }

    /// The window parameters reproduce each sponsor's own stated instants for 2026.
    ///
    /// ⚠️ TNQP is why `start_hour_utc` reaches 47: the sponsor's PDF publishes
    /// *"1700z Sunday, September 6 until 0300z Monday, September 7, 2026"* and no
    /// annual rule at all, so the only expressible reading is the SUNDAY of the first
    /// full September weekend — 41 hours from 0000Z on that Saturday.
    #[test]
    fn the_qso_party_windows_reproduce_the_sponsors_2026_instants() {
        let at = |y, m, d, h: u64| days_from_civil(y, m, d) as u64 * 86_400 + h * 3600;
        for (event, start, end) in [
            // 1700Z Sun 6 Sep 2026 → 0300Z Mon 7 Sep 2026.
            ("tnqp", at(2026, 9, 6, 17), at(2026, 9, 7, 3)),
            // "fourth Saturday of August … 1600Z Saturday until 0400Z Sunday."
            ("ohqp", at(2026, 8, 22, 16), at(2026, 8, 23, 4)),
            // 1600 UTC 3 Oct 2026 → 2200 UTC 4 Oct 2026.
            ("cqp", at(2026, 10, 3, 16), at(2026, 10, 4, 22)),
            // "third weekend … from 1400Z on SATURDAY … to 2000Z on SUNDAY" — the
            // OUTER ENVELOPE; the sponsor's 0200Z–1400Z Sunday break is not
            // expressible and is recorded in `_provenance`.
            ("txqp", at(2026, 9, 19, 14), at(2026, 9, 20, 20)),
        ] {
            let w = party(event).event_window(2026);
            assert_eq!(w.start_unix, start, "{event} start");
            assert_eq!(w.end_unix, end, "{event} end");
        }
    }

    /// ⭐⭐ **EVERY SENT SLOT HAS A DECLARED SOURCE** (§2.5, §3.4) — the rule that makes
    /// batch 9 onward genuinely data-only.
    ///
    /// The failure it closes is not hypothetical. Sweepstakes' role sends a `CK` (the
    /// year the operator was first licensed) and a `SEC`; batch 8 added
    /// `contest_qth_*` for the QSO parties and batch 9 added nothing, so both slots
    /// had **no source anywhere in the build and no check said so**. What that looks
    /// like on the day is an operator at 1400 on contest Saturday who cannot fill
    /// their own exchange. With this rule the rules file does not load.
    ///
    /// Each arm below has its positive control on the same file, because a validator
    /// that refused everything would pass the refusals alone.
    #[test]
    fn a_sent_slot_with_no_declared_source_is_refused_and_the_same_file_with_one_loads() {
        // Field Day's own CLASS slot: sent by its one role, sourced from `fd_class`.
        let at = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            f(&mut v);
            parse_spec(&v.to_string())
        };
        let src = |v: &mut serde_json::Value, s: &str| {
            v["rulesets"][0]["exchange"]["fields"][0]["source"] = s.into();
        };

        // (a) No source at all.
        let e = at(&|v| src(v, "")).unwrap_err();
        assert!(
            e.contains("CLASS") && e.contains("no source"),
            "names the slot and what is missing: {e}"
        );

        // (b) A source this build cannot supply — a settings field that does not
        // exist. This is the arm that would have caught the Sweepstakes blocker.
        let e = at(&|v| src(v, "setting:first_licensed_year")).unwrap_err();
        assert!(
            e.contains("CLASS") && e.contains("first_licensed_year"),
            "names the slot and the source it cannot supply: {e}"
        );

        // (c) A derivation this build does not know.
        let e = at(&|v| src(v, "derived:vibes")).unwrap_err();
        assert!(e.contains("derived:vibes"), "{e}");

        // (d) `derived:` is not an escape hatch: a derivation is only usable when
        // every setting it is built FROM is one this build carries.
        assert!(
            SENT_SLOT_DERIVATIONS
                .iter()
                .all(|(_, ins)| ins.iter().all(|i| SENT_SLOT_SETTINGS.contains(i))),
            "every derivation's inputs are declared settings"
        );

        // (e) A slot NO role sends must not claim a source it can never use.
        let e = at(&|v| {
            v["rulesets"][0]["exchange"]["roles"][0]["sends"] = serde_json::json!(["SECTION"]);
            v["rulesets"][0]["exchange"]["roles"][0]["constant_sent"] = serde_json::json!([]);
        })
        .unwrap_err();
        assert!(
            e.contains("CLASS") && e.contains("no role sends it"),
            "a received-only slot declares no source: {e}"
        );

        // POSITIVE CONTROLS — every legal source shape, on the same file.
        for good in [
            "constant",
            "serial",
            "setting:fd_class",
            "setting:contest_check",
            "setting:mygrid",
            "derived:my_location",
        ] {
            assert!(
                at(&|v| src(v, good)).is_ok(),
                "{good} is a source this build can supply"
            );
        }
        // …and the untouched seed, which is the file that actually ships.
        assert!(parse_spec(SEED).is_ok());
    }

    /// ⭐ **`always` may be the LAST role, and only the last** — the relaxation the
    /// QSO parties need, with the rule it relaxes still enforced.
    ///
    /// §2.3's own worked Ohio example ends `role "dx": Always` three roles deep, and
    /// that is the catch-all `ContestSession::role` documents itself as falling
    /// through to; the earlier "must be the ONLY role" refused it. What has not
    /// changed is the reason the rule exists — a role AFTER an unconditional one could
    /// never be reached.
    #[test]
    fn always_may_be_the_last_role_but_never_an_earlier_one() {
        let with_roles = |roles: serde_json::Value| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            v["rulesets"][0]["exchange"]["roles"] = roles;
            parse_spec(&v.to_string())
        };
        let role = |id: &str, sel: serde_json::Value| {
            serde_json::json!({
                "id": id, "selector": sel,
                "sends": ["CLASS", "SECTION"], "receives": ["CLASS", "SECTION"],
                "constant_sent": []
            })
        };
        let loc = serde_json::json!({ "type": "my_location_in", "locations": ["OH"] });
        let always = serde_json::json!({ "type": "always" });

        // Last: legal. This is the shape all four QSO parties ship.
        assert!(with_roles(serde_json::json!([
            role("in_state", loc.clone()),
            role("dx", always.clone())
        ]))
        .is_ok());
        // Only: still legal — both Field Day events.
        assert!(with_roles(serde_json::json!([role("", always.clone())])).is_ok());
        // First of two: refused, and the message says why.
        let e = with_roles(serde_json::json!([
            role("dx", always.clone()),
            role("in_state", loc.clone())
        ]))
        .unwrap_err();
        assert!(e.contains("always") && e.contains("LAST"), "{e}");
        // Middle of three: refused too — "last" is not "not first".
        let e = with_roles(serde_json::json!([
            role("in_state", loc.clone()),
            role("dx", always.clone()),
            role("w_ve", loc.clone())
        ]))
        .unwrap_err();
        assert!(e.contains("always"), "{e}");
    }

    /// A scoring model with no post-multiplier declares no power tiers, and a model
    /// that HAS one still must — both directions, because one is half a test.
    #[test]
    fn the_none_scoring_model_declares_no_power_tiers_and_the_others_still_must() {
        let at = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            f(&mut v);
            parse_spec(&v.to_string())
        };
        // arrlfd is `powered_multiplier`; the QSO parties are `none`.
        let party_ix = |v: &serde_json::Value| {
            v["rulesets"]
                .as_array()
                .unwrap()
                .iter()
                .position(|r| r["event"] == "tnqp")
                .expect("the seed carries tnqp")
        };
        let e = at(&|v| {
            let i = party_ix(v);
            v["rulesets"][i]["scoring"]["power_tiers"] = serde_json::json!([1, 2, 5]);
        })
        .unwrap_err();
        assert!(e.contains("none") && e.contains("power_tiers"), "{e}");
        // The old rule, unchanged, on a model that does apply a power multiplier.
        let e = at(&|v| v["rulesets"][0]["scoring"]["power_tiers"] = serde_json::json!([]))
            .unwrap_err();
        assert!(e.contains("empty power_tiers"), "{e}");
        // POSITIVE CONTROL: the seed as it ships has both shapes in it and loads.
        assert!(parse_spec(SEED).is_ok());
    }

    /// `start_hour_utc` counts from 0000Z on the anchor Saturday and reaches 47, which
    /// is the only way TNQP's Sunday start is expressible. 48 is still refused.
    #[test]
    fn a_window_start_hour_spans_the_weekend_but_stops_at_48() {
        let at = |h: u64| {
            let mut v: serde_json::Value = serde_json::from_str(SEED).unwrap();
            v["rulesets"][0]["window"]["start_hour_utc"] = h.into();
            parse_spec(&v.to_string())
        };
        assert!(at(41).is_ok(), "1700Z on the Sunday");
        assert!(at(47).is_ok());
        assert!(at(48).unwrap_err().contains("start_hour_utc 48"));
    }

    /// ⭐ **Each ruleset round-trips.** The seed is parsed, re-serialised through serde,
    /// and parsed again; the table built from the second pass must be the same table.
    ///
    /// This is not ceremony. `build` LEAKS its way to `&'static` and the exchange block
    /// is the one part of a ruleset with nested, recursive shape (`one_of` arms inside a
    /// slot kind inside a field inside an exchange) — a `KindSpec` arm that serialised
    /// under a name `KindSpec` does not deserialise would be invisible until a rules
    /// PUSH, which is the one path where nothing in this repo would catch it.
    #[test]
    fn every_ruleset_survives_a_serde_round_trip_unchanged() {
        let once: serde_json::Value = serde_json::from_str(SEED).unwrap();
        let twice = serde_json::to_string(&once).expect("the seed re-serialises");
        let a = build(parse_spec(SEED).expect("the seed parses"));
        let b = build(parse_spec(&twice).expect("and parses again after a round trip"));
        assert_eq!(
            a.rulesets.len(),
            12,
            "two Field Day events + four QSO parties + both Sweepstakes weekends + \
             CQ WW's two and CQ WPX's two"
        );
        assert_eq!(a.rulesets.len(), b.rulesets.len());
        for (x, y) in a.rulesets.iter().zip(b.rulesets) {
            assert_eq!(x.event, y.event);
            assert_eq!(x.contest_id, y.contest_id);
            assert_eq!(x.rules_year, y.rules_year);
            assert_eq!(x.scoring, y.scoring, "{}: scoring", x.event);
            assert_eq!(x.dupe_rule, y.dupe_rule, "{}: dupe", x.event);
            assert_eq!(x.window, y.window, "{}: window", x.event);
            assert_eq!(x.score_note_key, y.score_note_key);
            // The exchange, in full: slots (keys, kinds, adif tags, labels) and roles
            // (ids, selectors, send/receive order). `ExchangeSpec` is `PartialEq` all
            // the way down, so this compares the recursive `one_of` arms too.
            assert_eq!(x.exchange, y.exchange, "{}: exchange", x.event);
            assert_eq!(
                x.domains.len(),
                y.domains.len(),
                "{}: domain count",
                x.event
            );
            for (dx, dy) in x.domains.iter().zip(y.domains) {
                assert_eq!(dx, dy, "{}: domain {}", x.event, dx.id);
            }
        }
        // POSITIVE CONTROL. Comparing two builds of the same bytes would pass even if
        // `build` dropped a field entirely, so prove the comparison DISCRIMINATES: a
        // ruleset whose exchange really is different must not compare equal.
        let mut altered = once.clone();
        altered["rulesets"][0]["exchange"]["fields"][0]["required"] = false.into();
        let c = build(parse_spec(&altered.to_string()).expect("still valid"));
        assert_ne!(
            a.rulesets[0].exchange, c.rulesets[0].exchange,
            "the exchange comparison must be able to fail"
        );
    }
}
