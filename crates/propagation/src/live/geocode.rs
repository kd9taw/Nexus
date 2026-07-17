//! City-name → coordinates via OSM Nominatim (the `live` feature) — the
//! "repeaters near Gatlinburg, TN" convenience path in the Program section.
//! Fetch + parse live together here (the `contests.rs` precedent — one small
//! feed, one module). Grid entry stays the primary, offline-capable origin;
//! this is only the friendly override.
//!
//! Nominatim usage policy: identifying User-Agent, ≤1 req/s. Every call here is
//! a single user-initiated Search click (no keystroke queries — the UI gates
//! that), so we sit far inside the policy. Results require the attribution
//! "Geocoding © OpenStreetMap contributors" wherever they're shown.

use std::time::Duration;

const UA: &str = "Nexus (radio programming; https://hamradiotools.io; kd9taw@protonmail.com)";

/// One geocoding candidate the operator picks from.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoCandidate {
    /// Human-readable place ("Gatlinburg, Sevier County, Tennessee, USA").
    pub display_name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Search a free-text place name; returns up to 5 candidates (empty = no match).
pub fn search_city(query: &str) -> Result<Vec<GeoCandidate>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .https_only(true)
        .build()
        .map_err(|_| "Geocoding: HTTP client initialization failed".to_string())?;
    let url = reqwest::Url::parse_with_params(
        "https://nominatim.openstreetmap.org/search",
        &[("q", q), ("format", "jsonv2"), ("limit", "5")],
    )
    .map_err(|_| "Geocoding: bad query".to_string())?;
    let resp = c.get(url).send().map_err(redact)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!(
            "Geocoding: server returned HTTP {}",
            status.as_u16()
        ));
    }
    let body = resp
        .text()
        .map_err(|_| "Geocoding: could not read the response body".to_string())?;
    Ok(parse_nominatim(&body))
}

/// Parse a Nominatim `format=jsonv2` array (lat/lon arrive as strings).
pub fn parse_nominatim(json: &str) -> Vec<GeoCandidate> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    rows.iter()
        .filter_map(|v| {
            let lat: f64 = v.get("lat")?.as_str()?.parse().ok()?;
            let lon: f64 = v.get("lon")?.as_str()?.parse().ok()?;
            let display_name = v.get("display_name")?.as_str()?.to_string();
            Some(GeoCandidate {
                display_name,
                lat,
                lon,
            })
        })
        .collect()
}

fn redact(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "Geocoding: request timed out — try again shortly".to_string()
    } else if e.is_connect() {
        "Geocoding: could not connect — check your network".to_string()
    } else {
        "Geocoding: request failed".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nominatim_jsonv2() {
        let json = r#"[
          {"lat":"35.7143", "lon":"-83.5102", "display_name":"Gatlinburg, Sevier County, Tennessee, USA"},
          {"lat":"bad", "lon":"-83.5", "display_name":"junk row skipped"}
        ]"#;
        let out = parse_nominatim(json);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].display_name,
            "Gatlinburg, Sevier County, Tennessee, USA"
        );
        assert!((out[0].lat - 35.7143).abs() < 1e-9);
        assert!(parse_nominatim("not json").is_empty());
    }
}
