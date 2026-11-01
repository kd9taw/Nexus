//! Polar-cap absorption (PCA) — the D-RAP "release 2" proton piece.
//!
//! During a solar proton event, ≥MeV protons spiral into the polar caps and
//! ionize the D region, wiping out HF paths that cross high geomagnetic
//! latitudes for hours-to-days (the part of a radio blackout the X-ray/flare
//! layer can't see: it persists through local night and lives at the poles).
//!
//! Model: Sauer & Wilkinson (2008), "Note on the disturbed reference ionosphere
//! model used by D-RAP" — the same formulation NOAA's D-RAP2 product uses:
//! - Day:   A_day(30 MHz)   = 0.115 · √J(E > 5.2 MeV)   dB (vertical-incidence)
//! - Night: A_night(30 MHz) = 0.020 · √J(E > 2.2 MeV)   dB
//! - Twilight: bilinear blend over solar elevation El ∈ [−10°, +10°]:
//!   A30 = A_day·(El+10)/20 − A_night·(El−10)/20
//! - Frequency scaling: A(f) = (30/f)^1.5 · A30
//!
//! APPROXIMATION (documented, honest): GOES integral-proton telemetry publishes
//! ≥1/≥5/≥10 MeV channels, not the model's 2.2/5.2 MeV integral thresholds. We
//! feed J(≥5 MeV) to the day term and J(≥1 MeV) to the night term — the nearest
//! available channels; this slightly over-estimates both (a conservative,
//! operator-safe bias: it warns a little early, never late).
//!
//! Geometry: protons only reach geomagnetic latitudes poleward of the Störmer
//! cutoff, which erodes equatorward as the field disturbs. v1 approximation:
//!     Λ(Kp) = 66° − 1.2°·Kp   (clamped ≥ 55°)
//! with a 5° cosine taper equatorward of Λ (full absorption poleward of Λ,
//! nothing below Λ−5°). Geomagnetic latitude is the centered-dipole value
//! ([`crate::geo::geomagnetic_lat_deg`]).
//!
//! Everything here is pure math over caller-supplied flux/Kp — the live GOES
//! fetch lives in [`crate::live::protons`]; no data is ever fabricated.

use serde::Serialize;

use crate::geo::{geomagnetic_lat_deg, solar_elevation_deg};

/// Day-side 30 MHz absorption (dB) for J(≥5 MeV) in pfu (cm⁻²·s⁻¹·sr⁻¹).
pub fn a30_day(j5: f64) -> f64 {
    0.115 * j5.max(0.0).sqrt()
}

/// Night-side 30 MHz absorption (dB) for J(≥1 MeV) in pfu.
pub fn a30_night(j1: f64) -> f64 {
    0.020 * j1.max(0.0).sqrt()
}

/// The 30 MHz absorption at a given solar elevation: day / night / the
/// Sauer-Wilkinson bilinear twilight blend between them.
pub fn a30_blend(j5: f64, j1: f64, el_deg: f64) -> f64 {
    let day = a30_day(j5);
    let night = a30_night(j1);
    if el_deg >= 10.0 {
        day
    } else if el_deg <= -10.0 {
        night
    } else {
        day * (el_deg + 10.0) / 20.0 - night * (el_deg - 10.0) / 20.0
    }
}

/// Scale a 30 MHz absorption to another HF frequency: A(f) = (30/f)^1.5 · A30.
pub fn scale_to_freq(a30_db: f64, freq_mhz: f64) -> f64 {
    a30_db * (30.0 / freq_mhz.max(1.0)).powf(1.5)
}

/// The polar-cap cutoff (geomagnetic latitude, degrees): Λ(Kp) = 66° − 1.2°·Kp,
/// clamped to ≥ 55° — the cap grows equatorward as the storm disturbs the field.
pub fn cutoff_lat_deg(kp: f64) -> f64 {
    (66.0 - 1.2 * kp.clamp(0.0, 9.0)).max(55.0)
}

/// How much of the full PCA applies at a geomagnetic latitude: 1 poleward of
/// the cutoff Λ, cosine-tapering to 0 at Λ−5° (both hemispheres).
pub fn cap_factor(geomag_lat_deg: f64, cutoff_deg: f64) -> f64 {
    let alat = geomag_lat_deg.abs();
    if alat >= cutoff_deg {
        1.0
    } else if alat <= cutoff_deg - 5.0 {
        0.0
    } else {
        let x = (cutoff_deg - alat) / 5.0; // 0 at Λ → 1 at Λ−5°
        0.5 * (1.0 + (std::f64::consts::PI * x).cos())
    }
}

