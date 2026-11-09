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
    /// Model-free path geometry (polar cap, auroral oval, grey line, Kp per hour) for a
    /// per-path request. The engines never fill it — the caller attaches it with the live
    /// space weather it holds ([`crate::path_geometry`]); absent, the key is omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<crate::pathgeo::PathGeometry>,
    /// The same request predicted along the long path (the other way round the great
    /// circle), with its own geometry. Like `geometry`, attached by the per-path caller and
    /// only when the engine can model the long route (P.533); absent, the key is omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_path: Option<Box<PathPrediction>>,
    /// Per band, which route the model favours at the request's hour and why
    /// ([`crate::route::compare_routes`]). Present only beside `long_path`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routes: Option<Vec<crate::route::RouteCall>>,
    /// The request's day as a band × UTC-hour grid of both routes, with the route the model
    /// favours each hour and the hour's Kp ([`crate::pathday::path_day`]) — what the
    /// path-over-the-day strip draws. Attached by the per-path caller; absent, the key is omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<crate::pathday::PathDay>,
    /// What a flare or a proton event does to this route NOW, band by band
    /// ([`crate::pathnow::path_overlay`]): display only, laid over the model's current hour and
    /// never folded into its numbers. Attached by the per-path caller after the card is kept for
    /// contact grading, so it never travels with a contact; absent, the key is omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<crate::pathnow::PathHourOverlay>,
    /// The "now" line beside the climatology ([`crate::pathnow::space_now`]): a flare, a proton
    /// event, a NOAA watch. Attached by the outlook callers; absent when nothing is happening.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_now: Option<crate::pathnow::SpaceNow>,
    /// The configured engine could not run, so this is the heuristic's answer
    /// ([`with_fallback`]). The screen says so; a cache never keeps it. Absent when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub engine_fallback: bool,
    /// The storm correction laid over this P.533 answer ([`crate::storm`]); absent when it changed
    /// nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storm: Option<crate::storm::StormNote>,
    /// The transmit power the answer assumes and where it came from, set by the caller that
    /// chose it. Only P.533 uses power; absent from the engines themselves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<AssumedPower>,
}

/// The transmit power a prediction assumes, and where that figure came from, for the screen to
/// say. `amplifier` is true when an amplifier is set up on the radio, so a figure that is not the
/// amplifier's own may be below the power on the air.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssumedPower {
    pub watts: f64,
    pub source: PowerSource,
    pub amplifier: bool,
}

/// Where an [`AssumedPower`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerSource {
    /// Settings ▸ Station power.
    Setting,
    /// The radio's forward power on the last transmission Nexus measured.
    Rig,
    /// The amplifier's output on the last transmission Nexus measured.
    Amplifier,
    /// Nothing set and nothing measured: 100 W.
    Default,
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

    /// [`Self::predict`] along a chosen route. The default models the short path only and
    /// returns `None` for the long one, so an engine with no long-path method says so instead
    /// of answering with the short path's numbers.
    fn predict_route(
        &self,
        dx: (f64, f64),
        route: crate::route::Route,
        from_unix: i64,
        wx: &SpaceWx,
    ) -> Option<PathPrediction> {
        match route {
            crate::route::Route::Short => Some(self.predict(dx, from_unix, wx)),
            crate::route::Route::Long => None,
        }
    }

    /// The storm correction this engine has laid over any answer since it was built
    /// ([`crate::storm::with_storm_correction`]); none for an engine that makes none.
    fn storm_note(&self) -> Option<crate::storm::StormNote> {
        None
    }

    /// Whether this engine could not run and answered from the heuristic ([`with_fallback`]),
    /// at any time since it was built. An engine that cannot fail never has.
    fn fell_back(&self) -> bool {
        false
    }
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
            geometry: None,
            long_path: None,
            routes: None,
            day: None,
            overlay: None,
            space_now: None,
            engine_fallback: false,
            storm: None,
            power: None,
        }
    }
}

/// Build the configured path-prediction engine by name. `"p533"`, the default, returns the
/// validated ITU-R P.533/P.372 engine (its TX power taken from the station
/// power setting when present, plus `ant_gain_dbi` — the combined TX+RX
/// antenna gain, 0 = isotropic — as a link-budget adder), which falls back to the heuristic if
/// it cannot run ([`with_fallback`]); any other name — including `"heuristic"` — returns the
/// always-available physics-lite engine (which ignores gain), so a stale or unknown setting
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
            with_fallback(
                crate::p533::engine::P533Engine::with_config(me_latlon, cfg),
                me_latlon,
            )
        }
        _ => Box::new(HeuristicEngine::new(me_latlon)),
    }
}

