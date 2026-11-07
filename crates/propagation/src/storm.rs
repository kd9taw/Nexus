//! A storm correction for P.533's answers.
//!
//! P.533 is a monthly median and reads no Kp, so on its own a geomagnetic storm moves none of its
//! scores. This lays the heuristic's own storm penalty ([`crate::likelihood::PathModel`]'s auroral
//! factor: a mild global loss above Kp 2, and a harsh one above Kp 3 on a path that crosses high
//! geomagnetic latitude) over each hour of a P.533 answer, so the "now" answers (the current hour,
//! a chase window, an alarm time, Band Outlook's now) fall in a storm as they did before P.533 was
//! the default. Nothing new is modelled here: the factor is the heuristic's, to the number.
//!
//! **Which Kp each hour gets** ([`crate::pathgeo::kp_at`]): the SWPC outlook's measured or
//! forecast Kp for the 3-hour period holding the hour, and for the current hour SWPC's latest
//! reading when no period holds it. A later hour no period holds gets NO correction (not the
//! current Kp carried forward), and the answer says so ([`StormNote::hours_without_kp`]).
//!
//! **Quiet sky, no change.** At Kp 2 or below the factor is exactly 1, so the answer is the
//! engine's unchanged and carries no note. [`storm_key`] is empty then too, so the caches that
//! hold P.533's answers keep exactly the keys they had.
//!
//! Only answers that name P.533 are touched. The heuristic already reads Kp itself, and an answer
//! P.533 could not give ([`crate::predict::with_fallback`]) is the heuristic's.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::geo;
use crate::kpforecast::KpForecast;
use crate::likelihood::{mode_now_at, PathModel, Workability};
use crate::model::{Band, SpaceWx};
use crate::pathgeo::KpSource;
use crate::predict::{PathPrediction, PathPredictor};
use crate::route::Route;

/// What the correction did to an answer, for the screen to say. Absent when it changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StormNote {
    /// The highest Kp that lowered any hour.
    pub kp: f32,
    /// Some hour of the answer had no Kp, measured or forecast, and was left as P.533 gave it.
    pub hours_without_kp: bool,
}

impl StormNote {
    fn merge(a: Option<StormNote>, b: Option<StormNote>) -> Option<StormNote> {
        match (a, b) {
            (Some(a), Some(b)) => Some(StormNote {
                kp: a.kp.max(b.kp),
                hours_without_kp: a.hours_without_kp || b.hours_without_kp,
            }),
            (a, b) => a.or(b),
        }
    }

    /// Fold `other`'s note into `into` (a ring merges its directions' notes so).
    pub fn fold(into: &mut Option<StormNote>, other: Option<StormNote>) {
        *into = Self::merge(*into, other);
    }
}

/// The Kp that may correct the hour starting at `unix`: a measured or forecast period's, or, for
/// the current hour only, SWPC's latest reading. `None` for any other hour.
fn hour_kp(
    unix: i64,
    current: bool,
    kp_now: Option<f32>,
    forecast: Option<&KpForecast>,
) -> Option<f32> {
    let (kp, source) = crate::pathgeo::kp_at(unix, kp_now, forecast);
    match source {
        KpSource::Measured | KpSource::Forecast => kp,
        KpSource::Current if current => kp,
        _ => None,
    }
}

/// What the correction depends on beyond what the caches already key on: every Kp above 2 that
/// could reach an hour, from now on. Empty on a quiet sky, so a cache keeps its key.
pub fn storm_key(now: i64, kp_now: Option<f32>, forecast: Option<&KpForecast>) -> String {
    const PERIOD: i64 = 3 * 3600;
    let mut parts: Vec<String> = Vec::new();
    if let Some(kp) = kp_now.filter(|kp| *kp > 2.0) {
        parts.push(format!("n{kp:.2}"));
    }
    if let Some(f) = forecast {
        for p in f
            .points
            .iter()
            .filter(|p| p.time_unix + PERIOD > now && p.kp > 2.0)
        {
            parts.push(format!("{}:{:.2}", p.time_unix, p.kp));
        }
    }
    parts.join(",")
}

