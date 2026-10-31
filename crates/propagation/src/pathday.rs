//! A path over the day — the per-path prediction folded into one compact band × UTC-hour grid
//! a strip can draw, and the ten fixed DX regions the region strips sweep.
//!
//! [`path_day`] takes the short-path prediction (with its geometry) and, when the engine models
//! it, the long-path one, and keeps per band and hour exactly what a strip shows: each route's
//! modelled circuit reliability as a whole percent, and the route the model favours that hour
//! ([`crate::route::compare_routes_at`] — the same call the Path card makes for the current hour,
//! so the strip and the card never disagree). Per hour it keeps the band-independent geometry:
//! the hour's Kp and where that Kp came from, whether each route crosses the polar cap at that
//! Kp, and whether either end is on the grey line.
//!
//! The model part (the cells and the MUF) is a monthly median and changes once a day; the Kp
//! part follows the SWPC outlook, which is re-issued every half hour. So a cached day keeps its
//! cells and has its hours re-derived from fresh geometry ([`refresh_hours`]) each time it is
//! served — the geometry is microseconds, the prediction is not.
//!
//! Nothing here is a measurement: the cells are P.533's (or the heuristic's) monthly median, the
//! Kp is labelled with its own source, and the long path is the model's unvalidated opinion.

use serde::{Deserialize, Serialize};

use crate::kpforecast::KpForecast;
use crate::model::{Band, SpaceWx};
use crate::pathgeo::{path_geometry_route, KpSource, PathGeometry};
use crate::predict::{PathPrediction, PathPredictor};
use crate::route::{compare_routes_at, Route, RouteConfidence, RouteVerdict};

/// One band at one UTC hour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDayCell {
    /// Short-path modelled reliability, whole percent (0..=100).
    pub short_pct: u8,
    /// Long-path modelled reliability, when the engine models the long path.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub long_pct: Option<u8>,
    /// The route the model favours this hour; absent without a long path to compare.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub verdict: Option<RouteVerdict>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub confidence: Option<RouteConfidence>,
}

/// One band's 24 hours (index = UTC hour).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDayBand {
    pub band: String,
    pub cells: Vec<PathDayCell>,
}

/// One UTC hour's band-independent geometry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDayHour {
    /// Start of the hour described: its NEXT occurrence from the request (unix seconds UTC).
    pub unix: i64,
    /// `None` exactly when `kp_source` is unknown (no space-weather data held).
    pub kp: Option<f32>,
    pub kp_source: KpSource,
    /// The short / long path is known to cross the polar cap at that hour's Kp — false when it
    /// does not, and when the Kp is unknown (nothing is claimed).
    pub polar_short: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub polar_long: Option<bool>,
    /// The operator's end / the far end is on the grey line.
    pub grey_me: bool,
    pub grey_dx: bool,
}

/// A path's day: what the strip draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDay {
    /// The request's UTC hour.
    pub now_hour: u8,
    /// Per-UTC-hour MUF (MHz) of each route — the ceiling line.
    pub muf_short: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub muf_long: Option<Vec<f32>>,
    /// Each route's highest geomagnetic latitude (absolute, degrees): how deep a crossing goes —
    /// the polar cap proper, or only the auroral zone. Re-derived with the hours.
    #[serde(default)]
    pub max_gmlat_short: f32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_gmlat_long: Option<f32>,
    /// Highest band first, so a strip's rows rise with frequency like the MUF does.
    pub bands: Vec<PathDayBand>,
    /// 24 entries, UTC hour 0..23.
    pub hours: Vec<PathDayHour>,
}

