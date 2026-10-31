//! A solar flare as NOAA itself describes it: the D-RAP absorption grid, the flare event list,
//! the six-hour X-ray trace, and the polite fetch schedule they share.
//!
//! WHAT IS TRUTH HERE. NOAA's D-RAP grid (`drap_global_frequencies.txt`, 2° × 4°, every minute)
//! is the authoritative absorption footprint: it is NOAA's own model run on the measured flux
//! and protons, with NOAA's real proton cutoff. Nexus's analytic field ([`analytic_haf`], the
//! same D-RAP law: subsolar HAF `10·log10(flux) + 65` MHz tapered by `cos χ^0.75`) is only the
//! gap-filler between grid updates and when the grid is unreachable, and is labelled so
//! wherever it is drawn. [`agreement`] measures how far the two differ (the dev comparison).
//!
//! NOAA's event list (`xray-flares-latest.json`) gives the flare's begin / peak / end in NOAA's
//! words; a time NOAA has not called yet (`"Unk"` or null) is `None`, never a guess.
//!
//! THE FETCH RULE. Every product rides a [`Lane`]: the ask is stamped BEFORE it goes out and
//! counts as a failure until it answers, so concurrent callers never double-ask and a failing
//! product backs off (doubling, capped) whether or not it ever answered. A caller polling
//! faster than the lane's period is served the kept answer; it never causes a request.
//!
//! Every value carries its own time: the grid its `Product Valid At`, the event its NOAA
//! stamps, the trace each sample's GOES time. Nothing here is stamped with the fetch time.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kc2g::parse_naive_utc_unix;

/// NOAA's D-RAP global grid of Highest Affected Frequency (MHz): the frequency at which the
/// D-region absorbs 1 dB on a vertical path. Rows run north to south, columns west to east,
/// on a regular spacing; the columns span the whole globe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrapGrid {
    /// NOAA's `Product Valid At` (Unix seconds, UTC).
    pub valid_at: i64,
    /// NOAA's `Estimated Recovery Time`, when it gives one (`No Estimate` → `None`).
    pub recovery_at: Option<i64>,
    /// Row latitudes, north first (89 … −89).
    pub lats: Vec<f32>,
    /// Column longitudes, west first (−178 … 178).
    pub lons: Vec<f32>,
    /// HAF in MHz, `lats.len() × lons.len()`, row-major.
    pub haf: Vec<f32>,
}

impl DrapGrid {
    /// The HAF at a point, bilinear between the four surrounding cells. Latitudes beyond the
    /// outer rows take the outer row; longitudes wrap across the dateline.
    pub fn at(&self, lat: f64, lon: f64) -> f32 {
        let (rows, cols) = (self.lats.len(), self.lons.len());
        if rows < 2 || cols < 2 || self.haf.len() != rows * cols {
            return 0.0;
        }
        // Rows: north first, a regular step (checked at parse).
        let dlat = f64::from(self.lats[0] - self.lats[1]);
        let r = ((f64::from(self.lats[0]) - lat) / dlat).clamp(0.0, (rows - 1) as f64);
        let r0 = (r.floor() as usize).min(rows - 2);
        let fr = (r - r0 as f64) as f32;
        // Columns: wrap the whole globe.
        let dlon = f64::from(self.lons[1] - self.lons[0]);
        let c = (lon - f64::from(self.lons[0])) / dlon;
        let cf = c.floor();
        let c0 = (cf as i64).rem_euclid(cols as i64) as usize;
        let c1 = (c0 + 1) % cols;
        let fc = (c - cf) as f32;
        let v = |r: usize, c: usize| self.haf[r * cols + c];
        let top = v(r0, c0) * (1.0 - fc) + v(r0, c1) * fc;
        let bot = v(r0 + 1, c0) * (1.0 - fc) + v(r0 + 1, c1) * fc;
        top * (1.0 - fr) + bot * fr
    }

