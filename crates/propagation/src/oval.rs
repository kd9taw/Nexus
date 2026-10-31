//! The aurora oval's equatorward edge, two ways, never mixed.
//!
//! **NOAA's** ([`ovation_edge`]): where NOAA OVATION's aurora probability first reaches
//! [`EDGE_PROB`] %, coming in from the equator, per [`EDGE_BIN_DEG`]° of longitude, in each
//! hemisphere. Read from the whole 1° grid (before the overlay's thresholding and thinning), so
//! the line is the forecast's own. A longitude band where OVATION never reaches it has no edge
//! there (`None`): the line is broken, never bridged by a guess.
//!
//! **Modelled from Kp** ([`kp_fit_oval`]): the fallback the map draws only when OVATION is stale.
//! The empirical boundary is Starkov's (1994) EQUATORWARD BOUNDARY OF THE DIFFUSE AURORA,
//!
//! ```text
//! θ = A0 + A1·cos[15(t + α1)] + A2·cos[15(2t + α2)] + A3·cos[15(3t + α3)]   (degrees, t = MLT hours)
//! A_i, α_i = b0 + b1·log10|AL| + b2·log10²|AL| + b3·log10³|AL|
//! AL = 18 − 12.3·Kp + 27.2·Kp² − 2·Kp³
//! ```
//!
//! θ is the geomagnetic co-latitude of the boundary. Sources:
//! - G. V. Starkov, "Mathematical model of the auroral boundaries", Geomagnetism and Aeronomy
//!   34, 331–336 (1994) — the boundary model;
//! - G. V. Starkov, "Statistical dependences between the magnetic activity indices",
//!   Geomagnetism and Aeronomy 34, 101–103 (1994) — the Kp → AL relation;
//! - the coefficients as tabulated by F. Sigernes et al., "Real time aurora oval forecasting —
//!   SvalTrackII", Óptica Pura y Aplicada 44(4), 599–603 (2011), Table 1, "Equatorward boundary
//!   of the diffuse aurora".
//!
//! WHY THE DIFFUSE BOUNDARY, not the oval's (the same table's middle block). OVATION's
//! probability includes the diffuse aurora. Against NOAA's own OVATION of 31 Oct 2026 23:23Z
//! (Kp 3.67 est.) the diffuse boundary sat within 3° of OVATION's 10 % edge in 30 of 42 longitude
//! bands (median 2°), the oval boundary in 13 (median 5°, all of it poleward). The test below
//! pins that file; `examples/oval_compare.rs` keeps measuring it against the live feed.
//!
//! APPROXIMATIONS, said wherever the line is drawn ("modelled from Kp"): Starkov's latitudes are
//! corrected geomagnetic; this uses the centred dipole ([`crate::geo::geomagnetic_latlon`]), the
//! same one the polar-cap edge uses, which can sit a few degrees off CGM. The southern edge is
//! taken as the northern one's magnetic conjugate. Kp is clamped to 0..9.

use serde::Serialize;
use serde_json::Value;

use crate::geo::{geographic_from_geomagnetic, geomagnetic_latlon, magnetic_local_time};

/// OVATION's probability (%) that marks the oval's equatorward edge.
pub const EDGE_PROB: f32 = 10.0;
/// The edge is one latitude per this many degrees of longitude.
pub const EDGE_BIN_DEG: f64 = 15.0;
const EDGE_BINS: usize = 24;

/// The centre longitude of edge bin `i` (bin 0 is −180…−165, so its centre is −172.5).
pub fn edge_bin_lon(i: usize) -> f64 {
    -180.0 + EDGE_BIN_DEG * (i as f64 + 0.5)
}

/// NOAA OVATION's equatorward edge: per longitude bin ([`edge_bin_lon`]), the latitude nearest
/// the equator where the probability reaches [`EDGE_PROB`] (south as a negative latitude),
/// `None` where it never does.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OvalEdge {
    pub north: Vec<Option<f32>>,
    pub south: Vec<Option<f32>>,
}

/// Read OVATION's equatorward edge from the whole `coordinates` grid
/// (`[lon 0..359, lat −90..90, probability]`). Pure.
pub fn ovation_edge(v: &Value) -> OvalEdge {
    let mut north: Vec<Option<f32>> = vec![None; EDGE_BINS];
    let mut south: Vec<Option<f32>> = vec![None; EDGE_BINS];
    let Some(coords) = v.get("coordinates").and_then(Value::as_array) else {
        return OvalEdge { north, south };
    };
    for c in coords {
        let Some(t) = c.as_array().filter(|t| t.len() >= 3) else {
            continue;
        };
        let (Some(lon), Some(lat), Some(p)) = (t[0].as_f64(), t[1].as_f64(), t[2].as_f64()) else {
            continue;
        };
        if (p as f32) < EDGE_PROB || lat == 0.0 {
            continue;
        }
        let lon = (lon + 540.0).rem_euclid(360.0) - 180.0;
        let bin = (((lon + 180.0) / EDGE_BIN_DEG).floor() as usize).min(EDGE_BINS - 1);
        let (side, lat) = if lat > 0.0 {
            (&mut north, lat as f32)
        } else {
            (&mut south, lat as f32)
        };
        if side[bin].is_none_or(|k| lat.abs() < k.abs()) {
            side[bin] = Some(lat);
        }
    }
    OvalEdge { north, south }
}