/// `primary`, answering from the heuristic if it panics, so a screen shows a labelled answer and
/// never a blank. P.533 reads no file and no network (its tables are compiled in), so a panic is
/// the one way it can fail to run. It says `primary`'s [`PathPredictor::name`] throughout, so a
/// caller still knows which engine was asked for; each answer it replaces carries
/// [`PathPrediction::engine_fallback`] and names the heuristic, and
/// [`PathPredictor::fell_back`] stays true from then on, for a caller deciding whether to keep
/// what it just computed. The app's panic hook logs the panic itself.
pub fn with_fallback<P: PathPredictor + 'static>(
    primary: P,
    me_latlon: Option<(f64, f64)>,
) -> Box<dyn PathPredictor> {
    Box::new(WithFallback {
        primary,
        floor: HeuristicEngine::new(me_latlon),
        failed: std::sync::atomic::AtomicBool::new(false),
    })
}

struct WithFallback<P> {
    primary: P,
    floor: HeuristicEngine,
    failed: std::sync::atomic::AtomicBool,
}

impl<P: PathPredictor> WithFallback<P> {
    /// `run` on the primary engine, or `None` if it panicked (noted).
    fn attempt<T>(&self, run: impl FnOnce(&P) -> T) -> Option<T> {
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&self.primary)));
        match ran {
            Ok(answer) => Some(answer),
            Err(_) => {
                self.failed
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                None
            }
        }
    }

    fn floor_answer(&self, dx: (f64, f64), from_unix: i64, wx: &SpaceWx) -> PathPrediction {
        let mut p = self.floor.predict(dx, from_unix, wx);
        p.engine_fallback = true;
        p
    }
}