/// The highest geomagnetic latitude along `route` from `me` to `dx`, at nine points.
fn route_max_geomag(me: (f64, f64), dx: (f64, f64), route: Route) -> f64 {
    let point = |i: u32| -> (f64, f64) {
        let f = f64::from(i) / 8.0;
        match route {
            Route::Short => geo::interpolate(me, dx, f),
            Route::Long => {
                let long_km = 2.0 * std::f64::consts::PI * 6371.0 - geo::haversine_km(me, dx);
                geo::destination_point(me, (geo::bearing_deg(me, dx) + 180.0) % 360.0, long_km * f)
            }
        }
    };
    (0..=8)
        .map(|i| {
            let (lat, lon) = point(i);
            geo::geomagnetic_lat_deg(lat, lon).abs()
        })
        .fold(0.0, f64::max)
}

/// `inner` with the storm correction laid over every P.533 answer it gives, for the Kp held at
/// `now`. Build one per request, as `make_predictor` is: the Kp it holds is that request's.
pub fn with_storm_correction(
    inner: Box<dyn PathPredictor>,
    me: Option<(f64, f64)>,
    now: i64,
    kp_now: Option<f32>,
    forecast: Option<KpForecast>,
) -> Box<dyn PathPredictor> {
    Box::new(StormCorrected {
        inner,
        me,
        now,
        kp_now,
        forecast,
        model: PathModel::new(me),
        note: Mutex::new(None),
    })
}

struct StormCorrected {
    inner: Box<dyn PathPredictor>,
    me: Option<(f64, f64)>,
    now: i64,
    kp_now: Option<f32>,
    forecast: Option<KpForecast>,
    model: PathModel,
    note: Mutex<Option<StormNote>>,
}

impl StormCorrected {
    fn correct(&self, p: &mut PathPrediction, dx: (f64, f64), route: Route, from_unix: i64) {
        let Some(me) = self.me else {
            return;
        };
        if p.engine != "p533" {
            return;
        }
        let max_geo = route_max_geomag(me, dx, route);
        let day0 = from_unix - from_unix.rem_euclid(86_400);
        let mut changed_kp: Option<f32> = None;
        let mut without_kp = false;
        for h in 0..24usize {
            // The hour's NEXT occurrence, as the day strip reads it: a past hour of today is
            // tomorrow's.
            let mut unix = day0 + h as i64 * 3600;
            if unix + 3600 <= self.now {
                unix += 86_400;
            }
            let current = unix <= self.now && self.now < unix + 3600;
            let Some(kp) = hour_kp(unix, current, self.kp_now, self.forecast.as_ref()) else {
                without_kp = true;
                continue;
            };
            for b in &mut p.bands {
                let Some(band) = Band::from_label(&b.band) else {
                    continue;
                };
                let factor = self.model.storm_factor(band, f64::from(kp), max_geo) as f32;
                if factor >= 1.0 {
                    continue;
                }
                if let Some(s) = b.hourly.get_mut(h) {
                    *s *= factor;
                }
                for m in &mut b.mode_hourly {
                    if let Some(s) = m.hourly.get_mut(h) {
                        *s *= factor;
                    }
                }
                changed_kp = Some(changed_kp.map_or(kp, |k: f32| k.max(kp)));
            }
        }
        let Some(kp) = changed_kp else {
            return;
        };
        let now_hour = (from_unix.rem_euclid(86_400) / 3600) as usize;
        let usable = (crate::p533::engine::P533Config::default().bcr_usable_pct / 100.0) as f32;
        for b in &mut p.bands {
            let Some(band) = Band::from_label(&b.band) else {
                continue;
            };
            let Ok(hourly) = <[f32; 24]>::try_from(b.hourly.as_slice()) else {
                continue;
            };
            let (window, grayline, peak, _) = crate::p533::engine::best_window(&hourly, day0, band);
            b.window = window;
            b.grayline = grayline;
            b.score = peak;
            b.workability = Workability::from_score(peak).label().to_string();
            b.reliability = hourly.iter().filter(|&&s| s >= usable).count() as f32 / 24.0 * 100.0;
            if !b.mode_hourly.is_empty() {
                b.mode_now = mode_now_at(&b.mode_hourly, now_hour);
            }
        }
        p.bands.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let note = Some(StormNote {
            kp,
            hours_without_kp: without_kp,
        });
        p.storm = note;
        let mut held = self.note.lock().unwrap_or_else(|e| e.into_inner());
        StormNote::fold(&mut held, note);
    }
}

