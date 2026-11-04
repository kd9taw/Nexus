//! THE MOON, as an EME station's rotator needs it: where it stands in the sky from a station, now.
//!
//! The geocentric position is Meeus's lunar theory (Jean Meeus, *Astronomical Algorithms*, 2nd ed.,
//! Willmann-Bell 1998, chapter 47: the main terms of Chapront's ELP-2000/82, tables 47.A and 47.B),
//! which Meeus gives as about 10″ in longitude and 4″ in latitude. Nutation is his low-precision
//! form (chapter 22, about 0.5″) and sidereal time is IAU 1982 (eq. 12.4), as `sat.rs` uses.
//!
//! What a rotator points at is TOPOCENTRIC, and for the Moon that is not a nicety: the Moon is
//! close enough that a station on the Earth's surface sees it up to about 0.95° away from where it
//! is seen from the Earth's centre (its horizontal parallax), which is more than an EME dish's
//! beamwidth. So the station is placed on the WGS-84 ellipsoid and the Moon is looked at from
//! there, the same vector arithmetic the satellite look angles use, and the elevation is measured
//! from the ellipsoid's local horizontal, as JPL Horizons measures it.
//!
//! Geometric, with no refraction: the elevation is where the Moon is, not where the atmosphere
//! bends it to near the horizon (about half a degree at 0°), which is what Horizons' "airless"
//! figures give and what the tests hold this to. The ui's `mapGeo.ts moonAt` is a separate,
//! coarser series (0.35°) for drawing the sub-lunar point on the map; it is not this.

/// Days from the Unix epoch to J2000.0 (2000-01-01 12:00 TT, taken as UT here; ΔT is added below).
const J2000_UNIX_DAYS: f64 = 10_957.5;
/// TT − UT in seconds for the 2020s (IERS: 69.2 s in 2025). The Moon moves about 0.55″ a second,
/// so leaving it out would cost 0.01°; its drift over a decade costs a thousandth of that.
const DELTA_T_S: f64 = 69.0;

/// WGS-84 equatorial radius (km) and flattening.
const WGS84_A_KM: f64 = 6_378.137;
const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// Where the Moon is from the Earth's centre: apparent ecliptic longitude and latitude of date
/// (degrees), distance (km), and the apparent right ascension and declination (degrees) they make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonGeocentric {
    pub lambda_deg: f64,
    pub beta_deg: f64,
    pub distance_km: f64,
    pub ra_deg: f64,
    pub dec_deg: f64,
}

/// Where the Moon is from a station: azimuth (degrees from true north, clockwise), elevation
/// (degrees above the horizon, negative below it; geometric, no refraction) and range (km).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoonLook {
    pub az_deg: f64,
    pub el_deg: f64,
    pub range_km: f64,
}

