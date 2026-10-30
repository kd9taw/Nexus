//! Your contacts — which way round a LOGGED contact's signal most likely went, and how sure.
//!
//! [`crate::route`] answers "which route does the model favour" for a band and an hour. A logged
//! contact adds evidence the model never has: the station's own beam heading at the time, other
//! receivers along one route, the station's own decodes. This module grades one contact from all
//! of it, strongest first, and says which evidence each part of the answer stands on:
//!
//! 1. **The station's beam heading** ([`confirm_by_heading`]). Within ±30° of one route's bearing,
//!    and the other route modelled at least 6 dB worse: that route is *confirmed* — the only way a
//!    grade reaches *confirmed*, because it is the station's own evidence, not the model's.
//!    Only from a connected rotator (which turns a beam); an omni or a station with no rotator
//!    never gets a heading, so never *confirmed*. **The long path is never confirmed**: a
//!    long-path verdict is held at *possible* until the long path is validated, and the heading
//!    cannot lift it — it is said as a reason instead.
//! 2. **The model's margin** ([`crate::route::infer_route`], the same call the path card and the
//!    day strip make for that band and hour).
//! 3. **Grey line** on 160/80/40 m — inside `infer_route`, one notch, never past *likely*.
//! 4. **Receivers along the route** ([`receivers_along`]): other receivers between the two ends,
//!    on one route only, hearing the same signal within the quarter hour. It says where the
//!    signal was heard, not how it reached you, so it is a reason and never raises a confidence.
//! 5. **FT8 DT** ([`late_dt_ms`]). ⚠️ **A single decode's DT is NOT evidence of the long path,
//!    and must never be made into it.** The long path's extra delay is at most ~67 ms (20 000 km
//!    at the speed of light), and both stations' clock errors are routinely larger than that, so
//!    one DT says nothing about the route. Only an aggregate is admitted — a station whose DT sits
//!    at least 50 ms above each period's median over at least ten decodes — and even then it is a
//!    reason string, never a confidence. Do not "improve" this into a per-decode test.
//!
//! The model is a monthly median at a sunspot number. For a past contact that number is the one
//! the contact itself logged (its `SFI`), or for a recent contact today's ([`ssn_for_contact`]);
//! a contact older than that with no flux logged is not graded at all rather than graded against
//! the wrong part of the solar cycle.
//!
//! [`ValidationTally`] counts how often the model's open hours and the station's own headings
//! agree with the contacts actually made — the expert's check on all of the above.

use serde::{Deserialize, Serialize};

use crate::geo;
use crate::model::Band;
use crate::p533::engine::{ssn_from_sfi, P533Engine};
use crate::pathgeo::path_geometry_route;
use crate::route::{
    infer_route, Route, RouteCall, RouteConfidence, RouteEvidence, RouteReason, RouteVerdict,
    CLOSED_BCR, MARGIN_DB, OPEN_BCR,
};

/// A beam heading within this many degrees of a route's bearing points along that route.
pub const HEADING_TOLERANCE_DEG: f32 = 30.0;
/// A route whose highest geomagnetic latitude reaches this crosses the polar cap — geometry
/// alone, the same at every Kp (the auroral zone below it moves with Kp; the cap does not).
pub const POLAR_CAP_GMLAT_DEG: f32 = 70.0;
/// Today's sunspot number stands in for a contact made at most this long ago with no flux
/// logged: the smoothed number moves a few units in three months.
pub const RECENT_SSN_SECS: i64 = 90 * 86_400;
/// A receiver within this distance of either end is at that end, not along the route.
pub const ALONG_END_KM: f64 = 1_000.0;
/// A receiver within this distance of a route's great circle lies along it.
pub const ALONG_TRACK_KM: f64 = 1_000.0;
/// [`late_dt_ms`]'s gate: this many decodes at least …
pub const LATE_DT_MIN_DECODES: usize = 10;
/// … whose median lateness against their periods' median is at least this (seconds).
pub const LATE_DT_MIN_SECS: f32 = 0.050;

const EARTH_KM: f64 = 6371.0;

/// The smaller angle between two bearings, degrees (0..=180).
fn bearing_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// The route a beam heading points along: within [`HEADING_TOLERANCE_DEG`] of that route's
/// bearing. The two bearings are opposite, so at most one matches.
pub fn heading_route(heading_deg: f32, short_bearing_deg: f32) -> Option<Route> {
    let long_bearing = (short_bearing_deg + 180.0).rem_euclid(360.0);
    if bearing_gap(heading_deg, short_bearing_deg) <= HEADING_TOLERANCE_DEG {
        Some(Route::Short)
    } else if bearing_gap(heading_deg, long_bearing) <= HEADING_TOLERANCE_DEG {
        Some(Route::Long)
    } else {
        None
    }
}

