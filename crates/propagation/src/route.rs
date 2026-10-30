//! Short path or long path — which way round the great circle the model says a signal goes.
//!
//! The P.533 engine predicts each route on its own ([`crate::PathPredictor::predict_route`]);
//! this module compares the two for one band at one hour and says which one the MODEL favours,
//! with a fixed confidence vocabulary and the reasons behind it, so a surface can show the
//! answer and why without inventing either:
//!
//! - **Model margin.** One route open (BCR ≥ 50 %) and the other closed (< 10 %): that route,
//!   *likely*. Otherwise a median-SNR margin of ≥ 6 dB picks the better route — *likely* when
//!   that route is open, *possible* when it is not. Inside 6 dB it is *either*. Both closed is
//!   *unknown*: nothing the model knows explains a contact then.
//! - **Grey line.** On 160/80/40 m, a route whose arc lies along the terminator while the
//!   other does not moves the answer one notch toward it, and never past *likely*.
//! - **The long path is held at *possible*.** The long route has no reference to validate it
//!   against (and a cross-check against another model found it markedly more optimistic on the
//!   longest arcs), so until it is validated a long-path verdict never reads *likely*. The
//!   short path, which is validated, keeps the full scale.
//!
//! This is the model's opinion only. P.533 is a monthly median, the long route has no
//! reference fixture to validate against, and skewed (off-great-circle) paths are not
//! modelled; "confirmed" needs evidence from the station itself (a beam heading), which this
//! module never claims.

use serde::{Deserialize, Serialize};

use crate::model::Band;
use crate::predict::PathPrediction;

/// Which way round the great circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Route {
    Short,
    Long,
}

/// The model's answer for one band at one hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteVerdict {
    Short,
    Long,
    /// Both routes modelled within the margin — a real answer, not a failure.
    Either,
    /// Both routes modelled closed.
    Unknown,
}

/// How strongly the model favours the route it names (`Short`/`Long` verdicts only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteConfidence {
    Likely,
    Possible,
}

/// One piece of evidence behind a verdict, for the surface to put into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RouteReason {
    /// One route modelled open, the other closed.
    OneClosed { open: Route },
    /// The median SNR differs by at least the margin.
    Margin { better: Route },
    /// The two routes are modelled within the margin of each other.
    WithinMargin,
    /// Both routes modelled closed.
    BothClosed,
    /// A low band whose arc on this route lies along the grey line and the other does not.
    GreyLine { route: Route },
}

/// The model's route call for one band at one hour.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteCall {
    pub band: String,
    pub verdict: RouteVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<RouteConfidence>,
    /// Long-path minus short-path median SNR (dB), when the model gives both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_db: Option<f32>,
    /// Each route's modelled circuit reliability this hour (0..1).
    pub short_bcr: f32,
    pub long_bcr: f32,
    pub reasons: Vec<RouteReason>,
}

/// A route counts as open from this circuit reliability …
pub const OPEN_BCR: f32 = 0.5;
/// … and closed below this one.
pub const CLOSED_BCR: f32 = 0.1;
/// Median-SNR difference (dB) that separates the routes.
pub const MARGIN_DB: f32 = 6.0;
/// Share of an arc on the grey line that counts as "along the terminator".
pub const GREY_FRAC: f32 = 0.2;

/// One band's evidence for one route at one hour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteEvidence {
    pub bcr: f32,
    /// Median SNR (dB), when the model gives it.
    pub snr_db: Option<f32>,
    /// Share of the arc on the grey line this hour, when the geometry is held.
    pub greyline_frac: Option<f32>,
}

/// −2 short likely … 0 either … +2 long likely.
fn notch(v: RouteVerdict, c: Option<RouteConfidence>) -> i8 {
    let n = match c {
        Some(RouteConfidence::Likely) => 2,
        _ => 1,
    };
    match v {
        RouteVerdict::Short => -n,
        RouteVerdict::Long => n,
        _ => 0,
    }
}

fn from_notch(n: i8) -> (RouteVerdict, Option<RouteConfidence>) {
    match n {
        i8::MIN..=-2 => (RouteVerdict::Short, Some(RouteConfidence::Likely)),
        -1 => (RouteVerdict::Short, Some(RouteConfidence::Possible)),
        0 => (RouteVerdict::Either, None),
        1 => (RouteVerdict::Long, Some(RouteConfidence::Possible)),
        _ => (RouteVerdict::Long, Some(RouteConfidence::Likely)),
    }
}

fn is_low_band(band: &str) -> bool {
    matches!(
        Band::from_label(band),
        Some(Band::B160 | Band::B80 | Band::B40)
    )
}