// Table 47.A: the arguments D, M, M', F, and the sine (longitude, 1e-6°) and cosine (distance,
// 1e-3 km) coefficients.
#[rustfmt::skip]
const LR: [(i8, i8, i8, i8, i32, i32); 60] = [
    (0, 0, 1, 0, 6_288_774, -20_905_355), (2, 0, -1, 0, 1_274_027, -3_699_111),
    (2, 0, 0, 0, 658_314, -2_955_968),    (0, 0, 2, 0, 213_618, -569_925),
    (0, 1, 0, 0, -185_116, 48_888),       (0, 0, 0, 2, -114_332, -3_149),
    (2, 0, -2, 0, 58_793, 246_158),       (2, -1, -1, 0, 57_066, -152_138),
    (2, 0, 1, 0, 53_322, -170_733),       (2, -1, 0, 0, 45_758, -204_586),
    (0, 1, -1, 0, -40_923, -129_620),     (1, 0, 0, 0, -34_720, 108_743),
    (0, 1, 1, 0, -30_383, 104_755),       (2, 0, 0, -2, 15_327, 10_321),
    (0, 0, 1, 2, -12_528, 0),             (0, 0, 1, -2, 10_980, 79_661),
    (4, 0, -1, 0, 10_675, -34_782),       (0, 0, 3, 0, 10_034, -23_210),
    (4, 0, -2, 0, 8_548, -21_636),        (2, 1, -1, 0, -7_888, 24_208),
    (2, 1, 0, 0, -6_766, 30_824),         (1, 0, -1, 0, -5_163, -8_379),
    (1, 1, 0, 0, 4_987, -16_675),         (2, -1, 1, 0, 4_036, -12_831),
    (2, 0, 2, 0, 3_994, -10_445),         (4, 0, 0, 0, 3_861, -11_650),
    (2, 0, -3, 0, 3_665, 14_403),         (0, 1, -2, 0, -2_689, -7_003),
    (2, 0, -1, 2, -2_602, 0),             (2, -1, -2, 0, 2_390, 10_056),
    (1, 0, 1, 0, -2_348, 6_322),          (2, -2, 0, 0, 2_236, -9_884),
    (0, 1, 2, 0, -2_120, 5_751),          (0, 2, 0, 0, -2_069, 0),
    (2, -2, -1, 0, 2_048, -4_950),        (2, 0, 1, -2, -1_773, 4_130),
    (2, 0, 0, 2, -1_595, 0),              (4, -1, -1, 0, 1_215, -3_958),
    (0, 0, 2, 2, -1_110, 0),              (3, 0, -1, 0, -892, 3_258),
    (2, 1, 1, 0, -810, 2_616),            (4, -1, -2, 0, 759, -1_897),
    (0, 2, -1, 0, -713, -2_117),          (2, 2, -1, 0, -700, 2_354),
    (2, 1, -2, 0, 691, 0),                (2, -1, 0, -2, 596, 0),
    (4, 0, 1, 0, 549, -1_423),            (0, 0, 4, 0, 537, -1_117),
    (4, -1, 0, 0, 520, -1_571),           (1, 0, -2, 0, -487, -1_739),
    (2, 1, 0, -2, -399, 0),               (0, 0, 2, -2, -381, -4_421),
    (1, 1, 1, 0, 351, 0),                 (3, 0, -2, 0, -340, 0),
    (4, 0, -3, 0, 330, 0),                (2, -1, 2, 0, 327, 0),
    (0, 2, 1, 0, -323, 1_165),            (1, 1, -1, 0, 299, 0),
    (2, 0, 3, 0, 294, 0),                 (2, 0, -1, -2, 0, 8_752),
];

// Table 47.B: the arguments, and the sine coefficient for latitude (1e-6°).
#[rustfmt::skip]
const B: [(i8, i8, i8, i8, i32); 60] = [
    (0, 0, 0, 1, 5_128_122), (0, 0, 1, 1, 280_602),  (0, 0, 1, -1, 277_693), (2, 0, 0, -1, 173_237),
    (2, 0, -1, 1, 55_413),   (2, 0, -1, -1, 46_271), (2, 0, 0, 1, 32_573),   (0, 0, 2, 1, 17_198),
    (2, 0, 1, -1, 9_266),    (0, 0, 2, -1, 8_822),   (2, -1, 0, -1, 8_216),  (2, 0, -2, -1, 4_324),
    (2, 0, 1, 1, 4_200),     (2, 1, 0, -1, -3_359),  (2, -1, -1, 1, 2_463),  (2, -1, 0, 1, 2_211),
    (2, -1, -1, -1, 2_065),  (0, 1, -1, -1, -1_870), (4, 0, -1, -1, 1_828),  (0, 1, 0, 1, -1_794),
    (0, 0, 0, 3, -1_749),    (0, 1, -1, 1, -1_565),  (1, 0, 0, 1, -1_491),   (0, 1, 1, 1, -1_475),
    (0, 1, 1, -1, -1_410),   (0, 1, 0, -1, -1_344),  (1, 0, 0, -1, -1_335),  (0, 0, 3, 1, 1_107),
    (4, 0, 0, -1, 1_021),    (4, 0, -1, 1, 833),     (0, 0, 1, -3, 777),     (4, 0, -2, 1, 671),
    (2, 0, 0, -3, 607),      (2, 0, 2, -1, 596),     (2, -1, 1, -1, 491),    (2, 0, -2, 1, -451),
    (0, 0, 3, -1, 439),      (2, 0, 2, 1, 422),      (2, 0, -3, -1, 421),    (2, 1, -1, 1, -366),
    (2, 1, 0, 1, -351),      (4, 0, 0, 1, 331),      (2, -1, 1, 1, 315),     (2, -2, 0, -1, 302),
    (0, 0, 1, 3, -283),      (2, 1, 1, -1, -229),    (1, 1, 0, -1, 223),     (1, 1, 0, 1, 223),
    (0, 1, -2, -1, -220),    (2, 1, -1, -1, -220),   (1, 0, 1, 1, -185),     (2, -1, -2, -1, 181),
    (0, 1, 2, 1, -177),      (4, 0, -2, -1, 176),    (4, -1, -1, -1, 166),   (1, 0, 1, -1, -164),
    (4, 0, 1, -1, 132),      (1, 0, -1, -1, -119),   (4, -1, 0, -1, 115),    (2, -2, 0, 1, 107),
];

