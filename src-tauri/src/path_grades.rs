//! Your contacts' paths — which way round each logged contact's signal most likely went, graded
//! from the propagation model and the station's own evidence ([`propagation::contact_path`]).
//!
//! Two halves, with opposite rules about the log:
//!
//! 1. **A NEW contact is graded as it is logged** ([`at_log`], the engine's path grader). It reads
//!    only what this module already holds — the path card computed for that station
//!    ([`keep_outlook`]) and the PSK Reporter spots — because it runs under the Engine lock: no
//!    prediction, no disk. The record gains the grade fields only
//!    (`tempo_core::logbook::path_stamp`), never `ANT_AZ` or `ANT_PATH`. A contact with no card
//!    for its station logs with no grade; the second half grades it for display. A contact that
//!    did not go by the ionosphere ([`ionospheric`]: satellite, EME, a repeater …) is never
//!    graded.
//! 2. **The existing log is graded for DISPLAY ONLY** ([`capture`] then [`run_pass`]). ⛔ It
//!    never writes a contact: the grades live in this module's own file (`path_grades.jsonl`
//!    beside `settings.json`), keyed by what a grade depends on — the two grid squares, the band,
//!    the month and hour, and the sunspot number — so an edited contact simply looks up another
//!    cell. The pass takes the Engine lock once, for the log's read handle and the settings, and
//!    then holds no reference to the Engine at all ([`Job`] cannot name it), so grading cannot
//!    take the lock by construction. It grades in chunks, appending each chunk to the file, so a
//!    pass cut short by a quit resumes where it stopped: a cell already in the file is never
//!    graded again.
//!
//! The Logbook asks for the grade of the rows it shows ([`view_for`]): the contact's own stamp
//! when it carries one made for its current grid and band, else the display-only cell. The
//! expert's validation counters ([`propagation::contact_path::ValidationTally`]) are counted at
//! the end of each pass over the last 30 days.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use propagation::contact_path::{self as cp, ContactGrade, SsnSource, ValidationTally};
use propagation::{PathPrediction, Route, RouteCall, RouteConfidence, RouteReason, RouteVerdict};
use serde::{Deserialize, Serialize};
use tempo_app::logstore::LogRows;
use tempo_core::logbook::path_stamp::{self, PathStamp, StampConf, StampRoute};
use tempo_core::logbook::sqlite::{Narrow, Order, Scope};
use tempo_core::logbook::QsoRecord;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A grid's 4-character square, upper case; `None` for a grid too short to name one.
pub(crate) fn grid4(g: &str) -> Option<String> {
    let g = g.trim();
    (g.len() >= 4 && g.is_char_boundary(4)).then(|| g[..4].to_ascii_uppercase())
}

fn latlon(g: &str) -> Option<(f64, f64)> {
    propagation::geo::maidenhead_to_latlon(g.trim())
}

fn letter(r: Route) -> &'static str {
    match r {
        Route::Short => "S",
        Route::Long => "L",
    }
}

/// A reason as the log and the Logbook carry it: a token the UI puts into words.
pub(crate) fn reason_token(r: &RouteReason) -> String {
    match r {
        RouteReason::OneClosed { open } => format!("OPEN_{}", letter(*open)),
        RouteReason::Margin { better } => format!("MARGIN_{}", letter(*better)),
        RouteReason::Reliability { better } => format!("RELIAB_{}", letter(*better)),
        RouteReason::WithinMargin => "WITHIN".into(),
        RouteReason::BothClosed => "CLOSED".into(),
        RouteReason::GreyLine { route } => format!("GREY_{}", letter(*route)),
        RouteReason::HeardAlong { route, receivers } => {
            format!("HEARD_{}:{receivers}", letter(*route))
        }
        RouteReason::LateDt { ms, decodes } => format!("LATEDT:{ms}/{decodes}"),
    }
}

fn stamp_route(v: RouteVerdict) -> StampRoute {
    match v {
        RouteVerdict::Short => StampRoute::Short,
        RouteVerdict::Long => StampRoute::Long,
        RouteVerdict::Either => StampRoute::Either,
        RouteVerdict::Unknown => StampRoute::Unknown,
    }
}

