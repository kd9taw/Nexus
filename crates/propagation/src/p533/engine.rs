//! The `"p533"` [`PathPredictor`] engine — the validated P.533/P.372 chain
//! packaged behind the app's swappable prediction seam.
//!
//! Per band (at its digital watering-hole dial) the engine runs the full chain
//! for each of the 24 UT hours of the day containing the request, and maps the
//! outputs onto the existing [`BandOutlook`] shape:
//! - `hourly[h]` = Basic Circuit Reliability / 100 (a REAL per-hour "percent of
//!   days this circuit works" — the number the heuristic only approximated),
//! - `reliability` = the share of hours with BCR ≥ 50%,
//! - `muf_hourly` = the operational MUF, `window` formatted identically to the
//!   heuristic's (same contiguous-run + greyline logic and format strings).
//!
//! Inputs: the smoothed sunspot number rides in [`SpaceWx::ssn`] when the live
//! solar-cycle feed has it; otherwise it is derived from SFI by inverting the
//! Covington relation (documented approximation). Assumptions (isotropic
//! antennas, TX power, man-made noise class, required SNR) live in
//! [`P533Config`] — every constant tunable, none buried in the math.

use crate::geo;
use crate::likelihood::{
    hhmm, hhmm_z, mode_now_at, BandOutlook, ModeHourly, PathHour, PathLayer, Workability,
};
use crate::model::{band_digital_mhz, Band, SpaceWx};
use crate::predict::{PathPrediction, PathPredictor};

use super::geometry::Location;
use super::noise::ManMadeCategory;
use super::power::Dominant;
use super::reliability::bcr_for_required_snr;
use super::{run_p533_route, P533Params, P533Run};
use crate::route::Route;

/// Required SNR (dB in 1 Hz) per operating mode, for the per-mode "workable
/// now" rows. VOACAP-conventional figures: FT8 13 dB·Hz (−21 dB in 2.5 kHz),
/// FT4 ≈ 3.5 dB less sensitive, CW 24 dB·Hz (skilled ear), SSB 38 dB·Hz
/// (communication quality). Table order = display order.
const MODE_SNR_DBHZ: [(&str, f64); 4] = [("FT8", 13.0), ("FT4", 16.5), ("CW", 24.0), ("SSB", 38.0)];

/// Tunable system assumptions for the p533 engine.
#[derive(Debug, Clone, Copy)]
pub struct P533Config {
    /// Transmit power, dB(1 kW). 100 W = −10 dB(1 kW), the app default.
    pub txpower_dbkw: f64,
    /// Man-made noise environment at the receiver.
    pub man_made: ManMadeCategory,
    /// Required SNR (dB in 1 Hz) for the "circuit works" criterion. 13 dB·Hz
    /// ≈ FT8's −21 dB in 2500 Hz — the digital-first default.
    pub required_snr_dbhz: f64,
    /// BCR (%) a hour must clear to count toward `BandOutlook.reliability`.
    pub bcr_usable_pct: f64,
    /// Combined antenna gain (dBi, TX + RX) applied as a plain dB adder to the
    /// link budget (v1: no pattern/takeoff-angle modelling — the P.372 noise is
    /// referenced to a short lossless antenna, so signal-side gain shifts SNR 1:1).
    pub ant_gain_dbi: f64,
}

impl Default for P533Config {
    fn default() -> Self {
        Self {
            txpower_dbkw: -10.0,
            man_made: ManMadeCategory::Rural,
            required_snr_dbhz: 13.0,
            bcr_usable_pct: 50.0,
            ant_gain_dbi: 0.0,
        }
    }
}

impl P533Config {
    /// Config with the transmit power taken from a station-power setting (W).
    pub fn with_power_watts(watts: f64) -> Self {
        Self {
            txpower_dbkw: 10.0 * (watts.max(0.1) / 1000.0).log10(),
            ..Self::default()
        }
    }
}