/// Julian centuries of TT from J2000.0 at `unix` (UTC seconds).
fn centuries_tt(unix: f64) -> f64 {
    ((unix + DELTA_T_S) / 86_400.0 - J2000_UNIX_DAYS) / 36_525.0
}

/// Meeus chapter 47 at `t` Julian centuries of TT, with the apparent position (nutation added).
fn geocentric_at(t: f64) -> MoonGeocentric {
    let (t2, t3, t4) = (t * t, t * t * t, t * t * t * t);
    let lp = 218.316_447_7 + 481_267.881_234_21 * t - 0.001_578_6 * t2 + t3 / 538_841.0
        - t4 / 65_194_000.0;
    let d = 297.850_192_1 + 445_267.111_403_4 * t - 0.001_881_9 * t2 + t3 / 545_868.0
        - t4 / 113_065_000.0;
    let m = 357.529_109_2 + 35_999.050_290_9 * t - 0.000_153_6 * t2 + t3 / 24_490_000.0;
    let mp = 134.963_396_4 + 477_198.867_505_5 * t + 0.008_741_4 * t2 + t3 / 69_699.0
        - t4 / 14_712_000.0;
    let f = 93.272_095_0 + 483_202.017_523_3 * t - 0.003_653_9 * t2 - t3 / 3_526_000.0
        + t4 / 863_310_000.0;
    let a1 = 119.75 + 131.849 * t;
    let a2 = 53.09 + 479_264.290 * t;
    let a3 = 313.45 + 481_266.484 * t;
    let e = 1.0 - 0.002_516 * t - 0.000_007_4 * t2;
    let rad = f64::to_radians;
    // A term carrying M is scaled by E for each power of M: the Earth's orbit's shrinking
    // eccentricity (Meeus, below table 47.B).
    let ecc = |km: i8| match km.abs() {
        1 => e,
        2 => e * e,
        _ => 1.0,
    };
    let arg = |cd: i8, cm: i8, cmp: i8, cf: i8| {
        rad(f64::from(cd) * d + f64::from(cm) * m + f64::from(cmp) * mp + f64::from(cf) * f)
    };
    let (mut sl, mut sr, mut sb) = (0.0, 0.0, 0.0);
    for &(cd, cm, cmp, cf, l, r) in &LR {
        let a = arg(cd, cm, cmp, cf);
        sl += f64::from(l) * ecc(cm) * a.sin();
        sr += f64::from(r) * ecc(cm) * a.cos();
    }
    for &(cd, cm, cmp, cf, b) in &B {
        sb += f64::from(b) * ecc(cm) * arg(cd, cm, cmp, cf).sin();
    }
    // The additive terms: Venus (A1), Jupiter (A2) and the Earth's flattening (L').
    sl += 3_958.0 * rad(a1).sin() + 1_962.0 * rad(lp - f).sin() + 318.0 * rad(a2).sin();
    sb += -2_235.0 * rad(lp).sin()
        + 382.0 * rad(a3).sin()
        + 175.0 * rad(a1 - f).sin()
        + 175.0 * rad(a1 + f).sin()
        + 127.0 * rad(lp - mp).sin()
        - 115.0 * rad(lp + mp).sin();
    let lambda = lp + sl / 1e6;
    let beta = sb / 1e6;
    let distance_km = 385_000.56 + sr / 1e3;
    // Nutation and the obliquity, Meeus chapter 22 (low precision), in degrees.
    let omega = rad(125.044_52 - 1_934.136_261 * t);
    let l_sun = rad(280.466_5 + 36_000.769_8 * t);
    let l_moon = rad(218.316_5 + 481_267.881_3 * t);
    let dpsi = (-17.20 * omega.sin() - 1.32 * (2.0 * l_sun).sin() - 0.23 * (2.0 * l_moon).sin()
        + 0.21 * (2.0 * omega).sin())
        / 3_600.0;
    let deps = (9.20 * omega.cos() + 0.57 * (2.0 * l_sun).cos() + 0.10 * (2.0 * l_moon).cos()
        - 0.09 * (2.0 * omega).cos())
        / 3_600.0;
    let eps0 = 23.0 + 26.0 / 60.0 + 21.448 / 3_600.0
        - (46.815_0 * t + 0.000_59 * t2 - 0.001_813 * t3) / 3_600.0;
    let eps = rad(eps0 + deps);
    let lam = rad(lambda + dpsi);
    let bet = rad(beta);
    let ra = (lam.sin() * eps.cos() - bet.tan() * eps.sin())
        .atan2(lam.cos())
        .to_degrees()
        .rem_euclid(360.0);
    let dec = (bet.sin() * eps.cos() + bet.cos() * eps.sin() * lam.sin())
        .asin()
        .to_degrees();
    MoonGeocentric {
        lambda_deg: (lambda + dpsi).rem_euclid(360.0),
        beta_deg: beta,
        distance_km,
        ra_deg: ra,
        dec_deg: dec,
    }
}