/// Evidence 1: the station's beam heading at the time of the contact. A heading along the short
/// path, with the long path modelled at least [`MARGIN_DB`] worse, makes the call *confirmed*
/// short. A heading along the long path is said (a [`RouteReason::Heading`]) and lifts nothing:
/// the long path is held at *possible* until it is validated. A heading along neither route adds
/// nothing — it may be a skewed path, which the model does not know.
pub fn confirm_by_heading(call: &mut RouteCall, heading_deg: f32, short_bearing_deg: f32) {
    let Some(route) = heading_route(heading_deg, short_bearing_deg) else {
        return;
    };
    call.reasons.push(RouteReason::Heading { route });
    let long_worse = call.margin_db.is_some_and(|m| m <= -MARGIN_DB);
    if route == Route::Short && long_worse {
        call.verdict = RouteVerdict::Short;
        call.confidence = Some(RouteConfidence::Confirmed);
    }
}

/// Where `p` lies against the great circle from `a` toward `b`: its cross-track distance (km,
/// either side) and whether its foot falls on the short arc between them.
fn against_arc(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> (f64, bool) {
    let d13 = geo::haversine_km(a, p) / EARTH_KM;
    let t13 = geo::bearing_deg(a, p).to_radians();
    let t12 = geo::bearing_deg(a, b).to_radians();
    let xt = (d13.sin() * (t13 - t12).sin()).clamp(-1.0, 1.0).asin();
    let along = (d13.cos() / xt.cos()).clamp(-1.0, 1.0).acos() * EARTH_KM;
    let forward = (t13 - t12).cos() > 0.0;
    let on_short = forward && along <= geo::haversine_km(a, b);
    (xt.abs() * EARTH_KM, on_short)
}

/// Evidence 4: how many of `receivers` (lat, lon) lie along each route between `me` and `dx` —
/// `[short, long]`. A receiver within [`ALONG_END_KM`] of either end, or further than
/// [`ALONG_TRACK_KM`] from the great circle, counts for neither. Where a signal was heard is not
/// how it reached the operator, so this only ever becomes a reason ([`heard_along_reason`]).
pub fn receivers_along(me: (f64, f64), dx: (f64, f64), receivers: &[(f64, f64)]) -> [u16; 2] {
    let mut n = [0u16; 2];
    for &p in receivers {
        if geo::haversine_km(me, p) < ALONG_END_KM || geo::haversine_km(dx, p) < ALONG_END_KM {
            continue;
        }
        let (xt, on_short) = against_arc(me, dx, p);
        if xt > ALONG_TRACK_KM {
            continue;
        }
        let i = if on_short { 0 } else { 1 };
        n[i] = n[i].saturating_add(1);
    }
    n
}

/// [`receivers_along`]'s count as a reason: only when one route has receivers and the other none.
pub fn heard_along_reason(counts: [u16; 2]) -> Option<RouteReason> {
    match counts {
        [s, 0] if s > 0 => Some(RouteReason::HeardAlong {
            route: Route::Short,
            receivers: s,
        }),
        [0, l] if l > 0 => Some(RouteReason::HeardAlong {
            route: Route::Long,
            receivers: l,
        }),
        _ => None,
    }
}

/// Evidence 5, the only admissible form of it (see the module header): `late_secs` holds, for
/// each of a station's decodes, its DT minus the median DT of every decode in the same period.
/// `Some((ms, decodes))` when there are at least [`LATE_DT_MIN_DECODES`] of them and their median
/// is at least [`LATE_DT_MIN_SECS`] late. Never evidence on its own: a reason, nothing more.
pub fn late_dt_ms(late_secs: &[f32]) -> Option<(u16, u16)> {
    if late_secs.len() < LATE_DT_MIN_DECODES {
        return None;
    }
    let mut v: Vec<f32> = late_secs
        .iter()
        .copied()
        .filter(|x| x.is_finite())
        .collect();
    if v.len() < LATE_DT_MIN_DECODES {
        return None;
    }
    v.sort_by(f32::total_cmp);
    let mid = v.len() / 2;
    let median = if v.len().is_multiple_of(2) {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    };
    (median >= LATE_DT_MIN_SECS).then(|| {
        (
            (median * 1000.0).round().min(u16::MAX as f32) as u16,
            v.len().min(u16::MAX as usize) as u16,
        )
    })
}

/// Where a contact's sunspot number came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SsnSource {
    /// From the solar flux logged with the contact.
    Logged,
    /// Today's, for a contact made within [`RECENT_SSN_SECS`] with no flux logged.
    Recent,
}

