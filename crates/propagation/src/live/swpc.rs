//! NOAA SWPC space-weather adapter (the `live` feature).
//!
//! Pulls the current solar flux (F10.7), planetary K-index, and GOES long-band
//! X-ray flux from the public SWPC JSON services (no auth) and normalizes them
//! into a [`SpaceWx`]. The A-index is estimated from Kp (instantaneous ap).

use std::time::Duration;

use serde_json::Value;

use crate::model::SpaceWx;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())
}

fn get_json(c: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .map_err(|e| e.to_string())
}

/// Fetch the current space-weather snapshot from NOAA SWPC.
pub fn fetch_space_wx() -> Result<SpaceWx, String> {
    let c = client()?;
    let sfi = fetch_sfi(&c)?;
    let kp = fetch_kp(&c)?;
    // X-ray is best-effort: a fade-out advisory, not load-bearing.
    let xray = fetch_xray(&c).unwrap_or(1e-7);
    Ok(SpaceWx {
        sfi,
        ssn: None, // R12 rides the separate solar-cycle feed (LAST_SSN)
        kp,
        a_index: kp_to_ap(kp),
        xray_long: xray,
    })
}

/// Latest 10.7 cm solar flux (`f107_cm_flux.json` is newest-first).
fn fetch_sfi(c: &reqwest::blocking::Client) -> Result<f32, String> {
    let v = get_json(c, "https://services.swpc.noaa.gov/json/f107_cm_flux.json")?;
    let arr = v.as_array().ok_or("sfi: not an array")?;
    let first = arr.first().ok_or("sfi: empty")?;
    let flux = first
        .get("flux")
        .and_then(|x| x.as_f64())
        .ok_or("sfi: no flux")?;
    Ok(flux as f32)
}

/// Latest planetary K-index (`planetary_k_index_1m.json` is newest-last).
fn fetch_kp(c: &reqwest::blocking::Client) -> Result<f32, String> {
    let v = get_json(
        c,
        "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json",
    )?;
    let arr = v.as_array().ok_or("kp: not an array")?;
    let last = arr.last().ok_or("kp: empty")?;
    let kp = last
        .get("estimated_kp")
        .or_else(|| last.get("kp_index"))
        .and_then(|x| x.as_f64())
        .ok_or("kp: no value")?;
    Ok(kp as f32)
}

/// Standalone GOES X-ray fetch — the 60 s "fast lane" behind `get_xray_now`, so
/// a flare's onset reaches the UI in ~1 min instead of the 5-min snapshot TTL.
pub fn fetch_xray_now() -> Result<f32, String> {
    fetch_xray(&client()?)
}

/// Latest GOES long-band (0.1–0.8 nm) X-ray flux (W/m²).
fn fetch_xray(c: &reqwest::blocking::Client) -> Result<f32, String> {
    let v = get_json(
        c,
        "https://services.swpc.noaa.gov/json/goes/primary/xrays-6-hour.json",
    )?;
    let arr = v.as_array().ok_or("xray: not an array")?;
    let long = arr
        .iter()
        .rev()
        .find(|e| e.get("energy").and_then(|x| x.as_str()) == Some("0.1-0.8nm"))
        .ok_or("xray: no long-band sample")?;
    let flux = long
        .get("flux")
        .and_then(|x| x.as_f64())
        .ok_or("xray: no flux")?;
    Ok(flux as f32)
}

/// Rough Kp → ap (instantaneous) for the displayed A-index estimate.
fn kp_to_ap(kp: f32) -> f32 {
    const AP: [f32; 10] = [0.0, 4.0, 7.0, 15.0, 27.0, 48.0, 80.0, 140.0, 240.0, 400.0];
    let k = kp.clamp(0.0, 9.0);
    let i = k.floor() as usize;
    if i >= 9 {
        return AP[9];
    }
    AP[i] + (AP[i + 1] - AP[i]) * (k - i as f32)
}

/// The NOAA planetary-K outlook (3-hourly, about a week back and three days
/// forward). See [`crate::kpforecast`] for what the samples mean; this is only the
/// wire.
pub fn fetch_kp_forecast() -> Result<crate::kpforecast::KpForecast, String> {
    let c = client()?;
    let body = c
        .get("https://services.swpc.noaa.gov/products/noaa-planetary-k-index-forecast.json")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    Ok(crate::kpforecast::parse_kp_forecast(&body))
}

/// NOAA's daily solar indices: the last thirty days of 10.7 cm flux and sunspot number. See
/// [`crate::solar_indices`] for what the rows mean; this is only the wire.
///
/// A body with no dated row is an ERROR, never an empty success: the command caches only what
/// this returns `Ok`, so an error page served with a 200 cannot replace thirty real days with
/// nothing, and the last good copy keeps being served with its own dates on it.
pub fn fetch_daily_solar_indices() -> Result<crate::solar_indices::DailySolarIndices, String> {
    let c = client()?;
    let body = c
        .get("https://services.swpc.noaa.gov/text/daily-solar-indices.txt")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    crate::solar_indices::parse_daily_solar_indices(&body)
        .ok_or_else(|| "daily solar indices: no dated row in the response".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Network test: `#[ignore]`d, run on demand with
    // `cargo test -p propagation --features live -- --ignored live::swpc`.

    /// The real file through the real client. The parser's offline tests pin the rows; this pins
    /// the wire: SWPC still serves the text product at this URL, in a shape that parses.
    #[test]
    #[ignore = "network: services.swpc.noaa.gov"]
    fn live_daily_solar_indices_arrive_and_parse() {
        let f = fetch_daily_solar_indices().expect("SWPC answered with a file that parses");
        assert!(
            f.days.len() >= 25,
            "about thirty days, got {}",
            f.days.len()
        );
        assert!(
            f.days.iter().any(|d| d.sfi.is_some() && d.ssn.is_some()),
            "no day carries both a flux and a sunspot number"
        );
        let newest = f.days.last().unwrap();
        println!(
            "SWPC daily indices: {} days, newest day_unix {} sfi {:?} ssn {:?}",
            f.days.len(),
            newest.day_unix,
            newest.sfi,
            newest.ssn
        );
    }
}