/// The Moon from the Earth's centre at `unix` (UTC seconds).
pub fn geocentric(unix: i64) -> MoonGeocentric {
    geocentric_at(centuries_tt(unix as f64))
}

/// Greenwich APPARENT sidereal time (degrees) at `unix`: IAU 1982 mean sidereal time (UT) plus
/// the equation of the equinoxes, so it matches the apparent right ascension above.
fn gast_deg(unix: f64, t_tt: f64) -> f64 {
    let d = unix / 86_400.0 - J2000_UNIX_DAYS;
    let t = d / 36_525.0;
    let gmst =
        280.460_618_37 + 360.985_647_366_29 * d + 0.000_387_933 * t * t - t * t * t / 38_710_000.0;
    let omega = (125.044_52 - 1_934.136_261 * t_tt).to_radians();
    let l_sun = (280.466_5 + 36_000.769_8 * t_tt).to_radians();
    let dpsi = (-17.20 * omega.sin() - 1.32 * (2.0 * l_sun).sin()) / 3_600.0;
    (gmst + dpsi * 23.439_3_f64.to_radians().cos()).rem_euclid(360.0)
}

/// A point on the WGS-84 ellipsoid (height 0) in Earth-fixed coordinates, km.
fn station_ecef(lat_deg: f64, lon_deg: f64) -> [f64; 3] {
    let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians());
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let n = WGS84_A_KM / (1.0 - e2 * lat.sin() * lat.sin()).sqrt();
    [
        n * lat.cos() * lon.cos(),
        n * lat.cos() * lon.sin(),
        n * (1.0 - e2) * lat.sin(),
    ]
}

/// Where the Moon stands from the station at `observer` (geodetic latitude, longitude in degrees)
/// at `unix`: topocentric, parallax included, geometric (no refraction).
pub fn look(observer: (f64, f64), unix: i64) -> MoonLook {
    let u = unix as f64;
    let t = centuries_tt(u);
    let g = geocentric_at(t);
    // The Moon in Earth-fixed coordinates: its apparent RA/dec turned by the apparent sidereal
    // time, so the Earth's rotation is the only thing between the two frames.
    let (ra, dec) = (g.ra_deg.to_radians(), g.dec_deg.to_radians());
    let ha_g = (gast_deg(u, t).to_radians()) - ra; // Greenwich hour angle
    let moon = [
        g.distance_km * dec.cos() * ha_g.cos(),
        -g.distance_km * dec.cos() * ha_g.sin(),
        g.distance_km * dec.sin(),
    ];
    let (lat_deg, lon_deg) = observer;
    let st = station_ecef(lat_deg, lon_deg);
    let v = [moon[0] - st[0], moon[1] - st[1], moon[2] - st[2]];
    let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians());
    // East, north, up at the station (the ellipsoid's own normal for "up").
    let east = -lon.sin() * v[0] + lon.cos() * v[1];
    let north = -lat.sin() * lon.cos() * v[0] - lat.sin() * lon.sin() * v[1] + lat.cos() * v[2];
    let up = lat.cos() * lon.cos() * v[0] + lat.cos() * lon.sin() * v[1] + lat.sin() * v[2];
    let range_km = (east * east + north * north + up * up).sqrt();
    MoonLook {
        az_deg: east.atan2(north).to_degrees().rem_euclid(360.0),
        el_deg: (up / range_km).asin().to_degrees(),
        range_km,
    }
}

