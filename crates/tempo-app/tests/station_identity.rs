//! Golden identity harness for the STATION-WIDE half of `Engine`.
//!
//! # Why this exists — `watch_identity` is blind here
//!
//! `watch_identity` byte-compares a scripted session's snapshot and is the acceptance gate for
//! the multi-radio work. But every station-wide field in its fixture sits at its DEFAULT:
//! `highlights: []`, `hunt: null`, `pendingLog: null`, `uploadNote: null`, `radios: []`, and
//! every roster station `worked:false country:null gridRarity:null lotwUser:false`. It proves
//! the per-chain half survived and that the station half kept its SHAPE — it cannot detect a
//! semantic change in any station-wide behaviour.
//!
//! That was fine for Phase 1a, which moved per-chain decoder state (richly covered). It is
//! exactly the wrong gate for the StationCore extraction, which moves the logbook, the
//! worked-before indices, the connectors, the activation and the hunt target — i.e. precisely
//! the fields the other fixture leaves empty.
//!
//! So this harness drives station state to NON-DEFAULT values and pins the result:
//!   * a logbook with real QSOs, so worked-before stamping has something to say
//!   * DXCC / grid-rarity / LoTW resolvers installed, so country, gridRarity and lotwUser
//!     stamp onto the roster instead of staying null
//!   * an active POTA activation, which log_qso stamps onto every contact
//!   * a pending hunt target, which the snapshot surfaces
//!
//! Same rules as its sibling: byte-compare against a checked-in fixture, blank ONLY genuinely
//! time-derived fields, and treat any other difference as a finding rather than a rebaseline.

use serde_json::Value;
use std::path::PathBuf;
use tempo_app::dto::Tier;
use tempo_app::engine::Engine;
use tempo_app::settings::Settings;
use tempo_core::logbook::QsoRecord;

const F0_HZ: f32 = 1500.0;

fn fixed_settings() -> Settings {
    Settings {
        mycall: "W9XYZ".into(),
        mygrid: "EN37".into(),
        dial_mhz: 14.074,
        band: "20m".into(),
        sideband: "USB".into(),
        ..Default::default()
    }
}

/// A logged QSO with every field pinned — no clock reads, no randomness.
fn qso(call: &str, band: &str, mode: &str, when: u64) -> QsoRecord {
    // Written out in full rather than via Default: QsoRecord has none, and spelling every
    // field keeps the fixture pinned if a new field is added (it becomes a compile error
    // here, which is the point of a golden harness).
    QsoRecord {
        call: call.into(),
        grid: Some("FN31".into()),
        country: None,
        state: None,
        band: band.into(),
        freq_mhz: 14.074,
        mode: mode.into(),
        rst_sent: Some("-10".into()),
        rst_rcvd: Some("-12".into()),
        name: None,
        qth: None,
        comment: None,
        notes: None,
        tx_power: None,
        when_unix: when,
        time_off_unix: None,
        confirmed: false,
        award_confirmed: false,
        qsl_rcvd: Default::default(),
        qsl_sent: Default::default(),
        credit_granted: Vec::new(),
        credit_submitted: Vec::new(),
        upload: Default::default(),
        ota: Default::default(),
    }
}

/// Drive the STATION half to non-default values, deterministically.
fn scripted_station() -> Engine {
    let mut eng = Engine::with_settings(fixed_settings());

    // Resolvers first: set_dxcc_resolver rebuilds the worked index as a side effect, so
    // installing them before the log exercises that path rather than skipping it.
    eng.set_dxcc_resolver(|call| match &call[..1] {
        "K" | "N" | "W" => Some("United States".into()),
        "G" => Some("England".into()),
        _ => None,
    });
    eng.set_grid_rarity_resolver(|grid| {
        if grid.starts_with("FN") {
            Some(3)
        } else {
            None
        }
    });
    eng.set_lotw_resolver(|call| call.starts_with('K'));

    // A real logbook, so worked-before / worked-entities / worked-grids are non-empty.
    eng.log_qso(qso("K2DEF", "20m", "FT8", 1_700_000_000));
    eng.log_qso(qso("G3ABC", "40m", "CW", 1_700_000_100));

    // Station intents the snapshot surfaces.
    let _ = eng.set_activation("POTA", "K-1234");
    let _ = eng.set_hunt_target("N7GHI", "POTA", "K-5678");

    // One decode so the roster exists and carries the stamped country/rarity/lotw flags —
    // the fields the other fixture leaves null.
    eng.set_tier(Tier::Ft8);
    eng.set_frequency(14.074, "20m", "USB");
    eng.set_rx_offset(F0_HZ);
    eng
}

/// Same four rules as watch_identity. Anything else differing is a FINDING.
fn normalize(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if k == "nextSlotMs"
                    || k == "clockOffsetMs"
                    || k == "qsoStartUnix"
                    || k.ends_with("Tick")
                {
                    *val = Value::Null;
                } else {
                    normalize(val);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize),
        _ => {}
    }
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/station_identity_snapshot.json")
}

fn golden_doc() -> String {
    let eng = scripted_station();
    let mut snap = serde_json::to_value(eng.snapshot()).expect("snapshot serializes");
    normalize(&mut snap);
    format!("{}\n", serde_json::to_string_pretty(&snap).unwrap())
}

#[test]
fn station_state_is_byte_identical_to_golden() {
    let got = golden_doc();
    let want = std::fs::read_to_string(fixture_path()).unwrap_or_default();
    assert_eq!(
        got, want,
        "station-wide snapshot changed. This fixture covers the half watch_identity CANNOT \
         see — logbook, worked-before indices, resolvers, activation, hunt. Do not rebaseline: \
         find what changed."
    );
}

/// The point of the harness: if these are default, it is testing nothing.
#[test]
fn the_fixture_actually_exercises_station_state() {
    let eng = scripted_station();
    let snap = eng.snapshot();
    let v = serde_json::to_value(&snap).unwrap();

    assert!(
        v.get("hunt").map(|h| !h.is_null()).unwrap_or(false),
        "hunt target is null — this harness would be as blind as watch_identity"
    );
    // Dump the station-relevant fields so a failure says WHICH one went default.
    let probe: Vec<(&str, String)> = ["hunt", "highlights", "uploadNote", "qso", "mycall"]
        .iter()
        .map(|k| {
            (
                *k,
                v.get(*k)
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "<absent>".into()),
            )
        })
        .collect();
    assert!(
        probe.iter().any(|(k, val)| *k == "hunt" && val != "null"),
        "station state is at defaults — this harness would be as blind as watch_identity: {probe:?}"
    );
}

#[test]
#[ignore = "regenerates the station fixture; run explicitly on a known-good tree"]
fn regenerate_station_fixture() {
    std::fs::write(fixture_path(), golden_doc()).expect("write fixture");
}
