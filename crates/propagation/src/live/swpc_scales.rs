//! NOAA SWPC scales + alerts fetchers (the `live` feature).
//!
//! Networked half: pulls the two public SWPC products and hands each to the
//! pure parsers in [`crate::swpc_scales`].

use std::time::Duration;

use serde_json::Value;

use crate::swpc_scales::{parse_alerts, parse_noaa_scales, AlertView, NoaaScalesView};

const UA: &str = "nexus-propagation/0.1 (+ham radio space weather)";
const SCALES_URL: &str = "https://services.swpc.noaa.gov/products/noaa-scales.json";
const ALERTS_URL: &str = "https://services.swpc.noaa.gov/products/alerts.json";

/// Keep at most this many recent bulletins from `alerts.json`.
const ALERTS_MAX: usize = 20;

/// The most either product is read to: on 2026-11-02 the alerts were 66 KB and the scales 1 KB.
const REPLY_BYTES: u64 = 1024 * 1024;

/// The client both fetches use: HTTPS only, on [`builder`]'s timeout and redirect policy.
fn client() -> Result<reqwest::blocking::Client, String> {
    builder()
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())
}

/// A timeout, and no redirect followed (SWPC serves each product at its own address, so a
/// redirect is a fault, not a move). The loopback tests build on this (plain HTTP); the fetches
/// themselves always go through [`client`].
fn builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
}

fn get_text(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let resp = c.get(url).send().map_err(|e| e.to_string())?;
    super::body::read_ok(resp, REPLY_BYTES, "NOAA SWPC")
}

fn get_json(c: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    serde_json::from_str(&get_text(c, url)?).map_err(|e| e.to_string())
}

/// Fetch + parse the current NOAA R/S/G scales (today + tomorrow's G forecast).
pub fn fetch_noaa_scales() -> Result<NoaaScalesView, String> {
    let c = client()?;
    let v = get_json(&c, SCALES_URL)?;
    Ok(parse_noaa_scales(&v))
}

/// Fetch + parse the most recent SWPC alert/watch/warning bulletins.
pub fn fetch_alerts() -> Result<Vec<AlertView>, String> {
    let c = client()?;
    let v = get_json(&c, ALERTS_URL)?;
    Ok(parse_alerts(&v, ALERTS_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both products are read through `get_text` on `client`: HTTPS only, no redirect followed,
    /// at most `REPLY_BYTES`.
    #[test]
    fn the_wire_keeps_its_limits() {
        super::super::body::assert_wire_limits(builder, client, get_text, REPLY_BYTES, "NOAA SWPC");
    }
}