fn stamp_conf(c: RouteConfidence) -> StampConf {
    match c {
        RouteConfidence::Likely => StampConf::Likely,
        RouteConfidence::Possible => StampConf::Possible,
    }
}

/// The record's stamp for a call made under `key`.
pub(crate) fn stamp_of(call: &RouteCall, key: String) -> PathStamp {
    PathStamp {
        route: stamp_route(call.verdict),
        conf: call.confidence.map(stamp_conf),
        why: call.reasons.iter().map(reason_token).collect(),
        key,
    }
}

// ── the path cards, kept for the log-time grade ──────────────────────────────────────────────

/// A path card is a grade's source for this long either side of the contact (and the same UTC
/// month: the model is a monthly median).
pub(crate) const OUTLOOK_FRESH_SECS: i64 = 6 * 3600;
const OUTLOOKS_KEPT: usize = 8;

struct KeptOutlook {
    me4: String,
    dx4: String,
    at: i64,
    pred: Arc<PathPrediction>,
}

static OUTLOOKS: Mutex<VecDeque<KeptOutlook>> = Mutex::new(VecDeque::new());

/// Keep the path card `get_path_outlook` just computed (both routes) for the log-time grade.
pub(crate) fn keep_outlook(mygrid: &str, dxgrid: &str, at: i64, pred: &PathPrediction) {
    let (Some(me4), Some(dx4)) = (grid4(mygrid), grid4(dxgrid)) else {
        return;
    };
    if pred.long_path.is_none() {
        return;
    }
    let mut kept = lock(&OUTLOOKS);
    kept.retain(|k| !(k.me4 == me4 && k.dx4 == dx4));
    kept.push_back(KeptOutlook {
        me4,
        dx4,
        at,
        pred: Arc::new(pred.clone()),
    });
    while kept.len() > OUTLOOKS_KEPT {
        kept.pop_front();
    }
}

fn month_of(t: i64) -> (i64, u32) {
    propagation::solar_cycle::year_month(t)
}

fn outlook_for(me4: &str, dx4: &str, when: i64) -> Option<Arc<PathPrediction>> {
    lock(&OUTLOOKS)
        .iter()
        .rev()
        .find(|k| {
            k.me4 == me4
                && k.dx4 == dx4
                && (k.at - when).abs() <= OUTLOOK_FRESH_SECS
                && month_of(k.at) == month_of(when)
        })
        .map(|k| Arc::clone(&k.pred))
}

// ── the log-time grade ───────────────────────────────────────────────────────────────────────

/// How far either side of a contact's start a PSK Reporter spot counts as the same moment.
const SPOT_WINDOW_SECS: i64 = 900;

/// Where the spots heard around a contact were received: the operator's own reports (who heard
/// me) and the spots of the worked station's own signal.
fn receivers(
    rec: &QsoRecord,
    live: &Mutex<propagation::LiveSpots>,
    region: &Mutex<propagation::LiveSpots>,
) -> Vec<(f64, f64)> {
    let when = rec.when_unix as i64;
    let mut out = Vec::new();
    // Never wait on a feed from under the Engine lock: a busy feed only costs the reason.
    if let Ok(spots) = live.try_lock() {
        out.extend(
            spots
                .recent_iter(when + SPOT_WINDOW_SECS, 2 * SPOT_WINDOW_SECS)
                .filter(|s| s.time <= when + SPOT_WINDOW_SECS)
                .filter_map(|s| s.rx_grid.as_deref().and_then(latlon)),
        );
    }
    if let Ok(spots) = region.try_lock() {
        out.extend(
            spots
                .recent_iter(when + SPOT_WINDOW_SECS, 2 * SPOT_WINDOW_SECS)
                .filter(|s| s.time <= when + SPOT_WINDOW_SECS)
                .filter(|s| s.tx_call.eq_ignore_ascii_case(rec.call.trim()))
                .filter_map(|s| s.rx_grid.as_deref().and_then(latlon)),
        );
    }
    out
}

/// Whether a contact went by the ionosphere, as far as its record says: no `PROP_MODE`, or one
/// naming the ionosphere (`F2`, `ION`). A satellite, EME, repeater, internet-linked or any other
/// contact is not a path the model describes, and is never graded.
pub(crate) fn ionospheric(prop_mode: Option<&str>) -> bool {
    match prop_mode.map(str::trim).filter(|m| !m.is_empty()) {
        None => true,
        Some(m) => m.eq_ignore_ascii_case("F2") || m.eq_ignore_ascii_case("ION"),
    }
}

