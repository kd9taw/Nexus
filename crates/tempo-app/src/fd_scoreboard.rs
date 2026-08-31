//! Field Day spectator scoreboard — a read-only LAN HTTP server for the club TV.
//!
//! Host Nexus serves one self-contained dark big-screen page (embedded at build
//! time, nothing on disk) plus two JSON routes the page polls every 3 s. The
//! server is hand-rolled over `std::net::TcpListener` in the mold of
//! `tempo_audio::rigctld_server` — GET/HEAD only, no bodies read, no keep-alive,
//! `Connection: close` on every reply — so there is no HTTP dependency to
//! license-review and nothing here a request can mutate.
//!
//! **Read-only by construction**: the serve loop takes a [`BoardSource`] — a
//! snapshot-JSON provider — never an engine or app-state handle. There is no
//! code path from a socket to a setter; "never expose rig control" is
//! structural, not filtered. A colocated test pins that this file names no
//! app-state type at all.
//!
//! **Threat model** (stated, not hidden): anyone on the LAN can read the board —
//! the contest log (calls/sections/bands), operator callsigns per position,
//! claimed bonuses. All of it is information already broadcast on the air in
//! cleartext by the very nature of the event; that is why no-auth plain HTTP is
//! acceptable *for this data* and would not be for anything else. A hostile LAN
//! peer can hammer the port: the request-line/header caps, the 5 s read
//! timeout and the ≤ 32-connection ceiling bound the cost — worst case the
//! board goes stale, the radio side is untouched. The feature is default-off;
//! enabling it is the deliberate LAN opt-in.
//!
//! **Scoring honesty**: the score block is computed by replaying the merged
//! rows through [`tempo_core::fieldday::FieldDayLog`] and asking the active
//! [`tempo_core::fd_rules`] ruleset — the same dedupe and the same math every
//! other surface uses, never re-derived. For WFD (`ScoringModel::Objectives`)
//! the payload carries **no power fields at all**, so the page *cannot* render
//! ARRL power math for an event that has none: the headline is raw QSO points
//! and the ×(n+1) projection is a labelled secondary (multipliers apply at
//! submission). WFD bonus points ride along informationally but are not folded
//! into the provisional headline. Golden-payload tests pin both shapes.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tempo_core::fd_rules;
use tempo_core::fieldday::{Exchange, FdEvent, FieldDayLog};

/// The whole spectator page — one self-contained HTML file (inline CSS + JS,
/// no framework, no external reference; a test proves the zero-internet claim).
pub const SCOREBOARD_PAGE: &str = include_str!("../assets/fd_scoreboard.html");

/// What `data.json` answers when this instance has no board to serve — the
/// scoreboard shows real data only on the HOST position (non-host positions
/// hold only a compact club mirror, no per-QSO attribution).
pub const INACTIVE_BODY: &str = r#"{"active":false,"reason":"host-only"}"#;

// Robustness caps (Decision 4): bound what a hostile LAN peer can cost us.
const MAX_REQUEST_LINE: usize = 2048;
const MAX_HEADER_BYTES: usize = 8192;
const MAX_CONNS: usize = 32;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// The seam to the sync layer
// ---------------------------------------------------------------------------

/// One row of the merged event log, as the sync layer hands it over — the
/// reconciled shape from the fd sync design. `posid` is the machine identity
/// (persisted 8-hex per instance); `operator` was stamped at enqueue time, so
/// an operator swap changes subsequent rows, not history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FdBoardRow {
    pub posid: String,
    pub seq: u64,
    pub call: String,
    pub class: String,
    pub section: String,
    pub band: String,
    /// Scoring class: "PH" | "CW" | "DIG".
    pub mode_class: String,
    /// Actual on-air mode behind a "DIG" class ("FT8", "RTTY"…), else empty.
    pub submode: String,
    pub when_unix: u64,
    pub operator: String,
}

/// A known position (identity + label + current operator). Rows referencing a
/// posid absent from this list still count — they render under the raw posid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FdBoardPosition {
    pub id: String,
    pub label: String,
    pub operator: String,
    /// PRESENCE — the band and mode this position last reported it is running,
    /// empty until it has sent one. Not log history: this is where the tent is
    /// sitting right now, which is the question a multi-station club board
    /// exists to answer.
    pub band: String,
    pub mode: String,
    /// Host clock when this position was last heard from on its socket. Never
    /// serialized — it is the input to the payload's `stale` flag (see
    /// [`PositionRow`]).
    pub last_seen_unix: u64,
}

/// The bounded clone the host engine hands the board: identity + settings the
/// score needs + the merged rows. All aggregation (dedupe, counters, buckets)
/// happens HERE, off the engine lock — the engine side is one clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FdBoardData {
    pub event: FdEvent,
    /// The club station identity (call + exchange), for the header.
    pub call: String,
    pub class: String,
    pub section: String,
    /// Stored power multiplier setting — consulted for SFD only; WFD's payload
    /// carries no power fields regardless of what is stored here.
    pub power_mult: u32,
    /// Claimed bonus/objective ids (settings), filtered against the menu.
    pub claimed: Vec<String>,
    pub positions: Vec<FdBoardPosition>,
    pub rows: Vec<FdBoardRow>,
}

/// What the serve loop reads — snapshot JSON and the page, nothing else. The
/// wiring pass implements this over the host engine's `fd_board_snapshot()`
/// via [`CachedBoard`]; tests implement it with fixture strings. Deliberately
/// the ONLY thing [`serve_until`] accepts: the server cannot reach an engine.
pub trait BoardSource: Send + Sync {
    /// Full body for `GET /scoreboard/data.json` (includes `rev`/`now_unix`),
    /// or [`INACTIVE_BODY`] when this instance is not the host.
    fn data(&self) -> String;
    /// Full body for `GET /scoreboard/meta.json`.
    fn meta(&self) -> String;
    /// The page for `GET /` and `GET /<base>`.
    fn page(&self) -> &'static str {
        SCOREBOARD_PAGE
    }
    /// First path segment this source answers on, so a SECOND read-only board can
    /// reuse this server without its URLs claiming to be a scoreboard. `/` always
    /// serves the page whatever this is — a TV browser gets the bare host:port.
    fn base(&self) -> &'static str {
        "scoreboard"
    }
}

// ---------------------------------------------------------------------------
// The cached source: 1 s TTL, rev counter, build off-lock
// ---------------------------------------------------------------------------

struct Cache {
    built: Option<Instant>,
    rev: u64,
    /// The comparable payload (no rev/now_unix) — rev bumps only when it changes.
    core: String,
    data_body: String,
    meta_body: String,
}

