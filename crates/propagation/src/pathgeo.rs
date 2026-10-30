//! Path geometry along the great circle — the cheap, model-free half of "which way is this
//! signal going, and what is in its way".
//!
//! The P.533 engine is a monthly-median model: it knows nothing about today's storm or where
//! the terminator sits on the path right now. This module answers those from geometry alone,
//! in microseconds and with no engine run, by sampling the short-path great circle and asking
//! of each sample:
//!
//! - **auroral zone / polar cap** — is it poleward of the cap boundary Λ(Kp)
//!   ([`crate::pca::cutoff_lat_deg`], the same cutoff the map's polar-cap layer draws), with Kp
//!   taken per hour from the SWPC outlook ([`KpForecast`]) so a storm forecast for 18Z moves the
//!   18Z answer and no other. At quiet Kp that edge sits near 64° geomagnetic, so a path that
//!   only grazes it is in the auroral zone; the polar cap proper lies further poleward;
//! - **auroral oval** — is it inside the live OVATION oval the map's aurora layer draws (a
//!   nowcast, so it answers for the current hour only, and only when the oval is held);
//! - **grey line** — is the sun within ±6° of the horizon there ([`GREYLINE_DEG`], the band the
//!   grey-line window uses), and is it sunlit.
//!
//! Every Kp value carries its source, so a surface can say "forecast" for a forecast and never
//! present a prediction as an observation. With no space-weather data at all an hour's Kp is
//! [`KpSource::Unknown`] and every Kp-dependent number is absent rather than a stand-in. Either
//! route: the long path samples the other arc of the same great circle ([`path_geometry_route`]).

use serde::Serialize;

use crate::geo;
use crate::kpforecast::{KpForecast, KpKind};
use crate::pca::cutoff_lat_deg;
use crate::route::Route;

/// Samples along the arc, both ends included. 64 segments keep the spacing ≤ ~310 km
/// (≤ 2.8°) on the longest short path, finer than the 2° OVATION cells it is matched to.
pub const SAMPLES: usize = 65;
/// Samples along the long path, which is up to twice as long: the same spacing.
pub const SAMPLES_LONG: usize = 129;
/// Earth circumference (km) on the sphere [`geo`] uses (radius 6371 km).
const CIRCUMFERENCE_KM: f64 = 2.0 * std::f64::consts::PI * 6371.0;

/// Solar elevation band (degrees either side of the horizon) counted as grey line.
pub const GREYLINE_DEG: f64 = 6.0;

/// Where an hour's Kp came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KpSource {
    /// SWPC measured it (the period has passed or is in progress with final data).
    Measured,
    /// SWPC's estimate or forecast for the period — a prediction, worded as one.
    Forecast,
    /// No outlook sample covers the hour: SWPC's latest reading, assumed to persist.
    Current,
    /// No outlook sample covers the hour and no SWPC reading is held (cold cache, or the last
    /// fetch failed): Kp is unknown, so the hour carries no Kp, cap edge or polar share.
    Unknown,
}

/// One UTC hour of path geometry.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathGeoHour {
    /// Start of the hour this entry describes (unix seconds UTC) — the NEXT occurrence of
    /// that UTC hour from the request, the current hour included.
    pub unix: i64,
    /// `None` exactly when `kp_source` is [`KpSource::Unknown`], and so are the two below.
    pub kp: Option<f32>,
    pub kp_source: KpSource,
    /// The polar-cap boundary at that Kp (geomagnetic latitude, degrees).
    pub cap_lat_deg: Option<f32>,
    /// Share of the path (0..1) poleward of the cap boundary.
    pub polar_frac: Option<f32>,
    /// Share of the path (0..1) on the grey line (sun within ±6° of the horizon).
    pub greyline_frac: f32,
    /// Share of the path (0..1) in sunlight.
    pub sunlit_frac: f32,
    /// Grey line at the operator's end / the far end.
    pub grey_me: bool,
    pub grey_dx: bool,
}

/// What a stretch of the arc is crossing in the request's hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SegmentKind {
    /// Poleward of the polar-cap boundary at the hour's Kp.
    Polar,
    /// On the grey line (sun within ±6° of the horizon).
    Grey,
}

/// A stretch of the arc, as fractions of its length from the operator (0) to the far end (1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathSegment {
    pub from: f32,
    pub to: f32,
    pub kind: SegmentKind,
}