/// The grade for a contact being logged — the engine's path grader. `my_grid` is the station's
/// grid from Settings, for a record that names none; `late_secs` the worked station's decode
/// lateness against each period's median (the aggregate DT gate). Never an antenna field.
pub(crate) fn at_log(
    rec: &QsoRecord,
    my_grid: &str,
    live: &Mutex<propagation::LiveSpots>,
    region: &Mutex<propagation::LiveSpots>,
    late_secs: &[f32],
) -> Option<PathStamp> {
    if !ionospheric(rec.prop_mode.as_deref()) {
        return None;
    }
    let start = rec.when_unix as i64;
    let key = path_stamp::grade_key(rec)?;
    let mine = rec
        .my_grid
        .as_deref()
        .filter(|g| !g.trim().is_empty())
        .unwrap_or(my_grid);
    let me4 = grid4(mine)?;
    let dx4 = grid4(rec.grid.as_deref()?)?;
    let pred = outlook_for(&me4, &dx4, start)?;
    let lp = pred.long_path.as_deref()?;
    let hour = (start.rem_euclid(86_400) / 3600) as usize;
    let mut call = propagation::compare_routes_at(&pred, lp, hour)
        .into_iter()
        .find(|c| c.band.eq_ignore_ascii_case(rec.band.trim()))?;
    if let (Some(me), Some(dx)) = (latlon(mine), rec.grid.as_deref().and_then(latlon)) {
        let along = cp::receivers_along(me, dx, &receivers(rec, live, region));
        call.reasons.extend(cp::heard_along_reason(along));
    }
    if let Some((ms, decodes)) = cp::late_dt_ms(late_secs) {
        call.reasons.push(RouteReason::LateDt { ms, decodes });
    }
    Some(stamp_of(&call, key))
}

// ── the display-only grades of the existing log ──────────────────────────────────────────────

/// One graded cell, as kept in memory and in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Cell {
    pub(crate) verdict: RouteVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) confidence: Option<RouteConfidence>,
    pub(crate) why: Vec<String>,
    pub(crate) short_bcr: f32,
    pub(crate) long_bcr: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) margin_db: Option<f32>,
    pub(crate) short_bearing_deg: f32,
    pub(crate) polar_short: bool,
    pub(crate) polar_long: bool,
    pub(crate) ssn_source: SsnSource,
}

impl Cell {
    fn of(g: &ContactGrade, src: SsnSource) -> Cell {
        Cell {
            verdict: g.call.verdict,
            confidence: g.call.confidence,
            why: g.call.reasons.iter().map(reason_token).collect(),
            short_bcr: g.call.short_bcr,
            long_bcr: g.call.long_bcr,
            margin_db: g.call.margin_db,
            short_bearing_deg: g.short_bearing_deg,
            polar_short: g.polar_short,
            polar_long: g.polar_long,
            ssn_source: src,
        }
    }

    /// The grade again, for the tally (which reads no reasons).
    fn grade(&self, band: &str) -> ContactGrade {
        ContactGrade {
            call: RouteCall {
                band: band.to_string(),
                verdict: self.verdict,
                confidence: self.confidence,
                margin_db: self.margin_db,
                short_bcr: self.short_bcr,
                long_bcr: self.long_bcr,
                reasons: Vec::new(),
            },
            short_bearing_deg: self.short_bearing_deg,
            polar_short: self.polar_short,
            polar_long: self.polar_long,
        }
    }
}

/// What a contact's display grade depends on, as the fields it reads.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GradeAsk {
    pub(crate) when_unix: u64,
    pub(crate) band: String,
    #[serde(default)]
    pub(crate) grid: Option<String>,
    #[serde(default)]
    pub(crate) my_grid: Option<String>,
    /// ADIF `PROP_MODE`: anything but the ionosphere is never graded ([`ionospheric`]).
    #[serde(default)]
    pub(crate) prop_mode: Option<String>,
    /// The passthrough fields the grade reads (`SFI`, `K_INDEX`, `ANT_AZ`, the stamp's); others
    /// ignored.
    #[serde(default)]
    pub(crate) extra: Vec<(String, String)>,
}