/// The sunspot number to model a contact made at `when_unix` with: from the flux it logged, else
/// today's (`current_ssn`) if it was made recently, else none — a contact from another part of
/// the solar cycle is not modelled against this one.
pub fn ssn_for_contact(
    logged_sfi: Option<f64>,
    when_unix: i64,
    now_unix: i64,
    current_ssn: Option<f64>,
) -> Option<(f64, SsnSource)> {
    if let Some(sfi) = logged_sfi.filter(|s| s.is_finite() && *s > 0.0) {
        return Some((ssn_from_sfi(sfi), SsnSource::Logged));
    }
    let recent = (now_unix - when_unix).abs() <= RECENT_SSN_SECS;
    current_ssn
        .filter(|s| s.is_finite() && recent)
        .map(|s| (s, SsnSource::Recent))
}

/// One contact's grade.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactGrade {
    /// The route call at the contact's hour, with every reason behind it.
    pub call: RouteCall,
    /// Bearing of the short path from the operator (degrees true); the long path is opposite.
    pub short_bearing_deg: f32,
    /// Whether each route crosses the polar cap ([`POLAR_CAP_GMLAT_DEG`]).
    pub polar_short: bool,
    pub polar_long: bool,
}

impl ContactGrade {
    /// Whether the route the grade names crosses the polar cap; with no route named (either way,
    /// or both closed), whether the short path does.
    pub fn polar(&self) -> bool {
        match self.call.verdict {
            RouteVerdict::Long => self.polar_long,
            _ => self.polar_short,
        }
    }
}

/// Grade a contact on `band` made at `at_unix` between `me` and `dx`, modelled at `ssn`: both
/// routes' figures for that band and hour (one chain run each), the grey line along each arc,
/// and the model's call ([`infer_route`]). `None` for a band the model does not cover and for
/// two stations too close together to have a long path. Heading, receivers and DT are added by
/// the caller, who holds that evidence.
pub fn grade_contact(
    engine: &P533Engine,
    me: (f64, f64),
    dx: (f64, f64),
    band: &str,
    at_unix: i64,
    ssn: f64,
) -> Option<ContactGrade> {
    let b = Band::from_label(band)?;
    let sp = engine.band_hour(dx, Route::Short, b, at_unix, ssn)?;
    let lp = engine.band_hour(dx, Route::Long, b, at_unix, ssn)?;
    let h = (at_unix.rem_euclid(86_400) / 3600) as usize;
    let gs = path_geometry_route(me, dx, Route::Short, at_unix, None, None, None);
    let gl = path_geometry_route(me, dx, Route::Long, at_unix, None, None, None);
    let grey = |g: &crate::pathgeo::PathGeometry| g.hours.get(h).map(|x| x.greyline_frac);
    let call = infer_route(
        b.label(),
        RouteEvidence {
            bcr: sp.bcr,
            snr_db: sp.snr_db,
            greyline_frac: grey(&gs),
        },
        RouteEvidence {
            bcr: lp.bcr,
            snr_db: lp.snr_db,
            greyline_frac: grey(&gl),
        },
    );
    Some(ContactGrade {
        call,
        short_bearing_deg: gs.bearing_deg,
        polar_short: gs.max_gmlat_deg >= POLAR_CAP_GMLAT_DEG,
        polar_long: gl.max_gmlat_deg >= POLAR_CAP_GMLAT_DEG,
    })
}

/// One share of graded contacts: how many, how many fell in an hour the model calls open on at
/// least one route, and how many in an hour it calls closed on both ("outside the model").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub graded: u32,
    pub open: u32,
    pub outside: u32,
}

impl Coverage {
    fn add(&mut self, call: &RouteCall) {
        self.graded += 1;
        if call.short_bcr.max(call.long_bcr) >= OPEN_BCR {
            self.open += 1;
        }
        if call.short_bcr < CLOSED_BCR && call.long_bcr < CLOSED_BCR {
            self.outside += 1;
        }
    }
}