    /// The largest HAF anywhere in the grid (0 for an empty one).
    pub fn max(&self) -> f32 {
        self.haf.iter().copied().fold(0.0, f32::max)
    }
}

/// Parse `drap_global_frequencies.txt`. `None` for anything that is not a whole, dated grid —
/// an error page, a cut-short body, a row of the wrong width — so a broken answer never
/// replaces a good one.
pub fn parse_drap_grid(body: &str) -> Option<DrapGrid> {
    let mut valid_at = None;
    let mut recovery_at = None;
    let mut lons: Vec<f32> = Vec::new();
    let mut lats: Vec<f32> = Vec::new();
    let mut haf: Vec<f32> = Vec::new();
    for line in body.lines() {
        if let Some(h) = line.strip_prefix('#') {
            if let Some((k, v)) = h.split_once(':') {
                let v = v.trim().trim_end_matches("UTC").trim();
                match k.trim() {
                    "Product Valid At" => valid_at = parse_naive_utc_unix(v),
                    // "No Estimate" (or anything undated) is no time.
                    "Estimated Recovery Time" => recovery_at = parse_naive_utc_unix(v),
                    _ => {}
                }
            }
            continue;
        }
        if let Some((lat, cells)) = line.split_once('|') {
            let lat: f32 = lat.trim().parse().ok()?;
            let row: Vec<f32> = cells
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            if lons.is_empty() || row.len() != lons.len() {
                return None; // a row before the header, or of the wrong width (cut short)
            }
            lats.push(lat);
            haf.extend(row);
        } else if lons.is_empty() {
            // The longitude header: a line of numbers only (the dashed rule parses as none).
            let nums: Option<Vec<f32>> = line.split_whitespace().map(|t| t.parse().ok()).collect();
            if let Some(n) = nums.filter(|n| n.len() >= 2) {
                lons = n;
            }
        }
    }
    let regular = |v: &[f32], whole_globe: bool| {
        let step = v[1] - v[0];
        step != 0.0
            && v.windows(2).all(|w| ((w[1] - w[0]) - step).abs() < 1e-3)
            && (!whole_globe || ((step * v.len() as f32).abs() - 360.0).abs() < 1e-3)
    };
    // A whole grid only: the rows NOAA always sends, a regular spacing, columns round the globe.
    if lats.len() < 90 || lons.len() < 2 || !regular(&lats, false) || !regular(&lons, true) {
        return None;
    }
    if lats[0] < lats[1] || lons[0] > lons[1] {
        return None; // the sampler assumes north-first rows and west-first columns
    }
    Some(DrapGrid {
        valid_at: valid_at?,
        recovery_at,
        lats,
        lons,
        haf,
    })
}

/// One flare as NOAA's event list reports it. Times are Unix seconds, UTC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlareEvent {
    /// The list's own stamp (`time_tag`): when NOAA last updated this entry.
    pub at: i64,
    pub begin_at: i64,
    pub begin_class: Option<String>,
    /// `None` while NOAA has not called the peak.
    pub max_at: Option<i64>,
    pub max_class: Option<String>,
    /// The peak long-band flux, W/m².
    pub max_flux: Option<f32>,
    /// `None` while the flare is still running.
    pub end_at: Option<i64>,
    pub current_class: Option<String>,
}

/// The newest event in `xray-flares-latest.json` (an array of one). `None` without a dated
/// begin time.
pub fn parse_flare_latest(v: &Value) -> Option<FlareEvent> {
    let e = v.as_array()?.last()?;
    // "Unk", null or a missing field: NOAA has not called it.
    let time = |k: &str| {
        e.get(k)
            .and_then(Value::as_str)
            .and_then(parse_naive_utc_unix)
    };
    let class = |k: &str| {
        e.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty() && *c != "Unk")
            .map(str::to_string)
    };
    Some(FlareEvent {
        at: time("time_tag")?,
        begin_at: time("begin_time")?,
        begin_class: class("begin_class"),
        max_at: time("max_time"),
        max_class: class("max_class"),
        max_flux: e
            .get("max_xrlong")
            .and_then(Value::as_f64)
            .map(|f| f as f32),
        end_at: time("end_time"),
        current_class: class("current_class"),
    })
}