impl GradeAsk {
    pub(crate) fn of(r: &QsoRecord) -> GradeAsk {
        GradeAsk {
            when_unix: r.when_unix,
            band: r.band.clone(),
            grid: r.grid.clone(),
            my_grid: r.my_grid.clone(),
            prop_mode: r.prop_mode.clone(),
            extra: r
                .extra
                .iter()
                .filter(|(k, _)| READS.contains(&k.as_str()))
                .cloned()
                .collect(),
        }
    }

    fn tag(&self, t: &str) -> Option<&str> {
        self.extra
            .iter()
            .find(|(k, _)| k == t)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    }

    fn record(&self) -> QsoRecord {
        let mut r = tempo_core::logbook::parse_adif("<EOH><CALL:1>X<EOR>").remove(0);
        r.when_unix = self.when_unix;
        r.band = self.band.clone();
        r.grid = self.grid.clone();
        r.my_grid = self.my_grid.clone();
        r.prop_mode = self.prop_mode.clone();
        r.extra = self.extra.clone();
        r.extra.sort();
        r
    }
}

/// The passthrough fields a grade reads.
pub(crate) const READS: &[&str] = &[
    "SFI",
    path_stamp::ANT_AZ_FIELD,
    path_stamp::PATH_FIELD,
    path_stamp::PATH_CONF_FIELD,
    path_stamp::PATH_WHY_FIELD,
];

/// The `qso` columns a grade reads.
pub(crate) const COLUMNS: &[&str] = &[
    "when_unix",
    "time_off_unix",
    "band",
    "grid",
    "my_grid",
    "prop_mode",
];

/// A cell's key and what grading it needs.
struct CellAsk {
    key: String,
    me: (f64, f64),
    dx: (f64, f64),
    band: String,
    when: i64,
    ssn: f64,
    src: SsnSource,
}

fn cell_ask(a: &GradeAsk, my_grid: &str, now: i64, current_ssn: Option<f64>) -> Option<CellAsk> {
    if !ionospheric(a.prop_mode.as_deref()) {
        return None;
    }
    let mine = a
        .my_grid
        .as_deref()
        .filter(|g| !g.trim().is_empty())
        .unwrap_or(my_grid);
    let me4 = grid4(mine)?;
    let dx4 = grid4(a.grid.as_deref()?)?;
    let band = propagation::model::Band::from_label(a.band.trim())?;
    if band.is_vhf() {
        return None;
    }
    let when = a.when_unix as i64;
    let sfi = a.tag("SFI").and_then(|v| v.parse::<f64>().ok());
    let (ssn, src) = cp::ssn_for_contact(sfi, when, now, current_ssn)?;
    let (y, m) = month_of(when);
    let hour = when.rem_euclid(86_400) / 3600;
    let src_tag = match src {
        SsnSource::Logged => "l",
        SsnSource::Recent => "r",
    };
    Some(CellAsk {
        key: format!(
            "{me4}|{dx4}|{}|{y:04}-{m:02}|{hour:02}|{}|{src_tag}",
            band.label(),
            ssn.round() as i64
        ),
        me: latlon(&me4)?,
        dx: latlon(&dx4)?,
        band: band.label().to_string(),
        // The middle of the hour: the cell stands for the whole hour.
        when: when - when.rem_euclid(3600) + 1800,
        ssn,
        src,
    })
}

/// How a pass is going, and what it found — the expert's view of the grading.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub(crate) running: bool,
    /// Contacts in the log at the last pass, those it could grade, and those it could not
    /// (no grid, a band the model skips, or no sunspot number for their part of the cycle).
    pub(crate) contacts: u32,
    pub(crate) graded: u32,
    pub(crate) ungradable: u32,
    /// Cells graded so far and the cells the last pass found to grade.
    pub(crate) cells_done: u32,
    pub(crate) cells_total: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) finished_unix: Option<i64>,
    /// The success measures over the last 30 days of graded contacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_30_days: Option<ValidationTally>,
}