/// The standard [`BoardSource`]: wraps a snapshot provider (`None` = not the
/// host / FD not active) behind a 1 s-TTL cache so all concurrent viewers
/// share one build and an idle board costs nothing — no background work with
/// zero viewers, at most one bounded provider call per second with many.
pub struct CachedBoard<F: Fn() -> Option<FdBoardData> + Send + Sync> {
    provider: F,
    ttl: Duration,
    cache: Mutex<Cache>,
}

impl<F: Fn() -> Option<FdBoardData> + Send + Sync> CachedBoard<F> {
    pub fn new(provider: F) -> Self {
        Self::with_ttl(provider, Duration::from_secs(1))
    }

    /// Test seam: a zero TTL rebuilds on every request.
    pub fn with_ttl(provider: F, ttl: Duration) -> Self {
        Self {
            provider,
            ttl,
            cache: Mutex::new(Cache {
                built: None,
                rev: 0,
                core: String::new(),
                data_body: INACTIVE_BODY.to_string(),
                meta_body: INACTIVE_BODY.to_string(),
            }),
        }
    }

    fn refresh(&self) -> std::sync::MutexGuard<'_, Cache> {
        let mut c = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = c.built {
            if at.elapsed() < self.ttl {
                return c;
            }
        }
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match (self.provider)() {
            None => {
                c.core.clear();
                c.data_body = INACTIVE_BODY.to_string();
                c.meta_body = INACTIVE_BODY.to_string();
            }
            Some(d) => {
                // Provider returned its bounded clone; everything below runs
                // with no lock held but ours.
                let core = build_data_core(&d, now_unix);
                if core != c.core {
                    c.rev += 1;
                    c.core = core;
                }
                c.data_body = format!(
                    "{{\"rev\":{},\"now_unix\":{},{}",
                    c.rev,
                    now_unix,
                    &c.core[1..]
                );
                c.meta_body = build_meta(&d, now_unix);
            }
        }
        c.built = Some(Instant::now());
        c
    }
}

impl<F: Fn() -> Option<FdBoardData> + Send + Sync> BoardSource for CachedBoard<F> {
    fn data(&self) -> String {
        self.refresh().data_body.clone()
    }
    fn meta(&self) -> String {
        self.refresh().meta_body.clone()
    }
}

// ---------------------------------------------------------------------------
// The read model (data.json / meta.json)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EventIdent {
    kind: &'static str,
    name: &'static str,
    year: u16,
    start_unix: u64,
    end_unix: u64,
    call: String,
    class: String,
    section: String,
}

#[derive(Serialize)]
struct ScoreBlock {
    model: &'static str,
    qso_points: u32,
    bonus_points: u32,
    /// SFD only — ABSENT for WFD so the page cannot render power math.
    #[serde(skip_serializing_if = "Option::is_none")]
    power_mult: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    powered_points: Option<u32>,
    total: u32,
    /// WFD only: qso_points × (objectives_claimed + 1), labelled provisional.
    #[serde(skip_serializing_if = "Option::is_none")]
    projected_at_submission: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    objectives_claimed: Option<u32>,
}

#[derive(Serialize)]
struct QsoBlock {
    count: u32,
    rate_hour: u32,
    rate_10min: u32,
    /// Counting QSOs per hour since event start (the sparkline).
    hourly: Vec<u32>,
}

#[derive(Serialize)]
struct TickerRow {
    when_unix: u64,
    call: String,
    class: String,
    section: String,
    band: String,
    mode: String,
    submode: String,
    position: String,
    operator: String,
}

#[derive(Serialize)]
struct BandModeRow {
    band: String,
    ph: u32,
    cw: u32,
    dig: u32,
}

#[derive(Serialize)]
struct PositionRow {
    id: String,
    label: String,
    operator: String,
    /// WHERE THIS POSITION IS RIGHT NOW — the band/mode it last reported, or
    /// empty if it never has. Presence, not log history: the club board is
    /// there so the tent can see who is on 20m at this moment.
    band: String,
    mode: String,
    /// The presence above is older than `fdsync::DEAD_SECS` — the position's
    /// link has gone quiet and its band reading is no longer something anyone
    /// has confirmed.
    ///
    /// A FLAG rather than the `last_seen` timestamp on purpose: a timestamp
    /// refreshed by every 5 s heartbeat differs on every build, so it would
    /// bump `rev` — and repaint the whole board, globe included — twelve
    /// times a minute with nothing actually changed.
    stale: bool,
    /// Raw merged-row count — what the position board shows (N3FJP behaviour).
    qsos_raw: u32,
    /// Rows surviving the cross-position (call, band, modeclass) dedupe,
    /// earliest-wins — what the club totals above are built from.
    qsos_unique: u32,
    points: u32,
    last_qso_unix: u64,
}

#[derive(Serialize)]
struct DataCore {
    event: EventIdent,
    score: ScoreBlock,
    qsos: QsoBlock,
    ticker: Vec<TickerRow>,
    band_mode: Vec<BandModeRow>,
    sections_worked: Vec<String>,
    positions: Vec<PositionRow>,
    claimed: Vec<String>,
}

#[derive(Serialize)]
struct MetaSection {
    code: &'static str,
    name: &'static str,
    division: &'static str,
}

#[derive(Serialize)]
struct MetaBonus {
    id: &'static str,
    label: &'static str,
    points: u32,
}

#[derive(Serialize)]
struct MetaDoc {
    event: EventIdent,
    scoring_model: &'static str,
    rules_year: u16,
    rules_generated: &'static str,
    sections: Vec<MetaSection>,
    /// The bonus/objective menu the `claimed` ids index into. WFD reuses the
    /// bonus menu until fd_rules grows a real objectives table — when it does,
    /// the board follows automatically with zero page changes.
    bonuses: Vec<MetaBonus>,
}

/// The events' own names — invariant, never translated (mirrors the UI's
/// `FD_EVENT_NAMES` contract: a translated event name names nothing).
fn event_name(e: FdEvent) -> &'static str {
    match e {
        FdEvent::ArrlFd => "ARRL Field Day",
        FdEvent::WinterFd => "Winter Field Day",
    }
}

fn event_kind(e: FdEvent) -> &'static str {
    match e {
        FdEvent::ArrlFd => "arrlfd",
        FdEvent::WinterFd => "wfd",
    }
}

fn model_tag(s: &fd_rules::ScoringModel) -> &'static str {
    match s {
        fd_rules::ScoringModel::PoweredMultiplier { .. } => "powered",
        fd_rules::ScoringModel::Objectives { .. } => "objectives",
    }
}

fn mode_points_of(s: &fd_rules::ScoringModel) -> fd_rules::ModePoints {
    match s {
        fd_rules::ScoringModel::PoweredMultiplier { points, .. } => *points,
        fd_rules::ScoringModel::Objectives { points, .. } => *points,
    }
}