/// The GOES long-band (0.1–0.8 nm) trace from `xrays-6-hour.json` for the flare chip's
/// sparkline: one `(unix, flux)` per `bucket_secs`, the bucket's PEAK sample with its own time
/// (a short spike must survive the thinning), oldest first. Rows without a flux or a time are
/// skipped.
pub fn xray_series(v: &Value, bucket_secs: i64) -> Vec<(i64, f32)> {
    let bucket_secs = bucket_secs.max(1);
    let mut out: Vec<(i64, f32)> = Vec::new();
    let rows = v.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut samples: Vec<(i64, f32)> = rows
        .iter()
        .filter(|r| r.get("energy").and_then(Value::as_str) == Some("0.1-0.8nm"))
        .filter_map(|r| {
            let at = parse_naive_utc_unix(r.get("time_tag")?.as_str()?)?;
            let flux = r.get("flux")?.as_f64()? as f32;
            (flux > 0.0).then_some((at, flux))
        })
        .collect();
    samples.sort_by_key(|s| s.0);
    for s in samples {
        match out.last_mut() {
            Some(last) if last.0.div_euclid(bucket_secs) == s.0.div_euclid(bucket_secs) => {
                if s.1 > last.1 {
                    *last = s;
                }
            }
            _ => out.push(s),
        }
    }
    out
}

/// The analytic D-RAP HAF (MHz) at a point and instant for a long-band flux: NOAA's subsolar
/// law tapered by `cos χ^0.75`, zero on the night side. The gap-filler, never the truth layer.
pub fn analytic_haf(lat: f64, lon: f64, unix: i64, xray_long: f32) -> f32 {
    let cos_chi = crate::geo::solar_elevation_deg(lat, lon, unix)
        .to_radians()
        .sin();
    if cos_chi <= 0.0 {
        return 0.0;
    }
    crate::model::flare_haf_mhz(xray_long) * (cos_chi as f32).powf(0.75)
}

/// How far NOAA's grid and the analytic field differ, cell by cell, at the grid's valid time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Agreement {
    /// Median |grid − analytic| (MHz) where the sun is high (cos χ ≥ 0.5).
    pub dayside_median: f32,
    /// Median |grid − analytic| (MHz) in the taper toward the terminator (0 < cos χ < 0.5).
    pub taper_median: f32,
    /// Cells counted in each.
    pub dayside_cells: usize,
    pub taper_cells: usize,
}

/// Compare a grid with the analytic field for `xray_long` (the dev comparison script and its
/// success measure: dayside ≤ 1 MHz, taper ≤ 3 MHz).
pub fn agreement(grid: &DrapGrid, xray_long: f32) -> Agreement {
    let mut day = Vec::new();
    let mut taper = Vec::new();
    let cols = grid.lons.len();
    for (r, lat) in grid.lats.iter().enumerate() {
        for (c, lon) in grid.lons.iter().enumerate() {
            let (lat, lon) = (f64::from(*lat), f64::from(*lon));
            let cos_chi = crate::geo::solar_elevation_deg(lat, lon, grid.valid_at)
                .to_radians()
                .sin();
            if cos_chi <= 0.0 {
                continue;
            }
            let d =
                (grid.haf[r * cols + c] - analytic_haf(lat, lon, grid.valid_at, xray_long)).abs();
            if cos_chi >= 0.5 { &mut day } else { &mut taper }.push(d);
        }
    }
    Agreement {
        dayside_median: median(&mut day),
        taper_median: median(&mut taper),
        dayside_cells: day.len(),
        taper_cells: taper.len(),
    }
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 0 {
        (v[m - 1] + v[m]) / 2.0
    } else {
        v[m]
    }
}

