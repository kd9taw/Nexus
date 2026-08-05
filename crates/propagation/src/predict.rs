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

use crate::likelihood::{BandOutlook, PathModel, Workability};
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
    /// Controlling MUF (MHz) on this path RIGHT NOW — the band ceiling (bands below
    /// it are open). 0 when the operator location is unknown.
    pub muf_now: f32,
    /// Per-UTC-hour MUF (24 values, hour 0..23) for the day containing the scan —
    /// the ceiling line drawn above the band×hour outlook heatmap.
    pub muf_hourly: Vec<f32>,
}

impl PathPrediction {
    /// Re-rank this prediction for a **DX-chase** headline — see [`rank_for_chase`].
    ///
    /// Opt-in, and deliberately NOT applied inside [`PathPredictor::predict`]: the same
    /// prediction feeds "which bands are open right now" surfaces (`get_path_outlook`,
    /// [`band_outlook_ring`]) where 60 m is a perfectly good answer and demoting it would
    /// be a lie. Only a caller that knows it is answering "what should I CHASE" may apply it.
    pub fn demote_award_ineligible(&mut self) {
        rank_for_chase(&mut self.bands);
    }
}

/// Order `bands` (already sorted best-first) for a **DX-chase** recommendation: give the
/// headline slot to the best band that can actually earn ARRL DXCC credit, and leave every
/// other band exactly where its score put it.
///
/// The operator's point (2026-08-05), and it is an award fact, not a propagation one: 60 m
/// earns no DXCC credit of any kind (see [`crate::awards::earns_dxcc_credit`] — one rule,
/// one home), and most DXpeditions do not even run it (secondary, channelised, and a
/// single-channel receiver rules out the split that runs a pileup). Headlining it to someone
/// chasing DXCC recommends a band the award cannot count.
///
/// **The rule is only "it may not be the HEADLINE"**, which is the whole of the complaint —
/// not "it ranks last". An earlier draft sank it below every workable creditable band; that
/// is worse, because callers `truncate(4)` this array, so on a night with four open bands the
/// demotion would delete 60 m from the card altogether. Displacing exactly one position keeps
/// it adjacent to the headline, with its true score and its real window, for the operator who
/// wants that contact for its own sake — some operations really do announce 60 m (3C2MD ran
/// 5.357), and `calendar_outlook`'s list IS the announced one.
///
/// "Workable" is ≥ [`Workability::Fair`] (0.30) — the same line the outlook window and the
/// chase alarm already use. If nothing creditable clears it, the order is left alone and 60 m
/// leads honestly: it really is the best shot, and saying so beats naming a closed band.
///
/// A band whose label does not parse cannot be shown to earn credit, so it never takes the
/// headline either — that slot should not go to a band we failed to identify.
///
/// Idempotent (after one pass the headline is already creditable, so the rotate is a no-op).
pub fn rank_for_chase(bands: &mut [BandOutlook]) {
    const USABLE: f32 = 0.30;
    let creditable_and_workable = |b: &BandOutlook| {
        b.score >= USABLE && Band::from_label(&b.band).is_some_and(crate::awards::earns_dxcc_credit)
    };
    if let Some(i) = bands.iter().position(creditable_and_workable) {
        // Move that band to the front; everything it passes keeps its relative order.
        bands[..=i].rotate_right(1);
    }
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
        // The path's MUF ceiling now + per-UTC-hour (anchored to UTC midnight like
        // the band hourly arrays, so the heatmap x-axis and the ceiling line align).
        let muf_now = self.model.muf(dx, from_unix, wx) as f32;
        let day0 = from_unix - from_unix.rem_euclid(86_400);
        let muf_hourly: Vec<f32> = (0..24)
            .map(|h| self.model.muf(dx, day0 + h * 3600, wx) as f32)
            .collect();
        PathPrediction {
            engine: "heuristic".to_string(),
            bands,
            muf_now,
            muf_hourly,
        }
    }
}