/// The model's call for `band` from each route's evidence at one hour.
pub fn infer_route(band: &str, sp: RouteEvidence, lp: RouteEvidence) -> RouteCall {
    let margin_db = sp.snr_db.zip(lp.snr_db).map(|(s, l)| l - s);
    let open = |e: &RouteEvidence| e.bcr >= OPEN_BCR;
    let closed = |e: &RouteEvidence| e.bcr < CLOSED_BCR;
    let mut reasons = Vec::new();
    let call = |verdict, confidence, reasons| RouteCall {
        band: band.to_string(),
        verdict,
        confidence,
        margin_db,
        short_bcr: sp.bcr,
        long_bcr: lp.bcr,
        reasons,
    };

    if closed(&sp) && closed(&lp) {
        reasons.push(RouteReason::BothClosed);
        return call(RouteVerdict::Unknown, None, reasons);
    }
    let (mut verdict, mut confidence) = if open(&sp) && closed(&lp) {
        reasons.push(RouteReason::OneClosed { open: Route::Short });
        (RouteVerdict::Short, Some(RouteConfidence::Likely))
    } else if open(&lp) && closed(&sp) {
        reasons.push(RouteReason::OneClosed { open: Route::Long });
        (RouteVerdict::Long, Some(RouteConfidence::Likely))
    } else {
        match margin_db {
            Some(m) if m.abs() >= MARGIN_DB => {
                let (better, e) = if m > 0.0 {
                    (Route::Long, &lp)
                } else {
                    (Route::Short, &sp)
                };
                reasons.push(RouteReason::Margin { better });
                let conf = if open(e) {
                    RouteConfidence::Likely
                } else {
                    RouteConfidence::Possible
                };
                let v = match better {
                    Route::Short => RouteVerdict::Short,
                    Route::Long => RouteVerdict::Long,
                };
                (v, Some(conf))
            }
            // No SNR from the model: the BCR alone, the better one at most "possible".
            None if (sp.bcr - lp.bcr).abs() >= OPEN_BCR - CLOSED_BCR => {
                let v = if lp.bcr > sp.bcr {
                    RouteVerdict::Long
                } else {
                    RouteVerdict::Short
                };
                (v, Some(RouteConfidence::Possible))
            }
            _ => {
                reasons.push(RouteReason::WithinMargin);
                (RouteVerdict::Either, None)
            }
        }
    };

    if is_low_band(band) {
        let g = |e: &RouteEvidence| e.greyline_frac.unwrap_or(0.0);
        let toward = if g(&lp) >= GREY_FRAC && g(&sp) < GREY_FRAC {
            Some(Route::Long)
        } else if g(&sp) >= GREY_FRAC && g(&lp) < GREY_FRAC {
            Some(Route::Short)
        } else {
            None
        };
        if let Some(route) = toward {
            let step = if route == Route::Long { 1 } else { -1 };
            let n = (notch(verdict, confidence) + step).clamp(-2, 2);
            (verdict, confidence) = from_notch(n);
            reasons.push(RouteReason::GreyLine { route });
        }
    }
    if verdict == RouteVerdict::Long {
        confidence = Some(RouteConfidence::Possible);
    }
    call(verdict, confidence, reasons)
}

/// Route calls for every band at the request's hour, from a short-path and a long-path
/// prediction of the same request (each with its geometry when held). Bands missing from
/// either prediction are skipped. The hour is the short path's geometry `now_hour`; without
/// geometry there is no hour to call, so the list is empty.
pub fn compare_routes(sp: &PathPrediction, lp: &PathPrediction) -> Vec<RouteCall> {
    let Some(h) = sp.geometry.as_ref().map(|g| g.now_hour as usize) else {
        return Vec::new();
    };
    compare_routes_at(sp, lp, h)
}

