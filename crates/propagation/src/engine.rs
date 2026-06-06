//! Assembles the three pillars (opening detector + adaptive advisor + DXpedition
//! tracker) into one serializable [`PropagationSnapshot`] the UI renders, and a
//! deterministic [`demo`] scene so the Propagation section renders without a
//! live network feed.

use serde::Serialize;

use crate::advisor::{PropAdvisor, PropAdvisory};
use crate::dxped::{
    DxpedDashboard, DxpeditionPlan, DxpeditionTracker, Ft8DxpMode, NeedsSet, OperatorNeeds,
};
use crate::geo::compass_octant;
use crate::model::{Band, Confidence, PathSpot, PropMode, SpaceWx};
use crate::opening::{detect as detect_opening_signals, OpeningConfig};

/// A detected opening, projected for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningView {
    pub band: String,
    pub mode: String,
    pub octant: String,
    pub bearing_deg: f32,
    pub max_km: f32,
    /// Legacy 0..1 opening-strength score consumed by the map LUT (MapView).
    /// Currently equals `confidence_score`; kept distinct so the map render path
    /// is unaffected if the two are given separate meanings later.
    pub probability: f32,
    pub stations: u32,
    /// Categorical confidence word (derived from `confidence_score`).
    pub confidence: String,
    /// Numeric confidence in [0, 1] (the v2 detector's combined score).
    pub confidence_score: f32,
    /// Far stations confirmed two-way with the operator in the window.
    pub reciprocal_pairs: u32,
    /// Onset anomaly z-score (how far above the band's own baseline).
    pub anomaly_z: f32,
    /// Seconds since this opening's onset (0 until the stateful tracker stamps it
    /// in the command layer; the engine is rebuilt per call and can't persist it).
    pub onset_secs: i64,
    /// Just opened this poll (tracker-stamped; false at the engine layer).
    pub is_new: bool,
    /// Extra guidance, e.g. the 6m→2m escalator hint.
    pub note: String,
}

/// Space-weather, projected for the UI strip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceWxView {
    pub sfi: f32,
    pub kp: f32,
    pub a_index: f32,
    pub xray_class: String,
    pub flare: bool,
}

/// The whole propagation nowcast the UI section renders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PropagationSnapshot {
    pub advisory: PropAdvisory,
    pub openings: Vec<OpeningView>,
    pub dxpeditions: DxpedDashboard,
    pub space_wx: SpaceWxView,
    /// Provenance so the UI never silently shows stale/fake data:
    /// `"live"` (fresh fetch), `"cached"` (last-good after a failed refetch),
    /// or `"demo"` (the offline demo scene). Set by the caller.
    pub source: String,
    /// When this snapshot's data was produced (Unix seconds, UTC).
    pub as_of: i64,
}

/// Ties the three pillars to one operator identity.
pub struct PropagationEngine {
    me_call: String,
    me_grid: String,
    advisor: PropAdvisor,
    tracker: DxpeditionTracker,
}

impl PropagationEngine {
    pub fn new(me_call: &str, me_grid: &str) -> Self {
        Self {
            me_call: me_call.to_string(),
            me_grid: me_grid.to_string(),
            advisor: PropAdvisor::new(me_call, me_grid),
            tracker: DxpeditionTracker::new(me_grid),
        }
    }

    /// Build the full nowcast from the current inputs.
    pub fn snapshot(
        &self,
        now: i64,
        spots: &[PathSpot],
        wx: &SpaceWx,
        plans: &[DxpeditionPlan],
        needs: &dyn OperatorNeeds,
    ) -> PropagationSnapshot {
        let advisory = self.advisor.advise(now, spots, wx);
        let openings = self.detect_openings(now, spots, wx);
        let dxpeditions = self.tracker.dashboard(now, plans, needs, &advisory, wx);
        PropagationSnapshot {
            advisory,
            openings,
            dxpeditions,
            space_wx: SpaceWxView {
                sfi: wx.sfi,
                kp: wx.kp,
                a_index: wx.a_index,
                xray_class: format!("{}-class", wx.xray_class()),
                flare: wx.flare_in_progress(),
            },
            // Default provenance; live/cached callers override `source`.
            source: "live".to_string(),
            as_of: now,
        }
    }