/// Starkov's equatorward boundary of the diffuse aurora: b0..b3 (rows) for A0, A1, A2, A3, α1,
/// α2, α3 (columns), exactly as Sigernes et al. (2011) Table 1 prints them.
const STARKOV_DIFFUSE: [[f64; 7]; 4] = [
    [3.44, -2.41, -0.74, -2.12, -1.68, 8.69, 8.61],
    [29.77, 7.89, 3.94, 3.24, -2.48, -20.73, -5.34],
    [-16.38, -4.32, -3.09, -1.67, 1.58, 13.03, -1.36],
    [3.35, 0.87, 0.72, 0.31, -0.28, -2.14, 0.76],
];

/// Starkov's Kp → AL (nT, as a magnitude): `18 − 12.3·Kp + 27.2·Kp² − 2·Kp³`.
pub fn kp_to_al(kp: f64) -> f64 {
    let k = kp.clamp(0.0, 9.0);
    18.0 - 12.3 * k + 27.2 * k * k - 2.0 * k * k * k
}

/// The diffuse aurora's equatorward boundary (geomagnetic latitude, degrees) at magnetic local
/// time `mlt` (hours) for `kp`.
pub fn diffuse_edge_mlat(kp: f64, mlt: f64) -> f64 {
    let l = kp_to_al(kp).abs().log10();
    let c: Vec<f64> = (0..7)
        .map(|j| {
            let b = |i: usize| STARKOV_DIFFUSE[i][j];
            b(0) + b(1) * l + b(2) * l * l + b(3) * l * l * l
        })
        .collect();
    let cosd = |deg: f64| deg.to_radians().cos();
    let theta = c[0]
        + c[1] * cosd(15.0 * (mlt + c[4]))
        + c[2] * cosd(15.0 * (2.0 * mlt + c[5]))
        + c[3] * cosd(15.0 * (3.0 * mlt + c[6]));
    90.0 - theta
}

/// The Kp-fit edge as the map draws it: the Kp it was modelled from, that Kp's own sample time,
/// and the two rings ([`kp_fit_oval`]). Built by the command layer only from a measured Kp.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KpFitOval {
    pub kp: f32,
    pub kp_at: i64,
    pub north: Vec<[f32; 2]>,
    pub south: Vec<[f32; 2]>,
}

/// [`kp_fit_oval`] with its inputs, for the map.
pub fn kp_fit(kp: f32, kp_at: i64, unix: i64) -> KpFitOval {
    let (north, south) = kp_fit_oval(f64::from(kp), unix);
    KpFitOval {
        kp,
        kp_at,
        north,
        south,
    }
}

/// Points per Kp-fit ring (every 15 minutes of magnetic local time).
const FIT_STEPS: usize = 96;

/// The Kp-fit edge at `unix`, as two closed `[lon, lat]` rings (north, south): the boundary
/// walked round in magnetic local time and turned into geographic coordinates.
pub fn kp_fit_oval(kp: f64, unix: i64) -> (Vec<[f32; 2]>, Vec<[f32; 2]>) {
    // The magnetic longitude of magnetic noon, so MLT t sits at noon + 15·(t − 12)°.
    let noon = {
        let (slat, slon) = crate::geo::subsolar_point(unix);
        geomagnetic_latlon(slat, slon).1
    };
    let mut north = Vec::with_capacity(FIT_STEPS + 1);
    let mut south = Vec::with_capacity(FIT_STEPS + 1);
    for s in 0..FIT_STEPS {
        let mlt = 24.0 * s as f64 / FIT_STEPS as f64;
        let mlat = diffuse_edge_mlat(kp, mlt);
        let mlon = noon + 15.0 * (mlt - 12.0);
        for (ring, m) in [(&mut north, mlat), (&mut south, -mlat)] {
            let (lat, lon) = geographic_from_geomagnetic(m, mlon);
            ring.push([lon as f32, lat as f32]);
        }
    }
    north.push(north[0]);
    south.push(south[0]);
    (north, south)
}

