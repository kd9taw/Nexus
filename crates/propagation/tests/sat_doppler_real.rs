//! Doppler prediction checked against REAL RECORDED SIGNALS FROM SPACE.
//!
//! The sibling `sat_golden.rs` proves we agree with another implementation of
//! the same theory. That is necessary but not sufficient: two implementations
//! can share a misconception. This one leaves theory entirely and compares
//! against carriers that actually arrived at real antennas.
//!
//! The observations come from the SatNOGS network. Each waterfall carries a
//! calibrated frequency and time axis in its PNG metadata, so the drift of a
//! satellite's carrier through a pass can be recovered as (UTC, Hz) — real
//! measured RF, from ground stations on three continents, with the same
//! orbital elements the station used at the time.
//! `fixtures/sat_doppler_real/generate.py` documents the extraction and the
//! curation, which is most of the work.
//!
//! # The one fitted parameter, and why it is legitimate
//!
//! Every observation needs a single constant frequency offset. Cubesat
//! transmitters and ground-station local oscillators are off by tens of parts
//! per million — an absolute error of tens of kHz that has nothing to do with
//! Doppler. That offset is fitted HERE, by this test, from the data: the
//! fixture stores raw measured frequency and nothing else, because a fixture
//! pre-fitted toward a prediction would agree with any predictor by
//! construction and prove nothing.
//!
//! So this validates the SHAPE of the Doppler curve — the part we compute —
//! and cannot validate absolute frequency, which is the transmitter's business.
//! The shape is the whole of the physics: a sign error, a missing
//! Earth-rotation term or a bad coordinate transform all deform it, and none of
//! them survive a constant offset.
//!
//! # What this catches, measured rather than assumed
//!
//! Thresholds were set by injecting faults, not by picking round numbers. With
//! a 150 Hz bound, a geocentric-instead-of-geodetic station latitude (worst 137
//! Hz) and a missing Earth-rotation term in the topocentric velocity (worst 147
//! Hz) both passed. They are caught by the tighter per-observation bound and,
//! more reliably, by the bound on the MEAN across all eight — a systematic
//! error lifts every observation at once, which is exactly what an average sees
//! and a worst case does not. A Doppler sign flip lands at 4 500 Hz mean and a
//! longitude sign flip at 2 790 Hz; those were never the hard cases.
//!
//! # What this does NOT catch, stated so nobody assumes otherwise
//!
//! * **Station altitude** is not exercised at all — zeroing it changes nothing
//!   measurable here.
//! * **Time-base errors up to about a second** hide inside each pass's own
//!   along-track element error; the residual cannot separate "your clock is
//!   wrong" from "these elements are stale".
//! * The floor is TLE error, not measurement noise. Per-row scatter is ~10–25
//!   Hz, so a perfect predictor still lands at 15–60 Hz on this fixture.
//! * Curation admitted passes where SGP4 with the given elements is accurate,
//!   so a misconception shared with SGP4 itself would not show up.

use std::path::PathBuf;

#[derive(serde::Deserialize)]
struct Fixture {
    observations: Vec<Obs>,
}

#[derive(serde::Deserialize)]
struct Obs {
    observation_id: u64,
    satellite: String,
    line1: String,
    line2: String,
    station_lat: f64,
    station_lng: f64,
    center_freq_hz: f64,
    samples: Vec<Sample>,
}

#[derive(serde::Deserialize)]
struct Sample {
    unix: f64,
    observed_hz: f64,
}

/// Speed of light, m/s.
const C_M_S: f64 = 299_792_458.0;

/// Per-observation residual bound, in Hz RMS.
///
/// Our worst on this fixture is 80.1 Hz (OBJECT BT), so this is ~1.25×.
/// Deliberately NOT the 150 Hz that "looks safe": at 150, injected
/// geocentric-instead-of-geodetic latitude (137 Hz) and a missing
/// Earth-rotation term (147 Hz) both passed. The tight bound is affordable
/// because the fixture is frozen — no network, fixed elements, fixed samples —
/// so run-to-run variance is exactly zero and headroom only has to cover a
/// deliberate code change.
const MAX_RMS_HZ: f64 = 100.0;

