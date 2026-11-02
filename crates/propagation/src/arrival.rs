//! "Coming" — what NOAA expects to reach Earth over the next days, and the moment it does.
//!
//! A flare is never "coming": its X-rays arrive at light speed, so it is here the instant GOES
//! sees it. What genuinely travels is the solar wind — a coronal mass ejection or a coronal-hole
//! high-speed stream takes one to three days from the sun — and that is what decides whether
//! tonight's HF is worth sitting down for. This module reads the four public NOAA SWPC products
//! that answer "is something coming, and when", and keeps the three ways of knowing apart:
//!
//! 1. **NOAA's watch** (`products/alerts.json`, message codes `WATA…`): the forecasters' call, a
//!    G level per UT day. The newest geomagnetic watch supersedes every earlier one (the bulletin
//!    says so in as many words), and a `CANCEL WATCH` ends it. Plus the scales product's day
//!    blocks (`products/noaa-scales.json`): the forecast G level and the R/S probabilities per
//!    day, which the space-weather strip does not read.
//! 2. **The model** (`json/enlil_time_series.json`): WSA-Enlil's predicted solar wind at Earth.
//!    The arrival window is the first stretch where the modelled speed rises by
//!    [`MODEL_RISE_KMS`] inside [`MODEL_RISE_SECS`] or the model marks a CME cloud, bracketed by
//!    ±[`MODEL_BRACKET_SECS`] (the model's typical arrival-time error). A model run, never a fact.
//! 3. **The measurement** (`products/geospace/propagated-solar-wind-1-hour.json`): the solar
//!    wind measured at L1 with the time it reaches Earth. A step in speed or field strength there
//!    is the one thing that may be called "arrived".
//!
//! Plus the forecasters' own reasoning, the geomagnetic "Rationale:" paragraph of
//! `text/3-day-forecast.txt`, with its issue time.
//!
//! ⚠️ NEVER MERGED. Each source keeps its own time stamp and its own word all the way to the
//! screen; nothing here turns a watch or a model run into a reading. Pure parsing only (no `live`
//! feature); the fetchers are in [`crate::live::arrival`] and the caching in the shell.

use serde::Serialize;
use serde_json::Value;

use crate::geo::{civil_from_days, days_from_civil};
use crate::kc2g::parse_naive_utc_unix;

/// A modelled speed rise at least this large…
pub const MODEL_RISE_KMS: f32 = 100.0;
/// …inside this long is read as the model's arrival.
pub const MODEL_RISE_SECS: i64 = 6 * 3600;
/// The window drawn around the model's arrival: WSA-Enlil's typical arrival-time error.
pub const MODEL_BRACKET_SECS: i64 = 6 * 3600;
/// The model trace is downsampled to this step (the file carries a sample every ~2 min).
pub const MODEL_STEP_SECS: i64 = 15 * 60;
/// A measured jump at L1: the mean speed of the [`JUMP_WINDOW`] samples after a minute exceeds
/// the mean of the ones before by at least this much…
pub const JUMP_SPEED_KMS: f32 = 50.0;
/// …or the field strength at least doubles to at least [`JUMP_BT_MIN_NT`]. Nexus's own
/// threshold for "something arrived", not a NOAA shock catalogue.
pub const JUMP_BT_RATIO: f32 = 2.0;
pub const JUMP_BT_MIN_NT: f32 = 10.0;
/// One-minute samples either side of a candidate jump.
pub const JUMP_WINDOW: usize = 5;

const DAY: i64 = 86_400;

/// One UT day of a NOAA geomagnetic watch: the highest G level predicted for it (0 = below G1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchDay {
    /// 00:00 UTC of the day, Unix seconds.
    pub day: i64,
    pub g: u8,
}

/// A NOAA geomagnetic storm watch, as issued.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Watch {
    /// NOAA's serial number — the toast's one-per-watch key.
    pub serial: u32,
    /// The message code, e.g. "WATA20".
    pub code: String,
    /// Issue time, Unix seconds UTC.
    pub issued: i64,
    pub days: Vec<WatchDay>,
    /// The forecasters' comment when the bulletin carries one (often the cause: "CH HSS", "CME").
    pub comment: Option<String>,
    /// Start of the first day at G1 or above … end of the last one. `None` when every day is
    /// below G1.
    pub window: Option<(i64, i64)>,
    /// The highest level on any day.
    pub max_g: u8,
}

/// One forecast day from `noaa-scales.json`: the G level and the R/S probabilities (percent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaleDay {
    /// 00:00 UTC of the day NOAA dates the block with.
    pub day: i64,
    pub g: u8,
    /// Chance of R1–R2 (minor) radio blackouts.
    pub r_minor_pct: Option<u8>,
    /// Chance of R3 or greater.
    pub r_major_pct: Option<u8>,
    /// Chance of S1 or greater.
    pub s_pct: Option<u8>,
}

