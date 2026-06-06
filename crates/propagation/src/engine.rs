//! Assembles the three pillars (opening detector + adaptive advisor + DXpedition
//! tracker) into one serializable [`PropagationSnapshot`] the UI renders, and a
//! deterministic [`demo`] scene so the Propagation section renders without a
//! live network feed.

use serde::Serialize;

use crate::advisor::{PropAdvisor, PropAdvisory};
use crate::detector::OpeningDetector;
use crate::dxped::{
    DxpedDashboard, DxpeditionPlan, DxpeditionTracker, Ft8DxpMode, NeedsSet, OperatorNeeds,
};
use crate::geo::{bearing_deg, compass_octant, grid_distance_km, maidenhead_to_latlon};
use crate::model::{classify_vhf_mode, Band, Confidence, PathSpot, SpaceWx};
use crate::spot::Spot;

/// A detected VHF opening, projected for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningView {
    pub band: String,
    pub mode: String,
    pub octant: String,
    pub bearing_deg: f32,
    pub max_km: f32,
    pub probability: f32,
    pub stations: u32,
    pub confidence: String,
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
    detector: OpeningDetector,
    advisor: PropAdvisor,
    tracker: DxpeditionTracker,
}

impl PropagationEngine {
    pub fn new(me_call: &str, me_grid: &str) -> Self {
        Self {
            me_call: me_call.to_string(),
            me_grid: me_grid.to_string(),
            detector: OpeningDetector::new(),
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

    /// Run the ported detector per VHF band and classify any opening's mode.
    fn detect_openings(&self, now: i64, spots: &[PathSpot], wx: &SpaceWx) -> Vec<OpeningView> {
        let mut out = Vec::new();
        for band in [Band::B6, Band::B4, Band::B2] {
            // Far ends (relative to me) of this band's spots → the detector's
            // single-ended Spot population (matches weak-signal-sleuth's input).
            let mut det_spots = Vec::new();
            let mut far_grids: Vec<String> = Vec::new();
            let mut heard_me = false;
            let mut i_heard = false;
            for s in spots.iter().filter(|s| s.band == band) {
                if let (Some(call), Some(grid)) =
                    (s.far_call(&self.me_call), s.far_grid(&self.me_call))
                {
                    det_spots.push(Spot::new(
                        s.time,
                        call,
                        grid,
                        s.mode.as_deref().unwrap_or(""),
                    ));
                    far_grids.push(grid.to_string());
                    match s.side(&self.me_call) {
                        crate::model::Side::HeardMe => heard_me = true,
                        crate::model::Side::IHeard => i_heard = true,
                        _ => {}
                    }
                }
            }
            let st = self.detector.detect(&det_spots, now);
            if !st.open {
                continue;
            }

            // Path geometry from the operator to the far ends.
            let mut dists: Vec<f64> = far_grids
                .iter()
                .filter_map(|g| grid_distance_km(&self.me_grid, g))
                .collect();
            dists.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let median = dists.get(dists.len() / 2).copied().unwrap_or(0.0);
            let max = dists.last().copied().unwrap_or(0.0);
            let min = dists.first().copied().unwrap_or(0.0);

            let mode = classify_vhf_mode(median, max, wx);
            let bearing = self.mean_bearing(&far_grids);
            let stations = st.features.unique_calls as u32;
            let confidence = Confidence::from_evidence(stations as usize, heard_me && i_heard);

            // 6m→2m escalator: short-skip 6m (high foEs) ⇒ watch the higher bands.
            let note = if band == Band::B6 && min > 0.0 && min < 1000.0 {
                "High-MUF Es — watch 4 m / 2 m next".to_string()
            } else {
                String::new()
            };

            out.push(OpeningView {
                band: band.label().to_string(),
                mode: mode.label().to_string(),
                octant: compass_octant(bearing).to_string(),
                bearing_deg: bearing as f32,
                max_km: max as f32,
                probability: st.probability,
                stations,
                confidence: confidence.label().to_string(),
                note,
            });
        }
        out
    }

    fn mean_bearing(&self, far_grids: &[String]) -> f64 {
        let Some(me) = maidenhead_to_latlon(&self.me_grid) else {
            return 0.0;
        };
        // Vector mean of bearings (handles wrap).
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        let mut n = 0u32;
        for g in far_grids {
            if let Some(dx) = maidenhead_to_latlon(g) {
                let b = bearing_deg(me, dx).to_radians();
                sx += b.cos();
                sy += b.sin();
                n += 1;
            }
        }
        if n == 0 {
            0.0
        } else {
            (sy.atan2(sx).to_degrees() + 360.0) % 360.0
        }
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
        sfi: 142.0,
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