/// The sunspot number the engine runs on: the R12 held, else the one derived from the SFI. Every
/// cache of this engine's answers keys on this, never on the held R12 alone, so a changed SFI is a
/// different answer when no R12 is held.
pub fn effective_ssn(wx: &SpaceWx) -> f64 {
    wx.ssn
        .map(f64::from)
        .unwrap_or_else(|| ssn_from_sfi(wx.sfi as f64))
}

/// Smoothed sunspot number from SFI by inverting Covington
/// (SFI ≈ 63.75 + 0.728·R + 0.00089·R²) — the offline fallback when the live
/// solar-cycle feed hasn't supplied a real R12.
pub fn ssn_from_sfi(sfi: f64) -> f64 {
    // Quadratic inversion; clamped to the physical range.
    let a = 0.00089;
    let b = 0.728;
    let c = 63.75 - sfi;
    let disc = (b * b - 4.0 * a * c).max(0.0);
    ((-b + disc.sqrt()) / (2.0 * a)).clamp(0.0, 250.0)
}

/// The p533 engine: the operator location is baked in like [`crate::predict::HeuristicEngine`].
pub struct P533Engine {
    me: Option<(f64, f64)>,
    cfg: P533Config,
}

impl P533Engine {
    pub fn new(me_latlon: Option<(f64, f64)>) -> Self {
        Self {
            me: me_latlon,
            cfg: P533Config::default(),
        }
    }

    pub fn with_config(me_latlon: Option<(f64, f64)>, cfg: P533Config) -> Self {
        Self { me: me_latlon, cfg }
    }
}

/// How this run's signal travels: the dominant mode's layer, hops and take-off angle
/// (≤ 7000 km, the short model's §6 dominant mode), or the long model's fM hop geometry
/// beyond 7000 km, where the method assumes F2 hops and names no single mode (flagged
/// `estimated`: that geometry follows from distance alone). `None`
/// when no mode carries the frequency. Reads values the chain already computed.
fn path_hour(run: &P533Run) -> Option<PathHour> {
    let estimated = run.path.distance > 7000.0;
    let (layer, hops, ele) = if estimated {
        if run.long_hops == 0 {
            return None;
        }
        (PathLayer::F2, run.long_hops as usize, run.long_ele)
    } else {
        match run.power.dominant? {
            Dominant::E(i) => (PathLayer::E, i + 1, run.path.md_e[i].ele),
            Dominant::F2(i) => (PathLayer::F2, i + 1, run.path.md_f2[i].ele),
        }
    };
    Some(PathHour {
        layer,
        hops: hops.min(u8::MAX as usize) as u8,
        takeoff_deg: ele.to_degrees() as f32,
        snr_db: run.rel.snr as f32,
        estimated,
    })
}

/// The heuristic's window formatting over a 24×1-hour score array: the
/// contiguous ≥Fair run containing the peak, with the low-band greyline label
/// (byte-identical format strings so the two engines read the same in the UI).
fn best_window(scores: &[f32; 24], day0: i64, band: Band) -> (String, bool, f32, usize) {
    const STEP: i64 = 3600;
    let (mut peak_i, mut peak) = (0usize, 0.0f32);
    for (i, &s) in scores.iter().enumerate() {
        if s > peak {
            peak = s;
            peak_i = i;
        }
    }
    let mut grayline = false;
    let window = if peak < 0.30 {
        "—".to_string()
    } else {
        let thresh = 0.30f32;
        let mut lo = peak_i;
        while lo > 0 && scores[lo - 1] >= thresh {
            lo -= 1;
        }
        let mut hi = peak_i;
        while (hi + 1) < 24 && scores[hi + 1] >= thresh {
            hi += 1;
        }
        let start = day0 + lo as i64 * STEP;
        let end = day0 + (hi as i64 + 1) * STEP;
        let width = end - start;
        let low_band = matches!(band, Band::B160 | Band::B80 | Band::B40);
        if low_band && width <= 2 * 3600 {
            grayline = true;
            format!("~{} greyline", hhmm_z(day0 + peak_i as i64 * STEP))
        } else {
            format!("{}–{}", hhmm(start), hhmm_z(end))
        }
    };
    (window, grayline, peak, peak_i)
}