/// NOAA's forecast days and when NOAA stamped the product.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaleOutlook {
    pub issued: Option<i64>,
    pub days: Vec<ScaleDay>,
}

/// One downsampled WSA-Enlil sample at Earth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPoint {
    pub at: i64,
    /// Radial speed, km/s.
    pub speed: f32,
    /// Density, particles/cm³.
    pub density: f32,
    /// The model marks this sample as inside a CME cloud.
    pub cloud: bool,
}

/// The model's arrival: the rise, bracketed by the model's typical error.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelArrival {
    /// Where the rise (or the cloud) begins.
    pub rise_at: i64,
    /// Window start/end: the rise ± [`MODEL_BRACKET_SECS`].
    pub start: i64,
    pub end: i64,
    /// Modelled speed before and after the rise.
    pub speed_from: f32,
    pub speed_to: f32,
    /// The model drew a CME cloud, not only a faster stream.
    pub cloud: bool,
}

/// One propagated L1 sample: the wind measured at L1 and when it reaches Earth.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct L1Sample {
    /// Measured at L1, Unix seconds.
    pub at: i64,
    /// NOAA's propagated arrival at Earth, Unix seconds.
    pub arrives: i64,
    pub speed: Option<f32>,
    pub bt: Option<f32>,
}

/// A measured step at L1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct L1Jump {
    /// The first sample after the step, measured at L1.
    pub at: i64,
    /// When that wind reaches Earth (NOAA's propagation).
    pub arrives: i64,
    pub speed_before: Option<f32>,
    pub speed_after: Option<f32>,
    pub bt_before: Option<f32>,
    pub bt_after: Option<f32>,
}

/// The forecasters' geomagnetic reasoning from the 3-day forecast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rationale {
    pub issued: i64,
    /// NOAA's own words, one paragraph, untranslated.
    pub text: String,
}

/// The model as served: its run (the file's `Last-Modified`), when Nexus fetched it, and what it
/// says about the days ahead.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    pub run_at: Option<i64>,
    pub fetched_at: i64,
    /// The fastest modelled wind from now on.
    pub peak: Option<ModelPoint>,
    pub arrival: Option<ModelArrival>,
}

/// L1 as served: the newest measured minute (absent once it is too old to call current) and the
/// newest measured step (kept for hours: it is a dated event, not a reading).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct L1View {
    pub latest: Option<L1Sample>,
    pub jump: Option<L1Jump>,
}

/// Everything "coming", each part with its own source and time, absent when it is not known.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrivalOutlook {
    pub watch: Option<Watch>,
    pub model: Option<ModelView>,
    pub l1: Option<L1View>,
    pub rationale: Option<Rationale>,
}

// ---- watches -------------------------------------------------------------------------------

/// Every geomagnetic watch and watch cancellation in `alerts.json`, oldest first. A cancellation
/// comes back as a `Watch` with no days and its `serial` set to the serial it cancels.
fn watch_bulletins(alerts: &Value) -> Vec<(Watch, bool)> {
    let Some(arr) = alerts.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<(Watch, bool)> = arr
        .iter()
        .filter_map(|item| {
            let msg = item.get("message")?.as_str()?;
            let issued = item
                .get("issue_datetime")
                .and_then(|x| x.as_str())
                .and_then(parse_naive_utc_unix)?;
            parse_watch_message(msg, issued)
        })
        .collect();
    out.sort_by_key(|(w, _)| w.issued);
    out
}

/// One bulletin → a watch (`false`) or a cancellation (`true`); `None` for anything that is not a
/// geomagnetic watch.
fn parse_watch_message(msg: &str, issued: i64) -> Option<(Watch, bool)> {
    let code = field(msg, "Space Weather Message Code:")?;
    if !code.starts_with("WATA") {
        return None;
    }
    let upper = msg.to_ascii_uppercase();
    if upper.contains("CANCEL WATCH") {
        let serial = field(msg, "Cancel Serial Number:")?.parse().ok()?;
        return Some((
            Watch {
                serial,
                code,
                issued,
                days: Vec::new(),
                comment: None,
                window: None,
                max_g: 0,
            },
            true,
        ));
    }
    let serial: u32 = field(msg, "Serial Number:")?.parse().ok()?;
    let days = watch_days(msg, issued);
    if days.is_empty() {
        return None;
    }
    let comment = field(msg, "Comment:").filter(|c| !c.is_empty());
    let max_g = days.iter().map(|d| d.g).max().unwrap_or(0);
    let stormy: Vec<&WatchDay> = days.iter().filter(|d| d.g > 0).collect();
    let window = match (stormy.first(), stormy.last()) {
        (Some(a), Some(b)) => Some((a.day, b.day + DAY)),
        _ => None,
    };
    Some((
        Watch {
            serial,
            code,
            issued,
            days,
            comment,
            window,
            max_g,
        },
        false,
    ))
}