/// Full composition: the PCA absorption (dB) at a geographic point and time,
/// for a given frequency, proton environment, and Kp.
pub fn absorption_db(
    lat: f64,
    lon: f64,
    unix: i64,
    freq_mhz: f64,
    j5: f64,
    j1: f64,
    kp: f64,
) -> f64 {
    let factor = cap_factor(geomagnetic_lat_deg(lat, lon), cutoff_lat_deg(kp));
    if factor <= 0.0 {
        return 0.0;
    }
    let a30 = a30_blend(j5, j1, solar_elevation_deg(lat, lon, unix));
    scale_to_freq(a30 * factor, freq_mhz)
}

/// NOAA's S-scale (solar radiation storm) from J(≥10 MeV) in pfu: S1 at 10, S2 at 100, … S5 at
/// 10⁵ and above (a huge finite flux saturates at S5); 0 below S1, and 0 for a non-finite flux (NaN
/// or ±∞ is no reading, so no event: fail closed).
pub fn s_scale(j10: f64) -> u8 {
    if !j10.is_finite() {
        return 0;
    }
    [1e5, 1e4, 1e3, 100.0, 10.0]
        .iter()
        .position(|&t| j10 >= t)
        .map_or(0, |i| 5 - i as u8)
}

/// NOAA's rule of thumb (the D-RAP documentation) for how long a proton event's polar-cap
/// absorption lasts, from the J(≥10 MeV) flux: `24.235·log10(J/15)` hours. `None` at or below
/// 15 pfu, where the rule gives nothing (zero or negative hours) — never a made-up figure.
pub fn sep_recovery_hours(j10: f64) -> Option<f64> {
    (j10 > 15.0).then(|| 24.235 * (j10 / 15.0).log10())
}

/// How old a GOES proton sample may be and still say a proton event is on (the product is
/// every 5 minutes; the shack asks every 5).
pub const PROTON_FRESH_SECS: i64 = 30 * 60;

/// How far ahead of the clock a proton sample may be dated and still be taken (clock skew between
/// the shack and NOAA); further ahead it is refused. The path overlay's `DRAP_AHEAD_SECS`.
pub const PROTON_AHEAD_SECS: i64 = 120;

/// Where a proton reading stands against the rule ([`proton_event`]), and so what the map may say:
/// held, or the reason it is not. A reading that fails more than one test takes the first of
/// undated, ahead, stale, then quiet: a reading whose time is wrong or old says nothing about the
/// S level now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtonState {
    /// A proton event is held now: S1+, dated, age in −[`PROTON_AHEAD_SECS`]..[`PROTON_FRESH_SECS`].
    Held,
    /// The sample carries no time.
    Undated,
    /// Dated more than [`PROTON_AHEAD_SECS`] ahead of the clock.
    Ahead,
    /// [`PROTON_FRESH_SECS`] old or more.
    Stale,
    /// Current, but below S1 (or the flux is not a finite number): no proton event.
    Quiet,
}

/// [`ProtonState`] of a reading at `now`. The one place the rule is written.
pub fn proton_state(j10: f64, at: Option<i64>, now: i64) -> ProtonState {
    let Some(at) = at else {
        return ProtonState::Undated;
    };
    // Saturating: no sample time can overflow the age.
    let age = now.saturating_sub(at);
    if age < -PROTON_AHEAD_SECS {
        ProtonState::Ahead
    } else if age >= PROTON_FRESH_SECS {
        ProtonState::Stale
    } else if s_scale(j10) < 1 {
        ProtonState::Quiet
    } else {
        ProtonState::Held
    }
}

/// THE rule for "a proton event is held now", one for the map and the path overlay: S1+ from a
/// finite J(≥10 MeV) ([`s_scale`]), at a dated sample whose age is at least −[`PROTON_AHEAD_SECS`]
/// and under [`PROTON_FRESH_SECS`]. An undated, old, or too-far-future sample is no event (fail
/// closed); the age never overflows.
pub fn proton_event(j10: f64, at: Option<i64>, now: i64) -> bool {
    proton_state(j10, at, now) == ProtonState::Held
}