/// Bound on the MEAN residual across every observation, in Hz RMS.
///
/// Our measured mean is 36.9 Hz, so this is ~1.22×. This is the sharp gate: a
/// systematic geometry error raises all eight observations together, which an
/// average detects long before any single one crosses its own limit.
///
/// For reference, the same fixture measured against Skyfield gives 32.0 Hz.
/// The ~5 Hz we give up is the UT1−UTC term we deliberately do not model —
/// the same simplification `sat_golden.rs` quantifies from the other direction.
const MAX_MEAN_RMS_HZ: f64 = 45.0;

fn load() -> Fixture {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "sat_doppler_real",
        "observed.json",
    ]
    .iter()
    .collect();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("observed.json is valid JSON")
}

/// Predicted received frequency, before the transmitter's own offset: the
/// classic one-way Doppler on the downlink, from OUR range-rate.
fn predict(obs: &Obs, tle: &propagation::sat::Tle, unix: f64) -> Option<f64> {
    let (_, rate) = propagation::sat::range_rate_at(tle, (obs.station_lat, obs.station_lng), unix)?;
    // Receding (positive rate) ⇒ heard low.
    Some(obs.center_freq_hz * (1.0 - (rate * 1000.0) / C_M_S))
}

/// Residuals after removing the single best constant offset (its mean), and the
/// RMS of what is left.
fn residual_rms(obs: &Obs) -> (f64, usize, f64) {
    let tle = propagation::sat::Tle {
        name: obs.satellite.clone(),
        line1: obs.line1.clone(),
        line2: obs.line2.clone(),
    };
    let diffs: Vec<f64> = obs
        .samples
        .iter()
        .filter_map(|s| predict(obs, &tle, s.unix).map(|p| s.observed_hz - p))
        .collect();
    assert!(
        diffs.len() == obs.samples.len(),
        "observation {}: propagation diverged on {} of {} samples",
        obs.observation_id,
        obs.samples.len() - diffs.len(),
        obs.samples.len()
    );
    let offset = diffs.iter().sum::<f64>() / diffs.len() as f64;
    let rms = (diffs.iter().map(|d| (d - offset).powi(2)).sum::<f64>() / diffs.len() as f64).sqrt();
    (rms, diffs.len(), offset)
}

#[test]
fn predicted_doppler_matches_carriers_recorded_off_the_air() {
    let fx = load();
    assert!(
        fx.observations.len() >= 6,
        "the fixture lost observations — it needs several stations and satellites \
         so the test cannot pass by fitting one station's quirks"
    );

    let mut rmss = Vec::new();
    for obs in &fx.observations {
        let (rms, n, offset) = residual_rms(obs);
        let ppm = offset / obs.center_freq_hz * 1e6;
        println!(
            "obs {:>9}  {:<16} n={n:>4}  RMS {rms:>6.1} Hz  (fitted offset {offset:>9.0} Hz = {ppm:>7.2} ppm)",
            obs.observation_id, obs.satellite
        );
        assert!(
            rms <= MAX_RMS_HZ,
            "observation {} ({}): residual {rms:.1} Hz RMS exceeds {MAX_RMS_HZ} Hz. \
             This is a real recorded carrier, so either the Doppler geometry has \
             regressed or these orbital elements no longer describe the pass.",
            obs.observation_id,
            obs.satellite
        );
        rmss.push(rms);
    }

    let mean = rmss.iter().sum::<f64>() / rmss.len() as f64;
    println!(
        "mean {mean:.1} Hz RMS across {} observations (gate {MAX_MEAN_RMS_HZ} Hz)",
        rmss.len()
    );
    assert!(
        mean <= MAX_MEAN_RMS_HZ,
        "mean residual {mean:.1} Hz RMS exceeds {MAX_MEAN_RMS_HZ} Hz. Every \
         observation moving together is the signature of a systematic geometry \
         error rather than one stale element set."
    );
}

