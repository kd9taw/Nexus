//! Passive projection of the existing station board. No fetch, predictor, DSP,
//! filesystem, notification or transmit path is reachable from this reader.
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::Arc;

const MAX_BYTES: usize = 192 * 1024;
const MAX_ENTRIES: usize = 256;
const WINDOWS_TTL_MS: u64 = crate::DXPED_WINDOWS_TTL_SECS * 1000;
const TEXT: usize = 1024;

fn text(values: &[&str]) -> bool {
    values.iter().all(|s| s.len() <= TEXT)
}
fn strings(values: &[String], max: usize) -> bool {
    values.len() <= max && values.iter().all(|s| s.len() <= TEXT)
}
fn outlook(values: &[propagation::BandOutlook]) -> bool {
    values.len() <= 16
        && values.iter().all(|v| {
            text(&[&v.band, &v.workability, &v.window])
                && v.hourly.len() == 24
                && v.mode_now.len() <= 16
                && v.mode_now.iter().all(|m| m.mode.len() <= TEXT)
        })
}
fn board(v: &propagation::DxpedDashboard) -> bool {
    strings(&v.active, MAX_ENTRIES)
        && v.workable_now.len() <= MAX_ENTRIES
        && v.upcoming.len() <= MAX_ENTRIES
        && v.workable_now.iter().all(|c| {
            text(&[
                &c.call,
                &c.entity,
                &c.band,
                &c.octant,
                &c.likelihood,
                &c.how_to_call,
                &c.window_hint,
            ]) && strings(&c.modes, 16)
        })
        && v.upcoming.iter().all(|c| {
            text(&[&c.call, &c.entity, &c.region, &c.octant, &c.best])
                && c.website.as_ref().is_none_or(|s| s.len() <= 2048)
                && strings(&c.bands, 32)
                && strings(&c.modes, 16)
                && outlook(&c.outlook)
        })
}
fn windows(v: &[crate::DxpedWindow]) -> bool {
    v.len() <= MAX_ENTRIES
        && v.iter().all(|w| {
            text(&[&w.call, &w.engine, &w.best])
                && outlook(&w.outlook)
                && w.days.len() <= 10
                && w.days.iter().all(|d| d.best.len() <= TEXT)
        })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Projection<'a> {
    dxpeditions: &'a propagation::DxpedDashboard,
    source: &'a str,
    as_of: i64,
    source_age_ms: u64,
    windows: Option<&'a [crate::DxpedWindow]>,
    window_age_ms: Option<u64>,
    window_valid_for_ms: Option<u64>,
}
struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(std::io::Error::other("bounded Remote DX board"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn read_engine(
    engine: &crate::SharedEngine,
    cache: &crate::PropCache,
) -> Result<Value, &'static str> {
    let ssn = *crate::LAST_SSN.try_lock().map_err(|_| "applicationBusy")?;
    read_cached(engine, cache, &crate::DXPED_WINDOWS, ssn)
}
fn read_cached(
    engine: &crate::SharedEngine,
    cache: &crate::PropCache,
    forecasts: &crate::DxpedWindowsCache,
    ssn: Option<f32>,
) -> Result<Value, &'static str> {
    if crate::unassisted() {
        return Err("applicationUnavailable");
    }
    let (call, grid, log, model, power, gain) = {
        let eng = engine.try_lock().map_err(|_| "applicationBusy")?;
        let s = eng.settings();
        if !text(&[&s.mycall, &s.mygrid, &s.prop_engine]) {
            return Err("applicationTooLarge");
        }
        (
            s.mycall.clone(),
            s.mygrid.clone(),
            eng.log_read_token(),
            s.prop_engine.clone(),
            s.station_power_w,
            s.ant_tx_gain_dbi + s.ant_rx_gain_dbi,
        )
    };
    let now = crate::now_unix();
    let bytes = {
        let guard = cache.try_lock().map_err(|_| "applicationBusy")?;
        let (at, snap, context) = guard.as_ref().ok_or("applicationUnavailable")?;
        if call != context.call
            || grid != context.grid
            || !Arc::ptr_eq(&log, &context.log)
            || !crate::is_real_call(&call)
            || !["live", "partial", "cached"].contains(&snap.source.as_str())
            || snap.as_of <= 0
            || snap.as_of > now + 5
        {
            return Err("applicationUnavailable");
        }
        let age = (at.elapsed().as_millis() as u64)
            .max(now.saturating_sub(snap.as_of).max(0) as u64 * 1000);
        if age >= crate::PROP_TTL_SECS * 1000 {
            return Err("queryExpired");
        }
        if !board(&snap.dxpeditions) {
            return Err("applicationTooLarge");
        }
        let calls: BTreeSet<&str> = snap
            .dxpeditions
            .workable_now
            .iter()
            .map(|c| c.call.as_str())
            .chain(snap.dxpeditions.upcoming.iter().map(|c| c.call.as_str()))
            .collect();
        let key = crate::dxped_windows_key(
            now / 86_400,
            7,
            &grid,
            &model,
            power,
            gain,
            ssn,
            calls.into_iter().collect(),
        );
        // A cold/busy/oversized forecast cache is explicitly absent. Reading the
        // board never starts or waits for a P.533 sweep. The native cache identity
        // and TTL remain the authority for a matching seven-day result.
        let forecasts = forecasts.try_lock().ok();
        let found = forecasts.as_ref().and_then(|g| {
            g.iter().take(32).find(|(when, k, values)| {
                k == &key
                    && when.elapsed().as_millis() < u128::from(WINDOWS_TTL_MS)
                    && windows(values)
            })
        });
        let projection = Projection {
            dxpeditions: &snap.dxpeditions,
            source: &snap.source,
            as_of: snap.as_of,
            source_age_ms: age,
            windows: found.map(|(_, _, values)| values.as_slice()),
            window_age_ms: found.map(|(when, _, _)| when.elapsed().as_millis() as u64),
            window_valid_for_ms: found.map(|(when, _, _)| {
                // The native key belongs to one UTC day. Never keep yesterday's
                // forecast visible across midnight just because its six hours
                // have not elapsed. The browser consumes this monotonic budget.
                let day_end = ((now / 86_400 + 1) * 86_400_000) as u64;
                WINDOWS_TTL_MS
                    .saturating_sub(when.elapsed().as_millis() as u64)
                    .min(day_end.saturating_sub(super::super::now_ms()))
            }),
        };
        let mut bytes = Bounded(Vec::new());
        serde_json::to_writer(&mut bytes, &projection).map_err(|_| "applicationTooLarge")?;
        bytes.0
    };
    // Keep every projection tied to the same settings/log at the end of copying.
    // No engine lock is held while serializing or looking up cached predictions.
    let eng = engine.try_lock().map_err(|_| "applicationBusy")?;
    let s = eng.settings();
    if crate::unassisted()
        || s.mycall != call
        || s.mygrid != grid
        || !Arc::ptr_eq(&log, &eng.log_read_token())
        || s.prop_engine != model
        || s.station_power_w != power
        || s.ant_tx_gain_dbi + s.ant_rx_gain_dbi != gain
    {
        return Err("applicationUnavailable");
    }
    drop(eng);
    serde_json::from_slice(&bytes).map_err(|_| "applicationUnavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    fn fixture() -> (
        crate::SharedEngine,
        crate::PropCache,
        crate::DxpedWindowsCache,
    ) {
        let settings = tempo_app::settings::Settings {
            mycall: "W1AW".into(),
            mygrid: "FN31".into(),
            ..Default::default()
        };
        let e = tempo_app::engine::Engine::with_settings(settings);
        let context = crate::PropContext {
            call: "W1AW".into(),
            grid: "FN31".into(),
            log: e.log_read_token(),
        };
        let mut snapshot = propagation::offline(crate::now_unix(), "W1AW", "FN31");
        snapshot.source = "live".into();
        snapshot
            .dxpeditions
            .workable_now
            .push(propagation::WorkableCard {
                call: "3Y0TEST".into(),
                entity: "Bouvet Island".into(),
                need: propagation::NeedKind::Atno,
                band: "20m".into(),
                bearing_deg: 145.,
                octant: "SE".into(),
                distance_km: 12500.,
                status: propagation::WorkStatus::WorkNow,
                likelihood: "Good".into(),
                likelihood_score: 0.8,
                live_confirmed: true,
                how_to_call: "Synthetic test advice".into(),
                ft8_mode: None,
                window_hint: "1400–1700Z".into(),
                priority: 100,
                modes: vec!["CW".into()],
            });
        snapshot.dxpeditions.active.push("3Y0TEST".into());
        (
            Arc::new(Mutex::new(e)),
            Arc::new(Mutex::new(Some((Instant::now(), snapshot, context)))),
            Mutex::new(Vec::new()),
        )
    }
    #[test]
    fn cached_board_preserves_live_need_and_mode_without_native_work_or_forecast() {
        let (engine, cache, forecasts) = fixture();
        let before = engine.lock().unwrap().snapshot();
        let value = read_cached(&engine, &cache, &forecasts, None).unwrap();
        assert_eq!(
            value["dxpeditions"],
            serde_json::from_str::<Value>(
                &serde_json::to_string(&cache.lock().unwrap().as_ref().unwrap().1.dxpeditions)
                    .unwrap()
            )
            .unwrap()
        );
        assert_eq!(value["dxpeditions"]["workableNow"][0]["modes"][0], "CW");
        assert!(value["windows"].is_null());
        assert!(value["windowAgeMs"].is_null());
        let e = engine.lock().unwrap();
        assert_eq!(before.radio.tx_enabled, e.snapshot().radio.tx_enabled);
        assert!(e.log_records().is_empty());
        assert!(Arc::ptr_eq(
            &e.log_read_token(),
            &cache.lock().unwrap().as_ref().unwrap().2.log
        ));
        assert_eq!(e.settings().mycall, "W1AW");
        assert_eq!(e.settings().mygrid, "FN31");
        assert!(
            forecasts.lock().unwrap().is_empty(),
            "a read cannot create a predictor result"
        );
    }
    #[test]
    fn cold_expired_busy_and_changed_contexts_refuse_with_recovery() {
        let (engine, cache, forecasts) = fixture();
        let saved = cache.lock().unwrap().take();
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationUnavailable")
        );
        *cache.lock().unwrap() = saved;
        let held = cache.lock().unwrap();
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationBusy")
        );
        drop(held);
        let held = engine.lock().unwrap();
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationBusy")
        );
        drop(held);
        assert!(read_cached(&engine, &cache, &forecasts, None).is_ok());
        cache.lock().unwrap().as_mut().unwrap().0 = Instant::now() - Duration::from_secs(301);
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("queryExpired")
        );
        cache.lock().unwrap().as_mut().unwrap().0 = Instant::now();
        cache.lock().unwrap().as_mut().unwrap().2.grid = "AA00".into();
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationUnavailable")
        );
        cache.lock().unwrap().as_mut().unwrap().2.grid = "FN31".into();
        engine.lock().unwrap().import_adif(
            "<CALL:5>JA1AA<BAND:3>20m<MODE:2>CW<QSO_DATE:8>20260909<TIME_ON:6>120000<EOR>",
        );
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationUnavailable")
        );
    }
    #[test]
    fn oversized_fields_rows_and_encoded_board_refuse_the_whole_capture() {
        let (engine, cache, forecasts) = fixture();
        let card = cache
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .1
            .dxpeditions
            .workable_now[0]
            .clone();
        cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .1
            .dxpeditions
            .workable_now[0]
            .how_to_call = "x".repeat(TEXT + 1);
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationTooLarge")
        );
        cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .1
            .dxpeditions
            .workable_now = vec![card.clone(); MAX_ENTRIES + 1];
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationTooLarge")
        );
        let mut large = card.clone();
        large.how_to_call = "x".repeat(TEXT);
        cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .1
            .dxpeditions
            .workable_now = vec![large; MAX_ENTRIES];
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, None),
            Err("applicationTooLarge")
        );
        cache
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .1
            .dxpeditions
            .workable_now = vec![card];
        assert!(read_cached(&engine, &cache, &forecasts, None).is_ok());
    }
    #[test]
    fn forecasts_require_the_native_key_day_and_ttl_and_never_block_the_board() {
        let (engine, cache, forecasts) = fixture();
        let settings = engine.lock().unwrap().settings().clone();
        let day = crate::now_unix() / 86_400;
        let key = crate::dxped_windows_key(
            day,
            7,
            &settings.mygrid,
            &settings.prop_engine,
            settings.station_power_w,
            settings.ant_tx_gain_dbi + settings.ant_rx_gain_dbi,
            Some(110.4),
            vec!["3Y0TEST"],
        );
        let window = crate::DxpedWindow {
            call: "3Y0TEST".into(),
            engine: settings.prop_engine.clone(),
            best: "20m Good".into(),
            outlook: Vec::new(),
            days: Vec::new(),
            start_unix: None,
            end_unix: None,
        };
        forecasts
            .lock()
            .unwrap()
            .push((Instant::now(), key.clone(), vec![window]));
        assert_eq!(
            read_cached(&engine, &cache, &forecasts, Some(110.49)).unwrap()["windows"][0]["call"],
            "3Y0TEST"
        );
        assert!(read_cached(&engine, &cache, &forecasts, Some(111.)).unwrap()["windows"].is_null());
        forecasts.lock().unwrap()[0].1 = key.replacen(&day.to_string(), &(day - 1).to_string(), 1);
        assert!(
            read_cached(&engine, &cache, &forecasts, Some(110.4)).unwrap()["windows"].is_null()
        );
        forecasts.lock().unwrap()[0].1 = key;
        forecasts.lock().unwrap()[0].0 = Instant::now() - Duration::from_millis(WINDOWS_TTL_MS);
        assert!(
            read_cached(&engine, &cache, &forecasts, Some(110.4)).unwrap()["windows"].is_null()
        );
        let held = forecasts.lock().unwrap();
        assert!(
            read_cached(&engine, &cache, &forecasts, Some(110.4)).unwrap()["windows"].is_null()
        );
        drop(held);
    }
}
