//! `get_path_outlook`'s kept predictions — the per-path card's compute budget.
//!
//! With P.533 a path outlook is two full prediction chains (short and long path, ~100 ms each in
//! release). The answer is a monthly-median day: it changes with the UTC day, the two grids, the
//! engine, the station's power and antenna gain and the sunspot number, and with nothing else. So
//! each request's pair of predictions is kept for six hours under exactly those, and a request
//! that finds them pays only for the cheap parts:
//!
//! - the "now" scalars — the MUF now and each band's per-mode chips — are re-derived for the
//!   serving hour from the day's own hourly arrays ([`refresh_now`]), the ring cache's rule;
//! - the geometry (Kp per hour and its source, polar shares, grey line, the live oval), the
//!   route calls and the day are recomputed by the caller on every serve from the space weather
//!   held at that moment, so a kept prediction never serves a stale Kp.
//!
//! The client asks again every 60 s while a station stays selected, so the current hour moves on
//! without a fresh prediction. The heuristic engine is microseconds and reads the current Kp, so
//! it is never kept. Up to [`CAP`] pairs are held; expired ones go on insert, then the oldest.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use propagation::PathPrediction;

/// How long a kept pair is served.
pub(crate) const TTL: Duration = Duration::from_secs(6 * 3600);
/// How many pairs are held at most (a station a click, so a session's worth of selections).
pub(crate) const CAP: usize = 32;

/// A kept pair: when it was computed, its key, the short path and (when modelled) the long path,
/// each without geometry.
type Kept = (Instant, String, PathPrediction, Option<PathPrediction>);

static KEPT: Mutex<Vec<Kept>> = Mutex::new(Vec::new());

/// Everything a path outlook's predictions depend on, besides the hour.
#[allow(clippy::too_many_arguments)]
pub(crate) fn key(
    t: i64,
    mygrid: &str,
    dx_grid: &str,
    engine: &str,
    station_power_w: Option<f64>,
    ant_gain_dbi: f64,
    ssn: Option<f32>,
) -> String {
    format!(
        "{}|{}|{}|{engine}|{ant_gain_dbi}|{station_power_w:?}|{:?}",
        t.div_euclid(86_400),
        mygrid.trim().to_ascii_uppercase(),
        dx_grid.trim().to_ascii_uppercase(),
        ssn.map(|v| v.round() as i32),
    )
}

fn find(
    kept: &[Kept],
    key: &str,
    now: Instant,
) -> Option<(PathPrediction, Option<PathPrediction>)> {
    kept.iter()
        .find(|(at, k, _, _)| k == key && now.saturating_duration_since(*at) < TTL)
        .map(|(_, _, sp, lp)| (sp.clone(), lp.clone()))
}

fn insert(kept: &mut Vec<Kept>, entry: Kept, now: Instant) {
    kept.retain(|(at, k, _, _)| *k != entry.1 && now.saturating_duration_since(*at) < TTL);
    if kept.len() >= CAP {
        // The oldest goes first.
        if let Some(i) = kept
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.0)
            .map(|(i, _)| i)
        {
            kept.remove(i);
        }
    }
    kept.push(entry);
}

/// The kept pair for `key`, when it is younger than [`TTL`].
pub(crate) fn get(key: &str) -> Option<(PathPrediction, Option<PathPrediction>)> {
    let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    find(&kept, key, Instant::now())
}

/// Keep a freshly computed pair under `key` (geometry, routes and day stripped: they are
/// recomputed on every serve).
pub(crate) fn put(key: String, mut sp: PathPrediction, mut lp: Option<PathPrediction>) {
    for p in std::iter::once(&mut sp).chain(lp.as_mut()) {
        p.geometry = None;
        p.routes = None;
        p.day = None;
        p.long_path = None;
    }
    let now = Instant::now();
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    insert(&mut kept, (now, key, sp, lp), now);
}