/// When a held reading stops being held: its sample time plus [`PROTON_FRESH_SECS`] (saturating).
/// The map stops drawing the event at this time on its own clock, without a new answer from the
/// shack — so a reading kept through failed polls still ends at 30 minutes.
pub fn proton_event_until(at: i64) -> i64 {
    at.saturating_add(PROTON_FRESH_SECS)
}

/// The differential case table for [`proton_event`]: every caller that decides "a proton event is
/// held" (the polar-cap layer, the path overlay) runs its own rule over these and must agree.
/// `(name, j10, age in seconds (None = undated), held)`; the sample time is `now − age`, saturating.
#[cfg(test)]
pub(crate) mod cases {
    pub const PROTON_EVENT_CASES: &[(&str, f64, Option<i64>, bool)] = &[
        ("fresh S1 (10 pfu, 5 min)", 10.0, Some(300), true),
        ("fresh S2 (150 pfu, 5 min)", 150.0, Some(300), true),
        ("S2 just now", 150.0, Some(0), true),
        ("S2 at 30 min less 1 s", 150.0, Some(30 * 60 - 1), true),
        ("S2 at exactly 30 min", 150.0, Some(30 * 60), false),
        ("S2 6 h old", 150.0, Some(6 * 3600), false),
        (
            "S2 dated 120 s ahead (the skew limit)",
            150.0,
            Some(-120),
            true,
        ),
        ("S2 dated 121 s ahead", 150.0, Some(-121), false),
        (
            "S2 dated 3 min ahead (clock skew)",
            150.0,
            Some(-180),
            false,
        ),
        ("S2 undated", 150.0, None, false),
        ("quiet 0.3 pfu, fresh", 0.3, Some(300), false),
        ("sub-S1 9.99 pfu, fresh", 9.99, Some(300), false),
        ("5 pfu, fresh", 5.0, Some(300), false),
        ("NaN flux, fresh", f64::NAN, Some(300), false),
        ("+inf flux, fresh", f64::INFINITY, Some(300), false),
        ("-inf flux, fresh", f64::NEG_INFINITY, Some(300), false),
        ("huge finite flux (1e300), fresh", 1e300, Some(300), true),
        ("f64::MAX flux, fresh", f64::MAX, Some(300), true),
        (
            "age i64::MAX (dated at the far past)",
            150.0,
            Some(i64::MAX),
            false,
        ),
        (
            "age i64::MIN (dated at the far future)",
            150.0,
            Some(i64::MIN),
            false,
        ),
    ];
}

/// Bearing step (degrees) round the cap edge.
const EDGE_STEP_DEG: usize = 4;

/// The polar-cap edge at cutoff `cutoff_deg` (geomagnetic latitude): one closed `[lon, lat]`
/// ring per hemisphere (north, south) — the small circle that far from each centred-dipole
/// pole. The cap is poleward of it; the 5° taper ([`cap_factor`]) lies just outside.
pub fn cap_edge_rings(cutoff_deg: f64) -> [Vec<[f32; 2]>; 2] {
    let ring = |sign: f64| {
        let mut r: Vec<[f32; 2]> = (0..360)
            .step_by(EDGE_STEP_DEG)
            .map(|mlon| {
                let (lat, lon) =
                    crate::geo::geographic_from_geomagnetic(sign * cutoff_deg, mlon as f64);
                [lon as f32, lat as f32]
            })
            .collect();
        r.push(r[0]);
        r
    };
    [ring(1.0), ring(-1.0)]
}

/// One polar-shading sample for the map overlay: where, and the 30 MHz
/// absorption there (dB) — the UI maps dB → opacity.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcaPoint {
    pub lat: f32,
    pub lon: f32,
    pub db30: f32,
}

