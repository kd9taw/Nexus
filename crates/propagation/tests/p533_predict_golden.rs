//! The path predictions the app already shows must not move when the engine starts carrying
//! more out of each run. The golden file was written by the engine BEFORE the per-hour path
//! detail existed; every number in it (BCR hourly, windows, reliability, mode chips, MUF) must
//! come back byte-identical, with only the new `detail` key added on top.
//!
//! Regenerate (only when a deliberate model change moves the numbers):
//! `NEXUS_WRITE_PREDICT_GOLDEN=1 cargo test -p propagation --test p533_predict_golden`

use propagation::p533::engine::P533Engine;
use propagation::{HeuristicEngine, PathPredictor, SpaceWx};

const GOLDEN: &str = "tests/fixtures/p533_predict_golden.txt";

/// (label, engine, operator, DX, unix time). One path per P.533 distance regime:
/// short model (< 7000 km), the 7000–9000 km blend, and the long model (> 9000 km).
type Case = (&'static str, Box<dyn PathPredictor>, (f64, f64), i64);

fn cases() -> Vec<Case> {
    let may_12z = 1_778_846_400; // 2026-05-15 12:00 UTC
    let dec_06z = 1_796_277_600; // 2026-12-03 06:00 UTC
    let boston = (42.36, -71.06);
    let chicago = (41.98, -87.9);
    vec![
        (
            "p533 boston-london may 12z",
            Box::new(P533Engine::new(Some(boston))),
            (51.5, -0.12),
            may_12z,
        ),
        (
            "p533 chicago-munich may 12z",
            Box::new(P533Engine::new(Some(chicago))),
            (48.35, 11.79),
            may_12z,
        ),
        (
            "p533 boston-tokyo dec 06z",
            Box::new(P533Engine::new(Some(boston))),
            (35.68, 139.69),
            dec_06z,
        ),
        (
            "heuristic boston-tokyo dec 06z",
            Box::new(HeuristicEngine::new(Some(boston))),
            (35.68, 139.69),
            dec_06z,
        ),
    ]
}

/// The prediction as JSON, minus the keys the path-detail work added (so the comparison is
/// over exactly the fields that existed before it).
fn legacy_json(eng: &dyn PathPredictor, dx: (f64, f64), t: i64) -> serde_json::Value {
    let wx = SpaceWx {
        sfi: 150.0,
        kp: 2.0,
        ..Default::default()
    };
    let mut v = serde_json::to_value(eng.predict(dx, t, &wx)).unwrap();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("geometry");
    }
    if let Some(bands) = v.get_mut("bands").and_then(|b| b.as_array_mut()) {
        for b in bands {
            if let Some(obj) = b.as_object_mut() {
                obj.remove("detail");
            }
        }
    }
    v
}

#[test]
fn existing_prediction_numbers_are_byte_identical() {
    // One `label<TAB>json` line per case, compared as raw bytes: re-parsing the JSON would
    // round-trip floats through serde_json's parser, which is not exact to the last digit.
    let now: String = cases()
        .into_iter()
        .map(|(label, eng, dx, t)| {
            let json = serde_json::to_string(&legacy_json(eng.as_ref(), dx, t)).unwrap();
            format!("{label}\t{json}\n")
        })
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("NEXUS_WRITE_PREDICT_GOLDEN").is_some() {
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert_eq!(want.lines().count(), cases().len());
    for (w, n) in want.lines().zip(now.lines()) {
        let label = w.split('\t').next().unwrap();
        assert_eq!(n, w, "{label}: a pre-existing prediction number moved");
    }
}