/// Re-derive a kept prediction's "now" scalars for the UTC hour containing `t`: the MUF now from
/// the day's hourly MUF, and each band's per-mode chips from its hourly modes.
pub(crate) fn refresh_now(p: &mut PathPrediction, t: i64) {
    let h = (t.rem_euclid(86_400) / 3600) as usize;
    if let Some(&m) = p.muf_hourly.get(h) {
        p.muf_now = m;
    }
    for b in &mut p.bands {
        if !b.mode_hourly.is_empty() {
            b.mode_now = propagation::mode_now_at(&b.mode_hourly, h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use propagation::{PathPredictor, SpaceWx};

    const BOSTON: (f64, f64) = (42.36, -71.06);
    const LONDON: (f64, f64) = (51.5, -0.12);
    /// 2026-05-15 12:00 UTC.
    const T: i64 = 1_778_846_400;

    fn pred() -> PathPrediction {
        propagation::HeuristicEngine::new(Some(BOSTON)).predict(LONDON, T, &SpaceWx::default())
    }

    #[test]
    fn the_key_moves_with_every_input_and_not_with_the_hour() {
        let k = |t, g: &str, d: &str, e: &str, p, a, s| key(t, g, d, e, p, a, s);
        let base = k(T, "FN42", "IO91", "p533", Some(100.0), 0.0, Some(105.0));
        assert_eq!(
            base,
            k(
                T + 3600,
                " fn42",
                "io91 ",
                "p533",
                Some(100.0),
                0.0,
                Some(104.6)
            )
        );
        for other in [
            k(
                T + 86_400,
                "FN42",
                "IO91",
                "p533",
                Some(100.0),
                0.0,
                Some(105.0),
            ),
            k(T, "FN43", "IO91", "p533", Some(100.0), 0.0, Some(105.0)),
            k(T, "FN42", "IO91WM", "p533", Some(100.0), 0.0, Some(105.0)),
            k(
                T,
                "FN42",
                "IO91",
                "heuristic",
                Some(100.0),
                0.0,
                Some(105.0),
            ),
            k(T, "FN42", "IO91", "p533", Some(10.0), 0.0, Some(105.0)),
            k(T, "FN42", "IO91", "p533", Some(100.0), 3.0, Some(105.0)),
            k(T, "FN42", "IO91", "p533", Some(100.0), 0.0, Some(80.0)),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn a_kept_pair_is_served_for_its_key_until_it_expires() {
        let now = Instant::now();
        let mut kept = Vec::new();
        insert(&mut kept, (now, "a".into(), pred(), None), now);
        assert!(find(&kept, "a", now + Duration::from_secs(60)).is_some());
        assert!(find(&kept, "b", now).is_none(), "another key");
        assert!(find(&kept, "a", now + TTL).is_none(), "six hours old");
        // A newer pair under the same key replaces the old one.
        insert(&mut kept, (now, "a".into(), pred(), None), now);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn the_store_is_capped_and_drops_the_oldest() {
        let t0 = Instant::now();
        let mut kept = Vec::new();
        for i in 0..CAP + 3 {
            let at = t0 + Duration::from_secs(i as u64);
            insert(&mut kept, (at, format!("k{i}"), pred(), None), at);
        }
        assert_eq!(kept.len(), CAP);
        let now = t0 + Duration::from_secs((CAP + 3) as u64);
        assert!(find(&kept, "k0", now).is_none() && find(&kept, "k2", now).is_none());
        assert!(
            find(&kept, "k3", now).is_some()
                && find(&kept, &format!("k{}", CAP + 2), now).is_some()
        );
    }

    #[test]
    fn put_strips_what_every_serve_recomputes() {
        let mut sp = pred();
        sp.geometry = Some(propagation::path_geometry(
            BOSTON,
            LONDON,
            T,
            Some(2.0),
            None,
            None,
        ));
        let k = format!("strip-test-{T}");
        put(k.clone(), sp, None);
        let (got, lp) = get(&k).expect("kept");
        assert!(
            got.geometry.is_none() && got.day.is_none() && got.routes.is_none() && lp.is_none()
        );
    }

    #[test]
    fn the_now_scalars_follow_the_serving_hour() {
        let mut p = pred();
        p.muf_hourly = (0..24).map(|h| h as f32).collect();
        refresh_now(&mut p, T + 5 * 3600);
        assert_eq!(p.muf_now, 17.0);
        refresh_now(&mut p, T - 12 * 3600);
        assert_eq!(p.muf_now, 0.0);
    }
}
