//! POTA / SOTA activator-spot fetch (the `live` feature). Thin blocking HTTP that
//! pulls the public spot feeds and hands the bytes to the pure
//! [`crate::pota`] parsers. No auth.

use std::time::Duration;

use crate::pota::{parse_pota_spots, parse_sota_spots, OtaSpot};

const UA: &str = "nexus-pota/0.1 (+ham radio parks/summits on the air)";
const POTA_SPOTS_URL: &str = "https://api.pota.app/spot/activator";

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())
}

fn get_text(c: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    c.get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// Fetch current POTA activator spots ("who's on the air now").
pub fn fetch_pota_spots() -> Result<Vec<OtaSpot>, String> {
    let c = client()?;
    Ok(parse_pota_spots(&get_text(&c, POTA_SPOTS_URL)?))
}

/// Fetch the most recent `count` SOTAwatch spots (clamped 1..=50).
pub fn fetch_sota_spots(count: u32) -> Result<Vec<OtaSpot>, String> {
    let c = client()?;
    let n = count.clamp(1, 50);
    let url = format!("https://api-db2.sota.org.uk/api/spots/{n}/all");
    Ok(parse_sota_spots(&get_text(&c, &url)?))
}

const POTA_PARK_URL: &str = "https://api.pota.app/park/";

/// One park's details from the live POTA directory — includes the lat/lon the local CSV index
/// doesn't carry, and backfills parks missing from a stale (or empty) local list.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveParkDetail {
    pub reference: String,
    pub name: String,
    pub grid: String,
    pub location: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(serde::Deserialize)]
struct ApiPark {
    #[serde(default)]
    reference: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    grid6: Option<String>,
    #[serde(default)]
    grid4: Option<String>,
    #[serde(default, rename = "locationDesc")]
    location: String,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
}

/// Fetch one park's details live from the POTA directory (name / grid / location + coordinates).
/// `reference` should be a normalized ref (e.g. `K-1234`). An unknown park or unparseable body
/// surfaces as `Err`. NOTE: the POTA API returns HTTP 200 with a JSON `null` body for an unknown
/// park (not a 404), so we deserialize as `Option` and treat null / empty as "not found".
pub fn fetch_park(reference: &str) -> Result<LiveParkDetail, String> {
    let c = client()?;
    let body = get_text(&c, &format!("{POTA_PARK_URL}{reference}"))?;
    let p = serde_json::from_str::<Option<ApiPark>>(&body)
        .map_err(|e| e.to_string())?
        .filter(|p| !(p.reference.is_empty() && p.name.is_empty()))
        .ok_or_else(|| format!("no park found for {reference}"))?;
    // Prefer the 6-char grid, fall back to the 4-char.
    let grid = p
        .grid6
        .filter(|s| !s.is_empty())
        .or(p.grid4)
        .unwrap_or_default();
    Ok(LiveParkDetail {
        reference: if p.reference.is_empty() {
            reference.to_string()
        } else {
            p.reference
        },
        name: p.name,
        grid,
        location: p.location,
        latitude: p.latitude,
        longitude: p.longitude,
    })
}
