//! Cross-implementation check of the look-angle and range-rate geometry.
//!
//! # Why an external reference at all
//!
//! Satellite Doppler is closed-form — `f × (1 ∓ ṙ/c)` — so nothing about the
//! correction itself is hard to get right. What is easy to get *quietly* wrong
//! is the range-rate `ṙ` fed into it, because every plausible mistake still
//! produces a smooth, believable-looking Doppler curve:
//!
//! * the sidereal rotation applied with the wrong sense,
//! * the observer treated as stationary (they are not: the ground station is
//!   carried east at up to ~0.46 km/s, ~10 % of a LEO range-rate),
//! * the `ω × r` term dropped when converting the satellite's TEME velocity to
//!   an Earth-fixed one,
//! * an off-by-one in the epoch, worth a few hundred metres of sub-point.
//!
//! None of those throw. All of them put the operator on the wrong frequency.
//!
//! # What is already validated, and what is not
//!
//! The propagator is NOT the risk. The `sgp4` crate runs the published
//! AFSPC/Vallado verification vectors as its own test suite — 33 element sets,
//! position asserted to 1e-6 km and velocity to 1e-9 km/s — so orbital state
//! arrives externally validated. Everything ABOVE it is ours: the TEME→ECEF
//! rotation, the observer's position, the topocentric transform, and the
//! range-rate. That is exactly what this file covers.
//!
//! The reference is Skyfield, an independent implementation with proper time
//! scales: 5 556 points over 12 cases spanning LEO (inclined, polar and
//! near-circular), geostationary and a Molniya-class deep-space orbit, seen
//! from observers at the equator, mid latitudes, inside the Arctic circle, in
//! the southern hemisphere and near the date line. See
//! `fixtures/sat_golden/generate.py` for what each case is there to break.
//! Skyfield is not a build dependency: the fixture is committed and this test
//! reads it.
//!
//! # What it found, and how well we agree
//!
//! Worst disagreement across all 5 556 points:
//!
//! | quantity   | worst Δ      | in operator terms                          |
//! |------------|--------------|--------------------------------------------|
//! | range-rate | 0.0017 km/s  | 0.8 Hz on 2 m, 2.5 Hz on 70 cm, 61 Hz on QO-100 |
//! | range      | 0.41 km      | display only                               |
//! | elevation  | 0.011°       | 1/100 of a rotator step                    |
//! | azimuth    | 0.016°       | 1/60 of a rotator step                     |
//!
//! Residual disagreement is dominated by time scales: Skyfield models UT1−UTC
//! (up to 0.9 s, ≈13 arcsec of Earth rotation) where Nexus uses UTC directly.
//! That is a deliberate simplification and the table is what it costs.
//!
//! The QO-100 column is the one to watch. 61 Hz is nothing on a 2 m or 70 cm
//! SSB signal but it is real on a 10 GHz narrowband downlink, so a future
//! geostationary-microwave mode would want the UT1 term rather than this
//! approximation — it is a known, quantified limit rather than an unknown.
//!
//! Tolerances are set from those measured figures with ~3× headroom, not at
//! round numbers, so a regression fails here long before it is audible.

use std::path::PathBuf;

