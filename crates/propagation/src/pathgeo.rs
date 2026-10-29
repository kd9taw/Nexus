//! Path geometry along the great circle — the cheap, model-free half of "which way is this
//! signal going, and what is in its way".
//!
//! The P.533 engine is a monthly-median model: it knows nothing about today's storm or where
//! the terminator sits on the path right now. This module answers those from geometry alone,
//! in microseconds and with no engine run, by sampling the short-path great circle and asking
//! of each sample:
//!
//! - **polar cap** — is it poleward of the cap boundary Λ(Kp) ([`crate::pca::cutoff_lat_deg`],
//!   the same cutoff the map's polar-cap layer draws), with Kp taken per hour from the SWPC
//!   outlook ([`KpForecast`]) so a storm forecast for 18Z moves the 18Z answer and no other;
//! - **auroral oval** — is it inside the live OVATION oval the map's aurora layer draws (a
//!   nowcast, so it answers for the current hour only, and only when the oval is held);
//! - **grey line** — is the sun within ±6° of the horizon there ([`GREYLINE_DEG`], the band the
//!   grey-line window uses), and is it sunlit.
//!
//! Every Kp value carries its source, so a surface can say "forecast" for a forecast and never
//! present a prediction as an observation. Short path only; the long path arrives with the
//! route parameter.

use serde::Serialize;

use crate::geo;
use crate::kpforecast::{KpForecast, KpKind};
use crate::pca::cutoff_lat_deg;

/// Samples along the arc, both ends included. 64 segments keep the spacing ≤ ~310 km
/// (≤ 2.8°) on the longest short path, finer than the 2° OVATION cells it is matched to.
pub const SAMPLES: usize = 65;

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
    /// No outlook sample covers the hour: the current Kp, assumed to persist.
    Current,
}

/// One UTC hour of path geometry.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathGeoHour {
    /// Start of the hour this entry describes (unix seconds UTC) — the NEXT occurrence of
    /// that UTC hour from the request, the current hour included.
    pub unix: i64,
    pub kp: f32,
    pub kp_source: KpSource,
    /// The polar-cap boundary at that Kp (geomagnetic latitude, degrees).
    pub cap_lat_deg: f32,
    /// Share of the path (0..1) poleward of the cap boundary.
    pub polar_frac: f32,
    /// Share of the path (0..1) on the grey line (sun within ±6° of the horizon).
    pub greyline_frac: f32,
    /// Share of the path (0..1) in sunlight.
    pub sunlit_frac: f32,
    /// Grey line at the operator's end / the far end.
    pub grey_me: bool,
    pub grey_dx: bool,
}

/// The short-path geometry between two stations over the next 24 hours.
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
}

