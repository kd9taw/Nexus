//! Live DXpedition feed (the `live` feature): NG3K ADXO (the forward **calendar**
//! of announced operations — dates, entity, bands, modes) overlaid with ClubLog
//! `expeditions.php` (which calls are **active on the air now**). Each plan's
//! location comes from [`crate::dxcc`] so distance/bearing/region work.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::dxcc;
use crate::dxped::{DxpeditionPlan, Ft8DxpMode};
use crate::geo::latlon_to_maidenhead;
use crate::model::Band;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The ClubLog API key (from Settings), pushed in by the command layer at startup
/// and on settings save — keeps the live-fetch layer decoupled from settings IO.
/// Empty = no key → the most-wanted ranks simply stay `None`.
static CLUBLOG_KEY: Mutex<String> = Mutex::new(String::new());
pub fn set_clublog_key(key: &str) {
    if let Ok(mut k) = CLUBLOG_KEY.lock() {
        *k = key.trim().to_string();
    }
}

/// Plans cache: NG3K is a daily-updated page and ClubLog's active list moves
/// slowly — refetching on every 5-min snapshot miss was needless, and a transient
/// outage emptied the whole DXpedition board. 30 min TTL, and on a fetch FAILURE
/// the last-good list is served stale (an expedition board that flickers empty is
/// worse than one 30 minutes old).
static PLANS_CACHE: Mutex<Option<(Instant, Vec<DxpeditionPlan>)>> = Mutex::new(None);
const PLANS_TTL_SECS: u64 = 1800;

/// Most-wanted cache (entity name → ClubLog rank). The list changes ~monthly;
/// 24 h TTL is generous.
static MOST_WANTED: Mutex<Option<(Instant, HashMap<String, u32>)>> = Mutex::new(None);
const MOST_WANTED_TTL_SECS: u64 = 86_400;

/// Fetch the merged DXpedition plan list: NG3K's announced calendar overlaid
/// with ClubLog's currently-active callsigns (so within-window plans confirmed
/// on the air are forced active). Cached (30 min TTL, stale-on-error).
pub fn fetch_plans() -> Result<Vec<DxpeditionPlan>, String> {
    {
        let cache = PLANS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((when, plans)) = cache.as_ref() {
            if when.elapsed().as_secs() < PLANS_TTL_SECS {
                return Ok(plans.clone());
            }
        }
    }
    match fetch_plans_uncached() {
        Ok(plans) => {
            let mut cache = PLANS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some((Instant::now(), plans.clone()));
            Ok(plans)
        }
        Err(e) => {
            // Serve the last-good list stale rather than an empty board — and advance
            // the timestamp to a short-retry point (5 min) so an outage is retried on
            // a BACKOFF, not with a full 2-host network round-trip on every call.
            let mut cache = PLANS_CACHE.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((when, plans)) = cache.as_mut() {
                *when = Instant::now() - Duration::from_secs(PLANS_TTL_SECS.saturating_sub(300));
                return Ok(plans.clone());
            }
            Err(e)
        }
    }
}

/// The currently-active expedition calls from the CACHED plan list (no network) —
/// what the Needed board's DXpedition tagging reads. Cheap and lock-only, so it is
/// safe on every alerts poll; warmed by [`fetch_plans`] (a startup primer spawns
/// one). Calls are uppercased.
pub fn cached_active_calls(now_unix: i64) -> Vec<String> {
    let cache = PLANS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match cache.as_ref() {
        Some((_, plans)) => plans
            .iter()
            .filter(|p| p.start_unix <= now_unix && now_unix <= p.end_unix)
            .map(|p| p.call.to_uppercase())
            .collect(),
        None => Vec::new(),
    }
}

fn fetch_plans_uncached() -> Result<Vec<DxpeditionPlan>, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())?;

    let active = fetch_active(&c).unwrap_or_default();
    let html = c
        .get("https://www.ng3k.com/Misc/adxo.html")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;

    let mut plans = parse_adxo(&html);
    let now = now_unix();
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
    // ClubLog most-wanted rank (needs an API key; absent → ranks stay None). The
    // WorkableCard priority formula already weights this — it was just never fed.
    let wanted = most_wanted(&c);
    if !wanted.is_empty() {
        for p in &mut plans {
            if p.most_wanted_rank.is_none() {
                if let Some(info) = dxcc::resolve(&p.call) {
                    p.most_wanted_rank = wanted.get(info.entity).copied();
                }
            }
        }
    }
    Ok(plans)
}

/// ClubLog most-wanted list → entity-name → rank, cached 24 h. Accepts both JSON
/// shapes ClubLog has used: {"1":"P5",...} (rank→prefix) and {"P5":1,...}
/// (prefix→rank). Prefixes resolve to entity names via cty.dat so plan calls can
/// match regardless of the operation's actual callsign.
fn most_wanted(c: &reqwest::blocking::Client) -> HashMap<String, u32> {
    {
        let cache = MOST_WANTED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((when, map)) = cache.as_ref() {
            if when.elapsed().as_secs() < MOST_WANTED_TTL_SECS {
                return map.clone();
            }
        }
    }
    let mut key = CLUBLOG_KEY.lock().map(|k| k.clone()).unwrap_or_default();
    if key.is_empty() {
        // Parity with the upload path: a build-time key works there too.
        key = option_env!("CLUBLOG_API_KEY").unwrap_or("").to_string();
    }
    if key.is_empty() {
        return HashMap::new();
    }
    let mut out: HashMap<String, u32> = HashMap::new();
    let url = format!("https://clublog.org/mostwanted.php?api={key}");
    if let Ok(resp) = c.get(&url).send() {
        if let Ok(v) = resp.json::<serde_json::Value>() {
            // ClubLog reports problems as {"error": "..."} with HTTP 200 — surface it
            // rather than silently behaving like "no key".
            if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                eprintln!("propagation: ClubLog most-wanted error: {err}");
                return HashMap::new();
            }
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
        }
    }
    if !out.is_empty() {
        let mut cache = MOST_WANTED.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some((Instant::now(), out.clone()));
    }
    out
}

/// ClubLog's active-expeditions set (uppercased callsigns).
fn fetch_active(c: &reqwest::blocking::Client) -> Result<HashSet<String>, String> {
    let v: serde_json::Value = c
        .get("https://clublog.org/expeditions.php?api=1")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let mut set = HashSet::new();
    if let Some(arr) = v.as_array() {
        for row in arr {
            if let Some(call) = row.get(0).and_then(|x| x.as_str()) {
                set.insert(call.to_uppercase());
            }
        }
    } else {
        // Schema drift must be visible in the log, not a silent empty set.
        eprintln!("propagation: ClubLog expeditions.php unexpected JSON shape");
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
    let month = month_num(&md[..3])?;
    let day: u32 = md[3..].trim().parse().ok()?;
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