/// The civil (UTC) year containing a Unix timestamp — Howard Hinnant's civil
/// algorithm, for the display year only (the window itself comes from fd_rules).
fn civil_year_of_unix(t: u64) -> u16 {
    let z = (t / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }) as u16
}

/// Meters for a band label ("160m" → 160.0, "70cm" → 0.7) so the band×mode
/// grid sorts longest wavelength first; unparseable labels sort last, by name.
fn band_meters(band: &str) -> Option<f64> {
    let b = band.trim().to_ascii_lowercase();
    if let Some(cm) = b.strip_suffix("cm") {
        return cm.parse::<f64>().ok().map(|v| v / 100.0);
    }
    b.strip_suffix('m').and_then(|m| m.parse::<f64>().ok())
}

fn event_ident(d: &FdBoardData, rs: &fd_rules::FdRuleset, now_unix: u64) -> EventIdent {
    let w = rs.next_or_running(now_unix);
    EventIdent {
        kind: event_kind(d.event),
        name: event_name(d.event),
        year: civil_year_of_unix(w.start_unix),
        start_unix: w.start_unix,
        end_unix: w.end_unix,
        call: d.call.clone(),
        class: d.class.clone(),
        section: d.section.clone(),
    }
}

/// Build the comparable data payload (everything but `rev`/`now_unix`, which
/// the cache splices in). Public-in-crate so the golden tests hit the exact
/// serialization the wire carries.
pub fn build_data_core(d: &FdBoardData, now_unix: u64) -> String {
    let rs = fd_rules::ruleset(d.event, fd_rules::CURRENT_RULES_YEAR);
    let window = rs.next_or_running(now_unix);

    // Earliest-first replay order defines the cross-position dedupe winner.
    let mut order: Vec<usize> = (0..d.rows.len()).collect();
    order.sort_by_key(|&i| (d.rows[i].when_unix, d.rows[i].seq));

    // Replay through THE FieldDayLog — its (call, band, modeclass) worked-set
    // is the scoring dedupe; fd_rules then scores the surviving log. Never
    // re-derive either.
    let mut log = FieldDayLog::new(&d.call, Exchange::new(&d.class, &d.section), "");
    log.event = d.event;
    let mut unique = vec![false; d.rows.len()];
    for &i in &order {
        let r = &d.rows[i];
        log.band = r.band.trim().to_string();
        unique[i] = log.log_submode_at(
            &r.call,
            &r.class,
            &r.section,
            &r.mode_class,
            &r.submode,
            0,
            r.when_unix,
        );
    }

    let (qso_points, powered) = rs.scoring.qso_and_powered(&log, d.power_mult);
    let menu = if rs.objectives.is_empty() {
        rs.bonuses
    } else {
        rs.objectives
    };
    let claimed: Vec<String> = d
        .claimed
        .iter()
        .filter(|id| menu.iter().any(|b| b.id == *id))
        .cloned()
        .collect();
    let bonus_points = rs.bonus_points(&claimed);
    let score = match rs.scoring {
        fd_rules::ScoringModel::PoweredMultiplier { .. } => ScoreBlock {
            model: model_tag(&rs.scoring),
            qso_points,
            bonus_points,
            // The multiplier as fd_rules actually applied it (legal-snapped);
            // derived from the scored output, not re-snapped here.
            power_mult: Some(if qso_points > 0 {
                powered / qso_points
            } else {
                d.power_mult.max(1)
            }),
            powered_points: Some(powered),
            total: powered + bonus_points,
            projected_at_submission: None,
            objectives_claimed: None,
        },
        fd_rules::ScoringModel::Objectives { .. } => {
            let n = claimed.len() as u32;
            ScoreBlock {
                model: model_tag(&rs.scoring),
                qso_points,
                bonus_points,
                power_mult: None,
                powered_points: None,
                total: qso_points,
                projected_at_submission: Some(qso_points * (n + 1)),
                objectives_claimed: Some(n),
            }
        }
    };

    // Rates + hourly buckets over the COUNTING (deduped) QSOs.
    let counted: Vec<u64> = log.qsos().iter().map(|q| q.when_unix).collect();
    let rate_hour = counted
        .iter()
        .filter(|&&w| w + 3600 > now_unix && w <= now_unix)
        .count() as u32;
    let rate_10min = counted
        .iter()
        .filter(|&&w| w + 600 > now_unix && w <= now_unix)
        .count() as u32;
    let bucket_end = now_unix.min(window.end_unix.saturating_sub(1));
    let hourly = if bucket_end >= window.start_unix {
        let n = ((bucket_end - window.start_unix) / 3600 + 1) as usize;
        let mut buckets = vec![0u32; n];
        for &w in &counted {
            if w >= window.start_unix && w <= bucket_end {
                buckets[((w - window.start_unix) / 3600) as usize] += 1;
            }
        }
        buckets
    } else {
        Vec::new()
    };

    // Positions: seed from the known list (keeps label + current operator),
    // grow for any posid the rows carry that the list does not.
    let labels: HashMap<&str, &FdBoardPosition> =
        d.positions.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut pos: BTreeMap<&str, PositionRow> = d
        .positions
        .iter()
        .map(|p| {
            (
                p.id.as_str(),
                PositionRow {
                    id: p.id.clone(),
                    label: p.label.clone(),
                    operator: p.operator.clone(),
                    band: p.band.clone(),
                    mode: p.mode.clone(),
                    stale: now_unix.saturating_sub(p.last_seen_unix.min(now_unix))
                        > tempo_net::fdsync::DEAD_SECS,
                    qsos_raw: 0,
                    qsos_unique: 0,
                    points: 0,
                    last_qso_unix: 0,
                },
            )
        })
        .collect();
    let points_table = mode_points_of(&rs.scoring);
    for (i, r) in d.rows.iter().enumerate() {
        let e = pos.entry(r.posid.as_str()).or_insert_with(|| PositionRow {
            id: r.posid.clone(),
            label: r.posid.clone(),
            operator: String::new(),
            // A posid the positions list does not know: rows but no presence,
            // so there is no band to claim and nothing to keep fresh.
            band: String::new(),
            mode: String::new(),
            stale: true,
            qsos_raw: 0,
            qsos_unique: 0,
            points: 0,
            last_qso_unix: 0,
        });
        e.qsos_raw += 1;
        e.last_qso_unix = e.last_qso_unix.max(r.when_unix);
        if unique[i] {
            e.qsos_unique += 1;
            e.points += points_table.for_mode(&r.mode_class);
        }
    }
    let mut positions: Vec<PositionRow> = pos.into_values().collect();
    positions.sort_by(|a, b| {
        (b.points, b.qsos_raw, a.label.as_str()).cmp(&(a.points, a.qsos_raw, b.label.as_str()))
    });

    // Band × mode grid over the counting QSOs, longest wavelength first.
    let mut bands: BTreeMap<String, BandModeRow> = BTreeMap::new();
    for q in log.qsos() {
        let e = bands.entry(q.band.clone()).or_insert_with(|| BandModeRow {
            band: q.band.clone(),
            ph: 0,
            cw: 0,
            dig: 0,
        });
        match q.mode.as_str() {
            "PH" | "PHONE" | "SSB" | "FM" => e.ph += 1,
            "CW" => e.cw += 1,
            _ => e.dig += 1,
        }
    }
    let mut band_mode: Vec<BandModeRow> = bands.into_values().collect();
    band_mode.sort_by(|a, b| match (band_meters(&a.band), band_meters(&b.band)) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.band.cmp(&b.band),
    });

    let mut sections_worked: Vec<String> = log
        .worked_sections()
        .into_iter()
        .map(|s| s.to_ascii_uppercase())
        .collect();
    sections_worked.sort();
    sections_worked.dedup();

    // Ticker: last 25 RAW rows (activity, not scoring), newest first, with the
    // position label resolved server-side so the page stays dumb.
    let mut newest: Vec<usize> = (0..d.rows.len()).collect();
    newest.sort_by_key(|&i| std::cmp::Reverse((d.rows[i].when_unix, d.rows[i].seq)));
    let ticker: Vec<TickerRow> = newest
        .into_iter()
        .take(25)
        .map(|i| {
            let r = &d.rows[i];
            TickerRow {
                when_unix: r.when_unix,
                call: r.call.clone(),
                class: r.class.clone(),
                section: r.section.to_ascii_uppercase(),
                band: r.band.clone(),
                mode: r.mode_class.to_ascii_uppercase(),
                submode: r.submode.clone(),
                position: labels
                    .get(r.posid.as_str())
                    .map(|p| p.label.clone())
                    .unwrap_or_else(|| r.posid.clone()),
                operator: r.operator.clone(),
            }
        })
        .collect();

    let core = DataCore {
        event: event_ident(d, rs, now_unix),
        score,
        qsos: QsoBlock {
            count: log.qso_count() as u32,
            rate_hour,
            rate_10min,
            hourly,
        },
        ticker,
        band_mode,
        sections_worked,
        positions,
        claimed,
    };
    serde_json::to_string(&core).expect("board read-model serializes")
}