/// The trimmed text after `key` on the first line that starts with it.
fn field(msg: &str, key: &str) -> Option<String> {
    msg.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(key))
        .map(|v| v.trim().to_string())
}

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

fn month_of(s: &str) -> Option<u32> {
    let s = s.to_ascii_uppercase();
    MONTHS.iter().position(|m| *m == s).map(|i| i as u32 + 1)
}

/// "Highest Storm Level Predicted by Day: Oct 30:  None (Below G1)   Oct 31:  G1 (Minor) …".
/// The bulletin names month and day only; the year is the issue year, rolled forward when a
/// December watch names January.
fn watch_days(msg: &str, issued: i64) -> Vec<WatchDay> {
    let Some(start) = msg.find("Predicted by Day:") else {
        return Vec::new();
    };
    let rest = &msg[start + "Predicted by Day:".len()..];
    let end = rest.find("THIS SUPERSEDES").unwrap_or(rest.len());
    let toks: Vec<&str> = rest[..end].split_whitespace().collect();
    let (iy, im, _) = civil_from_days(issued.div_euclid(DAY));
    let mut out = Vec::new();
    for w in toks.windows(3) {
        let (Some(m), Some(d)) = (
            month_of(w[0]),
            w[1].strip_suffix(':').and_then(|d| d.parse::<u32>().ok()),
        ) else {
            continue;
        };
        if !(1..=31).contains(&d) {
            continue;
        }
        let g = if w[2].eq_ignore_ascii_case("None") {
            0
        } else if let Some(n) = w[2].strip_prefix('G').and_then(|n| n.parse::<u8>().ok()) {
            n.min(5)
        } else {
            continue;
        };
        let year = if m < im && im - m > 6 { iy + 1 } else { iy };
        out.push(WatchDay {
            day: days_from_civil(year, m, d) * DAY,
            g,
        });
    }
    out
}

/// The geomagnetic watch in force at `now`: the newest watch (it supersedes every earlier one),
/// unless a later bulletin cancelled it or its last stormy day is over.
pub fn active_watch(alerts: &Value, now: i64) -> Option<Watch> {
    let bulletins = watch_bulletins(alerts);
    let (newest, _) = bulletins.iter().rev().find(|(_, c)| !c)?;
    let cancelled = bulletins
        .iter()
        .any(|(w, c)| *c && w.serial == newest.serial && w.issued >= newest.issued);
    if cancelled {
        return None;
    }
    match newest.window {
        Some((_, end)) if end > now => Some(newest.clone()),
        _ => None,
    }
}

// ---- scales --------------------------------------------------------------------------------

fn scale_num(v: Option<&Value>) -> Option<u8> {
    let v = v?;
    v.as_str()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .or_else(|| v.as_u64().and_then(|n| u8::try_from(n).ok()))
}

/// The forecast day blocks "1".."3" of `noaa-scales.json`, each dated by its own `DateStamp`,
/// plus the product's issue stamp (block "0"'s `DateStamp` + `TimeStamp`).
pub fn parse_scale_outlook(v: &Value) -> ScaleOutlook {
    let stamp = |b: &Value| -> Option<i64> {
        let d = b.get("DateStamp")?.as_str()?;
        let t = b
            .get("TimeStamp")
            .and_then(|x| x.as_str())
            .unwrap_or("00:00:00");
        parse_naive_utc_unix(&format!("{d} {t}"))
    };
    let issued = v.get("0").and_then(stamp);
    let days = ["1", "2", "3"]
        .iter()
        .filter_map(|k| {
            let b = v.get(*k)?;
            let d = b.get("DateStamp")?.as_str()?;
            let day = parse_naive_utc_unix(&format!("{d} 00:00:00"))?;
            Some(ScaleDay {
                day,
                g: scale_num(b.get("G").and_then(|g| g.get("Scale")))
                    .unwrap_or(0)
                    .min(5),
                r_minor_pct: scale_num(b.get("R").and_then(|r| r.get("MinorProb"))),
                r_major_pct: scale_num(b.get("R").and_then(|r| r.get("MajorProb"))),
                s_pct: scale_num(b.get("S").and_then(|s| s.get("Prob"))),
            })
        })
        .collect();
    ScaleOutlook { issued, days }
}

// ---- the model -----------------------------------------------------------------------------