/// Whether NOAA's D-RAP grid is worth asking for at this long-band flux: from M1 (R1), the
/// same onset where the map starts drawing the flare footprint. Below it nothing would draw
/// the grid, so a quiet sun costs NOAA no requests.
pub fn wants_drap(xray_long: f32) -> bool {
    crate::model::r_scale(xray_long) >= 1
}

/// How often one product is asked: `period` seconds after an answer; after `n` failures in a
/// row, `period · 2ⁿ` capped at `cap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    pub period: i64,
    pub cap: i64,
}

/// One polled product: the last good answer and when it may be asked again.
#[derive(Debug)]
pub struct Lane<T> {
    kept: Option<T>,
    /// Unix seconds of the last ask, answered or not; 0 = never asked.
    asked: i64,
    /// Failures in a row since the last answer. An ask still out counts as one.
    failures: u32,
}

impl<T> Lane<T> {
    pub const fn new() -> Self {
        Lane {
            kept: None,
            asked: 0,
            failures: 0,
        }
    }

    /// Whether the caller should fetch now. When it should, the ask is recorded first (and
    /// counted as a failure until [`Lane::answered`]), so a second caller is told no.
    pub fn claim(&mut self, now: i64, c: Cadence) -> bool {
        let wait = match self.failures {
            0 => c.period,
            n => c
                .period
                .saturating_mul(1i64 << n.min(30))
                .min(c.cap.max(c.period)),
        };
        // Never asked, or the clock stepped back past the last ask, or the wait is over.
        let due = self.asked == 0 || now < self.asked || now >= self.asked.saturating_add(wait);
        if due {
            self.asked = now;
            self.failures = self.failures.saturating_add(1);
        }
        due
    }

    /// The fetch answered: keep it and clear the failure count.
    pub fn answered(&mut self, v: T) {
        self.kept = Some(v);
        self.failures = 0;
    }

    /// The last good answer, however old (its own stamps say how old).
    pub fn kept(&self) -> Option<&T> {
        self.kept.as_ref()
    }
}

