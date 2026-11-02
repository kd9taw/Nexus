//! NOAA OVATION's aurora-oval equatorward edge: where its aurora probability first reaches
//! [`EDGE_PROB`] %, coming in from the equator, per [`EDGE_BIN_DEG`]° of longitude, in each
//! hemisphere. Read from the whole 1° grid (before the overlay's thresholding and thinning), so
//! the line is the forecast's own. A longitude band where OVATION never reaches it has no edge
//! there (`None`): the line is broken, never bridged by a guess.
//!
//! There is deliberately no fallback. When OVATION is stale or missing the map draws no oval and
//! no edge at all (the operator's rule): Kp and NOAA's Kp outlook stay on screen with their own
//! labels, and no oval is ever modelled from them.

use serde::Serialize;
use serde_json::Value;

/// OVATION's probability (%) that marks the oval's equatorward edge.
pub const EDGE_PROB: f32 = 10.0;
/// The edge is one latitude per this many degrees of longitude.
pub const EDGE_BIN_DEG: f64 = 15.0;
const EDGE_BINS: usize = 24;

/// The centre longitude of edge bin `i` (bin 0 is −180…−165, so its centre is −172.5).
pub fn edge_bin_lon(i: usize) -> f64 {
    -180.0 + EDGE_BIN_DEG * (i as f64 + 0.5)
}

/// NOAA OVATION's equatorward edge: per longitude bin ([`edge_bin_lon`]), the latitude nearest
/// the equator where the probability reaches [`EDGE_PROB`] (south as a negative latitude),
/// `None` where it never does.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OvalEdge {
    pub north: Vec<Option<f32>>,
    pub south: Vec<Option<f32>>,
}

/// Read OVATION's equatorward edge from the whole `coordinates` grid
/// (`[lon 0..359, lat −90..90, probability]`). Pure.
pub fn ovation_edge(v: &Value) -> OvalEdge {
    let mut north: Vec<Option<f32>> = vec![None; EDGE_BINS];
    let mut south: Vec<Option<f32>> = vec![None; EDGE_BINS];
    let Some(coords) = v.get("coordinates").and_then(Value::as_array) else {
        return OvalEdge { north, south };
    };
    for c in coords {
        let Some(t) = c.as_array().filter(|t| t.len() >= 3) else {
            continue;
        };
        let (Some(lon), Some(lat), Some(p)) = (t[0].as_f64(), t[1].as_f64(), t[2].as_f64()) else {
            continue;
        };
        if (p as f32) < EDGE_PROB || lat == 0.0 {
            continue;
        }
        let lon = (lon + 540.0).rem_euclid(360.0) - 180.0;
        let bin = (((lon + 180.0) / EDGE_BIN_DEG).floor() as usize).min(EDGE_BINS - 1);
        let (side, lat) = if lat > 0.0 {
            (&mut north, lat as f32)
        } else {
            (&mut south, lat as f32)
        };
        if side[bin].is_none_or(|k| lat.abs() < k.abs()) {
            side[bin] = Some(lat);
        }
    }
    OvalEdge { north, south }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_ovation_edge_is_the_first_ten_percent_cell_from_the_equator() {
        let v = json!({"coordinates": [
            [10.0, 60.0, 9.0],   // below the edge probability
            [10.0, 62.0, 10.0],  // the edge in bin (−180+190)/15 = 12
            [11.0, 66.0, 40.0],  // same bin, further poleward
            [200.0, -55.0, 30.0], // lon −160 → bin 1, south
            [200.0, -70.0, 50.0],
            [100.0, 0.0, 90.0],  // the equator is neither hemisphere
        ]});
        let e = ovation_edge(&v);
        assert_eq!(e.north[12], Some(62.0));
        assert_eq!(e.south[1], Some(-55.0));
        assert_eq!(e.north.iter().flatten().count(), 1);
        assert_eq!(e.south.iter().flatten().count(), 1);
        assert_eq!(edge_bin_lon(12), 7.5);
        assert!(ovation_edge(&json!({})).north.iter().all(Option::is_none));
    }
}