/// `enlil_time_series.json` → the trace at Earth, oldest first, one sample per
/// [`MODEL_STEP_SECS`] (the first in each step). Rows without a time or a finite speed are
/// skipped.
pub fn parse_enlil(v: &Value) -> Vec<ModelPoint> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut pts: Vec<ModelPoint> = arr
        .iter()
        .filter_map(|r| {
            let at = parse_naive_utc_unix(r.get("time_tag")?.as_str()?)?;
            let speed = r.get("v_r")?.as_f64().filter(|x| x.is_finite())? as f32;
            let density = r
                .get("earth_particles_per_cm3")
                .and_then(|x| x.as_f64())
                .filter(|x| x.is_finite())
                .unwrap_or(0.0) as f32;
            let cloud = r.get("cloud").is_some_and(|c| !c.is_null());
            Some(ModelPoint {
                at,
                speed,
                density,
                cloud,
            })
        })
        .collect();
    pts.sort_by_key(|p| p.at);
    let mut out: Vec<ModelPoint> = Vec::new();
    for p in pts {
        let bucket = p.at.div_euclid(MODEL_STEP_SECS);
        match out.last_mut() {
            Some(last) if last.at.div_euclid(MODEL_STEP_SECS) == bucket => {
                // Keep the bucket's first sample, but never lose a cloud flag inside it.
                last.cloud |= p.cloud;
            }
            _ => out.push(p),
        }
    }
    out
}

/// The model's next arrival: scanning from `now − MODEL_BRACKET_SECS`, the first sample that is
/// a CME cloud, or from which the speed rises by [`MODEL_RISE_KMS`] within [`MODEL_RISE_SECS`].
pub fn model_arrival(points: &[ModelPoint], now: i64) -> Option<ModelArrival> {
    let from = now - MODEL_BRACKET_SECS;
    for (i, p) in points.iter().enumerate().filter(|(_, p)| p.at >= from) {
        if p.cloud {
            return Some(ModelArrival {
                rise_at: p.at,
                start: p.at - MODEL_BRACKET_SECS,
                end: p.at + MODEL_BRACKET_SECS,
                speed_from: p.speed,
                speed_to: points[i..]
                    .iter()
                    .take_while(|q| q.cloud)
                    .map(|q| q.speed)
                    .fold(p.speed, f32::max),
                cloud: true,
            });
        }
        let rise = points[i + 1..]
            .iter()
            .take_while(|q| q.at - p.at <= MODEL_RISE_SECS)
            .find(|q| q.speed - p.speed >= MODEL_RISE_KMS);
        if let Some(q) = rise {
            return Some(ModelArrival {
                rise_at: p.at,
                start: p.at - MODEL_BRACKET_SECS,
                end: q.at + MODEL_BRACKET_SECS,
                speed_from: p.speed,
                speed_to: q.speed,
                cloud: false,
            });
        }
    }
    None
}

/// The fastest modelled sample from `now` on.
pub fn model_peak(points: &[ModelPoint], now: i64) -> Option<ModelPoint> {
    points
        .iter()
        .filter(|p| p.at >= now)
        .copied()
        .max_by(|a, b| a.speed.total_cmp(&b.speed))
}

// ---- L1 ------------------------------------------------------------------------------------

/// `propagated-solar-wind-1-hour.json`: a header row naming the columns, then one row a minute.
/// Rows without both stamps are skipped; a null speed or field stays `None`. Oldest first.
pub fn parse_propagated_wind(v: &Value) -> Vec<L1Sample> {
    let Some(rows) = v.as_array() else {
        return Vec::new();
    };
    let Some(header) = rows.first().and_then(|h| h.as_array()) else {
        return Vec::new();
    };
    let col = |name: &str| header.iter().position(|h| h.as_str() == Some(name));
    let (Some(ti), Some(pi)) = (col("time_tag"), col("propagated_time_tag")) else {
        return Vec::new();
    };
    let (si, bi) = (col("speed"), col("bt"));
    let num = |r: &[Value], i: Option<usize>| -> Option<f32> {
        let v = r.get(i?)?;
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .filter(|x| x.is_finite() && *x >= 0.0)
            .map(|x| x as f32)
    };
    let mut out: Vec<L1Sample> = rows[1..]
        .iter()
        .filter_map(|r| {
            let r = r.as_array()?;
            Some(L1Sample {
                at: parse_naive_utc_unix(r.get(ti)?.as_str()?)?,
                arrives: parse_naive_utc_unix(r.get(pi)?.as_str()?)?,
                speed: num(r, si),
                bt: num(r, bi),
            })
        })
        .collect();
    out.sort_by_key(|s| s.at);
    out
}