/// Sample the current PCA into a light map overlay: a 2°×4° grid over both
/// polar regions, keeping points whose 30 MHz absorption ≥ `min_db`. Empty when
/// the proton environment is quiet — the honest "draw nothing" state.
pub fn pca_layer(j5: f64, j1: f64, kp: f64, unix: i64, min_db: f64) -> Vec<PcaPoint> {
    // Quiet-sky short-circuit: if even the strongest case (day, pole) is below
    // the floor, there is nothing to draw.
    if a30_day(j5).max(a30_night(j1)) < min_db {
        return Vec::new();
    }
    let cutoff = cutoff_lat_deg(kp);
    let mut out = Vec::new();
    for hemi in [1.0f64, -1.0] {
        // Start just equatorward of the taper so the fade edge renders.
        let lat0 = (cutoff - 6.0).max(40.0);
        let mut lat = lat0;
        while lat <= 88.0 {
            let mut lon = -180.0f64;
            while lon < 180.0 {
                let (plat, plon) = (lat * hemi, lon);
                let factor = cap_factor(geomagnetic_lat_deg(plat, plon), cutoff);
                if factor > 0.0 {
                    let a30 = a30_blend(j5, j1, solar_elevation_deg(plat, plon, unix)) * factor;
                    if a30 >= min_db {
                        out.push(PcaPoint {
                            lat: plat as f32,
                            lon: plon as f32,
                            db30: a30 as f32,
                        });
                    }
                }
                lon += 4.0;
            }
            lat += 2.0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_night_endpoints_match_sauer_wilkinson() {
        // S2-ish event: J(≥5)=100 pfu → day 0.115·10 = 1.15 dB @30 MHz.
        assert!((a30_blend(100.0, 1000.0, 45.0) - 1.15).abs() < 1e-9);
        // Night uses the ≥1 MeV channel: 0.020·√1000 ≈ 0.632 dB.
        assert!((a30_blend(100.0, 1000.0, -45.0) - 0.632_455).abs() < 1e-3);
    }

    #[test]
    fn twilight_blends_between_day_and_night() {
        let day = a30_blend(400.0, 400.0, 10.0);
        let night = a30_blend(400.0, 400.0, -10.0);
        let mid = a30_blend(400.0, 400.0, 0.0);
        assert!((mid - (day + night) / 2.0).abs() < 1e-9);
        assert!(night < mid && mid < day);
    }

    #[test]
    fn frequency_scaling_is_inverse_power_1_5() {
        // 7.5 MHz = 30/4 → (4)^1.5 = 8× the 30 MHz value.
        assert!((scale_to_freq(1.0, 7.5) - 8.0).abs() < 1e-9);
        assert!((scale_to_freq(1.0, 30.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cutoff_erodes_with_kp_and_clamps() {
        assert!((cutoff_lat_deg(0.0) - 66.0).abs() < 1e-9);
        assert!((cutoff_lat_deg(5.0) - 60.0).abs() < 1e-9);
        assert!((cutoff_lat_deg(9.0) - 55.2).abs() < 1e-9); // 66−10.8, still above the 55° clamp
        assert!((cutoff_lat_deg(20.0) - 55.2).abs() < 1e-9); // Kp clamps at 9 first
    }

    #[test]
    fn cap_factor_tapers_over_five_degrees() {
        assert_eq!(cap_factor(70.0, 62.0), 1.0);
        assert_eq!(cap_factor(-70.0, 62.0), 1.0); // south cap too
        assert_eq!(cap_factor(50.0, 62.0), 0.0);
        let midway = cap_factor(59.5, 62.0);
        assert!((midway - 0.5).abs() < 1e-9); // cosine taper midpoint
    }

    #[test]
    fn the_s_scale_follows_noaas_thresholds() {
        let at = [
            (0.2, 0),
            (9.99, 0),
            (10.0, 1),
            (99.0, 1),
            (100.0, 2),
            (1e3, 3),
            (1e4, 4),
            (1e5, 5),
            (1e7, 5),
        ];
        for (j, s) in at {
            assert_eq!(s_scale(j), s, "J≥10 = {j}");
        }
        assert_eq!(s_scale(f64::NAN), 0);
    }

    #[test]
    fn sep_recovery_is_noaas_rule_and_says_nothing_below_its_floor() {
        // 24.235·log10(150/15) = 24.235 h; 1500 pfu → 48.47 h.
        assert!((sep_recovery_hours(150.0).unwrap() - 24.235).abs() < 1e-9);
        assert!((sep_recovery_hours(1500.0).unwrap() - 48.47).abs() < 1e-9);
        assert_eq!(sep_recovery_hours(15.0), None);
        assert_eq!(sep_recovery_hours(12.0), None);
    }

    /// The shared table: the rule's own answer for every case (sample time `now − age`, saturating).
    #[test]
    fn proton_event_agrees_with_the_differential_case_table() {
        let now = 1_800_000_000_i64;
        for &(name, j10, age, held) in cases::PROTON_EVENT_CASES {
            let at = age.map(|a| now.saturating_sub(a));
            assert_eq!(proton_event(j10, at, now), held, "{name}");
        }
        // The age itself never overflows, whatever the sample time.
        assert!(!proton_event(150.0, Some(i64::MIN), now));
        assert!(!proton_event(150.0, Some(i64::MAX), now));
        assert!(!proton_event(150.0, Some(i64::MIN), i64::MAX));
    }

    #[test]
    fn the_proton_state_names_why_a_reading_is_not_held() {
        let now = 1_800_000_000_i64;
        let st =
            |j10: f64, age: Option<i64>| proton_state(j10, age.map(|a| now.saturating_sub(a)), now);
        assert_eq!(st(150.0, Some(300)), ProtonState::Held);
        assert_eq!(st(150.0, None), ProtonState::Undated);
        assert_eq!(st(150.0, Some(-180)), ProtonState::Ahead, "3 min ahead");
        assert_eq!(
            st(150.0, Some(-120)),
            ProtonState::Held,
            "the skew allowance"
        );
        assert_eq!(st(150.0, Some(30 * 60)), ProtonState::Stale);
        assert_eq!(st(0.3, Some(300)), ProtonState::Quiet);
        assert_eq!(st(f64::NAN, Some(300)), ProtonState::Quiet);
        // a time problem is named before the S level: an old or future reading says nothing of now
        assert_eq!(st(0.3, Some(7200)), ProtonState::Stale);
        assert_eq!(st(0.3, Some(-600)), ProtonState::Ahead);
        assert_eq!(st(0.3, None), ProtonState::Undated);
        // the state and the event are one rule, case for case
        for &(name, j10, age, held) in cases::PROTON_EVENT_CASES {
            assert_eq!(st(j10, age) == ProtonState::Held, held, "{name}");
        }
        assert_eq!(proton_event_until(now), now + PROTON_FRESH_SECS);
        assert_eq!(proton_event_until(i64::MAX), i64::MAX);
    }

    #[test]
    fn the_s_scale_is_safe_at_the_edges_of_f64() {
        assert_eq!(s_scale(f64::NAN), 0);
        assert_eq!(s_scale(f64::INFINITY), 0, "a non-finite flux is no reading");
        assert_eq!(s_scale(f64::NEG_INFINITY), 0);
        assert_eq!(s_scale(1e300), 5, "a huge finite flux saturates at S5");
        assert_eq!(s_scale(f64::MAX), 5);
        assert_eq!(s_scale(-1.0), 0);
    }

    #[test]
    fn a_proton_event_needs_s1_and_a_fresh_dated_sample() {
        let now = 1_800_000_000;
        assert!(proton_event(12.0, Some(now - 300), now));
        assert!(!proton_event(9.0, Some(now - 300), now), "below S1");
        assert!(!proton_event(500.0, None, now), "undated");
        assert!(
            !proton_event(500.0, Some(now - PROTON_FRESH_SECS), now),
            "old"
        );
        assert!(proton_event(500.0, Some(now - PROTON_FRESH_SECS + 1), now));
    }

    #[test]
    fn the_cap_edge_is_the_cutoff_latitude_round_each_pole() {
        let [n, s] = cap_edge_rings(62.0);
        assert_eq!(n.first(), n.last());
        assert_eq!(n.len(), 360 / EDGE_STEP_DEG + 1);
        for p in &n {
            let m = crate::geo::geomagnetic_lat_deg(f64::from(p[1]), f64::from(p[0]));
            assert!((m - 62.0).abs() < 1e-3, "{p:?} at {m}");
        }
        for p in &s {
            let m = crate::geo::geomagnetic_lat_deg(f64::from(p[1]), f64::from(p[0]));
            assert!((m + 62.0).abs() < 1e-3, "{p:?} at {m}");
        }
    }

    #[test]
    fn quiet_sky_draws_nothing() {
        assert!(pca_layer(0.1, 1.0, 2.0, 1_760_000_000, 0.5).is_empty());
    }

    #[test]
    fn event_shades_the_polar_caps_only() {
        // S3: J(≥5)~1000 → day ~3.6 dB. Layer must exist and stay polar.
        let pts = pca_layer(1000.0, 10_000.0, 4.0, 1_760_000_000, 0.5);
        assert!(!pts.is_empty());
        assert!(pts.iter().all(|p| p.lat.abs() >= 40.0));
        assert!(pts.iter().any(|p| p.lat > 0.0) && pts.iter().any(|p| p.lat < 0.0));
        // Every kept point clears the floor.
        assert!(pts.iter().all(|p| p.db30 >= 0.5));
    }
}