fn pct(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// The day for the short-path prediction `sp` and, when the engine models it, the long-path
/// prediction `lp`. `None` without the short path's geometry (no operator grid).
pub fn path_day(sp: &PathPrediction, lp: Option<&PathPrediction>) -> Option<PathDay> {
    let gsp = sp.geometry.as_ref()?;
    // Per hour, the route call for every band (the card's own comparison, at that hour).
    let calls: Option<Vec<Vec<crate::route::RouteCall>>> =
        lp.map(|lp| (0..24).map(|h| compare_routes_at(sp, lp, h)).collect());
    let mut bands: Vec<PathDayBand> = sp
        .bands
        .iter()
        .map(|sb| {
            let lb = lp.and_then(|lp| lp.bands.iter().find(|b| b.band == sb.band));
            let cells = (0..24)
                .map(|h| {
                    let call = calls
                        .as_ref()
                        .and_then(|c| c[h].iter().find(|c| c.band == sb.band));
                    PathDayCell {
                        short_pct: pct(sb.hourly.get(h).copied().unwrap_or(0.0)),
                        long_pct: lb.map(|b| pct(b.hourly.get(h).copied().unwrap_or(0.0))),
                        verdict: call.map(|c| c.verdict),
                        confidence: call.and_then(|c| c.confidence),
                    }
                })
                .collect();
            PathDayBand {
                band: sb.band.clone(),
                cells,
            }
        })
        .collect();
    let mhz = |b: &PathDayBand| Band::from_label(&b.band).map_or(0.0, |b| b.center_mhz());
    bands.sort_by(|a, b| mhz(b).total_cmp(&mhz(a)));
    let mut day = PathDay {
        now_hour: gsp.now_hour,
        muf_short: sp.muf_hourly.clone(),
        muf_long: lp.map(|lp| lp.muf_hourly.clone()),
        max_gmlat_short: 0.0,
        max_gmlat_long: None,
        bands,
        hours: Vec::new(),
    };
    refresh_hours(&mut day, gsp, lp.and_then(|lp| lp.geometry.as_ref()));
    Some(day)
}

/// A polar share known to be above zero.
fn crosses(polar_frac: Option<f32>) -> bool {
    polar_frac.is_some_and(|p| p > 0.0)
}

/// Re-derive the day's per-hour geometry (Kp, its source, polar crossings, grey ends) and its
/// current hour from fresh geometry — what a cached day needs before it is served again.
pub fn refresh_hours(day: &mut PathDay, gsp: &PathGeometry, glp: Option<&PathGeometry>) {
    day.now_hour = gsp.now_hour;
    day.max_gmlat_short = gsp.max_gmlat_deg;
    day.max_gmlat_long = glp.map(|g| g.max_gmlat_deg);
    day.hours = gsp
        .hours
        .iter()
        .enumerate()
        .map(|(h, s)| PathDayHour {
            unix: s.unix,
            kp: s.kp,
            kp_source: s.kp_source,
            polar_short: crosses(s.polar_frac),
            polar_long: glp
                .and_then(|g| g.hours.get(h))
                .map(|l| crosses(l.polar_frac)),
            grey_me: s.grey_me,
            grey_dx: s.grey_dx,
        })
        .collect();
}

/// Whether a day read back from disk has the shape this build draws: 24 hours, 24 cells a band,
/// a MUF a hour, percents in range. Anything else is discarded, never drawn.
pub fn day_is_well_formed(day: &PathDay) -> bool {
    let pct_ok = |c: &PathDayCell| c.short_pct <= 100 && c.long_pct.is_none_or(|l| l <= 100);
    day.now_hour < 24
        && day.hours.len() == 24
        && day.muf_short.len() == 24
        && day.muf_long.as_ref().is_none_or(|m| m.len() == 24)
        && day.bands.iter().all(|b| {
            Band::from_label(&b.band).is_some() && b.cells.len() == 24 && b.cells.iter().all(pct_ok)
        })
}

/// One of the fixed DX regions the region strips sweep.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRegion {
    /// Stable id (the UI names it; never shown).
    pub id: &'static str,
    /// The anchor point the region's path is predicted to (lat, lon degrees).
    pub lat: f64,
    pub lon: f64,
    /// The CQ zones the region counts, for ordering by the operator's own needs and contacts.
    pub zones: &'static [u8],
}

