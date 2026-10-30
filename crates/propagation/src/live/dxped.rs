//! Live DXpedition feed (the `live` feature): NG3K ADXO (the forward **calendar**
//! of announced operations — dates, entity, bands, modes) overlaid with ClubLog
//! `expeditions.php` (which calls are **active on the air now**) and ClubLog's
//! most-wanted ranks. Each plan's location comes from [`crate::dxcc`] so
//! distance/bearing/region work.
//!
//! # How often each source is asked
//!
//! [`fetch_plans`] runs on every propagation snapshot, so it must never be what decides how
//! often a third party is asked. Each of the three sources keeps its own answer and its own
//! schedule, and none of them waits on another:
//!
//! | source | asked at most | first retry after a failure |
//! |---|---|---|
//! | NG3K calendar | every 30 min | 5 min |
//! | ClubLog `expeditions.php` | once a day | 1 h |
//! | ClubLog `mostwanted.php` | once every 30 days | 1 h |
//!
//! ⛔ Every ask is recorded before it goes out and counts as a failure until it answers. A
//! failure waits before the next ask: the first retry above, doubling per failure in a row,
//! never longer than the normal period. A source that has never answered gets the same
//! back-off — "nothing kept" is not a reason to ask again, and that gap is how a dead NG3K
//! once had ClubLog asked on every snapshot. The last good answer is served stale meanwhile.
//! One ask per source at a time: a caller that finds an ask out is served what is kept.
//!
//! The two ClubLog answers are written to disk with the time of their last ask
//! ([`set_cache_dir`]), the ask time before the ask goes out, so a restart — even one in the
//! middle of an ask — does not ask ClubLog again before its schedule says so.
//! `mostwanted.php` is public and asked as `?api=1`: no key goes with it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::dxcc;
use crate::dxped::{DxpeditionPlan, Ft8DxpMode};
use crate::geo::latlon_to_maidenhead;
use crate::model::Band;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

const NG3K_URL: &str = "https://www.ng3k.com/Misc/adxo.html";
const EXPEDITIONS_URL: &str = "https://clublog.org/expeditions.php?api=1";
/// Public: ClubLog asks for `api=1` here, not an application key.
const MOST_WANTED_URL: &str = "https://clublog.org/mostwanted.php?api=1";

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// When a source may be asked again (seconds).
#[derive(Clone, Copy)]
struct Schedule {
    /// The wait after a success.
    period: i64,
    /// The wait after the first failure in a row; doubles per further failure, capped at `period`.
    first_retry: i64,
}

/// NG3K updates its page about daily; half an hour keeps a new announcement prompt.
const NG3K_SCHEDULE: Schedule = Schedule {
    period: 1800,
    first_retry: 300,
};
/// ClubLog's own limit for `expeditions.php`: once a day.
const EXPEDITIONS_SCHEDULE: Schedule = Schedule {
    period: 86_400,
    first_retry: 3600,
};
/// ClubLog's own limit for `mostwanted.php`: once a month.
const MOST_WANTED_SCHEDULE: Schedule = Schedule {
    period: 30 * 86_400,
    first_retry: 3600,
};

/// The two ClubLog answers on disk, beside each other in the folder [`set_cache_dir`] names.
const EXPEDITIONS_FILE: &str = "clublog_expeditions.json";
const MOST_WANTED_FILE: &str = "clublog_mostwanted.json";

/// One source's last good answer and when it was last asked. What is written to disk for the
/// ClubLog sources.
#[derive(Serialize, Deserialize)]
struct Kept<T> {
    data: Option<T>,
    /// Unix seconds of the last ask, success or failure; 0 = never asked.
    asked_unix: i64,
    /// Failures in a row since the last success. An ask still out counts as one.
    failures: u32,
}

impl<T> Kept<T> {
    const fn new() -> Self {
        Kept {
            data: None,
            asked_unix: 0,
            failures: 0,
        }
    }

    fn due(&self, s: Schedule, now: i64) -> bool {
        if self.asked_unix == 0 {
            return true;
        }
        let wait = match self.failures {
            0 => s.period,
            n => s
                .first_retry
                .saturating_mul(1i64 << (n - 1).min(30))
                .min(s.period),
        };
        now >= self.asked_unix.saturating_add(wait)
    }
}

/// Ask `fetch` when `slot` is due; `keep` is handed the slot after each change (to write it).
fn refresh<T>(
    slot: &Mutex<Kept<T>>,
    s: Schedule,
    now: i64,
    fetch: impl FnOnce() -> Result<T, String>,
    keep: impl Fn(&Kept<T>),
) {
    {
        let mut k = lock(slot);
        if !k.due(s, now) {
            return;
        }
        // ⛔ Recorded BEFORE the ask goes out, under the lock, and as a failure until it
        // answers. A second caller now finds the source not due and is served what is kept
        // (one ask at a time, without holding the lock over the network); and an ask that
        // never answers — a hang, Nexus closed or killed mid-ask — still counts, on disk too,
        // and backs off like any failure.
        k.asked_unix = now;
        k.failures = k.failures.saturating_add(1);
        keep(&k);
    }
    // A failure leaves the last good answer, served stale.
    if let Ok(v) = fetch() {
        let mut k = lock(slot);
        k.data = Some(v);
        k.failures = 0;
        keep(&k);
    }
}

/// What the feed asks the network for — a seam, so the schedule can be driven in tests by a
/// fake that counts asks.
trait Source {
    /// NG3K's ADXO page, parsed.
    fn ng3k(&self) -> Result<Vec<DxpeditionPlan>, String>;
    /// ClubLog's active-expeditions set (uppercased callsigns).
    fn active(&self) -> Result<HashSet<String>, String>;
    /// ClubLog's most-wanted ranks by entity name.
    fn most_wanted(&self) -> Result<HashMap<String, u32>, String>;
}