impl PathPredictor for StormCorrected {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn predict(&self, dx: (f64, f64), from_unix: i64, wx: &SpaceWx) -> PathPrediction {
        let mut p = self.inner.predict(dx, from_unix, wx);
        self.correct(&mut p, dx, Route::Short, from_unix);
        p
    }

    fn predict_route(
        &self,
        dx: (f64, f64),
        route: Route,
        from_unix: i64,
        wx: &SpaceWx,
    ) -> Option<PathPrediction> {
        let mut p = self.inner.predict_route(dx, route, from_unix, wx)?;
        self.correct(&mut p, dx, route, from_unix);
        Some(p)
    }

    fn fell_back(&self) -> bool {
        self.inner.fell_back()
    }

    fn storm_note(&self) -> Option<StormNote> {
        *self.note.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kpforecast::{KpKind, KpPoint};
    use crate::predict::make_predictor;

    const ME: (f64, f64) = (41.7, -87.9);
    const EUROPE: (f64, f64) = (51.5, -0.1);
    const BRAZIL: (f64, f64) = (-23.5, -46.6);
    /// 2026-11-12 18:00 UTC.
    const T: i64 = 1_794_009_600 + 18 * 3600;
    const H: usize = 18;

    fn p533() -> Box<dyn PathPredictor> {
        make_predictor("p533", Some(ME), Some(100.0), 0.0)
    }

    fn corrected(kp_now: Option<f32>, forecast: Option<KpForecast>) -> Box<dyn PathPredictor> {
        with_storm_correction(p533(), Some(ME), T, kp_now, forecast)
    }

    fn band<'a>(p: &'a PathPrediction, label: &str) -> &'a crate::likelihood::BandOutlook {
        p.bands.iter().find(|b| b.band == label).unwrap()
    }

    fn forecast(points: &[(i64, f32, KpKind)]) -> KpForecast {
        KpForecast {
            points: points
                .iter()
                .map(|&(time_unix, kp, kind)| KpPoint {
                    time_unix,
                    kp,
                    kind,
                    noaa_scale: None,
                })
                .collect(),
            ..KpForecast::default()
        }
    }

    #[test]
    fn a_quiet_sky_changes_nothing_and_says_nothing() {
        let wx = SpaceWx::default();
        let raw = p533().predict(EUROPE, T, &wx);
        for kp_now in [None, Some(0.0), Some(2.0)] {
            let quiet = forecast(&[
                (T - 3600, 2.0, KpKind::Observed),
                (T + 7200, 1.7, KpKind::Predicted),
            ]);
            for f in [None, Some(quiet)] {
                let eng = corrected(kp_now, f);
                let p = eng.predict(EUROPE, T, &wx);
                assert!(p.storm.is_none(), "{kp_now:?}");
                assert!(eng.storm_note().is_none());
                for b in &raw.bands {
                    assert_eq!(band(&p, &b.band).hourly, b.hourly, "{kp_now:?} {}", b.band);
                    assert_eq!(band(&p, &b.band).window, b.window);
                }
            }
        }
        assert_eq!(storm_key(T, Some(2.0), None), "");
    }

    /// The Kp 7 example: Chicago to London at 18Z, where P.533 alone has 20 m wide open.
    #[test]
    fn kp_7_now_closes_the_current_hour_on_a_polar_path() {
        let wx = SpaceWx::default();
        let raw = p533().predict(EUROPE, T, &wx);
        let p = corrected(Some(7.0), None).predict(EUROPE, T, &wx);
        let (before, after) = (band(&raw, "20m").hourly[H], band(&p, "20m").hourly[H]);
        assert!(before > 0.7, "P.533 alone: {before}");
        assert!(after < 0.1, "corrected: {after}");
        assert_eq!(
            p.storm,
            Some(StormNote {
                kp: 7.0,
                hours_without_kp: true
            })
        );
        // With no forecast only the current hour is corrected; the rest is P.533's.
        for h in (0..24).filter(|h| *h != H) {
            assert_eq!(
                band(&p, "20m").hourly[h],
                band(&raw, "20m").hourly[h],
                "hour {h}"
            );
        }
    }

    /// The heuristic's own factor, to the number, not a new one.
    #[test]
    fn the_factor_is_the_heuristics() {
        let wx = SpaceWx::default();
        let raw = p533().predict(BRAZIL, T, &wx);
        let p = corrected(Some(5.0), None).predict(BRAZIL, T, &wx);
        let model = PathModel::new(Some(ME));
        let storm = SpaceWx {
            kp: 5.0,
            ..SpaceWx::default()
        };
        let want = model.aurora_factor(ME, BRAZIL, Band::B20, &storm) as f32;
        let got = band(&p, "20m").hourly[H] / band(&raw, "20m").hourly[H];
        assert!((got - want).abs() < 1e-4, "{got} vs {want}");
        assert!(
            want < 1.0 && want > 0.8,
            "a low-latitude path loses only the global share: {want}"
        );
    }

    /// A forecast storm corrects the hours it covers, and no others.
    #[test]
    fn forecast_hours_take_the_forecast_and_hours_without_one_are_left() {
        let wx = SpaceWx::default();
        let raw = p533().predict(EUROPE, T, &wx);
        let start = T + 3 * 3600; // 21Z..24Z
        let p = corrected(None, Some(forecast(&[(start, 7.0, KpKind::Predicted)])))
            .predict(EUROPE, T, &wx);
        let b = band(&p, "20m");
        let r = band(&raw, "20m");
        for h in 21..24 {
            assert!(
                b.hourly[h] < r.hourly[h] || r.hourly[h] == 0.0,
                "hour {h} corrected"
            );
        }
        for h in (0..24).filter(|h| !(21..24).contains(h)) {
            assert_eq!(b.hourly[h], r.hourly[h], "hour {h} left");
        }
        assert_eq!(p.storm.map(|n| n.hours_without_kp), Some(true));
    }

    /// The day's peak, window and word follow the corrected hours.
    #[test]
    fn a_corrected_band_is_rebuilt_from_its_hours() {
        let wx = SpaceWx::default();
        let all_day: Vec<(i64, f32, KpKind)> = (-1..8)
            .map(|i| {
                (
                    T - T.rem_euclid(10_800) + i * 10_800,
                    7.0,
                    KpKind::Predicted,
                )
            })
            .collect();
        let p = corrected(Some(7.0), Some(forecast(&all_day))).predict(EUROPE, T, &wx);
        for b in &p.bands {
            let peak = b.hourly.iter().copied().fold(0.0f32, f32::max);
            assert!((b.score - peak).abs() < 1e-6, "{}", b.band);
            assert_eq!(b.workability, Workability::from_score(peak).label());
        }
        assert!(band(&p, "20m").score < 0.3, "the storm closes 20m all day");
        assert!(!p.storm.unwrap().hours_without_kp);
    }

    #[test]
    fn the_heuristics_answers_are_left_to_its_own_kp() {
        let wx = SpaceWx::default();
        let h = make_predictor("heuristic", Some(ME), None, 0.0);
        let raw = h.predict(EUROPE, T, &wx);
        let p = with_storm_correction(
            make_predictor("heuristic", Some(ME), None, 0.0),
            Some(ME),
            T,
            Some(7.0),
            None,
        )
        .predict(EUROPE, T, &wx);
        assert!(p.storm.is_none());
        assert_eq!(band(&p, "20m").hourly, band(&raw, "20m").hourly);
    }

    #[test]
    fn the_long_route_is_corrected_along_its_own_path() {
        let wx = SpaceWx::default();
        let p = corrected(Some(7.0), None)
            .predict_route(EUROPE, Route::Long, T, &wx)
            .unwrap();
        assert_eq!(p.storm.map(|n| n.kp), Some(7.0));
    }

    #[test]
    fn the_key_names_every_storm_kp_and_nothing_quiet() {
        let f = forecast(&[
            (T - 4 * 3600, 6.0, KpKind::Observed),
            (T, 5.0, KpKind::Predicted),
            (T + 3 * 3600, 2.0, KpKind::Predicted),
        ]);
        assert_eq!(storm_key(T, Some(5.0), Some(&f)), format!("n5.00,{T}:5.00"));
        assert_ne!(
            storm_key(T, Some(5.0), Some(&f)),
            storm_key(T, Some(6.0), Some(&f))
        );
    }
}