/// The ten regions, in their default order (used as-is on an empty log). Each anchor is a point
/// with real on-air activity inside the region; the zones are the CQ zones the region stands
/// for. North America is not one of them, and Antarctica has no zone of its own (its stations
/// count in the zones of their longitude), so the polar region counts the Arctic zone only.
pub const PATH_REGIONS: [PathRegion; 10] = [
    PathRegion {
        id: "eu",
        lat: 50.1,
        lon: 8.7,
        zones: &[14, 15, 16, 20],
    },
    PathRegion {
        id: "ja",
        lat: 35.7,
        lon: 139.7,
        zones: &[18, 19, 23, 24, 25],
    },
    PathRegion {
        id: "vk",
        lat: -33.9,
        lon: 151.2,
        zones: &[29, 30, 32],
    },
    PathRegion {
        id: "pacific",
        lat: -18.1,
        lon: 178.4,
        zones: &[27, 31],
    },
    PathRegion {
        id: "sa",
        lat: -23.5,
        lon: -46.6,
        zones: &[9, 10, 11, 12, 13],
    },
    PathRegion {
        id: "af",
        lat: -1.3,
        lon: 36.8,
        zones: &[33, 34, 35, 36, 37, 38, 39],
    },
    PathRegion {
        id: "carib",
        lat: 18.5,
        lon: -69.9,
        zones: &[7, 8],
    },
    PathRegion {
        id: "me",
        lat: 24.7,
        lon: 46.7,
        zones: &[21],
    },
    PathRegion {
        id: "sas",
        lat: 20.6,
        lon: 78.9,
        zones: &[22, 26, 28],
    },
    PathRegion {
        id: "polar",
        lat: -62.2,
        lon: -58.9,
        zones: &[40],
    },
];

/// One region's day, as the sweep makes it and the shell keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionDay {
    /// A [`PATH_REGIONS`] id.
    pub id: String,
    pub day: PathDay,
}

/// Both routes' geometry from `me` to a region's anchor.
fn anchor_geometry(
    me: (f64, f64),
    r: &PathRegion,
    t: i64,
    kp_now: Option<f32>,
    forecast: Option<&KpForecast>,
) -> (PathGeometry, PathGeometry) {
    let g = |route| path_geometry_route(me, (r.lat, r.lon), route, t, kp_now, forecast, None);
    (g(Route::Short), g(Route::Long))
}

/// Predict every region's day from `me` at `t`: each anchor's short path and, when the engine
/// models it, its long path (the two on their own threads, so the sweep waits one prediction per
/// region), folded by [`path_day`]. Pure compute with no lock held: the caller runs it on a
/// blocking thread and keeps the answer.
pub fn sweep_regions(
    eng: &dyn PathPredictor,
    me: (f64, f64),
    t: i64,
    wx: &SpaceWx,
    kp_now: Option<f32>,
    forecast: Option<&KpForecast>,
) -> Vec<RegionDay> {
    PATH_REGIONS
        .iter()
        .filter_map(|r| {
            let dx = (r.lat, r.lon);
            let (mut sp, lp) = std::thread::scope(|s| {
                let lp = s.spawn(|| eng.predict_route(dx, Route::Long, t, wx));
                (eng.predict(dx, t, wx), lp.join().ok().flatten())
            });
            let (gs, gl) = anchor_geometry(me, r, t, kp_now, forecast);
            sp.geometry = Some(gs);
            let lp = lp.map(|mut lp| {
                lp.geometry = Some(gl);
                lp
            });
            Some(RegionDay {
                id: r.id.to_string(),
                day: path_day(&sp, lp.as_ref())?,
            })
        })
        .collect()
}