impl<T> Default for Lane<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Live SWPC products captured 2026-10-30 ~02:47 UT (public domain).
    const DRAP: &str =
        include_str!("../tests/fixtures/swpc-drap-global-frequencies-2026-10-30.txt");
    const FLARES_LATEST: &str =
        include_str!("../tests/fixtures/swpc-xray-flares-latest-2026-10-30.json");

    fn utc(s: &str) -> i64 {
        parse_naive_utc_unix(s).unwrap()
    }

    /// A body in NOAA's exact layout whose cells are the analytic field for `xray` at `valid`,
    /// scaled by `gain` — the stand-in for a flare-time grid (none was captured).
    fn synth_body(valid: i64, xray: f32, gain: f32, recovery: &str) -> String {
        let lons: Vec<i32> = (0..90).map(|i| -178 + 4 * i).collect();
        let mut s = String::new();
        s.push_str("# DRAP Tabular Values\n");
        let t = chrono_like(valid);
        s.push_str(&format!("# Product Valid At : {t} UTC\n#\n"));
        s.push_str(&format!("# Estimated Recovery Time : {recovery}\n#\n"));
        s.push_str("# Frequency (MHz) as a function of Latitude and Longitude\n#\n");
        s.push_str("     ");
        for l in &lons {
            s.push_str(&format!("{l:5}"));
        }
        s.push_str("\n-------------\n");
        for r in 0..90 {
            let lat = 89 - 2 * r;
            s.push_str(&format!("{lat:3} |"));
            for l in &lons {
                let v = analytic_haf(lat as f64, *l as f64, valid, xray) * gain;
                s.push_str(&format!("{v:5.1}"));
            }
            s.push('\n');
        }
        s
    }

    /// `YYYY-MM-DD HH:MM` for a Unix time (UTC), as NOAA writes it.
    fn chrono_like(unix: i64) -> String {
        let days = unix.div_euclid(86_400);
        let (y, m, d) = crate::geo::civil_from_days(days);
        let secs = unix.rem_euclid(86_400);
        format!(
            "{y:04}-{m:02}-{d:02} {:02}:{:02}",
            secs / 3600,
            secs / 60 % 60
        )
    }

    #[test]
    fn the_real_drap_file_parses_with_its_valid_time_and_shape() {
        let g = parse_drap_grid(DRAP).expect("NOAA's live file parses");
        assert_eq!(g.valid_at, utc("2026-10-30 02:47"));
        assert_eq!(g.recovery_at, None, "\"No Estimate\" is no time");
        assert_eq!((g.lats.len(), g.lons.len()), (90, 90));
        assert_eq!((g.lats[0], g.lats[89]), (89.0, -89.0));
        assert_eq!((g.lons[0], g.lons[89]), (-178.0, 178.0));
        assert_eq!(g.haf.len(), 8100);
        assert!(g.max() < 2.0, "a quiet sun: {}", g.max());
    }

    #[test]
    fn noaas_recovery_time_is_read_when_it_gives_one() {
        let valid = utc("2026-10-30 14:00");
        let g = parse_drap_grid(&synth_body(valid, 1e-4, 1.0, "2026-10-30 14:58 UTC")).unwrap();
        assert_eq!(g.recovery_at, Some(utc("2026-10-30 14:58")));
        assert_eq!(g.valid_at, valid);
        // Positive control: the synthesized X1 grid peaks near X1's 25 MHz.
        assert!((g.max() - 25.0).abs() < 1.0, "{}", g.max());
    }

    #[test]
    fn a_broken_answer_is_not_a_grid() {
        assert_eq!(parse_drap_grid("<html><body>503</body></html>"), None);
        assert_eq!(parse_drap_grid(""), None);
        // No valid time: undated, so not a reading.
        let undated: String = DRAP
            .lines()
            .filter(|l| !l.contains("Valid At"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(parse_drap_grid(&undated), None);
        // Cut short mid-row.
        let cut = &DRAP[..DRAP.len() - 40];
        assert_eq!(parse_drap_grid(cut), None);
        // Missing rows.
        let half: String = DRAP.lines().take(60).map(|l| format!("{l}\n")).collect();
        assert_eq!(parse_drap_grid(&half), None);
    }

    #[test]
    fn the_grid_samples_bilinearly_and_wraps_the_dateline() {
        let valid = utc("2026-10-30 18:00");
        let g = parse_drap_grid(&synth_body(valid, 2e-4, 1.0, "No Estimate")).unwrap();
        let cell = |r: usize, c: usize| g.haf[r * 90 + c];
        // A cell centre reads its own value.
        assert!((g.at(31.0, -90.0) - cell(29, 22)).abs() < 1e-4);
        // Halfway between two rows and two columns: the mean of the four.
        let mid = g.at(30.0, -88.0);
        let mean = (cell(29, 22) + cell(29, 23) + cell(30, 22) + cell(30, 23)) / 4.0;
        assert!((mid - mean).abs() < 1e-3, "{mid} vs {mean}");
        // Across the dateline: 180° sits halfway between the 178 and −178 columns.
        let wrap = g.at(31.0, 180.0);
        let pair = (cell(29, 89) + cell(29, 0)) / 2.0;
        assert!((wrap - pair).abs() < 1e-3, "{wrap} vs {pair}");
        assert!((g.at(31.0, -180.0) - wrap).abs() < 1e-4);
        // Beyond the outer rows: the outer row.
        assert!((g.at(90.0, -90.0) - cell(0, 22)).abs() < 1e-4);
        assert!((g.at(-90.0, -90.0) - cell(89, 22)).abs() < 1e-4);
    }

    #[test]
    fn the_latest_flare_event_reads_noaas_own_times_and_classes() {
        let v: Value = serde_json::from_str(FLARES_LATEST).unwrap();
        let e = parse_flare_latest(&v).unwrap();
        assert_eq!(e.at, utc("2026-10-30T02:46:00Z"));
        assert_eq!(e.begin_at, utc("2026-10-28T12:38:00Z"));
        assert_eq!(e.max_at, Some(utc("2026-10-28T12:52:00Z")));
        assert_eq!(e.end_at, Some(utc("2026-10-28T13:03:00Z")));
        assert_eq!(e.max_class.as_deref(), Some("B6.4"));
        assert_eq!(e.begin_class.as_deref(), Some("B3.2"));
        assert_eq!(e.current_class.as_deref(), Some("B2.1"));
        assert!((e.max_flux.unwrap() - 6.468e-7).abs() < 1e-9);
    }

    #[test]
    fn a_running_flare_has_no_peak_or_end_until_noaa_calls_them() {
        let v = serde_json::json!([{
            "time_tag": "2026-10-30T12:44:00Z", "begin_time": "2026-10-30T12:38:00Z",
            "begin_class": "C4.0", "max_time": "Unk", "max_class": null, "max_xrlong": null,
            "end_time": "Unk", "current_class": "M2.1"
        }]);
        let e = parse_flare_latest(&v).unwrap();
        assert_eq!((e.max_at, e.end_at), (None, None));
        assert_eq!(e.max_class, None);
        assert_eq!(e.current_class.as_deref(), Some("M2.1"));
        // No dated begin: not an event.
        let undated =
            serde_json::json!([{"time_tag": "2026-10-30T12:44:00Z", "begin_time": "Unk"}]);
        assert_eq!(parse_flare_latest(&undated), None);
        assert_eq!(parse_flare_latest(&serde_json::json!({})), None);
        assert_eq!(parse_flare_latest(&serde_json::json!([])), None);
    }

    #[test]
    fn the_trace_keeps_each_buckets_peak_with_its_own_time() {
        let t0 = utc("2026-10-30T12:00:00Z");
        let mut rows = Vec::new();
        for m in 0..30 {
            let flux = if m == 7 { 3e-5 } else { 2e-7 };
            let tag = |t: i64| {
                let s = chrono_like(t);
                format!("{}T{}:00Z", &s[..10], &s[11..])
            };
            rows.push(serde_json::json!({"time_tag": tag(t0 + m * 60), "flux": flux, "energy": "0.1-0.8nm"}));
            // The short band must be ignored.
            rows.push(serde_json::json!({"time_tag": tag(t0 + m * 60), "flux": 9e-4, "energy": "0.05-0.4nm"}));
        }
        rows.push(serde_json::json!({"time_tag": "2026-10-30T12:31:00Z", "flux": null, "energy": "0.1-0.8nm"}));
        let s = xray_series(&Value::Array(rows), 300);
        assert_eq!(s.len(), 6, "{s:?}");
        assert!(s.windows(2).all(|w| w[0].0 < w[1].0), "oldest first");
        // The one-minute M3 spike at 12:07 survives the thinning, at its own time.
        assert_eq!(s[1], (t0 + 7 * 60, 3e-5));
        assert!(
            s.iter().all(|p| p.1 < 1e-4),
            "the short band never leaks in"
        );
    }

    #[test]
    fn the_analytic_field_is_noaas_law_and_dark_at_night() {
        let t = utc("2026-06-21T12:00:00Z");
        // Near the subsolar point (≈ 23.4°N, ≈ 0°E at noon UT on the solstice): X1 → 25 MHz.
        let sub = analytic_haf(23.4, 0.5, t, 1e-4);
        assert!((sub - 25.0).abs() < 0.3, "{sub}");
        // Night side: nothing.
        assert_eq!(analytic_haf(-23.4, 180.0, t, 1e-4), 0.0);
        // The taper: 60° from the sun, cos χ = 0.5 → 25 · 0.5^0.75 ≈ 14.9.
        let off = analytic_haf(23.4 - 60.0, 0.5, t, 1e-4);
        assert!((off - 25.0 * 0.5f32.powf(0.75)).abs() < 0.5, "{off}");
        // A quiet sun has no HF footprint at all.
        assert_eq!(analytic_haf(23.4, 0.5, t, 1e-7), 0.0);
    }

    #[test]
    fn agreement_is_zero_for_the_same_law_and_measures_a_real_difference() {
        let valid = utc("2026-10-30 18:00");
        let same = parse_drap_grid(&synth_body(valid, 1e-4, 1.0, "No Estimate")).unwrap();
        let a = agreement(&same, 1e-4);
        assert!(a.dayside_median < 0.1 && a.taper_median < 0.1, "{a:?}");
        assert!(a.dayside_cells > 500 && a.taper_cells > 500, "{a:?}");
        // Negative control: a grid 20 % hotter than the GOES flux implies disagrees by ~3–5 MHz
        // under the sun, and less in the taper where the numbers are smaller.
        let hot = parse_drap_grid(&synth_body(valid, 1e-4, 1.2, "No Estimate")).unwrap();
        let b = agreement(&hot, 1e-4);
        assert!(b.dayside_median > 3.0 && b.dayside_median < 5.0, "{b:?}");
        assert!(b.taper_median < b.dayside_median, "{b:?}");
    }

    #[test]
    fn the_grid_is_asked_for_only_at_a_flare() {
        assert!(!wants_drap(9.9e-6), "C9.9: the map draws nothing");
        assert!(wants_drap(1e-5), "M1: the footprint starts");
        assert!(wants_drap(3e-4));
        assert!(!wants_drap(0.0) && !wants_drap(f32::NAN));
    }

    const C: Cadence = Cadence {
        period: 60,
        cap: 900,
    };

    #[test]
    fn a_lane_asks_once_per_period_and_serves_the_kept_answer_between() {
        let mut l: Lane<u32> = Lane::new();
        assert!(l.claim(1000, C), "never asked: ask");
        assert!(!l.claim(1000, C), "in flight: a second caller does not ask");
        l.answered(7);
        assert!(!l.claim(1030, C), "inside the period");
        assert!(!l.claim(1059, C));
        assert_eq!(l.kept(), Some(&7));
        assert!(l.claim(1060, C), "a period later");
    }

    /// The hole Club Log fell into: a product that has NEVER answered must back off too, not
    /// be asked on every poll.
    #[test]
    fn a_failing_lane_backs_off_even_if_it_never_answered() {
        let mut l: Lane<u32> = Lane::new();
        let mut asks = Vec::new();
        for now in (0..3600).step_by(5) {
            if l.claim(10_000 + now, C) {
                asks.push(now); // ...and never answers
            }
        }
        // 0, then 120, 240, 480, 900 (capped) between asks.
        assert_eq!(asks, vec![0, 120, 360, 840, 1740, 2640, 3540], "{asks:?}");
        // One answer resets the schedule to the plain period.
        l.answered(1);
        assert!(!l.claim(10_000 + 3540 + 59, C));
        assert!(l.claim(10_000 + 3540 + 60, C));
    }

    #[test]
    fn a_failing_lane_keeps_serving_its_last_answer() {
        let mut l: Lane<&str> = Lane::new();
        assert!(l.claim(0, C));
        l.answered("good");
        assert!(l.claim(60, C)); // this one fails: no answered()
        assert_eq!(l.kept(), Some(&"good"));
        assert!(!l.claim(150, C), "backing off after the failure");
        assert!(l.claim(180, C));
    }

    /// A clock stepped backwards must not wedge a lane until the old time comes round again.
    #[test]
    fn a_clock_stepped_back_does_not_wedge_the_lane() {
        let mut l: Lane<u8> = Lane::new();
        assert!(l.claim(100_000, C));
        l.answered(1);
        assert!(l.claim(50_000, C));
    }
}