/// The fitted offset must behave like a physical constant, not like a free
/// parameter absorbing our errors.
///
/// If the offset were soaking up a prediction error it would vary with the
/// geometry — different passes of the same satellite over the same station
/// would want different values. They do not: the same transmitter and the same
/// receiver reproduce the same offset across passes hours apart, because it IS
/// the transmitter's crystal and the station's local oscillator.
#[test]
fn the_fitted_offset_is_a_property_of_the_hardware_not_of_the_fit() {
    let fx = load();
    let mut by_pair: std::collections::HashMap<String, Vec<(u64, f64)>> = Default::default();
    for obs in &fx.observations {
        let (_, _, offset) = residual_rms(obs);
        let ppm = offset / obs.center_freq_hz * 1e6;
        by_pair
            .entry(obs.satellite.clone())
            .or_default()
            .push((obs.observation_id, ppm));
    }
    let mut compared = 0;
    for (sat, mut passes) in by_pair {
        if passes.len() < 2 {
            continue;
        }
        passes.sort_by_key(|(id, _)| *id);
        let (lo, hi) = passes.iter().fold((f64::MAX, f64::MIN), |(l, h), (_, p)| {
            (l.min(*p), h.max(*p))
        });
        println!(
            "{sat}: offset {lo:.2}..{hi:.2} ppm across {} passes",
            passes.len()
        );
        assert!(
            (hi - lo).abs() < 0.5,
            "{sat}: the fitted offset moved {:.2} ppm between passes ({passes:?}). \
             A hardware constant does not do that — an offset that drifts with \
             geometry means it is absorbing prediction error, and the residuals \
             above are flattering us.",
            hi - lo
        );
        compared += 1;
    }
    assert!(
        compared > 0,
        "no satellite appears twice, so this check never ran — the fixture must \
         keep at least one repeated satellite/station pair"
    );
}

/// Whole-second time resolution is fine for a rotator and NOT fine for Doppler.
/// This measures the cost rather than asserting a belief about it.
#[test]
fn sub_second_timing_matters_for_doppler_but_not_for_pointing() {
    let fx = load();
    let obs = fx
        .observations
        .iter()
        .max_by_key(|o| o.samples.len())
        .expect("fixture is not empty");
    let tle = propagation::sat::Tle {
        name: obs.satellite.clone(),
        line1: obs.line1.clone(),
        line2: obs.line2.clone(),
    };
    let station = (obs.station_lat, obs.station_lng);

    // Worst error introduced by rounding the evaluation instant to the second,
    // expressed as Hz on this observation's own downlink.
    let mut worst_hz = 0.0f64;
    for s in &obs.samples {
        let exact = propagation::sat::range_rate_at(&tle, station, s.unix);
        let rounded = propagation::sat::range_rate(&tle, station, s.unix.round() as i64);
        if let (Some((_, a)), Some((_, b))) = (exact, rounded) {
            worst_hz = worst_hz.max((a - b).abs() * 1000.0 / C_M_S * obs.center_freq_hz);
        }
    }
    println!(
        "{}: rounding the evaluation instant to the second costs up to {worst_hz:.1} Hz at {:.1} MHz",
        obs.satellite,
        obs.center_freq_hz / 1e6
    );
    // Not a regression gate — a measurement, kept honest with a loose bound so
    // it fails only if the sub-second path stops being sub-second.
    assert!(
        worst_hz > 0.0,
        "rounding to the second changed nothing, so `range_rate_at` is not \
         actually evaluating at the fractional instant"
    );
    assert!(
        worst_hz < 500.0,
        "implausible timing sensitivity: {worst_hz} Hz"
    );
}