/// A kept sweep made current: every day's hours re-derived for `t` from fresh geometry (the Kp
/// outlook moves every half hour; the model does not), and anything that is not one of today's
/// regions or not a well-formed day dropped.
pub fn refresh_regions(
    days: &mut Vec<RegionDay>,
    me: (f64, f64),
    t: i64,
    kp_now: Option<f32>,
    forecast: Option<&KpForecast>,
) {
    days.retain(|d| day_is_well_formed(&d.day));
    days.retain_mut(|d| {
        let Some(r) = PATH_REGIONS.iter().find(|r| r.id == d.id) else {
            return false;
        };
        let (gs, gl) = anchor_geometry(me, r, t, kp_now, forecast);
        let long = d.day.muf_long.is_some();
        refresh_hours(&mut d.day, &gs, long.then_some(&gl));
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::likelihood::BandOutlook;
    use crate::predict::HeuristicEngine;

    /// 2026-05-15 12:00 UTC.
    const MAY_12Z: i64 = 1_778_846_400;
    const BOSTON: (f64, f64) = (42.36, -71.06);
    const SYDNEY: (f64, f64) = (-33.87, 151.21);
    const TOKYO: (f64, f64) = (35.68, 139.69);

    fn band(name: &str, hourly: Vec<f32>) -> BandOutlook {
        BandOutlook {
            band: name.to_string(),
            workability: "Fair".into(),
            score: 0.5,
            window: String::new(),
            grayline: false,
            hourly,
            reliability: 50.0,
            mode_now: Vec::new(),
            mode_hourly: Vec::new(),
            detail: None,
        }
    }

    fn pred(bands: Vec<BandOutlook>, route: Route) -> PathPrediction {
        PathPrediction {
            engine: "p533".into(),
            bands,
            muf_now: 0.0,
            muf_hourly: (0..24).map(|h| 10.0 + h as f32).collect(),
            geometry: Some(path_geometry_route(
                BOSTON,
                SYDNEY,
                route,
                MAY_12Z,
                Some(3.0),
                None,
                None,
            )),
            long_path: None,
            routes: None,
            day: None,
        }
    }

    /// Short path open in the first half of the day, long path in the second.
    fn ramp(open_first: bool) -> Vec<f32> {
        (0..24)
            .map(|h| if (h < 12) == open_first { 0.8 } else { 0.02 })
            .collect()
    }

    #[test]
    fn each_hour_carries_both_routes_and_the_cards_own_call_for_that_hour() {
        let sp = pred(
            vec![band("20m", ramp(true)), band("40m", vec![0.3; 24])],
            Route::Short,
        );
        let lp = pred(
            vec![band("20m", ramp(false)), band("40m", vec![0.3; 24])],
            Route::Long,
        );
        let day = path_day(&sp, Some(&lp)).unwrap();
        let b20 = day.bands.iter().find(|b| b.band == "20m").unwrap();
        assert_eq!(b20.cells.len(), 24);
        assert_eq!(
            (b20.cells[3].short_pct, b20.cells[3].long_pct),
            (80, Some(2))
        );
        assert_eq!(b20.cells[3].verdict, Some(RouteVerdict::Short));
        assert_eq!(b20.cells[20].verdict, Some(RouteVerdict::Long));
        // Every cell's call is exactly what the card computes for that hour.
        for h in 0..24 {
            for c in compare_routes_at(&sp, &lp, h) {
                let cell = day.bands.iter().find(|b| b.band == c.band).unwrap().cells[h];
                assert_eq!(
                    (cell.verdict, cell.confidence),
                    (Some(c.verdict), c.confidence)
                );
            }
        }
        assert_eq!(day.muf_long.as_ref().map(Vec::len), Some(24));
    }

    #[test]
    fn near_the_antipode_no_hour_calls_a_route() {
        // Boston's antipode is near 42.4° S 108.9° E: every heading is about the same distance,
        // so the strip makes no call, as the card makes none.
        let near = (-42.0, 109.3);
        let at = |route| {
            let mut p = pred(vec![band("20m", ramp(true))], route);
            p.geometry = Some(path_geometry_route(
                BOSTON,
                near,
                route,
                MAY_12Z,
                Some(3.0),
                None,
                None,
            ));
            p
        };
        let day = path_day(&at(Route::Short), Some(&at(Route::Long))).unwrap();
        assert!(day.bands[0]
            .cells
            .iter()
            .all(|c| c.verdict.is_none() && c.long_pct.is_some()));
        // Positive control: the same figures to Sydney get a call every hour.
        let sp = pred(vec![band("20m", ramp(true))], Route::Short);
        let lp = pred(vec![band("20m", ramp(false))], Route::Long);
        let d = path_day(&sp, Some(&lp)).unwrap();
        assert!(d.bands[0].cells.iter().all(|c| c.verdict.is_some()));
    }

    #[test]
    fn without_a_long_path_the_cells_carry_the_short_path_alone() {
        let sp = pred(vec![band("20m", ramp(true))], Route::Short);
        let day = path_day(&sp, None).unwrap();
        let c = day.bands[0].cells[0];
        assert_eq!((c.short_pct, c.long_pct, c.verdict), (80, None, None));
        assert!(day.muf_long.is_none());
        assert!(day.hours.iter().all(|h| h.polar_long.is_none()));
    }

    #[test]
    fn no_geometry_means_no_day() {
        let mut sp = pred(vec![band("20m", ramp(true))], Route::Short);
        sp.geometry = None;
        assert!(path_day(&sp, None).is_none());
    }

    #[test]
    fn bands_run_from_the_highest_down() {
        let sp = pred(
            vec![
                band("40m", vec![0.1; 24]),
                band("10m", vec![0.1; 24]),
                band("20m", vec![0.1; 24]),
            ],
            Route::Short,
        );
        let day = path_day(&sp, None).unwrap();
        let order: Vec<&str> = day.bands.iter().map(|b| b.band.as_str()).collect();
        assert_eq!(order, ["10m", "20m", "40m"]);
    }

    #[test]
    fn the_hours_are_the_geometry_s_kp_and_polar_crossings_per_route() {
        let sp = pred(vec![band("20m", ramp(true))], Route::Short);
        let lp = pred(vec![band("20m", ramp(false))], Route::Long);
        let day = path_day(&sp, Some(&lp)).unwrap();
        let (gs, gl) = (sp.geometry.as_ref().unwrap(), lp.geometry.as_ref().unwrap());
        assert_eq!(day.now_hour, gs.now_hour);
        for h in 0..24 {
            let d = day.hours[h];
            assert_eq!(
                (d.unix, d.kp, d.kp_source),
                (gs.hours[h].unix, gs.hours[h].kp, gs.hours[h].kp_source)
            );
            assert_eq!(d.polar_short, gs.hours[h].polar_frac.unwrap() > 0.0);
            assert_eq!(d.polar_long, Some(gl.hours[h].polar_frac.unwrap() > 0.0));
            assert_eq!(
                (day.max_gmlat_short, day.max_gmlat_long),
                (gs.max_gmlat_deg, Some(gl.max_gmlat_deg))
            );
            assert_eq!(
                (d.grey_me, d.grey_dx),
                (gs.hours[h].grey_me, gs.hours[h].grey_dx)
            );
        }
        // Positive control: Boston–Tokyo is in the cap both ways round every hour at Kp 3 …
        let tokyo =
            |route| path_geometry_route(BOSTON, TOKYO, route, MAY_12Z, Some(3.0), None, None);
        let mut d = day.clone();
        refresh_hours(&mut d, &tokyo(Route::Short), Some(&tokyo(Route::Long)));
        assert!(d
            .hours
            .iter()
            .all(|h| h.polar_short && h.polar_long == Some(true)));
        // … each route on its own: Quito–Santiago's short arc stays at low latitude while its long
        // arc runs over both poles, so only the long path is in the cap …
        let qs = |route| {
            path_geometry_route(
                (-0.18, -78.47),
                (-33.45, -70.67),
                route,
                MAY_12Z,
                Some(3.0),
                None,
                None,
            )
        };
        refresh_hours(&mut d, &qs(Route::Short), Some(&qs(Route::Long)));
        assert!(d
            .hours
            .iter()
            .all(|h| !h.polar_short && h.polar_long == Some(true)));
        // … negative control: Singapore–Nairobi never is, even at Kp 9.
        let eq = |route| {
            path_geometry_route(
                (1.35, 103.8),
                (-1.29, 36.82),
                route,
                MAY_12Z,
                Some(9.0),
                None,
                None,
            )
        };
        refresh_hours(&mut d, &eq(Route::Short), Some(&eq(Route::Long)));
        assert!(d
            .hours
            .iter()
            .all(|h| !h.polar_short && h.polar_long == Some(false)));
    }

    #[test]
    fn an_unknown_kp_is_carried_as_unknown_and_claims_no_polar_crossing() {
        let sp = pred(vec![band("20m", ramp(true))], Route::Short);
        let mut day = path_day(&sp, None).unwrap();
        let tokyo = |route| path_geometry_route(BOSTON, TOKYO, route, MAY_12Z, None, None, None);
        refresh_hours(&mut day, &tokyo(Route::Short), Some(&tokyo(Route::Long)));
        for h in &day.hours {
            assert_eq!((h.kp, h.kp_source), (None, KpSource::Unknown));
            assert!(!h.polar_short && h.polar_long == Some(false));
        }
    }

    #[test]
    fn a_cached_day_takes_fresh_hours_and_keeps_its_cells() {
        let sp = pred(vec![band("20m", ramp(true))], Route::Short);
        let mut day = path_day(&sp, None).unwrap();
        let cells = day.bands.clone();
        let later = path_geometry_route(
            BOSTON,
            SYDNEY,
            Route::Short,
            MAY_12Z + 5 * 3600,
            Some(9.0),
            None,
            None,
        );
        refresh_hours(&mut day, &later, None);
        assert_eq!(day.now_hour, 17);
        assert!(day.hours.iter().all(|h| h.kp == Some(9.0)));
        assert_eq!(day.bands, cells);
    }

    #[test]
    fn only_a_well_formed_day_is_kept() {
        let sp = pred(vec![band("20m", ramp(true))], Route::Short);
        let good = path_day(&sp, None).unwrap();
        assert!(day_is_well_formed(&good));
        let mut d = good.clone();
        d.hours.pop();
        assert!(!day_is_well_formed(&d));
        let mut d = good.clone();
        d.bands[0].cells[0].short_pct = 101;
        assert!(!day_is_well_formed(&d));
        let mut d = good.clone();
        d.bands[0].band = "99m".into();
        assert!(!day_is_well_formed(&d));
        let mut d = good.clone();
        d.muf_short.truncate(3);
        assert!(!day_is_well_formed(&d));
        let mut d = good;
        d.now_hour = 24;
        assert!(!day_is_well_formed(&d));
    }

    #[test]
    fn the_sweep_gives_every_region_a_day_and_a_kept_one_is_made_current() {
        let wx = SpaceWx::default();
        let eng = HeuristicEngine::new(Some(BOSTON));
        let mut days = sweep_regions(&eng, BOSTON, MAY_12Z, &wx, Some(2.0), None);
        let ids: Vec<&str> = days.iter().map(|d| d.id.as_str()).collect();
        let want: Vec<&str> = PATH_REGIONS.iter().map(|r| r.id).collect();
        assert_eq!(ids, want);
        assert!(days.iter().all(|d| day_is_well_formed(&d.day)));
        // The heuristic models no long path: nothing long is claimed.
        assert!(days.iter().all(|d| d.day.muf_long.is_none()
            && d.day
                .bands
                .iter()
                .all(|b| b.cells.iter().all(|c| c.long_pct.is_none()))));
        // Served five hours later in a storm: fresh hours, same cells.
        let cells: Vec<_> = days.iter().map(|d| d.day.bands.clone()).collect();
        days.push(RegionDay {
            id: "atlantis".into(),
            day: days[0].day.clone(),
        });
        let mut broken = days[0].clone();
        broken.day.hours.clear();
        days.push(broken);
        refresh_regions(&mut days, BOSTON, MAY_12Z + 5 * 3600, Some(9.0), None);
        assert_eq!(
            days.len(),
            10,
            "an unknown region and a malformed day are dropped"
        );
        assert!(days
            .iter()
            .all(|d| d.day.now_hour == 17 && d.day.hours.iter().all(|h| h.kp == Some(9.0))));
        assert_eq!(
            days.iter().map(|d| d.day.bands.clone()).collect::<Vec<_>>(),
            cells
        );
        // The polar region's short path from Boston crosses the cap in a Kp 9 storm; Europe's
        // does too, the Caribbean's does not (positive and negative control).
        let polar = |id: &str| days.iter().find(|d| d.id == id).unwrap().day.hours[0].polar_short;
        assert!(polar("eu") && !polar("carib"));
    }

    #[test]
    fn ten_regions_with_distinct_ids_and_zones() {
        let mut ids: Vec<&str> = PATH_REGIONS.iter().map(|r| r.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 10);
        let mut zones: Vec<u8> = PATH_REGIONS
            .iter()
            .flat_map(|r| r.zones.iter().copied())
            .collect();
        let n = zones.len();
        zones.sort_unstable();
        zones.dedup();
        assert_eq!(zones.len(), n, "a CQ zone counts in one region only");
        assert!(zones.iter().all(|z| (1..=40).contains(z)));
        assert!(PATH_REGIONS
            .iter()
            .all(|r| r.lat.abs() <= 90.0 && r.lon.abs() <= 180.0));
    }
}