#[derive(serde::Deserialize)]
struct Golden {
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
struct Case {
    label: String,
    name: String,
    line1: String,
    line2: String,
    observer_lat: f64,
    observer_lon: f64,
    samples: Vec<Sample>,
}

#[derive(serde::Deserialize)]
struct Sample {
    unix: i64,
    az_deg: f64,
    el_deg: f64,
    range_km: f64,
    range_rate_km_s: f64,
}

/// Speed of light in km/s, so a range-rate difference converts straight to Hz.
const C_KM_S: f64 = 299_792.458;

/// Doppler error in Hz for a range-rate error, at a given carrier.
fn hz_at(rate_err_km_s: f64, mhz: f64) -> f64 {
    rate_err_km_s * mhz * 1e6 / C_KM_S
}

/// Range-rate — the one that reaches the air.
///
/// Set from the MEASURED agreement (worst 0.0017 km/s) with ~3× headroom,
/// rather than at a round number that would let a real regression through. At
/// this bound the worst-case Doppler error is 2.4 Hz on 2 m, 7.3 Hz on 70 cm
/// and 175 Hz on the QO-100 downlink — the first two are inaudible on SSB and
/// below the ~20 Hz step Nexus will even write to the radio.
///
/// A dropped `ω × r` term is hundreds of m/s and a sign error doubles the
/// quantity, so both fail this by orders of magnitude.
const TOL_RATE_KM_S: f64 = 0.005;
/// Slant range feeds the display and the footprint, not the radio. Measured
/// worst is 0.41 km.
const TOL_RANGE_KM: f64 = 1.0;
/// Elevation: a tenth of a rotator step (measured worst 0.011°).
const TOL_EL_DEG: f64 = 0.1;
/// Azimuth: the same, but only meaningful when the bird is up (see below).
const TOL_AZ_DEG: f64 = 0.1;
/// Below this elevation the azimuth is geometrically ill-conditioned — a
/// fraction of a degree of elevation error swings it — and no rotator is
/// tracking there anyway.
const AZ_CHECK_EL_FLOOR: f64 = 5.0;

fn shortest_az(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

fn load() -> Golden {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "sat_golden",
        "golden.json",
    ]
    .iter()
    .collect();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("golden.json is valid JSON")
}

#[test]
fn look_angles_and_range_rate_match_an_independent_implementation() {
    let golden = load();
    assert!(golden.cases.len() >= 8, "the fixture lost cases");

    // Worst observed error per quantity, reported at the end whether or not the
    // test passes: a run that is passing at 0.9× the tolerance is a different
    // situation from one passing at 0.01×, and only one of them is comfortable.
    let (mut worst_rate, mut worst_range, mut worst_el, mut worst_az) =
        (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut checked = 0usize;

    for case in &golden.cases {
        let tle = propagation::sat::Tle {
            name: case.name.clone(),
            line1: case.line1.clone(),
            line2: case.line2.clone(),
        };
        let obs = (case.observer_lat, case.observer_lon);

        for s in &case.samples {
            let (az, el) = propagation::sat::look_at(&tle, obs, s.unix)
                .unwrap_or_else(|| panic!("{}: look_at diverged at {}", case.label, s.unix));
            let (range, rate) = propagation::sat::range_rate(&tle, obs, s.unix)
                .unwrap_or_else(|| panic!("{}: range_rate diverged at {}", case.label, s.unix));

            let d_el = (el - s.el_deg).abs();
            let d_range = (range - s.range_km).abs();
            let d_rate = (rate - s.range_rate_km_s).abs();
            worst_el = worst_el.max(d_el);
            worst_range = worst_range.max(d_range);
            worst_rate = worst_rate.max(d_rate);

            assert!(
                d_el <= TOL_EL_DEG,
                "{} @ {}: elevation {el:.4}° vs reference {:.4}° (Δ {d_el:.4}°)",
                case.label,
                s.unix,
                s.el_deg
            );
            assert!(
                d_range <= TOL_RANGE_KM,
                "{} @ {}: range {range:.3} km vs reference {:.3} km (Δ {d_range:.3} km)",
                case.label,
                s.unix,
                s.range_km
            );
            assert!(
                d_rate <= TOL_RATE_KM_S,
                "{} @ {}: range-rate {rate:.6} km/s vs reference {:.6} km/s \
                 (Δ {d_rate:.6} km/s ≈ {:.1} Hz at 435 MHz) — a dropped observer-rotation \
                 term or a sign error looks exactly like this",
                case.label,
                s.unix,
                s.range_rate_km_s,
                hz_at(d_rate, 435.0)
            );

            if s.el_deg >= AZ_CHECK_EL_FLOOR {
                let d_az = shortest_az(az, s.az_deg);
                worst_az = worst_az.max(d_az);
                assert!(
                    d_az <= TOL_AZ_DEG,
                    "{} @ {}: azimuth {az:.4}° vs reference {:.4}° (Δ {d_az:.4}°)",
                    case.label,
                    s.unix,
                    s.az_deg
                );
            }
            checked += 1;
        }
    }

    assert!(
        checked > 3_000,
        "expected thousands of comparisons, got {checked}"
    );
    println!(
        "sat golden: {checked} points across {} cases — worst Δ: rate {worst_rate:.6} km/s \
         ({:.2} Hz @ 2 m, {:.2} Hz @ 70 cm, {:.1} Hz @ QO-100), range {worst_range:.3} km, \
         el {worst_el:.4}°, az {worst_az:.4}°",
        golden.cases.len(),
        hz_at(worst_rate, 145.9),
        hz_at(worst_rate, 435.0),
        hz_at(worst_rate, 10_489.0)
    );
}

/// The sign convention, checked against the reference rather than against
/// ourselves. Getting this backwards doubles the error on the air and is
/// invisible in review — a receding bird still produces a smooth curve.
#[test]
fn receding_is_positive_in_both_implementations() {
    let golden = load();
    let mut agreed = 0usize;
    for case in &golden.cases {
        let tle = propagation::sat::Tle {
            name: case.name.clone(),
            line1: case.line1.clone(),
            line2: case.line2.clone(),
        };
        let obs = (case.observer_lat, case.observer_lon);
        for s in &case.samples {
            // Compare against the REFERENCE's own instantaneous rate, not
            // against a finite difference of the sampled ranges. The coarse
            // block steps 7 minutes, and a LEO bird reverses direction several
            // times inside that — so a difference-based check would be
            // measuring the sampling interval, not the convention. (It did,
            // and it failed on correct code before this was fixed.)
            if s.range_rate_km_s.abs() < 0.05 {
                continue; // straddling closest approach: sign is not meaningful
            }
            let (_, rate) = propagation::sat::range_rate(&tle, obs, s.unix).unwrap();
            assert_eq!(
                rate > 0.0,
                s.range_rate_km_s > 0.0,
                "{} @ {}: our rate {rate:.4} km/s, reference {:.4} km/s — the sign \
                 convention is inverted, which transmits on the wrong side of the passband",
                case.label,
                s.unix,
                s.range_rate_km_s
            );
            agreed += 1;
        }
    }
    assert!(agreed > 500, "expected many signed samples, got {agreed}");
}