/// [`compare_routes`] at UTC hour `h` (0..23) — the path-over-the-day strip asks it for every
/// hour, so each cell carries the call the card would make at that hour.
pub fn compare_routes_at(sp: &PathPrediction, lp: &PathPrediction, h: usize) -> Vec<RouteCall> {
    let evidence = |p: &PathPrediction, b: &crate::likelihood::BandOutlook| RouteEvidence {
        bcr: b.hourly.get(h).copied().unwrap_or(0.0),
        snr_db: b
            .detail
            .as_ref()
            .and_then(|d| d.get(h).copied().flatten())
            .map(|ph| ph.snr_db),
        greyline_frac: p
            .geometry
            .as_ref()
            .and_then(|g| g.hours.get(h))
            .map(|x| x.greyline_frac),
    };
    sp.bands
        .iter()
        .filter_map(|sb| {
            let lb = lp.bands.iter().find(|b| b.band == sb.band)?;
            Some(infer_route(&sb.band, evidence(sp, sb), evidence(lp, lb)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(bcr: f32, snr: f32) -> RouteEvidence {
        RouteEvidence {
            bcr,
            snr_db: Some(snr),
            greyline_frac: None,
        }
    }

    fn grey(mut e: RouteEvidence, f: f32) -> RouteEvidence {
        e.greyline_frac = Some(f);
        e
    }

    fn vc(c: &RouteCall) -> (RouteVerdict, Option<RouteConfidence>) {
        (c.verdict, c.confidence)
    }

    use RouteConfidence::*;
    use RouteVerdict as V;

    #[test]
    fn a_long_path_verdict_is_held_at_possible_until_the_long_path_is_validated() {
        // Long open, short closed: the strongest long-path case the model has, still possible.
        let c = infer_route("20m", ev(0.05, 0.0), ev(0.7, 2.0));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        // Both open, long 9 dB better: possible, not likely.
        let c = infer_route("20m", ev(0.6, 0.0), ev(0.9, 9.0));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        // A grey-line nudge cannot lift it past possible either.
        let c = infer_route("80m", grey(ev(0.3, 0.0), 0.0), grey(ev(0.3, 7.0), 0.4));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        // Negative control: the validated short path keeps "likely".
        let c = infer_route("20m", ev(0.8, 10.0), ev(0.02, 9.0));
        assert_eq!(vc(&c), (V::Short, Some(Likely)));
    }

    #[test]
    fn one_route_open_and_the_other_closed_is_likely_that_route() {
        // (The long path is held at possible until it is validated.)
        let c = infer_route("20m", ev(0.05, 0.0), ev(0.7, 2.0));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        assert_eq!(
            c.reasons,
            vec![RouteReason::OneClosed { open: Route::Long }]
        );
        assert_eq!(c.margin_db, Some(2.0));
        let c = infer_route("20m", ev(0.8, 10.0), ev(0.02, 9.0));
        assert_eq!(vc(&c), (V::Short, Some(Likely)));
    }

    #[test]
    fn a_margin_of_6_db_picks_a_route_and_less_is_either() {
        // Both open, long 9 dB better: long (possible — the long path's cap).
        let c = infer_route("20m", ev(0.6, 0.0), ev(0.9, 9.0));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        assert_eq!(
            c.reasons,
            vec![RouteReason::Margin {
                better: Route::Long
            }]
        );
        // Both marginal, short 7 dB better: only possible (the better one is not open).
        let c = infer_route("20m", ev(0.4, 7.0), ev(0.2, 0.0));
        assert_eq!(vc(&c), (V::Short, Some(Possible)));
        // 5.9 dB apart: either, said as such.
        let c = infer_route("20m", ev(0.6, 0.0), ev(0.7, 5.9));
        assert_eq!(vc(&c), (V::Either, None));
        assert_eq!(c.reasons, vec![RouteReason::WithinMargin]);
    }

    #[test]
    fn both_closed_is_unknown_whatever_the_margin() {
        let c = infer_route("20m", ev(0.05, -20.0), ev(0.01, 0.0));
        assert_eq!(vc(&c), (V::Unknown, None));
        assert_eq!(c.reasons, vec![RouteReason::BothClosed]);
        // Grey line does not rescue a closed band either.
        let c = infer_route("40m", ev(0.05, -20.0), grey(ev(0.01, 0.0), 0.9));
        assert_eq!(c.verdict, V::Unknown);
    }

    #[test]
    fn grey_line_nudges_a_low_band_one_notch_and_never_past_likely() {
        // Either → possible long when only the long arc lies along the terminator.
        let c = infer_route("40m", grey(ev(0.6, 0.0), 0.0), grey(ev(0.6, 3.0), 0.5));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        assert!(c
            .reasons
            .contains(&RouteReason::GreyLine { route: Route::Long }));
        // Possible long → likely long, held at possible by the long path's cap.
        let c = infer_route("80m", grey(ev(0.3, 0.0), 0.0), grey(ev(0.3, 7.0), 0.4));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        // Already the strongest long call: no further.
        let c = infer_route("160m", grey(ev(0.05, 0.0), 0.0), grey(ev(0.8, 9.0), 0.4));
        assert_eq!(vc(&c), (V::Long, Some(Possible)));
        // Toward the other route: likely short becomes possible short.
        let c = infer_route("40m", grey(ev(0.8, 9.0), 0.0), grey(ev(0.05, 0.0), 0.4));
        assert_eq!(vc(&c), (V::Short, Some(Possible)));
        // Negative controls: a high band ignores grey line; both arcs grey is no nudge.
        let c = infer_route("20m", grey(ev(0.6, 0.0), 0.0), grey(ev(0.6, 3.0), 0.5));
        assert_eq!(vc(&c), (V::Either, None));
        let c = infer_route("40m", grey(ev(0.6, 0.0), 0.3), grey(ev(0.6, 3.0), 0.5));
        assert_eq!(vc(&c), (V::Either, None));
    }

    #[test]
    fn without_snr_only_a_large_bcr_gap_names_a_route() {
        let no = |bcr| RouteEvidence {
            bcr,
            snr_db: None,
            greyline_frac: None,
        };
        assert_eq!(
            vc(&infer_route("20m", no(0.45,), no(0.0))),
            (V::Short, Some(Possible))
        );
        assert_eq!(vc(&infer_route("20m", no(0.3), no(0.4))), (V::Either, None));
        assert_eq!(infer_route("20m", no(0.3), no(0.4)).margin_db, None);
    }
}