struct Net(reqwest::blocking::Client);

impl Source for Net {
    fn ng3k(&self) -> Result<Vec<DxpeditionPlan>, String> {
        let html = self
            .0
            .get(NG3K_URL)
            .send()
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .text()
            .map_err(|e| e.to_string())?;
        Ok(parse_adxo(&html))
    }
    fn active(&self) -> Result<HashSet<String>, String> {
        fetch_active(&self.0)
    }
    fn most_wanted(&self) -> Result<HashMap<String, u32>, String> {
        most_wanted(&self.0)
    }
}

/// Every source's kept answer, plus the merged plan list the Needed board reads.
struct Feeds {
    ng3k: Mutex<Kept<Vec<DxpeditionPlan>>>,
    active: Mutex<Kept<HashSet<String>>>,
    wanted: Mutex<Kept<HashMap<String, u32>>>,
    merged: Mutex<Vec<DxpeditionPlan>>,
    /// Where the ClubLog answers are written; `None` keeps them in memory only.
    dir: Mutex<Option<PathBuf>>,
}

static FEEDS: Feeds = Feeds::new();

impl Feeds {
    const fn new() -> Self {
        Feeds {
            ng3k: Mutex::new(Kept::new()),
            active: Mutex::new(Kept::new()),
            wanted: Mutex::new(Kept::new()),
            merged: Mutex::new(Vec::new()),
            dir: Mutex::new(None),
        }
    }

    /// Ask whichever sources are due, then merge what is kept.
    fn plans(&self, src: &dyn Source, now: i64) -> Result<Vec<DxpeditionPlan>, String> {
        refresh(&self.ng3k, NG3K_SCHEDULE, now, || src.ng3k(), |_| {});
        refresh(
            &self.active,
            EXPEDITIONS_SCHEDULE,
            now,
            || src.active(),
            |k| self.save(k, EXPEDITIONS_FILE),
        );
        refresh(
            &self.wanted,
            MOST_WANTED_SCHEDULE,
            now,
            || src.most_wanted(),
            |k| self.save(k, MOST_WANTED_FILE),
        );

        let Some(mut plans) = lock(&self.ng3k).data.clone() else {
            return Err("the DXpedition calendar has not been fetched yet".to_string());
        };
        if let Some(active) = lock(&self.active).data.as_ref() {
            for p in &mut plans {
                if active.iter().any(|a| call_matches(a, &p.call)) {
                    // ClubLog confirms it's on air — make sure the window includes now.
                    if p.start_unix > now {
                        p.start_unix = now - 3600;
                    }
                    if p.end_unix < now {
                        p.end_unix = now + 3600;
                    }
                }
            }
        }
        // ClubLog most-wanted rank. The WorkableCard priority formula weights this.
        if let Some(wanted) = lock(&self.wanted).data.as_ref() {
            for p in &mut plans {
                if p.most_wanted_rank.is_none() {
                    if let Some(info) = dxcc::resolve(&p.call) {
                        p.most_wanted_rank = wanted.get(info.entity).copied();
                    }
                }
            }
        }
        *lock(&self.merged) = plans.clone();
        Ok(plans)
    }

    /// Write one ClubLog answer and its ask time (tmp + rename; best effort — a failed write
    /// only means the next start asks once more than it needed to).
    fn save<T: Serialize>(&self, kept: &Kept<T>, name: &str) {
        let Some(dir) = lock(&self.dir).clone() else {
            return;
        };
        let Ok(text) = serde_json::to_string(kept) else {
            return;
        };
        let path = dir.join(name);
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Read both ClubLog answers back from `dir` and write there from now on.
    fn load_from(&self, dir: &Path, now: i64) {
        load_kept(&self.active, &dir.join(EXPEDITIONS_FILE), now);
        load_kept(&self.wanted, &dir.join(MOST_WANTED_FILE), now);
        *lock(&self.dir) = Some(dir.to_path_buf());
    }
}

/// A file that does not read is ignored (that source is simply asked when due). An ask time in
/// the future — a clock that was wrong, a file from a machine that is — is clamped to `now`:
/// left alone it would hold the next ask off until that future date plus a whole period.
fn load_kept<T: DeserializeOwned>(slot: &Mutex<Kept<T>>, path: &Path, now: i64) {
    let Some(mut k) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Kept<T>>(&t).ok())
    else {
        return;
    };
    k.asked_unix = k.asked_unix.min(now);
    *lock(slot) = k;
}

/// Where the ClubLog answers are kept across restarts. Called once at startup, before the
/// first [`fetch_plans`]; reads what an earlier run kept. Disk only, no network.
pub fn set_cache_dir(dir: &Path) {
    FEEDS.load_from(dir, now_unix());
}

/// The merged DXpedition plan list: NG3K's announced calendar overlaid with ClubLog's
/// currently-active callsigns (so within-window plans confirmed on the air are forced active)
/// and most-wanted ranks. Each source is asked only when its schedule says so (see the module
/// header); otherwise this is lock-only. `Err` only while NG3K has never answered.
pub fn fetch_plans() -> Result<Vec<DxpeditionPlan>, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())?;
    FEEDS.plans(&Net(c), now_unix())
}

/// The currently-active expedition calls from the CACHED plan list (no network) —
/// what the Needed board's DXpedition tagging reads. Cheap and lock-only, so it is
/// safe on every alerts poll; warmed by [`fetch_plans`] (a startup primer spawns
/// one). Calls are uppercased.
pub fn cached_active_calls(now_unix: i64) -> Vec<String> {
    lock(&FEEDS.merged)
        .iter()
        .filter(|p| p.start_unix <= now_unix && now_unix <= p.end_unix)
        .map(|p| p.call.to_uppercase())
        .collect()
}