/// The display-only grades held in memory: what the file holds, under the inputs it was graded
/// with (engine, power, antenna gain — anything else is in each cell's key).
#[derive(Default)]
struct Grades {
    inputs: String,
    /// The file holds nothing for these inputs yet: the next write starts it over, header first.
    file_fresh: bool,
    loaded: bool,
    cells: HashMap<String, Cell>,
    /// Moves on every chunk, so a filter on the grades is built again as grading proceeds.
    rev: u64,
    status: Status,
}

static GRADES: std::sync::LazyLock<Mutex<Grades>> =
    std::sync::LazyLock::new(|| Mutex::new(Grades::default()));

/// One pass at a time.
static PASSING: Mutex<()> = Mutex::new(());

/// The grades file, beside `settings.json`.
pub(crate) fn file_path() -> PathBuf {
    crate::settings_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("path_grades.jsonl")
}

/// The grades file in `dir` (a test's own folder).
#[cfg(test)]
pub(crate) fn file_path_in(dir: &Path) -> PathBuf {
    dir.join("path_grades.jsonl")
}

#[derive(Serialize, Deserialize)]
struct Header {
    v: u32,
    inputs: String,
}

#[derive(Serialize, Deserialize)]
struct Line {
    k: String,
    g: Cell,
}

const FILE_VERSION: u32 = 1;

/// Read the grades file made under `inputs`. A file made under other inputs, of another
/// version, or unreadable, holds nothing for them: `None`. A torn last line (a pass killed
/// mid-write) is skipped.
fn load_file(path: &Path, inputs: &str) -> Option<HashMap<String, Cell>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let h: Header = serde_json::from_str(lines.next()?).ok()?;
    if h.v != FILE_VERSION || h.inputs != inputs {
        return None;
    }
    Some(
        lines
            .filter_map(|l| serde_json::from_str::<Line>(l).ok())
            .map(|l| (l.k, l.g))
            .collect(),
    )
}

/// Append graded cells (starting the file over, header first, when `fresh`). Best effort: a
/// failed write only means a later pass grades those cells again.
fn append_file(path: &Path, inputs: &str, fresh: bool, cells: &[(String, Cell)]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut text = String::new();
    if fresh {
        let h = Header {
            v: FILE_VERSION,
            inputs: inputs.to_string(),
        };
        text.push_str(&serde_json::to_string(&h).unwrap_or_default());
        text.push('\n');
    }
    for (k, g) in cells {
        if let Ok(l) = serde_json::to_string(&Line {
            k: k.clone(),
            g: g.clone(),
        }) {
            text.push_str(&l);
            text.push('\n');
        }
    }
    let file = if fresh {
        std::fs::File::create(path)
    } else {
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
    };
    if let Ok(mut f) = file {
        let _ = f.write_all(text.as_bytes());
    }
}

/// What a pass needs, taken under the Engine lock in one breath ([`capture`]). It holds the
/// log's read handle and plain values, and nothing that can reach the Engine.
pub(crate) struct Job {
    pub(crate) rows: LogRows,
    pub(crate) my_grid: String,
    pub(crate) power_w: Option<f64>,
    pub(crate) ant_gain_dbi: f64,
    pub(crate) now: i64,
    pub(crate) current_ssn: Option<f64>,
}

/// Take a pass's inputs: the Engine lock once, released before this returns.
pub(crate) fn capture(engine: &crate::SharedEngine, now: i64, current_ssn: Option<f64>) -> Job {
    let eng = tempo_app::engine::engine_lock(engine);
    let st = eng.settings();
    Job {
        rows: eng.log_rows(),
        my_grid: st.mygrid.clone(),
        power_w: st.station_power_w,
        ant_gain_dbi: st.ant_tx_gain_dbi + st.ant_rx_gain_dbi,
        now,
        current_ssn,
    }
}

/// The inputs every cell of a file shares.
fn inputs_of(job: &Job) -> String {
    format!("p533|{:?}|{}", job.power_w, job.ant_gain_dbi)
}

/// Cells graded per chunk: each chunk is appended to the file before the next starts.
pub(crate) const CHUNK: usize = 64;

