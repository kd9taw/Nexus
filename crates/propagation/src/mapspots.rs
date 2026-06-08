//! Locate spots for the map. Turns the merged spot window (own-call PSKR + region
//! + DX-cluster/RBN + the operator's own decodes) into plottable points: each
//! station placed by its Maidenhead grid when known (precise), else by its DXCC
//! entity centroid (approximate) so the grid-less RBN/cluster firehose still fills
//! the map HamClock-style. Deduped per call (most-recent kept) and capped.

use std::collections::HashMap;

use serde::Serialize;

use crate::dxcc;
use crate::geo::maidenhead_to_latlon;
use crate::model::{PathSpot, Side};

/// One plottable spot for the map.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapSpot {
    pub call: String,
    pub lat: f64,
    pub lon: f64,
    pub band: String,
    /// This station heard ME (the "getting out" set) vs general band activity.
    pub heard_me: bool,
    pub age_secs: i64,
    /// Placed by DXCC centroid (true) rather than an exact grid (false).
    pub approx: bool,
}

/// Build the deduped, located, capped map-spot set from a spot window.
pub fn build_map_spots(now: i64, me_call: &str, spots: &[PathSpot], cap: usize) -> Vec<MapSpot> {
    // Best spot per callsign (freshest wins; a "heard me" or precise fix upgrades).
    let mut best: HashMap<String, MapSpot> = HashMap::new();

    for s in spots {
        // Which station do we plot, and did it hear me?
        let (subject, subject_grid, heard_me) = match s.side(me_call) {
            Side::HeardMe => (s.far_call(me_call), s.far_grid(me_call), true),
            Side::IHeard => (s.far_call(me_call), s.far_grid(me_call), false),
            // Far↔far (cluster/RBN): plot the spotted DX (the tx).
            Side::Neither => (Some(s.tx_call.as_str()), s.tx_grid.as_deref(), false),
        };
        let Some(call) = subject else { continue };
        let call = call.to_uppercase();
        if call == me_call.to_uppercase() {
            continue;
        }
        // Locate: exact grid first, else DXCC entity centroid.
        let (lat, lon, approx) = match subject_grid.and_then(maidenhead_to_latlon) {
            Some((la, lo)) => (la, lo, false),
            None => match dxcc::resolve(&call) {
                Some(info) => (info.lat, info.lon, true),
                None => continue, // can't place it
            },
        };
        let age = (now - s.time).max(0);
        let cand = MapSpot {
            call: call.clone(),
            lat,
            lon,
            band: s.band.label().to_string(),
            heard_me,
            age_secs: age,
            approx,
        };
        best
            .entry(call)
            .and_modify(|e| {
                // Prefer a precise fix, then "heard me", then the fresher spot.
                let upgrade = (!cand.approx && e.approx)
                    || (cand.approx == e.approx && cand.heard_me && !e.heard_me)
                    || (cand.approx == e.approx && cand.heard_me == e.heard_me && cand.age_secs < e.age_secs);
                if upgrade {
                    *e = cand.clone();
                }
            })
            .or_insert(cand);
    }

    let mut out: Vec<MapSpot> = best.into_values().collect();
    // Freshest first, then cap — a busy RBN window must not flood the canvas.
    out.sort_by(|a, b| a.age_secs.cmp(&b.age_secs));
    out.truncate(cap);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Band;

    const NOW: i64 = 1_700_000_000;

    fn spot(tx: &str, txg: Option<&str>, rx: &str, rxg: Option<&str>, dt: i64) -> PathSpot {
        PathSpot {
            time: NOW - dt,
            tx_call: tx.into(),
            tx_grid: txg.map(|g| g.into()),
            rx_call: rx.into(),
            rx_grid: rxg.map(|g| g.into()),
            band: Band::B20,
            mode: Some("FT8".into()),
            snr: Some(-12.0),
        }
    }

    #[test]
    fn places_by_grid_and_falls_back_to_dxcc_centroid() {
        let spots = vec![
            // I heard DL1ABC (gridded) — precise.
            spot("DL1ABC", Some("JN58"), "KD9TAW", Some("EN52"), 60),
            // Far↔far RBN: spotter heard JA1XYZ (no grid) — DXCC centroid (Japan).
            spot("JA1XYZ", None, "W1SKM", None, 30),
        ];
        let out = build_map_spots(NOW, "KD9TAW", &spots, 100);
        assert_eq!(out.len(), 2);
        let dl = out.iter().find(|m| m.call == "DL1ABC").unwrap();
        assert!(!dl.approx, "gridded → precise");
        let ja = out.iter().find(|m| m.call == "JA1XYZ").unwrap();
        assert!(ja.approx, "grid-less → DXCC centroid");
        assert!(ja.lon > 100.0, "JA centroid is in the Far East, got lon {}", ja.lon);
    }

    #[test]
    fn dedups_per_call_keeping_freshest_and_caps() {
        let spots = vec![
            spot("DL1ABC", Some("JN58"), "KD9TAW", Some("EN52"), 200),
            spot("DL1ABC", Some("JN58"), "KD9TAW", Some("EN52"), 20), // fresher
            spot("F5XYZ", Some("JN12"), "KD9TAW", Some("EN52"), 50),
        ];
        let out = build_map_spots(NOW, "KD9TAW", &spots, 1);
        assert_eq!(out.len(), 1, "capped to 1");
        assert_eq!(out[0].call, "DL1ABC", "freshest kept (20s DL beats 50s F5)");
        assert_eq!(out[0].age_secs, 20);
    }

    #[test]
    fn heard_me_flag_set_for_who_hears_me() {
        // KD9TAW transmitted; DL1ABC received → DL1ABC heard me.
        let spots = vec![spot("KD9TAW", Some("EN52"), "DL1ABC", Some("JN58"), 30)];
        let out = build_map_spots(NOW, "KD9TAW", &spots, 100);
        assert_eq!(out.len(), 1);
        assert!(out[0].heard_me);
        assert_eq!(out[0].call, "DL1ABC");
    }
}