/// The success measures for the graded contacts, for the expert: (1) model-vs-observed coverage,
/// overall, by band and polar against non-polar (a polar deficit is the aurora's signature); and
/// (2) where the station logged a beam heading along one route and the model separates the
/// routes by at least [`MARGIN_DB`], how often the model's better route is the one the beam
/// pointed along.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationTally {
    pub all: Coverage,
    pub polar: Coverage,
    pub non_polar: Coverage,
    /// Band label → its coverage, in band order.
    pub by_band: Vec<(String, Coverage)>,
    /// Contacts whose heading and the model's margin both name a route …
    pub heading_judged: u32,
    /// … and of those, how many name the same one.
    pub heading_agree: u32,
}

impl ValidationTally {
    /// Count one graded contact; `heading_deg` is the beam heading it logged, if any.
    pub fn add(&mut self, grade: &ContactGrade, heading_deg: Option<f32>) {
        let call = &grade.call;
        self.all.add(call);
        if grade.polar() {
            self.polar.add(call);
        } else {
            self.non_polar.add(call);
        }
        match self.by_band.iter_mut().find(|(b, _)| *b == call.band) {
            Some((_, c)) => c.add(call),
            None => {
                let mut c = Coverage::default();
                c.add(call);
                self.by_band.push((call.band.clone(), c));
                let rank = |b: &str| Band::from_label(b).map_or(usize::MAX, |b| b as usize);
                self.by_band.sort_by_key(|(b, _)| rank(b));
            }
        }
        let beam = heading_deg.and_then(|h| heading_route(h, grade.short_bearing_deg));
        let model = call.margin_db.and_then(|m| {
            if m >= MARGIN_DB {
                Some(Route::Long)
            } else if m <= -MARGIN_DB {
                Some(Route::Short)
            } else {
                None
            }
        });
        if let (Some(beam), Some(model)) = (beam, model) {
            self.heading_judged += 1;
            if beam == model {
                self.heading_agree += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::compare_routes_at;
    use crate::PathPredictor;

    fn call(
        margin: Option<f32>,
        verdict: RouteVerdict,
        conf: Option<RouteConfidence>,
    ) -> RouteCall {
        RouteCall {
            band: "20m".into(),
            verdict,
            confidence: conf,
            margin_db: margin,
            short_bcr: 0.6,
            long_bcr: 0.2,
            reasons: Vec::new(),
        }
    }

    const BOSTON: (f64, f64) = (42.36, -71.06);
    const TOKYO: (f64, f64) = (35.68, 139.69);
    const SYDNEY: (f64, f64) = (-33.87, 151.21);
    const SINGAPORE: (f64, f64) = (1.35, 103.82);
    const NAIROBI: (f64, f64) = (-1.29, 36.82);

    #[test]
    fn a_heading_points_along_the_route_within_30_degrees_and_never_both() {
        assert_eq!(heading_route(330.0, 312.0), Some(Route::Short));
        assert_eq!(heading_route(290.0, 312.0), Some(Route::Short));
        assert_eq!(heading_route(140.0, 312.0), Some(Route::Long));
        // Across north: 350° and 10° are 20° apart.
        assert_eq!(heading_route(350.0, 10.0), Some(Route::Short));
        // Square to both routes: neither (a skewed path, if anything).
        assert_eq!(heading_route(42.0, 312.0), None);
        assert_eq!(heading_route(312.0 + 31.0, 312.0), None);
    }

    #[test]
    fn a_short_path_heading_with_the_long_path_6_db_worse_is_confirmed() {
        let mut c = call(
            Some(-7.0),
            RouteVerdict::Short,
            Some(RouteConfidence::Likely),
        );
        confirm_by_heading(&mut c, 300.0, 312.0);
        assert_eq!(c.verdict, RouteVerdict::Short);
        assert_eq!(c.confidence, Some(RouteConfidence::Confirmed));
        assert_eq!(
            c.reasons,
            vec![RouteReason::Heading {
                route: Route::Short
            }]
        );
        // Also from "either" (inside 6 dB on BCR, but the SNR margin separates them).
        let mut c = call(Some(-6.0), RouteVerdict::Either, None);
        confirm_by_heading(&mut c, 312.0, 312.0);
        assert_eq!(
            (c.verdict, c.confidence),
            (RouteVerdict::Short, Some(RouteConfidence::Confirmed))
        );
    }

    #[test]
    fn a_heading_alone_or_inside_the_margin_confirms_nothing() {
        // The beam on the short path but the long path only 5 dB worse: said, not confirmed.
        let mut c = call(
            Some(-5.0),
            RouteVerdict::Short,
            Some(RouteConfidence::Possible),
        );
        confirm_by_heading(&mut c, 312.0, 312.0);
        assert_eq!(c.confidence, Some(RouteConfidence::Possible));
        assert_eq!(
            c.reasons,
            vec![RouteReason::Heading {
                route: Route::Short
            }]
        );
        // No SNR from the model: nothing to measure the margin with.
        let mut c = call(None, RouteVerdict::Short, Some(RouteConfidence::Likely));
        confirm_by_heading(&mut c, 312.0, 312.0);
        assert_eq!(c.confidence, Some(RouteConfidence::Likely));
        // The beam on the short path while the model favours the long path: the model's call
        // stands, and the disagreement is said.
        let mut c = call(
            Some(9.0),
            RouteVerdict::Long,
            Some(RouteConfidence::Possible),
        );
        confirm_by_heading(&mut c, 312.0, 312.0);
        assert_eq!(
            (c.verdict, c.confidence),
            (RouteVerdict::Long, Some(RouteConfidence::Possible))
        );
        // The beam along neither route: no reason at all.
        let mut c = call(
            Some(-9.0),
            RouteVerdict::Short,
            Some(RouteConfidence::Likely),
        );
        confirm_by_heading(&mut c, 42.0, 312.0);
        assert_eq!(c.confidence, Some(RouteConfidence::Likely));
        assert!(c.reasons.is_empty());
    }

    #[test]
    fn a_long_path_heading_is_never_confirmed() {
        // The strongest long-path case there is: beam on the long path, long 12 dB better.
        let mut c = call(
            Some(12.0),
            RouteVerdict::Long,
            Some(RouteConfidence::Possible),
        );
        confirm_by_heading(&mut c, 132.0, 312.0);
        assert_eq!(
            (c.verdict, c.confidence),
            (RouteVerdict::Long, Some(RouteConfidence::Possible))
        );
        assert_eq!(c.reasons, vec![RouteReason::Heading { route: Route::Long }]);
    }

    #[test]
    fn receivers_count_for_the_route_they_lie_along_and_never_at_an_end() {
        let sp_brg = geo::bearing_deg(BOSTON, SYDNEY);
        let d = geo::haversine_km(BOSTON, SYDNEY);
        let on_short = geo::destination_point(BOSTON, sp_brg, d / 2.0);
        let on_long = geo::destination_point(BOSTON, sp_brg + 180.0, (40_030.0 - d) / 2.0);
        // 3000 km off the short arc's middle: along neither.
        let off = geo::destination_point(on_short, sp_brg + 90.0, 3000.0);
        // 300 km from Boston: at an end.
        let at_end = geo::destination_point(BOSTON, sp_brg, 300.0);
        assert_eq!(receivers_along(BOSTON, SYDNEY, &[on_short]), [1, 0]);
        assert_eq!(receivers_along(BOSTON, SYDNEY, &[on_long, on_long]), [0, 2]);
        assert_eq!(receivers_along(BOSTON, SYDNEY, &[off, at_end]), [0, 0]);
        assert_eq!(
            heard_along_reason(receivers_along(BOSTON, SYDNEY, &[on_long])),
            Some(RouteReason::HeardAlong {
                route: Route::Long,
                receivers: 1
            })
        );
        // Both routes heard: no reason.
        assert_eq!(heard_along_reason([2, 1]), None);
        assert_eq!(heard_along_reason([0, 0]), None);
    }

    #[test]
    fn late_dt_needs_ten_decodes_and_50_ms() {
        assert_eq!(late_dt_ms(&[0.06; 9]), None);
        assert_eq!(late_dt_ms(&[0.06; 10]), Some((60, 10)));
        assert_eq!(late_dt_ms(&[0.04; 30]), None);
        // The median, not the mean: one wild decode does not make a station late.
        let mut v = vec![0.0; 11];
        v[0] = 5.0;
        assert_eq!(late_dt_ms(&v), None);
    }

    #[test]
    fn the_sunspot_number_is_the_contacts_own_else_todays_only_if_recent() {
        let now = 1_800_000_000;
        let (ssn, src) =
            ssn_for_contact(Some(150.0), now - 10 * 365 * 86_400, now, Some(20.0)).unwrap();
        assert_eq!(src, SsnSource::Logged);
        assert!((ssn - ssn_from_sfi(150.0)).abs() < 1e-9);
        assert_eq!(
            ssn_for_contact(None, now - 86_400, now, Some(80.0)),
            Some((80.0, SsnSource::Recent))
        );
        // Older than three months with no flux logged: not modelled.
        assert_eq!(
            ssn_for_contact(None, now - RECENT_SSN_SECS - 1, now, Some(80.0)),
            None
        );
        // Recent but nothing held now: not modelled either.
        assert_eq!(ssn_for_contact(None, now - 60, now, None), None);
    }

    #[test]
    fn a_contacts_grade_is_the_call_the_day_strip_makes_for_that_band_and_hour() {
        // Boston–Sydney on 20 m, May 2026, every hour: the single-run grade equals the card's
        // comparison of the two whole-day predictions at the same hour.
        let eng = P533Engine::new(Some(BOSTON));
        let day0 = 1_778_112_000; // 2026-05-07 00:00Z
        let ssn = 105.0;
        let wx = crate::model::SpaceWx {
            ssn: Some(ssn as f32),
            ..Default::default()
        };
        let mut sp = eng.predict(SYDNEY, day0, &wx);
        let mut lp = eng.predict_route(SYDNEY, Route::Long, day0, &wx).unwrap();
        sp.geometry = Some(path_geometry_route(
            BOSTON,
            SYDNEY,
            Route::Short,
            day0,
            None,
            None,
            None,
        ));
        lp.geometry = Some(path_geometry_route(
            BOSTON,
            SYDNEY,
            Route::Long,
            day0,
            None,
            None,
            None,
        ));
        let mut long_hours = 0;
        for h in 0..24usize {
            let at = day0 + h as i64 * 3600 + 1200;
            let g = grade_contact(&eng, BOSTON, SYDNEY, "20m", at, ssn).unwrap();
            let want = compare_routes_at(&sp, &lp, h)
                .into_iter()
                .find(|c| c.band == "20m")
                .unwrap();
            assert_eq!(g.call, want, "hour {h}");
            long_hours += usize::from(g.call.verdict == RouteVerdict::Long);
        }
        // Positive control: the long path is named somewhere in the day on this pair.
        assert!(long_hours > 0);
    }

    #[test]
    fn a_polar_route_is_flagged_and_an_equatorial_one_is_not() {
        let eng = P533Engine::new(Some(BOSTON));
        let g = grade_contact(&eng, BOSTON, TOKYO, "20m", 1_778_150_000, 100.0).unwrap();
        assert!(g.polar_short);
        let eng = P533Engine::new(Some(SINGAPORE));
        let g = grade_contact(&eng, SINGAPORE, NAIROBI, "20m", 1_778_150_000, 100.0).unwrap();
        assert!(!g.polar_short && !g.polar_long);
        assert!(!g.polar());
        // No long path between co-located stations, and no grade on a band the model skips.
        let eng = P533Engine::new(Some(BOSTON));
        assert!(grade_contact(&eng, BOSTON, BOSTON, "20m", 1_778_150_000, 100.0).is_none());
        assert!(grade_contact(&eng, BOSTON, TOKYO, "2m", 1_778_150_000, 100.0).is_none());
    }

    #[test]
    fn the_tally_counts_coverage_and_heading_agreement() {
        let grade = |sb: f32, lb: f32, margin: Option<f32>, polar: bool| ContactGrade {
            call: RouteCall {
                short_bcr: sb,
                long_bcr: lb,
                ..call(margin, RouteVerdict::Short, None)
            },
            short_bearing_deg: 312.0,
            polar_short: polar,
            polar_long: false,
        };
        let mut t = ValidationTally::default();
        t.add(&grade(0.8, 0.0, Some(-9.0), false), Some(310.0)); // open, beam agrees
        t.add(&grade(0.05, 0.02, Some(-9.0), true), Some(130.0)); // outside, beam disagrees
        t.add(&grade(0.3, 0.3, Some(1.0), false), Some(310.0)); // neither; inside the margin
        t.add(&grade(0.6, 0.0, Some(-9.0), false), None); // no heading logged
        assert_eq!(
            t.all,
            Coverage {
                graded: 4,
                open: 2,
                outside: 1
            }
        );
        assert_eq!(
            t.polar,
            Coverage {
                graded: 1,
                open: 0,
                outside: 1
            }
        );
        assert_eq!(t.non_polar.graded, 3);
        assert_eq!((t.heading_judged, t.heading_agree), (2, 1));
        assert_eq!(t.by_band.len(), 1);
    }
}