/// Build `meta.json`: the section universe, the bonus/objective menu and the
/// event identity — all from the ACTIVE fd_rules data, so a rules-year edit
/// flows through to the board with zero page changes.
pub fn build_meta(d: &FdBoardData, now_unix: u64) -> String {
    let rs = fd_rules::ruleset(d.event, fd_rules::CURRENT_RULES_YEAR);
    let menu = if rs.objectives.is_empty() {
        rs.bonuses
    } else {
        rs.objectives
    };
    let meta = MetaDoc {
        event: event_ident(d, rs, now_unix),
        scoring_model: model_tag(&rs.scoring),
        rules_year: rs.rules_year,
        rules_generated: fd_rules::active_generated(),
        sections: fd_rules::sections()
            .iter()
            .map(|s| MetaSection {
                code: s.code,
                name: s.name,
                division: s.division,
            })
            .collect(),
        bonuses: menu
            .iter()
            .map(|b| MetaBonus {
                id: b.id,
                label: b.label,
                points: b.points,
            })
            .collect(),
    };
    serde_json::to_string(&meta).expect("board meta serializes")
}

// ---------------------------------------------------------------------------
// HTTP: parse → route → respond (GET/HEAD only)
// ---------------------------------------------------------------------------

struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: String,
    /// `Allow` header for 405s.
    allow: Option<&'static str>,
}

/// Route one parsed request. Pure — the whole routing table is unit-testable
/// without a socket. Query strings are ignored; unknown path → 404; any method
/// but GET/HEAD → 405.
fn route(method: &str, path: &str, source: &dyn BoardSource) -> Response {
    if method != "GET" && method != "HEAD" {
        return Response {
            status: 405,
            reason: "Method Not Allowed",
            content_type: "text/plain; charset=utf-8",
            body: "GET and HEAD only\n".to_string(),
            allow: Some("GET, HEAD"),
        };
    }
    let path = path.split('?').next().unwrap_or(path);
    let base = source.base();
    let (content_type, body) = match path.trim_end_matches('/') {
        // `/` normalizes to "" here, which is the bare host:port a TV browser gets.
        "" => ("text/html; charset=utf-8", source.page().to_string()),
        p if p == format!("/{base}") => ("text/html; charset=utf-8", source.page().to_string()),
        p if p == format!("/{base}/data.json") => ("application/json", source.data()),
        p if p == format!("/{base}/meta.json") => ("application/json", source.meta()),
        _ => {
            return Response {
                status: 404,
                reason: "Not Found",
                content_type: "text/plain; charset=utf-8",
                body: "not found\n".to_string(),
                allow: None,
            }
        }
    };
    Response {
        status: 200,
        reason: "OK",
        content_type,
        body,
        allow: None,
    }
}

fn bad_request() -> Response {
    Response {
        status: 400,
        reason: "Bad Request",
        content_type: "text/plain; charset=utf-8",
        body: "bad request\n".to_string(),
        allow: None,
    }
}

/// Read one LF-terminated line, refusing past `cap` bytes (the caps are the
/// slow-loris / flood bound). `Ok(None)` = clean EOF before any byte.
fn read_capped_line<R: BufRead>(r: &mut R, cap: usize) -> Result<Option<Vec<u8>>, ()> {
    let mut out = Vec::new();
    loop {
        let buf = r.fill_buf().map_err(|_| ())?;
        if buf.is_empty() {
            // EOF: a partial line is a malformed request, not a line.
            return if out.is_empty() { Ok(None) } else { Err(()) };
        }
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            out.extend_from_slice(&buf[..=pos]);
            r.consume(pos + 1);
            if out.len() > cap {
                return Err(());
            }
            return Ok(Some(out));
        }
        out.extend_from_slice(buf);
        let n = buf.len();
        r.consume(n);
        if out.len() > cap {
            return Err(());
        }
    }
}

