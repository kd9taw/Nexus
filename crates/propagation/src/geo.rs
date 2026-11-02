//! Geo helpers the detector needs: Maidenhead grid → lat/lon, great-circle
//! distance, bearing, and the time-of-day encoding.
//!
//! weak-signal-sleuth imported `maidenheadToLatLon` / `haversineKm` from
//! `./utils` but only `timeOfDaySinCos` was actually defined there (so the
//! geo-spread feature was effectively dead at runtime). These are the correct,
//! standard implementations its code intended — same haversine (R = 6371 km)
//! and the same time-of-day sin/cos encoding, ported to Rust.

use std::f64::consts::PI;

/// Maidenhead locator → (lat, lon) at the **center** of the grid square.
/// Accepts 4- or 6-character locators; returns `None` if malformed.
pub fn maidenhead_to_latlon(grid: &str) -> Option<(f64, f64)> {
    let g = grid.trim().to_uppercase();
    let b = g.as_bytes();
    if b.len() < 4 {
        return None;
    }
    // Field (A–R): 20° lon, 10° lat.
    let f_lon = b[0].checked_sub(b'A')? as f64;
    let f_lat = b[1].checked_sub(b'A')? as f64;
    if f_lon > 17.0 || f_lat > 17.0 {
        return None;
    }
    // Square (0–9): 2° lon, 1° lat.
    if !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return None;
    }
    let s_lon = (b[2] - b'0') as f64;
    let s_lat = (b[3] - b'0') as f64;
    let mut lon = -180.0 + f_lon * 20.0 + s_lon * 2.0;
    let mut lat = -90.0 + f_lat * 10.0 + s_lat * 1.0;
    if b.len() >= 6 {
        // Subsquare (A–X, already uppercased): 5′ lon, 2.5′ lat. Add half a
        // subsquare to land at the center.
        let ss_lon = b[4].checked_sub(b'A')? as f64;
        let ss_lat = b[5].checked_sub(b'A')? as f64;
        if ss_lon > 23.0 || ss_lat > 23.0 {
            return None;
        }
        lon += ss_lon * (5.0 / 60.0) + (2.5 / 60.0);
        lat += ss_lat * (2.5 / 60.0) + (1.25 / 60.0);
    } else {
        // Center of the 2° × 1° square.
        lon += 1.0;
        lat += 0.5;
    }
    Some((lat, lon))
}

/// (lat, lon) → 4-character Maidenhead grid (square center precision). Inverse
/// of [`maidenhead_to_latlon`] at 4-char resolution — used to give a grid to a
/// DXCC-resolved location so the rest of the pipeline (distance/bearing) works.
pub fn latlon_to_maidenhead(lat: f64, lon: f64) -> String {
    let lon = (lon + 180.0).clamp(0.0, 359.999);
    let lat = (lat + 90.0).clamp(0.0, 179.999);
    let f_lon = (lon / 20.0) as u8;
    let f_lat = (lat / 10.0) as u8;
    let s_lon = ((lon % 20.0) / 2.0) as u8;
    let s_lat = ((lat % 10.0) / 1.0) as u8;
    let mut s = String::with_capacity(4);
    s.push((b'A' + f_lon) as char);
    s.push((b'A' + f_lat) as char);
    s.push((b'0' + s_lon) as char);
    s.push((b'0' + s_lat) as char);
    s
}

/// Great-circle distance (km) between two (lat, lon) points. Mirrors
/// weak-signal-sleuth's haversine (Earth radius 6371 km).
pub fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6371.0;
    let (lat1, lon1) = a;
    let (lat2, lon2) = b;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let h = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Destination point (lat, lon in degrees) reached from `origin` by travelling
/// `dist_km` along the great circle on initial `bearing_deg` (0 = N). Spherical
/// earth; longitude normalized to [-180, 180]. Inverse of [`bearing_deg`] +
/// [`haversine_km`]. Used to synthesize representative DX points for the
/// no-selection modeled band outlook.
pub fn destination_point(origin: (f64, f64), bearing_deg: f64, dist_km: f64) -> (f64, f64) {
    const R: f64 = 6371.0;
    let ang = dist_km / R; // angular distance (radians)
    let brg = bearing_deg.to_radians();
    let lat1 = origin.0.to_radians();
    let lon1 = origin.1.to_radians();
    let lat2 = (lat1.sin() * ang.cos() + lat1.cos() * ang.sin() * brg.cos()).asin();
    let lon2 =
        lon1 + (brg.sin() * ang.sin() * lat1.cos()).atan2(ang.cos() - lat1.sin() * lat2.sin());
    let lon_deg = ((lon2.to_degrees() + 540.0).rem_euclid(360.0)) - 180.0;
    (lat2.to_degrees(), lon_deg)
}

/// Initial great-circle bearing (degrees, 0 = N) from `a` to `b`.
pub fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let dlon = lon2 - lon1;
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Distance (km) between two Maidenhead grids, or `None` if either is malformed.
pub fn grid_distance_km(a: &str, b: &str) -> Option<f64> {
    Some(haversine_km(
        maidenhead_to_latlon(a)?,
        maidenhead_to_latlon(b)?,
    ))
}

/// A distance inside a sentence this crate writes for the screen: `{km:26.4}` where the sentence
/// would have said "26 km". The UI writes it in the operator's units (`fmtKmTokens`,
/// `ui/src/units.ts`), so the backend never chooses between miles and kilometres and a sentence
/// cannot disagree with the Units setting every other distance on the screen follows. The full
/// precision crosses: rounding here and again into miles would put some figures a mile out.
pub fn km_token(km: f64) -> String {
    format!("{{km:{km}}}")
}