/// The geometry of one route between two stations over the next 24 hours.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathGeometry {
    /// Great-circle distance (km) and initial bearing from the operator (degrees true).
    pub dist_km: f32,
    pub bearing_deg: f32,
    /// Highest geomagnetic latitude the path reaches (absolute, degrees).
    pub max_gmlat_deg: f32,
    /// The UTC hour of the request (index into `hours`).
    pub now_hour: u8,
    /// Whether the path crosses the live auroral oval right now; `None` when no oval is held.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oval_now: Option<bool>,
    /// 24 entries, UTC hour 0..23, aligned with the engine's `hourly` arrays.
    pub hours: Vec<PathGeoHour>,
    /// The stretches of the arc inside the polar cap or on the grey line in the request's
    /// hour — what a map draws over the path.
    pub segments_now: Vec<PathSegment>,
}

/// Runs of flagged samples as arc fractions; each sample stands for half a step either side.
fn segments_of(flags: &[bool], kind: SegmentKind, out: &mut Vec<PathSegment>) {
    let last = (flags.len() - 1) as f32;
    let half = 0.5 / last;
    let mut i = 0;
    while i < flags.len() {
        if !flags[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < flags.len() && flags[i] {
            i += 1;
        }
        out.push(PathSegment {
            from: (start as f32 / last - half).max(0.0),
            to: ((i - 1) as f32 / last + half).min(1.0),
            kind,
        });
    }
}

/// Kp for the hour starting at `unix`: the outlook sample whose 3-hour period contains it,
/// else SWPC's latest reading, else unknown.
fn kp_at(unix: i64, kp_now: Option<f32>, forecast: Option<&KpForecast>) -> (Option<f32>, KpSource) {
    const PERIOD: i64 = 3 * 3600;
    forecast
        .and_then(|f| {
            f.points
                .iter()
                .find(|p| p.time_unix <= unix && unix < p.time_unix + PERIOD)
        })
        .map(|p| {
            let src = match p.kind {
                KpKind::Observed => KpSource::Measured,
                KpKind::Estimated | KpKind::Predicted => KpSource::Forecast,
            };
            (Some(p.kp), src)
        })
        .unwrap_or(match kp_now {
            Some(kp) => (Some(kp), KpSource::Current),
            None => (None, KpSource::Unknown),
        })
}

/// Key of the 2° OVATION cell nearest a point (longitude wrapped to −180..180).
fn oval_cell(lat: f64, lon: f64) -> (i32, i32) {
    let lon = (lon + 540.0).rem_euclid(360.0) - 180.0;
    (
        (lat / 2.0).round() as i32,
        ((lon / 2.0).round() as i32 + 90).rem_euclid(180),
    )
}

/// Classify the short path `me` → `dx` (lat, lon degrees) for the 24 UTC hours starting at
/// the hour containing `from_unix`. `kp_now` is the planetary K SWPC last reported — `None`
/// when no reading is held, never a stand-in; `forecast` the SWPC outlook when held; `oval` the live OVATION oval as (lat, lon) cells on its 2° grid — every
/// cell the map's aurora layer draws.
pub fn path_geometry(
    me: (f64, f64),
    dx: (f64, f64),
    from_unix: i64,
    kp_now: Option<f32>,
    forecast: Option<&KpForecast>,
    oval: Option<&[(f32, f32)]>,
) -> PathGeometry {
    path_geometry_route(me, dx, Route::Short, from_unix, kp_now, forecast, oval)
}

/// [`path_geometry`] for either route. The long path leaves on the reciprocal bearing and
/// runs the rest of the great circle; its distance and bearing are the long path's.
pub fn path_geometry_route(
    me: (f64, f64),
    dx: (f64, f64),
    route: Route,
    from_unix: i64,
    kp_now: Option<f32>,
    forecast: Option<&KpForecast>,
    oval: Option<&[(f32, f32)]>,
) -> PathGeometry {
    let sp_km = geo::haversine_km(me, dx);
    let sp_brg = geo::bearing_deg(me, dx);
    let (pts, dist_km, bearing_deg): (Vec<(f64, f64)>, f64, f64) = match route {
        Route::Short => (
            (0..SAMPLES)
                .map(|i| geo::interpolate(me, dx, i as f64 / (SAMPLES - 1) as f64))
                .collect(),
            sp_km,
            sp_brg,
        ),
        Route::Long => {
            let km = CIRCUMFERENCE_KM - sp_km;
            let brg = (sp_brg + 180.0) % 360.0;
            let n = SAMPLES_LONG;
            (
                (0..n)
                    .map(|i| geo::destination_point(me, brg, km * i as f64 / (n - 1) as f64))
                    .collect(),
                km,
                brg,
            )
        }
    };
    let samples = pts.len();
    let gmlat: Vec<f64> = pts
        .iter()
        .map(|&(la, lo)| geo::geomagnetic_lat_deg(la, lo).abs())
        .collect();
    let max_gmlat = gmlat.iter().cloned().fold(0.0f64, f64::max);

    let oval_now = oval.map(|cells| {
        let set: std::collections::HashSet<(i32, i32)> = cells
            .iter()
            .map(|&(la, lo)| oval_cell(la as f64, lo as f64))
            .collect();
        pts.iter().any(|&(la, lo)| {
            let (r, c) = oval_cell(la, lo);
            // The cell and its neighbours: a sample anywhere within one cell of the oval.
            (-1..=1).any(|dr| (-1..=1).any(|dc| set.contains(&(r + dr, (c + dc).rem_euclid(180)))))
        })
    });

    let n = samples as f32;
    let mut segments_now = Vec::new();
    let hour0 = from_unix - from_unix.rem_euclid(3600);
    let now_hour = (from_unix.rem_euclid(86_400) / 3600) as usize;
    let hours = (0..24usize)
        .map(|h| {
            // The next occurrence of UTC hour h, the current hour included.
            let unix = hour0 + (((h + 24 - now_hour) % 24) as i64) * 3600;
            // Solar position at the middle of the hour.
            let mid = unix + 1800;
            let (kp, kp_source) = kp_at(unix, kp_now, forecast);
            let cap = kp.map(|kp| cutoff_lat_deg(kp as f64));
            let polar = cap.map(|cap| gmlat.iter().filter(|&&g| g >= cap).count());
            let elev: Vec<f64> = pts
                .iter()
                .map(|&(la, lo)| geo::solar_elevation_deg(la, lo, mid))
                .collect();
            let grey = |e: f64| e.abs() <= GREYLINE_DEG;
            if h == now_hour {
                // An unknown Kp has no cap edge, so no polar stretch is drawn from it.
                if let Some(cap) = cap {
                    let polar: Vec<bool> = gmlat.iter().map(|&g| g >= cap).collect();
                    segments_of(&polar, SegmentKind::Polar, &mut segments_now);
                }
                let greys: Vec<bool> = elev.iter().map(|&e| grey(e)).collect();
                segments_of(&greys, SegmentKind::Grey, &mut segments_now);
            }
            PathGeoHour {
                unix,
                kp,
                kp_source,
                cap_lat_deg: cap.map(|c| c as f32),
                polar_frac: polar.map(|p| p as f32 / n),
                greyline_frac: elev.iter().filter(|&&e| grey(e)).count() as f32 / n,
                sunlit_frac: elev.iter().filter(|&&e| e > 0.0).count() as f32 / n,
                grey_me: grey(elev[0]),
                grey_dx: grey(elev[samples - 1]),
            }
        })
        .collect();

    PathGeometry {
        dist_km: dist_km as f32,
        bearing_deg: bearing_deg as f32,
        max_gmlat_deg: max_gmlat as f32,
        now_hour: now_hour as u8,
        oval_now,
        hours,
        segments_now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kpforecast::KpPoint;

    const BOSTON: (f64, f64) = (42.36, -71.06);
    const LONDON: (f64, f64) = (51.5, -0.12);
    const TOKYO: (f64, f64) = (35.68, 139.69);
    /// 2026-05-15 12:00 UTC.
    const MAY_12Z: i64 = 1_778_846_400;

    fn quiet(me: (f64, f64), dx: (f64, f64), kp: f32) -> PathGeometry {
        path_geometry(me, dx, MAY_12Z, Some(kp), None, None)
    }

    #[test]
    fn boston_tokyo_crosses_the_polar_cap_at_quiet_kp() {
        let g = quiet(BOSTON, TOKYO, 2.0);
        // The short path to Japan runs up over Hudson Bay and Alaska.
        assert!(g.max_gmlat_deg > 66.0, "{}", g.max_gmlat_deg);
        let h = &g.hours[12];
        assert!(h.polar_frac.unwrap() > 0.1, "{:?}", h.polar_frac);
        assert_eq!(h.kp_source, KpSource::Current);
        // Bearing ~ north-north-west, ~10 800 km.
        assert!((330.0..360.0).contains(&g.bearing_deg), "{}", g.bearing_deg);
        assert!((10_500.0..11_100.0).contains(&g.dist_km), "{}", g.dist_km);
    }

    #[test]
    fn boston_london_stays_out_of_the_cap_at_low_kp_and_enters_in_a_storm() {
        // Negative control: Kp 1 puts the cap boundary near 65°.
        let low = quiet(BOSTON, LONDON, 1.0);
        assert!(low.hours.iter().all(|h| h.polar_frac == Some(0.0)));
        assert!(
            low.max_gmlat_deg < cutoff_lat_deg(1.0) as f32,
            "{}",
            low.max_gmlat_deg
        );
        // The same path under a severe storm: the cap grows equatorward over it.
        let storm = quiet(BOSTON, LONDON, 8.0);
        assert!(
            storm.hours.iter().all(|h| h.polar_frac.unwrap() > 0.0),
            "{}",
            storm.max_gmlat_deg
        );
    }

    #[test]
    fn an_equatorial_path_never_touches_the_cap_even_at_kp_9() {
        // Singapore → Nairobi.
        let g = quiet((1.35, 103.82), (-1.29, 36.82), 9.0);
        assert!(g.hours.iter().all(|h| h.polar_frac == Some(0.0)));
        assert!(g.max_gmlat_deg < 30.0);
    }

    #[test]
    fn kp_is_taken_per_hour_from_the_outlook() {
        // Kp 1 measured for 12–15Z, Kp 8 forecast for 18–21Z; nothing covers 15–18Z.
        let f = KpForecast {
            points: vec![
                KpPoint {
                    time_unix: MAY_12Z,
                    kp: 1.0,
                    kind: KpKind::Observed,
                    noaa_scale: None,
                },
                KpPoint {
                    time_unix: MAY_12Z + 6 * 3600,
                    kp: 8.0,
                    kind: KpKind::Predicted,
                    noaa_scale: Some("G4".into()),
                },
            ],
        };
        let g = path_geometry(BOSTON, LONDON, MAY_12Z + 600, Some(2.0), Some(&f), None);
        assert_eq!(g.now_hour, 12);
        assert_eq!(
            (g.hours[12].kp, g.hours[12].kp_source),
            (Some(1.0), KpSource::Measured)
        );
        assert_eq!(g.hours[12].polar_frac, Some(0.0));
        assert_eq!(
            (g.hours[16].kp, g.hours[16].kp_source),
            (Some(2.0), KpSource::Current)
        );
        assert_eq!(
            (g.hours[19].kp, g.hours[19].kp_source),
            (Some(8.0), KpSource::Forecast)
        );
        assert!(
            g.hours[19].polar_frac.unwrap() > 0.0,
            "the forecast storm reaches this path at 19Z"
        );
        assert!(g.hours[19].cap_lat_deg < g.hours[12].cap_lat_deg);
        // Hours before now describe TOMORROW's occurrence, so 11Z is 23 h ahead.
        assert_eq!(g.hours[11].unix, MAY_12Z + 23 * 3600);
        assert_eq!(g.hours[12].unix, MAY_12Z);
    }

    #[test]
    fn a_path_along_the_sunrise_terminator_is_mostly_grey_line() {
        // Equinox 2026-03-20 06:00 UTC: the terminator lies close to the Greenwich meridian,
        // so London → Accra (due south along ~0°) runs down it at sunrise.
        let eq_06z = 1_773_986_400;
        let g = path_geometry(LONDON, (5.6, -0.19), eq_06z, Some(2.0), None, None);
        let at = &g.hours[6];
        assert!(at.greyline_frac > 0.8, "{}", at.greyline_frac);
        assert!(at.grey_me && at.grey_dx);
        // Negative controls: the same path at noon, and an east–west path at the same instant.
        assert_eq!(g.hours[12].greyline_frac, 0.0);
        assert_eq!(g.hours[12].sunlit_frac, 1.0);
        let ew = path_geometry(LONDON, (55.75, 37.62), eq_06z, Some(2.0), None, None);
        assert!(
            ew.hours[6].greyline_frac < 0.5,
            "{}",
            ew.hours[6].greyline_frac
        );
    }

    #[test]
    fn the_live_oval_is_matched_cell_by_cell() {
        // A ring of oval cells at 64–66° N, every 2° of longitude.
        let ring: Vec<(f32, f32)> = (-90..90)
            .flat_map(|i| [(64.0, i as f32 * 2.0), (66.0, i as f32 * 2.0)])
            .collect();
        let g = path_geometry(BOSTON, TOKYO, MAY_12Z, Some(2.0), None, Some(&ring));
        assert_eq!(g.oval_now, Some(true));
        // Negative controls: an empty oval is "no"; no oval held at all is "unknown", not "no";
        // and an oval wholly poleward of the transatlantic path is not crossed.
        let g = path_geometry(BOSTON, LONDON, MAY_12Z, Some(2.0), None, Some(&ring[..0]));
        assert_eq!(g.oval_now, Some(false));
        assert_eq!(quiet(BOSTON, LONDON, 2.0).oval_now, None);
        let high: Vec<(f32, f32)> = (-90..90).map(|i| (80.0, i as f32 * 2.0)).collect();
        let g = path_geometry(BOSTON, LONDON, MAY_12Z, Some(2.0), None, Some(&high));
        assert_eq!(g.oval_now, Some(false));
    }

    fn long(me: (f64, f64), dx: (f64, f64), kp: f32) -> PathGeometry {
        path_geometry_route(me, dx, Route::Long, MAY_12Z, Some(kp), None, None)
    }

    #[test]
    fn the_long_path_is_the_rest_of_the_circle_on_the_reciprocal_bearing() {
        let (sp, lp) = (quiet(BOSTON, TOKYO, 2.0), long(BOSTON, TOKYO, 2.0));
        assert!(
            (sp.dist_km + lp.dist_km - CIRCUMFERENCE_KM as f32).abs() < 1.0,
            "{} + {}",
            sp.dist_km,
            lp.dist_km
        );
        let d = (lp.bearing_deg - sp.bearing_deg).rem_euclid(360.0);
        assert!((d - 180.0).abs() < 1e-3, "{d}");
        // The last long-path sample is the far station, reached the other way round.
        let end = geo::destination_point(BOSTON, lp.bearing_deg as f64, lp.dist_km as f64);
        assert!(geo::haversine_km(end, TOKYO) < 5.0, "{end:?}");
    }

    #[test]
    fn a_polar_short_path_has_a_long_path_over_the_other_cap() {
        // Boston -> Tokyo runs over the northern cap at quiet Kp. Its long path is no escape:
        // every great circle reaches the same geomagnetic latitude in both hemispheres (the
        // dipole frame is a rotation), and the short path holds one apex, so the long path holds
        // the other — it crosses the southern cap instead.
        let (sp, lp) = (quiet(BOSTON, TOKYO, 3.0), long(BOSTON, TOKYO, 3.0));
        assert!(sp.hours.iter().all(|h| h.polar_frac.unwrap() > 0.0));
        assert!(lp.hours.iter().all(|h| h.polar_frac.unwrap() > 0.0));
        assert!(
            lp.max_gmlat_deg >= sp.max_gmlat_deg - 0.5,
            "{} vs {}",
            lp.max_gmlat_deg,
            sp.max_gmlat_deg
        );
        // Where the cap stretch is: north on the short path, south on the long one.
        let mid_lat = |g: &PathGeometry| {
            let s = g
                .segments_now
                .iter()
                .find(|s| s.kind == SegmentKind::Polar)
                .expect("a polar stretch");
            let f = ((s.from + s.to) / 2.0) as f64;
            geo::destination_point(BOSTON, g.bearing_deg as f64, g.dist_km as f64 * f).0
        };
        assert!(mid_lat(&sp) > 50.0, "{}", mid_lat(&sp));
        assert!(mid_lat(&lp) < -50.0, "{}", mid_lat(&lp));
    }

    #[test]
    fn with_kp_unknown_neither_route_draws_a_polar_stretch() {
        // Boston -> Tokyo crosses a cap both ways round at any real Kp (positive control:
        // the test above). With no Kp held there is no cap edge to draw from.
        for route in [Route::Short, Route::Long] {
            let g = path_geometry_route(BOSTON, TOKYO, route, MAY_12Z, None, None, None);
            assert!(
                g.segments_now.iter().all(|s| s.kind != SegmentKind::Polar),
                "{route:?}: {:?}",
                g.segments_now
            );
            assert!(g.hours.iter().all(|h| h.polar_frac.is_none()));
            let known = path_geometry_route(BOSTON, TOKYO, route, MAY_12Z, Some(2.0), None, None);
            assert!(known
                .segments_now
                .iter()
                .any(|s| s.kind == SegmentKind::Polar));
        }
    }

    #[test]
    fn an_equatorial_long_path_stays_out_of_the_cap_at_kp_9() {
        // Singapore -> Nairobi: the short path and its long path both hug the equator.
        let lp = path_geometry_route(
            (1.35, 103.82),
            (-1.29, 36.82),
            Route::Long,
            MAY_12Z,
            Some(9.0),
            None,
            None,
        );
        assert!(lp.hours.iter().all(|h| h.polar_frac == Some(0.0)));
        assert!(lp.max_gmlat_deg < 35.0, "{}", lp.max_gmlat_deg);
        assert!((32_000.0..34_000.0).contains(&lp.dist_km), "{}", lp.dist_km);
    }

    #[test]
    fn segments_cover_exactly_the_flagged_stretch() {
        let mut out = Vec::new();
        // 5 samples (4 steps): samples 1..=2 flagged → 0.25±0.125 .. 0.5±0.125.
        segments_of(
            &[false, true, true, false, true],
            SegmentKind::Grey,
            &mut out,
        );
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].from, out[0].to), (0.125, 0.625));
        // The end sample is clamped to the arc.
        assert_eq!((out[1].from, out[1].to), (0.875, 1.0));
        out.clear();
        segments_of(&[false; 5], SegmentKind::Grey, &mut out);
        assert!(out.is_empty());
        // At equinox 06Z the London -> Accra arc is grey end to end: one segment, 0..1.
        let g = path_geometry(LONDON, (5.6, -0.19), 1_773_986_400, Some(2.0), None, None);
        let grey: Vec<_> = g
            .segments_now
            .iter()
            .filter(|s| s.kind == SegmentKind::Grey)
            .collect();
        assert_eq!(grey.len(), 1);
        assert!(grey[0].from == 0.0 && grey[0].to == 1.0, "{grey:?}");
    }

    const MINNEAPOLIS: (f64, f64) = (44.98, -93.27);

    #[test]
    fn with_no_space_weather_kp_is_unknown_and_nothing_depends_on_it() {
        // Cold cache or a failed SWPC fetch: no reading is handed in, no outlook is held.
        let g = path_geometry(MINNEAPOLIS, TOKYO, MAY_12Z, None, None, None);
        for h in &g.hours {
            assert_eq!(h.kp_source, KpSource::Unknown);
            assert_eq!((h.kp, h.cap_lat_deg, h.polar_frac), (None, None, None));
        }
        // The model-free geometry is still there.
        assert!(g.max_gmlat_deg > 60.0);
        // An outlook still answers the hours it covers; only the gaps are unknown.
        let f = KpForecast {
            points: vec![KpPoint {
                time_unix: MAY_12Z,
                kp: 3.0,
                kind: KpKind::Predicted,
                noaa_scale: None,
            }],
        };
        let g = path_geometry(MINNEAPOLIS, TOKYO, MAY_12Z, None, Some(&f), None);
        assert_eq!(
            (g.hours[13].kp, g.hours[13].kp_source),
            (Some(3.0), KpSource::Forecast)
        );
        assert!(g.hours[13].polar_frac.is_some());
        assert_eq!(g.hours[15].kp_source, KpSource::Unknown);
        assert_eq!(g.hours[15].polar_frac, None);
        // Positive control: a reading SWPC gave is carried, labelled as the latest reading.
        let g = path_geometry(MINNEAPOLIS, TOKYO, MAY_12Z, Some(2.0), Some(&f), None);
        assert_eq!(
            (g.hours[15].kp, g.hours[15].kp_source),
            (Some(2.0), KpSource::Current)
        );
    }

    #[test]
    fn minneapolis_tokyo_grazes_the_edge_between_kp_1_and_kp_2() {
        // Its highest geomagnetic latitude (~64.7°) lies between the Kp 2 edge (63.6°) and the
        // Kp 1 edge (64.8°): exactly the case a stand-in Kp would decide.
        let at2 = quiet(MINNEAPOLIS, TOKYO, 2.0);
        assert!(
            (cutoff_lat_deg(2.0) as f32..cutoff_lat_deg(1.0) as f32).contains(&at2.max_gmlat_deg),
            "{}",
            at2.max_gmlat_deg
        );
        assert!(at2.hours.iter().all(|h| h.polar_frac.unwrap() > 0.0));
        let at1 = quiet(MINNEAPOLIS, TOKYO, 1.0);
        assert!(at1.hours.iter().all(|h| h.polar_frac == Some(0.0)));
    }

    #[test]
    fn grey_line_is_told_apart_at_each_end() {
        // Equinox 06Z: sunrise at London, mid-morning in Moscow.
        let eq_06z = 1_773_986_400;
        let moscow = (55.75, 37.62);
        let g = path_geometry(LONDON, moscow, eq_06z, Some(2.0), None, None);
        assert_eq!((g.hours[6].grey_me, g.hours[6].grey_dx), (true, false));
        // The same path the other way round swaps the ends.
        let r = path_geometry(moscow, LONDON, eq_06z, Some(2.0), None, None);
        assert_eq!((r.hours[6].grey_me, r.hours[6].grey_dx), (false, true));
    }

    #[test]
    fn twilight_is_grey_line_not_daylight_and_the_sun_is_taken_mid_hour() {
        // A 10 km path on the Dutch coast, equinox: at 05:30Z the sun is ~4° below the horizon
        // (civil twilight), at 05:00Z ~9° below (dark), at 06:30Z ~5° up.
        let eq_05z = 1_773_986_400 - 3600;
        let g = path_geometry((51.5, 3.0), (51.59, 3.0), eq_05z, Some(2.0), None, None);
        let h = &g.hours[5];
        // Mid-hour sun: twilight, so all grey line. At the hour's start it would be none.
        assert_eq!(h.greyline_frac, 1.0);
        // Below the horizon is not daylight, twilight or not.
        assert_eq!(h.sunlit_frac, 0.0);
        // Positive control: an hour later the sun is up and still low — grey and sunlit.
        assert_eq!(
            (g.hours[6].greyline_frac, g.hours[6].sunlit_frac),
            (1.0, 1.0)
        );
        // Negative control: noon is full daylight, no grey line.
        assert_eq!(
            (g.hours[12].greyline_frac, g.hours[12].sunlit_frac),
            (0.0, 1.0)
        );
    }

    #[test]
    fn the_oval_counts_within_one_cell_of_the_path_and_no_further() {
        // A short path near Boston, all on the 42° cell row; one oval cell straight north.
        let near = (42.36, -70.0);
        let one_north = [(44.0f32, -71.0f32)];
        let g = path_geometry(BOSTON, near, MAY_12Z, Some(2.0), None, Some(&one_north));
        assert_eq!(g.oval_now, Some(true), "one cell away counts");
        let two_north = [(46.0f32, -71.0f32)];
        let g = path_geometry(BOSTON, near, MAY_12Z, Some(2.0), None, Some(&two_north));
        assert_eq!(g.oval_now, Some(false), "two cells away does not");
    }

    #[test]
    fn classifying_a_path_is_cheap() {
        // Budget: tens of µs per path in release, about 0.1 ms in a debug build — cheap enough
        // to run beside every outlook request with no caching. The debug budget leaves ~500×
        // headroom so a loaded runner cannot flake it; it still catches a classifier that
        // starts doing per-sample work it should not.
        let f = KpForecast::default();
        let ring: Vec<(f32, f32)> = (-90..90).map(|i| (66.0, i as f32 * 2.0)).collect();
        let runs = 200;
        let t0 = std::time::Instant::now();
        for i in 0..runs {
            std::hint::black_box(path_geometry(
                BOSTON,
                TOKYO,
                MAY_12Z + i * 60,
                Some(3.0),
                Some(&f),
                Some(&ring),
            ));
        }
        let per = t0.elapsed() / runs as u32;
        let budget = if cfg!(debug_assertions) {
            std::time::Duration::from_millis(50)
        } else {
            std::time::Duration::from_micros(500)
        };
        assert!(per < budget, "{per:?} per path");
    }
}