    /// Detect openings with the v2 detector (anomaly/onset gate + rule-ordered
    /// Es/F2-TEP/Aurora/Tropo classifier) across the F2-prone HF + VHF bands, and
    /// project the open ones. `onset_secs`/`is_new` are stamped by the stateful
    /// `OpeningTracker` in the command layer (the engine is rebuilt per call and
    /// cannot persist tracker state), so they default to 0/false here.
    fn detect_openings(&self, now: i64, spots: &[PathSpot], wx: &SpaceWx) -> Vec<OpeningView> {
        const BANDS: [Band; 7] = [
            Band::B20,
            Band::B15,
            Band::B12,
            Band::B10,
            Band::B6,
            Band::B4,
            Band::B2,
        ];
        let cfg = OpeningConfig::default();
        let signals =
            detect_opening_signals(spots, &self.me_call, &self.me_grid, now, wx, &cfg, &BANDS);

        let mut out = Vec::new();
        for s in signals.into_iter().filter(|s| s.raw_open) {
            let f = &s.features;
            // Distinct far stations (union of the two directions; reciprocal ones
            // are counted on both sides, so subtract the overlap).
            let stations =
                (f.unique_far_rx + f.unique_far_tx).saturating_sub(f.reciprocal_pairs) as u32;
            let note = if s.mode == PropMode::Unknown {
                "Opening — mode uncertain".to_string()
            } else if s.band == Band::B6 && f.min_km > 0.0 && f.min_km < 1000.0 {
                "High-MUF Es — watch 4 m / 2 m next".to_string()
            } else {
                String::new()
            };

            out.push(OpeningView {
                band: s.band.label().to_string(),
                mode: s.mode.label().to_string(),
                octant: compass_octant(f.bearing_mean_deg).to_string(),
                bearing_deg: f.bearing_mean_deg as f32,
                max_km: f.max_km as f32,
                probability: s.confidence,
                stations,
                confidence: confidence_word(s.confidence).label().to_string(),
                confidence_score: s.confidence,
                reciprocal_pairs: f.reciprocal_pairs as u32,
                anomaly_z: f.anomaly_z,
                onset_secs: 0,
                is_new: false,
                note,
            });
        }
        out
    }
}

/// Categorical confidence word from the v2 numeric score.
fn confidence_word(score: f32) -> Confidence {
    if score >= 0.66 {
        Confidence::Strong
    } else if score >= 0.33 {
        Confidence::Likely
    } else {
        Confidence::Marginal
    }
}