/// One pass over the log: grade every contact's cell not yet graded, chunk by chunk, then count
/// the last 30 days. ⛔ Reads the log and writes only this module's file. `stop` ends it between
/// chunks; what was graded stays.
pub(crate) fn run_pass(job: Job, path: &Path, stop: &AtomicBool) -> Result<Status, String> {
    let _one = lock(&PASSING);
    let inputs = inputs_of(&job);
    // The file, once, for these inputs.
    {
        let mut g = lock(&GRADES);
        if !g.loaded || g.inputs != inputs {
            let loaded = load_file(path, &inputs);
            g.file_fresh = loaded.is_none();
            g.cells = loaded.unwrap_or_default();
            g.inputs = inputs.clone();
            g.loaded = true;
            g.rev += 1;
        }
    }
    // The log, read off every lock but its own.
    let mut asks: Vec<GradeAsk> = Vec::new();
    job.rows
        .each_with(
            Narrow {
                columns: COLUMNS,
                uploads: false,
            },
            READS,
            Scope::All,
            Order::Log,
            &mut |r| {
                asks.push(GradeAsk::of(r));
                std::ops::ControlFlow::Continue(())
            },
        )
        .map_err(|e| e.to_string())?;
    let mut todo: Vec<CellAsk> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let (mut graded, mut ungradable) = (0u32, 0u32);
    {
        let g = lock(&GRADES);
        for a in &asks {
            match cell_ask(a, &job.my_grid, job.now, job.current_ssn) {
                Some(c) => {
                    graded += 1;
                    if !g.cells.contains_key(&c.key) && seen.insert(c.key.clone()) {
                        todo.push(c);
                    }
                }
                None => ungradable += 1,
            }
        }
    }
    {
        let mut g = lock(&GRADES);
        g.status.running = true;
        g.status.contacts = asks.len() as u32;
        g.status.graded = graded;
        g.status.ungradable = ungradable;
        g.status.cells_done = 0;
        g.status.cells_total = todo.len() as u32;
    }
    let mut cfg = job
        .power_w
        .map(propagation::p533::engine::P533Config::with_power_watts)
        .unwrap_or_default();
    cfg.ant_gain_dbi = job.ant_gain_dbi;
    let mut engines: HashMap<(i64, i64), propagation::p533::engine::P533Engine> = HashMap::new();
    let mut done = 0u32;
    for chunk in todo.chunks(CHUNK) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let mut out = Vec::with_capacity(chunk.len());
        for c in chunk {
            let key = ((c.me.0 * 1e4) as i64, (c.me.1 * 1e4) as i64);
            let eng = engines.entry(key).or_insert_with(|| {
                propagation::p533::engine::P533Engine::with_config(Some(c.me), cfg)
            });
            if let Some(grade) = cp::grade_contact(eng, c.me, c.dx, &c.band, c.when, c.ssn) {
                out.push((c.key.clone(), Cell::of(&grade, c.src)));
            }
        }
        done += chunk.len() as u32;
        let mut g = lock(&GRADES);
        append_file(path, &inputs, g.file_fresh, &out);
        g.file_fresh = false;
        g.cells.extend(out);
        g.rev += 1;
        g.status.cells_done = done;
    }
    // The success measures, over the last 30 days.
    let since = job.now - 30 * 86_400;
    let mut tally = ValidationTally::default();
    {
        let g = lock(&GRADES);
        for a in asks.iter().filter(|a| a.when_unix as i64 >= since) {
            let Some(c) = cell_ask(a, &job.my_grid, job.now, job.current_ssn) else {
                continue;
            };
            let Some(cell) = g.cells.get(&c.key) else {
                continue;
            };
            let heading = a
                .tag(path_stamp::ANT_AZ_FIELD)
                .and_then(|v| v.parse::<f32>().ok());
            tally.add(&cell.grade(&c.band), heading);
        }
    }
    let mut g = lock(&GRADES);
    g.status.running = false;
    g.status.finished_unix = Some(job.now);
    g.status.last_30_days = Some(tally);
    Ok(g.status.clone())
}

/// The grading's status.
pub(crate) fn status() -> Status {
    lock(&GRADES).status.clone()
}

/// Moves whenever the display-only grades change.
pub(crate) fn rev() -> u64 {
    lock(&GRADES).rev
}