/// Build the configured path-prediction engine by name. `"p533"` returns the
/// validated ITU-R P.533/P.372 engine (its TX power taken from the station
/// power setting when present, plus `ant_gain_dbi` — the combined TX+RX
/// antenna gain, 0 = isotropic — as a link-budget adder); any other name —
/// including the default `"heuristic"` — returns the always-available
/// physics-lite fallback (which ignores gain), so a stale or unknown setting
/// can never break predictions.
pub fn make_predictor(
    name: &str,
    me_latlon: Option<(f64, f64)>,
    station_power_w: Option<f64>,
    ant_gain_dbi: f64,
) -> Box<dyn PathPredictor> {
    match name {
        "p533" => {
            let mut cfg = station_power_w
                .map(crate::p533::engine::P533Config::with_power_watts)
                .unwrap_or_default();
            cfg.ant_gain_dbi = ant_gain_dbi;
            Box::new(crate::p533::engine::P533Engine::with_config(me_latlon, cfg))
        }
        _ => Box::new(HeuristicEngine::new(me_latlon)),
    }
}

/// Instantaneous, model-only band openness "right now" (NOT a 24 h peak): for each HF
/// band, the BEST [`PathModel::score`] to any of `n_dirs` azimuths at `dist_km` — i.e.
/// "is this band open to DX in SOME direction now", derivable with ZERO observed spots.
/// The advisor uses this as the physics prior for sparse bands so an open-but-unheard
/// band reads "open, no spots heard" rather than dead. `muf_now` is the ring-max
/// controlling MUF (MHz), reused by the trend/insight layer.
pub struct ModeledNow {
    pub bands: std::collections::HashMap<Band, (Workability, f32)>,
    pub muf_now: f32,
}

/// Compute [`ModeledNow`] for the operator at `me` (lat, lon). Reuses
/// [`PathModel::score`]/[`PathModel::muf`] + [`crate::geo::destination_point`]. HF only
/// (VHF is Es/aurora-driven — the advisor handles it separately). `n_dirs` clamps to 1..=36.
pub fn modeled_now(
    me: (f64, f64),
    dist_km: f64,
    n_dirs: usize,
    now: i64,
    wx: &SpaceWx,
) -> ModeledNow {
    use std::collections::HashMap;
    let model = PathModel::new(Some(me));
    let n = n_dirs.clamp(1, 36);
    let dirs: Vec<(f64, f64)> = (0..n)
        .map(|i| crate::geo::destination_point(me, (i as f64) * 360.0 / (n as f64), dist_km))
        .collect();
    let mut bands: HashMap<Band, (Workability, f32)> = HashMap::new();
    for &b in Band::ALL.iter().filter(|b| !b.is_vhf()) {
        let best = dirs
            .iter()
            .map(|&dx| model.score(dx, b, now, wx))
            .fold(0.0f32, f32::max);
        bands.insert(b, (Workability::from_score(best), best));
    }
    let muf_now = dirs
        .iter()
        .map(|&dx| model.muf(dx, now, wx) as f32)
        .fold(0.0f32, f32::max);
    ModeledNow { bands, muf_now }
}

/// The ring-max controlling MUF (MHz) for the operator right now — the single MUF
/// number tracked for the "MUF building/falling" trend. 8 azimuths at ~9000 km.
pub fn representative_muf(me: (f64, f64), now: i64, wx: &SpaceWx) -> f32 {
    modeled_now(me, 9000.0, 8, now, wx).muf_now
}

