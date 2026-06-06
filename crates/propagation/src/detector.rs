//! Band-opening detector — a faithful Rust port of weak-signal-sleuth's proven
//! two-tier logic (the author's working 6 m opening detector):
//!
//! 1. **Heuristic score** (`src/App.tsx`):
//!    `score = unique_stations/min_stations + reciprocal_paths/min_reciprocal
//!             + max_grid_km/geo_spread_km`,  open when `score >= open_threshold`.
//!    Defaults: 5 / 2 / 200 km / 1.0 over a 30-minute window.
//! 2. **Logistic-regression ML model** (`src/ml.ts` + tuned `ml_weights.json`):
//!    `P(open) = sigmoid(Σ wᵢ·featureᵢ + b)` over 7 features. Weights/bias are the
//!    author's tuned values, carried over verbatim.
//!
//! Reciprocal paths = unique `callsign-grid` pairs on FT-family modes; rarity_sum
//! sums weak-signal-sleuth's grid-rarity table over recent spots; time-of-day is
//! the same sin/cos UTC encoding. The numbers are unchanged so the port behaves
//! like the original.

use std::collections::HashSet;

use serde::Serialize;

use crate::geo::{grid_distance_km, time_of_day_sin_cos};
use crate::rarity::rarity_of;
use crate::spot::Spot;

/// Logistic-regression weights, ported verbatim from
/// `weak-signal-sleuth/public/ml_weights.json`.
#[derive(Debug, Clone, Serialize)]
pub struct Weights {
    /// Coefficients for [spot_count, unique_calls, reciprocal_paths,
    /// geo_spread_km, rarity_sum, time_of_day_sin, time_of_day_cos].
    pub w: [f32; 7],
    pub b: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            w: [0.018, 0.045, 0.06, 0.004, 0.006, 0.2, 0.1],
            b: -2.2,
        }
    }
}

/// Heuristic thresholds (ported defaults from weak-signal-sleuth's UI).
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Lookback window (seconds). weak-signal-sleuth uses 1800 (30 min).
    pub window_secs: i64,
    pub min_stations: f32,
    pub min_reciprocal: f32,
    pub geo_spread_km: f32,
    pub open_threshold: f32,
    pub weights: Weights,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            window_secs: 1800,
            min_stations: 5.0,
            min_reciprocal: 2.0,
            geo_spread_km: 200.0,
            open_threshold: 1.0,
            weights: Weights::default(),
        }
    }
}

/// The 7 ML features (ported from `ml.ts` `Features`).
#[derive(Debug, Clone, Serialize)]
pub struct Features {
    pub spot_count: f32,
    pub unique_calls: f32,
    pub reciprocal_paths: f32,
    pub geo_spread_km: f32,
    pub rarity_sum: f32,
    pub time_of_day_sin: f32,
    pub time_of_day_cos: f32,
}

impl Features {
    fn vector(&self) -> [f32; 7] {
        [
            self.spot_count,
            self.unique_calls,
            self.reciprocal_paths,
            self.geo_spread_km,
            self.rarity_sum,
            self.time_of_day_sin,
            self.time_of_day_cos,
        ]
    }
}

/// The detector's verdict for one band/window.
#[derive(Debug, Clone, Serialize)]
pub struct OpeningStatus {
    /// Heuristic verdict (`score >= open_threshold`).
    pub open: bool,
    /// Heuristic score.
    pub score: f32,
    /// ML model P(open) in [0, 1].
    pub probability: f32,
    /// Widest grid-pair path seen in the window (km) — the heuristic geo term.
    pub max_path_km: f32,
    /// The feature vector the ML model scored.
    pub features: Features,
}

/// Logistic sigmoid (ported from `ml.ts`).
pub fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

/// The opening detector. Stateless apart from its config; call [`detect`] each
/// time the spot window updates.
///
/// [`detect`]: OpeningDetector::detect
#[derive(Default)]
pub struct OpeningDetector {
    pub config: DetectorConfig,
}

impl OpeningDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: DetectorConfig) -> Self {
        Self { config }
    }

    /// ML probability for an explicit feature vector (ported `predictProb`).
    pub fn predict_prob(&self, f: &Features) -> f32 {
        let x = f.vector();
        let w = &self.config.weights;
        let z = x
            .iter()
            .zip(w.w.iter())
            .map(|(xi, wi)| xi * wi)
            .sum::<f32>()
            + w.b;
        sigmoid(z).clamp(0.0, 1.0)
    }

    /// Evaluate the window of `spots` ending at `now` (Unix seconds).
    pub fn detect(&self, spots: &[Spot], now: i64) -> OpeningStatus {
        let cutoff = now - self.config.window_secs;
        let recent: Vec<&Spot> = spots.iter().filter(|s| s.time >= cutoff).collect();

        let stations: HashSet<&str> = recent.iter().map(|s| s.callsign.as_str()).collect();
        let reciprocal: HashSet<String> = recent
            .iter()
            .filter(|s| s.is_ft_mode())
            .map(|s| format!("{}-{}", s.callsign, s.grid.clone().unwrap_or_default()))
            .collect();

        // Heuristic geo term: widest path over the last 25 UNIQUE grids.
        let heur_grids = last_unique_grids(&recent, 25);
        let heur_max_km = max_pairwise_km(&heur_grids) as f32;

        // ML geo + rarity: over the last 60 spots that carry a grid.
        let ml_grids = last_grids(&recent, 60);
        let rarity_sum: f32 = ml_grids.iter().map(|g| rarity_of(g)).sum();
        let ml_max_km = max_pairwise_km(&ml_grids) as f32;

        let (tod_sin, tod_cos) = time_of_day_sin_cos(now);

        let features = Features {
            spot_count: recent.len() as f32,
            unique_calls: stations.len() as f32,
            reciprocal_paths: reciprocal.len() as f32,
            geo_spread_km: ml_max_km,
            rarity_sum,
            time_of_day_sin: tod_sin,
            time_of_day_cos: tod_cos,
        };

        let score = stations.len() as f32 / self.config.min_stations
            + reciprocal.len() as f32 / self.config.min_reciprocal
            + heur_max_km / self.config.geo_spread_km;

        OpeningStatus {
            open: score >= self.config.open_threshold,
            score,
            probability: self.predict_prob(&features),
            max_path_km: heur_max_km,
            features,
        }
    }
}