/// When the Moon is next above the horizon at BOTH stations — an EME window — searching from
/// `from_unix` for `hours`, in steps of `step_s` seconds: `Some((start, end))` of the first window
/// found (`start` is `from_unix` itself when the window is open now; `end` is clipped to the
/// search), or `None` when there is none in that span. `min_el_deg` is the elevation each end
/// needs (0 for the geometric horizon).
pub fn eme_window(
    a: (f64, f64),
    b: (f64, f64),
    from_unix: i64,
    hours: i64,
    step_s: i64,
    min_el_deg: f64,
) -> Option<(i64, i64)> {
    let up = |t: i64| look(a, t).el_deg >= min_el_deg && look(b, t).el_deg >= min_el_deg;
    let until = from_unix + hours * 3_600;
    let step = step_s.max(1);
    let mut t = from_unix;
    while t <= until && !up(t) {
        t += step;
    }
    if t > until {
        return None;
    }
    let start = t;
    while t <= until && up(t) {
        t += step;
    }
    Some((start, t.min(until)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1992-04-12 00:00 TD, Meeus's own worked example (Example 47.a), as a Unix time in UT
    /// (ΔT is added back by `centuries_tt`, so the series sees exactly JDE 2448724.5).
    const MEEUS_47A_UNIX: f64 = (2_448_724.5 - 2_440_587.5) * 86_400.0 - DELTA_T_S;

    #[test]
    fn meeus_example_47a_is_reproduced() {
        // Source: Meeus, Astronomical Algorithms, 2nd ed., Example 47.a: λ = 133.162655°,
        // β = −3.229126°, Δ = 368409.7 km, apparent λ = 133.167265°, α = 134.688470°,
        // δ = 13.768368°.
        let g = geocentric_at(centuries_tt(MEEUS_47A_UNIX));
        assert!((g.beta_deg - -3.229_126).abs() < 1e-5, "β {}", g.beta_deg);
        assert!(
            (g.distance_km - 368_409.7).abs() < 0.5,
            "Δ {}",
            g.distance_km
        );
        assert!(
            (g.lambda_deg - 133.167_265).abs() < 1e-4,
            "apparent λ {}",
            g.lambda_deg
        );
        assert!((g.ra_deg - 134.688_470).abs() < 2e-4, "α {}", g.ra_deg);
        assert!((g.dec_deg - 13.768_368).abs() < 2e-4, "δ {}", g.dec_deg);
    }

    fn unix(y: i32, mo: u32, d: u32, h: u32) -> i64 {
        crate::geo::days_from_civil(y as i64, mo, d) * 86_400 + i64::from(h) * 3_600
    }

    /// Source: JPL Horizons (DE441), target 301 (Moon), observer on the WGS-84 ellipsoid at
    /// height 0, quantities 4 (apparent azimuth and elevation) and 20 (range), APPARENT=AIRLESS,
    /// UT, fetched 2026-11-03 from ssd.jpl.nasa.gov/api/horizons.api.
    /// Rows: (y, m, d, h, azimuth °, elevation °, range AU).
    #[rustfmt::skip]
    const GREENWICH_2026_11_03: [(i32, u32, u32, u32, f64, f64, f64); 9] = [
        (2026, 11, 3, 0, 69.908_421, 0.165_418, 0.002_516_213_321_28),
        (2026, 11, 3, 3, 104.748_041, 26.378_792, 0.002_500_352_217_21),
        (2026, 11, 3, 6, 153.009_108, 47.023_132, 0.002_491_132_718_57),
        (2026, 11, 3, 9, 216.226_727, 43.951_245, 0.002_495_658_242_33),
        (2026, 11, 3, 12, 259.831_501, 20.240_177, 0.002_513_342_940_73),
        (2026, 11, 3, 15, 293.677_482, -6.963_176, 0.002_536_213_272_75),
        (2026, 11, 3, 18, 333.502_249, -27.101_986, 0.002_553_519_876_32),
        (2026, 11, 3, 21, 22.226_680, -28.889_831, 0.002_557_662_904_67),
        (2026, 11, 4, 0, 64.214_607, -11.159_264, 0.002_548_179_291_95),
    ];
    /// The same, from Sydney (33.8688° S, 151.2093° E), 2025-06-10.
    #[rustfmt::skip]
    const SYDNEY_2025_06_10: [(i32, u32, u32, u32, f64, f64, f64); 7] = [
        (2025, 6, 10, 0, 192.223_542, -30.164_868, 0.002_713_970_084_28),
        (2025, 6, 10, 4, 137.714_723, -16.590_765, 0.002_702_382_435_23),
        (2025, 6, 10, 8, 106.004_349, 24.647_692, 0.002_670_056_237_98),
        (2025, 6, 10, 12, 70.398_050, 72.781_738, 0.002_644_871_114_40),
        (2025, 6, 10, 16, 270.044_988, 54.600_209, 0.002_648_203_754_86),
        (2025, 6, 10, 20, 242.834_569, 8.055_871, 0.002_674_139_620_39),
        (2025, 6, 11, 0, 203.462_040, -25.110_679, 0.002_695_543_181_45),
    ];
    const AU_KM: f64 = 149_597_870.7;

    /// Measured: within 0.0006° of Horizons in azimuth (along the sky) and elevation on every row
    /// below; held to 0.01°, a tenth of the brief's 0.1°.
    fn holds(observer: (f64, f64), rows: &[(i32, u32, u32, u32, f64, f64, f64)]) {
        for &(y, mo, d, h, az, el, au) in rows {
            let m = look(observer, unix(y, mo, d, h));
            // Azimuth error is measured along the sky (it stretches near the zenith).
            let daz = ((m.az_deg - az + 540.0).rem_euclid(360.0) - 180.0) * el.to_radians().cos();
            assert!(
                daz.abs() < 0.01 && (m.el_deg - el).abs() < 0.01,
                "{y}-{mo}-{d} {h}h: az {:.4} el {:.4}, Horizons {az} {el}",
                m.az_deg,
                m.el_deg
            );
            // The distance series is the truncated one: measured 4 to 38 km off Horizons over these
            // rows (0.01 %), which moves the parallax by a few thousandths of a degree.
            assert!(
                (m.range_km - au * AU_KM).abs() < 50.0,
                "range {}",
                m.range_km
            );
        }
    }

    #[test]
    fn the_moon_from_greenwich_matches_jpl_horizons_to_a_hundredth_of_a_degree() {
        holds((51.4769, 0.0), &GREENWICH_2026_11_03);
    }

    #[test]
    fn the_moon_from_sydney_matches_jpl_horizons_including_near_the_zenith() {
        holds((-33.8688, 151.2093), &SYDNEY_2025_06_10);
    }

    #[test]
    fn parallax_is_applied_the_station_is_not_at_the_earths_centre() {
        // Near the horizon the Moon sits about 0.95° lower from the surface than from the
        // centre. The geocentric elevation of the first Greenwich row is about +1.1°; Horizons
        // gives 0.165° from the station. Dropping parallax would miss by a whole beamwidth.
        let t = unix(2026, 11, 3, 0);
        let m = look((51.4769, 0.0), t);
        let g = geocentric(t);
        let (lat, ra, dec) = (
            51.4769_f64.to_radians(),
            g.ra_deg.to_radians(),
            g.dec_deg.to_radians(),
        );
        let ha = (gast_deg(t as f64, centuries_tt(t as f64)).to_radians()) - ra;
        let geo_el = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos())
            .asin()
            .to_degrees();
        assert!(
            geo_el - m.el_deg > 0.8,
            "geocentric {geo_el}, topocentric {}",
            m.el_deg
        );
    }

    #[test]
    fn an_eme_window_is_when_the_moon_is_up_at_both_ends() {
        let greenwich = (51.4769, 0.0);
        let sydney = (-33.8688, 151.2093);
        let from = unix(2026, 11, 3, 0);
        let (start, end) =
            eme_window(greenwich, sydney, from, 48, 300, 0.0).expect("a window in two days");
        assert!(start < end);
        for t in [start, (start + end) / 2, end - 300] {
            assert!(
                look(greenwich, t).el_deg >= 0.0 && look(sydney, t).el_deg >= 0.0,
                "{t}"
            );
        }
        // Just before it opens, one end has the Moon down (unless it was open at `from`).
        if start > from {
            let t = start - 300;
            assert!(look(greenwich, t).el_deg < 0.0 || look(sydney, t).el_deg < 0.0);
        }
        // The control: a station and its own antipode never share the Moon above the horizon.
        assert_eq!(
            eme_window((10.0, 20.0), (-10.0, -160.0), from, 48, 300, 0.0),
            None
        );
    }
}