/// Start the background grader: a first pass a while after launch, then one whenever the log
/// has changed since the last, checked every [`PASS_EVERY`]. Each pass takes the Engine lock
/// once ([`capture`]) and grades with it released.
pub(crate) const FIRST_PASS_AFTER: std::time::Duration = std::time::Duration::from_secs(120);
pub(crate) const PASS_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[cfg(feature = "radio")]
pub(crate) fn start_grader(engine: crate::SharedEngine) {
    let _ = std::thread::Builder::new()
        .name("path-grades".into())
        .spawn(move || {
            let stop = &tempo_audio::service::SHUTDOWN;
            std::thread::sleep(FIRST_PASS_AFTER);
            let mut last_rev: Option<u64> = None;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let rev = tempo_app::engine::engine_lock(&engine).log_revision();
                if last_rev != Some(rev) {
                    let job = capture(&engine, crate::now_unix(), crate::current_ssn());
                    if run_pass(job, &file_path(), stop).is_ok() {
                        last_rev = Some(rev);
                    }
                }
                std::thread::sleep(PASS_EVERY);
            }
        });
}

// ── what the Logbook shows ───────────────────────────────────────────────────────────────────

/// Where a shown grade comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GradeTier {
    /// Stamped on the contact when it was logged, from the path card and the station's own
    /// evidence at the time.
    Logged,
    /// Graded afterwards from the model alone, for display.
    Modelled,
}

/// One contact's grade as the Logbook shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GradeView {
    pub(crate) verdict: RouteVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) confidence: Option<RouteConfidence>,
    pub(crate) why: Vec<String>,
    pub(crate) tier: GradeTier,
    /// Whether the route named (the short path when none is) crosses the polar cap.
    pub(crate) polar: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ssn_source: Option<SsnSource>,
    /// The contact's stamp was made for another grid or band than it holds now (it was edited
    /// since); the model's cell is shown instead.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) stamp_stale: bool,
}

fn verdict_of(r: StampRoute) -> RouteVerdict {
    match r {
        StampRoute::Short => RouteVerdict::Short,
        StampRoute::Long => RouteVerdict::Long,
        StampRoute::Either => RouteVerdict::Either,
        StampRoute::Unknown => RouteVerdict::Unknown,
    }
}

fn conf_of(c: StampConf) -> RouteConfidence {
    match c {
        StampConf::Likely => RouteConfidence::Likely,
        StampConf::Possible => RouteConfidence::Possible,
    }
}

/// Whether the route `verdict` names between `my_grid` and the contact's grid crosses the polar
/// cap — geometry only.
fn polar_of(a: &GradeAsk, my_grid: &str, verdict: RouteVerdict) -> bool {
    let mine = a
        .my_grid
        .as_deref()
        .filter(|g| !g.trim().is_empty())
        .unwrap_or(my_grid);
    let (Some(me), Some(dx)) = (latlon(mine), a.grid.as_deref().and_then(latlon)) else {
        return false;
    };
    let route = if verdict == RouteVerdict::Long {
        Route::Long
    } else {
        Route::Short
    };
    propagation::path_geometry_route(me, dx, route, a.when_unix as i64, None, None, None)
        .max_gmlat_deg
        >= cp::POLAR_CAP_GMLAT_DEG
}

/// The grade a contact shows: its own stamp when it carries one made for its current grid and
/// band, else the display-only cell, else none.
pub(crate) fn view_for(
    a: &GradeAsk,
    my_grid: &str,
    now: i64,
    current_ssn: Option<f64>,
) -> Option<GradeView> {
    if !ionospheric(a.prop_mode.as_deref()) {
        return None;
    }
    let rec = a.record();
    let stamp = path_stamp::read_path(&rec);
    let current = path_stamp::grade_key(&rec);
    let mut stale = false;
    if let Some(s) = stamp {
        if current.as_deref() == Some(s.key.as_str()) {
            let verdict = verdict_of(s.route);
            return Some(GradeView {
                verdict,
                confidence: s.conf.map(conf_of),
                why: s.why,
                tier: GradeTier::Logged,
                polar: polar_of(a, my_grid, verdict),
                ssn_source: None,
                stamp_stale: false,
            });
        }
        stale = true;
    }
    let c = cell_ask(a, my_grid, now, current_ssn)?;
    let g = lock(&GRADES);
    let cell = g.cells.get(&c.key)?;
    let polar = if cell.verdict == RouteVerdict::Long {
        cell.polar_long
    } else {
        cell.polar_short
    };
    Some(GradeView {
        verdict: cell.verdict,
        confidence: cell.confidence,
        why: cell.why.clone(),
        tier: GradeTier::Modelled,
        polar,
        ssn_source: Some(cell.ssn_source),
        stamp_stale: stale,
    })
}

