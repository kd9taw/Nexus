//! The swappable per-path prediction seam.
//!
//! Per the locked architecture (hybrid): the operator's observed-reception engine
//! ([`crate::advisor`]) leads for "what's open now"; this layer answers the
//! per-path / future-hour / no-coverage question — "is THIS path to THAT station
//! workable, on which band, when" — that observation can't, because you have no
//! spots on a path you haven't worked.
//!
//! The engine is a commodity behind [`PathPredictor`]; the value is the
//! zero-parameter auto-config around it. The default/offline impl is
//! [`HeuristicEngine`] over the physics-lite [`crate::likelihood::PathModel`]
//! (median-conditions MUF/absorption/greyline/aurora — honest *relative*
//! workability, not absolute REL). A vendored VOACAP engine (voacapl) and ITU-R
//! P.533 slot in behind the SAME trait later, with no change to callers or UI.

use serde::Serialize;

use crate::likelihood::{BandOutlook, PathModel};
use crate::model::{Band, SpaceWx};

/// A per-path prediction: per-HF-band outlook for one operator↔DX great circle,
/// best-band first, tagged with the engine that produced it (so the UI can badge
/// "modelled" vs a future "VOACAP" and the user can trust accordingly).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathPrediction {
    /// Engine identity: `"heuristic"` today; `"voacap"` / `"p533"` later.
    pub engine: String,
    /// Per-HF-band outlook (workability word + peak score + best window + hourly),
    /// sorted best-first. VHF is excluded — it routes to the opening detector.
    pub bands: Vec<BandOutlook>,
}

/// A per-path HF predictor. Implementors are interchangeable; the fusion/UI layer
/// depends on the trait, never a concrete engine, so the offline path always
/// degrades to [`HeuristicEngine`] and VOACAP is a drop-in upgrade.
pub trait PathPredictor: Send + Sync {
    /// Stable engine id (matches [`PathPrediction::engine`]).
    fn name(&self) -> &'static str;

    /// Predict the path to `dx` (lat, lon) over the 24 h from `from_unix` under
    /// space weather `wx`. For "now", pass the current time.
    fn predict(&self, dx: (f64, f64), from_unix: i64, wx: &SpaceWx) -> PathPrediction;
}

/// Default offline engine — the physics-lite [`PathModel`]. Always available, no
/// network, no data files; the floor the hybrid degrades to.
pub struct HeuristicEngine {
    model: PathModel,
}

impl HeuristicEngine {
    /// Anchor at the operator's location (lat, lon); `None` ⇒ predictions are empty.
    pub fn new(me_latlon: Option<(f64, f64)>) -> Self {
        Self {
            model: PathModel::new(me_latlon),
        }
    }
}

impl PathPredictor for HeuristicEngine {
    fn name(&self) -> &'static str {
        "heuristic"
    }

    fn predict(&self, dx: (f64, f64), from_unix: i64, wx: &SpaceWx) -> PathPrediction {
        let mut bands: Vec<BandOutlook> = Band::ALL
            .iter()
            .filter(|b| !b.is_vhf()) // VHF (Es/aurora) is the opening detector's job
            .map(|&b| self.model.outlook_24h(dx, b, from_unix, wx))
            .collect();
        bands.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        PathPrediction {
            engine: "heuristic".to_string(),
            bands,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::maidenhead_to_latlon;

    const MIDNIGHT_UTC: i64 = 1_718_886_000 - 13 * 3600; // ~2024-06-20 00:00 UTC

    #[test]
    fn predicts_per_hf_band_best_first_excluding_vhf() {
        let me = maidenhead_to_latlon("EN52");
        let eng = HeuristicEngine::new(me);
        let dx = maidenhead_to_latlon("JN58").unwrap(); // EN52 → Munich
        let wx = SpaceWx {
            sfi: 150.0,
            kp: 1.0,
            ..Default::default()
        };
        let pred = eng.predict(dx, MIDNIGHT_UTC, &wx);
        assert_eq!(pred.engine, "heuristic");
        assert_eq!(eng.name(), "heuristic");
        // HF bands only — no 6m/4m/2m in the per-path outlook.
        assert!(pred.bands.iter().all(|b| !matches!(b.band.as_str(), "6m" | "4m" | "2m")));
        assert!(!pred.bands.is_empty());
        // Sorted best-first.
        for w in pred.bands.windows(2) {
            assert!(w[0].score >= w[1].score, "bands must be sorted best-first");
        }
        // A sunlit mid-latitude path at SFI 150 should find at least one workable
        // band over the day.
        assert!(
            pred.bands.iter().any(|b| b.score >= 0.3),
            "expected a workable band, got {:?}",
            pred.bands.iter().map(|b| (&b.band, b.score)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_operator_location_yields_empty_outlooks() {
        let eng = HeuristicEngine::new(None);
        let dx = maidenhead_to_latlon("JN58").unwrap();
        let pred = eng.predict(dx, MIDNIGHT_UTC, &SpaceWx::default());
        // Every band scores 0 with no anchor (PathModel returns 0), so none are workable.
        assert!(pred.bands.iter().all(|b| b.score == 0.0));
    }
}
