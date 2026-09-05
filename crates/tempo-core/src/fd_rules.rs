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
//! `rules_year` stamps each ruleset; the pinned per-event score fixtures in the
//! tests below run against [`ruleset`] = the BUNDLED SEED (an installed file is
//! invisible to them by design — its visibility to the operator is the status
//! surface and the Cabrillo `X-NEXUS-RULES-YEAR` header), so they fail if a
//! seed edit changes a score without a matching test update, catching drift
//! before it ships.

use crate::fieldday::{FdEvent, FieldDayLog};
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

/// The exchange both events use today: a transmitter Class (e.g. `3A`) and an
/// ARRL/RAC Section (e.g. `WI`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeSpec {
    pub class_label: &'static str,
    pub section_label: &'static str,
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

/// The dupe key both events use today: a station counts once per (call, band,
/// mode-class). Descriptive metadata — the check itself is enforced in
/// [`FieldDayLog`](crate::fieldday::FieldDayLog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupeRule {
    pub by_call: bool,
    pub by_band: bool,
    pub by_mode_class: bool,
}

/// A time window for one occurrence of an event (Unix seconds, UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventWindow {
    pub start_unix: u64,
    pub end_unix: u64,
}

/// Per-mode-class QSO points (the `points_by_mode_class` table in the data).
/// The seed matches the historical hardcoded map (phone 1, CW/digital 2); a
/// data edit here provably changes computed scores (the install integration
/// test's whole point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModePoints {
    pub ph: u32,
    pub cw: u32,
    pub dig: u32,
}

impl ModePoints {
    /// Points for one logged mode class — the same normalization as the legacy
    /// [`qso_points_for_mode`](crate::fieldday::qso_points_for_mode) (which
    /// keeps serving the per-QSO interop push with the historical constants).
    pub fn for_mode(&self, mode: &str) -> u32 {
        match mode.to_ascii_uppercase().as_str() {
            "PH" | "PHONE" | "SSB" | "FM" => self.ph,
            "CW" => self.cw,
            _ => self.dig, // digital
        }
    }
}

/// How an event turns a log into a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoringModel {
    /// ARRL Field Day: sum the per-mode QSO points (from the ruleset's
    /// [`ModePoints`] table — seed: phone 1, CW/digital 2), multiply by a legal
    /// power tier, then add claimed bonus points. This is the exact math the
    /// engine used inline before centralization.
    PoweredMultiplier {
        power_tiers: &'static [u32],
        points: ModePoints,
    },
    /// Winter Field Day: QSOs × (objectives + 1). Objective multipliers are
    /// applied at submission, so with no objective values in the data yet this
    /// scores RAW QSO points on the air and flags that the total is provisional
    /// (`multipliers_at_submission`). See the module-level concerns.
    Objectives {
        multipliers_at_submission: bool,
        points: ModePoints,
    },
}

impl ScoringModel {
    /// `(qso_pts, powered_pts)` for this log at the given stored power tier.
    /// `PoweredMultiplier` snaps the tier to a legal value and multiplies;
    /// `Objectives` returns `powered == qso_pts` (no on-air power multiplier).
    pub fn qso_and_powered(&self, log: &FieldDayLog, power_mult: u32) -> (u32, u32) {
        match self {
            ScoringModel::PoweredMultiplier {
                power_tiers,
                points,
            } => {
                let qso_pts = points_total(log, points);
                (qso_pts, qso_pts * legal_power(power_tiers, power_mult))
            }
            ScoringModel::Objectives { points, .. } => {
                let qso_pts = points_total(log, points);
                (qso_pts, qso_pts)
            }
        }
    }
}