/// Kp for the hour starting at `unix`: the outlook sample whose 3-hour period contains it,
/// else the current value.
fn kp_at(unix: i64, kp_now: f32, forecast: Option<&KpForecast>) -> (f32, KpSource) {
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
            (p.kp, src)
        })
        .unwrap_or((kp_now, KpSource::Current))
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
/// the hour containing `from_unix`. `kp_now` is the current planetary K; `forecast` the SWPC
/// outlook when held; `oval` the live OVATION oval as (lat, lon) cells on its 2° grid — every
/// cell the map's aurora layer draws.
pub fn path_geometry(
    me: (f64, f64),
    dx: (f64, f64),
    from_unix: i64,
    kp_now: f32,
    forecast: Option<&KpForecast>,
    oval: Option<&[(f32, f32)]>,
) -> PathGeometry {
    let pts: Vec<(f64, f64)> = (0..SAMPLES)
        .map(|i| geo::interpolate(me, dx, i as f64 / (SAMPLES - 1) as f64))
        .collect();
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

    let n = SAMPLES as f32;
    let hour0 = from_unix - from_unix.rem_euclid(3600);
    let now_hour = (from_unix.rem_euclid(86_400) / 3600) as usize;
    let hours = (0..24usize)
        .map(|h| {
            // The next occurrence of UTC hour h, the current hour included.
            let unix = hour0 + (((h + 24 - now_hour) % 24) as i64) * 3600;
            // Solar position at the middle of the hour.
            let mid = unix + 1800;
            let (kp, kp_source) = kp_at(unix, kp_now, forecast);
            let cap = cutoff_lat_deg(kp as f64);
            let polar = gmlat.iter().filter(|&&g| g >= cap).count();
            let elev: Vec<f64> = pts
                .iter()
                .map(|&(la, lo)| geo::solar_elevation_deg(la, lo, mid))
                .collect();
            let grey = |e: f64| e.abs() <= GREYLINE_DEG;
            PathGeoHour {
                unix,
                kp,
                kp_source,
                cap_lat_deg: cap as f32,
                polar_frac: polar as f32 / n,
                greyline_frac: elev.iter().filter(|&&e| grey(e)).count() as f32 / n,
                sunlit_frac: elev.iter().filter(|&&e| e > 0.0).count() as f32 / n,
                grey_me: grey(elev[0]),
                grey_dx: grey(elev[SAMPLES - 1]),
            }
        })
        .collect();

    PathGeometry {
        dist_km: geo::haversine_km(me, dx) as f32,
        bearing_deg: geo::bearing_deg(me, dx) as f32,
        max_gmlat_deg: max_gmlat as f32,
        now_hour: now_hour as u8,
        oval_now,
        hours,
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
        path_geometry(me, dx, MAY_12Z, kp, None, None)
    }

    #[test]
    fn boston_tokyo_crosses_the_polar_cap_at_quiet_kp() {
        let g = quiet(BOSTON, TOKYO, 2.0);
        // The short path to Japan runs up over Hudson Bay and Alaska.
        assert!(g.max_gmlat_deg > 66.0, "{}", g.max_gmlat_deg);
        let h = &g.hours[12];
        assert!(h.polar_frac > 0.1, "{}", h.polar_frac);
        assert_eq!(h.kp_source, KpSource::Current);
        // Bearing ~ north-north-west, ~10 800 km.
        assert!((330.0..360.0).contains(&g.bearing_deg), "{}", g.bearing_deg);
        assert!((10_500.0..11_100.0).contains(&g.dist_km), "{}", g.dist_km);
    }

    #[test]
    fn boston_london_stays_out_of_the_cap_at_low_kp_and_enters_in_a_storm() {
        // Negative control: Kp 1 puts the cap boundary near 65°.
        let low = quiet(BOSTON, LONDON, 1.0);
        assert!(low.hours.iter().all(|h| h.polar_frac == 0.0));
        assert!(
            low.max_gmlat_deg < cutoff_lat_deg(1.0) as f32,
            "{}",
            low.max_gmlat_deg
        );
        // The same path under a severe storm: the cap grows equatorward over it.
        let storm = quiet(BOSTON, LONDON, 8.0);
        assert!(
            storm.hours.iter().all(|h| h.polar_frac > 0.0),
            "{}",
            storm.max_gmlat_deg
        );
    }

    #[test]
    fn an_equatorial_path_never_touches_the_cap_even_at_kp_9() {
        // Singapore → Nairobi.
        let g = quiet((1.35, 103.82), (-1.29, 36.82), 9.0);
        assert!(g.hours.iter().all(|h| h.polar_frac == 0.0));
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
        let g = path_geometry(BOSTON, LONDON, MAY_12Z + 600, 2.0, Some(&f), None);
        assert_eq!(g.now_hour, 12);
        assert_eq!(
            (g.hours[12].kp, g.hours[12].kp_source),
            (1.0, KpSource::Measured)
        );
        assert_eq!(g.hours[12].polar_frac, 0.0);
        assert_eq!(
            (g.hours[16].kp, g.hours[16].kp_source),
            (2.0, KpSource::Current)
        );
        assert_eq!(
            (g.hours[19].kp, g.hours[19].kp_source),
            (8.0, KpSource::Forecast)
        );
        assert!(
            g.hours[19].polar_frac > 0.0,
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
        let g = path_geometry(LONDON, (5.6, -0.19), eq_06z, 2.0, None, None);
        let at = &g.hours[6];
        assert!(at.greyline_frac > 0.8, "{}", at.greyline_frac);
        assert!(at.grey_me && at.grey_dx);
        // Negative controls: the same path at noon, and an east–west path at the same instant.
        assert_eq!(g.hours[12].greyline_frac, 0.0);
        assert_eq!(g.hours[12].sunlit_frac, 1.0);
        let ew = path_geometry(LONDON, (55.75, 37.62), eq_06z, 2.0, None, None);
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
        let g = path_geometry(BOSTON, TOKYO, MAY_12Z, 2.0, None, Some(&ring));
        assert_eq!(g.oval_now, Some(true));
        // Negative controls: an empty oval is "no"; no oval held at all is "unknown", not "no";
        // and an oval wholly poleward of the transatlantic path is not crossed.
        let g = path_geometry(BOSTON, LONDON, MAY_12Z, 2.0, None, Some(&ring[..0]));
        assert_eq!(g.oval_now, Some(false));
        assert_eq!(quiet(BOSTON, LONDON, 2.0).oval_now, None);
        let high: Vec<(f32, f32)> = (-90..90).map(|i| (80.0, i as f32 * 2.0)).collect();
        let g = path_geometry(BOSTON, LONDON, MAY_12Z, 2.0, None, Some(&high));
        assert_eq!(g.oval_now, Some(false));
    }

    #[test]
    fn classifying_a_path_is_cheap() {
        // Budget: well under a millisecond per path even in a debug build (release is tens of
        // µs) — cheap enough to run beside every outlook request with no caching.
        let f = KpForecast::default();
        let ring: Vec<(f32, f32)> = (-90..90).map(|i| (66.0, i as f32 * 2.0)).collect();
        let runs = 200;
        let t0 = std::time::Instant::now();
        for i in 0..runs {
            std::hint::black_box(path_geometry(
                BOSTON,
                TOKYO,
                MAY_12Z + i * 60,
                3.0,
                Some(&f),
                Some(&ring),
            ));
        }
        let per = t0.elapsed() / runs as u32;
        let budget = if cfg!(debug_assertions) {
            std::time::Duration::from_millis(5)
        } else {
            std::time::Duration::from_micros(500)
        };
        assert!(per < budget, "{per:?} per path");
    }
}