/// [`km_token`] for a span of distances, `{km:500-2500}`, which the UI writes "500–2500 km" or
/// "311–1553 mi".
pub fn km_range_token(lo_km: f64, hi_km: f64) -> String {
    format!("{{km:{lo_km}-{hi_km}}}")
}

/// Whether a string is a Maidenhead square a QSO record could carry — exactly
/// 4, 6 or 8 characters of the locator alphabet. The Rust twin of
/// `ui/src/grid.ts`'s `isValidLoggedGrid`, and deliberately that rule rather
/// than the operator's-own-square rule: 8-character squares are what the
/// VHF/microwave and satellite operators who exchange grids at all actually
/// pass, and the places this gates are places the operator was GIVEN a square.
///
/// ⭐ THE LENGTH CHECK IS LOAD-BEARING and is the whole reason this exists
/// beside [`maidenhead_to_latlon`], which is a permissive locator *parser*:
/// it accepts any string of length ≥ 4 whose first four characters fit and
/// simply ignores the rest, so `is_some()` is not a grid test. A two-letter
/// prefix with a TWO-digit number — the shape a special-event callsign takes,
/// `OL20ABC` — clears every character class and is seven characters long, so
/// only the length rejects it. Asking "grid or callsign?" with the parser
/// would have sent that operator's sked request off to look up a grid square
/// in Czechia.
pub fn is_logged_grid(s: &str) -> bool {
    let b = s.trim().as_bytes();
    if !matches!(b.len(), 4 | 6 | 8) {
        return false;
    }
    let in_range = |i: usize, lo: u8, hi: u8| (lo..=hi).contains(&b[i].to_ascii_uppercase());
    in_range(0, b'A', b'R')
        && in_range(1, b'A', b'R')
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && (b.len() < 6 || (in_range(4, b'A', b'X') && in_range(5, b'A', b'X')))
        && (b.len() < 8 || (b[6].is_ascii_digit() && b[7].is_ascii_digit()))
}

/// Where one source puts a station: a locator it sent or was logged with, or a position a
/// callbook vouched for. What a rotator's point-at-call aims at, in place of the station's
/// country (a country centre is off by 20° from the Netherlands to Galicia).
#[derive(Debug, Clone, PartialEq)]
pub enum StationFix {
    /// A Maidenhead locator, 4, 6 or 8 characters ([`is_logged_grid`]).
    Grid(String),
    /// Latitude and longitude, in degrees.
    Position(f64, f64),
}

impl StationFix {
    /// The point a bearing is taken to: a locator's centre, or the position itself. `None` for
    /// a locator that is not one, or a position off the globe.
    pub fn latlon(&self) -> Option<(f64, f64)> {
        match self {
            StationFix::Grid(g) if is_logged_grid(g) => maidenhead_to_latlon(g),
            StationFix::Grid(_) => None,
            StationFix::Position(lat, lon) => (lat.is_finite()
                && lon.is_finite()
                && (-90.0..=90.0).contains(lat)
                && (-180.0..=180.0).contains(lon))
            .then_some((*lat, *lon)),
        }
    }

    /// How closely this pins the station down: a position, then a 6- or 8-character locator
    /// (read to 6: [`maidenhead_to_latlon`] stops at the subsquare), then a 4-character square.
    fn precision(&self) -> u8 {
        match self {
            StationFix::Position(..) => 3,
            StationFix::Grid(g) if g.trim().len() >= 6 => 2,
            StationFix::Grid(_) => 1,
        }
    }

    /// Does this fix lie inside `wider`'s square? Only a locator has an area to lie inside.
    fn within(&self, wider: &StationFix) -> bool {
        let StationFix::Grid(g) = wider else {
            return false;
        };
        let (Some((lat, lon)), Some((clat, clon))) = (self.latlon(), wider.latlon()) else {
            return false;
        };
        // The square's own size about its centre: 1° × 2° for four characters, 2.5′ × 5′ for six.
        let (h, w) = if g.trim().len() >= 6 {
            (2.5 / 60.0, 5.0 / 60.0)
        } else {
            (1.0, 2.0)
        };
        (lat - clat).abs() <= h / 2.0 && (lon - clon).abs() <= w / 2.0
    }
}

/// The best of `fixes`, which come MOST TRUSTED FIRST: the first usable one, replaced by a more
/// precise one only where that one lies inside it.
///
/// ⚠️ **Precision alone is the wrong rule.** A 6-character square logged in 2019 is finer than
/// the 4-character square a station sends today, and wrong when the callsign has moved (a
/// DXpedition call used again from another island, an operator portable away from the square
/// their callbook gives). So a finer fix may only REFINE the fix that is trusted more, never
/// overrule it: a callbook position inside the square the station is sending sharpens the aim,
/// a callbook position somewhere else is ignored.
pub fn best_fix(fixes: impl IntoIterator<Item = StationFix>) -> Option<StationFix> {
    let mut best: Option<StationFix> = None;
    for fix in fixes {
        if fix.latlon().is_none() {
            continue;
        }
        best = match best {
            None => Some(fix),
            Some(b) if fix.precision() > b.precision() && fix.within(&b) => Some(fix),
            keep => keep,
        };
    }
    best
}