impl PathPredictor for P533Engine {
    fn name(&self) -> &'static str {
        "p533"
    }

    fn predict(&self, dx: (f64, f64), from_unix: i64, wx: &SpaceWx) -> PathPrediction {
        self.predict_on(dx, Route::Short, from_unix, wx)
    }

    fn predict_route(
        &self,
        dx: (f64, f64),
        route: Route,
        from_unix: i64,
        wx: &SpaceWx,
    ) -> Option<PathPrediction> {
        // The long route of two co-located stations is the whole circle, which has no
        // direction; there is nothing to model.
        if route == Route::Long {
            let me = self.me?;
            if geo::haversine_km(me, dx) < MIN_LONG_PATH_SPAN_KM {
                return None;
            }
        }
        Some(self.predict_on(dx, route, from_unix, wx))
    }
}

/// Below this short-path distance the long path is not offered (see `predict_route`).
const MIN_LONG_PATH_SPAN_KM: f64 = 10.0;

/// One band at one UTC hour on one route: what grading a logged contact needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandHour {
    /// Basic circuit reliability, 0..1 — the day prediction's `hourly[h]`.
    pub bcr: f32,
    /// The hour's median SNR (dB), when a mode carries the frequency — the day prediction's
    /// `detail[h].snr_db`.
    pub snr_db: Option<f32>,
}

impl P533Engine {
    /// The chain's system parameters from this engine's configuration.
    fn sys(&self) -> P533Params {
        P533Params {
            // Antenna gain rides on the TX power term (see P533Config.ant_gain_dbi).
            txpower: self.cfg.txpower_dbkw + self.cfg.ant_gain_dbi,
            bw_hz: 1.0,
            snrr: self.cfg.required_snr_dbhz,
            snrxxp: 90,
            man_made: self.cfg.man_made,
        }
    }

    /// [`Self::predict_route`]'s figures for ONE band at the UTC hour containing `at_unix`, at the
    /// sunspot number given: one run of the chain instead of a day's 240, and the same numbers the
    /// day prediction carries in that band's cell (same month, slot, frequency and parameters).
    /// `None` without an operator location, for a band the engine does not model, and for the
    /// long route of two co-located stations — the cases the day prediction has no cell for.
    pub fn band_hour(
        &self,
        dx: (f64, f64),
        route: Route,
        band: Band,
        at_unix: i64,
        ssn: f64,
    ) -> Option<BandHour> {
        let me = self.me?;
        if band.is_vhf() {
            return None;
        }
        if route == Route::Long && geo::haversine_km(me, dx) < MIN_LONG_PATH_SPAN_KM {
            return None;
        }
        let day0 = at_unix - at_unix.rem_euclid(86_400);
        let (_, month1, _) = geo::civil_from_days(day0.div_euclid(86_400));
        let h = (at_unix.rem_euclid(86_400) / 3600) as usize;
        let slot = ((h + 23) % 24) as i32;
        let tx = Location::new(me.0.to_radians(), me.1.to_radians());
        let rx = Location::new(dx.0.to_radians(), dx.1.to_radians());
        let run = run_p533_route(
            tx,
            rx,
            route,
            (month1 - 1) as usize,
            slot,
            ssn,
            band_digital_mhz(band),
            &self.sys(),
        );
        Some(BandHour {
            bcr: (run.rel.bcr / 100.0).clamp(0.0, 1.0) as f32,
            snr_db: path_hour(&run).map(|p| p.snr_db),
        })
    }

