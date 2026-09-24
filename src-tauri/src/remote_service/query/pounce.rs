//! The rare-DX (Pounce) alerts the desktop has already raised, for a Remote browser that asked
//! to be told. The decision is NOT made here: every row is a `Pounce` exactly as the detector
//! handed it to the desktop's `pounce` event (see `crate::pouncer::run`), oldest first. The meta
//! carries the station's threshold so the browser can say when Pounce is off. No spot, gate,
//! threshold write, tune or command is reachable from this projection.
use serde_json::{json, Value};
use std::sync::TryLockError;

const CALL_BYTES: usize = 32;
const LABEL_BYTES: usize = 16;
const ENTITY_BYTES: usize = 128;
const TAGS: usize = 8;

fn row(p: &propagation::pounce::Pounce) -> Option<Value> {
    // An alert that cannot fit the browser's display contract is left out rather than failing
    // every read that happens to include it.
    (!p.call.is_empty()
        && p.call.len() <= CALL_BYTES
        && !p.band.is_empty()
        && p.band.len() <= LABEL_BYTES
        && !p.mode.is_empty()
        && p.mode.len() <= LABEL_BYTES
        && p.entity.len() <= ENTITY_BYTES
        && p.tags.len() <= TAGS
        && p.at_unix >= 0
        && p.freq_mhz.is_none_or(|f| f.is_finite() && f > 0.0))
    .then(|| serde_json::to_value(p).ok())
    .flatten()
}

pub(super) fn read(
    recent: &crate::pouncer::SharedRecent,
    engine: &crate::SharedEngine,
) -> Result<(Vec<Value>, usize, Value), &'static str> {
    let threshold = match tempo_app::engine::engine_try_lock(engine) {
        Ok(e) => e.settings().pounce_threshold,
        Err(TryLockError::WouldBlock) => return Err("applicationBusy"),
        Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
    };
    let rows: Vec<Value> = match recent.try_lock() {
        Ok(alerts) => alerts.iter().filter_map(row).collect(),
        Err(TryLockError::WouldBlock) => return Err("applicationBusy"),
        Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
    };
    let total = rows.len();
    Ok((rows, total, json!({ "threshold": threshold })))
}

#[cfg(test)]
mod tests {
    use super::super::{Publisher, Request};
    use super::*;
    use crate::pouncer::{channel, run, SharedRecent, SpotHint};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    const ID: &str = "10000000-0000-4000-8000-000000000001";

    fn engine(threshold: tempo_app::settings::PounceThreshold) -> crate::SharedEngine {
        let settings = tempo_app::settings::Settings {
            pounce_threshold: threshold,
            ..Default::default()
        };
        Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            settings,
        )))
    }

    #[test]
    fn rows_are_the_alerts_the_desktop_raised_for_the_same_spots() {
        let engine = engine(tempo_app::settings::PounceThreshold::Atno);
        let recent: SharedRecent = Default::default();
        let (tx, rx) = channel();
        let now = crate::now_unix();
        for (call, freq_mhz, mode) in [
            ("3Y0J", 14.025, "CW"),
            ("3Y0J", 14.026, "CW"), // the re-spot that follows a rare one: silent
            ("FT5ZM", 99.9, "FT8"), // off band: not workable, not news
            ("FT5ZM", 21.074, "FT8"),
        ] {
            tx.offer(SpotHint {
                call: call.into(),
                freq_mhz,
                mode: mode.into(),
                spotted_unix: now,
            });
        }
        drop(tx);
        let mut fired = Vec::new();
        run(
            engine.clone(),
            Default::default(),
            rx,
            recent.clone(),
            |p| fired.push(p),
        );
        assert_eq!(
            fired.len(),
            2,
            "positive control: the desktop detector really raised the alerts compared below"
        );
        let (rows, total, meta) = read(&recent, &engine).unwrap();
        let desktop: Vec<Value> = fired
            .iter()
            .map(|p| serde_json::to_value(p).unwrap())
            .collect();
        assert_eq!(rows, desktop);
        assert_eq!(total, 2);
        assert_eq!(meta, json!({ "threshold": "atno" }));
    }

    #[test]
    fn with_pounce_off_at_the_station_nothing_is_raised_and_the_read_says_off() {
        let engine = engine(tempo_app::settings::PounceThreshold::Off);
        let recent: SharedRecent = Default::default();
        let (tx, rx) = channel();
        tx.offer(SpotHint {
            call: "3Y0J".into(),
            freq_mhz: 14.025,
            mode: "CW".into(),
            spotted_unix: crate::now_unix(),
        });
        drop(tx);
        run(
            engine.clone(),
            Default::default(),
            rx,
            recent.clone(),
            |_| panic!("Pounce is off: the desktop raises nothing"),
        );
        assert_eq!(
            read(&recent, &engine).unwrap(),
            (Vec::new(), 0, json!({ "threshold": "off" }))
        );
    }

    #[test]
    fn the_read_is_argument_free_bounded_and_distinguishes_busy_from_missing() {
        let request = |patch: (&str, Value)| {
            let mut value = json!({ "requestId": ID, "collection": "pounce", "cursor": null, "search": "", "unconfirmed": false, "after": null });
            value[patch.0] = patch.1;
            serde_json::from_value::<Request>(value)
        };
        assert!(request(("search", json!(""))).unwrap().valid());
        for patch in [
            ("search", json!("3Y0J")),
            ("unconfirmed", json!(true)),
            ("after", json!(1)),
            ("cursor", json!(format!("{ID}:1"))),
        ] {
            assert!(!request(patch.clone()).unwrap().valid(), "{patch:?}");
        }
        assert!(request(("threshold", json!("off"))).is_err());

        let engine = engine(tempo_app::settings::PounceThreshold::Atno);
        let recent: SharedRecent = Default::default();
        let alert = |call: &str| propagation::pounce::Pounce {
            call: call.into(),
            band: "20m".into(),
            mode: "CW".into(),
            freq_mhz: Some(14.025),
            tags: vec![propagation::needalert::NeedTag::NewEntity],
            entity: "Bouvet Island".into(),
            at_unix: 1,
        };
        recent
            .lock()
            .unwrap()
            .extend([alert("3Y0J"), alert(&"X".repeat(40))]);
        let (rows, total, _) = read(&recent, &engine).unwrap();
        assert_eq!((rows.len(), total), (1, 1));
        assert_eq!(rows[0]["call"], "3Y0J");

        let held = recent.lock().unwrap();
        assert_eq!(read(&recent, &engine), Err("applicationBusy"));
        drop(held);
        let query = request(("search", json!(""))).unwrap();
        assert_eq!(
            Publisher::default().read(&query, &engine, None, Instant::now()),
            Err("applicationUnavailable")
        );
    }
}