/// Compass octant (N, NE, …) for a bearing in degrees — for plain-language
/// "point NW" guidance.
pub fn compass_octant(bearing: f64) -> &'static str {
    const DIRS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = (((bearing % 360.0) + 360.0) % 360.0 / 45.0).round() as usize % 8;
    DIRS[idx]
}

/// Time-of-day sin/cos encoding (UTC), ported from weak-signal-sleuth's
/// `timeOfDaySinCos`: angle = 2π · (minutes-of-day / 1440).
pub fn time_of_day_sin_cos(now_unix: i64) -> (f32, f32) {
    let minutes = (now_unix.rem_euclid(86_400) / 60) as f64;
    let ang = 2.0 * PI * (minutes / 1440.0);
    (ang.sin() as f32, ang.cos() as f32)
}

// --- Solar geometry & path sampling (for the contact-likelihood model) ---
//
// Engineering-grade solar position (NOAA/PV forms): good to ~±0.5° — far more
// than propagation resolution needs. Gives day/night, twilight (greyline), and
// the solar zenith that drives MUF (F2 ionization) and D-layer absorption.

/// Hinnant's civil-from-days: days since the Unix epoch → (year, month, day).
pub(crate) fn civil_from_days(z0: i64) -> (i64, u32, u32) {
    let z = z0 + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Day-of-year (1–366) for a Unix timestamp (UTC).
pub fn day_of_year(unix: i64) -> i64 {
    let z = unix.div_euclid(86_400);
    let (y, _, _) = civil_from_days(z);
    z - days_from_civil(y, 1, 1) + 1
}

// ── THE SUN: one ephemeris for the shack and the map ─────────────────────────────────────────
// The Astronomical Almanac's low-precision Sun (The Astronomical Almanac for the Year 2010, p. C5:
// about 0.01° from 1950 to 2050) for the apparent right ascension and declination, and IAU 1982
// Greenwich mean sidereal time (Meeus, Astronomical Algorithms, 2nd ed., eq. 12.4) for where on
// Earth that is overhead. `ui/src/mapGeo.ts sunAt` is the same arithmetic, line for line, and the
// two are held to one table of values (tests here and in `mapGeo.sky.test.ts`), so the
// terminator, the greyline pane, the path classifier, the flare footprint and the sun on the map
// all stand on one sun. Measured against JPL Horizons (DE441) it is 0.0063° worst over September
// 2026; the Cooper declination and day-of-year equation of time it replaces were 1.08° off near
// the equinox. P.533 keeps the ITU reference implementation's own sun (`p533::solar`): that is a
// port held to the reference for parity, and it models the 15th of the month.

/// The sun at a UTC instant, as seen from the Earth's centre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunPosition {
    /// Apparent declination (degrees): the latitude where the sun is overhead.
    pub decl_deg: f64,
    /// The longitude (−180..180) where the sun is overhead: its right ascension less Greenwich
    /// sidereal time.
    pub subsolar_lon_deg: f64,
    /// Apparent ecliptic longitude of date (0..360).
    pub ecliptic_lon_deg: f64,
}

/// The sun at `unix` (UTC seconds). Time is UT throughout: the minute or so between UT and the
/// dynamical time the series asks for moves the sun about 0.0007°.
pub fn sun_at(unix: i64) -> SunPosition {
    // Days from J2000.0 (2000-01-01 12:00 UT).
    let n = unix as f64 / 86_400.0 - 10_957.5;
    let mean_lon = 280.460 + 0.985_647_4 * n;
    let g = (357.528 + 0.985_600_3 * n).to_radians();
    let lambda = (mean_lon + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();
    let eps = (23.439 - 0.000_000_4 * n).to_radians();
    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos()).to_degrees();
    let decl = (eps.sin() * lambda.sin()).asin().to_degrees();
    // Reduced before the subtraction: n days of sidereal time run to millions of degrees.
    let gmst = (280.460_618_37 + 360.985_647_366_29 * n).rem_euclid(360.0);
    SunPosition {
        decl_deg: decl,
        subsolar_lon_deg: (ra.rem_euclid(360.0) - gmst + 540.0).rem_euclid(360.0) - 180.0,
        ecliptic_lon_deg: lambda.to_degrees().rem_euclid(360.0),
    }
}

/// Solar declination (degrees) at a UTC instant: [`sun_at`]'s.
pub fn solar_declination_deg(unix: i64) -> f64 {
    sun_at(unix).decl_deg
}