/// A deterministic demo nowcast — a 6 m Es opening + a 20 m run to Europe + an
/// active needed DXpedition — so the Propagation UI renders without live feeds.
pub fn demo() -> PropagationSnapshot {
    // Fixed June-midday UTC timestamp (plausible Es; keeps time-of-day stable).
    const NOW: i64 = 1_718_886_000; // ~2024-06-20 13:00 UTC
    let me_call = "KD9TAW";
    let me_grid = "EN52";

    let mut spots: Vec<PathSpot> = Vec::new();
    let mk = |tx: &str, txg: &str, rx: &str, rxg: &str, band: Band, dt: i64| PathSpot {
        time: NOW - dt,
        tx_call: tx.to_string(),
        tx_grid: Some(txg.to_string()),
        rx_call: rx.to_string(),
        rx_grid: Some(rxg.to_string()),
        band,
        mode: Some("FT8".to_string()),
        snr: Some(-12.0),
    };

    // 6 m Sporadic-E burst: many stations both ways across ~1000–2000 km grids,
    // plus one short-skip path (escalator).
    let six = ["EM12", "FM18", "EL96", "DM79", "EN90", "FN42", "EN61"];
    for (i, g) in six.iter().cycle().take(16).enumerate() {
        spots.push(mk(
            me_call,
            me_grid,
            &format!("W{i}ES"),
            g,
            Band::B6,
            (i as i64) * 20,
        ));
        spots.push(mk(
            &format!("W{i}ES"),
            g,
            me_call,
            me_grid,
            Band::B6,
            (i as i64) * 20 + 7,
        ));
    }

    // 20 m run to Europe.
    let eu = ["JN58", "JO31", "IO91", "JN47", "JO62"];
    for (i, g) in eu.iter().cycle().take(14).enumerate() {
        spots.push(mk(
            me_call,
            me_grid,
            &format!("DL{i}EU"),
            g,
            Band::B20,
            (i as i64) * 25,
        ));
        if i < 6 {
            spots.push(mk(
                &format!("DL{i}EU"),
                g,
                me_call,
                me_grid,
                Band::B20,
                (i as i64) * 25 + 5,
            ));
        }
    }
    // A little 40 m.
    for i in 0..3 {
        spots.push(mk(
            me_call,
            me_grid,
            &format!("K{i}NA"),
            "FN31",
            Band::B40,
            (i as i64) * 40,
        ));
    }

    let wx = SpaceWx {
        sfi: 155.0, // high flux — the long-haul 20 m EU run classifies as F2
        kp: 3.0,
        a_index: 9.0,
        xray_long: 3e-7,
    };

    let plans = vec![
        DxpeditionPlan {
            call: "C91RU".to_string(),
            entity: "Mozambique".to_string(),
            grid: Some("KG43".to_string()),
            start_unix: NOW - 7200,
            end_unix: NOW + 7200,
            bands: vec![Band::B20, Band::B40],
            modes: vec!["CW".into(), "SSB".into(), "FT8".into()],
            ft8_mode: Some(Ft8DxpMode::FoxHound),
            most_wanted_rank: Some(38),
        },
        DxpeditionPlan {
            call: "VP8XYZ".to_string(),
            entity: "South Georgia".to_string(),
            grid: Some("GD18".to_string()),
            start_unix: NOW + 86_400 * 5,
            end_unix: NOW + 86_400 * 18,
            bands: vec![Band::B20, Band::B15, Band::B6],
            modes: vec!["CW".into(), "FT8".into()],
            ft8_mode: Some(Ft8DxpMode::SuperFox),
            most_wanted_rank: Some(7),
        },
    ];

    let mut needs = NeedsSet::default();
    needs.atno.insert("Mozambique".to_string());
    needs.atno.insert("South Georgia".to_string());

    let mut snap =
        PropagationEngine::new(me_call, me_grid).snapshot(NOW, &spots, &wx, &plans, &needs);
    snap.source = "demo".to_string();
    snap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_snapshot_is_rich() {
        let s = demo();
        // 6 m opening detected and classified Es, with the escalator note.
        let six = s
            .openings
            .iter()
            .find(|o| o.band == "6m")
            .expect("6m opening");
        assert_eq!(six.mode, "Sporadic-E");
        assert!(six.note.contains("watch"), "escalator note: {:?}", six.note);
        // No opening is left "mode uncertain" (the HF widening must classify, not
        // emit Unknowns); the 20 m long-haul run classifies as F2.
        assert!(
            s.openings.iter().all(|o| o.mode != "Unknown"),
            "no Unknown-mode openings: {:?}",
            s.openings
                .iter()
                .map(|o| (&o.band, &o.mode))
                .collect::<Vec<_>>()
        );
        assert!(
            s.openings.iter().any(|o| o.band == "20m" && o.mode == "F2"),
            "20m long-haul EU run should classify as F2"
        );
        // Headline loudly surfaces the 6 m opening.
        assert!(
            s.advisory.headline.contains("6M"),
            "headline: {}",
            s.advisory.headline
        );
        // 20 m is a ranked band.
        assert!(s.advisory.bands.iter().any(|b| b.band == "20m"));
        // The needed, active Mozambique DXpedition is a workable card.
        let card = s
            .dxpeditions
            .workable_now
            .iter()
            .find(|c| c.call == "C91RU")
            .expect("C91RU card");
        assert!(card.how_to_call.contains("Hound"));
        // South Georgia is upcoming (calendar), not active.
        assert!(s.dxpeditions.upcoming.iter().any(|c| c.call == "VP8XYZ"));
        // Provenance is stamped so the UI can flag non-live data.
        assert_eq!(s.source, "demo");
        assert_eq!(s.as_of, 1_718_886_000); // demo()'s fixed NOW
    }
}
