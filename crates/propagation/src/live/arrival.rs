//! "Coming" fetchers (the `live` feature).
//!
//! Networked half of [`crate::arrival`]: one function per NOAA SWPC product, each a single GET
//! that hands its body to the pure parser. No caching or retry here — the shell owns both, per
//! product, so a failing product backs off on its own and never costs a request per poll.
//!
//! Sizes on 2026-10-31 (raw / on the wire with gzip): alerts 58 KB / ~10 KB, scales 1 KB, the
//! WSA-Enlil trace 1.5 MB / ~150 KB, the propagated L1 wind 6 KB, the 3-day forecast 2 KB.

use std::time::Duration;

use serde_json::Value;

use crate::arrival::{
    active_watch, parse_enlil, parse_http_date, parse_propagated_wind, parse_scale_outlook,
    parse_three_day_rationale, L1Sample, ModelPoint, Rationale, ScaleOutlook, Watch,
};

const UA: &str = "nexus-propagation/0.1 (+ham radio space weather)";
const ALERTS_URL: &str = "https://services.swpc.noaa.gov/products/alerts.json";
const SCALES_URL: &str = "https://services.swpc.noaa.gov/products/noaa-scales.json";
const ENLIL_URL: &str = "https://services.swpc.noaa.gov/json/enlil_time_series.json";
const L1_URL: &str =
    "https://services.swpc.noaa.gov/products/geospace/propagated-solar-wind-1-hour.json";
const THREE_DAY_URL: &str = "https://services.swpc.noaa.gov/text/3-day-forecast.txt";

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())
}

fn get(url: &str) -> Result<reqwest::blocking::Response, String> {
    client()?
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())
}

fn get_json(url: &str) -> Result<Value, String> {
    get(url)?.json::<Value>().map_err(|e| e.to_string())
}

/// The geomagnetic watch in force at `now`, if any. `Ok(None)` is a real answer: no watch.
pub fn fetch_watch(now: i64) -> Result<Option<Watch>, String> {
    let v = get_json(ALERTS_URL)?;
    if !v.is_array() {
        return Err("alerts.json is not a list".into());
    }
    Ok(active_watch(&v, now))
}

/// NOAA's forecast days (G level, R/S probabilities).
pub fn fetch_scale_outlook() -> Result<ScaleOutlook, String> {
    let s = parse_scale_outlook(&get_json(SCALES_URL)?);
    if s.days.is_empty() {
        return Err("noaa-scales.json carried no forecast days".into());
    }
    Ok(s)
}

/// The WSA-Enlil trace at Earth, downsampled, and the run time (`Last-Modified`) when served.
pub fn fetch_model() -> Result<(Vec<ModelPoint>, Option<i64>), String> {
    let resp = get(ENLIL_URL)?;
    let run_at = resp
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_http_date);
    let pts = parse_enlil(&resp.json::<Value>().map_err(|e| e.to_string())?);
    if pts.is_empty() {
        return Err("enlil_time_series.json carried no samples".into());
    }
    Ok((pts, run_at))
}

/// The last hour of L1 wind with NOAA's propagated arrival times.
pub fn fetch_l1() -> Result<Vec<L1Sample>, String> {
    let s = parse_propagated_wind(&get_json(L1_URL)?);
    if s.is_empty() {
        return Err("propagated-solar-wind-1-hour.json carried no samples".into());
    }
    Ok(s)
}

/// The forecasters' geomagnetic rationale from the 3-day forecast.
pub fn fetch_rationale() -> Result<Rationale, String> {
    let txt = get(THREE_DAY_URL)?.text().map_err(|e| e.to_string())?;
    parse_three_day_rationale(&txt).ok_or_else(|| "3-day-forecast.txt had no rationale".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Network test: `#[ignore]`d, run on demand with
    // `cargo test -p propagation --features live -- --ignored live::arrival`.

    /// Every product at its URL, in a shape that parses. Pins the wire, not the weather: a watch
    /// may or may not be in force.
    #[test]
    #[ignore = "network: services.swpc.noaa.gov"]
    fn live_products_arrive_and_parse() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let w = fetch_watch(now).expect("alerts.json");
        let s = fetch_scale_outlook().expect("noaa-scales.json");
        let (m, run) = fetch_model().expect("enlil_time_series.json");
        let l1 = fetch_l1().expect("propagated wind");
        let r = fetch_rationale().expect("3-day forecast");
        assert!(run.is_some(), "the model file lost its Last-Modified");
        assert!(
            now - l1.last().unwrap().at < 2 * 3600,
            "L1 wind is hours old"
        );
        println!(
            "watch {:?}; {} scale days; {} model points, run {:?}; {} L1 rows; rationale {:?}",
            w.map(|w| (w.serial, w.window)),
            s.days.len(),
            m.len(),
            run,
            l1.len(),
            r.text
        );
    }
}