/// Aggregate a modeled per-band outlook over a RING of representative long-haul DX
/// directions — the no-selection "general band outlook". Each band reports its BEST
/// modeled workability to ANY of `n_dirs` evenly-spaced azimuths at `dist_km`, plus
/// the ring's max MUF (now + per-hour). Direction-agnostic: answers "which bands are
/// modeled-workable to DX right now" without picking one arbitrary path. Runs on the
/// CALLER'S engine (heuristic stays cheap-and-sync; p533 is compute-heavy — cache it
/// and keep it off the async core); `n_dirs` is clamped to 1..=36.
pub fn band_outlook_ring(
    eng: &dyn PathPredictor,
    me: (f64, f64),
    dist_km: f64,
    n_dirs: usize,
    now: i64,
    wx: &SpaceWx,
) -> PathPrediction {
    use std::collections::HashMap;
    let n = n_dirs.clamp(1, 36);
    let mut best: HashMap<String, BandOutlook> = HashMap::new();
    let mut muf_now = 0f32;
    let mut muf_hourly = vec![0f32; 24];
    for i in 0..n {
        let brg = (i as f64) * 360.0 / (n as f64);
        let dx = crate::geo::destination_point(me, brg, dist_km);
        let p = eng.predict(dx, now, wx);
        muf_now = muf_now.max(p.muf_now);
        for (h, slot) in muf_hourly.iter_mut().enumerate() {
            if let Some(&m) = p.muf_hourly.get(h) {
                *slot = slot.max(m);
            }
        }
        for bo in p.bands {
            best.entry(bo.band.clone())
                .and_modify(|e| {
                    if bo.score > e.score {
                        *e = bo.clone();
                    }
                })
                .or_insert(bo);
        }
    }
    let mut bands: Vec<BandOutlook> = best.into_values().collect();
    bands.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    PathPrediction {
        engine: eng.name().to_string(),
        bands,
        muf_now,
        muf_hourly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::maidenhead_to_latlon;

    const MIDNIGHT_UTC: i64 = 1_718_886_000 - 13 * 3600; // ~2024-06-20 00:00 UTC

    /// A real scene, not a synthetic one: the operator's own QTH to C6 (Bahamas), one of
    /// the operations on the board he reported. At SFI 70 / Kp 0 the physics genuinely
    /// puts 60 m on top (0.800 vs 80 m 0.720 and 40 m 0.711) even WITH the `band_ceiling`
    /// arm — so this exercises the award rule, not the scoring fix.
    const C6_BAHAMAS: (f64, f64) = (25.03, -77.40);
    const FIELD_T0: i64 = 1_785_888_000; // 2026-08-05 00:00 UTC

    fn quiet_low_flux() -> SpaceWx {
        SpaceWx {
            sfi: 70.0,
            kp: 0.0,
            ..Default::default()
        }
    }

    fn labels(p: &PathPrediction) -> Vec<String> {
        p.bands.iter().map(|b| b.band.clone()).collect()
    }

    /// FIELD REPORT 2026-08-05, the AWARD half: "for true chasing, 60 m is not an eligible
    /// band for DXCC, so many DXpeditions don't even run it." Correct, and stronger than
    /// stated — ARRL excludes 60 m from the award programme outright.
    ///
    /// NON-VACUITY — delete the `rotate_right` in [`rank_for_chase`] and this goes red:
    /// measured 2026-08-05, EN52 → C6 at SFI 70 / Kp 0 the raw engine order is
    /// `60m 0.800, 80m 0.720, 40m 0.711, …`, so the card headlines a band that can never
    /// earn DXCC credit.
    #[test]
    fn chase_headline_is_never_a_band_the_award_cannot_credit() {
        let me = maidenhead_to_latlon("EN52");
        let eng = HeuristicEngine::new(me);
        let wx = quiet_low_flux();
        let mut p = eng.predict(C6_BAHAMAS, FIELD_T0, &wx);

        // Precondition: this scene really is one where 60 m leads on physics alone.
        assert_eq!(
            p.bands[0].band,
            "60m",
            "scene no longer exercises the rule; raw order was {:?}",
            labels(&p)
        );
        let sixty_before = p.bands.iter().find(|b| b.band == "60m").unwrap().clone();

        p.demote_award_ineligible();

        assert_ne!(p.bands[0].band, "60m", "60 m still headlines a chase card");
        assert!(
            p.bands[0].score >= 0.30,
            "the headline must be a workable band, got {} {}",
            p.bands[0].band,
            p.bands[0].score
        );

        // Demoted, NOT deleted, NOT rescored — and still adjacent to the headline so a
        // `truncate(4)` on the card cannot drop it.
        let sixty_at = p.bands.iter().position(|b| b.band == "60m");
        assert_eq!(sixty_at, Some(1), "60 m must stay next to the headline");
        let sixty_after = &p.bands[1];
        assert_eq!(
            sixty_after.score, sixty_before.score,
            "score must be untouched"
        );
        assert_eq!(
            sixty_after.window, sixty_before.window,
            "window must survive"
        );
        assert_eq!(p.bands.len(), labels(&p).len(), "no band may be dropped");
    }

    /// The demotion displaces ONE position and preserves every other band's score order —
    /// it is not a re-sort. (The naive version WAS a re-sort, pushing 60 m below even
    /// CLOSED bands; with the callers' `truncate(4)` that deleted it from the card, which
    /// is the one thing the rule must never do.)
    #[test]
    fn demotion_preserves_the_rest_of_the_score_order() {
        let eng = HeuristicEngine::new(maidenhead_to_latlon("EN52"));
        let wx = quiet_low_flux();
        let mut p = eng.predict(C6_BAHAMAS, FIELD_T0, &wx);
        let without_sixty: Vec<String> = labels(&p).into_iter().filter(|b| b != "60m").collect();
        p.demote_award_ineligible();
        let after: Vec<String> = labels(&p).into_iter().filter(|b| b != "60m").collect();
        assert_eq!(
            without_sixty, after,
            "only 60 m may move; the rest keep their score order"
        );
    }

    /// If nothing creditable is workable, 60 m leads honestly — recommending a closed band
    /// instead would be a worse answer, not a more compliant one.
    #[test]
    fn ineligible_band_still_leads_when_nothing_creditable_is_open() {
        let mut bands = vec![
            mk("60m", 0.62),
            mk("80m", 0.11),
            mk("40m", 0.05),
            mk("20m", 0.00),
        ];
        rank_for_chase(&mut bands);
        assert_eq!(bands[0].band, "60m");
        // ...and it yields the moment something creditable clears the 0.30 line.
        bands[1].score = 0.31;
        rank_for_chase(&mut bands);
        assert_eq!(bands[0].band, "80m");
        assert_eq!(bands[1].band, "60m");
    }

    #[test]
    fn rank_for_chase_is_idempotent_and_total() {
        let mut once = vec![mk("60m", 0.80), mk("80m", 0.72), mk("40m", 0.71)];
        rank_for_chase(&mut once);
        let after_one = once.clone();
        rank_for_chase(&mut once);
        assert_eq!(
            labels_of(&once),
            labels_of(&after_one),
            "second pass must be a no-op"
        );
        // An empty list and an all-ineligible list must both be safe.
        rank_for_chase(&mut []);
        let mut only_sixty = vec![mk("60m", 0.9)];
        rank_for_chase(&mut only_sixty);
        assert_eq!(only_sixty[0].band, "60m");
        // An unparseable label never takes the headline.
        let mut junk = vec![mk("60m", 0.9), mk("not-a-band", 0.95), mk("40m", 0.5)];
        rank_for_chase(&mut junk);
        assert_eq!(junk[0].band, "40m");
    }

    /// ⭐ THE BLAST-RADIUS GUARD. The demotion is opt-in and must NOT leak into
    /// [`PathPredictor::predict`] itself, because the same prediction feeds the
    /// "which bands are open right now" surfaces (`get_path_outlook`,
    /// [`band_outlook_ring`]) — there 60 m is a legitimate answer and hiding it
    /// would be a lie. `predict()` stays pure score order.
    #[test]
    fn predict_itself_is_not_award_filtered() {
        let eng = HeuristicEngine::new(maidenhead_to_latlon("EN52"));
        let p = eng.predict(C6_BAHAMAS, FIELD_T0, &quiet_low_flux());
        assert_eq!(
            p.bands[0].band, "60m",
            "predict() must rank on physics alone, not award eligibility"
        );
        for w in p.bands.windows(2) {
            assert!(w[0].score >= w[1].score, "predict() stays score-sorted");
        }
        // The ring aggregator is built on predict() and must keep 60 m too.
        let me = maidenhead_to_latlon("EN52").unwrap();
        let ring = band_outlook_ring(
            &HeuristicEngine::new(Some(me)),
            me,
            9000.0,
            8,
            FIELD_T0,
            &quiet_low_flux(),
        );
        assert!(
            ring.bands.iter().any(|b| b.band == "60m"),
            "the general band ladder must still offer 60 m"
        );
    }

    fn labels_of(v: &[BandOutlook]) -> Vec<String> {
        v.iter().map(|b| b.band.clone()).collect()
    }

    fn mk(band: &str, score: f32) -> BandOutlook {
        BandOutlook {
            band: band.to_string(),
            workability: Workability::from_score(score).label().to_string(),
            score,
            window: "0000–0100Z".to_string(),
            grayline: false,
            hourly: vec![score; 24],
            reliability: 0.0,
            mode_now: Vec::new(),
            mode_hourly: Vec::new(),
        }
    }

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
        assert!(pred
            .bands
            .iter()
            .all(|b| !matches!(b.band.as_str(), "6m" | "4m" | "2m")));
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
            pred.bands
                .iter()
                .map(|b| (&b.band, b.score))
                .collect::<Vec<_>>()
        );
        // MUF ceiling: 24 hourly values, and a real positive ceiling at SFI 150.
        assert_eq!(pred.muf_hourly.len(), 24, "24 hourly MUF values");
        assert!(
            pred.muf_hourly.iter().any(|&m| m > 0.0),
            "a sunlit day has a positive MUF somewhere"
        );
        // MUF rises with solar flux: SFI 200 must give a higher midday ceiling than SFI 90.
        let midday = MIDNIGHT_UTC + 12 * 3600;
        let me_ll = me.unwrap();
        let model_hi = crate::likelihood::PathModel::new(Some(me_ll));
        let lo = model_hi.muf(
            dx,
            midday,
            &SpaceWx {
                sfi: 90.0,
                ..Default::default()
            },
        );
        let hi = model_hi.muf(
            dx,
            midday,
            &SpaceWx {
                sfi: 200.0,
                ..Default::default()
            },
        );
        assert!(hi > lo, "MUF must rise with SFI: hi({hi}) > lo({lo})");
    }

    #[test]
    fn no_operator_location_yields_empty_outlooks() {
        let eng = HeuristicEngine::new(None);
        let dx = maidenhead_to_latlon("JN58").unwrap();
        let pred = eng.predict(dx, MIDNIGHT_UTC, &SpaceWx::default());
        // Every band scores 0 with no anchor (PathModel returns 0), so none are workable.
        assert!(pred.bands.iter().all(|b| b.score == 0.0));
    }

    #[test]
    fn modeled_now_is_hf_only_with_positive_muf() {
        let me = maidenhead_to_latlon("EN52").unwrap();
        let wx = SpaceWx {
            sfi: 150.0,
            kp: 1.0,
            ..Default::default()
        };
        let midday = MIDNIGHT_UTC + 18 * 3600;
        let m = modeled_now(me, 9000.0, 8, midday, &wx);
        // HF only — the opening detector owns VHF.
        assert!(m.bands.keys().all(|b| !b.is_vhf()));
        assert!(m.bands.contains_key(&Band::B20));
        assert!(m.muf_now > 0.0, "ring MUF positive at SFI 150 midday");
        // Best-over-ring must be ≥ any single direction (it's a max).
        let one = crate::likelihood::PathModel::new(Some(me)).score(
            crate::geo::destination_point(me, 90.0, 9000.0),
            Band::B20,
            midday,
            &wx,
        );
        assert!(m.bands[&Band::B20].1 >= one - 1e-6);
        // representative_muf is the ring-max MUF.
        assert!(representative_muf(me, midday, &wx) > 0.0);
    }

    #[test]
    fn band_outlook_ring_aggregates_best_per_band_over_directions() {
        let me = maidenhead_to_latlon("EN52").unwrap();
        let wx = SpaceWx {
            sfi: 150.0,
            kp: 1.0,
            ..Default::default()
        };
        // Midday: at least one direction is sunlit, so the ring finds workable HF.
        let midday = MIDNIGHT_UTC + 18 * 3600;
        let ring = band_outlook_ring(&HeuristicEngine::new(Some(me)), me, 9000.0, 8, midday, &wx);
        assert!(!ring.bands.is_empty());
        // HF only, sorted best-first, 24 MUF hours, positive ceiling somewhere.
        assert!(ring
            .bands
            .iter()
            .all(|b| !matches!(b.band.as_str(), "6m" | "4m" | "2m")));
        for w in ring.bands.windows(2) {
            assert!(w[0].score >= w[1].score, "ring bands sorted best-first");
        }
        assert_eq!(ring.muf_hourly.len(), 24);
        assert!(
            ring.muf_now > 0.0,
            "ring MUF ceiling positive at SFI 150 midday"
        );
        // The ring's best-per-band must be >= any single direction's (it's a max).
        let one = HeuristicEngine::new(Some(me)).predict(
            crate::geo::destination_point(me, 90.0, 9000.0),
            midday,
            &wx,
        );
        for ob in &one.bands {
            if let Some(rb) = ring.bands.iter().find(|b| b.band == ob.band) {
                assert!(
                    rb.score >= ob.score - 1e-6,
                    "ring score >= single-direction for {}",
                    ob.band
                );
            }
        }
    }
}