/// Sum the log's QSO points from the ruleset's data-carried table (the seed
/// matches `FieldDayLog::qso_points()`'s historical constants; an installed
/// file's table wins here, which is what makes the parameters DATA).
fn points_total(log: &FieldDayLog, p: &ModePoints) -> u32 {
    log.qsos().iter().map(|q| p.for_mode(&q.mode)).sum()
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
    pub exchange: ExchangeSpec,
    pub scoring: ScoringModel,
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

/// Snap a stored power multiplier to the highest legal tier ≤ `v` (or the
/// smallest tier). Replaces the engine's old `legal_fd_power` for the ARRL
/// `{1, 2, 5}` tiers — a hand-edited settings file must never score with a
/// ×3/×4 that isn't a real tier.
fn legal_power(tiers: &[u32], v: u32) -> u32 {
    tiers
        .iter()
        .rev()
        .copied()
        .find(|&t| v >= t)
        .unwrap_or_else(|| tiers.first().copied().unwrap_or(1))
}

const EXCHANGE_CLASS_SECTION: ExchangeSpec = ExchangeSpec {
    class_label: "Class",
    section_label: "Section",
};

const DUPE_CALL_BAND_MODE: DupeRule = DupeRule {
    by_call: true,
    by_band: true,
    by_mode_class: true,
};

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
    scoring: String,
    points_by_mode_class: BTreeMap<String, u32>,
    power_tiers: Vec<u32>,
    bonuses: Vec<BonusSpec>,
    banned_modes: Vec<String>,
    tempo_fd: bool,
    assistance: AssistanceSpec,
    enforcement: String,
    #[serde(default)]
    objectives: Vec<BonusSpec>,
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

/// Parse + the structural validation both the loader and the download client
/// run (and the publish workflow re-runs in node — keep the two in step).
fn parse_spec(text: &str) -> Result<FileSpec, String> {
    let spec: FileSpec = serde_json::from_str(text).map_err(|e| format!("bad JSON: {e}"))?;
    if spec.schema != 1 {
        return Err(format!(
            "schema {} (this build reads schema 1)",
            spec.schema
        ));
    }
    if spec.generated.is_empty() {
        return Err("empty `generated` stamp".into());
    }
    for want in ["arrlfd", "wfd"] {
        if !spec.rulesets.iter().any(|r| r.event == want) {
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
        if !matches!(r.scoring.as_str(), "powered_multiplier" | "objectives") {
            return Err(format!("{tag}: unknown scoring model {:?}", r.scoring));
        }
        for k in ["PH", "CW", "DIG"] {
            if !r.points_by_mode_class.contains_key(k) {
                return Err(format!("{tag}: points_by_mode_class misses {k}"));
            }
        }
        if r.power_tiers.is_empty() {
            return Err(format!("{tag}: empty power_tiers"));
        }
        if !r.power_tiers.windows(2).all(|w| w[0] < w[1]) {
            return Err(format!("{tag}: power_tiers not strictly ascending"));
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

/// One-time at load (validated spec in, `&'static` table out) — the leak IS
/// the lifetime strategy: every consumer keeps its `&'static` field types.
fn build(spec: FileSpec) -> RulesTable {
    let rulesets: Vec<&'static FdRuleset> = spec
        .rulesets
        .into_iter()
        .map(|r| {
            let points = ModePoints {
                ph: r.points_by_mode_class["PH"],
                cw: r.points_by_mode_class["CW"],
                dig: r.points_by_mode_class["DIG"],
            };
            let power_tiers: &'static [u32] = Box::leak(r.power_tiers.into_boxed_slice());
            let scoring = match r.scoring.as_str() {
                "powered_multiplier" => ScoringModel::PoweredMultiplier {
                    power_tiers,
                    points,
                },
                _ => ScoringModel::Objectives {
                    multipliers_at_submission: true,
                    points,
                },
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
            &*Box::leak(Box::new(FdRuleset {
                event: event_of(&r.event).expect("validated"),
                rules_year: r.rules_year,
                contest_id: leak_str(r.contest_id),
                exchange: EXCHANGE_CLASS_SECTION,
                scoring,
                bonuses: leak_bonuses(r.bonuses),
                dupe_rule: DUPE_CALL_BAND_MODE,
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
        sections: Box::leak(
            spec.sections
                .into_iter()
                .map(|s| Section {
                    code: leak_str(s.code),
                    name: leak_str(s.name),
                    division: leak_str(s.division),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
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
    use crate::fieldday::{Exchange, FieldDayLog};

    /// Build a log from `(call, mode-class)` pairs — distinct calls so nothing
    /// dupes; class/section are constant (irrelevant to the point math).
    fn log_with(contacts: &[(&str, &str)]) -> FieldDayLog {
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
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
        let (qso_pts, powered) = rs.scoring.qso_and_powered(&log, 2);
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
        let (qso_pts, powered) = rs.scoring.qso_and_powered(&log, 5);
        assert_eq!((qso_pts, powered), (5, 5), "raw points, power tier ignored");
        assert!(matches!(
            rs.scoring,
            ScoringModel::Objectives {
                multipliers_at_submission: true,
                ..
            }
        ));
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
    fn legal_power_snaps_to_arrl_tiers() {
        let tiers = &[1u32, 2, 5][..];
        // Matches the engine's old `legal_fd_power`: ≥5→5, ≥2→2, else 1.
        for (v, want) in [
            (0, 1),
            (1, 1),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 5),
            (6, 5),
            (150, 5),
        ] {
            assert_eq!(legal_power(tiers, v), want, "power {v}");
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
        assert!(
            corrupt(&|v| v["schema"] = 2.into())
                .unwrap_err()
                .contains("schema"),
            "wrong schema version"
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
            corrupt(&|v| v["rulesets"][0]["power_tiers"] = serde_json::json!([]))
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
            &|v| v["rulesets"][0]["points_by_mode_class"] = serde_json::json!({"PH": 1})
        )
        .unwrap_err()
        .contains("points_by_mode_class"),);
        assert!(validate("not json").is_err(), "garbage is refused");
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
