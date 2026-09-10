//! Existing station-owned hunter feeds and OTA context. No fetch, file write,
//! Hunt/QSY or activation command is reachable from this projection.
use super::Sources;
use propagation::OtaSpot;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, TryLockError};
use std::time::{Duration, Instant};

const MAX_SPOTS: usize = 512;
const TEXT: usize = 1024;
const FEED_TEXT_BYTES: usize = 128 * 1024;
const PAGE_BYTES: usize = 192 * 1024;
const SOURCE_TTL_MS: u64 = 15 * 60 * 1000;
const LOG_ROWS: usize = 1_000_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Spot {
    #[serde(flatten)]
    source: OtaSpot,
    new_park: bool,
    band_open: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Feed {
    program: &'static str,
    status: &'static str,
    source_age_ms: Option<u64>,
    spots: Vec<Spot>,
}
fn valid_spot(s: &OtaSpot, program: &str) -> Option<usize> {
    let strings = [&s.program, &s.reference, &s.name, &s.activator, &s.mode]
        .into_iter()
        .chain(
            [s.spotter.as_ref(), s.comment.as_ref(), s.grid.as_ref()]
                .into_iter()
                .flatten(),
        );
    let mut bytes = 0;
    for text in strings {
        if text.len() > TEXT {
            return None;
        }
        bytes += text.len();
    }
    (s.program == program
        && !s.reference.is_empty()
        && !s.activator.is_empty()
        && s.freq_khz.is_finite()
        && s.freq_khz > 0.0
        && s.freq_khz <= 1_000_000_000.0
        && s.lat
            .is_none_or(|n| n.is_finite() && (-90.0..=90.0).contains(&n))
        && s.lon
            .is_none_or(|n| n.is_finite() && (-180.0..=180.0).contains(&n))
        && s.spot_time_unix
            .is_none_or(|n| (0..=9_007_199_254_740_991).contains(&n)))
    .then_some(bytes)
}
fn feed(sources: &Sources, program: &'static str, now: i64) -> Result<Feed, &'static str> {
    let mut result = Feed {
        program,
        status: "unavailable",
        source_age_ms: None,
        spots: Vec::new(),
    };
    let cache = sources.ota.try_lock().map_err(|_| "applicationBusy")?;
    let Some((at, rows)) = cache.get(program) else {
        return Ok(result);
    };
    let Some(age) = now
        .checked_sub(*at)
        .filter(|n| *n >= 0)
        .and_then(|n| u64::try_from(n).ok())
        .and_then(|n| n.checked_mul(1000))
        .filter(|n| *n <= 9_007_199_254_740_991)
    else {
        return Ok(result);
    };
    result.source_age_ms = Some(age);
    if age >= SOURCE_TTL_MS {
        result.status = "expired";
        return Ok(result);
    }
    if rows.len() > MAX_SPOTS {
        return Err("applicationTooLarge");
    }
    let mut bytes = 0;
    for row in rows {
        bytes += valid_spot(row, program).ok_or("applicationTooLarge")?;
        if bytes > FEED_TEXT_BYTES {
            return Err("applicationTooLarge");
        }
    }
    result.spots = rows
        .iter()
        .map(|s| Spot {
            source: s.clone(),
            new_park: false,
            band_open: false,
        })
        .collect();
    result.status = "ready";
    Ok(result)
}

pub(super) fn read_engine(
    engine: &crate::SharedEngine,
    sources: &Sources,
) -> Result<Value, &'static str> {
    // The native hunter feed is deliberately separate from the cluster/RBN
    // unassisted switch (get_ota_spots and assist.why.notCovered). Preserve it.
    read_chunks(engine, sources, crate::now_unix(), |_| {})
}

fn read_chunks(
    engine: &crate::SharedEngine,
    sources: &Sources,
    now: i64,
    mut after_chunk: impl FnMut(usize),
) -> Result<Value, &'static str> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut feeds = [feed(sources, "POTA", now)?, feed(sources, "SOTA", now)?];
    let park_count = sources
        .parks
        .try_lock()
        .map_err(|_| "applicationBusy")?
        .len();
    let lock = || loop {
        if Instant::now() >= deadline {
            return Err("applicationBusy");
        }
        match engine.try_lock() {
            Ok(e) => return Ok(e),
            Err(TryLockError::Poisoned(_)) => return Err("applicationUnavailable"),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(1)),
        }
    };
    let (token, count, my_call, my_grid, activation, hunt, hunted_count) = {
        let e = lock()?;
        if e.log_records().len() > LOG_ROWS
            || e.settings().mycall.len() > TEXT
            || e.settings().mygrid.len() > TEXT
        {
            return Err("applicationTooLarge");
        }
        let activation = e.activation();
        let hunt = e.hunt_target();
        if activation
            .as_ref()
            .is_some_and(|(p, r)| !["POTA", "SOTA"].contains(&p.as_str()) || r.len() > TEXT)
            || hunt.as_ref().is_some_and(|(p, r, c)| {
                !["POTA", "SOTA"].contains(&p.as_str()) || r.len() > TEXT || c.len() > TEXT
            })
        {
            return Err("applicationTooLarge");
        }
        for feed in &mut feeds {
            for spot in &mut feed.spots {
                spot.new_park = !e.park_worked(&spot.source.reference);
            }
        }
        (
            e.log_read_token(),
            e.log_records().len(),
            e.settings().mycall.clone(),
            e.settings().mygrid.clone(),
            activation,
            hunt,
            e.hunted_parks_import_count(),
        )
    };
    // Same own-call/15-minute PSKR evidence as get_ota_spots, without copying
    // every receiver/grid from the path cache or holding the engine lock.
    let open_bands: HashSet<String> = {
        let paths = sources
            .live_paths
            .try_lock()
            .map_err(|_| "applicationBusy")?;
        if paths.len() > 20_000 {
            return Err("applicationTooLarge");
        }
        paths
            .recent_iter(now, 900)
            .filter(|p| tempo_core::message::same_call(&p.tx_call, &my_call))
            .map(|p| p.band.label().to_owned())
            .collect()
    };
    for feed in &mut feeds {
        for spot in &mut feed.spots {
            spot.band_open = propagation::Band::from_mhz(spot.source.freq_khz / 1000.0)
                .is_some_and(|b| open_bands.contains(b.label()));
        }
    }
    let unchanged = |e: &tempo_app::engine::Engine| {
        Arc::ptr_eq(&token, &e.log_read_token())
            && e.settings().mycall == my_call
            && e.settings().mygrid == my_grid
            && e.activation() == activation
    };
    let mut qso_count = 0;
    if let Some((_, reference)) = &activation {
        for offset in (0..count).step_by(128) {
            {
                let e = lock()?;
                if !unchanged(&e) {
                    return Err("applicationBusy");
                }
                qso_count += e.log_records()[offset..(offset + 128).min(count)]
                    .iter()
                    .filter(|q| q.ota.my_ref.as_deref() == Some(reference.as_str()))
                    .count();
            }
            after_chunk(offset);
        }
    }
    {
        let e = lock()?;
        if !unchanged(&e) {
            return Err("applicationBusy");
        }
    }
    let activation = match activation {
        Some((program, reference)) => {
            json!({ "program": program, "reference": reference, "qsoCount": qso_count })
        }
        None => json!({ "program": null, "reference": null, "qsoCount": 0 }),
    };
    let hunt = hunt.map(|(program, reference, call)| json!({ "program": program, "reference": reference, "call": call }));
    let value = json!({ "feeds": feeds, "activation": activation, "hunt": hunt, "parkCount": park_count, "huntedCount": hunted_count });
    if serde_json::to_vec(&value)
        .map_err(|_| "applicationUnavailable")?
        .len()
        > PAGE_BYTES
    {
        return Err("applicationTooLarge");
    }
    if Instant::now() >= deadline {
        return Err("applicationBusy");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn sources() -> Sources {
        Sources {
            spots: Default::default(),
            live_paths: Default::default(),
            region_paths: crate::SharedRegionPaths(Default::default()),
            ota: Default::default(),
            health: Default::default(),
            propagation: Default::default(),
            memories: Default::default(),
            sstv: Default::default(),
            parks: Default::default(),
        }
    }
    fn spot(program: &str, reference: &str) -> OtaSpot {
        OtaSpot {
            program: program.into(),
            reference: reference.into(),
            name: "Test park".into(),
            activator: "W1AW".into(),
            freq_khz: 14200.0,
            mode: "SSB".into(),
            spotter: None,
            comment: None,
            grid: None,
            lat: None,
            lon: None,
            spot_time_unix: Some(1000),
        }
    }
    fn engine(count: usize) -> crate::SharedEngine {
        let mut settings = tempo_app::settings::Settings::default();
        settings.mycall = "W1AW".into();
        settings.mygrid = "FN31".into();
        let mut engine = tempo_app::engine::Engine::with_settings(settings);
        let adif: String = (0..count).map(|i| {
            let call = format!("K1T{i}");
            format!("<CALL:{}>{call}<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20260909<TIME_ON:6>120000<MY_SIG:4>POTA<MY_SIG_INFO:7>US-0001<SIG:4>POTA<SIG_INFO:7>US-0002<EOR>\n", call.len())
        }).collect();
        engine.import_adif(&adif);
        assert_eq!(engine.log_records().len(), count);
        engine.set_activation("POTA", "US-0001").unwrap();
        engine.set_hunted_parks_import(vec!["US-0003".into()]);
        engine.set_hunt_target("W1AW", "POTA", "US-0004").unwrap();
        Arc::new(Mutex::new(engine))
    }
    #[test]
    fn full_activation_and_hunter_badges_match_station_sources_without_mutations() {
        let engine = engine(2301);
        let sources = sources();
        let rows: Vec<_> = ["US-0002", "US-0003", "US-0004"]
            .into_iter()
            .map(|r| spot("POTA", r))
            .collect();
        sources
            .ota
            .lock()
            .unwrap()
            .insert("POTA".into(), (1000, rows.clone()));
        sources
            .ota
            .lock()
            .unwrap()
            .insert("SOTA".into(), (1000, vec![]));
        sources
            .live_paths
            .lock()
            .unwrap()
            .push(propagation::PathSpot {
                time: 1000,
                tx_call: "W1AW".into(),
                tx_grid: None,
                rx_call: "K1ABC".into(),
                rx_grid: None,
                band: propagation::Band::B20,
                mode: None,
                snr: None,
                freq_mhz: None,
            });
        let (native_count, worked, original) = {
            let e = engine.lock().unwrap();
            (
                e.activation_qso_count(),
                rows.iter()
                    .map(|s| e.park_worked(&s.reference))
                    .collect::<Vec<_>>(),
                e.log_records().to_vec(),
            )
        };
        assert_eq!(native_count, 2301);
        assert_eq!(worked, [true, true, false]);
        let mut chunks = 0;
        let value = read_chunks(&engine, &sources, 1001, |_| {
            assert!(engine.try_lock().is_ok());
            chunks += 1;
        })
        .unwrap();
        assert_eq!(chunks, 18);
        assert_eq!(value["activation"]["qsoCount"], native_count);
        assert_eq!(value["hunt"]["reference"], "US-0004");
        assert_eq!(value["huntedCount"], 1);
        for (i, seen) in worked.iter().enumerate() {
            assert_eq!(value["feeds"][0]["spots"][i]["newPark"], !seen);
            assert_eq!(value["feeds"][0]["spots"][i]["bandOpen"], true);
        }
        assert_eq!(value["feeds"][1]["status"], "ready");
        assert_eq!(value["feeds"][1]["spots"], json!([]));
        assert_eq!(engine.lock().unwrap().log_records(), original);
        {
            let mut paths = sources.live_paths.lock().unwrap();
            *paths = Default::default();
            // A fresh report of another station, or an expired report of our
            // station, cannot turn this operator's band-open badge on.
            for (call, time) in [("K2ABC", 1000), ("W1AW", 0)] {
                paths.push(propagation::PathSpot {
                    time,
                    tx_call: call.into(),
                    tx_grid: None,
                    rx_call: "K1ABC".into(),
                    rx_grid: None,
                    band: propagation::Band::B20,
                    mode: None,
                    snr: None,
                    freq_mhz: None,
                });
            }
        }
        let without_own_evidence = read_chunks(&engine, &sources, 1001, |_| {}).unwrap();
        for spot in without_own_evidence["feeds"][0]["spots"]
            .as_array()
            .unwrap()
        {
            assert_eq!(spot["bandOpen"], false);
        }
    }
    #[test]
    fn independent_feed_loss_expiry_and_cache_contention_are_explicit() {
        let engine = engine(0);
        let sources = sources();
        sources
            .ota
            .lock()
            .unwrap()
            .insert("POTA".into(), (1000, vec![spot("POTA", "US-0004")]));
        let value = read_chunks(&engine, &sources, 1001, |_| {}).unwrap();
        assert_eq!(value["feeds"][0]["status"], "ready");
        assert_eq!(value["feeds"][1]["status"], "unavailable");
        let expired = read_chunks(&engine, &sources, 1900, |_| {}).unwrap();
        assert_eq!(expired["feeds"][0]["status"], "expired");
        assert_eq!(expired["feeds"][0]["spots"], json!([]));
        assert_eq!(
            read_chunks(&engine, &sources, 999, |_| {}).unwrap()["feeds"][0]["status"],
            "unavailable"
        );
        let guard = sources.ota.lock().unwrap();
        assert_eq!(
            read_chunks(&engine, &sources, 1001, |_| {}).unwrap_err(),
            "applicationBusy"
        );
        drop(guard);
        assert!(read_chunks(&engine, &sources, 1001, |_| {}).is_ok());
    }
    #[test]
    fn changed_log_or_activation_is_refused_between_bounded_chunks() {
        for activation in [false, true] {
            let engine = engine(270);
            let sources = sources();
            let value = read_chunks(&engine, &sources, 1000, |offset| {
                if offset != 0 {
                    return;
                }
                let mut e = engine.lock().unwrap();
                if activation {
                    e.set_activation("POTA", "US-0005").unwrap();
                } else {
                    let mut q = e.log_records()[0].clone();
                    q.ota.my_ref = Some("US-0005".into());
                    assert!(e.update_qso(0, q));
                }
            });
            assert_eq!(value.unwrap_err(), "applicationBusy");
            assert!(read_chunks(&engine, &sources, 1000, |_| {}).is_ok());
        }
    }
    #[test]
    fn count_text_and_byte_bounds_have_valid_near_limit_controls() {
        let engine = engine(0);
        let sources = sources();
        let set = |rows| {
            sources
                .ota
                .lock()
                .unwrap()
                .insert("POTA".into(), (1000, rows));
        };
        set((0..512)
            .map(|i| spot("POTA", &format!("US-{i:04}")))
            .collect());
        assert_eq!(
            read_chunks(&engine, &sources, 1000, |_| {}).unwrap()["feeds"][0]["spots"]
                .as_array()
                .unwrap()
                .len(),
            512
        );
        set(vec![spot("POTA", "US-0001"); 513]);
        assert_eq!(
            read_chunks(&engine, &sources, 1000, |_| {}).unwrap_err(),
            "applicationTooLarge"
        );
        let mut rich = spot("POTA", "US-0001");
        rich.name = "x".repeat(1024);
        rich.comment = Some("n".repeat(1024));
        set(vec![rich.clone(); 60]);
        assert!(read_chunks(&engine, &sources, 1000, |_| {}).is_ok());
        set(vec![rich.clone(); 70]);
        assert_eq!(
            read_chunks(&engine, &sources, 1000, |_| {}).unwrap_err(),
            "applicationTooLarge"
        );
        rich.name.push('x');
        set(vec![rich]);
        assert_eq!(
            read_chunks(&engine, &sources, 1000, |_| {}).unwrap_err(),
            "applicationTooLarge"
        );
    }
}
