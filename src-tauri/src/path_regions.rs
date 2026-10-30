//! The region strips' sweep, kept: ten fixed DX regions' path over the day from the operator's
//! grid ([`propagation::sweep_regions`]), computed once and served from memory and disk.
//!
//! # Budget
//!
//! With P.533 a sweep is ten regions × two routes of the full prediction chain — about two
//! seconds of pure compute, on a blocking thread, with no lock held but this module's own. The
//! answer is a monthly-median climatology, so it is kept for six hours or until the inputs move
//! (UTC day, grid, engine, power and antenna gain, sunspot number), and written beside
//! `settings.json` so a restart does not sweep again. Every sweep writes the file, so nothing
//! needs flushing at quit. One sweep runs at a time: a second ask waits for the first and is
//! served what it kept.
//!
//! The heuristic engine is microseconds a prediction and reads the current Kp, so it is swept
//! fresh on every ask and never kept.
//!
//! # What is kept, and what is made current
//!
//! Kept: each region's cells (both routes' reliability and the route the model favours per band
//! and hour) and its MUF. Made current on every serve, from the Kp outlook held at that moment:
//! each hour's Kp, its source and the polar-cap crossings ([`propagation::refresh_regions`]). So a
//! kept sweep never serves a stale Kp.
//!
//! A file read back is used only when its key matches, it is younger than six hours (and not
//! from the future), and every day in it is well formed; anything else is ignored and swept again.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use propagation::{KpForecast, PathDay, RegionDay, SpaceWx, PATH_REGIONS};
use serde::{Deserialize, Serialize};

/// How long a P.533 sweep is kept.
pub(crate) const TTL_SECS: i64 = 6 * 3600;

/// A kept sweep, as held in memory and on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Stored {
    pub(crate) key: String,
    pub(crate) computed_unix: i64,
    pub(crate) engine: String,
    pub(crate) regions: Vec<RegionDay>,
}

/// One region as the strips get it: the fixed anchor and zones beside its day.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RegionView {
    pub(crate) id: &'static str,
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    /// CQ zones the region counts (the strips order by the operator's needs and contacts in them).
    pub(crate) zones: &'static [u8],
    pub(crate) day: PathDay,
}

/// `get_path_regions`' answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PathRegionsView {
    pub(crate) engine: String,
    /// When the model part was computed (unix seconds) — the hours are always current.
    pub(crate) computed_unix: i64,
    pub(crate) regions: Vec<RegionView>,
}

/// Everything a sweep's answer depends on, besides the clock's hour.
pub(crate) fn sweep_key(
    t: i64,
    mygrid: &str,
    engine: &str,
    station_power_w: Option<f64>,
    ant_gain_dbi: f64,
    ssn: Option<f32>,
) -> String {
    format!(
        "{}|{}|{engine}|{ant_gain_dbi}|{station_power_w:?}|{:?}",
        t.div_euclid(86_400),
        mygrid.trim().to_ascii_uppercase(),
        ssn.map(|v| v.round() as i32),
    )
}

/// The kept sweep's file, beside `settings.json`.
pub(crate) fn file_path() -> PathBuf {
    crate::settings_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("path_regions.json")
}

/// A kept sweep is usable for `key` at `now`.
fn fresh(s: &Stored, key: &str, now: i64) -> bool {
    s.key == key && s.computed_unix <= now && now - s.computed_unix < TTL_SECS
}

/// The kept sweep in `path`, when it is usable for `key` at `now` and every day in it is well
/// formed. A missing, unreadable, stale or malformed file is `None`: the caller sweeps again.
pub(crate) fn load(path: &Path, key: &str, now: i64) -> Option<Stored> {
    let text = std::fs::read_to_string(path).ok()?;
    let s: Stored = serde_json::from_str(&text).ok()?;
    let known = |d: &RegionDay| PATH_REGIONS.iter().any(|r| r.id == d.id);
    (fresh(&s, key, now)
        && !s.regions.is_empty()
        && s.regions
            .iter()
            .all(|d| known(d) && propagation::day_is_well_formed(&d.day)))
    .then_some(s)
}