impl<P: PathPredictor> PathPredictor for WithFallback<P> {
    fn name(&self) -> &'static str {
        self.primary.name()
    }

    fn predict(&self, dx: (f64, f64), from_unix: i64, wx: &SpaceWx) -> PathPrediction {
        self.attempt(|p| p.predict(dx, from_unix, wx))
            .unwrap_or_else(|| self.floor_answer(dx, from_unix, wx))
    }

    fn predict_route(
        &self,
        dx: (f64, f64),
        route: crate::route::Route,
        from_unix: i64,
        wx: &SpaceWx,
    ) -> Option<PathPrediction> {
        match self.attempt(|p| p.predict_route(dx, route, from_unix, wx)) {
            Some(answer) => answer,
            // The heuristic has no long path, and says so rather than borrowing the short one.
            None => {
                (route == crate::route::Route::Short).then(|| self.floor_answer(dx, from_unix, wx))
            }
        }
    }

    fn fell_back(&self) -> bool {
        self.failed.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// The grade words, lowest first, as [`Workability`] labels them.
const WORDS: [&str; 5] = ["Closed", "Marginal", "Fair", "Good", "Excellent"];

fn word_rank(label: &str) -> u8 {
    WORDS
        .iter()
        .position(|w| *w == label)
        .map_or(0, |i| i as u8)
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
    let mut engine_fallback = false;
    let mut storm = None;
    // Each band's word toward each direction, P.533's only (see below).
    let mut words: HashMap<String, Vec<u8>> = HashMap::new();
    let mut all_p533 = true;
    for i in 0..n {
        let brg = (i as f64) * 360.0 / (n as f64);
        let dx = crate::geo::destination_point(me, brg, dist_km);
        let p = eng.predict(dx, now, wx);
        engine_fallback |= p.engine_fallback;
        crate::storm::StormNote::fold(&mut storm, p.storm);
        all_p533 &= p.engine == "p533";
        muf_now = muf_now.max(p.muf_now);
        for (h, slot) in muf_hourly.iter_mut().enumerate() {
            if let Some(&m) = p.muf_hourly.get(h) {
                *slot = slot.max(m);
            }
        }
        for mut bo in p.bands {
            // The ring's bands answer "which band, any direction": one azimuth's hop
            // geometry would be read as THE path's, so it is not carried.
            bo.detail = None;
            words
                .entry(bo.band.clone())
                .or_default()
                .push(word_rank(&bo.workability));
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
    // P.533's word for the ring is the one a band reaches toward at least half the directions.
    // Its best direction alone reads Good or Excellent on nearly every band at almost any power,
    // since some direction carries CW at some hour of the day: the all-alike outlook the grade
    // exists to avoid. The hours, the heat map and the timing stay the best direction's. The
    // heuristic keeps its own words, as before.
    if all_p533 {
        for bo in &mut bands {
            if let Some(v) = words.get_mut(&bo.band) {
                v.sort_unstable_by(|a, b| b.cmp(a));
                let k = v.len().div_ceil(2);
                bo.workability = WORDS[usize::from(v[k - 1])].to_string();
            }
        }
    }
    bands.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    PathPrediction {
        engine: if engine_fallback {
            "heuristic"
        } else {
            eng.name()
        }
        .to_string(),
        bands,
        muf_now,
        muf_hourly,
        geometry: None,
        long_path: None,
        routes: None,
        day: None,
        overlay: None,
        space_now: None,
        engine_fallback,
        storm,
        power: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::maidenhead_to_latlon;

    /// An engine that cannot run, on every route.
    struct Broken;
    impl PathPredictor for Broken {
        fn name(&self) -> &'static str {
            "p533"
        }
        fn predict(&self, _: (f64, f64), _: i64, _: &SpaceWx) -> PathPrediction {
            panic!("broken on purpose")
        }
        fn predict_route(
            &self,
            _: (f64, f64),
            _: crate::route::Route,
            _: i64,
            _: &SpaceWx,
        ) -> Option<PathPrediction> {
            panic!("broken on purpose")
        }
    }

    const ME: (f64, f64) = (42.36, -71.06);
    const DX: (f64, f64) = (51.5, -0.13);
    const FALLBACK_T: i64 = 1_794_009_600;

    #[test]
    fn an_engine_that_cannot_run_answers_from_the_heuristic_and_says_so() {
        let wx = SpaceWx::default();
        let eng = with_fallback(Broken, Some(ME));
        assert!(!eng.fell_back(), "nothing has failed yet");
        let p = eng.predict(DX, FALLBACK_T, &wx);
        assert!(p.engine_fallback);
        assert_eq!(p.engine, "heuristic");
        assert!(!p.bands.is_empty(), "an answer, not a blank");
        assert!(eng.fell_back());
        assert_eq!(eng.name(), "p533", "still names the engine asked for");
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"engineFallback\":true"), "{json}");
    }

    #[test]
    fn a_route_on_an_engine_that_cannot_run_keeps_the_short_path_and_has_no_long_one() {
        let wx = SpaceWx::default();
        let eng = with_fallback(Broken, Some(ME));
        let short = eng.predict_route(DX, crate::route::Route::Short, FALLBACK_T, &wx);
        assert!(short.is_some_and(|p| p.engine_fallback));
        assert!(eng
            .predict_route(DX, crate::route::Route::Long, FALLBACK_T, &wx)
            .is_none());
    }

    #[test]
    fn a_ring_on_an_engine_that_cannot_run_names_the_heuristic_and_says_so() {
        let wx = SpaceWx::default();
        let eng = with_fallback(Broken, Some(ME));
        let ring = band_outlook_ring(eng.as_ref(), ME, 9000.0, 8, FALLBACK_T, &wx);
        assert!(ring.engine_fallback);
        assert_eq!(ring.engine, "heuristic");
        assert!(!ring.bands.is_empty());
    }

    /// P.533's ring reads the word a band reaches toward at least half the directions, so it is
    /// not one word for every band; its hours stay the best direction's.
    #[test]
    fn a_p533_ring_is_graded_toward_half_the_directions() {
        let wx = SpaceWx::default();
        let me = (42.1, -88.0);
        let t = 1_794_009_600 + 21 * 3600;
        let eng = make_predictor("p533", Some(me), Some(100.0), 0.0);
        let ring = band_outlook_ring(eng.as_ref(), me, 9000.0, 8, t, &wx);
        let words: std::collections::HashSet<_> =
            ring.bands.iter().map(|b| b.workability.as_str()).collect();
        assert!(words.len() >= 3, "a spread of words: {words:?}");
        let most = words
            .iter()
            .map(|w| ring.bands.iter().filter(|b| b.workability == *w).count())
            .max()
            .unwrap();
        assert!(
            most <= 6,
            "no one word for most bands: {most} of {}",
            ring.bands.len()
        );
        assert!(
            !words.contains("Excellent"),
            "no band is Excellent toward half the world at 100 W"
        );
        let best = |band: &str| {
            (0..8)
                .map(|k| {
                    let dx = crate::geo::destination_point(me, f64::from(k) * 45.0, 9000.0);
                    let p = eng.predict(dx, t, &wx);
                    p.bands.iter().find(|b| b.band == band).unwrap().score
                })
                .fold(0.0f32, f32::max)
        };
        let b20 = ring.bands.iter().find(|b| b.band == "20m").unwrap();
        assert!(
            (b20.score - best("20m")).abs() < 1e-6,
            "the hours are the best direction's"
        );
    }

    /// The heuristic's ring keeps the heuristic's own words.
    #[test]
    fn the_heuristic_ring_keeps_its_own_words() {
        let wx = SpaceWx::default();
        let me = (42.1, -88.0);
        let t = 1_794_009_600 + 21 * 3600;
        let ring = band_outlook_ring(&HeuristicEngine::new(Some(me)), me, 9000.0, 8, t, &wx);
        for b in &ring.bands {
            assert_eq!(
                b.workability,
                Workability::from_score(b.score).label(),
                "{}",
                b.band
            );
        }
    }

    #[test]
    fn an_engine_that_runs_is_answered_untouched() {
        let wx = SpaceWx::default();
        let eng = make_predictor("p533", Some(ME), None, 0.0);
        let p = eng.predict(DX, FALLBACK_T, &wx);
        assert_eq!(p.engine, "p533");
        assert!(!p.engine_fallback);
        assert!(!eng.fell_back());
        assert!(!serde_json::to_string(&p)
            .unwrap()
            .contains("engineFallback"));
        let ring = band_outlook_ring(eng.as_ref(), ME, 9000.0, 1, FALLBACK_T, &wx);
        assert_eq!(ring.engine, "p533");
    }

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
            detail: None,
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