/// Read and parse the request line + skim headers (discarded, capped). Returns
/// `(method, path)`; `Err` = answer 400 and close. Bounded: the request line
/// is capped at [`MAX_REQUEST_LINE`], headers at [`MAX_HEADER_BYTES`], and the
/// socket carries a read timeout — a slow peer costs one thread for 5 s.
fn read_request(r: &mut impl BufRead) -> Result<(String, String), ()> {
    let line = read_capped_line(r, MAX_REQUEST_LINE)?.ok_or(())?;
    let text = String::from_utf8_lossy(&line);
    let mut tok = text.split_whitespace();
    let method = tok.next().ok_or(())?.to_string();
    let path = tok.next().ok_or(())?.to_string();
    if !path.starts_with('/') {
        return Err(());
    }
    // Skim headers to the blank line so the peer sees a clean close; never
    // parsed, never a body read (GET/HEAD carry none that we would honor).
    let mut total = 0usize;
    loop {
        match read_capped_line(r, MAX_HEADER_BYTES.saturating_sub(total))? {
            None => break, // EOF after the request line: fine, we have enough
            Some(h) if h == b"\r\n" || h == b"\n" => break,
            Some(h) => total += h.len(),
        }
    }
    Ok((method, path))
}

fn write_response(stream: &mut TcpStream, resp: &Response, head_only: bool) {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        resp.status,
        resp.reason,
        resp.content_type,
        resp.body.len()
    );
    if let Some(allow) = resp.allow {
        out.push_str("Allow: ");
        out.push_str(allow);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    if !head_only {
        out.push_str(&resp.body);
    }
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

/// Serve one connection: one request, one reply, close.
fn serve_connection(stream: TcpStream, source: &dyn BoardSource) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let (resp, head_only) = match read_request(&mut reader) {
        Ok((method, path)) => {
            let head = method == "HEAD";
            (route(&method, &path, source), head)
        }
        Err(()) => (bad_request(), false),
    };
    write_response(&mut writer, &resp, head_only);
}