fn mean(xs: impl Iterator<Item = Option<f32>>) -> Option<f32> {
    let v: Vec<f32> = xs.flatten().collect();
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// The largest measured step in the hour (not necessarily the newest): the split with the
/// largest speed rise among those that pass [`JUMP_SPEED_KMS`], or failing that the largest field
/// ratio that passes [`JUMP_BT_RATIO`]/[`JUMP_BT_MIN_NT`]. `None` on a steady wind, however fast.
pub fn detect_jump(samples: &[L1Sample]) -> Option<L1Jump> {
    if samples.len() < 2 * JUMP_WINDOW {
        return None;
    }
    let mut best_speed: Option<(f32, L1Jump)> = None;
    let mut best_bt: Option<(f32, L1Jump)> = None;
    for i in JUMP_WINDOW..=samples.len() - JUMP_WINDOW {
        let before = &samples[i - JUMP_WINDOW..i];
        let after = &samples[i..i + JUMP_WINDOW];
        let jump = L1Jump {
            at: samples[i].at,
            arrives: samples[i].arrives,
            speed_before: mean(before.iter().map(|s| s.speed)),
            speed_after: mean(after.iter().map(|s| s.speed)),
            bt_before: mean(before.iter().map(|s| s.bt)),
            bt_after: mean(after.iter().map(|s| s.bt)),
        };
        if let (Some(a), Some(b)) = (jump.speed_before, jump.speed_after) {
            let d = b - a;
            if d >= JUMP_SPEED_KMS && best_speed.as_ref().is_none_or(|(x, _)| d > *x) {
                best_speed = Some((d, jump));
            }
        }
        if let (Some(a), Some(b)) = (jump.bt_before, jump.bt_after) {
            if a > 0.0 && b >= JUMP_BT_MIN_NT {
                let r = b / a;
                if r >= JUMP_BT_RATIO && best_bt.as_ref().is_none_or(|(x, _)| r > *x) {
                    best_bt = Some((r, jump));
                }
            }
        }
    }
    best_speed.or(best_bt).map(|(_, j)| j)
}

// ---- the 3-day forecast --------------------------------------------------------------------

/// "2026 Oct 31 0030" → Unix seconds. `None` unless the time is exactly four ASCII digits, so a
/// garbled stamp is no stamp: checking only its byte length would let the hour slice below cut
/// through a multibyte character and panic.
fn parse_swpc_issue(s: &str) -> Option<i64> {
    let mut it = s.split_whitespace();
    let y: i64 = it.next()?.parse().ok()?;
    let m = month_of(it.next()?)?;
    let d: u32 = it.next()?.parse().ok()?;
    let hm = it.next()?;
    if hm.len() != 4
        || !hm.bytes().all(|b| b.is_ascii_digit())
        || !(1900..=2100).contains(&y)
        || !(1..=31).contains(&d)
    {
        return None;
    }
    let (h, mi): (i64, i64) = (hm[..2].parse().ok()?, hm[2..].parse().ok()?);
    if !(0..24).contains(&h) || !(0..60).contains(&mi) {
        return None;
    }
    Some(days_from_civil(y, m, d) * DAY + h * 3600 + mi * 60)
}

/// The geomagnetic (section A) "Rationale:" paragraph of `3-day-forecast.txt`, joined onto one
/// line, with the product's `:Issued:` time. `None` when either is missing.
pub fn parse_three_day_rationale(txt: &str) -> Option<Rationale> {
    let issued = txt
        .lines()
        .find_map(|l| l.trim().strip_prefix(":Issued:"))
        .and_then(|s| parse_swpc_issue(s.trim().trim_end_matches("UTC")))?;
    let a = txt.find("A. NOAA Geomagnetic")?;
    let section = &txt[a..];
    let section = &section[..section.find("\nB. ").unwrap_or(section.len())];
    let r = section.find("Rationale:")?;
    let mut words = Vec::new();
    for line in section[r + "Rationale:".len()..].lines() {
        let line = line.trim();
        if line.is_empty() {
            if words.is_empty() {
                continue;
            }
            break;
        }
        words.push(line);
    }
    let text = words.join(" ");
    (!text.is_empty()).then_some(Rationale { issued, text })
}

/// An HTTP date ("Sat, 31 Oct 2026 04:13:31 GMT", RFC 9110's IMF-fixdate) → Unix seconds. Used
/// for the model file's `Last-Modified`, the only run time the trace has.
pub fn parse_http_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let s = s.split_once(',').map_or(s, |(_, r)| r).trim();
    let mut it = s.split_whitespace();
    let d: u32 = it.next()?.parse().ok()?;
    let m = month_of(it.next()?)?;
    let y: i64 = it.next()?.parse().ok()?;
    let hms = it.next()?;
    if it.next()? != "GMT" {
        return None;
    }
    parse_naive_utc_unix(&format!("{y:04}-{m:02}-{d:02} {hms}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALERTS: &str = include_str!("../tests/fixtures/swpc-alerts-watches-2026-10-31.json");
    const SCALES: &str = include_str!("../tests/fixtures/swpc-noaa-scales-2026-10-31.json");
    const ENLIL: &str =
        include_str!("../tests/fixtures/swpc-enlil-time-series-2026-10-30-decimated.json");
    const L1: &str = include_str!("../tests/fixtures/swpc-propagated-solar-wind-2026-10-31.json");
    const THREE_DAY: &str = include_str!("../tests/fixtures/swpc-3-day-forecast-2026-10-31.txt");

    fn at(s: &str) -> i64 {
        parse_naive_utc_unix(s).unwrap()
    }

    fn watch_msg(serial: u32, issue: &str, days: &str) -> Value {
        json!({
            "product_id": "A20F",
            "issue_datetime": issue,
            "message": format!(
                "Space Weather Message Code: WATA20\r\nSerial Number: {serial}\r\nIssue Time: x\r\n\r\n\
                 WATCH: Geomagnetic Storm Category G1 Predicted \r\nHighest Storm Level Predicted by Day:\r\n\
                 {days}\r\nTHIS SUPERSEDES ANY/ALL PRIOR WATCHES IN EFFECT\r\nComment: \r\n"
            )
        })
    }

    /// The real bulletins: three G1 watches and three K-index warnings/alerts. The newest watch
    /// (serial 1134, Oct 30 None, Oct 31 G1, Nov 01 G1) is the one in force; the K bulletins are
    /// not watches.
    #[test]
    fn the_newest_real_watch_is_in_force_with_its_days_and_window() {
        let v: Value = serde_json::from_str(ALERTS).unwrap();
        let w = active_watch(&v, at("2026-10-31 04:00:00")).expect("watch 1134 is in force");
        assert_eq!(w.serial, 1134);
        assert_eq!(w.code, "WATA20");
        assert_eq!(w.issued, at("2026-10-29 19:10:52"));
        assert_eq!(
            w.days.iter().map(|d| (d.day, d.g)).collect::<Vec<_>>(),
            vec![
                (at("2026-10-30 00:00:00"), 0),
                (at("2026-10-31 00:00:00"), 1),
                (at("2026-11-01 00:00:00"), 1),
            ]
        );
        assert_eq!(
            w.window,
            Some((at("2026-10-31 00:00:00"), at("2026-11-02 00:00:00")))
        );
        assert_eq!(w.max_g, 1);
        assert_eq!(w.comment, None, "an empty Comment: is no comment");
    }

    /// Positive and negative control on time: before the window ends the watch is in force;
    /// once its last stormy day is over it is not, even though it is still the newest.
    #[test]
    fn a_watch_whose_last_stormy_day_is_over_is_not_in_force() {
        let v: Value = serde_json::from_str(ALERTS).unwrap();
        assert!(active_watch(&v, at("2026-11-01 23:59:00")).is_some());
        assert!(active_watch(&v, at("2026-11-02 00:00:00")).is_none());
    }

    #[test]
    fn a_newer_watch_supersedes_and_a_cancel_ends_it() {
        let older = watch_msg(
            7,
            "2026-10-01 10:00:00.000",
            "Oct 02:  G2 (Moderate)   Oct 03:  None (Below G1)",
        );
        let newer = watch_msg(
            8,
            "2026-10-01 18:00:00.000",
            "Oct 02:  G1 (Minor)   Oct 03:  G1 (Minor)",
        );
        let now = at("2026-10-01 20:00:00");
        let v = json!([older.clone(), newer.clone()]);
        let w = active_watch(&v, now).unwrap();
        assert_eq!((w.serial, w.max_g), (8, 1), "the newest watch supersedes");

        let cancel = json!({
            "product_id": "A20C",
            "issue_datetime": "2026-10-01 19:00:00.000",
            "message": "Space Weather Message Code: WATA20\r\nSerial Number: 9\r\nIssue Time: x\r\n\r\n\
                        CANCEL WATCH: Geomagnetic Storm Category G1 Predicted\r\nCancel Serial Number: 8\r\n"
        });
        assert!(active_watch(&json!([older, newer, cancel]), now).is_none());
    }

    /// A cancel ends only the watch it names: one naming an older serial leaves the newest watch
    /// in force.
    #[test]
    fn a_cancel_naming_another_serial_leaves_the_newest_watch() {
        let older = watch_msg(7, "2026-10-01 10:00:00.000", "Oct 02:  G1 (Minor)");
        let newer = watch_msg(8, "2026-10-01 18:00:00.000", "Oct 02:  G2 (Moderate)");
        let cancel_older = json!({
            "product_id": "A20C",
            "issue_datetime": "2026-10-01 19:00:00.000",
            "message": "Space Weather Message Code: WATA20\r\nSerial Number: 9\r\nIssue Time: x\r\n\r\n\
                        CANCEL WATCH: Geomagnetic Storm Category G1 Predicted\r\nCancel Serial Number: 7\r\n"
        });
        let w = active_watch(
            &json!([older, newer, cancel_older]),
            at("2026-10-01 20:00:00"),
        )
        .expect("a cancel of serial 7 ended watch 8");
        assert_eq!((w.serial, w.max_g), (8, 2));
    }

    #[test]
    fn a_december_watch_naming_january_rolls_the_year() {
        let v = json!([watch_msg(
            1,
            "2026-12-31 20:00:00.000",
            "Dec 31:  None (Below G1)   Jan 01:  G3 (Strong)"
        )]);
        let w = active_watch(&v, at("2026-12-31 21:00:00")).unwrap();
        assert_eq!(
            w.window,
            Some((at("2027-01-01 00:00:00"), at("2027-01-02 00:00:00")))
        );
        assert_eq!(w.max_g, 3);
    }

    #[test]
    fn no_watches_or_junk_is_no_watch() {
        assert!(active_watch(&json!([]), 0).is_none());
        assert!(active_watch(&json!({}), 0).is_none());
        // A watch with every day below G1 has no window.
        let v = json!([watch_msg(
            2,
            "2026-10-01 10:00:00.000",
            "Oct 02:  None (Below G1)"
        )]);
        assert!(active_watch(&v, at("2026-10-01 11:00:00")).is_none());
    }

    #[test]
    fn scale_days_carry_their_own_dates_and_probabilities() {
        let v: Value = serde_json::from_str(SCALES).unwrap();
        let s = parse_scale_outlook(&v);
        assert_eq!(s.issued, Some(at("2026-10-31 04:14:00")));
        assert_eq!(s.days.len(), 3);
        assert_eq!(s.days[0].day, at("2026-10-31 00:00:00"));
        assert_eq!(s.days[1].day, at("2026-11-01 00:00:00"));
        assert_eq!(
            s.days.iter().map(|d| d.g).collect::<Vec<_>>(),
            vec![1, 1, 0]
        );
        assert_eq!(s.days[0].r_minor_pct, Some(1));
        assert_eq!(s.days[0].r_major_pct, Some(1));
        assert_eq!(s.days[0].s_pct, Some(1));
        assert!(parse_scale_outlook(&json!({})).days.is_empty());
    }

    #[test]
    fn the_model_trace_is_time_ordered_and_downsampled() {
        let v: Value = serde_json::from_str(ENLIL).unwrap();
        let pts = parse_enlil(&v);
        assert!(pts.len() > 100);
        assert!(pts
            .windows(2)
            .all(|w| w[1].at - w[0].at >= MODEL_STEP_SECS - 60));
        // The real run: the stream peaks near 517 km/s on 31 Oct.
        let peak = model_peak(&pts, at("2026-10-30 03:00:00")).unwrap();
        assert!((500.0..530.0).contains(&peak.speed), "peak {}", peak.speed);
        assert!(peak.at > at("2026-10-31 12:00:00") && peak.at < at("2026-11-01 06:00:00"));
        // A slow-rising stream (≈10 km/s an hour) never rises 100 km/s inside 6 h: no arrival.
        assert!(model_arrival(&pts, at("2026-10-30 03:00:00")).is_none());
    }

    #[test]
    fn a_modelled_step_or_cloud_is_an_arrival_bracketed_six_hours() {
        let t0 = at("2026-11-01 00:00:00");
        let mk = |h: i64, v: f32, cloud: bool| ModelPoint {
            at: t0 + h * 3600,
            speed: v,
            density: 5.0,
            cloud,
        };
        let step = vec![
            mk(0, 350.0, false),
            mk(3, 360.0, false),
            mk(6, 380.0, false),
            mk(8, 520.0, false),
        ];
        let a = model_arrival(&step, t0).unwrap();
        assert_eq!(
            a.rise_at,
            t0 + 3 * 3600,
            "the first sample from which it rises 100 inside 6 h"
        );
        assert_eq!((a.start, a.end), (t0 - 3 * 3600, t0 + 14 * 3600));
        assert_eq!((a.speed_from, a.speed_to, a.cloud), (360.0, 520.0, false));

        let cloud = vec![
            mk(0, 350.0, false),
            mk(5, 400.0, true),
            mk(6, 430.0, true),
            mk(9, 380.0, false),
        ];
        let c = model_arrival(&cloud, t0).unwrap();
        assert_eq!(
            (c.rise_at, c.cloud, c.speed_to),
            (t0 + 5 * 3600, true, 430.0)
        );
        // An arrival already more than the bracket in the past is not "coming".
        assert!(model_arrival(&step, t0 + 20 * 3600).is_none());
    }

    /// The peak is the fastest wind still AHEAD: a faster sample already behind us is not named.
    #[test]
    fn the_model_peak_never_names_a_time_already_past() {
        let t0 = at("2026-11-01 00:00:00");
        let mk = |h: i64, v: f32| ModelPoint {
            at: t0 + h * 3600,
            speed: v,
            density: 5.0,
            cloud: false,
        };
        let pts = vec![mk(0, 600.0), mk(6, 450.0), mk(12, 480.0), mk(18, 400.0)];
        let p = model_peak(&pts, t0 + 3 * 3600).unwrap();
        assert_eq!((p.at, p.speed), (t0 + 12 * 3600, 480.0));
        assert!(
            model_peak(&pts, t0 + 19 * 3600).is_none(),
            "nothing ahead, no peak"
        );
    }

    #[test]
    fn the_cloud_flag_survives_downsampling() {
        let v = json!([
            {"time_tag":"2026-11-01T00:01:00","v_r":400.0,"earth_particles_per_cm3":5.0,"cloud":null},
            {"time_tag":"2026-11-01T00:03:00","v_r":410.0,"earth_particles_per_cm3":5.0,"cloud":1.0},
            {"time_tag":"2026-11-01T00:16:00","v_r":"junk","cloud":null}
        ]);
        let pts = parse_enlil(&v);
        assert_eq!(pts.len(), 1);
        assert!(pts[0].cloud);
    }

    #[test]
    fn real_l1_wind_parses_with_both_stamps_and_a_steady_hour_is_no_jump() {
        let v: Value = serde_json::from_str(L1).unwrap();
        let s = parse_propagated_wind(&v);
        assert!(s.len() > 30);
        assert_eq!(s[0].at, at("2026-10-31 03:15:00"));
        assert_eq!(s[0].arrives, at("2026-10-31 04:20:13"));
        assert_eq!(s[0].speed, Some(335.5));
        assert_eq!(s[0].bt, Some(9.27));
        assert!(
            detect_jump(&s).is_none(),
            "a steady real hour was read as an arrival"
        );
        assert!(parse_propagated_wind(&json!([])).is_empty());
    }

    #[test]
    fn a_speed_step_at_l1_is_a_jump_at_the_step() {
        let t0 = at("2026-11-01 02:00:00");
        let s: Vec<L1Sample> = (0..20)
            .map(|i| L1Sample {
                at: t0 + i * 60,
                arrives: t0 + i * 60 + 3300,
                speed: Some(if i < 12 { 400.0 } else { 520.0 }),
                bt: Some(6.0),
            })
            .collect();
        let j = detect_jump(&s).unwrap();
        assert_eq!(
            j.at,
            t0 + 12 * 60,
            "the jump is dated at the step, not where the means first part"
        );
        assert_eq!(j.arrives, t0 + 12 * 60 + 3300);
        assert_eq!((j.speed_before, j.speed_after), (Some(400.0), Some(520.0)));
    }

    #[test]
    fn a_field_doubling_is_a_jump_and_a_small_field_doubling_is_not() {
        let t0 = 1_800_000_000;
        let series = |lo: f32, hi: f32| -> Vec<L1Sample> {
            (0..12)
                .map(|i| L1Sample {
                    at: t0 + i * 60,
                    arrives: t0 + i * 60 + 3000,
                    speed: Some(420.0),
                    bt: Some(if i < 6 { lo } else { hi }),
                })
                .collect()
        };
        assert_eq!(
            detect_jump(&series(6.0, 18.0)).map(|j| j.at),
            Some(t0 + 6 * 60)
        );
        assert!(
            detect_jump(&series(2.0, 5.0)).is_none(),
            "a doubling to 5 nT is noise"
        );
    }

    #[test]
    fn the_real_rationale_and_its_issue_time() {
        let r = parse_three_day_rationale(THREE_DAY).unwrap();
        assert_eq!(r.issued, at("2026-10-31 00:30:00"));
        assert_eq!(
            r.text,
            "The geomagnetic field is likely to reach G1 (Minor) geomagnetic storm levels on 31 Oct - \
             01 Nov due to influence from a positive polarity CH HSS. Unsettled conditions are likely \
             on 02 Nov as CH HSS influence wanes."
        );
        assert!(parse_three_day_rationale("no product here").is_none());
    }

    /// A garbled `:Issued:` time is no time, never a panic: four bytes that are not four ASCII
    /// digits (a multibyte character the hour would be cut through, a sign) are refused before
    /// the hour is sliced, and the rationale without its time is not served.
    #[test]
    fn a_garbled_issue_time_is_no_time_not_a_panic() {
        for hm in ["0é3", "é00", "+130", "-030", "0a30"] {
            assert_eq!(
                hm.len(),
                4,
                "{hm} must pass the length check to reach the slice"
            );
            assert_eq!(parse_swpc_issue(&format!("2026 Oct 31 {hm}")), None, "{hm}");
            let txt = THREE_DAY.replace("2026 Oct 31 0030 UTC", &format!("2026 Oct 31 {hm} UTC"));
            assert_ne!(txt, THREE_DAY, "the fixture's stamp moved");
            assert!(parse_three_day_rationale(&txt).is_none(), "{hm}");
        }
        // CONTROL: the well-formed stamp still reads.
        assert_eq!(
            parse_swpc_issue("2026 Oct 31 0030"),
            Some(at("2026-10-31 00:30:00"))
        );
    }

    #[test]
    fn http_dates() {
        assert_eq!(
            parse_http_date("Sat, 31 Oct 2026 04:13:31 GMT"),
            Some(at("2026-10-31 04:13:31"))
        );
        assert_eq!(parse_http_date("yesterday"), None);
    }
}