/// Write the kept sweep (tmp + rename; best effort — a failed write only means the next start
/// sweeps again).
pub(crate) fn save(path: &Path, s: &Stored) {
    let Ok(text) = serde_json::to_string(s) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// The sweep in memory, and the gate that lets one sweep run at a time.
static KEPT: Mutex<Option<Stored>> = Mutex::new(None);
static SWEEPING: Mutex<()> = Mutex::new(());

/// What one ask needs: the operator, the engine, and the live space weather held.
pub(crate) struct Ask {
    pub(crate) me: (f64, f64),
    pub(crate) key: String,
    pub(crate) engine: Box<dyn propagation::PathPredictor>,
    pub(crate) t: i64,
    pub(crate) wx: SpaceWx,
    pub(crate) kp_now: f32,
    pub(crate) forecast: Option<KpForecast>,
}

/// Serve an ask — blocking (P.533 sweeps here): call it from a blocking thread. `path` is the
/// kept file.
pub(crate) fn serve(ask: Ask, path: &Path) -> PathRegionsView {
    let engine = ask.engine.name().to_string();
    let sweep = || {
        propagation::sweep_regions(
            ask.engine.as_ref(),
            ask.me,
            ask.t,
            &ask.wx,
            ask.kp_now,
            ask.forecast.as_ref(),
        )
    };
    let (computed_unix, mut regions) = if engine != "p533" {
        (ask.t, sweep())
    } else {
        let _one = SWEEPING.lock().unwrap_or_else(|e| e.into_inner());
        let held = KEPT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|s| fresh(s, &ask.key, ask.t))
            .or_else(|| load(path, &ask.key, ask.t));
        let s = held.unwrap_or_else(|| {
            let s = Stored {
                key: ask.key.clone(),
                computed_unix: ask.t,
                engine: engine.clone(),
                regions: sweep(),
            };
            save(path, &s);
            s
        });
        *KEPT.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
        (s.computed_unix, s.regions)
    };
    propagation::refresh_regions(
        &mut regions,
        ask.me,
        ask.t,
        ask.kp_now,
        ask.forecast.as_ref(),
    );
    PathRegionsView {
        engine,
        computed_unix,
        regions: PATH_REGIONS
            .iter()
            .filter_map(|r| {
                let d = regions.iter().find(|d| d.id == r.id)?;
                Some(RegionView {
                    id: r.id,
                    lat: r.lat,
                    lon: r.lon,
                    zones: r.zones,
                    day: d.day.clone(),
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOSTON: (f64, f64) = (42.36, -71.06);
    /// 2026-05-15 12:00 UTC.
    const T: i64 = 1_778_846_400;

    fn heuristic_sweep() -> Vec<RegionDay> {
        let eng = propagation::HeuristicEngine::new(Some(BOSTON));
        propagation::sweep_regions(&eng, BOSTON, T, &SpaceWx::default(), 2.0, None)
    }

    fn stored(key: &str, at: i64) -> Stored {
        Stored {
            key: key.into(),
            computed_unix: at,
            engine: "p533".into(),
            regions: heuristic_sweep(),
        }
    }

    #[test]
    fn a_saved_sweep_reads_back_while_it_is_fresh_and_for_its_key_only() {
        let dir = crate::test_dirs::root();
        let path = dir.join("path_regions.json");
        let s = stored("k", T);
        save(&path, &s);
        assert_eq!(load(&path, "k", T + 60), Some(s.clone()));
        assert_eq!(load(&path, "k", T + TTL_SECS - 1), Some(s));
        assert_eq!(load(&path, "k", T + TTL_SECS), None, "six hours old");
        assert_eq!(load(&path, "other", T + 60), None, "another key");
        assert_eq!(load(&path, "k", T - 1), None, "from the future");
        assert_eq!(load(&dir.join("absent.json"), "k", T), None);
    }

    #[test]
    fn a_malformed_or_foreign_file_is_never_used() {
        let dir = crate::test_dirs::root();
        let path = dir.join("path_regions.json");
        let mut s = stored("k", T);
        s.regions[0].day.bands[0].cells.truncate(5);
        save(&path, &s);
        assert_eq!(load(&path, "k", T), None, "a band without 24 hours");
        let mut s = stored("k", T);
        s.regions[3].id = "atlantis".into();
        save(&path, &s);
        assert_eq!(
            load(&path, "k", T),
            None,
            "a region this build does not know"
        );
        let mut s = stored("k", T);
        s.regions.clear();
        save(&path, &s);
        assert_eq!(load(&path, "k", T), None, "no regions");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path, "k", T), None);
        // Positive control: the same file, well formed, is used.
        save(&path, &stored("k", T));
        assert!(load(&path, "k", T).is_some());
    }

    #[test]
    fn the_key_moves_with_every_input_and_not_with_the_hour() {
        let k = |t, g: &str, e: &str, p, a, s| sweep_key(t, g, e, p, a, s);
        let base = k(T, "FN42", "p533", Some(100.0), 0.0, Some(105.0));
        assert_eq!(
            base,
            k(T + 3600, "fn42 ", "p533", Some(100.0), 0.0, Some(105.2))
        );
        for other in [
            k(T + 86_400, "FN42", "p533", Some(100.0), 0.0, Some(105.0)),
            k(T, "FN43", "p533", Some(100.0), 0.0, Some(105.0)),
            k(T, "FN42", "heuristic", Some(100.0), 0.0, Some(105.0)),
            k(T, "FN42", "p533", Some(10.0), 0.0, Some(105.0)),
            k(T, "FN42", "p533", Some(100.0), 3.0, Some(105.0)),
            k(T, "FN42", "p533", Some(100.0), 0.0, Some(80.0)),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn a_heuristic_ask_is_swept_fresh_in_region_order_and_writes_nothing() {
        let path = crate::test_dirs::root().join("path_regions.json");
        let view = serve(
            Ask {
                me: BOSTON,
                key: "k".into(),
                engine: Box::new(propagation::HeuristicEngine::new(Some(BOSTON))),
                t: T,
                wx: SpaceWx::default(),
                kp_now: 2.0,
                forecast: None,
            },
            &path,
        );
        assert_eq!(view.engine, "heuristic");
        let ids: Vec<&str> = view.regions.iter().map(|r| r.id).collect();
        assert_eq!(ids, PATH_REGIONS.iter().map(|r| r.id).collect::<Vec<_>>());
        assert!(view.regions.iter().all(|r| r.day.hours.len() == 24));
        assert!(!path.exists(), "the heuristic is never kept");
    }

    /// The P.533 path end to end without a sweep: a kept file is served (no prediction runs — a
    /// sweep would take seconds and stamp a new time), then held in memory, its hours made
    /// current from the Kp at the ask. The only test that reaches the P.533 branch, so the
    /// in-memory copy it leaves cannot meet another test's key.
    #[test]
    fn a_kept_p533_sweep_is_served_from_disk_then_memory_with_current_hours() {
        let path = crate::test_dirs::root().join("path_regions.json");
        let key = "kept-sweep-test";
        let s = stored(key, T - 3600);
        save(&path, &s);
        let ask = |kp| Ask {
            me: BOSTON,
            key: key.into(),
            engine: propagation::make_predictor("p533", Some(BOSTON), None, 0.0),
            t: T,
            wx: SpaceWx::default(),
            kp_now: kp,
            forecast: None,
        };
        let started = std::time::Instant::now();
        let v = serve(ask(9.0), &path);
        assert_eq!(v.computed_unix, T - 3600, "the kept sweep, not a new one");
        assert_eq!(v.regions.len(), 10);
        assert!(v
            .regions
            .iter()
            .all(|r| r.day.hours.iter().all(|h| h.kp == 9.0)));
        std::fs::remove_file(&path).unwrap();
        let v = serve(ask(1.0), &path);
        assert_eq!(v.computed_unix, T - 3600, "held in memory once read");
        assert!(v
            .regions
            .iter()
            .all(|r| r.day.hours.iter().all(|h| h.kp == 1.0)));
        assert!(started.elapsed().as_secs() < 5, "no P.533 sweep ran");
    }
}