/// Unique grids (uppercased) in first-seen order, keeping the last `n`.
fn last_unique_grids(recent: &[&Spot], n: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut v = Vec::new();
    for s in recent {
        if let Some(g) = &s.grid {
            let u = g.to_uppercase();
            if seen.insert(u.clone()) {
                v.push(u);
            }
        }
    }
    if v.len() > n {
        v.split_off(v.len() - n)
    } else {
        v
    }
}

/// Grids (uppercased) of the last `n` spots that carry one (not deduped).
fn last_grids(recent: &[&Spot], n: usize) -> Vec<String> {
    let mut v: Vec<String> = recent
        .iter()
        .filter_map(|s| s.grid.as_ref().map(|g| g.to_uppercase()))
        .collect();
    if v.len() > n {
        v.split_off(v.len() - n)
    } else {
        v
    }
}

/// Widest great-circle distance (km) between any two of the given grids.
fn max_pairwise_km(grids: &[String]) -> f64 {
    let mut max = 0.0;
    for i in 0..grids.len() {
        for j in (i + 1)..grids.len() {
            if let Some(km) = grid_distance_km(&grids[i], &grids[j]) {
                if km > max {
                    max = km;
                }
            }
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000;

    /// A quiet band (a couple of co-located, non-FT spots — no reciprocal
    /// FT activity, no geographic spread) is NOT open and scores a low
    /// probability (≈ sigmoid(b)). NB: the ported heuristic is deliberately
    /// sensitive — even 2 FT spots clear `reciprocal/min_reciprocal = 1.0` — so
    /// "quiet" here means non-FT, which is the original detector's behavior.
    #[test]
    fn quiet_band_is_closed() {
        let spots = vec![
            Spot::new(NOW - 100, "W1AAA", "FN42", "CW"),
            Spot::new(NOW - 80, "W1BBB", "FN42", "CW"),
        ];
        let d = OpeningDetector::new();
        let st = d.detect(&spots, NOW);
        assert!(
            !st.open,
            "quiet band should not be flagged open: score={}",
            st.score
        );
        assert!(
            st.probability < 0.3,
            "quiet prob should be low: {}",
            st.probability
        );
    }

    /// A wide, busy FT8 burst across distant grids (classic 6 m Es signature) IS
    /// open and scores a high probability.
    #[test]
    fn wide_busy_burst_is_open() {
        let grids = [
            "EN52", "FN42", "EM12", "DM79", "CN87", "EL96", "FM18", "DN70", "CM87", "EN90",
        ];
        let mut spots = Vec::new();
        for (i, g) in grids.iter().enumerate() {
            // Two distinct callsigns per grid → many stations + reciprocal paths.
            spots.push(Spot::new(
                NOW - (i as i64) * 30,
                &format!("K{i}AAA"),
                g,
                "FT8",
            ));
            spots.push(Spot::new(
                NOW - (i as i64) * 30 - 5,
                &format!("K{i}BBB"),
                g,
                "FT8",
            ));
        }
        let d = OpeningDetector::new();
        let st = d.detect(&spots, NOW);
        assert!(
            st.open,
            "wide busy burst should be open: score={}",
            st.score
        );
        assert!(
            st.probability > 0.9,
            "burst prob should be high: {}",
            st.probability
        );
        assert!(
            st.max_path_km > 1000.0,
            "should see a wide path: {}",
            st.max_path_km
        );
        assert_eq!(st.features.unique_calls, 20.0);
    }

    /// Spots older than the window are ignored.
    #[test]
    fn stale_spots_excluded() {
        let spots = vec![
            Spot::new(NOW - 5000, "W1AAA", "FN42", "FT8"), // > 30 min old
            Spot::new(NOW - 4000, "W1BBB", "EN52", "FT8"),
        ];
        let st = OpeningDetector::new().detect(&spots, NOW);
        assert_eq!(st.features.spot_count, 0.0);
        assert!(!st.open);
    }

    /// The ML model matches the ported reference: all-zero features → sigmoid(b).
    #[test]
    fn ml_baseline_matches_reference() {
        let d = OpeningDetector::new();
        let f = Features {
            spot_count: 0.0,
            unique_calls: 0.0,
            reciprocal_paths: 0.0,
            geo_spread_km: 0.0,
            rarity_sum: 0.0,
            time_of_day_sin: 0.0,
            time_of_day_cos: 0.0,
        };
        // sigmoid(-2.2) ≈ 0.0998
        assert!((d.predict_prob(&f) - 0.0998).abs() < 0.002);
    }
}