/// Run the board's accept loop until `shutdown` is set (then the listener
/// drops and the port frees for a rebind) — the rigctld broker's `serve_until`
/// shape: non-blocking accept polled ~5×/s, a thread per connection, and the
/// ≤ [`MAX_CONNS`] ceiling enforced accept-then-close.
pub fn serve_until(listener: TcpListener, source: Arc<dyn BoardSource>, shutdown: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    let live = Arc::new(AtomicUsize::new(0));
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if live.load(Ordering::Relaxed) >= MAX_CONNS {
                    drop(stream); // over the ceiling: accept-then-close
                    continue;
                }
                let _ = stream.set_nonblocking(false); // per-client blocking reads
                live.fetch_add(1, Ordering::Relaxed);
                let s = Arc::clone(&source);
                let l = Arc::clone(&live);
                std::thread::spawn(move || {
                    serve_connection(stream, s.as_ref());
                    l.fetch_sub(1, Ordering::Relaxed);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(_) => break,
        }
    }
    // `listener` drops here → the port is released for a rebind.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::net::TcpStream;

    // -- fixtures ----------------------------------------------------------

    /// A [`BoardSource`] of plain strings — the proof the server needs nothing
    /// but snapshot JSON: this test double has no engine, no state, no lock.
    struct StaticSource {
        data: String,
        meta: String,
    }
    impl BoardSource for StaticSource {
        fn data(&self) -> String {
            self.data.clone()
        }
        fn meta(&self) -> String {
            self.meta.clone()
        }
    }

    #[allow(clippy::too_many_arguments)] // a merged-row literal, one arg per field
    fn row(
        posid: &str,
        seq: u64,
        call: &str,
        section: &str,
        band: &str,
        mode: &str,
        submode: &str,
        when: u64,
        op: &str,
    ) -> FdBoardRow {
        FdBoardRow {
            posid: posid.into(),
            seq,
            call: call.into(),
            class: "2A".into(),
            section: section.into(),
            band: band.into(),
            mode_class: mode.into(),
            submode: submode.into(),
            when_unix: when,
            operator: op.into(),
        }
    }

    /// The shared golden fixture: five merged rows across two positions,
    /// including one CROSS-POSITION dupe (row 4 repeats row 1's
    /// call/band/modeclass from the other position — earliest wins).
    /// Timestamps sit inside the event's 2026 window.
    fn fixture(event: FdEvent) -> (FdBoardData, u64) {
        let rs = fd_rules::ruleset(event, fd_rules::CURRENT_RULES_YEAR);
        let s = rs.event_window(2026).start_unix;
        let d = FdBoardData {
            event,
            call: "W9ABC".into(),
            class: "3A".into(),
            section: "WI".into(),
            power_mult: 2,
            claimed: vec![
                "emergency-power".into(), // 100
                "web-submission".into(),  // 50
                "not-a-real-bonus".into(),
            ],
            positions: vec![
                FdBoardPosition {
                    id: "aaaa1111".into(),
                    label: "CW tent".into(),
                    operator: "W9AAA".into(),
                    band: "20m".into(),
                    mode: "CW".into(),
                    last_seen_unix: s + 7495, // 5 s before "now" — live
                },
                FdBoardPosition {
                    id: "bbbb2222".into(),
                    label: "Phone tent".into(),
                    operator: "W9BBB".into(),
                    band: "40m".into(),
                    mode: "PH".into(),
                    last_seen_unix: s + 7000, // 500 s — the link has gone quiet
                },
            ],
            rows: vec![
                row(
                    "aaaa1111",
                    1,
                    "K1ABC",
                    "EMA",
                    "20m",
                    "CW",
                    "",
                    s + 100,
                    "W9AAA",
                ),
                row(
                    "bbbb2222",
                    1,
                    "K1ABC",
                    "EMA",
                    "20m",
                    "PH",
                    "",
                    s + 200,
                    "W9BBB",
                ),
                row(
                    "bbbb2222",
                    2,
                    "W5XYZ",
                    "STX",
                    "40m",
                    "PH",
                    "",
                    s + 3700,
                    "W9BBB",
                ),
                // Cross-position dupe of row 1 — logged later, must NOT count.
                row(
                    "bbbb2222",
                    3,
                    "K1ABC",
                    "EMA",
                    "20m",
                    "CW",
                    "",
                    s + 4000,
                    "W9BBB",
                ),
                row(
                    "aaaa1111",
                    2,
                    "VE3AAA",
                    "ONE",
                    "20m",
                    "DIG",
                    "FT8",
                    s + 7000,
                    "W9AAA",
                ),
            ],
        };
        (d, s + 7500) // "now": 2 h 5 min into the event
    }

    fn parse(core: &str) -> serde_json::Value {
        serde_json::from_str(core).expect("core payload is JSON")
    }

    // -- golden payloads (fd_rules math, pinned) ---------------------------

    /// SFD: qso_pts × power + bonus, exactly as fd_rules scores the replayed
    /// log. Unique rows: CW 2 + PH 1 + PH 1 + DIG 2 = 6 pts; ×2 power = 12;
    /// + 150 bonus (100 + 50; the unknown id scores nothing) = 162.
    #[test]
    fn golden_sfd_payload() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        assert_eq!(v["score"]["model"], "powered");
        assert_eq!(v["score"]["qso_points"], 6);
        assert_eq!(v["score"]["power_mult"], 2);
        assert_eq!(v["score"]["powered_points"], 12);
        assert_eq!(v["score"]["bonus_points"], 150);
        assert_eq!(v["score"]["total"], 162);
        assert!(v["score"].get("projected_at_submission").is_none());
        // The unknown claimed id was filtered out of the payload.
        assert_eq!(
            v["claimed"],
            serde_json::json!(["emergency-power", "web-submission"])
        );
        // Event identity flows from the ruleset's window, not page date math.
        let rs = fd_rules::ruleset(FdEvent::ArrlFd, fd_rules::CURRENT_RULES_YEAR);
        let w = rs.event_window(2026);
        assert_eq!(v["event"]["kind"], "arrlfd");
        assert_eq!(v["event"]["name"], "ARRL Field Day");
        assert_eq!(v["event"]["year"], 2026);
        assert_eq!(v["event"]["start_unix"], w.start_unix);
        assert_eq!(v["event"]["end_unix"], w.end_unix);
        assert_eq!(v["event"]["call"], "W9ABC");
    }

    /// THE scoring-honesty pin: the WFD payload contains NO power fields at
    /// all — not as nulls, not as zeros, ABSENT — so the page cannot render
    /// ARRL power math for an event that has none. The SFD control below
    /// proves the same serializer DOES emit the fields when the model has
    /// them (a absence claim needs a positive control).
    #[test]
    fn golden_wfd_payload_has_no_power_fields() {
        let (d, now) = fixture(FdEvent::WinterFd);
        let json = build_data_core(&d, now);
        assert!(
            !json.contains("power_mult") && !json.contains("powered_points"),
            "WFD payload must carry no power fields: {json}"
        );
        let v = parse(&json);
        assert_eq!(v["score"]["model"], "objectives");
        assert_eq!(v["score"]["qso_points"], 6);
        // Headline is RAW qso points; the ×(n+1) projection is a labelled
        // secondary (n = 2 valid claimed objectives → ×3).
        assert_eq!(v["score"]["total"], 6);
        assert_eq!(v["score"]["objectives_claimed"], 2);
        assert_eq!(v["score"]["projected_at_submission"], 18);
        assert_eq!(v["score"]["bonus_points"], 150);
        assert_eq!(v["event"]["kind"], "wfd");
        assert_eq!(v["event"]["name"], "Winter Field Day");

        // POSITIVE CONTROL: the SFD payload from the same fixture DOES carry
        // both fields — the absence above is the model, not a serializer bug.
        let (sfd, sfd_now) = fixture(FdEvent::ArrlFd);
        let control = build_data_core(&sfd, sfd_now);
        assert!(control.contains("\"power_mult\":"));
        assert!(control.contains("\"powered_points\":"));
    }

    /// WHO IS ON WHAT BAND. The in-app club board has carried each position's
    /// band and mode from the start; the spectator page showed label,
    /// operator, QSOs and points only — so the one board the whole tent can
    /// see could not answer the question a multi-station club runs the board
    /// for (operator report, 2026-08-30: "programs like N3FJP has a board to
    /// show where everyone is in on the bands").
    #[test]
    fn positions_say_which_band_and_mode_each_one_is_running() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        let pos = v["positions"].as_array().unwrap();
        assert_eq!(pos[0]["label"], "CW tent");
        assert_eq!(pos[0]["band"], "20m");
        assert_eq!(pos[0]["mode"], "CW");
        assert_eq!(pos[1]["label"], "Phone tent");
        assert_eq!(pos[1]["band"], "40m");
        assert_eq!(pos[1]["mode"], "PH");
    }

    /// …and the reading EXPIRES. Presence is not log history: a band a
    /// position last confirmed longer ago than the sync layer's own dead
    /// timeout is not where that tent is now, and a TV asserting it for the
    /// rest of the event is worse than saying nothing.
    ///
    /// Shipped as a FLAG, not as the `last_seen` timestamp, deliberately: a
    /// timestamp refreshed by every 5 s heartbeat would differ on every build
    /// and so bump `rev` twelve times a minute with nothing actually changed,
    /// repainting the whole board (globe included) each time.
    #[test]
    fn a_band_reading_nobody_has_confirmed_lately_is_marked_stale() {
        let (mut d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        assert_eq!(v["positions"][0]["stale"], false, "heard from 5 s ago");
        assert_eq!(v["positions"][1]["stale"], true, "last heard 500 s ago");
        // The boundary is fdsync's dead timeout itself, never a second
        // opinion about it — the in-app board marks the same rows.
        d.positions[0].last_seen_unix = now - tempo_net::fdsync::DEAD_SECS;
        assert_eq!(
            parse(&build_data_core(&d, now))["positions"][0]["stale"],
            false,
            "exactly at the timeout is still live"
        );
        d.positions[0].last_seen_unix = now - tempo_net::fdsync::DEAD_SECS - 1;
        assert_eq!(
            parse(&build_data_core(&d, now))["positions"][0]["stale"],
            true,
            "one second past it is not"
        );
    }

    // -- aggregation -------------------------------------------------------

    #[test]
    fn cross_position_dupe_earliest_wins_and_positions_carry_both_counts() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        // Club totals are DEDUPED: 5 raw rows, 4 counting QSOs.
        assert_eq!(v["qsos"]["count"], 4);
        let pos = v["positions"].as_array().unwrap();
        // Sorted by points desc: CW tent (2 unique × 2 pts = 4) first.
        assert_eq!(pos[0]["label"], "CW tent");
        assert_eq!(pos[0]["operator"], "W9AAA");
        assert_eq!(pos[0]["qsos_raw"], 2);
        assert_eq!(pos[0]["qsos_unique"], 2);
        assert_eq!(pos[0]["points"], 4);
        // Phone tent logged the dupe: 3 raw, 2 unique (the earliest logger of
        // K1ABC/20m/CW — the CW tent — keeps the credit).
        assert_eq!(pos[1]["label"], "Phone tent");
        assert_eq!(pos[1]["qsos_raw"], 3);
        assert_eq!(pos[1]["qsos_unique"], 2);
        assert_eq!(pos[1]["points"], 2);
        assert_eq!(pos[1]["last_qso_unix"], d.rows[3].when_unix);
    }

    #[test]
    fn band_mode_grid_counts_unique_and_sorts_longest_wavelength_first() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        let bm = v["band_mode"].as_array().unwrap();
        assert_eq!(bm.len(), 2);
        assert_eq!(bm[0]["band"], "40m"); // 40 m before 20 m
        assert_eq!(bm[0]["ph"], 1);
        assert_eq!(bm[1]["band"], "20m");
        assert_eq!(bm[1]["ph"], 1);
        assert_eq!(bm[1]["cw"], 1); // the dupe did not double CW
        assert_eq!(bm[1]["dig"], 1);
    }

    #[test]
    fn sections_worked_sorted_upper() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        assert_eq!(
            v["sections_worked"],
            serde_json::json!(["EMA", "ONE", "STX"])
        );
    }

    #[test]
    fn rates_and_hourly_buckets() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        // now = start + 7500 s → three hourly buckets. Counting QSOs at
        // +100/+200 (bucket 0) and +3700/+7000 (bucket 1); the +4000 dupe
        // does not count.
        assert_eq!(v["qsos"]["hourly"], serde_json::json!([2, 2, 0]));
        // Last hour (> +3900): only +7000. Last 10 min (> +6900): the same.
        assert_eq!(v["qsos"]["rate_hour"], 1);
        assert_eq!(v["qsos"]["rate_10min"], 1);
    }

    #[test]
    fn ticker_newest_first_with_position_attribution_and_cap_25() {
        let (mut d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_data_core(&d, now));
        let t = v["ticker"].as_array().unwrap();
        assert_eq!(t.len(), 5);
        assert_eq!(t[0]["call"], "VE3AAA");
        assert_eq!(t[0]["position"], "CW tent"); // posid resolved to label
        assert_eq!(t[0]["operator"], "W9AAA");
        assert_eq!(t[0]["submode"], "FT8");
        assert_eq!(t[4]["call"], "K1ABC");
        // Truncation: 40 more raw rows → exactly 25 ticker entries, and a row
        // whose posid is unknown falls back to the raw posid as its label.
        let s = d.rows[0].when_unix;
        for i in 0..40 {
            d.rows.push(row(
                "cccc3333",
                10 + i,
                &format!("N{i}XX"),
                "WI",
                "80m",
                "CW",
                "",
                s + 8000 + i,
                "W9CCC",
            ));
        }
        let v = parse(&build_data_core(&d, now));
        let t = v["ticker"].as_array().unwrap();
        assert_eq!(t.len(), 25);
        assert_eq!(t[0]["position"], "cccc3333");
    }

    // -- meta.json ---------------------------------------------------------

    #[test]
    fn meta_carries_the_section_universe_and_bonus_menu_from_the_rules_data() {
        let (d, now) = fixture(FdEvent::ArrlFd);
        let v = parse(&build_meta(&d, now));
        let secs = v["sections"].as_array().unwrap();
        assert_eq!(secs.len(), fd_rules::sections().len());
        assert_eq!(secs.len(), 83); // 71 US ARRL + 12 RAC
        assert!(secs[0]["code"].is_string() && secs[0]["division"].is_string());
        let menu = v["bonuses"].as_array().unwrap();
        let rs = fd_rules::ruleset(FdEvent::ArrlFd, fd_rules::CURRENT_RULES_YEAR);
        assert_eq!(menu.len(), rs.bonuses.len());
        assert_eq!(v["scoring_model"], "powered");
        assert_eq!(v["rules_year"], rs.rules_year);
        // WFD tags its model; its menu is the bonus menu until fd_rules grows
        // a real objectives table (then this follows automatically).
        let (w, wnow) = fixture(FdEvent::WinterFd);
        let mv = parse(&build_meta(&w, wnow));
        assert_eq!(mv["scoring_model"], "objectives");
        assert!(!mv["bonuses"].as_array().unwrap().is_empty());
    }

    // -- the cached source -------------------------------------------------

    #[test]
    fn cache_shares_one_build_within_ttl() {
        use std::sync::atomic::AtomicU32;
        let calls = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&calls);
        let (d, _) = fixture(FdEvent::ArrlFd);
        let board = CachedBoard::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            Some(d.clone())
        });
        let a = board.data();
        let b = board.data();
        let m = board.meta();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one build serves them all");
        assert_eq!(a, b);
        assert!(m.contains("\"sections\""));
    }

    #[test]
    fn rev_bumps_only_when_the_payload_changes_and_now_unix_rides_along() {
        use std::sync::atomic::AtomicU32;
        let extra = Arc::new(AtomicU32::new(0));
        let e = Arc::clone(&extra);
        let (d, _) = fixture(FdEvent::ArrlFd);
        let board = CachedBoard::with_ttl(
            move || {
                let mut d = d.clone();
                let s = d.rows[0].when_unix;
                for i in 0..e.load(Ordering::SeqCst) as u64 {
                    d.rows.push(row(
                        "aaaa1111",
                        100 + i,
                        &format!("A{i}B"),
                        "WI",
                        "80m",
                        "CW",
                        "",
                        s + 300 + i,
                        "W9AAA",
                    ));
                }
                Some(d)
            },
            Duration::ZERO,
        );
        let v1 = parse(&board.data());
        let v2 = parse(&board.data());
        assert_eq!(v1["rev"], v2["rev"], "unchanged data must not bump rev");
        assert!(v2["now_unix"].as_u64().unwrap() > 0);
        extra.fetch_add(1, Ordering::SeqCst);
        let v3 = parse(&board.data());
        assert_ne!(v1["rev"], v3["rev"], "a new QSO must bump rev");
    }

    #[test]
    fn non_host_serves_the_inactive_body_on_both_json_routes() {
        let board = CachedBoard::new(|| None);
        assert_eq!(board.data(), INACTIVE_BODY);
        assert_eq!(board.meta(), INACTIVE_BODY);
        let v: serde_json::Value = serde_json::from_str(INACTIVE_BODY).unwrap();
        assert_eq!(v["active"], false);
        assert_eq!(v["reason"], "host-only");
    }

    // -- HTTP: parse + route -----------------------------------------------

    fn src() -> StaticSource {
        StaticSource {
            data: r#"{"rev":1}"#.into(),
            meta: r#"{"sections":[]}"#.into(),
        }
    }

    #[test]
    fn routes_page_data_meta_404_and_query_strings() {
        let s = src();
        for path in ["/", "/scoreboard", "/scoreboard/"] {
            let r = route("GET", path, &s);
            assert_eq!(r.status, 200, "{path}");
            assert!(r.content_type.starts_with("text/html"));
            assert_eq!(r.body, SCOREBOARD_PAGE);
        }
        let r = route("GET", "/scoreboard/data.json", &s);
        assert_eq!((r.status, r.body.as_str()), (200, r#"{"rev":1}"#));
        assert_eq!(r.content_type, "application/json");
        let r = route("GET", "/scoreboard/meta.json?ts=123", &s);
        assert_eq!((r.status, r.body.as_str()), (200, r#"{"sections":[]}"#));
        for path in ["/nope", "/scoreboard/other.json", "/../etc/passwd"] {
            assert_eq!(route("GET", path, &s).status, 404, "{path}");
        }
    }

    /// Anything but GET/HEAD → 405 with an Allow header. There are no write
    /// routes to reach — but a writing METHOD must still be refused loudly.
    #[test]
    fn writes_are_405_read_only() {
        let s = src();
        for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS"] {
            let r = route(method, "/scoreboard/data.json", &s);
            assert_eq!(r.status, 405, "{method}");
            assert_eq!(r.allow, Some("GET, HEAD"));
        }
        assert_eq!(route("HEAD", "/scoreboard", &s).status, 200);
    }

    #[test]
    fn request_parsing_enforces_the_caps() {
        // Well-formed.
        let mut ok = &b"GET /scoreboard HTTP/1.1\r\nHost: x\r\nAccept: */*\r\n\r\n"[..];
        assert_eq!(
            read_request(&mut ok).unwrap(),
            ("GET".to_string(), "/scoreboard".to_string())
        );
        // Oversized request line → refused.
        let big = format!("GET /{} HTTP/1.1\r\n\r\n", "x".repeat(MAX_REQUEST_LINE));
        assert!(read_request(&mut big.as_bytes()).is_err());
        // Header flood past the cap → refused.
        let flood = format!(
            "GET / HTTP/1.1\r\n{}\r\n",
            "X-Pad: yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\r\n".repeat(400)
        );
        assert!(read_request(&mut flood.as_bytes()).is_err());
        // Garbage: no path, or a path not starting with '/'.
        assert!(read_request(&mut &b"GARBAGE\r\n\r\n"[..]).is_err());
        assert!(read_request(&mut &b"GET example HTTP/1.1\r\n\r\n"[..]).is_err());
        assert!(read_request(&mut &b""[..]).is_err());
    }

    // -- the page's own invariants -----------------------------------------

    fn has_external_url(s: &str) -> bool {
        s.contains("http://") || s.contains("https://")
    }

    /// The zero-internet proof: nothing in the page references an external
    /// host — it must render on a LAN with no route to the world. The control
    /// shows the checker actually catches a URL.
    #[test]
    fn page_references_nothing_external() {
        assert!(
            !has_external_url(SCOREBOARD_PAGE),
            "the board page must be fully self-contained"
        );
        // POSITIVE CONTROL: the checker fires on a real external reference.
        assert!(has_external_url("see https://example.com for details"));
        assert!(has_external_url("http://192.168.1.20:7373/scoreboard"));
    }

    /// The page talks to exactly the two JSON routes this module serves.
    #[test]
    fn page_fetches_only_the_board_routes() {
        let mut found = 0;
        for (i, _) in SCOREBOARD_PAGE.match_indices("fetch(") {
            let after = &SCOREBOARD_PAGE[i + "fetch(".len()..];
            assert!(
                after.starts_with("'/scoreboard/data.json'")
                    || after.starts_with("'/scoreboard/meta.json'"),
                "unexpected fetch target near byte {i}"
            );
            found += 1;
        }
        assert_eq!(found, 2, "one data fetch + one meta fetch");
    }

    /// The locale seam and the test seam the jsdom suite drives.
    #[test]
    fn page_exposes_strings_and_pure_render_functions() {
        assert!(SCOREBOARD_PAGE.contains("var STRINGS"));
        assert!(SCOREBOARD_PAGE.contains("function render(data, meta)"));
        assert!(SCOREBOARD_PAGE.contains("window.__fdboard"));
        assert!(SCOREBOARD_PAGE.contains("__FDBOARD_TEST__"));
        assert!(SCOREBOARD_PAGE.contains("<title>Field Day Scoreboard</title>"));
    }

    /// Read-only BY CONSTRUCTION: this module never names the engine or app
    /// state — the server cannot reach a setter because it cannot reach the
    /// types that have them. Source-scan with a positive control (lib.rs DOES
    /// name the app-state type, so the scan reads real content).
    #[test]
    fn module_source_never_touches_engine_or_app_state() {
        let me =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/fd_scoreboard.rs"))
                .unwrap();
        // Needles assembled at runtime so this test's own source can't match.
        let engine_use = ["use crate::", "engine"].concat();
        let app_state = ["App", "State"].concat();
        assert!(!me.contains(&engine_use));
        assert!(!me.contains(&app_state));
        // POSITIVE CONTROL: the same scan on lib.rs finds the app-state type.
        let lib =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
        assert!(lib.contains(&app_state));
    }

    // -- localhost integration (the rigctld_server pattern) ----------------

    fn talk(addr: std::net::SocketAddr, request: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn serves_page_and_json_over_a_real_socket_then_frees_the_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let sd = Arc::clone(&shutdown);
        let source: Arc<dyn BoardSource> = Arc::new(src());
        std::thread::spawn(move || serve_until(listener, source, sd));

        let page = talk(addr, "GET /scoreboard HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(page.contains("Connection: close"));
        assert!(page.contains("<title>Field Day Scoreboard</title>"));

        let data = talk(addr, "GET /scoreboard/data.json HTTP/1.1\r\n\r\n");
        assert!(data.ends_with(r#"{"rev":1}"#));

        let meta = talk(addr, "GET /scoreboard/meta.json HTTP/1.1\r\n\r\n");
        assert!(meta.ends_with(r#"{"sections":[]}"#));

        // HEAD: full headers (with the GET's Content-Length), empty body.
        let head = talk(addr, "HEAD /scoreboard/data.json HTTP/1.1\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains(&format!("Content-Length: {}", r#"{"rev":1}"#.len())));
        assert!(
            head.ends_with("\r\n\r\n"),
            "HEAD must carry no body: {head:?}"
        );

        let post = talk(addr, "POST /scoreboard/data.json HTTP/1.1\r\n\r\n");
        assert!(post.starts_with("HTTP/1.1 405 "));
        assert!(post.contains("Allow: GET, HEAD"));

        let missing = talk(addr, "GET /nope HTTP/1.1\r\n\r\n");
        assert!(missing.starts_with("HTTP/1.1 404 "));

        // An oversized request line is refused, not served.
        let huge = format!("GET /{} HTTP/1.1\r\n\r\n", "z".repeat(4000));
        assert!(talk(addr, &huge).starts_with("HTTP/1.1 400 "));

        // A few concurrent viewers all get answers.
        let workers: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || talk(addr, "GET /scoreboard/data.json HTTP/1.1\r\n\r\n"))
            })
            .collect();
        for w in workers {
            assert!(w.join().unwrap().starts_with("HTTP/1.1 200 OK\r\n"));
        }

        // Shutdown releases the port for a rebind (the hot-apply loop's need).
        shutdown.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match TcpListener::bind(addr) {
                Ok(_) => break,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("port never freed after shutdown: {e}"),
            }
        }
    }
}