/// Solar elevation angle (degrees, −90..+90) at (lat, lon) for a UTC instant: 90° less the
/// great-circle distance to the sub-solar point.
/// `> 0` = sun up (day); near 0 = greyline/terminator; `cos(zenith) = sin(elev)`.
pub fn solar_elevation_deg(lat: f64, lon: f64, unix: i64) -> f64 {
    let s = sun_at(unix);
    let decl = s.decl_deg.to_radians();
    let latr = lat.to_radians();
    let dlon = (lon - s.subsolar_lon_deg).to_radians();
    (latr.sin() * decl.sin() + latr.cos() * decl.cos() * dlon.cos())
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

/// The sub-solar point (lat, lon degrees) at a UTC instant: where [`solar_elevation_deg`] is 90°.
/// The map's `subsolarPoint` is the same ephemeris.
pub fn subsolar_point(unix: i64) -> (f64, f64) {
    let s = sun_at(unix);
    (s.decl_deg, s.subsolar_lon_deg)
}

/// Point at great-circle fraction `f` (0=a, 1=b) between two (lat, lon) — used to
/// sample a path's ionospheric control points.
pub fn interpolate(a: (f64, f64), b: (f64, f64), f: f64) -> (f64, f64) {
    let (p1, l1) = (a.0.to_radians(), a.1.to_radians());
    let (p2, l2) = (b.0.to_radians(), b.1.to_radians());
    let dp = p2 - p1;
    let dl = l2 - l1;
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let delta = 2.0 * h.sqrt().asin();
    if delta < 1e-9 {
        return a;
    }
    let av = ((1.0 - f) * delta).sin() / delta.sin();
    let bv = (f * delta).sin() / delta.sin();
    let x = av * p1.cos() * l1.cos() + bv * p2.cos() * l2.cos();
    let y = av * p1.cos() * l1.sin() + bv * p2.cos() * l2.sin();
    let z = av * p1.sin() + bv * p2.sin();
    (
        z.atan2((x * x + y * y).sqrt()).to_degrees(),
        y.atan2(x).to_degrees(),
    )
}

/// Geomagnetic latitude (degrees) via a centered-dipole approximation
/// (geomagnetic north pole ≈ 80.65°N, 72.68°W) — to flag high-latitude/polar
/// paths for the auroral penalty.
pub fn geomagnetic_lat_deg(lat: f64, lon: f64) -> f64 {
    const PLAT: f64 = 80.65;
    const PLON: f64 = -72.68;
    let p = lat.to_radians();
    let pp = PLAT.to_radians();
    let dl = (lon - PLON).to_radians();
    (p.sin() * pp.sin() + p.cos() * pp.cos() * dl.cos())
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

/// The centred-dipole geomagnetic north pole [`geomagnetic_lat_deg`] uses (degrees).
const DIPOLE_POLE: (f64, f64) = (80.65, -72.68);

/// Geographic (lat, lon) → centred-dipole geomagnetic (lat, lon), degrees. The latitude is
/// [`geomagnetic_lat_deg`]'s; the longitude's zero meridian is whatever the rotation gives (the
/// cap edge walks all of it, so only the inverse, [`geographic_from_geomagnetic`], must agree).
pub fn geomagnetic_latlon(lat: f64, lon: f64) -> (f64, f64) {
    let (p, l) = (lat.to_radians(), lon.to_radians());
    let (x, y, z) = (p.cos() * l.cos(), p.cos() * l.sin(), p.sin());
    // Turn the pole's meridian to 0°, then tip the pole onto the z axis.
    let a = DIPOLE_POLE.1.to_radians();
    let (x, y) = (x * a.cos() + y * a.sin(), -x * a.sin() + y * a.cos());
    let b = (90.0 - DIPOLE_POLE.0).to_radians();
    let (x, z) = (x * b.cos() - z * b.sin(), x * b.sin() + z * b.cos());
    (
        z.clamp(-1.0, 1.0).asin().to_degrees(),
        y.atan2(x).to_degrees(),
    )
}

/// The inverse of [`geomagnetic_latlon`]: centred-dipole (lat, lon) → geographic (lat, lon).
pub fn geographic_from_geomagnetic(mlat: f64, mlon: f64) -> (f64, f64) {
    let (p, l) = (mlat.to_radians(), mlon.to_radians());
    let (x, y, z) = (p.cos() * l.cos(), p.cos() * l.sin(), p.sin());
    let b = (90.0 - DIPOLE_POLE.0).to_radians();
    let (x, z) = (x * b.cos() + z * b.sin(), -x * b.sin() + z * b.cos());
    let a = DIPOLE_POLE.1.to_radians();
    let (x, y) = (x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
    (
        z.clamp(-1.0, 1.0).asin().to_degrees(),
        y.atan2(x).to_degrees(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geomagnetic_coordinates_round_trip_and_agree_with_the_latitude() {
        for &(lat, lon) in &[(42.5, -89.0), (-33.9, 18.4), (69.6, 18.9), (0.0, 179.0)] {
            let (mlat, mlon) = geomagnetic_latlon(lat, lon);
            assert!((mlat - geomagnetic_lat_deg(lat, lon)).abs() < 1e-9);
            let (lat2, lon2) = geographic_from_geomagnetic(mlat, mlon);
            assert!((lat2 - lat).abs() < 1e-9, "{lat} → {lat2}");
            assert!(((lon2 - lon + 540.0).rem_euclid(360.0) - 180.0).abs() < 1e-9);
        }
        // The dipole pole is the geomagnetic pole.
        assert!((geomagnetic_latlon(80.65, -72.68).0 - 90.0).abs() < 1e-6);
    }

    #[test]
    fn maidenhead_known_grids() {
        // EN52 (south-central WI) — center of the 4-char square.
        let (lat, lon) = maidenhead_to_latlon("EN52").unwrap();
        assert!((lat - 42.5).abs() < 0.01, "EN52 lat {lat}");
        assert!((lon - (-89.0)).abs() < 0.01, "EN52 lon {lon}");
        // 6-char resolves finer and stays within its square.
        let (lat6, lon6) = maidenhead_to_latlon("EN52ab").unwrap();
        assert!((lat6 - 42.0).abs() < 0.6 && (lon6 - (-89.0)).abs() < 1.1);
        assert!(maidenhead_to_latlon("ZZ").is_none());
        assert!(maidenhead_to_latlon("EN5X").is_none());
    }

    #[test]
    fn haversine_and_distance() {
        // EN52 (WI) to JN58 (Munich-ish) is a long DX path (> 7000 km).
        let km = grid_distance_km("EN52", "JN58").unwrap();
        assert!(km > 7000.0 && km < 8000.0, "EN52->JN58 = {km} km");
        // Same grid ~ 0.
        assert!(grid_distance_km("EN52", "EN52").unwrap() < 1.0);
    }

    #[test]
    fn destination_point_round_trips_distance_and_bearing() {
        let origin = maidenhead_to_latlon("EN52").unwrap();
        for &(brg, dist) in &[(60.0, 5000.0), (300.0, 9000.0), (180.0, 3000.0)] {
            let dx = destination_point(origin, brg, dist);
            // Travelling `dist` on `brg` then measuring back must reproduce both.
            assert!(
                (haversine_km(origin, dx) - dist).abs() < 1.0,
                "distance round-trips: {} vs {dist}",
                haversine_km(origin, dx)
            );
            let back = bearing_deg(origin, dx);
            let diff = ((back - brg + 540.0).rem_euclid(360.0)) - 180.0;
            assert!(diff.abs() < 0.5, "bearing round-trips: {back} vs {brg}");
        }
    }

    /// ABSOLUTE headings, not a round-trip. The round-trip above is self-referential:
    /// it pins `bearing_deg` against `destination_point`, so both could share a sign
    /// error and still agree. These numbers come from outside the module — cardinal
    /// pairs whose bearing is forced by geometry, plus two real paths — so a reciprocal
    /// or swapped-argument change cannot pass.
    ///
    /// The direction convention this fixes: the FIRST argument is where you are, the
    /// SECOND is what you are pointing at. All seven production call sites pass
    /// `(me, them)`; nothing else was pinning that.
    #[test]
    fn bearing_matches_known_headings() {
        let eq = (0.0, 0.0);
        // Due east/north/west/south from the equator+prime-meridian origin.
        for &(to, want) in &[
            ((0.0, 10.0), 90.0),
            ((10.0, 0.0), 0.0),
            ((0.0, -10.0), 270.0),
            ((-10.0, 0.0), 180.0),
        ] {
            let got = bearing_deg(eq, to);
            let d = ((got - want + 540.0).rem_euclid(360.0)) - 180.0;
            assert!(
                d.abs() < 1e-6,
                "bearing_deg({eq:?}, {to:?}) = {got}, want {want}"
            );
        }
        // Real paths from the reference QTH. EN52 → JN58 (Munich) leaves the US
        // north-EAST on the great circle, NOT due east as a flat map suggests;
        // EN52 → QF56 (Sydney) leaves to the WEST.
        let en52 = maidenhead_to_latlon("EN52").unwrap();
        for &(grid, want) in &[("JN58", 46.0), ("QF56", 260.2), ("FN31", 89.4)] {
            let got = bearing_deg(en52, maidenhead_to_latlon(grid).unwrap());
            let d = ((got - want + 540.0).rem_euclid(360.0)) - 180.0;
            assert!(d.abs() < 1.5, "EN52 -> {grid} = {got}°, want ~{want}°");
        }
    }

    #[test]
    fn octants() {
        assert_eq!(compass_octant(0.0), "N");
        assert_eq!(compass_octant(45.0), "NE");
        assert_eq!(compass_octant(315.0), "NW");
        assert_eq!(compass_octant(359.0), "N");
    }

    #[test]
    fn time_of_day_wraps() {
        let (s0, c0) = time_of_day_sin_cos(0); // midnight UTC
        assert!(s0.abs() < 1e-6 && (c0 - 1.0).abs() < 1e-6);
        let (_, c_noon) = time_of_day_sin_cos(12 * 3600); // noon
        assert!((c_noon - (-1.0)).abs() < 1e-4);
    }

    #[test]
    fn day_of_year_known() {
        // 2024-06-20 13:00 UTC ≈ DOY 172 (the engine demo's NOW).
        assert_eq!(day_of_year(1_718_886_000), 172);
        // 1970-01-01 00:00 UTC = DOY 1.
        assert_eq!(day_of_year(0), 1);
    }

    #[test]
    fn solar_geometry_day_and_night() {
        // March equinox 2024 (2024-03-20 ~03:06 UTC). At ~12 UTC the subsolar
        // point is near (0°, 0°): elevation there should be ~+90°, and the
        // antipode (0°, 180°) deep night (~−90°).
        let noon = days_from_civil(2024, 3, 20) * 86_400 + 12 * 3600;
        assert!(solar_elevation_deg(0.0, 0.0, noon) > 80.0);
        assert!(solar_elevation_deg(0.0, 180.0, noon) < -80.0);
        // Declination near 0 at the equinox.
        assert!(solar_declination_deg(noon).abs() < 1.5);
    }

    #[test]
    fn interpolate_and_geomag() {
        // Midpoint of EN52↔JN58 lies in the North Atlantic at high-ish lat.
        let a = maidenhead_to_latlon("EN52").unwrap();
        let b = maidenhead_to_latlon("JN58").unwrap();
        let mid = interpolate(a, b, 0.5);
        // Endpoints recovered.
        let a0 = interpolate(a, b, 0.0);
        assert!((a0.0 - a.0).abs() < 1e-6 && (a0.1 - a.1).abs() < 1e-6);
        // This NA↔EU path bows north — geomagnetic lat of the midpoint is high.
        assert!(geomagnetic_lat_deg(mid.0, mid.1) > 50.0);
    }
    #[test]
    fn is_logged_grid_takes_4_6_and_8_and_refuses_a_callsign() {
        for g in ["EN52", "FN42", "fn31pr", "FN31PR99", " IO91  "] {
            assert!(is_logged_grid(g), "{g} is a valid logged square");
        }
        // THE case the length check exists for: every character class fits, and
        // it is a callsign. The permissive parser says yes to it — the control
        // below is what makes this test about `is_logged_grid` and not about
        // the alphabet.
        assert!(
            maidenhead_to_latlon("OL20ABC").is_some(),
            "control: the parser is permissive, or this test proves nothing"
        );
        assert!(!is_logged_grid("OL20ABC"));

        for bad in [
            "W1AW",     // ordinary callsign: 'W' is past the field range
            "KB1ABC",   // char 3 is a letter
            "FN42X",    // 5 characters — a truncated square, not a square
            "FN42ABC",  // 7
            "FN42ABCD", // 8, but the extended pair must be digits
            "FN4",      // too short
            "",
        ] {
            assert!(!is_logged_grid(bad), "{bad:?} is not a logged square");
        }
    }

    fn grid(g: &str) -> StationFix {
        StationFix::Grid(g.to_string())
    }

    #[test]
    fn best_fix_takes_the_trusted_fix_and_sharpens_it_only_from_inside() {
        // Heard IN52 on the air, and the callbook knows the subsquare and the station's exact
        // position, both inside it: the aim sharpens all the way to the position.
        let at = maidenhead_to_latlon("IN52TK").unwrap();
        let exact = StationFix::Position(at.0 + 0.01, at.1 - 0.01);
        let chosen = best_fix([grid("IN52"), grid("IN52TK"), exact.clone()]);
        assert_eq!(chosen, Some(exact));

        // The callbook places the station somewhere else (their home square) while it sends a
        // different one today: the square it sends wins, however much finer the callbook's is.
        let chosen = best_fix([
            grid("IN73"),
            grid("IN52TK"),
            StationFix::Position(at.0, at.1),
        ]);
        assert_eq!(chosen, Some(grid("IN73")));

        // A coarser fix never replaces a finer, trusted one.
        assert_eq!(
            best_fix([grid("FN42KH"), grid("FN42")]),
            Some(grid("FN42KH"))
        );
    }

    #[test]
    fn best_fix_skips_what_is_not_a_location() {
        // QRZ answers its grid field with whatever the operator typed into their profile — a
        // rover's "EN52/EN53" or a callsign-shaped string is not a square, and neither is a
        // position off the globe. None of them may be aimed at, and none of them may block the
        // next fix in line.
        let chosen = best_fix([
            grid("EN52/EN53"),
            grid("OL20ABC"),
            StationFix::Position(f64::NAN, 10.0),
            StationFix::Position(95.0, 10.0),
            grid("en52"),
        ]);
        assert_eq!(chosen, Some(grid("en52")));
        assert_eq!(best_fix([]), None);
        assert_eq!(best_fix([grid("EN5X")]), None);
    }

    #[test]
    fn a_square_contains_its_own_subsquares_and_nothing_next_door() {
        let square = grid("IN52");
        assert!(grid("IN52AA").within(&square) && grid("IN52XX").within(&square));
        assert!(!grid("IN53AA").within(&square) && !grid("IN42XX").within(&square));
        let sub = grid("IN52TK");
        assert!(
            grid("IN52TK55").within(&sub),
            "an 8-character square inside its 6"
        );
        assert!(!grid("IN52TL").within(&sub));
        // A position has no area: nothing lies inside one.
        assert!(!grid("IN52TK").within(&StationFix::Position(42.4, -8.4)));
    }

    // ── THE ONE SUN: the shared ephemeris, checked against references it did not compute ──

    /// JPL Horizons DE441 (the table `ui/src/mapGeo.sky.test.ts` documents, queried 2026-09-29):
    /// [Unix seconds, the Sun's apparent RA (deg), apparent Dec (deg), Greenwich apparent sidereal
    /// time (h)]. The point with the sun overhead is (Dec, RA − GAST).
    const JPL_SUN: [(i64, f64, f64, f64); 30] = [
        (1_788_220_800, 160.217158964, 8.347657795, 22.6783640111),
        (1_788_310_800, 161.160737075, 7.969398240, 23.7468104918),
        (1_788_400_800, 162.103118471, 7.588775901, 0.8152581793),
        (1_788_490_800, 163.044393274, 7.205888661, 1.8837070681),
        (1_788_580_800, 163.984643790, 6.820838505, 2.9521568025),
        (1_788_670_800, 164.923944839, 6.433731239, 4.0206067478),
        (1_788_760_800, 165.862365751, 6.044675668, 5.0890561991),
        (1_788_850_800, 166.799973553, 5.653782435, 6.1575045823),
        (1_788_940_800, 167.736836422, 5.261162838, 7.2259516211),
        (1_789_030_800, 168.673026201, 4.866927983, 8.2943974125),
        (1_789_120_800, 169.608619176, 4.471188482, 9.3628423911),
        (1_789_210_800, 170.543695251, 4.074054543, 10.4312871360),
        (1_789_300_800, 171.478336449, 3.675636195, 11.4997321770),
        (1_789_390_800, 172.412625724, 3.276043345, 12.5681779092),
        (1_789_480_800, 173.346646544, 2.875385648, 13.6366245077),
        (1_789_570_800, 174.280483101, 2.473772271, 14.7050719299),
        (1_789_660_800, 175.214220791, 2.071311685, 15.7735200127),
        (1_789_750_800, 176.147946668, 1.668111542, 16.8419684885),
        (1_789_840_800, 177.081749800, 1.264278611, 17.9104170425),
        (1_789_930_800, 178.015721595, 0.859918731, 18.9788653666),
        (1_790_020_800, 178.949956228, 0.455136697, 20.0473131762),
        (1_790_110_800, 179.884551285, 0.050036064, 21.1157602399),
        (1_790_200_800, 180.819608650, -0.355281158, 22.1842064337),
        (1_790_290_800, 181.755235452, -0.760714822, 23.2526517995),
        (1_790_380_800, 182.691544722, -1.166166828, 0.3210965700),
        (1_790_470_800, 183.628655086, -1.571540928, 1.3895411780),
        (1_790_560_800, 184.566689000, -1.976741914, 2.4579861951),
        (1_790_650_800, 185.505769494, -2.381674198, 3.5264321569),
        (1_790_740_800, 186.446016152, -2.786240006, 4.5948793879),
        (1_790_830_800, 187.387541674, -3.190337758, 5.6633278908),
    ];

    fn norm180(x: f64) -> f64 {
        (x + 540.0).rem_euclid(360.0) - 180.0
    }

    /// Great-circle angle between two (lat, lon) points, degrees.
    fn separation(a: (f64, f64), b: (f64, f64)) -> f64 {
        let c = a.0.to_radians().sin() * b.0.to_radians().sin()
            + a.0.to_radians().cos() * b.0.to_radians().cos() * (a.1 - b.1).to_radians().cos();
        c.clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// The old sun this module carried (Cooper's declination, a 365-day year, the day-of-year
    /// equation of time), kept here only to show the new one moves nothing by more than the old
    /// one's own error.
    fn cooper_subsolar(unix: i64) -> (f64, f64) {
        let b = 2.0 * PI / 365.0 * (day_of_year(unix) as f64 - 81.0);
        let eq = 9.87 * (2.0 * b).sin() - 7.53 * b.cos() - 1.5 * b.sin();
        let utc_hours = (unix.rem_euclid(86_400) as f64) / 3600.0;
        (
            23.45 * b.sin(),
            norm180(-(utc_hours + eq / 60.0 - 12.0) * 15.0),
        )
    }

    #[test]
    fn the_sun_stands_within_two_hundredths_of_a_degree_of_jpl_horizons() {
        // The Almanac's low-precision Sun measures 0.0063° worst over these rows; Cooper's
        // declination measured 1.08°. A sign slip in any term is degrees off.
        let mut worst = 0.0f64;
        for (unix, ra, dec, gast) in JPL_SUN {
            let jpl = (dec, norm180(ra - gast * 15.0));
            let ss = subsolar_point(unix);
            worst = worst.max(separation(ss, jpl));
            assert!(
                (solar_declination_deg(unix) - dec).abs() < 0.02,
                "declination at {unix}"
            );
            assert!(
                solar_elevation_deg(jpl.0, jpl.1, unix) > 89.98,
                "the sun is overhead where JPL puts it, at {unix}"
            );
        }
        assert!(worst < 0.02, "worst {worst}°");
    }

    /// The map (`ui/src/mapGeo.ts sunAt`) carries the SAME table in `mapGeo.sky.test.ts`: both
    /// surfaces compute one ephemeris, to the last printed digit.
    pub(crate) const SHARED_SUN: [(i64, f64, f64); 8] = [
        (0, -23.054805878, -179.183587912),
        (946_728_000, -23.033428611, 0.825221470),
        (1_709_277_634, -7.362601813, 72.922484035),
        (1_774_224_000, 0.943719206, -178.320408714),
        (1_792_886_400, -12.023247005, 176.030833367),
        (1_801_051_234, -18.441072019, 3.027696348),
        (2_524_608_000, -22.997010267, -179.160260504),
        (1_718_886_000, 23.434995885, -4.571993163),
    ];

    #[test]
    fn the_shack_and_the_map_compute_one_sun() {
        for (unix, dec, lon) in SHARED_SUN {
            let (lat, l) = subsolar_point(unix);
            assert!(
                (lat - dec).abs() < 1e-8,
                "declination at {unix}: {lat} vs {dec}"
            );
            assert!((l - lon).abs() < 1e-8, "longitude at {unix}: {l} vs {lon}");
            assert_eq!(
                solar_declination_deg(unix),
                lat,
                "one declination at {unix}"
            );
        }
    }

    /// The sun at the 2026 equinoxes and solstices — at USNO's instant of each, and at 00Z and
    /// 12Z that day — from an INDEPENDENT ephemeris: Meeus, Astronomical Algorithms (2nd ed.) ch.
    /// 25's higher-accuracy solar position with nutation (the method NOAA's solar calculator uses),
    /// apparent right ascension and declination, less Greenwich APPARENT sidereal time (ch. 12 with
    /// the nutation in longitude). [Unix seconds, declination, sub-solar longitude]. The same rows
    /// are in `ui/src/mapGeo.sky.test.ts`.
    const SEASONS: [(i64, f64, f64); 12] = [
        (1_774_017_960, 0.002267, -39.644153), // March equinox, 2026-03-20 14:46Z (USNO)
        (1_773_964_800, -0.240961, -178.098777), // 2026-03-20 00Z
        (1_774_008_000, -0.043298, 1.864364),  // 2026-03-20 12Z
        (1_782_030_240, 23.438132, 54.448941), // June solstice, 2026-06-21 08:24Z (USNO)
        (1_782_000_000, 23.437716, -179.570131), // 2026-06-21 00Z
        (1_782_043_200, 23.438052, 0.457111),  // 2026-06-21 12Z
        (1_790_121_900, 0.001272, 176.883929), // September equinox, 2026-09-23 00:05Z (USNO)
        (1_790_121_600, 0.002624, 178.134234), // 2026-09-23 00Z
        (1_790_164_800, -0.191993, -1.909660), // 2026-09-23 12Z
        (1_797_886_200, -23.437838, -132.937534), // December solstice, 2026-12-21 20:50Z (USNO)
        (1_797_811_200, -23.434888, 179.454675), // 2026-12-21 00Z
        (1_797_854_400, -23.437309, -0.483256), // 2026-12-21 12Z
    ];

    #[test]
    fn the_sun_at_the_equinoxes_and_solstices_agrees_with_an_independent_ephemeris() {
        for (unix, dec, lon) in SEASONS {
            let ss = subsolar_point(unix);
            let e = separation(ss, (dec, lon));
            assert!(e < 0.01, "{unix}: {e}° from the reference");
        }
        // USNO's instants: the declination crosses zero at an equinox, and peaks at a solstice.
        for unix in [1_774_017_960, 1_790_121_900] {
            assert!(solar_declination_deg(unix).abs() < 0.01, "equinox {unix}");
        }
        let june = 1_782_030_240;
        let dec = solar_declination_deg(june);
        assert!(
            dec > 23.43 && dec < 23.445,
            "June solstice declination {dec}"
        );
        for d in [-2, -1, 1, 2] {
            assert!(
                solar_declination_deg(june + d * 86_400) < dec,
                "June solstice not a maximum"
            );
        }
        let dec = solar_declination_deg(1_797_886_200);
        assert!(
            dec < -23.43 && dec > -23.445,
            "December solstice declination {dec}"
        );
        for d in [-2, -1, 1, 2] {
            assert!(solar_declination_deg(1_797_886_200 + d * 86_400) > dec);
        }
    }

    #[test]
    fn at_the_poles_the_suns_elevation_is_the_declination() {
        for (unix, _, _) in SEASONS {
            let dec = solar_declination_deg(unix);
            for lon in [-180.0, -73.0, 0.0, 45.0, 179.0] {
                assert!((solar_elevation_deg(90.0, lon, unix) - dec).abs() < 1e-9);
                assert!((solar_elevation_deg(-90.0, lon, unix) + dec).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn near_the_poles_a_solstice_day_is_all_day_or_all_night() {
        // Every 10 minutes through the solstice's UTC day, at every longitude in 30° steps.
        for (day, north_up) in [
            (days_from_civil(2026, 6, 21), true),
            (days_from_civil(2026, 12, 21), false),
        ] {
            let mut n = 0;
            for step in 0..144 {
                let t = day * 86_400 + step * 600;
                for k in 0..12 {
                    let lon = -180.0 + 30.0 * k as f64;
                    let north = solar_elevation_deg(89.9, lon, t);
                    let south = solar_elevation_deg(-89.9, lon, t);
                    assert_eq!(north > 0.0, north_up, "89.9N at {t}, {lon}: {north}");
                    assert_eq!(south > 0.0, !north_up, "89.9S at {t}, {lon}: {south}");
                    n += 1;
                }
            }
            assert_eq!(n, 144 * 12, "CONTROL: the whole day was sampled");
        }
    }

    #[test]
    fn the_shared_sun_agrees_with_the_old_paths_within_their_own_error() {
        // A week and five hours apart over six years: every season, every hour of the day.
        let mut worst = 0.0f64;
        let mut t = days_from_civil(2024, 1, 1) * 86_400;
        let end = days_from_civil(2030, 1, 1) * 86_400;
        let mut n = 0;
        while t < end {
            let new = subsolar_point(t);
            worst = worst.max(separation(new, cooper_subsolar(t)));
            // Elevation and the subsolar point are one sun: overhead at the point, 90° away
            // from it on the terminator.
            assert!(solar_elevation_deg(new.0, new.1, t) > 89.999);
            let anti = (-new.0, norm180(new.1 + 180.0));
            assert!(solar_elevation_deg(anti.0, anti.1, t) < -89.999);
            t += 7 * 86_400 + 5 * 3600;
            n += 1;
        }
        assert!(n > 300, "CONTROL: the sweep ran ({n})");
        // The old sun's own error: 1.08° against JPL in September 2026, 1.33° worst over these six
        // years (its 365-day year drifts a quarter day each year between leap days). Nothing
        // moves further than that error.
        assert!(worst < 1.4, "worst {worst}°");
        assert!(
            worst > 0.2,
            "CONTROL: the old path really is the old one ({worst}°)"
        );
    }

    #[test]
    fn the_p533_reference_sun_stays_its_own_and_agrees_on_the_fifteenth() {
        // P.533 keeps the ITU reference implementation's sun (its own monthly-median recipe on
        // the 15th, ported for parity), so its numbers never move. It still agrees with the
        // shared sun on the day it models.
        let loc = crate::p533::geometry::Location::new(0.0, 0.0);
        for month0 in 0..12usize {
            // Slot 11 = UT hour 12 in the reference's hour convention.
            let p = crate::p533::solar::solar_parameters(loc, month0, 11.0);
            let unix = days_from_civil(2026, month0 as u32 + 1, 15) * 86_400 + 12 * 3600;
            let d = (p.decl.to_degrees() - solar_declination_deg(unix)).abs();
            assert!(
                d < 0.6,
                "month {month0}: P.533 {} vs {}",
                p.decl.to_degrees(),
                solar_declination_deg(unix)
            );
        }
    }
}
