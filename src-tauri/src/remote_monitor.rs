//! Local observer adapter. No network listener and no mutation dispatch.
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempo_app::remote_monitor::{Frame, Source, MAX_SEQUENCE, POLL_MS, VERSION};

pub struct Publisher {
    epoch: String,
    cache: Mutex<Option<(Instant, Frame)>>,
}

impl Default for Publisher {
    fn default() -> Self {
        Self {
            epoch: format!(
                "native-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
            cache: Mutex::new(None),
        }
    }
}

impl Publisher {
    fn read(&self, engine: &super::SharedEngine, now: Instant) -> Result<Frame, &'static str> {
        // Coalesce multiple observer windows; never queue behind capture/CAT work.
        let mut cache = self.cache.try_lock().map_err(|_| "monitorBusy")?;
        if let Some((published, frame)) = cache.as_ref() {
            if now.saturating_duration_since(*published) < Duration::from_millis(POLL_MS) {
                return Ok(frame
                    .clone()
                    .aged(now.saturating_duration_since(*published).as_millis() as u64));
            }
        }
        let sequence = cache.as_ref().map_or(1, |(_, frame)| frame.sequence + 1);
        if sequence > MAX_SEQUENCE {
            return Err("monitorSequenceExhausted");
        }
        let station = {
            let guard = engine.try_lock().map_err(|_| "monitorBusy")?;
            guard.remote_monitor_observation_at(now)
        }; // engine guard drops BEFORE framing and IPC serialization
        let frame = Frame {
            version: VERSION,
            source: Source::Native,
            epoch: self.epoch.clone(),
            sequence,
            generated_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(MAX_SEQUENCE as u128) as u64,
            station,
        };
        *cache = Some((now, frame.clone()));
        Ok(frame)
    }
}

#[tauri::command]
pub fn get_remote_monitor_frame(
    engine: tauri::State<'_, super::SharedEngine>,
    publisher: tauri::State<'_, Publisher>,
) -> Result<Frame, &'static str> {
    publisher.read(&engine, Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempo_app::engine::Engine;

    #[test]
    fn cached_publications_expire_real_hardware_readings_without_renewing_the_sequence() {
        let engine = Arc::new(Mutex::new(Engine::new("N0CALL", "AA00", 0)));
        let publisher = Publisher::default();
        let now = Instant::now();
        {
            let mut eng = engine.lock().unwrap();
            let connection = eng.remote_open_radio().unwrap();
            let read = eng
                .remote_radio_read(&connection, now - Duration::from_millis(4900))
                .unwrap();
            eng.remote_observe_cat(Some(&read), Some(true));
            eng.remote_observe_ptt(Some(&read), Some(false));
        }
        let first = publisher.read(&engine, now).unwrap();
        assert_eq!(first.station.radio.rig_keyed, Some(false));
        assert_eq!(first.station.radio.readings.ptt.unwrap().age_ms, 4900);
        let held = engine.lock().unwrap();
        let expired = publisher
            .read(&engine, now + Duration::from_millis(200))
            .unwrap();
        assert_eq!(expired.sequence, first.sequence);
        assert_eq!(expired.generated_at_ms, first.generated_at_ms);
        assert_eq!(expired.station.radio.rig_keyed, None);
        assert_eq!(expired.station.radio.readings.ptt, None);
        drop(held);
        let next = publisher
            .read(&engine, now + Duration::from_millis(500))
            .unwrap();
        assert_eq!(next.sequence, first.sequence + 1);
        assert_eq!(next.station.radio.rig_keyed, None);
    }

    #[test]
    fn cached_reads_do_not_renew_freshness_and_busy_engine_does_not_queue() {
        let engine = Arc::new(Mutex::new(Engine::new("N0CALL", "AA00", 0)));
        let publisher = Publisher::default();
        let now = Instant::now();
        let before = serde_json::to_value(engine.lock().unwrap().snapshot()).unwrap();
        let first = publisher.read(&engine, now).unwrap();
        let held = engine.lock().unwrap();
        assert_eq!(publisher.read(&engine, now).unwrap(), first);
        assert_eq!(
            publisher.read(&engine, now + Duration::from_secs(1)),
            Err("monitorBusy")
        );
        drop(held);
        let next = publisher
            .read(&engine, now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(next.sequence, first.sequence + 1);
        assert_eq!(next.epoch, first.epoch);
        assert_eq!(
            serde_json::to_value(engine.lock().unwrap().snapshot()).unwrap(),
            before
        );
        assert!(
            serde_json::to_vec(&next).unwrap().len() < tempo_app::remote_monitor::MAX_FRAME_BYTES
        );
    }
}