    fn predict_on(
        &self,
        dx: (f64, f64),
        route: Route,
        from_unix: i64,
        wx: &SpaceWx,
    ) -> PathPrediction {
        // No operator anchor → empty outlooks, mirroring HeuristicEngine.
        let Some(me) = self.me else {
            return PathPrediction {
                engine: "p533".to_string(),
                bands: Vec::new(),
                muf_now: 0.0,
                muf_hourly: vec![0.0; 24],
                geometry: None,
                long_path: None,
                routes: None,
                day: None,
            };
        };

        let ssn = effective_ssn(wx);

        // The day containing the request, anchored to UTC midnight like the
        // heuristic (predict.rs) so heatmap x-axes align; month from that day.
        let day0 = from_unix - from_unix.rem_euclid(86_400);
        let (_, month1, _) = geo::civil_from_days(day0.div_euclid(86_400));
        let month0 = (month1 - 1) as usize;
        let now_hour = (from_unix.rem_euclid(86_400) / 3600) as usize;

        let tx = Location::new(me.0.to_radians(), me.1.to_radians());
        let rx = Location::new(dx.0.to_radians(), dx.1.to_radians());
        let sys = self.sys();

        let mut bands: Vec<BandOutlook> = Vec::new();
        let mut muf_hourly = vec![0.0f32; 24];
        for &band in Band::ALL.iter().filter(|b| !b.is_vhf()) {
            let freq = band_digital_mhz(band);
            let mut hourly = [0.0f32; 24];
            let mut detail: Vec<Option<PathHour>> = vec![None; 24];
            let mut mode_hourly: Vec<ModeHourly> = MODE_SNR_DBHZ
                .iter()
                .map(|&(mode, _)| ModeHourly {
                    mode: mode.to_string(),
                    hourly: [0.0; 24],
                })
                .collect();
            for (h, slot_score) in hourly.iter_mut().enumerate() {
                // UT hour h ↔ reference slot h−1 (slot i is UT i+1).
                let slot = ((h + 23) % 24) as i32;
                let run = run_p533_route(tx, rx, route, month0, slot, ssn, freq, &sys);
                *slot_score = (run.rel.bcr / 100.0).clamp(0.0, 1.0) as f32;
                detail[h] = path_hour(&run);
                // The MUF ceiling is per-path (band-independent); fill once.
                if band == Band::B20 {
                    muf_hourly[h] = run.path.opmuf as f32;
                }
                // Per-mode workability: the hour's SNR distribution is
                // mode-independent, so each mode is one closed-form BCR
                // re-evaluation against its required SNR — no chain re-run.
                // ALL hours are kept (mode_hourly) so a day-scale cache can
                // re-derive the "now" chips at serve time.
                for (mi, &(_, snrr)) in MODE_SNR_DBHZ.iter().enumerate() {
                    mode_hourly[mi].hourly[h] =
                        (bcr_for_required_snr(run.rel.snr, run.rel.du_sn, run.rel.dl_sn, snrr)
                            / 100.0)
                            .clamp(0.0, 1.0) as f32;
                }
            }
            let mode_now = mode_now_at(&mode_hourly, now_hour);
            let (window, grayline, peak, _) = best_window(&hourly, day0, band);
            let usable = hourly
                .iter()
                .filter(|&&s| s * 100.0 >= self.cfg.bcr_usable_pct as f32)
                .count();
            bands.push(BandOutlook {
                band: band.label().to_string(),
                workability: Workability::from_score(peak).label().to_string(),
                score: peak,
                window,
                grayline,
                hourly: hourly.to_vec(),
                reliability: (usable as f32 / 24.0) * 100.0,
                mode_now,
                mode_hourly,
                detail: Some(detail),
            });
        }
        bands.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        PathPrediction {
            engine: "p533".to_string(),
            bands,
            muf_now: muf_hourly[now_hour],
            muf_hourly,
            geometry: None,
            long_path: None,
            routes: None,
            day: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p533::run_p533;

    #[test]
    fn one_band_hour_is_the_day_predictions_cell() {
        // Boston–Sydney: both routes, every hour, two bands — the single run grading a logged
        // contact uses is the same number the path card and the day strip show for that cell.
        let eng = P533Engine::new(Some((42.36, -71.06)));
        let dx = (-33.87, 151.21);
        let day0 = 1_778_112_000; // 2026-05-07 00:00Z
        let ssn = 105.0;
        let wx = SpaceWx {
            ssn: Some(ssn as f32),
            ..Default::default()
        };
        for route in [Route::Short, Route::Long] {
            let day = eng.predict_route(dx, route, day0, &wx).unwrap();
            for band in [Band::B20, Band::B40] {
                let b = day.bands.iter().find(|b| b.band == band.label()).unwrap();
                for h in 0..24usize {
                    let one = eng
                        .band_hour(dx, route, band, day0 + h as i64 * 3600 + 1799, ssn)
                        .unwrap();
                    assert_eq!(one.bcr, b.hourly[h], "{route:?} {band:?} {h}");
                    let snr = b.detail.as_ref().unwrap()[h].map(|p| p.snr_db);
                    assert_eq!(one.snr_db, snr, "{route:?} {band:?} {h}");
                }
            }
        }
        // Nothing the day prediction has no cell for.
        assert!(eng
            .band_hour(dx, Route::Short, Band::B2, day0, ssn)
            .is_none());
        let here = (42.36, -71.06);
        assert!(eng
            .band_hour(here, Route::Long, Band::B20, day0, ssn)
            .is_none());
        assert!(P533Engine::new(None)
            .band_hour(dx, Route::Short, Band::B20, day0, ssn)
            .is_none());
    }

    #[test]
    fn covington_inversion_roundtrips() {
        for r in [0.0f64, 25.0, 70.0, 120.0, 160.0] {
            let sfi = 63.75 + 0.728 * r + 0.00089 * r * r;
            assert!((ssn_from_sfi(sfi) - r).abs() < 0.5, "R {r}");
        }
        // Quiet-sun floor clamps at 0.
        assert_eq!(ssn_from_sfi(50.0), 0.0);
    }

    #[test]
    fn engine_produces_a_sane_prediction() {
        // Chicago → Munich, May, SSN via a mid-cycle SFI.
        let eng = P533Engine::new(Some((41.98, -87.9)));
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        // ~2026-05-15 12:00 UT.
        let t = crate::geo::days_from_civil(2026, 5, 15) * 86_400 + 12 * 3600;
        let pred = eng.predict((48.35, 11.79), t, &wx);
        assert_eq!(pred.engine, "p533");
        assert_eq!(eng.name(), "p533");
        assert!(!pred.bands.is_empty());
        // HF only, sorted best-first, sane ranges.
        assert!(pred
            .bands
            .iter()
            .all(|b| !matches!(b.band.as_str(), "6m" | "4m" | "2m")));
        for w in pred.bands.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
        for b in &pred.bands {
            assert_eq!(b.hourly.len(), 24);
            assert!(b.hourly.iter().all(|&s| (0.0..=1.0).contains(&s)));
            assert!((0.0..=100.0).contains(&b.reliability));
        }
        // A mid-latitude 7 Mm path at SFI 150 should find a usable band.
        assert!(
            pred.bands.iter().any(|b| b.score >= 0.3),
            "{:?}",
            pred.bands
                .iter()
                .map(|b| (&b.band, b.score))
                .collect::<Vec<_>>()
        );
        assert_eq!(pred.muf_hourly.len(), 24);
        assert!(pred.muf_now > 0.0, "OPMUF ceiling at SFI 150");
        // No anchor → empty.
        let none = P533Engine::new(None).predict((48.35, 11.79), t, &wx);
        assert!(none.bands.is_empty());
    }

    /// Re-run the chain exactly as `predict` does for one band and UT hour.
    fn run_like_predict(
        me: (f64, f64),
        dx: (f64, f64),
        t: i64,
        sfi: f64,
        band: Band,
        h: usize,
    ) -> P533Run {
        let cfg = P533Config::default();
        let (_, month1, _) = geo::civil_from_days((t - t.rem_euclid(86_400)).div_euclid(86_400));
        let sys = P533Params {
            txpower: cfg.txpower_dbkw + cfg.ant_gain_dbi,
            bw_hz: 1.0,
            snrr: cfg.required_snr_dbhz,
            snrxxp: 90,
            man_made: cfg.man_made,
        };
        run_p533(
            Location::new(me.0.to_radians(), me.1.to_radians()),
            Location::new(dx.0.to_radians(), dx.1.to_radians()),
            (month1 - 1) as usize,
            ((h + 23) % 24) as i32,
            ssn_from_sfi(sfi),
            band_digital_mhz(band),
            &sys,
        )
    }

    fn band_of(label: &str) -> Band {
        *Band::ALL.iter().find(|b| b.label() == label).unwrap()
    }

    #[test]
    fn detail_is_the_runs_own_dominant_mode_on_a_short_path() {
        // Boston → London, ~5300 km: the short model, where §6 names a dominant mode.
        let (me, dx) = ((42.36, -71.06), (51.5, -0.12));
        let t = 1_778_846_400; // 2026-05-15 12:00 UTC
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        let pred = P533Engine::new(Some(me)).predict(dx, t, &wx);
        let mut checked = 0;
        let mut multi_hop_f2 = 0;
        for b in &pred.bands {
            let detail = b.detail.as_ref().expect("p533 fills detail");
            assert_eq!(detail.len(), 24);
            for (h, d) in detail.iter().enumerate() {
                let run = run_like_predict(me, dx, t, 150.0, band_of(&b.band), h);
                assert!(run.path.distance < 7000.0);
                // Independently from the run's internals, not via path_hour().
                let want = run.power.dominant.map(|dm| match dm {
                    Dominant::E(i) => (PathLayer::E, i as u8 + 1, run.path.md_e[i].ele),
                    Dominant::F2(i) => (PathLayer::F2, i as u8 + 1, run.path.md_f2[i].ele),
                });
                match (d, want) {
                    (Some(d), Some((layer, hops, ele))) => {
                        assert_eq!((d.layer, d.hops), (layer, hops), "{} {h}Z", b.band);
                        assert!(!d.estimated, "a §6 dominant mode is modelled, not assumed");
                        assert_eq!(d.takeoff_deg, ele.to_degrees() as f32);
                        // The §6 path elevation IS the dominant mode's.
                        assert_eq!(run.power.ele, ele);
                        assert!(
                            d.takeoff_deg > 0.0 && d.takeoff_deg < 45.0,
                            "{}",
                            d.takeoff_deg
                        );
                        if layer == PathLayer::F2 && hops >= 2 {
                            multi_hop_f2 += 1;
                        }
                        checked += 1;
                    }
                    (None, None) => {}
                    other => panic!("{} {h}Z: detail/run disagree: {other:?}", b.band),
                }
            }
        }
        assert!(checked > 100, "most band-hours carry a mode ({checked})");
        // 5300 km is beyond one F2 hop's ~4000 km reach: 2+ F2 hops must appear.
        assert!(multi_hop_f2 > 0);
    }

    #[test]
    fn detail_is_the_long_models_hop_geometry_beyond_7000_km() {
        // Boston → Tokyo, ~10 800 km: the long model, F2 hops of ≤ 4000 km.
        let (me, dx) = ((42.36, -71.06), (35.68, 139.69));
        let t = 1_796_277_600; // 2026-12-03 06:00 UTC
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        let pred = P533Engine::new(Some(me)).predict(dx, t, &wx);
        let b20 = pred.bands.iter().find(|b| b.band == "20m").unwrap();
        let run = run_like_predict(me, dx, t, 150.0, Band::B20, 6);
        assert!(run.path.distance > 9000.0);
        // Below 7000 km the short model's §6 picks the mode; out here it names none.
        assert_eq!(run.power.dominant, None);
        let d = b20.detail.as_ref().unwrap()[6].expect("long model always has geometry");
        assert_eq!(d.layer, PathLayer::F2);
        assert_eq!(d.hops as u32, run.long_hops);
        assert!(d.hops >= 3, "10 800 km in ≤ 4000 km hops: {}", d.hops);
        assert_eq!(d.takeoff_deg, run.long_ele.to_degrees() as f32);
        assert!(d.estimated, "beyond 7000 km the hop geometry is assumed");
        // Positive control on the source: the >9000 km composite elevation is the same angle.
        assert!((run.long_ele - path_ele_beyond_9000(&run)).abs() < 1e-12);
    }

    #[test]
    fn detail_in_the_7000_to_9000_km_blend_is_the_long_models_geometry() {
        // Chicago → Munich, ~7300 km: P.533 blends the short and long models here; the detail
        // must be the long model's assumed geometry, not a short-model dominant mode.
        let (me, dx) = ((41.88, -87.63), (48.35, 11.79));
        let t = 1_778_846_400; // 2026-05-15 12:00 UTC
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        let pred = P533Engine::new(Some(me)).predict(dx, t, &wx);
        let mut checked = 0;
        for b in &pred.bands {
            for (h, d) in b.detail.as_ref().unwrap().iter().enumerate() {
                let run = run_like_predict(me, dx, t, 150.0, band_of(&b.band), h);
                assert!((7000.0..9000.0).contains(&run.path.distance));
                let d = d.expect("the long model always has geometry");
                assert_eq!((d.layer, d.hops as u32), (PathLayer::F2, run.long_hops));
                assert_eq!(d.takeoff_deg, run.long_ele.to_degrees() as f32);
                assert!(d.estimated);
                checked += 1;
            }
        }
        assert_eq!(checked, pred.bands.len() * 24);
    }

    #[test]
    fn an_e_layer_dominant_mode_is_reported_as_e() {
        // Boston → Chicago, ~1400 km, a May day: one E hop carries the low bands at midday.
        let (me, dx) = ((42.36, -71.06), (41.88, -87.63));
        let t = 1_778_846_400; // 2026-05-15 12:00 UTC
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        let pred = P533Engine::new(Some(me)).predict(dx, t, &wx);
        let mut e_hours = 0;
        for b in &pred.bands {
            for (h, d) in b.detail.as_ref().unwrap().iter().enumerate() {
                let run = run_like_predict(me, dx, t, 150.0, band_of(&b.band), h);
                if let (Some(d), Some(Dominant::E(i))) = (d, run.power.dominant) {
                    assert_eq!(
                        (d.layer, d.hops),
                        (PathLayer::E, i as u8 + 1),
                        "{} {h}Z",
                        b.band
                    );
                    assert_eq!(d.takeoff_deg, run.path.md_e[i].ele.to_degrees() as f32);
                    e_hours += 1;
                }
            }
        }
        assert!(e_hours > 0, "a daytime 1400 km path has E-dominant hours");
    }

    /// The elevation P.533 §5.3 stores beyond 9000 km (`ele`), re-derived from the run's
    /// own fM geometry so the long-path detail is shown to be the chain's angle.
    fn path_ele_beyond_9000(run: &P533Run) -> f64 {
        let d_m = run.path.distance / run.long_hops as f64;
        super::super::muf::elevation_angle(d_m, 300.0)
    }

    #[test]
    fn carrying_detail_costs_no_extra_chain_runs() {
        let me = (42.36, -71.06);
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        super::super::RUNS.with(|c| c.set(0));
        let pred = P533Engine::new(Some(me)).predict((51.5, -0.12), 1_778_846_400, &wx);
        let runs = super::super::RUNS.with(|c| c.get());
        // One run per HF band per UT hour, exactly as before the detail existed.
        assert_eq!(runs, pred.bands.len() * 24);
        assert_eq!(pred.bands.len(), 10);
    }

    #[test]
    fn heuristic_and_ring_carry_no_detail() {
        let me = (42.36, -71.06);
        let wx = SpaceWx {
            sfi: 150.0,
            ..Default::default()
        };
        let h = crate::HeuristicEngine::new(Some(me)).predict((51.5, -0.12), 1_778_846_400, &wx);
        assert!(h.bands.iter().all(|b| b.detail.is_none()));
        let json = serde_json::to_string(&h).unwrap();
        assert!(!json.contains("\"detail\""), "heuristic JSON unchanged");
        // Positive control: the p533 JSON does carry it.
        let p = P533Engine::new(Some(me)).predict((51.5, -0.12), 1_778_846_400, &wx);
        assert!(serde_json::to_string(&p).unwrap().contains("\"detail\":["));
        let ring = crate::band_outlook_ring(
            &P533Engine::new(Some(me)),
            me,
            9000.0,
            2,
            1_778_846_400,
            &wx,
        );
        assert!(ring.bands.iter().all(|b| b.detail.is_none()));
    }
}