/// The Kp-fit edge's geographic latitude at one longitude (the lowest |latitude| on or poleward
/// of the boundary, in 0.1° steps), north or south; `None` if none is found. The comparison
/// with OVATION's per-longitude edge uses it.
pub fn kp_fit_lat_at(kp: f64, lon: f64, north: bool, unix: i64) -> Option<f64> {
    let sign = if north { 1.0 } else { -1.0 };
    (300..=900)
        .map(|a| sign * f64::from(a) / 10.0)
        .find(|&lat| {
            let (mlat, mlon) = geomagnetic_latlon(lat, lon);
            mlat * sign >= diffuse_edge_mlat(kp, magnetic_local_time(mlon, unix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Starkov's Kp → AL, and the boundary at four magnetic local times, against values worked
    /// independently from the published table.
    #[test]
    fn the_fit_reproduces_the_published_formula() {
        assert!((kp_to_al(3.0) - 171.9).abs() < 1e-9);
        assert!((kp_to_al(6.0) - 491.4).abs() < 1e-9);
        let cases = [
            (0.0, [65.944, 67.493, 69.339, 70.722]),
            (3.0, [61.138, 62.929, 66.950, 66.757]),
            (6.0, [55.411, 57.453, 63.796, 62.449]),
            (9.0, [53.047, 55.047, 62.474, 60.776]),
        ];
        for (kp, want) in cases {
            for (mlt, w) in [0.0, 6.0, 12.0, 18.0].iter().zip(want) {
                let got = diffuse_edge_mlat(kp, *mlt);
                assert!((got - w).abs() < 0.01, "Kp {kp} MLT {mlt}: {got} vs {w}");
            }
        }
    }

    #[test]
    fn the_fit_comes_south_with_kp_and_furthest_at_midnight() {
        for mlt in [0.0, 6.0, 12.0, 18.0] {
            assert!(diffuse_edge_mlat(6.0, mlt) < diffuse_edge_mlat(2.0, mlt));
        }
        assert!(diffuse_edge_mlat(4.0, 0.0) < diffuse_edge_mlat(4.0, 12.0));
        // Kp beyond the scale is the scale's end, not an extrapolation.
        assert_eq!(diffuse_edge_mlat(12.0, 0.0), diffuse_edge_mlat(9.0, 0.0));
    }

    #[test]
    fn the_fit_rings_close_and_sit_at_the_boundary() {
        let unix = 1_793_488_980;
        let (n, s) = kp_fit_oval(4.0, unix);
        assert_eq!(n.len(), FIT_STEPS + 1);
        assert_eq!(n.first(), n.last());
        assert!(n.iter().all(|p| p[1] > 40.0) && s.iter().all(|p| p[1] < -40.0));
        // Every point is on the boundary at its own magnetic local time.
        for p in n.iter().chain(&s) {
            let (mlat, mlon) = geomagnetic_latlon(f64::from(p[1]), f64::from(p[0]));
            let want = diffuse_edge_mlat(4.0, magnetic_local_time(mlon, unix));
            assert!((mlat.abs() - want).abs() < 0.01, "{p:?}: {mlat} vs {want}");
        }
    }

    #[test]
    fn the_ovation_edge_is_the_first_ten_percent_cell_from_the_equator() {
        let v = json!({"coordinates": [
            [10.0, 60.0, 9.0],   // below the edge probability
            [10.0, 62.0, 10.0],  // the edge in bin (−180+190)/15 = 12
            [11.0, 66.0, 40.0],  // same bin, further poleward
            [200.0, -55.0, 30.0], // lon −160 → bin 1, south
            [200.0, -70.0, 50.0],
            [100.0, 0.0, 90.0],  // the equator is neither hemisphere
        ]});
        let e = ovation_edge(&v);
        assert_eq!(e.north[12], Some(62.0));
        assert_eq!(e.south[1], Some(-55.0));
        assert_eq!(e.north.iter().flatten().count(), 1);
        assert_eq!(e.south.iter().flatten().count(), 1);
        assert_eq!(edge_bin_lon(12), 7.5);
        assert!(ovation_edge(&json!({})).north.iter().all(Option::is_none));
    }

    /// NOAA's OVATION of 31 Oct 2026 (forecast for 23:23Z; Kp 3.67 estimated when its input was
    /// observed, 22:28Z). The cells at ≥ 5 % are kept, so the 10 % edge is the file's own.
    #[test]
    fn the_fit_tracks_a_real_ovation_edge() {
        let v: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/swpc-ovation-aurora-2026-10-31-ge5.json"
        ))
        .unwrap();
        let unix = crate::kc2g::parse_naive_utc_unix("2026-10-31T23:23:00Z").unwrap();
        let edge = ovation_edge(&v);
        let mut diffs = Vec::new();
        for (side, north) in [(&edge.north, true), (&edge.south, false)] {
            for (i, lat) in side.iter().enumerate() {
                let (Some(lat), Some(fit)) =
                    (lat, kp_fit_lat_at(3.67, edge_bin_lon(i), north, unix))
                else {
                    continue;
                };
                diffs.push((fit.abs() - f64::from(lat.abs())).abs());
            }
        }
        diffs.sort_by(f64::total_cmp);
        let within = diffs.iter().filter(|d| **d <= 3.0).count();
        assert_eq!(diffs.len(), 42, "OVATION's edge exists in 42 of 48 bands");
        assert!(within >= 28, "{within} of {} bands within 3°", diffs.len());
        assert!(
            diffs[diffs.len() / 2] <= 3.0,
            "median {}",
            diffs[diffs.len() / 2]
        );
    }
}