/// What to print when ClubLog answers `mostwanted.php` with an `{"error": …}` body — Nexus's
/// own breadcrumb, or `None` when the body is not an error at all.
///
/// ⛔ **The return type is the guard.** A `&'static str` cannot be ClubLog's sentence, and
/// ClubLog's sentence is the thing that must not be printed: a service's refusal is the most
/// likely place for whatever the request carried to come back (this request carried the API key
/// until it was made public, `?api=1`) — and stderr is not a dev convenience on Linux, where the
/// desktop session redirects a GUI process's stderr into `~/.xsession-errors`, mode 0644, kept
/// until the next login. Same rule as `conn_log` in `src-tauri/src/lib.rs`: a service's own text
/// goes to a screen, never to something that outlives the moment.
///
/// Split out of [`most_wanted`] so it can be driven from a hostile body without a live HTTP
/// client — the round-4 fix shipped with nothing testing it.
fn most_wanted_error_breadcrumb(v: &serde_json::Value) -> Option<&'static str> {
    v.get("error")
        .and_then(|e| e.as_str())
        .map(|_| "propagation: ClubLog most-wanted refused the request")
}

/// ClubLog most-wanted list → entity-name → rank. Accepts both JSON shapes ClubLog has
/// used: {"1":"P5",...} (rank→prefix) and {"P5":1,...} (prefix→rank). Prefixes resolve
/// to entity names via cty.dat so plan calls can match regardless of the operation's
/// actual callsign. An error body or a list that resolves to nothing is a failure, so the
/// schedule backs off and the last good list is kept.
fn most_wanted(c: &reqwest::blocking::Client) -> Result<HashMap<String, u32>, String> {
    let v: serde_json::Value = c
        .get(MOST_WANTED_URL)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    // ClubLog reports problems as {"error": "..."} with HTTP 200 — say so rather than
    // silently behaving like an empty list.
    //
    // ⛔ The breadcrumb names the fetcher and NOT ClubLog's sentence: a service's own words
    // about a rejected request are the most likely place for what the request carried to come
    // back; and stderr is not a dev convenience on Linux, where the desktop session redirects
    // a GUI process's stderr into `~/.xsession-errors`, mode 0644, kept until the next login.
    // Same rule as `conn_log` in `src-tauri/src/lib.rs`: a service's own text goes to a
    // screen, never to something that outlives the moment.
    if let Some(crumb) = most_wanted_error_breadcrumb(&v) {
        eprintln!("{crumb}");
        return Err(crumb.to_string());
    }
    let mut out: HashMap<String, u32> = HashMap::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            // rank→prefix shape: key parses as the rank, value is the prefix.
            if let (Ok(rank), Some(prefix)) = (k.parse::<u32>(), val.as_str()) {
                if let Some(info) = dxcc::resolve(prefix) {
                    out.entry(info.entity.to_string()).or_insert(rank);
                }
            // prefix→rank shape.
            } else if let Some(rank) = val.as_u64() {
                if let Some(info) = dxcc::resolve(k) {
                    out.entry(info.entity.to_string()).or_insert(rank as u32);
                }
            }
        }
    }
    if out.is_empty() {
        return Err("ClubLog most-wanted list resolved to nothing".to_string());
    }
    Ok(out)
}

/// ClubLog's active-expeditions set (uppercased callsigns). An empty array is a real answer
/// (nobody on the air); any other shape is a failure, so the last good set is kept.
fn fetch_active(c: &reqwest::blocking::Client) -> Result<HashSet<String>, String> {
    let v: serde_json::Value = c
        .get(EXPEDITIONS_URL)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let Some(arr) = v.as_array() else {
        // Schema drift must be visible in the log, not a silent empty set.
        eprintln!("propagation: ClubLog expeditions.php unexpected JSON shape");
        return Err("ClubLog expeditions.php unexpected JSON shape".to_string());
    };
    let mut set = HashSet::new();
    for row in arr {
        if let Some(call) = row.get(0).and_then(|x| x.as_str()) {
            set.insert(call.to_uppercase());
        }
    }
    Ok(set)
}

/// Suffix/prefix-tolerant expedition-call match ("3Y0J/MM" ⇔ "3Y0J") — shared by
/// the active-overlay fork and the Needed board's DXpedition tagging. The shorter
/// call must be a whole `/`-delimited token at the start or end of the longer one;
/// a raw substring prefix does NOT count, or a bare-prefix plan call like "FO"
/// would wrongly tag every unrelated station in that prefix (e.g. "FO4BM").
pub fn call_matches(active: &str, plan_call: &str) -> bool {
    let (a, p) = (active.to_uppercase(), plan_call.to_uppercase());
    if a == p {
        return true;
    }
    let (long, short) = if a.len() >= p.len() { (a, p) } else { (p, a) };
    if short.is_empty() {
        return false;
    }
    // "3Y0J/MM" ⇔ "3Y0J" (base is the leading token), "OX/K1ABC" ⇔ "K1ABC"
    // (base is the trailing token), "FO/F6BCW" ⇔ "FO" (portable prefix token).
    long.strip_prefix(&short)
        .is_some_and(|r| r.starts_with('/'))
        || long.strip_suffix(&short).is_some_and(|r| r.ends_with('/'))
}

/// Parse the NG3K ADXO HTML table into plans (best-effort; tolerant of markup).
fn parse_adxo(html: &str) -> Vec<DxpeditionPlan> {
    // ASCII-lowercase preserves byte length, so indices line up with `html`.
    let lower = html.to_ascii_lowercase();
    let mut plans = Vec::new();
    let mut pos = 0usize;
    while let Some(off) = lower[pos..].find("<tr") {
        let row_start = pos + off;
        let row_end = lower[row_start + 3..]
            .find("</tr")
            .map(|e| row_start + 3 + e)
            .unwrap_or(lower.len());
        let cells = extract_cells(&html[row_start..row_end], &lower[row_start..row_end]);
        if let Some(p) = plan_from_cells(&cells) {
            plans.push(p);
        }
        pos = (row_end + 4).min(lower.len());
        if pos >= lower.len() {
            break;
        }
    }
    plans
}

