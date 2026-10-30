//! Short path or long path, end to end: the P.533 engine predicts both routes of a real pair and
//! `compare_routes` names the one the model favours, hour by hour, exactly as the shell attaches
//! them to the Path card. Three classic geometries:
//!
//! - **W to VK** (Boston–Sydney, 20 m, May): the long path opens at Boston's sunset, when that
//!   end of the long arc sits on the grey line and the short path is closed; around midday the
//!   short path carries it instead.
//! - **A polar short path** (Boston–Tokyo): both routes cross a polar cap (one per hemisphere),
//!   so the long path is a different hour's answer, not an escape from the cap.
//! - **An equatorial pair** (Singapore–Nairobi): the long path never wins.
//!
//! The long route has no reference fixture (the reference implementation is shortest-arc only);
//! these pin what the model says, not that it is right, and until it is validated a long-path
//! call is held at "possible".

use propagation::p533::engine::P533Engine;
use propagation::{
    compare_routes, path_geometry_route, PathPrediction, PathPredictor, Route, RouteCall,
    RouteConfidence, RouteVerdict, SpaceWx,
};

const MAY_00Z: i64 = 1_778_803_200; // 2026-05-15 00:00 UTC
const BOSTON: (f64, f64) = (42.36, -71.06);
const SYDNEY: (f64, f64) = (-33.87, 151.21);
const TOKYO: (f64, f64) = (35.68, 139.69);
const SINGAPORE: (f64, f64) = (1.35, 103.82);
const NAIROBI: (f64, f64) = (-1.29, 36.82);

fn wx() -> SpaceWx {
    SpaceWx {
        sfi: 150.0,
        kp: 2.0,
        ..Default::default()
    }
}

/// Both routes for the day, as the shell builds them (minus the per-hour geometry).
fn both(me: (f64, f64), dx: (f64, f64)) -> (PathPrediction, PathPrediction) {
    let e = P533Engine::new(Some(me));
    let sp = e.predict(dx, MAY_00Z, &wx());
    let lp = e
        .predict_route(dx, Route::Long, MAY_00Z, &wx())
        .expect("P.533 models the long path");
    (sp, lp)
}

/// The route call for `band` at UTC `hour`, with each route's geometry for that hour attached.
fn call_at(
    me: (f64, f64),
    dx: (f64, f64),
    preds: &(PathPrediction, PathPrediction),
    band: &str,
    hour: i64,
) -> RouteCall {
    let (mut sp, mut lp) = preds.clone();
    let t = MAY_00Z + hour * 3600;
    sp.geometry = Some(path_geometry_route(
        me,
        dx,
        Route::Short,
        t,
        Some(2.0),
        None,
        None,
    ));
    lp.geometry = Some(path_geometry_route(
        me,
        dx,
        Route::Long,
        t,
        Some(2.0),
        None,
        None,
    ));
    compare_routes(&sp, &lp)
        .into_iter()
        .find(|c| c.band == band)
        .expect("band in both predictions")
}

#[test]
fn w_to_vk_on_20m_goes_long_path_at_bostons_sunset_and_short_at_midday() {
    let preds = both(BOSTON, SYDNEY);
    let c = call_at(BOSTON, SYDNEY, &preds, "20m", 23);
    assert_eq!(
        (c.verdict, c.confidence),
        (RouteVerdict::Long, Some(RouteConfidence::Possible)),
        "{c:?}"
    );
    assert!(c.long_bcr >= 0.5 && c.short_bcr < 0.1, "{c:?}");
    // That hour is the grey-line hour at the operator's end of the long arc.
    let g = path_geometry_route(
        BOSTON,
        SYDNEY,
        Route::Long,
        MAY_00Z + 23 * 3600,
        Some(2.0),
        None,
        None,
    );
    assert!(
        g.hours[23].grey_me,
        "Boston is on the grey line at 23Z in May"
    );
    // Midday in Boston: the short path carries it and the long path is closed.
    let c = call_at(BOSTON, SYDNEY, &preds, "20m", 12);
    assert_eq!(c.verdict, RouteVerdict::Short, "{c:?}");
}

#[test]
fn a_polar_short_path_and_its_long_path_trade_hours() {
    let preds = both(BOSTON, TOKYO);
    // Both routes cross a cap, so neither route escapes the aurora: the hours decide.
    for route in [Route::Short, Route::Long] {
        let g = path_geometry_route(BOSTON, TOKYO, route, MAY_00Z, Some(2.0), None, None);
        assert!(
            g.hours.iter().all(|h| h.polar_frac.unwrap() > 0.0),
            "{route:?}"
        );
    }
    let morning = call_at(BOSTON, TOKYO, &preds, "20m", 11);
    assert_eq!(morning.verdict, RouteVerdict::Short, "{morning:?}");
    let evening = call_at(BOSTON, TOKYO, &preds, "20m", 22);
    assert_eq!(
        (evening.verdict, evening.confidence),
        (RouteVerdict::Long, Some(RouteConfidence::Possible)),
        "{evening:?}"
    );
    assert!(evening.margin_db.expect("P.533 gives SNR") >= 6.0);
}

#[test]
fn an_equatorial_pair_never_goes_long_path() {
    let preds = both(SINGAPORE, NAIROBI);
    let mut short_hours = 0;
    for band in ["40m", "20m"] {
        for h in 0..24 {
            let c = call_at(SINGAPORE, NAIROBI, &preds, band, h);
            assert_ne!(c.verdict, RouteVerdict::Long, "{band} {h}Z {c:?}");
            short_hours += (c.verdict == RouteVerdict::Short) as usize;
        }
    }
    // Positive control: the short path is named, not just "not long".
    assert!(short_hours >= 30, "{short_hours}");
}

#[test]
fn near_the_antipode_no_route_is_called() {
    // Every 4-character grid has an exact antipodal grid centre: FN42 (42.5, -71) and OE47
    // (-42.5, 109). Madrid -> Wellington is about 1.5° off antipodal.
    let all = |me: (f64, f64), dx: (f64, f64), t: i64| {
        let e = P533Engine::new(Some(me));
        let mut sp = e.predict(dx, t, &wx());
        let mut lp = e
            .predict_route(dx, Route::Long, t, &wx())
            .expect("P.533 models the long path");
        sp.geometry = Some(path_geometry_route(
            me,
            dx,
            Route::Short,
            t,
            Some(2.0),
            None,
            None,
        ));
        lp.geometry = Some(path_geometry_route(
            me,
            dx,
            Route::Long,
            t,
            Some(2.0),
            None,
            None,
        ));
        compare_routes(&sp, &lp)
    };
    for t in [MAY_00Z, MAY_00Z + 12 * 3600] {
        assert!(all((42.5, -71.0), (-42.5, 109.0), t).is_empty());
        assert!(all((40.42, -3.7), (-41.29, 174.78), t).is_empty());
    }
    // Positive control: a pair 30° off antipodal is called.
    assert!(!all(BOSTON, SYDNEY, MAY_00Z).is_empty());
}