/// The Logbook's path filter for one contact ([`tempo_core::logbook::query::PathFilter`]): the
/// grade it shows names the long path, or its route crosses the polar cap. A contact with no
/// grade passes neither. `my_grid` is the station's grid now, as the Path column reads it.
pub(crate) fn filter_keeps(
    r: &QsoRecord,
    want: tempo_core::logbook::query::PathFilter,
    my_grid: &str,
) -> bool {
    use tempo_core::logbook::query::PathFilter;
    let Some(v) = view_for(
        &GradeAsk::of(r),
        my_grid,
        crate::now_unix(),
        crate::current_ssn(),
    ) else {
        return false;
    };
    match want {
        PathFilter::Long => v.verdict == RouteVerdict::Long,
        PathFilter::Polar => v.polar,
    }
}

/// A contact's path over the day it was made: both routes, every band and hour, at the sunspot
/// number its own flux gives (or today's for a recent contact), with the Kp it logged — and the
/// grade it shows. What the contact card draws.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContactDay {
    pub(crate) pred: PathPrediction,
    pub(crate) ssn_source: SsnSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) grade: Option<GradeView>,
}

/// [`ContactDay`] for one contact: two whole-day predictions (~0.2 s of compute; call it off
/// every lock). `None` without both grid squares, a sunspot number for its part of the cycle,
/// or a long path to compare.
pub(crate) fn contact_day(
    a: &GradeAsk,
    my_grid: &str,
    power_w: Option<f64>,
    ant_gain_dbi: f64,
    now: i64,
    current_ssn: Option<f64>,
) -> Option<ContactDay> {
    use propagation::PathPredictor;
    if !ionospheric(a.prop_mode.as_deref()) {
        return None;
    }
    let mine = a
        .my_grid
        .as_deref()
        .filter(|g| !g.trim().is_empty())
        .unwrap_or(my_grid);
    let me = latlon(mine)?;
    let dx = latlon(a.grid.as_deref()?)?;
    let t = a.when_unix as i64;
    let sfi = a.tag("SFI").and_then(|v| v.parse::<f64>().ok());
    let (ssn, src) = cp::ssn_for_contact(sfi, t, now, current_ssn)?;
    // The K index logged with the contact is a reading from that time; without one, unknown.
    let kp = a
        .extra
        .iter()
        .find(|(k, _)| k == "K_INDEX")
        .and_then(|(_, v)| v.trim().parse::<f32>().ok())
        .filter(|k| (0.0..=9.0).contains(k));
    let mut cfg = power_w
        .map(propagation::p533::engine::P533Config::with_power_watts)
        .unwrap_or_default();
    cfg.ant_gain_dbi = ant_gain_dbi;
    let eng = propagation::p533::engine::P533Engine::with_config(Some(me), cfg);
    let wx = propagation::SpaceWx {
        ssn: Some(ssn as f32),
        ..Default::default()
    };
    let (mut pred, long) = std::thread::scope(|s| {
        let lp = s.spawn(|| eng.predict_route(dx, Route::Long, t, &wx));
        (eng.predict(dx, t, &wx), lp.join().ok().flatten())
    });
    let mut lp = long?;
    let geom = |route| propagation::path_geometry_route(me, dx, route, t, kp, None, None);
    pred.geometry = Some(geom(Route::Short));
    lp.geometry = Some(geom(Route::Long));
    pred.day = propagation::path_day(&pred, Some(&lp));
    pred.routes = Some(propagation::compare_routes(&pred, &lp));
    pred.long_path = Some(Box::new(lp));
    Some(ContactDay {
        pred,
        ssn_source: src,
        grade: view_for(a, my_grid, now, current_ssn),
    })
}

/// The module's state is process-wide; the tests that touch it take turns.
#[cfg(test)]
pub(crate) static TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn forget_grades() {
    let mut g = lock(&GRADES);
    *g = Grades::default();
}

#[cfg(test)]
mod tests;
