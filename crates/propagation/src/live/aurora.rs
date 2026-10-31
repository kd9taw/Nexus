//! NOAA SWPC OVATION aurora-oval adapter (the `live` feature).
//!
//! `ovation_aurora_latest.json` carries a global grid in its `coordinates`
//! array, each entry `[longitude(0..359), latitude(-90..90), probability(0..100)]`.
//! We keep the oval (probability above a floor), downsample for a glanceable
//! overlay, and normalize longitude to −180..180 for the map.
//!
//! The oval is a FORECAST: OVATION runs on the solar wind measured at L1 and propagated to
//! Earth, so the file carries the time its inputs were observed (`Observation Time`) and the
//! time the oval is a forecast FOR (`Forecast Time`, typically 30–90 min later). Both ride with
//! the points ([`AuroraOval`]) so the map can say "expected at 04:04Z", never "now".

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

const OVATION_URL: &str = "https://services.swpc.noaa.gov/json/ovation_aurora_latest.json";
const UA: &str = "nexus-propagation/0.1 (+ham radio aurora overlay)";

/// One aurora-oval sample: where, and how likely an aurora is (0..100 %).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuroraPoint {
    pub lat: f32,
    pub lon: f32,
    pub prob: f32,
}

/// Parse OVATION json into a downsampled, thresholded oval. `min_prob` floors the
/// noise; `step` keeps every `step`-th integer degree (a light overlay). Pure, so
/// it's unit-testable without the network.
pub fn parse_aurora(v: &Value, min_prob: f32, step: i64) -> Vec<AuroraPoint> {
    let Some(coords) = v.get("coordinates").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    let step = step.max(1);
    let mut out = Vec::new();
    for c in coords {
        let Some(t) = c.as_array() else { continue };
        if t.len() < 3 {
            continue;
        }
        let lon = t[0].as_f64().unwrap_or(0.0);
        let lat = t[1].as_f64().unwrap_or(0.0);
        let prob = t[2].as_f64().unwrap_or(0.0) as f32;
        if prob < min_prob {
            continue;
        }
        // Downsample on the integer grid so the overlay stays light to render.
        if (lon.round() as i64) % step != 0 || (lat.round() as i64) % step != 0 {
            continue;
        }
        let lon = if lon > 180.0 { lon - 360.0 } else { lon };
        out.push(AuroraPoint {
            lat: lat as f32,
            lon: lon as f32,
            prob,
        });
    }
    out
}

/// The OVATION oval and the two times that say what it is: when its solar-wind input was
/// observed and the time it is a forecast for (Unix seconds, UTC; `None` if the file did not
/// carry a stamp it parses).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuroraOval {
    pub points: Vec<AuroraPoint>,
    pub observed_at: Option<i64>,
    pub forecast_at: Option<i64>,
    /// When Nexus fetched this copy (Unix seconds) — set by the command layer, so a copy
    /// served after a failed refetch says how old it is.
    #[serde(default)]
    pub fetched_at: Option<i64>,
}

/// The oval plus its `Observation Time` / `Forecast Time` stamps. Pure.
pub fn parse_oval(v: &Value, min_prob: f32, step: i64) -> AuroraOval {
    let stamp = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .and_then(crate::kc2g::parse_naive_utc_unix)
    };
    AuroraOval {
        points: parse_aurora(v, min_prob, step),
        observed_at: stamp("Observation Time"),
        forecast_at: stamp("Forecast Time"),
        fetched_at: None,
    }
}

/// Fetch + parse the current OVATION aurora oval (prob ≥ 8 %, every 2°).
pub fn fetch_aurora() -> Result<AuroraOval, String> {
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| e.to_string())?;
    let v = c
        .get(OVATION_URL)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .map_err(|e| e.to_string())?;
    Ok(parse_oval(&v, 8.0, 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn thresholds_downsamples_and_normalizes_longitude() {
        let v = json!({
            "coordinates": [
                [10.0, 66.0, 40.0],   // kept (prob≥8, lon/lat even)
                [11.0, 66.0, 40.0],   // dropped (lon odd, step 2)
                [200.0, 66.0, 30.0],  // kept, lon → -160
                [12.0, 66.0, 2.0],    // dropped (prob < 8)
                [10.0, 65.0, 40.0],   // dropped (lat odd, step 2)
            ]
        });
        let pts = parse_aurora(&v, 8.0, 2);
        assert_eq!(pts.len(), 2);
        assert!(pts
            .iter()
            .any(|p| (p.lon - 10.0).abs() < 0.1 && (p.lat - 66.0).abs() < 0.1));
        let west = pts.iter().find(|p| p.lon < 0.0).unwrap();
        assert!(
            (west.lon - (-160.0)).abs() < 0.1,
            "lon should normalize to -160, got {}",
            west.lon
        );
    }

    /// The live file's two stamps reach the oval: what it is a forecast FOR, and when its input
    /// was observed. A file without them carries `None`, never the fetch time.
    #[test]
    fn the_oval_carries_its_forecast_and_observation_times() {
        let v: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/swpc-ovation-aurora-2026-10-30.json"
        ))
        .unwrap();
        let oval = parse_oval(&v, 8.0, 1);
        let utc = |s| crate::kc2g::parse_naive_utc_unix(s).unwrap();
        assert_eq!(oval.forecast_at, Some(utc("2026-10-30T04:04:00Z")));
        assert_eq!(oval.observed_at, Some(utc("2026-10-30T02:39:00Z")));
        assert!(!oval.points.is_empty());
        let bare = parse_oval(&json!({"coordinates": []}), 8.0, 2);
        assert_eq!((bare.forecast_at, bare.observed_at), (None, None));
    }

    #[test]
    fn missing_coordinates_is_empty_not_an_error() {
        assert!(parse_aurora(&json!({}), 8.0, 2).is_empty());
    }
}