/// One parsed `<td>`: the display text with tags stripped, plus the raw inner
/// HTML. The raw HTML is kept because the operation's own website is carried in
/// the MARKUP, not the text — NG3K links the callsign to it, and `strip_tags`
/// throws that away.
struct Cell {
    text: String,
    html: String,
}

fn extract_cells(orig: &str, lower: &str) -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut i = 0usize;
    while let Some(off) = lower[i..].find("<td") {
        let open = i + off;
        let Some(gt) = lower[open..].find('>') else {
            break;
        };
        let content_start = open + gt + 1;
        let Some(close) = lower[content_start..].find("</td") else {
            break;
        };
        let content_end = content_start + close;
        let html = &orig[content_start..content_end];
        cells.push(Cell {
            text: strip_tags(html),
            html: html.to_string(),
        });
        i = content_end + 4;
    }
    cells
}

/// The operation's own website from the CALL cell, if the source published one.
///
/// NG3K wraps the callsign in an anchor exactly when the operation has a site:
/// `<td class="call"><span class="call"><a href="https://3y0l.com/">3Y0L</a></span></td>`
/// (12 of 38 announcements on a sampled page). The same cell usually ALSO holds a
/// `<span class="spots">` anchor pointing at a DXWatch cluster query — that is a
/// spot search, not the expedition, so only the anchor inside the `call` span
/// counts. Anything that is not plain http/https is refused here rather than at
/// the point of opening: this HTML is third-party, and a `javascript:` or `file:`
/// href must never reach the operator's browser.
fn expedition_site(cell_html: &str) -> Option<String> {
    let lower = cell_html.to_ascii_lowercase();
    let span = lower.find("<span class=\"call\"")?;
    // Stay inside that span — the spots anchor lives in the NEXT one.
    let end = lower[span..]
        .find("</span")
        .map(|e| span + e)
        .unwrap_or(lower.len());
    let href = lower[span..end].find("href=\"")? + span + 6;
    let close = cell_html[href..].find('"')? + href;
    let url = cell_html[href..close].trim();
    let scheme_ok = {
        let u = url.to_ascii_lowercase();
        u.starts_with("http://") || u.starts_with("https://")
    };
    (scheme_ok && !url.contains(char::is_whitespace)).then(|| url.replace("&amp;", "&"))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn plan_from_cells(cells: &[Cell]) -> Option<DxpeditionPlan> {
    if cells.len() < 4 {
        return None;
    }
    let start = parse_date(&cells[0].text)?;
    let end = parse_date(&cells[1].text)?;
    let entity = cells[2].text.trim().to_string();
    let call = cells[3].text.split_whitespace().next()?.to_uppercase();
    if entity.is_empty() || call.is_empty() {
        return None;
    }
    let website = expedition_site(&cells[3].html);
    let info = cells[4..]
        .iter()
        .map(|c| c.text.clone())
        .max_by_key(|c| c.len())
        .unwrap_or_default();
    let up = info.to_uppercase();
    let ft8_mode = if up.contains("SUPER FOX") || up.contains("SUPERFOX") {
        Some(Ft8DxpMode::SuperFox)
    } else if up.contains("F/H") || up.contains("FOX") || up.contains("HOUND") {
        Some(Ft8DxpMode::FoxHound)
    } else if up.contains("MSHV") {
        Some(Ft8DxpMode::Mshv)
    } else {
        None
    };
    let grid = dxcc::resolve(&call).map(|i| latlon_to_maidenhead(i.lat, i.lon));

    Some(DxpeditionPlan {
        call,
        entity,
        grid,
        start_unix: start,
        end_unix: end,
        bands: parse_bands(&info),
        modes: parse_modes(&info),
        ft8_mode,
        most_wanted_rank: None,
        website,
    })
}

/// Parse an NG3K date cell like `"2026 Jun01"` to a Unix timestamp.
fn parse_date(cell: &str) -> Option<i64> {
    let mut it = cell.split_whitespace();
    let year: i64 = it.next()?.parse().ok()?;
    if !(2000..2100).contains(&year) {
        return None;
    }
    let md = it.next()?; // e.g. "Jun01"
    if md.len() < 4 {
        return None;
    }
    // `get`: third-party HTML — a multi-byte character across byte 3 is an unreadable date.
    let month = month_num(md.get(..3)?)?;
    let day: u32 = md.get(3..)?.trim().parse().ok()?;
    Some(ymd_to_unix(year, month, day))
}

fn month_num(m: &str) -> Option<u32> {
    Some(match m.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// Civil date → Unix seconds (Howard Hinnant's days_from_civil).
fn ymd_to_unix(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) * 86400
}

fn wl_to_band(wl: i32) -> Option<Band> {
    Some(match wl {
        160 => Band::B160,
        80 => Band::B80,
        40 => Band::B40,
        30 => Band::B30,
        20 => Band::B20,
        17 => Band::B17,
        15 => Band::B15,
        12 => Band::B12,
        10 => Band::B10,
        6 => Band::B6,
        4 => Band::B4,
        2 => Band::B2,
        _ => return None,
    })
}

fn add_wl(out: &mut Vec<Band>, wl: i32) {
    if let Some(b) = wl_to_band(wl) {
        if !out.contains(&b) {
            out.push(b);
        }
    }
}

/// Parse bands from an Info free-text ("HF", "80-6m", "20m", "HF + 6 4m", …).
fn parse_bands(info: &str) -> Vec<Band> {
    let up = info.to_uppercase();
    let mut out: Vec<Band> = Vec::new();
    if up.contains("HF") {
        for wl in [160, 80, 40, 30, 20, 17, 15, 12, 10] {
            add_wl(&mut out, wl);
        }
    }
    let ch: Vec<char> = up.chars().collect();
    let n = ch.len();
    let mut i = 0;
    while i < n {
        if !ch[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut num1 = 0i32;
        while j < n && ch[j].is_ascii_digit() {
            num1 = num1 * 10 + (ch[j] as i32 - '0' as i32);
            j += 1;
        }
        // Range "a-bM".
        if j < n && ch[j] == '-' {
            let mut k = j + 1;
            let mut num2 = 0i32;
            let mut got = false;
            while k < n && ch[k].is_ascii_digit() {
                num2 = num2 * 10 + (ch[k] as i32 - '0' as i32);
                k += 1;
                got = true;
            }
            if got && k < n && ch[k] == 'M' {
                let (hi, lo) = (num1.max(num2), num1.min(num2));
                for wl in [160, 80, 40, 30, 20, 17, 15, 12, 10, 6, 4, 2] {
                    if wl >= lo && wl <= hi {
                        add_wl(&mut out, wl);
                    }
                }
                i = k + 1;
                continue;
            }
        }
        // Single "nM".
        if j < n && ch[j] == 'M' {
            add_wl(&mut out, num1);
            i = j + 1;
            continue;
        }
        i = j;
    }
    out
}

/// Parse operating modes from an Info free-text.
fn parse_modes(info: &str) -> Vec<String> {
    let up = info.to_uppercase();
    let mut v = Vec::new();
    for kw in ["CW", "SSB", "FT8", "FT4", "RTTY", "PSK"] {
        if up.contains(kw) {
            v.push(kw.to_string());
        }
    }
    if (up.contains("DIGI") || up.contains("DATA")) && !v.iter().any(|m| m == "FT8" || m == "FT4") {
        v.push("Digital".to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};

    const H: i64 = 3600;
    const DAY: i64 = 86_400;
    /// A fixed, realistic start time (2026-10-30), so nothing reads the wall clock.
    const T0: i64 = 1_793_318_400;

    /// One NG3K row (Mozambique, C91RU) that both ClubLog answers below can act on.
    const ADXO: &str = r#"<table><tr><td>2026 Jun01</td><td>2026 Jun07</td><td>Mozambique</td><td>C91RU</td><td>[spots]</td><td>OQRS</td><td>NG3K</td><td>By team; HF; CW SSB FT8</td></tr></table>"#;

    /// Counts every ask, per source; each source answers or fails as told.
    struct Fake {
        ng3k_ok: bool,
        active_ok: bool,
        wanted_ok: bool,
        /// How long each ClubLog ask takes (for the single-flight test).
        delay: Duration,
        ng3k: AtomicUsize,
        active: AtomicUsize,
        wanted: AtomicUsize,
    }

    impl Fake {
        fn new(ng3k_ok: bool, active_ok: bool, wanted_ok: bool) -> Self {
            Fake {
                ng3k_ok,
                active_ok,
                wanted_ok,
                delay: Duration::ZERO,
                ng3k: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                wanted: AtomicUsize::new(0),
            }
        }
        fn clublog_asks(&self) -> usize {
            self.active.load(Ordering::SeqCst) + self.wanted.load(Ordering::SeqCst)
        }
    }

    fn mozambique() -> String {
        dxcc::resolve("C91RU")
            .expect("cty knows C9")
            .entity
            .to_string()
    }

    impl Source for Fake {
        fn ng3k(&self) -> Result<Vec<DxpeditionPlan>, String> {
            self.ng3k.fetch_add(1, Ordering::SeqCst);
            if self.ng3k_ok {
                Ok(parse_adxo(ADXO))
            } else {
                Err("ng3k down".into())
            }
        }
        fn active(&self) -> Result<HashSet<String>, String> {
            self.active.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if self.active_ok {
                Ok(HashSet::from(["C91RU".to_string()]))
            } else {
                Err("clublog down".into())
            }
        }
        fn most_wanted(&self) -> Result<HashMap<String, u32>, String> {
            self.wanted.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if self.wanted_ok {
                Ok(HashMap::from([(mozambique(), 42)]))
            } else {
                Err("clublog refused".into())
            }
        }
    }

    /// THE REPORTED BUG. `fetch_plans` runs on every propagation snapshot. With NG3K failing
    /// and nothing ever kept, every snapshot asked ClubLog `expeditions.php` again (it was
    /// asked first, before NG3K, and the failure path had no cache to back off on). Snapshots
    /// every 5 minutes for a whole day, plus a burst of 50 at one instant: ClubLog is asked
    /// once per endpoint, and NG3K itself is retried on a back-off, not per snapshot.
    #[test]
    fn a_dead_ng3k_costs_club_log_one_ask_per_endpoint_not_one_per_snapshot() {
        let feeds = Feeds::new();
        let fake = Fake::new(false, true, true);
        for _ in 0..50 {
            assert!(feeds.plans(&fake, T0).is_err());
        }
        let mut t = T0;
        while t < T0 + DAY {
            let _ = feeds.plans(&fake, t);
            t += 300;
        }
        assert_eq!(
            fake.active.load(Ordering::SeqCst),
            1,
            "expeditions.php asked more than once in a day"
        );
        assert_eq!(
            fake.wanted.load(Ordering::SeqCst),
            1,
            "mostwanted.php asked more than once"
        );
        // NG3K: 5, 10, 20 min, then every 30 min — about 50 asks a day, never one per snapshot
        // (338 calls here). And it IS retried: a dead source is not given up on.
        let ng3k = fake.ng3k.load(Ordering::SeqCst);
        assert!((2..=55).contains(&ng3k), "NG3K asked {ng3k} times in a day");
    }

    /// ⛔ A ClubLog failure is retried after 1 h, then 2, 4, 8, 16 h, then once a day — never
    /// sooner, and never longer than the normal period. Meanwhile the last good answer is served.
    #[test]
    fn a_club_log_failure_backs_off_from_an_hour_to_a_day_and_serves_the_last_good_answer() {
        let feeds = Feeds::new();
        let good = Fake::new(true, true, true);
        let first = feeds.plans(&good, T0).expect("all sources up");
        assert_eq!(first[0].most_wanted_rank, Some(42));

        // ClubLog goes down one normal period later; asked every minute for three days.
        let down = Fake::new(true, false, false);
        let mut asks: Vec<i64> = Vec::new();
        let mut t = T0 + DAY;
        while t < T0 + 4 * DAY {
            let before = down.active.load(Ordering::SeqCst);
            let plans = feeds.plans(&down, t).expect("NG3K still up");
            if down.active.load(Ordering::SeqCst) > before {
                asks.push(t);
            }
            // The rank comes from the kept most-wanted list, stale.
            assert_eq!(
                plans[0].most_wanted_rank,
                Some(42),
                "stale list dropped at {t}"
            );
            t += 60;
        }
        let gaps: Vec<i64> = asks.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(
            gaps[..5],
            [H, 2 * H, 4 * H, 8 * H, 16 * H],
            "the back-off must start at 1 h and double: {gaps:?}"
        );
        assert!(
            gaps[5..].iter().all(|&g| g == DAY),
            "capped at once a day: {gaps:?}"
        );
        // Most-wanted was last asked at T0 and is not due until 30 days later — the
        // expeditions outage does not touch it.
        assert_eq!(down.wanted.load(Ordering::SeqCst), 0);
    }

    /// A source that has never answered gets the same back-off (this was the hole: nothing
    /// kept, so nothing to back off on).
    #[test]
    fn a_source_that_never_answered_still_backs_off() {
        let feeds = Feeds::new();
        let down = Fake::new(true, false, false);
        let mut t = T0;
        while t < T0 + H {
            let _ = feeds.plans(&down, t);
            t += 2;
        }
        assert_eq!(down.active.load(Ordering::SeqCst), 1);
        assert_eq!(down.wanted.load(Ordering::SeqCst), 1);
        // Control: at the first retry, each is asked again.
        let _ = feeds.plans(&down, T0 + H);
        assert_eq!(down.clublog_asks(), 4);
    }

    /// Healthy for 60 days, a snapshot every 5 minutes: expeditions once a day, most-wanted
    /// once a month.
    #[test]
    fn healthy_club_log_is_asked_once_a_day_and_once_a_month() {
        let feeds = Feeds::new();
        let up = Fake::new(true, true, true);
        let mut t = T0;
        while t < T0 + 60 * DAY {
            let plans = feeds.plans(&up, t).expect("all up");
            assert_eq!(plans[0].most_wanted_rank, Some(42));
            t += 300;
        }
        assert_eq!(up.active.load(Ordering::SeqCst), 60);
        assert_eq!(up.wanted.load(Ordering::SeqCst), 2);
        assert_eq!(up.ng3k.load(Ordering::SeqCst), 60 * 48);
    }

    /// Concurrent callers (the startup warm thread and the first snapshot land together)
    /// make ONE ask per endpoint; the others are served what is kept.
    #[test]
    fn concurrent_callers_make_one_ask_per_endpoint() {
        let feeds = Feeds::new();
        let mut fake = Fake::new(true, true, true);
        fake.delay = Duration::from_millis(300);
        let gate = std::sync::Barrier::new(8);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    gate.wait();
                    let _ = feeds.plans(&fake, T0);
                });
            }
        });
        assert_eq!(fake.active.load(Ordering::SeqCst), 1);
        assert_eq!(fake.wanted.load(Ordering::SeqCst), 1);
        assert_eq!(fake.ng3k.load(Ordering::SeqCst), 1);
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-dxped-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// A restart does not ask ClubLog again: both answers and their ask times are on disk,
    /// failures included.
    #[test]
    fn club_log_answers_and_ask_times_survive_a_restart() {
        let dir = scratch_dir("restart");
        let first = Feeds::new();
        first.load_from(&dir, T0);
        let _ = first.plans(&Fake::new(true, true, true), T0);
        assert!(dir.join(EXPEDITIONS_FILE).is_file());
        assert!(dir.join(MOST_WANTED_FILE).is_file());

        // Relaunched an hour later.
        let second = Feeds::new();
        second.load_from(&dir, T0 + H);
        let fake = Fake::new(true, true, true);
        let plans = second.plans(&fake, T0 + H).expect("NG3K up");
        assert_eq!(fake.clublog_asks(), 0, "a restart asked ClubLog again");
        assert_eq!(
            plans[0].most_wanted_rank,
            Some(42),
            "kept list not read back"
        );
        // Control: once a day has passed, expeditions is asked again — and only it.
        let _ = second.plans(&fake, T0 + DAY);
        assert_eq!(fake.active.load(Ordering::SeqCst), 1);
        assert_eq!(fake.wanted.load(Ordering::SeqCst), 0);

        // A failed ask is kept too: a restart inside the back-off does not ask.
        let dir2 = scratch_dir("restart-fail");
        let third = Feeds::new();
        third.load_from(&dir2, T0);
        let _ = third.plans(&Fake::new(true, false, false), T0);
        let fourth = Feeds::new();
        fourth.load_from(&dir2, T0 + 60);
        let fake = Fake::new(true, true, true);
        let _ = fourth.plans(&fake, T0 + 60);
        assert_eq!(
            fake.clublog_asks(),
            0,
            "a restart after a failure asked again"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// An ask is on disk before it goes out: Nexus started again while the first run's ask is
    /// still out (or after that run was killed mid-ask) does not ask ClubLog a second time.
    #[test]
    fn a_relaunch_mid_ask_does_not_ask_again() {
        let dir = scratch_dir("mid-ask");
        let first = Feeds::new();
        first.load_from(&dir, T0);
        let mut slow = Fake::new(true, true, true);
        slow.delay = Duration::from_millis(400);
        std::thread::scope(|s| {
            s.spawn(|| first.plans(&slow, T0));
            std::thread::sleep(Duration::from_millis(150));
            let second = Feeds::new();
            second.load_from(&dir, T0 + 1);
            let fake = Fake::new(true, true, true);
            let _ = second.plans(&fake, T0 + 1);
            // The first run's expeditions ask is out (its most-wanted ask has not started, so
            // that one is the relaunch's to make).
            assert_eq!(
                fake.active.load(Ordering::SeqCst),
                0,
                "a relaunch mid-ask asked again"
            );
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An ask time in the future (a clock that was wrong) is clamped to now on load, so the
    /// next ask is one period away rather than a period after that future date.
    #[test]
    fn a_future_ask_time_on_disk_is_clamped_to_now() {
        let dir = scratch_dir("future");
        let ahead = Feeds::new();
        ahead.load_from(&dir, T0 + 3650 * DAY);
        let _ = ahead.plans(&Fake::new(true, true, true), T0 + 3650 * DAY);

        let back = Feeds::new();
        back.load_from(&dir, T0);
        assert_eq!(lock(&back.active).asked_unix, T0);
        let fake = Fake::new(true, true, true);
        let _ = back.plans(&fake, T0 + DAY - 1);
        assert_eq!(
            fake.active.load(Ordering::SeqCst),
            0,
            "asked before a day passed"
        );
        let _ = back.plans(&fake, T0 + DAY);
        assert_eq!(
            fake.active.load(Ordering::SeqCst),
            1,
            "held off by an ask time ten years ahead"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `mostwanted.php` is public: `api=1`, and no application key in ClubLog's request logs.
    #[test]
    fn most_wanted_is_asked_without_a_key() {
        assert_eq!(MOST_WANTED_URL, "https://clublog.org/mostwanted.php?api=1");
        assert_eq!(EXPEDITIONS_URL, "https://clublog.org/expeditions.php?api=1");
    }

    /// ⛔ CLUBLOG'S OWN WORDS DO NOT REACH STDERR.
    ///
    /// A refusal is the single most likely answer to quote back what the request carried
    /// (`mostwanted.php` was fetched with the API key until it was made public, `?api=1`) —
    /// and on Linux a GUI process's stderr is redirected by the desktop session into
    /// `~/.xsession-errors`, mode 0644, kept until the next login. This used to print
    /// ClubLog's `error` string verbatim.
    ///
    /// Driven straight through the decider, so no live client is needed; the call site is
    /// `eprintln!("{crumb}")` over this function's `&'static str`, which cannot carry a body.
    #[test]
    fn the_most_wanted_breadcrumb_carries_none_of_clublogs_answer() {
        const KEY: &str = "clubl0g4p1k3yAbCdEf0123456789xyz";
        let hostile = serde_json::json!({
            "error": format!("\u{202e}denied for api={KEY}\u{200b}")
        });
        // The control: the body really does carry the key, so the assertion below is about
        // the breadcrumb and not about the fixture having lost it.
        assert!(
            hostile.to_string().contains(KEY),
            "control: the hostile body must actually hold the key"
        );

        let crumb = most_wanted_error_breadcrumb(&hostile)
            .expect("an error body must still produce a breadcrumb");
        assert!(
            !crumb.contains(KEY) && !crumb.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "ClubLog's own answer reached stderr: {crumb:?}"
        );
        assert!(
            crumb.contains("refused"),
            "the breadcrumb must still say what happened: {crumb:?}"
        );

        // The other half of the guard: a real answer is not mistaken for an error, or the
        // fetcher would return an empty map on every good fetch.
        assert!(most_wanted_error_breadcrumb(&serde_json::json!({"1": "P5"})).is_none());
        assert!(most_wanted_error_breadcrumb(&serde_json::json!({"P5": 1})).is_none());
    }

    #[test]
    fn parses_an_adxo_row() {
        // The shape NG3K emits (cells: start, end, entity, call, spots, qsl, src, info).
        let html = r#"<table>
<tr><td>2026 May05</td><td>2026 Jul20</td><td>French Polynesia</td><td>FO</td><td>[spots]</td><td>LoTW</td><td>TDDX</td><td>By F6BCW as FO/F6BCW; 80-6m; CW SSB; QSL via OQRS</td></tr>
<tr><td>2026 Jun01</td><td>2026 Jun07</td><td>Mozambique</td><td>C91RU</td><td>[spots]</td><td>OQRS</td><td>NG3K</td><td>By team; HF; CW SSB FT8 super fox</td></tr>
</table>"#;
        let plans = parse_adxo(html);
        assert_eq!(plans.len(), 2);
        let fo = &plans[0];
        assert_eq!(fo.call, "FO");
        assert_eq!(fo.entity, "French Polynesia");
        assert!(fo.bands.contains(&Band::B20) && fo.bands.contains(&Band::B6));
        assert!(fo.modes.contains(&"CW".to_string()) && fo.modes.contains(&"SSB".to_string()));
        assert!(fo.grid.is_some()); // resolved via dxcc (FO)
        let c9 = &plans[1];
        assert_eq!(c9.entity, "Mozambique");
        assert_eq!(c9.ft8_mode, Some(Ft8DxpMode::SuperFox));
        assert!(c9.bands.contains(&Band::B20)); // HF expands
    }

    /// NG3K's page is third-party HTML. A date cell with a multi-byte character across the
    /// month's third byte split that character and panicked the whole fetch; it is a date this
    /// cannot read, and the row is skipped like any other.
    #[test]
    fn a_non_ascii_date_cell_is_unreadable_not_a_panic() {
        assert_eq!(parse_date("2026 ééé01"), None);
        assert_eq!(parse_date("2026 J日n01"), None);
        assert!(
            parse_date("2026 Jun01").is_some(),
            "control: a real date still reads"
        );
    }

    /// The call cell exactly as NG3K emits it, both shapes: an operation WITH its
    /// own site (the callsign is the anchor) and one without (only the DXWatch
    /// `[spots]` anchor). Sampled from the live page 2026-07-29.
    #[test]
    fn extracts_the_expedition_site_from_the_call_cell() {
        let with_site = r#"<span class="call"><a href="https://3y0l.com/">3Y0L</a></span>"#;
        assert_eq!(
            expedition_site(with_site).as_deref(),
            Some("https://3y0l.com/")
        );
    }

    #[test]
    fn the_spots_link_is_not_an_expedition_site() {
        // THE trap in this cell: 6 of 38 sampled rows carry a DXWatch cluster-query
        // anchor and no site. Taking the first href in the cell would send the
        // operator to a spot search and call it the expedition's webpage.
        let spots_only = r#"<span class="call">TY5FR</span><br><span class="spots">
              <a href="http://www.dxwatch.com/dxsd1.php?f=0&amp;t=dx&amp;c=TY5FR">[spots]</a>
               </span>"#;
        assert_eq!(expedition_site(spots_only), None);
        // …and when BOTH are present, the site wins and the spots query is ignored.
        let both = r#"<span class="call"><a href="https://vk9xy2026.com/">VK9XY</a></span><br><span class="spots"><a href="http://www.dxwatch.com/dxsd1.php?c=VK9XY">[spots]</a></span>"#;
        assert_eq!(
            expedition_site(both).as_deref(),
            Some("https://vk9xy2026.com/")
        );
    }

    #[test]
    fn refuses_a_non_http_scheme() {
        // This HTML is third-party. A javascript:/file: href must die at the parser,
        // not at the point where the operator's browser is asked to open it.
        for bad in [
            r#"<span class="call"><a href="javascript:alert(1)">X</a></span>"#,
            r#"<span class="call"><a href="file:///etc/passwd">X</a></span>"#,
            r#"<span class="call"><a href="/relative/path">X</a></span>"#,
        ] {
            assert_eq!(expedition_site(bad), None, "accepted {bad}");
        }
    }

    /// Both rows are copied verbatim from the live page (2026-07-29), whitespace
    /// included. That matters in the second one: `strip_tags` does not insert a
    /// word boundary where a tag was, so the only thing separating the callsign
    /// from the `[spots]` label is the source's own newline inside the span. Trim
    /// it and the cell text becomes "TY5FR[spots]" and the call stops resolving —
    /// so keep these fixtures byte-faithful rather than tidying them up.
    #[test]
    fn a_row_carries_its_site_through_to_the_plan() {
        let html = r#"<table>
<tr class="adxoitem"><td class="date">2026 Oct07</td><td class="date">2026 Oct21</td><td class="cty">Solomon Is</td><td class="call"><span class="call"><a href="https://solomon2026.com/">H49A</a></span></td><td class="qsl">LoTW</td><td class="rep">OPDX</td><td class="info">By team; HF + 6m; CW SSB FT8</td></tr>
<tr class="adxoitem"><td class="date">2026 Jul07</td><td class="date">2026 Aug04</td><td class="cty">Benin</td><td><span class="call">TY5FR</span><br><span class="spots">
			  <a href="http://www.dxwatch.com/dxsd1.php?c=TY5FR">[spots]</a>
			   </span></td><td class="qsl">LoTW</td><td class="rep">TDDX</td><td class="info">By DL1BUG; 160-10m; CW SSB</td></tr>
</table>"#;
        let plans = parse_adxo(html);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].call, "H49A");
        assert_eq!(
            plans[0].website.as_deref(),
            Some("https://solomon2026.com/")
        );
        // Unchanged behaviour: the call is still read from the cell TEXT, and the
        // spots anchor leaves the second operation with no site at all.
        assert_eq!(plans[1].call, "TY5FR");
        assert_eq!(plans[1].website, None);
    }

    #[test]
    fn band_range_and_single() {
        assert!(parse_bands("80-6m").contains(&Band::B40));
        assert!(parse_bands("only 6m here").contains(&Band::B6));
        assert!(!parse_bands("12m only").contains(&Band::B2)); // "12m" is not 2m
    }

    #[test]
    fn call_matches_requires_slash_boundary() {
        // Legit portable variants match (base as leading/trailing token, or a
        // portable prefix token), commutatively.
        assert!(call_matches("3Y0J/MM", "3Y0J"));
        assert!(call_matches("3Y0J", "3Y0J/MM"));
        assert!(call_matches("OX/K1ABC", "K1ABC"));
        assert!(call_matches("FO/F6BCW", "FO"));
        assert!(call_matches("W1AW", "W1AW"));

        // The bug: a bare-prefix plan call must NOT tag unrelated stations that
        // merely start with (or extend) it — no '/' boundary, no match.
        assert!(!call_matches("FO", "FO4BM"));
        assert!(!call_matches("FO4BM", "FO"));
        assert!(!call_matches("W1AW", "W1AWX"));
        assert!(!call_matches("K", "K1ABC"));
        assert!(!call_matches("", "FO"));
    }
}
